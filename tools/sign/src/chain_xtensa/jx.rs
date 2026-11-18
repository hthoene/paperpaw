//! A `jx` must be tied to a jump table (#283): the S3 scan reads the flow of a
//! function with the words of its jump tables as entries, and a switch whose
//! table it missed would leave a path out of the proof.
//!
//! A table is tied to its function when one of the function's own `l32r`
//! literals gives its address ([`tied_tables`]). A function with a `jx` and no
//! such table fails unless `allow jx <function> <why>` names it: someone has
//! read the switch. A line that matches nothing any more is a warning.

use std::collections::BTreeSet;

use super::regs::Insn;
use super::{Allow, Around, Budget, Function, Reads, Report, allowed};

/// The first `jx` of a function that has one and no tied table.
pub(super) fn untied(insns: &[Insn], tied: &[u32]) -> Option<u32> {
    insns
        .iter()
        .find(|i| i.is_jx())
        .filter(|_| tied.is_empty())
        .map(|i| i.pc)
}

/// The jump tables a function itself names (#283): the words from the address
/// an `l32r` literal of the function holds, for as long as each is the address
/// of an instruction of the function. A switch's table base is loaded by an
/// `l32r` of the function (there is no `LITBASE`), whether the `jx` follows at
/// once or after a spill, so this finds a table of any length, which the
/// image-wide search (`Image::pointed`) does not (it takes runs of four or
/// more). A table of the function's own start only is a function pointer, not
/// a table. Empty: no table is tied to the function.
pub(super) fn tied_tables(insns: &[Insn], around: &Around<'_>) -> Vec<u32> {
    if !insns.iter().any(|i| i.is_jx()) {
        return Vec::new();
    }
    let start = insns.first().map_or(0, |i| i.pc);
    let starts: BTreeSet<u32> = insns.iter().map(|i| i.pc).collect();
    let mut found = BTreeSet::new();
    for i in insns {
        let Some((_, literal)) = i.l32r() else {
            continue;
        };
        let Some(base) = (around.word)(literal) else {
            continue;
        };
        let mut run = Vec::new();
        let mut at = base;
        while let Some(w) = (around.word)(at).filter(|w| starts.contains(w)) {
            run.push(w);
            at = at.wrapping_add(4);
        }
        if run.iter().any(|&w| w != start) {
            found.extend(run);
        }
    }
    found.into_iter().collect()
}

/// A function whose `jx` no table of its own is tied to fails unless an
/// `allow jx` line names it (someone has read the switch).
pub(super) fn judge(
    f: &Function,
    reads: &Reads,
    budget: &Budget,
    used: &mut [bool],
    report: &mut Report,
) {
    if let Some(pc) = reads.untied_jx
        && !allowed(
            budget,
            used,
            &|a| matches!(a, Allow::Jx { caller: c } if f.is(c)),
        )
    {
        report.problems.push(format!(
            "`{}` has a `jx` ({pc:#x}) and no `l32r` literal of its own gives a jump table: where it jumps is not proven (read it, then name it with `allow jx <function> <why>`)",
            f.shown
        ));
    }
}

/// `resolve` takes a jump table to be its function's own: a part split off a
/// function (`.cold`, `.part.N`) could be entered from the other's table.
pub(super) fn no_split_parts(funcs: &[Function]) -> Result<(), String> {
    let split = |f: &&Function| f.symbol.contains(".cold") || f.symbol.contains(".part.");
    funcs.iter().find(split).map_or(Ok(()), |f| {
        Err(format!(
            "`{}` is a part split off a function: this scan cannot place the jumps into it",
            f.symbol
        ))
    })
}

#[cfg(test)]
#[path = "tests_jx.rs"]
mod tests;
