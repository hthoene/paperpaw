//! `paperpaw-sign frames-s3 <S3 firmware.elf> --budget <file>`: the build-time
//! half of the ESP32-S3 stack band's bound (`abi/heap-word.md` §6, "The
//! ESP32-S3's band"; todo 206).
//!
//! The band's bound, at most **6 frames** run below it before the first spill
//! reaches it, holds only if (a) every frame on the way down is under `LIMIT`
//! (2,048 bytes) and (b) every call is a `call8` or `call12` (a `call4` lets
//! more windows be live, and the reserve below the band no longer covers the
//! depth). The packer holds a plugin's own code to both
//! ([`crate::frames_xtensa`]); this holds **the firmware's code that a plugin
//! can call into**: the host slots, the dispatch, the loader and whatever
//! they reach.
//!
//! The budget names the **roots**, families of functions by path as
//! `paperpaw-sign frames` names them (`paperpaw_core::native_host`, ...). The
//! set checked is the roots and everything they reach through **direct**
//! calls and jumps, read from the code (`call4`/`call8`/`call12`, `j`). On
//! every function of the set:
//!
//! - its frame (`entry`'s operand times 8, plus the 16 bytes of the base save
//!   area, as the packer counts it) is under the limit;
//! - no `call0`, `callx0`, `call4` or `callx4`;
//! - a change of `a1` other than `entry` (a dynamic `alloca`, `movsp`), or a
//!   function of no size in the symbol table (assembly), cannot be bounded:
//!   refused.
//!
//! What static reading cannot see is **reported and refused unless named**:
//! a call out of the image (the ROM, a blob) and a call into code with no
//! symbol of its own. The budget's `allow` lines name them. **Indirect**
//! calls (`callx8` and `callx12` through a vtable or a function pointer)
//! cannot be followed; they are counted per function and listed, and the
//! families the budget names are the ones the trait implementations a plugin
//! can reach live in. The device test (S4, the overflow test with Wi-Fi
//! traffic) covers the rest.
//!
//! Budget lines (`#` starts a comment):
//!
//! ```text
//! limit <bytes>                       the frame limit (2048 by default)
//! root <family>                       extra roots: functions of the family
//! table <symbol>                      a static (the host, heap or shared
//!                                     table) whose words are function
//!                                     pointers: each is a root
//! allow call <caller> -> <callee> <why>
//!                                     a call out of the image or into code
//!                                     with no symbol: one callee, by symbol
//!                                     or 0xaddress, from one caller
//! allow indirect <caller> <count> <why>
//!                                     exactly this many calls through a
//!                                     register in the caller
//! allow sp <caller> <why>             the caller changes the stack pointer
//!                                     other than by `entry`
//! allow nosize <name> <why>           a function with no size (assembly)
//! ```
//!
//! The roots come from the tables themselves (Kernel's ruling on todo 206):
//! every function pointer in the tables is read from the ELF and resolved to a
//! symbol, so a slot wired later is a root without a line here. Every entry of
//! `allow` is checked as used: one that matches nothing fails, so the list
//! cannot go stale, and a site that is not listed fails, also an existing
//! function that gains one. A callee outside the image is listed by its symbol
//! one by one; a range or a blob is never allowed whole.
//!
//! A root or table that matches nothing fails, so a rename cannot make the
//! check pass by matching nothing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::elf::{EM_XTENSA, Elf, SHF_ALLOC, SHF_EXECINSTR};
use crate::framelist::in_family;
use crate::frames::path;
use crate::frames_xtensa::{Class, entry_frame, length, narrow_class, wide_class};

/// `STT_FUNC` and `STT_OBJECT`.
const STT_FUNC: u8 = 2;
const STT_OBJECT: u8 = 1;

/// One `allow` line.
#[derive(Debug, PartialEq, Eq)]
pub enum Allow {
    /// `caller` may call `callee` (a symbol path or `0x` address).
    Call { caller: String, callee: String },
    /// `caller` has exactly `count` calls through a register.
    Indirect { caller: String, count: u32 },
    /// `caller` changes `a1` other than by `entry`.
    Sp { caller: String },
    /// `name` is assembly with no size.
    NoSize { name: String },
}

/// What the budget asks.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub limit: u32,
    pub roots: Vec<String>,
    pub tables: Vec<String>,
    /// Each `allow` line with its reason.
    pub allow: Vec<(Allow, String)>,
}

