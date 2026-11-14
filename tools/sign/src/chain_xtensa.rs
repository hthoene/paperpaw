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
//! symbol of its own. The budget's `allow` lines name them. **Calls through
//! a register**: the S3's code makes almost every call as `l32r aN, literal`
//! then `callx8 aN` (a long call), so a `callx8`/`callx12` whose register an
//! `l32r` loaded is followed as the direct call it is (see [`regs`]: the walk
//! goes back through straight-line code only, and gives up at a place another
//! path enters, a rewrite of the register, a call or a form it does not
//! name). The rest (a vtable, a function pointer) cannot be followed; they
//! are counted per function and listed, and the families the budget names are
//! the ones the trait implementations a plugin can reach live in. The device test (S4, the overflow test with Wi-Fi
//! traffic) covers the rest.
//!
//! Budget lines (`#` starts a comment):
//!
//! ```text
//! limit <bytes>                       the frame limit (2048 by default)
//! root <family>                       extra roots: functions of the family
//! root trait <path>                   extra roots: the methods of every impl
//!                                     of the trait, `<_ as path>::f`, and
//!                                     the trait's own default methods
//!                                     (what a `&dyn Trait` call can reach)
//! root impl <type>                    extra roots: the trait-impl methods of
//!                                     a concrete type, `<type as Trait>::f`,
//!                                     by the type's path (generics and
//!                                     lifetimes left out); the methods a
//!                                     table reaches through a trait object
//! absent trait <path> <why>           the trait has no code in the image yet
//!                                     (nothing implements it, so a call
//!                                     through `&dyn` it reaches nothing):
//!                                     FAILS once a function of it appears,
//!                                     so wiring it cannot go unnoticed
//!                                     while the budget has no root for it;
//!                                     replace the line with `root trait`
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
//! frames-in <family> <why>            every function of the crate (`prefix*`
//!                                     for several, or a module path such as
//!                                     `paperpaw_core::chrome`) in the image is
//!                                     held to the frame limit, whether the
//!                                     roots reach it or not; fails when it
//!                                     matches no function
//! allow rom <callee> <why>            any function may call this routine
//!                                     outside the image (the chip's ROM)
//! allow indirect-in <crate> <max> <why>
//!                                     the functions of the crate (`prefix*`
//!                                     for several) have at most `max` calls
//!                                     through a register between them that
//!                                     no `allow indirect` line lists
//! include <file>                      the lines of a file beside the budget
//! allow sp <caller> <why>             the caller changes the stack pointer
//!                                     other than by `entry`
//! allow nosize <name> <why>           a function with no size (assembly)
//! allow form <caller> <form> <why>    the caller uses a privileged form that
//!                                     leaves a1 alone (`rsil`, `waiti`,
//!                                     `wsr`, `xsr`, `wsr.ps`, `xsr.ps`,
//!                                     `l32e`, `s32e`)
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
//! check pass by matching nothing. A word of a table that points into the code
//! but not at a function's start fails too.
//!
//! **Every instruction form must be classified** (default deny, as the
//! packer's): a function with a form `wide_class` or `narrow_class` of
//! `frames_xtensa` does not know (`rf*`, the MAC16 forms, the S3's
//! `ee.*`) fails, as does an `entry` of a register other than `a1`. A `jx` is
//! counted with the indirect calls.
//!
//! Firmware code (a critical section's `rsil`, an idle `waiti`, a `wsr` or an
//! `xsr` of a special register) uses forms the packer refuses in a plugin.
//! They are not an unknown: each is named by [`named_form`], knows what it
//! writes (`rsil`, `xsr` and `l32e` write `at`, the others no `a` register),
//! and fails unless the budget has an `allow form` line for that function and
//! form, with a reason; one that writes `a1` is a change of the stack pointer
//! and is judged as such, whatever the line says. So is a `wsr` or `xsr` of
//! `WINDOWBASE` or `WINDOWSTART` (special registers 72 and 73): they change
//! the register window, which is what `a1` names (`allow sp` only). A `wsr` or
//! `xsr` of `PS` (230, which holds the window's own enable and increment)
//! is its own form, `wsr.ps` or `xsr.ps`: the generic `wsr` and `xsr` lines
//! cover the other special registers. `rotw` rotates the window, so it is a
//! change of the stack pointer too (`allow sp`, e.g. esp-backtrace's spill of
//! the windows on the panic path). The `rf*` returns, MAC16 and the S3's
//! `ee.*` forms stay unknown: no line can allow them.
//!
//! **Assumption: literals lie outside the functions.** A function's bytes are
//! decoded from its symbol's start for its size; the linker script puts the
//! `.literal` sections between the functions (not inside one), and an `l32r`
//! reads them there. Data inside a function would be decoded as code (and
//! refused by the default deny, or misread). The check on the real firmware
//! (with the S3 wiring, `ci.yml`) is to run this scan on it with no function
//! failing for a form, and to confirm that no function's symbol range holds
//! a word an `l32r` of its own reads (the packer's `uncovered` reads the
//! same layout for plugins).

mod family;
mod regs;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::elf::{EM_XTENSA, Elf, SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE};
use crate::framelist::in_family;
use crate::frames::path;
use crate::frames_xtensa::{Class, entry_frame, length, narrow_class, wide_class};
pub use family::expand_includes;
use regs::{Insn, Writes};

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
    /// `caller` uses the privileged form `form` (one of [`FORMS`]), which
    /// leaves `a1` alone.
    Form { caller: String, form: String },
    /// Any function may call `callee` outside the image (`allow rom`).
    Rom { callee: String },
    /// The functions of the crates `family` have at most `max` calls through
    /// a register that no line of their own lists (`allow indirect-in`).
    IndirectIn { family: String, max: u32 },
}

