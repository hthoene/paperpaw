//! The firmware's shared function tables, firmware side (ADR 0026, #44;
//! the plugin side is `paperpaw_table::shared!`).
//!
//! [`table`] is what a shell that loads code plugins for a RISC-V chip
//! writes into the family words of every plugin it loads
//! (`PAPERPAW_TABLE_<n>`): the address of each function the SDK and the
//! crates below it share, in the order of `shared.list`, which is the
//! plugin interface. A plugin built for such a chip calls these instead
//! of carrying its own copies.

use paperpaw_table::{Family, Table};

/// A plain array of function addresses, never written.
#[derive(Debug)]
pub struct Addresses<const N: usize>(pub [*const (); N]);

// SAFETY: addresses of functions that live as long as the program runs,
// never written.
unsafe impl<const N: usize> Sync for Addresses<N> {}

#[path = "shared_table.rs"]
mod tables;

/// The table of `family`, [`Table::NONE`] for one the firmware does not
/// share. The revisions of the families are `paperpaw_table::REVISIONS`.
#[must_use]
pub const fn table(family: Family) -> Table {
    tables::of(family.number())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every family with revisions has a table of one non-null address for
    /// each index of `paperpaw_table::index`.
    #[test]
    fn the_text_table_is_whole() {
        let t = table(Family::TEXT);
        assert!(!t.0.is_null());
        for i in 0..paperpaw_table::index::text::COUNT {
            // SAFETY: within the table.
            assert!(!unsafe { *t.0.add(i) }.is_null(), "entry {i}");
        }
        assert_eq!(table(Family::FONTS).0, Table::NONE.0);
    }
}
