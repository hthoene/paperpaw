//! Spare lines, pooled per crate (#315): LLVM's inlining moves a call site from a
//! function to its caller (or back) from one build to the next, so a site no line
//! names in this build may be one a line of the same crate names for a function
//! that no longer has it. The counts the budget lists are the reviewed ones, so
//! the scan takes such a site from the spare of the crate's own lines instead of
//! failing, and says so:
//!
//! - a function with more calls through a register than its `allow indirect`
//!   line gives (or with no line) draws on the spare of the lines of its crate:
//!   each line's count less what the functions it names have now;
//! - a call out of the image that no `allow call` line names for this caller
//!   draws on an unused `allow call` line of the same crate and callee.
//!
//! What cannot be drawn fails as before. The sum of the sites per crate never
//! exceeds the sum of the reviewed counts (plus each line's own margin), so the
//! bound is the same; what the pool takes away is the red base after a landing
//! that touched nothing of the function.

use super::{Allow, Budget, Function, INDIRECT_MARGIN, Report, family};
use std::collections::BTreeMap;

/// The crate a line's function name belongs to: the first segment of its path,
/// or the type's crate in an impl's symbol.
fn crate_of_name(name: &str) -> Option<String> {
    if name.starts_with("_ZN") {
        return family::crate_of_symbol(name);
    }
    let (head, _) = name.split_once("::")?;
    (!head.starts_with('<')).then(|| head.to_string())
}

/// Unlisted sites of the scan, held until the whole image is read.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Pool {
    /// Per `allow indirect` line (by its index): the calls the functions it names have.
    found: BTreeMap<usize, u32>,
    /// Functions over their line (or with none): crate, function, calls beyond
    /// the line's ceiling, all its calls.
    indirect: Vec<(Option<String>, String, u32, u32)>,
    /// Calls out of the image no line names: crate of the caller, callee, the problem.
    calls: Vec<(Option<String>, String, String)>,
}

impl Pool {
    /// `f` has `count` calls through a register that no line covers.
    pub(super) fn indirect(&mut self, budget: &Budget, f: &Function, count: u32) {
        let mut ceiling = 0;
        for (n, (allow, _)) in budget.allow.iter().enumerate() {
            if let Allow::Indirect { caller, count: c } = allow
                && f.is(caller)
            {
                *self.found.entry(n).or_default() += count;
                ceiling = ceiling.max(c + INDIRECT_MARGIN);
            }
        }
        let beyond = count.saturating_sub(ceiling);
        self.indirect
            .push((family::crate_of(f), f.shown.clone(), beyond, count));
    }

    /// `caller` calls `callee` outside the image and no line names the pair.
    pub(super) fn call(&mut self, caller: &Function, callee: &str, problem: String) {
        self.calls
            .push((family::crate_of(caller), callee.to_string(), problem));
    }
}

/// Settles the pool against the spare of the lines, then reports the lines nothing used.
pub(super) fn settle(budget: &Budget, used: &mut [bool], report: &mut Report) {
    let pool = std::mem::take(&mut report.pool);
    // The spare of each `allow indirect` line: its count less what its functions have now.
    let mut spare: BTreeMap<usize, u32> = BTreeMap::new();
    for (n, (allow, _)) in budget.allow.iter().enumerate() {
        if let Allow::Indirect { count, .. } = allow {
            let left = count.saturating_sub(pool.found.get(&n).copied().unwrap_or(0));
            if left > 0 {
                spare.insert(n, left);
            }
        }
    }
    let crate_of_line = |n: usize| match &budget.allow[n].0 {
        Allow::Indirect { caller, .. } | Allow::Call { caller, .. } => crate_of_name(caller),
        _ => None,
    };
    for (krate, shown, mut need, count) in pool.indirect {
        let from: Vec<usize> = spare
            .keys()
            .copied()
            .filter(|&n| krate.is_some() && crate_of_line(n) == krate)
            .collect();
        if need > 0 && need <= from.iter().map(|n| spare[n]).sum::<u32>() {
            for n in from {
                let part = need.min(spare[&n]);
                if part > 0 {
                    *spare.entry(n).or_default() -= part;
                    used[n] = true;
                    need -= part;
                }
            }
            report.warnings.push(format!(
                "`{shown}` has {count} call(s) through a register that no line names: taken from the spare of the lines of `{}` (inlining moved them; list it when you pass by)",
                krate.unwrap_or_default()
            ));
        } else {
            report.problems.push(format!(
                "`{shown}` has {count} call(s) through a register: not followed here (name them with `allow indirect`)"
            ));
        }
    }
    for (krate, callee, problem) in pool.calls {
        let line = budget.allow.iter().enumerate().find(|(n, (a, _))| {
            !used[*n]
                && matches!(a, Allow::Call { callee: e, .. } if *e == callee)
                && krate.is_some()
                && crate_of_line(*n) == krate
        });
        if let Some((n, _)) = line {
            used[n] = true;
            report.warnings.push(format!(
                "{problem}: covered by an unused `allow call` line of the same crate and callee (inlining moved the call; name the caller when you pass by)"
            ));
        } else {
            report.problems.push(problem);
        }
    }
    unused(budget, used, report);
}

/// The `allow` lines nothing used: a warning for the kinds that only grant what a site needs
/// (#309; they are the spare of their crate now, remove one only when the code behind it is
/// gone), a problem for the rest.
fn unused(budget: &Budget, used: &[bool], report: &mut Report) {
    for (n, (allow, _)) in budget.allow.iter().enumerate() {
        if used[n] {
            continue;
        }
        let line = format!("an `allow` line matches nothing any more: {allow:?}");
        if matches!(
            allow,
            Allow::Call { .. } | Allow::Indirect { .. } | Allow::Jx { .. }
        ) {
            report.warnings.push(format!("{line}: kept as the spare of its crate; remove it only when the code behind it is gone"));
        } else {
            report.problems.push(format!("{line}: remove it"));
        }
    }
}