/// The privileged forms an `allow form` line may name besides the ones of a
/// special register: `wsr.<register>` and `xsr.<register>` (see [`is_form`]).
pub const FORMS: [&str; 4] = ["rsil", "waiti", "l32e", "s32e"];

const SR_LITBASE: u32 = 5;

/// The special registers the firmware's critical sections and start-up code
/// write, by their number (Xtensa ISA, `RSR`/`WSR`): a form names its register
/// (`wsr.scompare1`), so that a line for one register does not cover another.
/// Any other register is `sr<number>`.
const SPECIAL_REGISTERS: [(u32, &str); 29] = [
    (0, "lbeg"),
    (1, "lend"),
    (2, "lcount"),
    (3, "sar"),
    (4, "br"),
    (12, "scompare1"),
    (16, "acclo"),
    (17, "acchi"),
    (32, "m0"),
    (33, "m1"),
    (34, "m2"),
    (35, "m3"),
    (83, "ptevaddr"),
    (99, "atomctl"),
    (177, "epc1"),
    (194, "depc"),
    (209, "excsave1"),
    (226, "interrupt"),
    (228, "intenable"),
    (231, "vecbase"),
    (232, "exccause"),
    (233, "debugcause"),
    (234, "ccount"),
    (236, "icount"),
    (238, "excvaddr"),
    (240, "ccompare0"),
    (241, "ccompare1"),
    (242, "ccompare2"),
    (230, "ps"),
];

/// A special register's name in a form: its name in the table, or `sr<number>`.
fn sr_name(sr: u32) -> String {
    SPECIAL_REGISTERS
        .iter()
        .find(|&&(n, _)| n == sr)
        .map_or_else(|| format!("sr{sr}"), |&(_, name)| name.to_string())
}

/// Whether `form` is one an `allow form` line may name: a name of [`FORMS`], or
/// `wsr.<register>` / `xsr.<register>` of a register in [`SPECIAL_REGISTERS`] or
/// `sr<number>`. Not `litbase` (the literal resolver assumes it off, so a write
/// is refused, never allowed) and not the window registers (`allow sp`).
fn is_form(form: &str) -> bool {
    if FORMS.contains(&form) {
        return true;
    }
    let Some(register) = form
        .strip_prefix("wsr.")
        .or_else(|| form.strip_prefix("xsr."))
    else {
        return false;
    };
    if let Some(n) = register.strip_prefix("sr") {
        return n.parse::<u32>().is_ok_and(|n| {
            n <= 255
                && n != SR_LITBASE
                && n != SR_WINDOWBASE
                && n != SR_WINDOWSTART
                && sr_name(n) == register
        });
    }
    SPECIAL_REGISTERS.iter().any(|&(_, name)| name == register)
}

/// The special registers that name the register window (`WINDOWBASE`,
/// `WINDOWSTART`) and the processor state (`PS`).
const SR_WINDOWBASE: u32 = 72;
const SR_WINDOWSTART: u32 = 73;

/// What the budget asks.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub limit: u32,
    pub roots: Vec<String>,
    /// `root impl <type>`: the trait impls of a concrete type (see [`impl_of`]).
    pub impl_roots: Vec<String>,
    /// `root trait <path>`: the methods of every impl of the trait (see [`trait_of`]).
    pub trait_roots: Vec<String>,
    /// `absent trait <path> <why>`: traits with no code in the image, each
    /// with its reason; a function of one fails (see [`trait_of`]).
    pub absent_traits: Vec<(String, String)>,
    pub tables: Vec<String>,
    /// Each `allow` line with its reason.
    pub allow: Vec<(Allow, String)>,
    /// `frames-in <crate|prefix*|path> <why>`: every function of the family in
    /// the image is held to the frame limit, reached or not.
    pub frames_in: Vec<(String, String)>,
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
                    if budget.limit == 0 || budget.limit > crate::frames_xtensa::LIMIT {
                        return Err(format!(
                            "line {at}: the limit is at most {} (the band's bound), not {}",
                            crate::frames_xtensa::LIMIT,
                            budget.limit
                        ));
                    }
                }
                ["root", family] => budget.roots.push((*family).to_string()),
                ["root", "impl", ty] => budget.impl_roots.push((*ty).to_string()),
                ["root", "trait", tr] => budget.trait_roots.push((*tr).to_string()),
                ["absent", "trait", tr, why @ ..] if !why.is_empty() => budget
                    .absent_traits
                    .push(((*tr).to_string(), why.join(" "))),
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
                ["frames-in", family, why @ ..] if !why.is_empty() => budget
                    .frames_in
                    .push(((*family).to_string(), why.join(" "))),
                ["allow", "rom" | "indirect-in", ..] => budget
                    .allow
                    .push(family::parse_line(&words).ok_or_else(bad)?),
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
                ["allow", "form", caller, form, why @ ..] if !why.is_empty() => {
                    if !is_form(form) {
                        return Err(format!(
                            "line {at}: `{form}` is not a form a line can allow (one of {}, or `wsr.<register>` / `xsr.<register>` naming the special register, such as `wsr.scompare1`)",
                            FORMS.join(", ")
                        ));
                    }
                    budget.allow.push((
                        Allow::Form {
                            caller: (*caller).to_string(),
                            form: (*form).to_string(),
                        },
                        why.join(" "),
                    ))
                }
                _ => return Err(bad()),
            }
        }
        if budget.roots.is_empty()
            && budget.impl_roots.is_empty()
            && budget.trait_roots.is_empty()
            && budget.tables.is_empty()
        {
            return Err("the budget names no root and no table".into());
        }
        Ok(budget)
    }
}

