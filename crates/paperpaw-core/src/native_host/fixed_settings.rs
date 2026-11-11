//! Settings a linked plugin keeps in the program (Beta 3's way) instead of
//! one heap-owned `SettingSpec` for each record it declares.
//!
//! The wire record stays the source of truth: the kernel makes the spec of a
//! record as it always did, and takes the plugin's table entry in its place
//! only when the two are equal field by field. Any other spec stays the owned
//! copy, so a table that is stale or wrong costs memory, never correctness.

use paperpaw_sdk::{SettingGroupSpec, SettingSpec};

/// The tables a linked plugin keeps in the program (its build script makes
/// them of the plugin's own declarations): its settings and its setting
/// groups. What a plugin declares that equals an entry is kept as that entry,
/// not as a copy on the heap; anything else is copied as before.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct FixedTables {
    /// The plugin's settings.
    pub settings: &'static [SettingSpec],
    /// The plugin's setting groups.
    pub groups: &'static [SettingGroupSpec],
}

impl FixedTables {
    /// No table: everything the plugin declares is copied.
    pub const NONE: Self = Self {
        settings: &[],
        groups: &[],
    };

    /// The tables `settings` and `groups`.
    #[must_use]
    pub const fn new(
        settings: &'static [SettingSpec],
        groups: &'static [SettingGroupSpec],
    ) -> Self {
        Self { settings, groups }
    }
}

/// The entry of `table` that equals the group `made` (its record's copy).
pub(super) fn find_group(
    table: &'static [SettingGroupSpec],
    made: &SettingGroupSpec,
) -> Option<&'static SettingGroupSpec> {
    table.iter().find(|kept| *kept == made)
}

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
