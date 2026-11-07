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
//! function whose path is `<name>` or ends in `::<name>`), `each <bytes> <family> [-<family>...]` (every function in the
//! family of `<family>`, a function and what is inside it or a module's, stays within `<bytes>`, but the
//! functions of the families after `-`) and `chain <bytes> <name> ...` (a word `<name>*<count>` is the sum of the largest `count` frames in the family of `<name>`: the function and what is compiled inside it, e.g. its closures) (the
//! sum of the largest frame of each name stays within `<bytes>`: what one call
//! down the chain takes at most). A name that matches no function fails too,
//! so a rename cannot make a check pass by matching nothing. A function whose
//! frame cannot be read counts as unbounded, and fails the budget: a dynamic
//! `alloca`, and a jump table that cannot be read. The firmware's own code
//! has jump tables (`match` on many arms): the listing reads each from the
//! firmware's read-only data (as the pack of a plugin does, but the code is
//! linked in place, so every word of the table is an address and none is
//! relocated) and walks every case from the jump with the frame it has
//! there. Where a function jumps through a register inside its frame and the
//! table cannot be read (none of its words points into the function, or one
//! points into the middle of an instruction), the walk reads only the cases
//! it can see from the jump, so its frame is a lower bound: the listing
//! shows it as `?>=<bytes>` and the budget refuses a budgeted function that
//! has one. A function of no size in the symbol table (the Wi-Fi blob's
//! assembly) is `unknown` and is not read at all. `#` starts a comment.

use std::fmt::Write as _;

use crate::elf::{Elf, SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE, Symbol};
use crate::frames::{Env, Tables, function_ranges, noreturn_functions, path, sizes_in};

/// The most the read-only data of a firmware may span (its first section to
/// its last, in address): the listing reads it as one run of bytes.
const RODATA_SPAN_MAX: u32 = 64 << 20;

/// The firmware's read-only data (its alloc sections that are neither
/// executable nor writable) as one run of bytes from the first section's
/// address, a gap between sections zero: the compiler's jump tables lie
/// there. Returns the address of the first byte and the bytes.
fn rodata(elf: &Elf<'_>) -> Result<(u32, Vec<u8>), String> {
    let parts: Vec<_> = elf
        .sections
        .iter()
        .filter(|s| {
            s.kind == 1 // SHT_PROGBITS
                && s.flags & SHF_ALLOC != 0
                && s.flags & (SHF_EXECINSTR | SHF_WRITE) == 0
                && s.size > 0
        })
        .collect();
    let Some(first) = parts.iter().map(|s| s.addr).min() else {
        return Ok((0, Vec::new()));
    };
    let mut end = first;
    for s in &parts {
        end = end.max(
            s.addr
                .checked_add(s.size)
                .ok_or_else(|| format!("section {} wraps the address space", s.name))?,
        );
    }
    if end - first > RODATA_SPAN_MAX {
        return Err(format!(
            "the read-only data spans {} bytes: more than the listing reads",
            end - first
        ));
    }
    let mut bytes = vec![0u8; (end - first) as usize];
    for s in parts {
        let at = (s.addr - first) as usize;
        let data = elf.data(s)?;
        bytes[at..at + data.len()].copy_from_slice(data);
    }
    Ok((first, bytes))
}

/// `STT_FUNC`.
const STT_FUNC: u8 = 2;

/// A function and its frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub address: u32,
    /// Bytes of the frame; `None` when it cannot be read.
    pub bytes: Option<u32>,
    /// What the walk saw at least, whether or not `bytes` is known: for a
    /// function with a jump table the listing's number is a lower bound.
    pub at_least: u32,
    /// The function's path, as far as its mangled name shows it.
    pub path: String,
}