impl Budget {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut budget = Budget {
            limit: crate::frames_xtensa::LIMIT,
            ..Budget::default()
        };
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let at = n + 1;
            let words: Vec<&str> = line.split_whitespace().collect();
            let bad = || format!("line {at}: not a line of the S3 budget (see the tool's help)");
            match words.as_slice() {
                ["limit", bytes] => {
                    budget.limit = bytes
                        .parse()
                        .map_err(|_| format!("line {at}: `limit <bytes>`"))?;
                }
                ["root", family] => budget.roots.push((*family).to_string()),
                ["table", symbol] => budget.tables.push((*symbol).to_string()),
                ["allow", "call", caller, "->", callee, why @ ..] if !why.is_empty() => {
                    budget.allow.push((
                        Allow::Call {
                            caller: (*caller).to_string(),
                            callee: (*callee).to_string(),
                        },
                        why.join(" "),
                    ))
                }
                ["allow", "indirect", caller, count, why @ ..] if !why.is_empty() => {
                    budget.allow.push((
                        Allow::Indirect {
                            caller: (*caller).to_string(),
                            count: count.parse().map_err(|_| bad())?,
                        },
                        why.join(" "),
                    ))
                }
                ["allow", "sp", caller, why @ ..] if !why.is_empty() => budget.allow.push((
                    Allow::Sp {
                        caller: (*caller).to_string(),
                    },
                    why.join(" "),
                )),
                ["allow", "nosize", name, why @ ..] if !why.is_empty() => budget.allow.push((
                    Allow::NoSize {
                        name: (*name).to_string(),
                    },
                    why.join(" "),
                )),
                _ => return Err(bad()),
            }
        }
        if budget.roots.is_empty() && budget.tables.is_empty() {
            return Err("the budget names no root and no table".into());
        }
        Ok(budget)
    }
}

/// A function of the firmware.
#[derive(Debug)]
struct Function {
    path: String,
    start: u32,
    /// 0: the symbol table gives it no size.
    size: u32,
    /// The executable section it is in, and where in its bytes.
    code: (usize, usize),
}

