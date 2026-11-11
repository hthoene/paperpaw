//! Settings a linked plugin keeps in the program (Beta 3's way) instead of
//! one heap-owned `SettingSpec` for each record it declares.
//!
//! The wire record stays the source of truth: the kernel makes the spec of a
//! record as it always did, and takes the plugin's table entry in its place
//! only when the two are equal field by field. Any other spec stays the owned
//! copy, so a table that is stale or wrong costs memory, never correctness.

use paperpaw_sdk::{SettingKind, SettingSpec};

/// Whether `table` entry `kept` is the same setting as `made` (the spec its
/// record makes): every field a reader of the registry can see.
pub(super) fn same_setting(made: &SettingSpec, kept: &SettingSpec) -> bool {
    made.id == kept.id
        && made.title == kept.title
        && made.default == kept.default
        && made.group == kept.group
        && made.level == kept.level
        && made.description == kept.description
        && made.scoped == kept.scoped
        && made.guarded == kept.guarded
        && made.kept_on_reset == kept.kept_on_reset
        && made.extra() == kept.extra()
        && same_kind(&made.kind, &kept.kind)
}

/// Kinds are equal when they are, and a choice list is the same list whether
/// it is kept in the program or made on the heap.
fn same_kind(made: &SettingKind, kept: &SettingKind) -> bool {
    match (made, kept) {
        (SettingKind::Choice(made), SettingKind::Choice(kept)) => made.iter().eq(kept.iter()),
        (made, kept) => made == kept,
    }
}

/// Where the entry of `table` that equals `made` is, trying `from` first (a
/// table lists the specs in the order the plugin declares them).
pub(super) fn find(table: &[SettingSpec], made: &SettingSpec, from: usize) -> Option<usize> {
    let tail = table.get(from..).unwrap_or_default();
    let at = tail
        .iter()
        .position(|kept| same_setting(made, kept))
        .map(|at| at + from)
        .or_else(|| {
            table[..from.min(table.len())]
                .iter()
                .position(|kept| same_setting(made, kept))
        })?;
    Some(at)
}

#[cfg(test)]
#[path = "fixed_settings_tests.rs"]
mod tests;
