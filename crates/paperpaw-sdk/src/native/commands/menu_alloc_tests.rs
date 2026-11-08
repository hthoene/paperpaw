//! Count requested temporary bytes, excluding caller buffers and host storage.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
extern crate std;
use super::*;
use core::{cell::Cell, ffi::c_void};
use paperpaw_native::host_glue::{self as abi, PpHost};
use std::alloc::{GlobalAlloc, Layout, System};

#[derive(Clone, Copy, Default)]
struct Counts {
    enabled: bool,
    refuse: bool,
    refused: bool,
    live: usize,
    peak: usize,
}
std::thread_local! {
    static COUNTS: Cell<Counts> = Cell::new(Counts::default());
}
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let refuse = COUNTS
            .try_with(|cell| {
                let mut count = cell.get();
                if count.enabled && count.refuse {
                    count.refuse = false;
                    count.refused = true;
                    cell.set(count);
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if refuse {
            return core::ptr::null_mut();
        }
        // SAFETY: forward the allocator's unchanged layout.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            COUNTS
                .try_with(|cell| {
                    let mut count = cell.get();
                    if count.enabled {
                        count.live += layout.size();
                        count.peak = count.peak.max(count.live);
                        cell.set(count);
                    }
                })
                .unwrap_or(());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        COUNTS
            .try_with(|cell| {
                let mut count = cell.get();
                if count.enabled {
                    count.live -= layout.size();
                    cell.set(count);
                }
            })
            .unwrap_or(());
        // SAFETY: forward the allocation and its original layout.
        unsafe { System.dealloc(pointer, layout) };
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn track<T>(refuse: bool, operation: impl FnOnce() -> T) -> (T, Counts) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            COUNTS.with(|cell| cell.set(Counts::default()));
        }
    }
    COUNTS.with(|cell| {
        cell.set(Counts {
            enabled: true,
            refuse,
            ..Counts::default()
        })
    });
    let reset = Reset;
    let result = operation();
    let counts = COUNTS.with(Cell::get);
    drop(reset);
    (result, counts)
}

unsafe extern "C" fn submit(ctx: *mut c_void, _: PpBytes) -> i32 {
    assert_eq!(
        COUNTS.with(|cell| cell.get().live),
        0,
        "temporary must be freed before host call"
    );
    // SAFETY: the fixture keeps its exclusive callback counter alive.
    let calls = unsafe { &*ctx.cast::<Cell<usize>>() };
    calls.set(calls.get() + 1);
    abi::OK
}
fn host<'a>(table: &'a PpHost, calls: &mut Cell<usize>) -> Host<'a> {
    Host {
        table,
        ctx: core::ptr::from_mut(calls).cast(),
    }
}

#[test]
fn maximum_menu_uses_exact_fallible_bytes_and_frees_them_before_submit() {
    let mut table = abi::host_table();
    table.commands_submit = submit;
    let mut calls = Cell::new(0);
    let host = host(&table, &mut calls);
    let regular = "a".repeat(31);
    let longer = "a".repeat(32);
    let mut items = [regular.as_str(); 64];
    items[0] = &longer;
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let (result, count) = track(false, || sender.set_menu("core.main", Some(&items)));
    assert_eq!(result, Ok(()));
    assert_eq!(count.peak, wire::COMMAND_SET_MENU_ITEMS_MAX_BYTES);
    assert_eq!(count.live, 0);
    assert_eq!(calls.get(), 1);
    let (result, count) = track(true, || sender.set_menu("core.main", Some(&items)));
    assert_eq!(result, Err(Error::NoMemory));
    assert!(count.refused);
    assert_eq!(count.live, 0);
    assert_eq!(calls.get(), 1);
}

#[test]
fn empty_reset_and_invalid_menu_lists_allocate_nothing() {
    let mut table = abi::host_table();
    table.commands_submit = submit;
    let mut calls = Cell::new(0);
    let host = host(&table, &mut calls);
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut sender = CommandSender::new(&host, &mut scratch);
    for items in [None, Some(&[][..])] {
        let (result, count) = track(true, || sender.set_menu("core.main", items));
        assert_eq!(result, Ok(()));
        assert_eq!(count.peak, 0);
        assert!(!count.refused);
    }
    let (result, count) = track(true, || sender.set_menu("core.main", Some(&[""])));
    assert_eq!(result, Err(Error::Invalid));
    assert_eq!(count.peak, 0);
    assert!(!count.refused);
    assert_eq!(calls.get(), 2);
}

unsafe extern "C" fn service_open(
    ctx: *mut c_void,
    _: PpBytes,
    _: u16,
    _: u16,
    handle: *mut u32,
) -> i32 {
    // SAFETY: the fixture retains the counter and aligned exclusive handle output.
    let calls = unsafe { &*ctx.cast::<Cell<usize>>() };
    calls.set(calls.get() + 1);
    unsafe { handle.write(7) };
    abi::OK
}
unsafe extern "C" fn service_call(
    ctx: *mut c_void,
    _: u32,
    _: u32,
    _: PpBytes,
    out: abi::PpBuffer,
) -> i32 {
    assert_eq!(
        COUNTS.with(|cell| cell.get().live),
        0,
        "handover temporary must be freed before host call"
    );
    // SAFETY: the fixture keeps the counter and exclusive output buffer live.
    let calls = unsafe { &*ctx.cast::<Cell<usize>>() };
    calls.set(calls.get() + 1);
    let output = unsafe { core::slice::from_raw_parts_mut(out.ptr, out.cap) };
    use crate::native::declare::records_wire as records;
    i32::try_from(
        records::record_encode_one(
            &records::Record::ServiceUiHandle(records::ServiceUiHandle { handle: 1 }),
            output,
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn menu_and_handover_temporaries_do_not_overlap_in_one_callback() {
    use crate::native::ui::{Param, Ui};
    let mut table = abi::host_table();
    table.commands_submit = submit;
    table.service_open = service_open;
    table.service_call = service_call;
    let mut calls = Cell::new(0);
    let host = host(&table, &mut calls);
    let ui = Ui::open(&host).unwrap();
    let keys: alloc::vec::Vec<_> = (0..32).map(|i| alloc::format!("p{i}")).collect();
    let params: alloc::vec::Vec<_> = keys
        .iter()
        .map(|key| Param::new(key, "").unwrap())
        .collect();
    let item = "a".repeat(31);
    let longer = "a".repeat(32);
    let mut items = [item.as_str(); 64];
    items[0] = &longer;
    let mut commands = [0; wire::COMMAND_MAX_BYTES];
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    let (result, count) = track(false, || {
        CommandSender::new(&host, &mut commands).set_menu("core.main", Some(&items))?;
        ui.handover(&host, &params, &mut input, &mut out)?;
        Ok::<_, Error>(())
    });
    assert_eq!(result, Ok(()));
    // The handover encodes its params as it walks them: no temporary at all.
    assert_eq!(count.peak, 2048);
    assert_eq!(count.live, 0);
    assert_eq!(calls.get(), 3);
}