/// What the code of a function does that matters here.
#[derive(Debug, Default)]
struct Reads {
    /// The largest frame an `entry` takes (with the base save area), if any.
    frame: Option<u32>,
    /// Direct calls and jumps out of the function: target addresses.
    edges: Vec<u32>,
    /// A `call0`, `callx0`, `call4` or `callx4`, at this address.
    narrow_call: Option<(u32, &'static str)>,
    /// Calls through a register (`callx8`, `callx12`).
    indirect: u32,
    /// An instruction that writes `a1`, not `entry`.
    writes_sp: Option<u32>,
    /// An instruction this scan does not know how long it is, or a form it
    /// does not know.
    unreadable: Option<u32>,
}

fn sext18(w: u32) -> i64 {
    let v = i64::from((w >> 6) & 0x3_FFFF);
    if v & 0x2_0000 != 0 { v - 0x4_0000 } else { v }
}

/// `t` (bits 7:4) if `w` is a `callx0`, `callx4`, `callx8` or `callx12`: RST0,
/// `st0`, `snm0` with `r` = 0 and `t` = 12 to 15 (`s` is the register).
fn callx_t(w: u32) -> Option<u32> {
    let t = (w >> 4) & 0xF;
    let rst0_snm0 = w & 0xF == 0 && (w >> 12) & 0xF == 0 && (w >> 16) & 0xFF == 0;
    (rst0_snm0 && t >= 0xC).then_some(t)
}

fn read_function(code: &[u8], start_addr: u32) -> Reads {
    let mut reads = Reads::default();
    let mut at = 0usize;
    while at < code.len() {
        let pc = start_addr.wrapping_add(at as u32);
        let Some(len) = length(code[at]) else {
            reads.unreadable.get_or_insert(pc);
            break;
        };
        if at + len > code.len() {
            reads.unreadable.get_or_insert(pc);
            break;
        }
        if len == 3 {
            let w =
                u32::from(code[at]) | u32::from(code[at + 1]) << 8 | u32::from(code[at + 2]) << 16;
            if let Some(size) = entry_frame(w) {
                reads.frame = Some(reads.frame.map_or(size, |f| f.max(size)));
            } else if w & 0xF == 5 {
                // CALLn: n in bits 5:4, the target a word offset from the next word.
                let target = ((i64::from(pc >> 2) + 1 + sext18(w)) << 2) as u32;
                match (w >> 4) & 3 {
                    0 => reads.narrow_call = reads.narrow_call.or(Some((pc, "call0"))),
                    1 => reads.narrow_call = reads.narrow_call.or(Some((pc, "call4"))),
                    _ => reads.edges.push(target),
                }
            } else if w & 0x3F == 0x06 {
                // J: the target is from the instruction's own address.
                let target = (i64::from(pc) + 4 + sext18(w)) as u32;
                if target < start_addr || target >= start_addr.wrapping_add(code.len() as u32) {
                    reads.edges.push(target);
                }
            } else if let Some(t) = callx_t(w) {
                match t {
                    0xC => reads.narrow_call = reads.narrow_call.or(Some((pc, "callx0"))),
                    0xD => reads.narrow_call = reads.narrow_call.or(Some((pc, "callx4"))),
                    // CALLX8 and CALLX12: through a register, not followed.
                    _ => reads.indirect += 1,
                }
            } else {
                match wide_class(w) {
                    Class::Safe => {}
                    Class::Writes => {
                        reads.writes_sp.get_or_insert(pc);
                    }
                    Class::Unknown => {}
                }
            }
        } else {
            let w = u32::from(code[at]) | u32::from(code[at + 1]) << 8;
            if narrow_class(w) == Class::Writes {
                reads.writes_sp.get_or_insert(pc);
            }
        }
        at += len;
    }
    reads
}

/// The firmware's functions by address, its executable sections (address and
/// bytes), and its data sections (alloc, not executable) for the tables.
struct Image<'a> {
    funcs: Vec<Function>,
    exec: Vec<(u32, &'a [u8])>,
    data: Vec<(u32, &'a [u8])>,
    /// Every named address (any symbol that has a value), for naming a callee
    /// out of the image (the ROM's symbols come from the linker script).
    names: BTreeMap<u32, String>,
    /// The symbols by path with their address and size, for the tables.
    objects: Vec<(String, u32, u32)>,
}

fn image<'a>(elf: &'a Elf<'a>) -> Result<Image<'a>, String> {
    let mut exec: Vec<(u32, &[u8])> = Vec::new();
    let mut data: Vec<(u32, &[u8])> = Vec::new();
    let mut index_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, s) in elf.sections.iter().enumerate() {
        if s.flags & SHF_ALLOC == 0 || s.kind != 1 {
            continue;
        }
        if s.flags & SHF_EXECINSTR != 0 {
            index_of.insert(i, exec.len());
            exec.push((s.addr, elf.data(s)?));
        } else {
            data.push((s.addr, elf.data(s)?));
        }
    }
    let (_, symbols) = elf
        .symbols()?
        .ok_or("no symbol table: do not strip the firmware")?;
    let mut found: BTreeMap<u32, Function> = BTreeMap::new();
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    let mut objects = Vec::new();
    for s in &symbols {
        if s.name.is_empty() || s.value == 0 {
            continue;
        }
        names.entry(s.value).or_insert_with(|| path(&s.name));
        if s.kind() == STT_OBJECT {
            objects.push((path(&s.name), s.value, s.size));
        }
        if s.kind() != STT_FUNC {
            continue;
        }
        let Some(&k) = index_of.get(&(s.shndx as usize)) else {
            continue;
        };
        let from = s.value.wrapping_sub(exec[k].0) as usize;
        if from > exec[k].1.len() {
            continue;
        }
        let function = Function {
            path: path(&s.name),
            start: s.value,
            size: s.size,
            code: (k, from),
        };
        // Aliases are one function, with the larger size and the first name.
        match found.get_mut(&s.value) {
            Some(known) if known.size >= s.size => {}
            _ => {
                found.insert(s.value, function);
            }
        }
    }
    Ok(Image {
        funcs: found.into_values().collect(),
        exec,
        data,
        names,
        objects,
    })
}

/// The words of the static `symbol` that are the start of a function.
fn table_roots(image: &Image<'_>, symbol: &str) -> Option<Vec<usize>> {
    let mut roots = Vec::new();
    let mut matched = false;
    for (name, address, size) in &image.objects {
        if !(name == symbol || name.ends_with(&format!("::{symbol}"))) {
            continue;
        }
        let Some((base, bytes)) = image
            .data
            .iter()
            .find(|(b, d)| *address >= *b && (*address - b) < d.len() as u32)
        else {
            continue;
        };
        matched = true;
        let from = (*address - base) as usize;
        let to = (from + *size as usize).min(bytes.len());
        let (words, _) = bytes[from..to].as_chunks::<4>();
        for word in words {
            let w = u32::from_le_bytes(*word);
            if let Ok(n) = image.funcs.binary_search_by_key(&w, |f| f.start) {
                roots.push(n);
            }
        }
    }
    matched.then_some(roots)
}

/// The result of reading a firmware.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Functions in the set checked.
    pub checked: usize,
    /// The roots, by where they came from: (extra families, table words).
    pub roots: (usize, usize),
    /// The largest frame in the set and where.
    pub largest: Option<(u32, String)>,
    /// Calls through a register in the set (not followed).
    pub indirect: u32,
    /// What the budget refuses, one line each.
    pub problems: Vec<String>,
}

