//! `paperpaw-sign frames <elf> [--limit N] [--budget FILE]`: the stack frame
//! of every function in a linked firmware, from the code (the scan of
//! [`crate::frames`]), and a check of the ones on the panic and dispatch
//! paths against a budget.
//!
//! The X4 keeps 2 KiB below the stack's end band for its panic handler
//! (`paperpaw_x4::stack_band::RESERVE`), and the deepest legitimate path must
//! stay above the band. The device measures both; this is the build-time half
//! for the functions that matter: a budget file names them and their largest
//! frame, so a change that makes the handler's frames larger fails the build.
//!
//! The budget is lines of `frame <bytes> <name>` (the largest frame of any
//! function whose path holds `<name>`) and `chain <bytes> <name> ...` (the
//! sum of the largest frame of each name stays within `<bytes>`: what one call
//! down the chain takes at most). A name that matches no function fails too,
//! so a rename cannot make a check pass by matching nothing. A function whose
//! frame cannot be read (a dynamic `alloca`) counts as unbounded. `#` starts a
//! comment.

use std::fmt::Write as _;

use crate::elf::{Elf, SHF_EXECINSTR, Symbol};
use crate::frames::frame_sizes;

/// `STT_FUNC`.
const STT_FUNC: u8 = 2;

/// A function and its frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub address: u32,
    /// Bytes of the frame; `None` when it cannot be read.
    pub bytes: Option<u32>,
    /// The function's path, as far as its mangled name shows it.
    pub path: String,
}

/// The names in a mangled symbol (legacy `_ZN…E` or v0 `_R…`) joined with
/// `::`: the length-prefixed identifiers, without the crate's disambiguator
/// and the hash. Not a demangler: enough to match `crash::panic`.
pub fn path(mangled: &str) -> String {
    let b = mangled.as_bytes();
    let v0 = mangled.starts_with("_R");
    let mut i = if v0 {
        2
    } else if mangled.starts_with("_ZN") {
        3
    } else {
        return mangled.to_string();
    };
    let mut parts: Vec<&str> = Vec::new();
    while i < b.len() {
        // A crate's disambiguator, `s<base62>_`, after its `C`.
        if b[i] == b'C' && b.get(i + 1) == Some(&b's') {
            i += 2;
            while i < b.len() && b[i] != b'_' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let n: usize = mangled[start..i].parse().unwrap_or(0);
            // In v0 a name that starts with an underscore or a digit follows
            // the number after an underscore of its own.
            if v0 && n > 0 && b.get(i) == Some(&b'_') {
                i += 1;
            }
            let ident = mangled
                .get(i..i + n)
                .filter(|id| n > 0 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'));
            if let Some(ident) = ident {
                // The legacy hash, `h` and sixteen hex digits.
                let hash = ident.len() == 17
                    && ident.starts_with('h')
                    && ident[1..].bytes().all(|c| c.is_ascii_hexdigit());
                if !hash {
                    parts.push(ident);
                }
                i += n;
            }
            continue;
        }
        i += 1;
    }
    parts.join("::")
}

/// Every function of the executable sections of `elf`, with its frame.
pub fn frames(elf: &Elf<'_>) -> Result<Vec<Frame>, String> {
    let (_, symbols) = elf
        .symbols()?
        .ok_or("no symbol table: the firmware is stripped")?;
    let mut out = Vec::new();
    for (index, section) in elf.sections.iter().enumerate() {
        if section.flags & SHF_EXECINSTR == 0 || section.size == 0 || section.kind == 8 {
            continue;
        }
        let code = elf.data(section)?;
        let mut funcs: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind() == STT_FUNC && s.shndx as usize == index && s.size > 0)
            .collect();
        funcs.sort_by_key(|s| s.value);
        funcs.dedup_by_key(|s| s.value);
        let starts: Vec<usize> = funcs
            .iter()
            .map(|s| s.value.wrapping_sub(section.addr) as usize)
            .filter(|&a| a < code.len())
            .collect();
        let sizes = frame_sizes(code, &starts);
        for (&at, bytes) in starts.iter().zip(sizes) {
            let symbol = funcs
                .iter()
                .find(|s| s.value.wrapping_sub(section.addr) as usize == at);
            let Some(symbol) = symbol else { continue };
            out.push(Frame {
                address: symbol.value,
                bytes,
                path: path(&symbol.name),
            });
        }
    }
    Ok(out)
}

