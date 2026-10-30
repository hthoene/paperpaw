//! The firmware's shared function table, firmware side (prototype of
//! option B2, #44; the plugin side is `paperpaw_native::shared!`).
//!
//! [`table`] is what a shell that loads code plugins for a chip gives
//! them in `Api::shared`: the address of each function the SDK and the
//! crates below it share, in the order of `shared.list` (the plugin
//! interface). A plugin built for a chip with `--cfg paperpaw_plugin`
//! calls these instead of its own copies.

/// A plain array of function addresses, never written.
#[derive(Debug)]
pub struct Addresses<const N: usize>(pub [*const (); N]);

// SAFETY: addresses of functions that live as long as the program runs,
// never written.
unsafe impl<const N: usize> Sync for Addresses<N> {}

#[cfg(not(all(paperpaw_plugin, target_os = "none")))]
#[path = "shared_table.rs"]
mod table;

/// The table for `Api::shared`.
#[cfg(not(all(paperpaw_plugin, target_os = "none")))]
pub const fn table() -> paperpaw_native::shared::Table {
    paperpaw_native::shared::Table(table::ADDRESSES.0.as_ptr())
}

/// How many entries [`table`] has, for `Api::shared_len`.
pub const COUNT: u32 = paperpaw_native::shared::NEEDS;

#[cfg(all(test, not(all(paperpaw_plugin, target_os = "none"))))]
mod tests {
    use super::*;

    /// The table has one non-null address for each index.
    #[test]
    fn the_table_is_whole() {
        let t = table();
        for i in 0..COUNT as usize {
            // SAFETY: within the table.
            assert!(!unsafe { *t.0.add(i) }.is_null(), "entry {i}");
        }
    }
}