impl Image<'_> {
    /// The function holding `address`, as an index into `funcs`.
    fn at(&self, address: u32) -> Option<usize> {
        let n = self.funcs.partition_point(|f| f.start <= address);
        let f = self.funcs.get(n.checked_sub(1)?)?;
        (address < f.start.saturating_add(f.size.max(1))).then_some(n - 1)
    }

    /// Whether `address` is inside an executable section of the firmware.
    fn in_image(&self, address: u32) -> bool {
        self.exec
            .iter()
            .any(|(base, data)| address >= *base && (address - base) < data.len() as u32)
    }
}

/// Follows the calls of `caller` that are direct: into a function of the
/// firmware (queued), or out of it (refused unless an `allow call` names it).
fn follow(
    image: &Image<'_>,
    budget: &Budget,
    caller: &str,
    edges: &[u32],
    used: &mut [bool],
    report: &mut Report,
    (queue, seen): (&mut VecDeque<usize>, &mut BTreeSet<usize>),
) {
    for &target in edges {
        if let Some(n) = image.at(target) {
            if seen.insert(n) {
                queue.push_back(n);
            }
            continue;
        }
        let callee = image
            .names
            .get(&target)
            .cloned()
            .unwrap_or_else(|| format!("{target:#x}"));
        let by_address = format!("{target:#x}");
        if !allowed(budget, used, &|a| {
            matches!(a, Allow::Call { caller: c, callee: e }
                if c == caller && (*e == callee || *e == by_address))
        }) {
            let where_ = if image.in_image(target) {
                "code of the image with no symbol"
            } else {
                "outside the image (ROM or a blob)"
            };
            report.problems.push(format!(
                "`{caller}` calls `{callee}` ({target:#x}), {where_}: not checkable here (name it with `allow call`)"
            ));
        }
    }
}

/// Puts the budget's roots and the words of its tables in `queue`.
fn seed(
    image: &Image<'_>,
    budget: &Budget,
    report: &mut Report,
    queue: &mut VecDeque<usize>,
    seen: &mut BTreeSet<usize>,
) {
    for root in &budget.roots {
        let members: Vec<usize> = (0..image.funcs.len())
            .filter(|&i| in_family(&image.funcs[i].path, root))
            .collect();
        if members.is_empty() {
            report
                .problems
                .push(format!("the root `{root}` matches no function"));
        }
        for i in members {
            if seen.insert(i) {
                report.roots.0 += 1;
                queue.push_back(i);
            }
        }
    }
    for table in &budget.tables {
        match table_roots(image, table) {
            None => report.problems.push(format!(
                "the table `{table}` matches no static in the firmware"
            )),
            Some(words) if words.is_empty() => report
                .problems
                .push(format!("the table `{table}` holds no function pointer")),
            Some(words) => {
                for i in words {
                    if seen.insert(i) {
                        report.roots.1 += 1;
                        queue.push_back(i);
                    }
                }
            }
        }
    }
}

/// Marks every `allow` line `wanted` selects as used; whether any did.
fn allowed(budget: &Budget, used: &mut [bool], wanted: &dyn Fn(&Allow) -> bool) -> bool {
    let mut any = false;
    for (n, (allow, _)) in budget.allow.iter().enumerate() {
        if wanted(allow) {
            used[n] = true;
            any = true;
        }
    }
    any
}

