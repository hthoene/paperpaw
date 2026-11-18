//! The one host table every plugin of a services type shares.

use super::{PpHost, Services, host_table};

/// The host table of `S` as one static: every plugin hosted with the same
/// services type is handed the same table, so a hosted plugin costs no copy
/// of it on the heap (`abi/heap-word.md`, built-ins).
pub(super) struct SharedTable<S>(core::marker::PhantomData<S>);

impl<S: Services + 'static> SharedTable<S> {
    /// Evaluated at compile time; a reference to a constant without interior
    /// mutability is promoted to a `'static` (read-only data, not heap).
    pub(super) const TABLE: &'static PpHost = &host_table::<S>();
}

impl<S: Services + 'static> super::Context<S> {
    /// The one table every plugin hosted with `S` shares: a static, never a
    /// copy per plugin (152 bytes on the X4), and not a pointer per plugin
    /// either, as it follows from `S` (#248).
    pub(crate) const fn table(&self) -> &'static PpHost {
        SharedTable::<S>::TABLE
    }
}

/// The table every plugin hosted with `S` is handed, as the loader hands it
/// (`SharedTable`). A shell that exposes the same table under a name of its
/// own (a static for a code scan) can check at start that it is this table:
/// see [`same_table`].
#[must_use]
pub fn handed_table<S: Services + 'static>() -> &'static PpHost {
    SharedTable::<S>::TABLE
}

/// Whether two host tables are the same, slot by slot (a function pointer
/// each, and the header). A table is nothing but its header (size, major,
/// minor) and function pointers, so it is compared as the machine words it is.
#[must_use]
pub fn same_table(a: &PpHost, b: &PpHost) -> bool {
    const WORD: usize = core::mem::size_of::<usize>();
    const _: () = assert!(core::mem::size_of::<PpHost>().is_multiple_of(WORD));
    let words = core::mem::size_of::<PpHost>() / WORD;
    let (a, b) = (
        core::ptr::from_ref(a).cast::<usize>(),
        core::ptr::from_ref(b).cast::<usize>(),
    );
    // SAFETY: both point at a whole `PpHost`, `repr(C)` with a header of
    // 8 bytes and then only function pointers of one word each, so every one
    // of its `words` words is initialised (no padding) and in bounds.
    (0..words).all(|i| unsafe { a.add(i).read() == b.add(i).read() })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_host::shell::host_table;

    struct Probe;
    impl Services for Probe {
        fn alloc(&self, _: usize, _: usize) -> *mut u8 {
            core::ptr::null_mut()
        }
        unsafe fn dealloc(&self, _: *mut u8, _: usize, _: usize) {}
        unsafe fn realloc(&self, _: *mut u8, _: usize, _: usize, _: usize) -> *mut u8 {
            core::ptr::null_mut()
        }
        fn micros(&self) -> u64 {
            0
        }
    }

    /// A slot no real entry has the address of (it answers 99).
    unsafe extern "C" fn stub(_: *mut core::ffi::c_void, _: *mut u64) -> i32 {
        99
    }

    /// The table is compared as machine words, so the test builds what it
    /// compares from the handed table itself and changes one thing at a time.
    /// It never compares the address of an entry of one table with one of
    /// another: the address of a function may be one per codegen unit, and
    /// the tables the host builds in two places need not agree on it.
    #[test]
    fn the_handed_table_is_the_table_of_the_services_and_a_changed_slot_shows() {
        let handed = handed_table::<Probe>();
        // The header is the services' table's header.
        let own = host_table::<Probe>();
        assert_eq!(
            (own.size, own.major, own.minor),
            (handed.size, handed.major, handed.minor)
        );
        // A table is the same as itself, and as a copy of itself.
        assert!(same_table(handed, handed));
        // SAFETY: `handed` is a whole, initialised `PpHost` (a header and
        // function pointers); the copy is a plain bitwise one of it.
        let mut copy = unsafe { core::ptr::read(handed) };
        assert!(same_table(&copy, handed));
        // The header differs: not the same table.
        copy.minor = copy.minor.wrapping_add(1);
        assert!(!same_table(&copy, handed));
        copy.minor = copy.minor.wrapping_sub(1);
        assert!(same_table(&copy, handed));
        // One slot answered by another function: not the same table.
        copy.clock_micros = stub;
        assert!(!same_table(&copy, handed));
        assert!(!same_table(handed, &copy));
    }
}