/// A function of the firmware.
#[derive(Debug)]
struct Function {
    path: String,
    /// The raw symbol (`_ZN…E`, with its hash): the one name no other function
    /// has.
    symbol: String,
    /// The symbol as a budget names it: [`stable`] (no hash, no `.NNNN`), the
    /// name that stays the same when other code in the build changes.
    stable: String,
    /// What the problems call the function: its path, or its symbol when
    /// another function of the image has the same path (the path drops the
    /// generics, so every `drop_in_place<…>` is `core::ptr`).
    shown: String,
    start: u32,
    /// 0: the symbol table gives it no size.
    size: u32,
    /// The executable section it is in, and where in its bytes.
    code: (usize, usize),
    /// The other names (symbol, path) of the same address: the linker or the
    /// compiler may fold identical functions or alias one, and a root or a
    /// trait is matched by every name the code has, not by the one kept above.
    aliases: Vec<(String, String)>,
}

impl Function {
    /// Every (symbol, path) the code at this address has.
    fn names(&self) -> impl Iterator<Item = (&str, &str)> {
        core::iter::once((self.symbol.as_str(), self.path.as_str())).chain(
            self.aliases
                .iter()
                .map(|(symbol, path)| (symbol.as_str(), path.as_str())),
        )
    }

    /// Whether `name`, as an `allow` line gives it, is this function's: its
    /// path, its stable symbol or its raw symbol. The stable symbol is the
    /// same for every instance of one generic function (the hash is what tells
    /// them apart), so a line naming it covers each instance whose site it
    /// describes: an `allow indirect` by its count, an `allow call` by its
    /// callee.
    fn is(&self, name: &str) -> bool {
        self.path == name || self.stable == name || self.symbol == name
    }
}

/// A symbol as a budget line names it: without the legacy hash (`17h` and
/// sixteen hex digits before the closing `E`) and without the suffix LLVM adds
/// to a local copy (`.1322`, `.llvm.1234`). Both change with the build (a
/// dependency, an unrelated change), so a line that kept them would fail
/// unrelated pull requests; the rest of the symbol (the path, the type of an
/// impl) is the function's own.
pub(crate) fn stable(symbol: &str) -> String {
    let mut s = symbol;
    // `.NNNN` and `.llvm.NNNN`, possibly both, at the end.
    while let Some((head, tail)) = s.rsplit_once('.') {
        if tail == "llvm" || (!tail.is_empty() && tail.bytes().all(|c| c.is_ascii_digit())) {
            s = head;
        } else {
            break;
        }
    }
    // `17h<16 hex>E` at the end of a legacy symbol: 19 bytes before the `E`.
    if let Some(body) = s.strip_suffix('E')
        && body.len() > 19
        && let Some((head, hash)) = body.split_at_checked(body.len() - 19)
        && hash.starts_with("17h")
        && hash[3..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return head.to_string();
    }
    s.to_string()
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
    /// Calls through a register (`callx8`, `callx12`) that no `l32r` literal
    /// gives.
    indirect: u32,
    /// Calls through a register that did: followed as direct calls.
    resolved: u32,
    /// An instruction that writes `a1`, not `entry`.
    writes_sp: Option<u32>,
    /// An instruction this scan does not know how long it is.
    unreadable: Option<u32>,
    /// An instruction of a form this scan does not classify (default deny, as
    /// the packer's): `rf*`, MAC16, the S3's `ee.*` and the rest.
    unknown: Option<u32>,
    /// The named privileged forms (see [`named_form`]) with the first address
    /// of each: they leave `a1` alone, and the budget must allow each.
    forms: BTreeMap<String, u32>,
    /// An `entry` of a register other than `a1`.
    entry_other: Option<u32>,
}

fn sext18(w: u32) -> i64 {
    let v = i64::from((w >> 6) & 0x3_FFFF);
    if v & 0x2_0000 != 0 { v - 0x4_0000 } else { v }
}

/// Whether `w` is a `jx` (RST0, `snm0` with `r` = 0, `t` = 10): a jump through
/// a register, which this scan cannot follow.
fn is_jx(w: u32) -> bool {
    w & 0xF == 0 && (w >> 4) & 0xF == 0xA && (w >> 12) & 0xFFF == 0
}

/// `t` (bits 7:4) if `w` is a `callx0`, `callx4`, `callx8` or `callx12`: RST0,
/// `st0`, `snm0` with `r` = 0 and `t` = 12 to 15 (`s` is the register).
fn callx_t(w: u32) -> Option<u32> {
    let t = (w >> 4) & 0xF;
    let rst0_snm0 = w & 0xF == 0 && (w >> 12) & 0xF == 0 && (w >> 16) & 0xFF == 0;
    (rst0_snm0 && t >= 0xC).then_some(t)
}