/// Every function of the executable sections of `elf`, with its frame. A
/// symbol of no size has none to read: its frame is unknown, and so is an
/// alias's of the same start that has none either way; an alias of a sized
/// function has that function's frame.
pub fn frames(elf: &Elf<'_>) -> Result<Vec<Frame>, String> {
    let (_, symbols) = elf
        .symbols()?
        .ok_or("no symbol table: the firmware is stripped")?;
    let (rodata_at, rodata_bytes) = rodata(elf)?;
    let tables = Tables {
        addr: rodata_at,
        bytes: &rodata_bytes,
        relocated: None,
    };
    let mut out = Vec::new();
    for (index, section) in elf.sections.iter().enumerate() {
        if section.flags & SHF_EXECINSTR == 0 || section.size == 0 || section.kind == 8 {
            continue;
        }
        let code = elf.data(section)?;
        let funcs: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind() == STT_FUNC && s.shndx as usize == index)
            .collect();
        // Each function within its symbol's size and no further than the next
        // function's start, as the pack-time scan reads it.
        let sized: Vec<(usize, usize)> = funcs
            .iter()
            .filter(|s| s.size > 0)
            .map(|s| (s.value.wrapping_sub(section.addr) as usize, s.size as usize))
            .collect();
        let ranges = function_ranges(&sized, code.len());
        let noreturn = noreturn_functions(code, &ranges, section.addr, tables, true);
        let env = Env {
            base: section.addr,
            noreturn: &noreturn,
            tables,
            lenient: true,
        };
        let sizes = sizes_in(code, &ranges, env);
        for symbol in funcs {
            let at = symbol.value.wrapping_sub(section.addr) as usize;
            let sized = (symbol.size > 0)
                .then(|| ranges.binary_search_by_key(&at, |&(s, _)| s).ok())
                .flatten()
                .map(|r| sizes[r]);
            out.push(Frame {
                address: symbol.value,
                bytes: sized.and_then(|s| s.bytes),
                at_least: sized.map_or(0, |s| s.at_least),
                path: path(&symbol.name),
            });
        }
    }
    Ok(out)
}

/// What a budget line checks.
#[derive(Debug, PartialEq, Eq)]
enum Rule {
    Frame {
        max: u32,
        name: String,
    },
    Each {
        max: u32,
        name: String,
        /// Families left out (`-<family>` words).
        except: Vec<String>,
    },
    Chain {
        max: u32,
        names: Vec<String>,
    },
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
            ("each", 1..) => {
                let (except, include): (Vec<&String>, Vec<&String>) =
                    names.iter().partition(|n| n.starts_with('-'));
                if include.len() != 1 || except.iter().any(|n| n.len() == 1) {
                    return Err(format!("line {at}: `each <bytes> <family> [-<family>...]`"));
                }
                rules.push(Rule::Each {
                    max,
                    name: include[0].clone(),
                    except: except.iter().map(|n| n[1..].to_string()).collect(),
                });
            }
            ("chain", 1..) => {
                if let Some(bad) = names
                    .iter()
                    .find(|n| family(n).is_some_and(|f| f.is_none()))
                {
                    return Err(format!("line {at}: `{bad}` is not `<name>*<count>`"));
                }
                rules.push(Rule::Chain { max, names });
            }
            _ => {
                return Err(format!(
                    "line {at}: `frame <bytes> <name>`, `each <bytes> <family>` or `chain <bytes> <name>...`"
                ));
            }
        }
    }
    Ok(rules)
}

/// A chain's family word, `<name>*<count>`: the sum of the `count` largest
/// frames among the functions of the family of `name` (a function and what is
/// compiled inside it: its closures, whose names change from one build to the
/// next). `None` if `word` has no `*`, `Some(None)` if it is malformed (no name,
/// or a count that is not a number of at least 1).
fn family(word: &str) -> Option<Option<(&str, usize)>> {
    let (name, count) = word.split_once('*')?;
    Some(
        count
            .parse::<usize>()
            .ok()
            .filter(|&n| n >= 1 && !name.is_empty())
            .map(|n| (name, n)),
    )
}

