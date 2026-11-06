#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use abi::{OK, PpBuffer, PpBytes, PpPlugin};
use alloc::rc::Rc;
use core::cell::Cell;
use core::{alloc::Layout, mem, ptr};

#[derive(Default)]
struct Context {
    allocated: Cell<usize>,
    freed: Cell<usize>,
    fail_alloc: Cell<bool>,
    clock_error: Cell<i32>,
    micros: Cell<u64>,
    dropped: Rc<Cell<usize>>,
    poisoned: Cell<bool>,
    batches: Cell<u32>,
}
unsafe extern "C" fn allocate(
    ctx: *mut c_void,
    size: usize,
    align: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: tests pass a live Context through the table.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    if ctx.fail_alloc.get() {
        return abi::ERR_NO_MEMORY;
    }
    // SAFETY: entry supplies a valid nonzero layout and writable output.
    unsafe {
        out.write(alloc::alloc::alloc(Layout::from_size_align(size, align).unwrap()).cast());
    }
    ctx.allocated.set(ctx.allocated.get() + 1);
    OK
}
unsafe extern "C" fn deallocate(
    ctx: *mut c_void,
    block: *mut c_void,
    size: usize,
    align: usize,
) -> i32 {
    // SAFETY: tests pass Context; block and layout come from allocate exactly once.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        ctx.freed.set(ctx.freed.get() + 1);
        alloc::alloc::dealloc(block.cast(), Layout::from_size_align(size, align).unwrap());
    }
    OK
}
unsafe extern "C" fn clock(ctx: *mut c_void, out: *mut u64) -> i32 {
    // SAFETY: tests pass live Context and a disjoint writable output.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    if ctx.clock_error.get() != OK {
        return ctx.clock_error.get();
    }
    ctx.micros.set(ctx.micros.get().saturating_add(1));
    // SAFETY: wrapper supplies aligned output storage.
    unsafe {
        out.write(ctx.micros.get());
    }
    OK
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.heap_alloc = allocate;
    table.heap_dealloc = deallocate;
    table.clock_micros = clock;
    table
}
struct TestPlugin {
    dropped: Rc<Cell<usize>>,
}
impl Plugin for TestPlugin {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        super::super::clock::micros(host)?;
        // SAFETY: this test's entry context is a live Context.
        let ctx = unsafe { &*host.ctx.cast::<Context>() };
        Ok(Self {
            dropped: Rc::clone(&ctx.dropped),
        })
    }
    fn event(&mut self, host: &Host<'_>, token: u32, input: &[u8]) -> Result<(), Error> {
        assert_eq!(token, 42);
        assert_eq!(input, b"event");
        assert!(super::super::clock::micros(host)? > 1);
        Ok(())
    }
    fn draw(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        assert_eq!(token, 43);
        super::super::clock::micros(host)?;
        Err(Error::Busy)
    }
}
impl Drop for TestPlugin {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
    }
}
const TEST_METADATA: &[u8; 32] =
    b"PPAM\x02\0\x20\0\x20\0\0\0\x07\0\x01\0\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
crate::native_plugin!(TestPlugin, metadata = TEST_METADATA);

#[test]
fn macro_entry_dispatches_and_drops_with_the_matching_host_allocator() {
    let table = table();
    let mut ctx = Context::default();
    let mut plugin: *const PpPlugin = ptr::null();
    // SAFETY: Entry is repr(transparent) over the generated entry record.
    let entry = unsafe { &*ptr::from_ref(&PAPERPAW_ENTRY).cast::<abi::PpEntry>() };
    assert_eq!(
        (entry.abi_major, entry.needs_minor),
        (abi::MAJOR, abi::MINOR)
    );
    // SAFETY: tables and context outlive the live instance and every callback.
    unsafe {
        assert_eq!(
            entry.make.unwrap()(&table, ptr::from_mut(&mut ctx).cast(), &mut plugin),
            OK
        );
        assert!(!plugin.is_null());
        let p = &*plugin;
        assert_eq!(
            p.dispatch_event.unwrap()(
                p.state,
                42,
                PpBytes {
                    ptr: b"event".as_ptr(),
                    len: 5
                },
                no_reply()
            ),
            OK
        );
        assert_eq!(p.draw.unwrap()(p.state, 43, empty()), abi::ERR_BUSY);
        assert_eq!(
            p.dispatch_event.unwrap()(
                p.state,
                42,
                PpBytes {
                    ptr: ptr::null(),
                    len: 1
                },
                no_reply()
            ),
            abi::ERR_INVALID
        );
        assert_eq!(
            p.dispatch_event.unwrap()(
                p.state,
                42,
                // N7: no fixed cap, only length consistency.
                PpBytes {
                    ptr: b"x".as_ptr(),
                    len: usize::MAX
                },
                no_reply()
            ),
            abi::ERR_INVALID
        );
        assert_eq!(
            p.draw.unwrap()(ptr::null_mut(), 43, empty()),
            abi::ERR_INVALID
        );
        assert_eq!(
            p.dispatch_event.unwrap()(
                ptr::null_mut(),
                42,
                PpBytes {
                    ptr: ptr::null(),
                    len: 0
                },
                no_reply()
            ),
            abi::ERR_INVALID
        );
        p.drop.unwrap()(p.state);
    }
    assert_eq!(
        (ctx.allocated.get(), ctx.freed.get(), ctx.dropped.get()),
        (1, 1, 1)
    );
}