/// A privileged form that `wide_class` leaves unknown but whose effect is
/// known: its name (one of [`FORMS`]), the `a` register it writes, if any, and
/// whether it changes the register window (a `wsr` or `xsr` of `WINDOWBASE` or
/// `WINDOWSTART`, which is a change of what `a1` names). `op0` is 0 (QRST) in
/// all of them. Encodings as the S3 toolchain's assembler makes them
/// (`tests.rs`); the special register is `r << 4 | s`.
fn named_form(w: u32) -> Option<(String, Option<u32>, bool)> {
    let t = (w >> 4) & 0xF;
    let r = (w >> 12) & 0xF;
    let s = (w >> 8) & 0xF;
    let sr = r << 4 | s;
    let window = sr == SR_WINDOWBASE || sr == SR_WINDOWSTART;
    let (op1, op2) = ((w >> 16) & 0xF, (w >> 20) & 0xF);
    if w & 0xF != 0 {
        return None;
    }
    match (op1, op2) {
        // RST0 `st0`: `rsil at, imm` (r = 6) writes `at`; `waiti imm`
        // (r = 7, t = 0) writes no `a` register.
        (0, 0) if r == 6 => Some(("rsil".to_string(), Some(t), false)),
        (0, 0) if r == 7 && t == 0 => Some(("waiti".to_string(), None, false)),
        // RST3: `wsr at, sr` (op2 = 1) writes a special register; RST1: `xsr
        // at, sr` (op2 = 6) swaps `at` with one. The form names the register.
        // LITBASE is no form: the literal resolver assumes it off.
        (3, 1) | (1, 6) if sr == SR_LITBASE => None,
        (3, 1) => Some((format!("wsr.{}", sr_name(sr)), None, window)),
        (1, 6) => Some((format!("xsr.{}", sr_name(sr)), Some(t), window)),
        // LSC4 (op1 = 9): `l32e at, as, imm` (op2 = 0) writes `at`;
        // `s32e at, as, imm` (op2 = 4) writes memory.
        (9, 0) => Some(("l32e".to_string(), Some(t), false)),
        (9, 4) => Some(("s32e".to_string(), None, false)),
        // RST0 `st1` `rotw` (op1 = 0, op2 = 4, `r` = 8): the window rotates by
        // the immediate, so `a1` names another frame (`allow sp`, never a form).
        (0, 4) if r == 8 => Some(("rotw".to_string(), None, true)),
        _ => None,
    }
}

/// What a function's reading may look up: the words of the image (an `l32r`'s
/// literal) and the addresses data points into code at (jump tables).
struct Around<'a> {
    word: &'a dyn Fn(u32) -> Option<u32>,
    pointed: &'a BTreeSet<u32>,
}

fn read_function(code: &[u8], start_addr: u32, around: &Around<'_>) -> Reads {
    let mut reads = Reads::default();
    let mut insns: Vec<Insn> = Vec::new();
    let mut sites: Vec<usize> = Vec::new();
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
            insns.push(Insn {
                pc,
                word: w,
                wide: true,
            });
            if let Some(size) = entry_frame(w) {
                // The frame is taken from the stack pointer only by `entry a1`.
                if (w >> 8) & 0xF != 1 {
                    reads.entry_other.get_or_insert(pc);
                }
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
            } else if is_jx(w) {
                // A jump through a register: not followed, counted as a call.
                reads.indirect += 1;
            } else if let Some(t) = callx_t(w) {
                match t {
                    0xC => reads.narrow_call = reads.narrow_call.or(Some((pc, "callx0"))),
                    0xD => reads.narrow_call = reads.narrow_call.or(Some((pc, "callx4"))),
                    // CALLX8 and CALLX12: through a register; resolved below
                    // when the register holds an `l32r` literal.
                    _ => sites.push(insns.len() - 1),
                }
            } else {
                match wide_class(w) {
                    Class::Safe => {}
                    Class::Writes => {
                        reads.writes_sp.get_or_insert(pc);
                    }
                    Class::Unknown => match named_form(w) {
                        // A written `a1`, or a changed register window, is a
                        // change of the stack pointer.
                        Some((_, Some(1), _) | (_, _, true)) => {
                            reads.writes_sp.get_or_insert(pc);
                        }
                        Some((name, _, _)) => {
                            reads.forms.entry(name).or_insert(pc);
                        }
                        None => {
                            reads.unknown.get_or_insert(pc);
                        }
                    },
                }
            }
        } else {
            let w = u32::from(code[at]) | u32::from(code[at + 1]) << 8;
            insns.push(Insn {
                pc,
                word: w,
                wide: false,
            });
            match narrow_class(w) {
                Class::Safe => {}
                Class::Writes => {
                    reads.writes_sp.get_or_insert(pc);
                }
                Class::Unknown => {
                    reads.unknown.get_or_insert(pc);
                }
            }
        }
        at += len;
    }
    resolve(&insns, &sites, start_addr, around, &mut reads);
    reads
}

/// What a register holds at a point of a function, for [`resolve`]: the
/// address of the `l32r` literal every path to the point loaded into it, or
/// nothing known.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    Literal(u32),
    Unknown,
}

type Regs = [Held; 16];

