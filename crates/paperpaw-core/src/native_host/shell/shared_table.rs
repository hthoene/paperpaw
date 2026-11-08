//! The one host table every plugin of a services type shares.

use super::{PpHost, Services, host_table};

/// The host table of `S` as one static: every plugin hosted with the same
/// services type is handed the same table, so a hosted plugin costs no copy
/// of it on the heap (`abi/heap-word.md`, built-ins).
pub(super) struct SharedTable<S>(core::marker::PhantomData<S>);

impl<S: Services> SharedTable<S> {
    /// Evaluated at compile time; a reference to a constant without interior
    /// mutability is promoted to a `'static` (read-only data, not heap).
    pub(super) const TABLE: &'static PpHost = &host_table::<S>();
}

/// The X4's per-plugin host context (`abi/heap-word.md` section 9): the boxed
/// owner of a hosted plugin holds a reference to the shared table, never a
/// copy of it. Measured on riscv32: owner 200 bytes, context 168, table 152.
#[cfg(target_arch = "riscv32")]
const _: () = {
    assert!(core::mem::size_of::<super::Owner<(), ()>>() <= OWNER_BUDGET);
    assert!(core::mem::size_of::<super::Context<()>>() < core::mem::size_of::<PpHost>() + 32);
};

/// The most the boxed owner of one hosted plugin may take on the X4
/// (`abi/heap-word.md` section 9).
#[cfg(target_arch = "riscv32")]
const OWNER_BUDGET: usize = 224;