#[test]
fn short_table_is_refused_before_reading_version_or_slot_pointers() {
    let mut ctx = Context::default();
    let mut output = ptr::dangling::<PpPlugin>();
    let original = output;
    for size in [0, 4, mem::size_of::<PpHost>() as u32 - 1] {
        // Only the header is accessible; the size check must read nothing else.
        let header = size;
        // SAFETY: failed size admission needs only the readable u32 header.
        let status = unsafe {
            entry::make::<TestPlugin, { abi::MINOR }>(
                ptr::from_ref(&header).cast(),
                ptr::from_mut(&mut ctx).cast(),
                &mut output,
            )
        };
        assert_eq!(status, abi::ERR_UNSUPPORTED);
        assert_eq!(output, original);
    }
    assert_eq!(ctx.allocated.get(), 0);
}

#[test]
fn null_arguments_and_incompatible_versions_do_not_allocate() {
    let mut ctx = Context::default();
    let ctx_ptr = ptr::from_mut(&mut ctx).cast();
    let mut output = ptr::null();
    let mut table = table();
    // SAFETY: nulls are explicitly allowed for validation; other pointers live.
    unsafe {
        assert_eq!(
            entry::make::<TestPlugin, { abi::MINOR }>(ptr::null(), ctx_ptr, &mut output),
            abi::ERR_INVALID
        );
        assert_eq!(
            entry::make::<TestPlugin, { abi::MINOR }>(&table, ptr::null_mut(), &mut output),
            abi::ERR_INVALID
        );
        assert_eq!(
            entry::make::<TestPlugin, { abi::MINOR }>(&table, ctx_ptr, ptr::null_mut()),
            abi::ERR_INVALID
        );
        table.major += 1;
        assert_eq!(
            entry::make::<TestPlugin, { abi::MINOR }>(&table, ctx_ptr, &mut output),
            abi::ERR_UNSUPPORTED
        );
        table.major = abi::MAJOR;
        table.minor = 0;
        assert_eq!(
            entry::make::<TestPlugin, { abi::MINOR }>(&table, ctx_ptr, &mut output),
            abi::ERR_UNSUPPORTED
        );
    }
    assert_eq!(ctx.allocated.get(), 0);
}

#[test]
fn allocation_and_plugin_make_failures_do_not_publish_state() {
    let table = table();
    let mut ctx = Context::default();
    let mut output = ptr::null();
    ctx.fail_alloc.set(true);
    // SAFETY: test context and table live; output is writable and disjoint.
    assert_eq!(
        unsafe {
            entry::make::<TestPlugin, { abi::MINOR }>(
                &table,
                ptr::from_mut(&mut ctx).cast(),
                &mut output,
            )
        },
        abi::ERR_NO_MEMORY
    );
    assert!(output.is_null());
    ctx.fail_alloc.set(false);
    ctx.clock_error.set(abi::ERR_INVALID);
    // SAFETY: same live test table/context/output.
    assert_eq!(
        unsafe {
            entry::make::<TestPlugin, { abi::MINOR }>(
                &table,
                ptr::from_mut(&mut ctx).cast(),
                &mut output,
            )
        },
        abi::ERR_INVALID
    );
    assert!(output.is_null());
    assert_eq!(
        (ctx.allocated.get(), ctx.freed.get(), ctx.dropped.get()),
        (1, 1, 0)
    );
}

#[test]
fn clock_errors_and_unknown_statuses_are_preserved() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    assert_eq!(super::super::clock::micros(&host), Ok(1));
    assert_eq!(super::super::clock::micros(&host), Ok(2));
    ctx.clock_error.set(abi::ERR_INVALID);
    assert_eq!(super::super::clock::micros(&host), Err(Error::Invalid));
    for status in -11..0 {
        assert_eq!(Error::from_status(status).status(), status);
    }
    assert_eq!(Error::from_status(-900).status(), -900);
    assert_eq!(Error::Other(1).status(), abi::ERR_INVALID);
    assert_eq!(i32::from(Error::Other(1)), 1);
    assert_eq!(i32::from(Error::Other(-900)), -900);
    assert_eq!(i32::from(Error::Busy), abi::ERR_BUSY);
}