/// Follows each `callx8`/`callx12` of `sites` to the `l32r` literal its
/// register holds, on every path to it: the call is then a direct one to the
/// literal's address. A forward analysis over the function's own instructions:
/// the state at the entry is "nothing known"; an `l32r` makes its register hold
/// its literal; a write of a register, a call (the registers from its rotation
/// up), `entry` or a window register makes the registers it takes unknown; the
/// state where paths meet keeps a register only if all of them agree. The
/// flow goes by fall-through and branch targets, and every address a word of
/// the image points at inside the function (a jump table) is an entry with
/// nothing known.
///
/// The reading assumes `LITBASE` is off, as the S3's toolchain leaves it (an
/// `l32r` is `pc`-relative: `regs::Insn::l32r`); a firmware that enabled the
/// register would read other addresses, and the literals this finds would be
/// wrong. It fails closed, leaving every call of the function indirect (counted
/// and listed in the budget): at a branch form `regs` does not model, a branch
/// into the middle of an instruction, a hardware `loop` (its back edge is no
/// instruction's), a `jx` in a function that has no address to jump to, and for
/// a site the flow never reaches. A word of the image that equals an address
/// inside the function but is no instruction's start is not a target and is
/// left out. An instruction `regs`
/// does not name makes every register unknown after it.
fn resolve(
    insns: &[Insn],
    sites: &[usize],
    start_addr: u32,
    around: &Around<'_>,
    reads: &mut Reads,
) {
    if sites.is_empty() {
        return;
    }
    let give_up = |reads: &mut Reads| reads.indirect += sites.len() as u32;
    let end = insns.last().map_or(start_addr, |i| i.next());
    let index: BTreeMap<u32, usize> = insns.iter().enumerate().map(|(n, i)| (i.pc, n)).collect();
    // Every address inside the function that a word of the image points at is
    // a place the flow may enter from outside this function's own
    // instructions (a jump table, here or in another function): an entry with
    // nothing known. A `jx` with no such address to go to is a jump this
    // reading cannot place: it gives the function up.
    let mut entries: Vec<usize> = Vec::new();
    for p in around.pointed.range(start_addr..end) {
        // A word that is no instruction's start is not a jump target (the
        // compiler's targets are instruction starts): a number in the data, or
        // bytes of a blob's code, that happens to equal an address here. Where
        // such words fall depends on where the build puts everything, so giving
        // the function up for one would make the result depend on the layout. The
        // reading stays out of step only if a direct branch lands in the middle
        // of an instruction, which gives the function up below.
        let Some(&to) = index.get(p) else {
            continue;
        };
        entries.push(to);
    }
    // The function's own start does not count as an address to jump to: a
    // vtable or a function pointer holds it, a jump table does not. Jump
    // tables are taken to hold absolute addresses of the instructions.
    if entries.iter().all(|&n| n == 0) && insns.iter().any(|i| i.is_jx()) {
        return give_up(reads);
    }
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); insns.len()];
    for (n, i) in insns.iter().enumerate() {
        let Some(targets) = i.targets() else {
            return give_up(reads);
        };
        if i.is_loop() {
            return give_up(reads);
        }
        for target in targets.into_iter().flatten() {
            if (start_addr..end).contains(&target) {
                // Into the middle of an instruction: this reading is out of step.
                let Some(&to) = index.get(&target) else {
                    return give_up(reads);
                };
                next[n].push(to);
            }
        }
        if !i.ends_flow() && n + 1 < insns.len() {
            next[n].push(n + 1);
        }
    }
    let mut state: Vec<Option<Regs>> = vec![None; insns.len()];
    let mut work: VecDeque<usize> = VecDeque::from([0]);
    for &n in std::iter::once(&0).chain(&entries) {
        state[n] = Some([Held::Unknown; 16]);
        work.push_back(n);
    }
    while let Some(n) = work.pop_front() {
        let Some(mut held) = state[n] else { continue };
        match insns[n].writes() {
            Writes::Nothing => {}
            Writes::Reg(r) => {
                held[r as usize] = match insns[n].l32r() {
                    Some((_, literal)) => Held::Literal(literal),
                    None => Held::Unknown,
                };
            }
            Writes::Call(rotation) => {
                for h in &mut held[rotation as usize..] {
                    *h = Held::Unknown;
                }
            }
            Writes::Window | Writes::Unknown => held = [Held::Unknown; 16],
        }
        for &to in &next[n] {
            let joined = match state[to] {
                None => held,
                Some(old) => {
                    let mut joined = old;
                    for (a, b) in joined.iter_mut().zip(&held) {
                        if a != b {
                            *a = Held::Unknown;
                        }
                    }
                    joined
                }
            };
            if state[to] != Some(joined) {
                state[to] = Some(joined);
                work.push_back(to);
            }
        }
    }
    for &k in sites {
        let literal =
            insns[k]
                .callx()
                .zip(state[k])
                .and_then(|(reg, held)| match held[reg as usize] {
                    Held::Literal(literal) => (around.word)(literal),
                    Held::Unknown => None,
                });
        match literal {
            Some(target) => {
                reads.edges.push(target);
                reads.resolved += 1;
            }
            None => reads.indirect += 1,
        }
    }
}

