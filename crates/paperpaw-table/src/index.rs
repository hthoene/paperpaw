// Made by tools/shared-table.py from shared.list: do not edit.

/// The revision of each family (`shared.list`).
pub const REVISIONS: crate::Revisions = [0, 0, 0, 0, 0, 0, 0, 0];
/// The oldest revision of each family whose plugins still run.
pub const OLDEST: crate::Revisions = [0, 0, 0, 0, 0, 0, 0, 0];

/// The text family's table.
pub mod text {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::TEXT;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}

/// The fonts family's table.
pub mod fonts {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::FONTS;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}

/// The ui family's table.
pub mod ui {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::UI;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}

/// The graphics family's table.
pub mod graphics {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::GRAPHICS;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}

/// The contribution family's table.
pub mod contribution {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::CONTRIBUTION;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}
