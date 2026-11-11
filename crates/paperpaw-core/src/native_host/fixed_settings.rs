//! Settings a linked plugin keeps in the program (Beta 3's way) instead of
//! one heap-owned `SettingSpec` for each record it declares.
//!
//! The wire record stays the source of truth: the kernel makes the spec of a
//! record as it always did, and takes the plugin's table entry in its place
//! only when the two are equal field by field. Any other spec stays the owned
//! copy, so a table that is stale or wrong costs memory, never correctness.

use paperpaw_sdk::SettingSpec;

/// Whether `table` entry `kept` is the same setting as `made` (the spec its
/// record makes): every field a reader of the registry can see
/// ([`SettingSpec::same_as`], which a new field breaks the build of).
pub(super) fn same_setting(made: &SettingSpec, kept: &SettingSpec) -> bool {
    made.same_as(kept)
}

/// Where the entry of `table` that equals `made` is. The records come in no
/// order the table keeps (hash order), so the whole table is looked through:
/// an id differs at once, so it costs a compare of two short texts for each.
pub(super) fn find(table: &[SettingSpec], made: &SettingSpec) -> Option<usize> {
    table.iter().position(|kept| same_setting(made, kept))
}

#[cfg(test)]
#[path = "fixed_settings_tests.rs"]
mod tests;