/// The firmware's functions by address, its executable sections (address and
/// bytes), and its data sections (alloc, not executable) for the tables.
struct Image<'a> {
    funcs: Vec<Function>,
    exec: Vec<(u32, &'a [u8])>,
    data: Vec<(u32, &'a [u8])>,
    /// Which of `exec` and `data` are writable sections: what the program can
    /// change is no literal a call can be read from.
    exec_rw: Vec<bool>,
    data_rw: Vec<bool>,
    /// Every named address (any symbol that has a value), for naming a callee
    /// out of the image (the ROM's symbols come from the linker script).
    names: BTreeMap<u32, String>,
    /// The symbols by path with their address and size, for the tables.
    objects: Vec<(String, u32, u32)>,
}

fn image<'a>(elf: &'a Elf<'a>) -> Result<Image<'a>, String> {
    let mut exec: Vec<(u32, &[u8])> = Vec::new();
    let mut data: Vec<(u32, &[u8])> = Vec::new();
    let (mut exec_rw, mut data_rw) = (Vec::new(), Vec::new());
    let mut index_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, s) in elf.sections.iter().enumerate() {
        if s.flags & SHF_ALLOC == 0 || s.kind != 1 {
            continue;
        }
        if s.flags & SHF_EXECINSTR != 0 {
            index_of.insert(i, exec.len());
            exec.push((s.addr, elf.data(s)?));
            exec_rw.push(s.flags & SHF_WRITE != 0);
        } else {
            data.push((s.addr, elf.data(s)?));
            data_rw.push(s.flags & SHF_WRITE != 0);
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
            symbol: s.name.clone(),
            stable: stable(&s.name),
            shown: String::new(),
            start: s.value,
            size: s.size,
            code: (k, from),
            aliases: Vec::new(),
        };
        // Aliases are one function, with the larger size and the first name;
        // the other names stay (`Function::aliases`), so a root or a trait is
        // found by any of them.
        match found.get_mut(&s.value) {
            Some(known) if known.size >= s.size => {
                if known.symbol != function.symbol {
                    known.aliases.push((function.symbol, function.path));
                }
            }
            Some(known) => {
                let mut larger = function;
                larger.aliases = core::mem::take(&mut known.aliases);
                if known.symbol != larger.symbol {
                    larger
                        .aliases
                        .push((known.symbol.clone(), known.path.clone()));
                }
                *known = larger;
            }
            None => {
                found.insert(s.value, function);
            }
        }
    }
    let mut funcs: Vec<Function> = found.into_values().collect();
    let mut by_path: BTreeMap<&str, u32> = BTreeMap::new();
    for f in &funcs {
        *by_path.entry(&f.path).or_default() += 1;
    }
    let shared: BTreeSet<String> = by_path
        .into_iter()
        .filter(|&(_, n)| n > 1)
        .map(|(p, _)| p.to_string())
        .collect();
    for f in &mut funcs {
        f.shown = if shared.contains(&f.path) {
            f.stable.clone()
        } else {
            f.path.clone()
        };
    }
    Ok(Image {
        funcs,
        exec,
        data,
        exec_rw,
        data_rw,
        names,
        objects,
    })
}

/// The words of the static `symbol` that are the start of a function.
fn table_roots(image: &Image<'_>, symbol: &str) -> Option<(Vec<usize>, Vec<u32>)> {
    let mut roots = Vec::new();
    let mut strays = Vec::new();
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
            } else if image.in_image(w) {
                // A word that points into the code but not at a function's
                // start: a slot the scan would otherwise skip unseen.
                strays.push(w);
            }
        }
    }
    matched.then_some((roots, strays))
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
    /// Calls through a register in the set that the scan cannot follow (each
    /// listed in the budget).
    pub indirect: u32,
    /// Calls through a register that an `l32r` literal gave: followed.
    pub resolved: u32,
    /// What the budget refuses, one line each.
    pub problems: Vec<String>,
}

impl Image<'_> {
    /// The word of the image at `address` (an `l32r`'s literal: it is in an
    /// executable section of the S3's linker script, or in a data one), from
    /// the sections the program cannot write only.
    fn word(&self, address: u32) -> Option<u32> {
        let exec = self.exec.iter().zip(&self.exec_rw);
        let data = self.data.iter().zip(&self.data_rw);
        exec.chain(data).find_map(|((base, bytes), &writable)| {
            if writable {
                return None;
            }
            let from = address.checked_sub(*base)? as usize;
            let word = bytes.get(from..from.checked_add(4)?)?;
            Some(u32::from_le_bytes(word.try_into().ok()?))
        })
    }

    /// Every address inside a function that a word outside the functions
    /// (data, or the literal pools between them) holds: where a jump table or
    /// a function pointer can send the flow, so a place another path enters.
    fn pointed(&self) -> BTreeSet<u32> {
        let mut found = BTreeSet::new();
        let mut take = |base: u32, bytes: &[u8], skip_functions: bool| {
            let first = (4 - base % 4) % 4;
            let Some(bytes) = bytes.get(first as usize..) else {
                return;
            };
            for (n, word) in bytes.as_chunks::<4>().0.iter().enumerate() {
                let value = u32::from_le_bytes(*word);
                if skip_functions && self.at(base + first + 4 * n as u32).is_some() {
                    continue;
                }
                if self.at(value).is_some() {
                    found.insert(value);
                }
            }
        };
        for (base, bytes) in &self.data {
            take(*base, bytes, false);
        }
        for (base, bytes) in &self.exec {
            take(*base, bytes, true);
        }
        found
    }

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
    caller: &Function,
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
        let outside = !image.in_image(target);
        if !allowed(budget, used, &|a| {
            matches!(a, Allow::Call { caller: c, callee: e }
                if caller.is(c) && (*e == callee || *e == by_address))
        }) && !(outside
            && allowed(
                budget,
                used,
                &|a| matches!(a, Allow::Rom { callee: e } if *e == callee || *e == by_address),
            ))
        {
            let where_ = if image.in_image(target) {
                "code of the image with no symbol"
            } else {
                "outside the image (ROM or a blob)"
            };
            report.problems.push(format!(
                "`{}` calls `{callee}` ({target:#x}), {where_}: not checkable here (name it with `allow call`)",
                caller.shown
            ));
        }
    }
}