/// What a budget line checks.
#[derive(Debug, PartialEq, Eq)]
enum Rule {
    Frame { max: u32, name: String },
    Chain { max: u32, names: Vec<String> },
}

fn parse(budget: &str) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::new();
    for (n, line) in budget.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = n + 1;
        let mut words = line.split_whitespace();
        let kind = words.next().unwrap_or("");
        let max: u32 = words
            .next()
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| format!("line {at}: expected a number of bytes after `{kind}`"))?;
        let names: Vec<String> = words.map(str::to_string).collect();
        match (kind, names.len()) {
            ("frame", 1) => rules.push(Rule::Frame {
                max,
                name: names[0].clone(),
            }),
            ("chain", 1..) => rules.push(Rule::Chain { max, names }),
            _ => {
                return Err(format!(
                    "line {at}: `frame <bytes> <name>` or `chain <bytes> <name>...`"
                ));
            }
        }
    }
    Ok(rules)
}

/// The largest frame of the functions whose path holds `name`: `Err` if
/// none does, `Ok(None)` if one of them has a frame that cannot be read.
fn largest(all: &[Frame], name: &str) -> Result<Option<u32>, String> {
    let mut found = false;
    let mut max = 0u32;
    for f in all.iter().filter(|f| f.path.contains(name)) {
        found = true;
        match f.bytes {
            None => return Ok(None),
            Some(b) => max = max.max(b),
        }
    }
    if found {
        Ok(Some(max))
    } else {
        Err(format!(
            "no function matches `{name}`: renamed? (update the budget)"
        ))
    }
}

/// The problems `budget` finds in `all`: one line each.
pub fn check(all: &[Frame], budget: &str) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    for rule in parse(budget)? {
        match rule {
            Rule::Frame { max, name } => match largest(all, &name) {
                Err(e) => problems.push(e),
                Ok(None) => problems.push(format!("`{name}` has a frame the code does not size")),
                Ok(Some(b)) if b > max => {
                    problems.push(format!("`{name}`: a frame of {b} bytes, over its {max}"))
                }
                Ok(Some(_)) => {}
            },
            Rule::Chain { max, names } => {
                let mut sum = 0u32;
                let mut bad = false;
                for name in &names {
                    match largest(all, name) {
                        Err(e) => {
                            problems.push(e);
                            bad = true;
                        }
                        Ok(None) => {
                            problems.push(format!("`{name}` has a frame the code does not size"));
                            bad = true;
                        }
                        Ok(Some(b)) => sum += b,
                    }
                }
                if !bad && sum > max {
                    problems.push(format!(
                        "chain {}: frames add up to {sum} bytes, over its {max}",
                        names.join(" > ")
                    ));
                }
            }
        }
    }
    Ok(problems)
}

/// The command: lists the frames of `limit` bytes or more, largest first,
/// and with a budget fails if it finds a problem.
pub fn run(elf: &str, limit: u32, budget: Option<&str>) -> Result<(), String> {
    let bytes = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let parsed = Elf::parse(&bytes).map_err(|e| format!("{elf}: {e}"))?;
    let mut all = frames(&parsed).map_err(|e| format!("{elf}: {e}"))?;
    all.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.address.cmp(&b.address)));
    let mut out = String::new();
    for f in all.iter().filter(|f| f.bytes.is_none_or(|b| b >= limit)) {
        let size = f.bytes.map_or("unknown".to_string(), |b| b.to_string());
        let _ = writeln!(out, "{size:>8}  {:08x}  {}", f.address, f.path);
    }
    print!("{out}");
    println!(
        "{} functions, {} listed (limit {limit})",
        all.len(),
        out.lines().count()
    );
    if let Some(file) = budget {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        let problems = check(&all, &text).map_err(|e| format!("{file}: {e}"))?;
        if !problems.is_empty() {
            return Err(format!(
                "the frame budget {file} fails:\n  {}",
                problems.join("\n  ")
            ));
        }
        println!("the frame budget {file} holds");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