unsafe extern "C" fn transactional_submit(ctx: *mut c_void, batch: PpBytes) -> i32 {
    // SAFETY: fake entry host retains this Context through construction.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    ctx.batches.set(ctx.batches.get() + 1);
    if batch.len > 4096 {
        ctx.poisoned.set(true);
        return abi::ERR_NO_SPACE;
    }
    if ctx.poisoned.get() {
        return abi::ERR_STATE;
    }
    OK
}

struct IgnoringPlugin(Rc<Cell<usize>>);
impl Plugin for IgnoringPlugin {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        // Deliberately ignore both errors: the host must refuse activation even
        // when plugin code swallows an earlier construction failure.
        let _ = super::super::registrations::submit(host, &[0; 4097]);
        let _ = super::super::registrations::submit(host, &[0]);
        // SAFETY: test host context is retained for this entire make call.
        let ctx = unsafe { &*host.ctx.cast::<Context>() };
        Ok(Self(Rc::clone(&ctx.dropped)))
    }
}
impl Drop for IgnoringPlugin {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn ignored_oversize_then_valid_submission_cannot_activate_and_state_is_freed_once() {
    let mut table = table();
    table.registrations_submit = transactional_submit;
    let mut ctx = Context::default();
    let mut plugin = ptr::null();
    // SAFETY: fake host table/context/output outlive this construction.
    assert_eq!(
        unsafe {
            entry::make::<IgnoringPlugin, { abi::MINOR }>(
                &table,
                ptr::from_mut(&mut ctx).cast(),
                &mut plugin,
            )
        },
        OK
    );
    assert_eq!(ctx.batches.get(), 2);
    assert!(ctx.poisoned.get());
    assert!(!plugin.is_null());
    // Fake host transaction commit: poison refuses activation and revokes the
    // returned state. This is independent of the plugin's successful make.
    let activation = if ctx.poisoned.get() {
        // SAFETY: returned state is live; refused construction destroys it once.
        unsafe {
            ((*plugin).drop.unwrap())((*plugin).state);
        }
        Err(Error::NoSpace)
    } else {
        Ok(())
    };
    assert_eq!(activation, Err(Error::NoSpace));
    assert_eq!(
        (ctx.allocated.get(), ctx.freed.get(), ctx.dropped.get()),
        (1, 1, 1)
    );
}

#[test]
fn entry_minor_is_derived_from_metadata_and_older_host_is_accepted() {
    const OLDER: &[u8; 32] = &{
        let mut bytes = *TEST_METADATA;
        bytes[14] = 0;
        bytes
    };
    let entry = entry::Entry::new::<TestPlugin, 0>(OLDER).unwrap();
    // SAFETY: Entry is repr(transparent) over its generated PpEntry field.
    let descriptor = unsafe { &*ptr::from_ref(&entry).cast::<abi::PpEntry>() };
    assert_eq!(descriptor.needs_minor, 0);
    // On riscv32, the linker and packer enforce entry alignment instead.
    #[cfg(target_pointer_width = "64")]
    assert_eq!(mem::align_of_val(&PAPERPAW_ENTRY), 8);
    // SAFETY: exported Entry is repr(transparent) over PpEntry.
    let exported = unsafe { &*ptr::from_ref(&PAPERPAW_ENTRY).cast::<abi::PpEntry>() };
    assert_eq!(exported.metadata.ptr, PAPERPAW_METADATA.as_ptr());
    assert_eq!(exported.metadata.len, PAPERPAW_METADATA.len());
    let mut table = table();
    table.minor = 0;
    let mut ctx = Context::default();
    let mut plugin = ptr::null();
    // SAFETY: fake host table/context and output live until destruction.
    unsafe {
        assert_eq!(
            descriptor.make.unwrap()(&table, ptr::from_mut(&mut ctx).cast(), &mut plugin),
            OK
        );
        ((*plugin).drop.unwrap())((*plugin).state);
    }
    assert_eq!(
        (ctx.allocated.get(), ctx.freed.get(), ctx.dropped.get()),
        (1, 1, 1)
    );
}

#[test]
fn invalid_metadata_prefixes_return_none_without_panicking() {
    assert_eq!(entry::metadata_minor(&[]), None);
    for offset in [0, 4, 6, 8, 12, 14] {
        let mut bytes = *TEST_METADATA;
        bytes[offset] = 255;
        assert_eq!(entry::metadata_minor(&bytes), None);
    }
    assert!(entry::Entry::new::<TestPlugin, 0>(TEST_METADATA).is_none());
}

fn no_reply() -> PpBuffer {
    PpBuffer {
        ptr: ptr::NonNull::dangling().as_ptr(),
        cap: 0,
    }
}

fn empty() -> PpBytes {
    PpBytes {
        ptr: ptr::NonNull::dangling().as_ptr(),
        len: 0,
    }
}