/// Whether `path` is in the family of `name`: it is `name` (or ends in
/// `::name`, see [`names`]) or lies inside it, `name::...` anywhere in the path.
fn in_family(path: &str, name: &str) -> bool {
    names(path, name)
        || path.starts_with(&format!("{name}::"))
        || path.contains(&format!("::{name}::"))
}

/// The sum of the `count` largest frames in the family of `name`: `Err` if
/// none is in it, `Ok(None)` if one has a frame that cannot be read (an unsized
/// member of the family fails the chain); a sum that overflows `u32` is
/// `u32::MAX`, over any budget.
fn family_sum(all: &[Frame], name: &str, count: usize) -> Result<Option<u32>, String> {
    let mut sizes = Vec::new();
    for f in all.iter().filter(|f| in_family(&f.path, name)) {
        match f.bytes {
            None => return Ok(None),
            Some(b) => sizes.push(b),
        }
    }
    if sizes.is_empty() {
        return Err(format!(
            "no function matches `{name}`: renamed? (update the budget)"
        ));
    }
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    Ok(Some(
        sizes
            .iter()
            .take(count)
            .try_fold(0u32, |sum, &b| sum.checked_add(b))
            .unwrap_or(u32::MAX),
    ))
}

/// Whether a function's `path` is `name`, or ends in `::name`: the whole
/// path, not a part of a segment (`handler` is not `handler_helper`).
fn names(path: &str, name: &str) -> bool {
    path == name
        || path
            .strip_suffix(name)
            .is_some_and(|before| before.ends_with("::"))
}

/// The largest frame of the functions whose path is `name` (see [`names`]):
/// `Err` if none is, `Ok(None)` if one of them has a frame that cannot be
/// read.
fn largest(all: &[Frame], name: &str) -> Result<Option<u32>, String> {
    let mut found = false;
    let mut max = 0u32;
    for f in all.iter().filter(|f| names(&f.path, name)) {
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
            Rule::Each { max, name, except } => {
                let members: Vec<&Frame> = all
                    .iter()
                    .filter(|f| in_family(&f.path, &name))
                    .filter(|f| !except.iter().any(|e| in_family(&f.path, e)))
                    .collect();
                if members.is_empty() {
                    problems.push(format!(
                        "no function matches `{name}`: renamed? (update the budget)"
                    ));
                }
                for f in &members {
                    match f.bytes {
                        None => problems.push(format!(
                            "`{}` (of `{name}`) has a frame the code does not size",
                            f.path
                        )),
                        Some(b) if b > max => problems.push(format!(
                            "`{}` (of `{name}`): a frame of {b} bytes, over its {max}",
                            f.path
                        )),
                        Some(_) => {}
                    }
                }
            }
            Rule::Chain { max, names } => {
                let mut sum = Some(0u32);
                let mut bad = false;
                for name in &names {
                    let measured = match family(name) {
                        Some(Some((family, count))) => family_sum(all, family, count),
                        _ => largest(all, name),
                    };
                    match measured {
                        Err(e) => {
                            problems.push(e);
                            bad = true;
                        }
                        Ok(None) => {
                            problems.push(format!("`{name}` has a frame the code does not size"));
                            bad = true;
                        }
                        // `u32::MAX` is a family sum that overflowed (no frame is that big).
                        Ok(Some(u32::MAX)) => sum = None,
                        Ok(Some(b)) => sum = sum.and_then(|s| s.checked_add(b)),
                    }
                }
                if !bad {
                    // A sum beyond u32 is over any budget.
                    match sum {
                        Some(sum) if sum <= max => {}
                        Some(sum) => problems.push(format!(
                            "chain {}: frames add up to {sum} bytes, over its {max}",
                            names.join(" > ")
                        )),
                        None => problems.push(format!(
                            "chain {}: frames add up to more than {} bytes, over its {max}",
                            names.join(" > "),
                            u32::MAX
                        )),
                    }
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
        let size = f.bytes.map_or_else(
            || {
                if f.at_least > 0 {
                    format!("?>={}", f.at_least)
                } else {
                    "unknown".to_string()
                }
            },
            |b| b.to_string(),
        );
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