/// Whether the legacy symbol `symbol` is a method of a trait impl of the type
/// `ty` (`a::b::Type`): `_ZN<n>_$LT$a..b..Type$u20$as$u20$…` for a type without
/// generics, `…Type$LT$…$GT$$u20$as…` for one with. A path cannot name these
/// (`path` drops what is not a plain identifier), and they are what a trait
/// object's vtable points at.
fn impl_of(symbol: &str, ty: &str) -> bool {
    let Some(rest) = symbol.strip_prefix("_ZN") else {
        return false;
    };
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    let Some(rest) = rest.strip_prefix("_$LT$") else {
        return false;
    };
    let dotted = ty.replace("::", "..");
    let Some(after) = rest.strip_prefix(dotted.as_str()) else {
        return false;
    };
    after.starts_with("$u20$as$u20$") || after.starts_with("$LT$")
}

/// Whether the function is a method of an impl of the trait `tr` (`a::b::Trait`),
/// or a default method of the trait itself. An impl method's symbol is
/// `…$u20$as$u20$a..b..Trait$GT$…` (or with generic arguments,
/// `…Trait$LT$…`); a default method (which the trait's vtable points at for
/// every impl that does not override it) is `a::b::Trait::method`, with the
/// trait's own path and the method after it: its path starts with
/// `a::b::Trait::`.
fn trait_of(symbol: &str, path: &str, tr: &str) -> bool {
    if !symbol.starts_with("_ZN") {
        return false;
    }
    if path.starts_with(&format!("{tr}::")) {
        return true;
    }
    let want = format!("$u20$as$u20${}", tr.replace("::", ".."));
    symbol.match_indices(&want).any(|(at, _)| {
        let after = &symbol[at + want.len()..];
        after.starts_with("$GT$") || after.starts_with("$LT$")
    })
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
            .filter(|&i| {
                image.funcs[i]
                    .names()
                    .any(|(_, path)| in_family(path, root))
            })
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
    for ty in &budget.impl_roots {
        let members: Vec<usize> = (0..image.funcs.len())
            .filter(|&i| {
                image.funcs[i]
                    .names()
                    .any(|(symbol, _)| impl_of(symbol, ty))
            })
            .collect();
        if members.is_empty() {
            report
                .problems
                .push(format!("the root `impl {ty}` matches no function"));
        }
        for i in members {
            if seen.insert(i) {
                report.roots.0 += 1;
                queue.push_back(i);
            }
        }
    }
    for tr in &budget.trait_roots {
        let members: Vec<usize> = (0..image.funcs.len())
            .filter(|&i| {
                image.funcs[i]
                    .names()
                    .any(|(symbol, path)| trait_of(symbol, path, tr))
            })
            .collect();
        if members.is_empty() {
            report
                .problems
                .push(format!("the root `trait {tr}` matches no function"));
        }
        for i in members {
            if seen.insert(i) {
                report.roots.0 += 1;
                queue.push_back(i);
            }
        }
    }
    // A trait the budget says has no code yet: the day it has, the roots must
    // follow (its methods are what a `&dyn` call reaches).
    for (tr, why) in &budget.absent_traits {
        if let Some(found) = (0..image.funcs.len()).find(|&i| {
            image.funcs[i]
                .names()
                .any(|(symbol, path)| trait_of(symbol, path, tr))
        }) {
            report.problems.push(format!(
                "the trait `{tr}` has code in the image now (`{}`), but the budget says it has none ({why}): replace `absent trait {tr}` with `root trait {tr}` and review what it reaches",
                image.funcs[found].path
            ));
        }
    }
    for table in &budget.tables {
        match table_roots(image, table) {
            None => report.problems.push(format!(
                "the table `{table}` matches no static in the firmware"
            )),
            Some((words, _)) if words.is_empty() => report
                .problems
                .push(format!("the table `{table}` holds no function pointer")),
            Some((words, strays)) => {
                for w in strays {
                    report.problems.push(format!(
                        "the table `{table}` holds {w:#x}, inside the code but not at the start of a function: its slot is not scanned"
                    ));
                }
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

/// The calls through a register of `f` (`count` of them) against the budget:
/// a line of its own, an `allow indirect-in` family, or a problem.
fn check_indirect(
    budget: &Budget,
    f: &Function,
    count: u32,
    (used, family_sites): (&mut [bool], &mut BTreeMap<usize, u32>),
    report: &mut Report,
) {
    let own = allowed(
        budget,
        used,
        &|a| matches!(a, Allow::Indirect { caller: c, count: n } if f.is(c) && *n == count),
    );
    if !own && !family::in_a_family(budget, used, family_sites, f, count) {
        report.problems.push(format!(
            "`{}` has {count} call(s) through a register: not followed here (name them with `allow indirect`)",
            f.shown
        ));
    }
}

/// What the function's own code says against the budget: frame, stack pointer,
/// call width, `entry` and forms the scan does not classify.
fn judge(f: &Function, reads: &Reads, budget: &Budget, used: &mut [bool], report: &mut Report) {
    if let Some(frame) = reads.frame {
        if report.largest.as_ref().is_none_or(|(b, _)| frame > *b) {
            report.largest = Some((frame, f.path.clone()));
        }
        if frame >= budget.limit {
            report.problems.push(format!(
                "`{}` takes a frame of {frame} bytes (limit {})",
                f.shown, budget.limit
            ));
        }
    }
    if let Some(pc) = reads.writes_sp
        && !allowed(
            budget,
            used,
            &|a| matches!(a, Allow::Sp { caller: c } if f.is(c)),
        )
    {
        report.problems.push(format!(
            "`{}` changes the stack pointer other than by `entry` ({pc:#x}): its frame cannot be bounded",
            f.shown
        ));
    }
    if let Some((pc, what)) = reads.narrow_call {
        report.problems.push(format!(
            "`{}` calls with {what} ({pc:#x}): only call8 and call12 keep the band's bound",
            f.shown
        ));
    }
    if let Some(pc) = reads.entry_other {
        report.problems.push(format!(
            "`{}` has an `entry` of a register other than a1 ({pc:#x}): its frame cannot be bounded",
            f.shown
        ));
    }
    for (form, &pc) in &reads.forms {
        if !allowed(
            budget,
            used,
            &|a| matches!(a, Allow::Form { caller: c, form: n } if f.is(c) && n == form),
        ) {
            report.problems.push(format!(
                "`{}` uses `{form}` ({pc:#x}): it leaves a1 alone, but the budget does not say so (name it with `allow form`)",
                f.shown
            ));
        }
    }
    if let Some(pc) = reads.unknown {
        report.problems.push(format!(
            "`{}` has an instruction of a form this scan does not classify ({pc:#x}): not known to leave a1 alone",
            f.shown
        ));
    }
    if let Some(pc) = reads.unreadable {
        report.problems.push(format!(
            "`{}` has code this scan cannot read ({pc:#x})",
            f.shown
        ));
    }
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
    let pointed = image.pointed();
    let word = |address: u32| image.word(address);
    let around = Around {
        word: &word,
        pointed: &pointed,
    };
    let mut family_sites: BTreeMap<usize, u32> = BTreeMap::new();
    seed(&image, budget, &mut report, &mut queue, &mut seen);
    while let Some(i) = queue.pop_front() {
        let f = &funcs[i];
        report.checked += 1;
        if f.size == 0 {
            if !allowed(
                budget,
                &mut used,
                &|a| matches!(a, Allow::NoSize { name: n } if f.is(n)),
            ) {
                report.problems.push(format!(
                    "`{}` ({:#x}) has no size in the symbol table: assembly this scan does not read",
                    f.shown, f.start
                ));
            }
            continue;
        }
        let (k, from) = f.code;
        let data = image.exec[k].1;
        let to = (from + f.size as usize).min(data.len());
        let reads = read_function(&data[from..to], f.start, &around);
        judge(f, &reads, budget, &mut used, &mut report);
        report.indirect += reads.indirect;
        report.resolved += reads.resolved;
        if reads.indirect > 0 {
            let sites = (used.as_mut_slice(), &mut family_sites);
            check_indirect(budget, f, reads.indirect, sites, &mut report);
        }
        follow(
            &image,
            budget,
            f,
            &reads.edges,
            &mut used,
            &mut report,
            (&mut queue, &mut seen),
        );
    }
    // A line names one function, or the instances of one generic function: a
    // path (which drops the generics, so several functions can share it) must
    // not match more than one function reached; a stable symbol can (the
    // instances differ only by their hash), and the line's own count or callee
    // says which of them it describes.
    for (allow, _) in &budget.allow {
        let name = match allow {
            Allow::Rom { .. } | Allow::IndirectIn { .. } => continue,
            Allow::Call { caller: n, .. }
            | Allow::Sp { caller: n }
            | Allow::Indirect { caller: n, .. }
            | Allow::Form { caller: n, .. }
            | Allow::NoSize { name: n } => n,
        };
        let named: Vec<&Function> = seen
            .iter()
            .map(|&i| &funcs[i])
            .filter(|f| f.is(name))
            .collect();
        let by_symbol = named.iter().all(|f| f.stable == *name || f.symbol == *name);
        if named.len() > 1 && !by_symbol {
            let mut symbols: Vec<&str> = named.iter().map(|f| f.stable.as_str()).collect();
            symbols.sort_unstable();
            symbols.dedup();
            report.problems.push(format!(
                "an `allow` line names {} functions, `{name}` is the path of each: name them by their symbol without the hash ({}), as the scan's reports give it",
                named.len(),
                symbols.join(", ")
            ));
        }
    }
    report
        .problems
        .extend(family::ceilings_exceeded(budget, &family_sites));
    report
        .problems
        .extend(family::frames_in_problems(&image, &around, budget));
    for (n, (allow, _)) in budget.allow.iter().enumerate() {
        if !used[n] {
            report.problems.push(format!(
                "an `allow` line matches nothing any more: {allow:?}: remove it"
            ));
        }
    }
    // One line per site is noise: a function calling `memcpy` fifty times is
    // one line (`allow call` is per caller and callee already).
    let mut seen_lines = BTreeSet::new();
    report.problems.retain(|p| seen_lines.insert(p.clone()));
    Ok(report)
}

pub fn run(elf: &str, budget: &str) -> Result<(), String> {
    let bytes = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let parsed = Elf::parse(&bytes).map_err(|e| format!("{elf}: {e}"))?;
    let text = std::fs::read_to_string(budget).map_err(|e| format!("{budget}: {e}"))?;
    let text = expand_includes(&text, std::path::Path::new(budget))
        .map_err(|e| format!("{budget}: {e}"))?;
    let budget = Budget::parse(&text).map_err(|e| format!("{budget}: {e}"))?;
    let report = check(&parsed, &budget).map_err(|e| format!("{elf}: {e}"))?;
    println!(
        "{} functions checked from {} named roots and {} table entries; largest frame {}; {} calls through a register followed (an `l32r` literal gave the target), {} not (each listed in the budget)",
        report.checked,
        report.roots.0,
        report.roots.1,
        report
            .largest
            .as_ref()
            .map_or_else(|| "none".to_string(), |(b, p)| format!("{b} ({p})")),
        report.resolved,
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