/// Reads `elf` and checks the set the budget's roots and tables reach.
pub fn check(elf: &Elf<'_>, budget: &Budget) -> Result<Report, String> {
    if elf.machine != EM_XTENSA {
        return Err("not an Xtensa firmware (the S3's)".into());
    }
    let image = image(elf)?;
    let funcs = &image.funcs;
    let mut report = Report::default();
    // Which `allow` lines were used.
    let mut used = vec![false; budget.allow.len()];
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    seed(&image, budget, &mut report, &mut queue, &mut seen);
    while let Some(i) = queue.pop_front() {
        let f = &funcs[i];
        report.checked += 1;
        if f.size == 0 {
            let name = f.path.clone();
            if !allowed(
                budget,
                &mut used,
                &|a| matches!(a, Allow::NoSize { name: n } if *n == name),
            ) {
                report.problems.push(format!(
                    "`{}` ({:#x}) has no size in the symbol table: assembly this scan does not read",
                    f.path, f.start
                ));
            }
            continue;
        }
        let (k, from) = f.code;
        let data = image.exec[k].1;
        let to = (from + f.size as usize).min(data.len());
        let reads = read_function(&data[from..to], f.start);
        if let Some(frame) = reads.frame {
            if report.largest.as_ref().is_none_or(|(b, _)| frame > *b) {
                report.largest = Some((frame, f.path.clone()));
            }
            if frame >= budget.limit {
                report.problems.push(format!(
                    "`{}` takes a frame of {frame} bytes (limit {})",
                    f.path, budget.limit
                ));
            }
        }
        let caller = f.path.clone();
        if let Some(pc) = reads.writes_sp
            && !allowed(
                budget,
                &mut used,
                &|a| matches!(a, Allow::Sp { caller: c } if *c == caller),
            )
        {
            report.problems.push(format!(
                "`{}` changes the stack pointer other than by `entry` ({pc:#x}): its frame cannot be bounded",
                f.path
            ));
        }
        if let Some((pc, what)) = reads.narrow_call {
            report.problems.push(format!(
                "`{}` calls with {what} ({pc:#x}): only call8 and call12 keep the band's bound",
                f.path
            ));
        }
        if let Some(pc) = reads.unreadable {
            report.problems.push(format!(
                "`{}` has code this scan cannot read ({pc:#x})",
                f.path
            ));
        }
        report.indirect += reads.indirect;
        if reads.indirect > 0 {
            let count = reads.indirect;
            if !allowed(
                budget,
                &mut used,
                &|a| matches!(a, Allow::Indirect { caller: c, count: n } if *c == caller && *n == count),
            ) {
                report.problems.push(format!(
                    "`{}` has {count} call(s) through a register: not followed here (name them with `allow indirect`)",
                    f.path
                ));
            }
        }
        follow(
            &image,
            budget,
            &caller,
            &reads.edges,
            &mut used,
            &mut report,
            (&mut queue, &mut seen),
        );
    }
    for (n, (allow, _)) in budget.allow.iter().enumerate() {
        if !used[n] {
            report.problems.push(format!(
                "an `allow` line matches nothing any more: {allow:?}: remove it"
            ));
        }
    }
    Ok(report)
}

pub fn run(elf: &str, budget: &str) -> Result<(), String> {
    let bytes = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let parsed = Elf::parse(&bytes).map_err(|e| format!("{elf}: {e}"))?;
    let text = std::fs::read_to_string(budget).map_err(|e| format!("{budget}: {e}"))?;
    let budget = Budget::parse(&text).map_err(|e| format!("{budget}: {e}"))?;
    let report = check(&parsed, &budget).map_err(|e| format!("{elf}: {e}"))?;
    println!(
        "{} functions checked from {} named roots and {} table entries; largest frame {}; {} indirect calls (not followed, each listed in the budget)",
        report.checked,
        report.roots.0,
        report.roots.1,
        report
            .largest
            .as_ref()
            .map_or_else(|| "none".to_string(), |(b, p)| format!("{b} ({p})")),
        report.indirect
    );
    if report.problems.is_empty() {
        println!("the S3 firmware's frames and calls hold");
        return Ok(());
    }
    Err(format!(
        "the S3 firmware fails the band's bound:\n  {}",
        report.problems.join("\n  ")
    ))
}

#[cfg(test)]
mod tests;
