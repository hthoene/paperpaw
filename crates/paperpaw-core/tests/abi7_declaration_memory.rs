//! Requested allocator bytes while S1 decoded data and raw snapshots coexist.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
#![allow(unsafe_code, reason = "test-only allocator counts the current thread")]
use paperpaw_core::native_host::{
    HostOwner,
    registration_scope::RegistrationScope,
    registrations::{Record, Registration, records_decode, submit},
};
use paperpaw_native::host_glue::{ERR_NO_SPACE, OK};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    pin::pin,
};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
    static FAIL_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
}
fn change(add: usize, remove: usize) {
    if ACTIVE.get() {
        let live = LIVE.get() + add - remove;
        LIVE.set(live);
        PEAK.set(PEAK.get().max(live));
    }
}
fn refuse_allocation() -> bool {
    if !ACTIVE.get() {
        return false;
    }
    match FAIL_AFTER.get() {
        None => false,
        Some(0) => true,
        Some(left) => {
            FAIL_AFTER.set(Some(left - 1));
            false
        }
    }
}
struct Counting;
// SAFETY: every allocation/deallocation is forwarded unchanged to System.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse_allocation() {
            return std::ptr::null_mut();
        }
        // SAFETY: caller supplies the GlobalAlloc contract.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            change(layout.size(), 0);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        change(0, layout.size());
        // SAFETY: forwarded pointer and original allocation layout.
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse_allocation() {
            return std::ptr::null_mut();
        }
        // SAFETY: forwarded allocation and requested replacement size.
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            change(size, layout.size());
        }
        replacement
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn measure(plugin: &str, golden: &[u8], image: bool) {
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    LIVE.set(0);
    PEAK.set(0);
    let started = std::time::Instant::now();
    ACTIVE.set(true);
    let mut construction = owner.as_ref().begin::<Registration>(plugin).unwrap();
    if image {
        construction.set_image(golden).unwrap();
    }
    let mut offset = 0;
    let mut batches = 0;
    let mut specs = 0;
    let mut accepted = 0;
    let mut accepted_specs = 0;
    let mut wire_bytes = 0;
    let mut status = OK;
    while offset < golden.len() {
        let len = u32::from_le_bytes(golden[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let wire = &golden[offset..offset + len];
        offset += len;
        let count = records_decode(wire)
            .unwrap()
            .iter()
            .filter(|r| matches!(r, Record::SettingSpec(_)))
            .count();
        specs += count;
        batches += 1;
        wire_bytes += len;
        if status == OK {
            status = scope
                .as_ref()
                .scope(&mut construction, || {
                    // SAFETY: golden is immutable, initialized static storage.
                    unsafe { submit(&scope, wire.as_ptr(), wire.len()) }
                })
                .unwrap();
            if status == OK {
                accepted += 1;
                accepted_specs += count;
            }
        }
    }
    let decoded = construction.declarations().bytes();
    let resident = LIVE.get();
    // A failure drops both representations; success transfers and then drops them.
    let committed = construction.commit(paperpaw_core::native_host::Teardown {
        cancel: |_| {},
        release: |_| {},
        destroy: |_| {},
    });
    let unavailable = matches!(
        committed,
        Err(paperpaw_core::native_host::Error::DeclarationViewsUnavailable)
    );
    drop(committed);
    let elapsed = started.elapsed();
    let peak = PEAK.get();
    let remaining = LIVE.get();
    ACTIVE.set(false);
    assert_eq!(remaining, 0);
    assert_eq!(unavailable, image);
    assert!(peak <= 256 * 1024);
    if plugin == "battery" || image {
        assert_eq!(status, OK);
    } else {
        assert_eq!(status, ERR_NO_SPACE);
        assert_eq!((batches, specs), (50, 330));
    }
    println!(
        "{plugin} image={image} elapsed_us={}: requested={batches} batches/{specs} specs/{wire_bytes} wire bytes; admitted={accepted} batches/{accepted_specs} specs; status={status}; S1 logical={decoded}; resident={resident}; peak={peak}; full-wire two-copies baseline={}",
        elapsed.as_micros(),
        wire_bytes * 2
    );
}
#[test]
fn battery_and_widgets_snapshot_peak() {
    measure(
        "battery",
        include_bytes!("../../../plugins/battery/testdata/declarations.bin.v1"),
        false,
    );
    measure(
        "widgets",
        include_bytes!("../../../plugins/widgets/testdata/declarations.bin.v1"),
        false,
    );
    measure(
        "widgets",
        include_bytes!("../../../plugins/widgets/testdata/declarations.bin.v1"),
        true,
    );
}

#[test]
fn every_construction_allocation_refusal_poisons_and_rolls_back_raw_and_decoded_data() {
    let golden = include_bytes!("../../../plugins/battery/testdata/declarations.bin.v1");
    let len = u32::from_le_bytes(golden[..4].try_into().unwrap()) as usize;
    let bytes = &golden[4..4 + len];
    let mut reached_success = false;
    for fail_after in 0..128 {
        let owner = pin!(HostOwner::new((), ()));
        let scope = pin!(RegistrationScope::<Registration>::new());
        let mut construction = owner.as_ref().begin::<Registration>("battery").unwrap();
        LIVE.set(0);
        PEAK.set(0);
        FAIL_AFTER.set(Some(fail_after));
        ACTIVE.set(true);
        let status = scope
            .as_ref()
            .scope(&mut construction, || {
                // SAFETY: immutable golden storage stays live throughout submit.
                unsafe { submit(&scope, bytes.as_ptr(), bytes.len()) }
            })
            .unwrap();
        FAIL_AFTER.set(None);
        let committed = construction.commit(paperpaw_core::native_host::Teardown {
            cancel: |_| {},
            release: |_| {},
            destroy: |_| {},
        });
        let success = committed.is_ok();
        drop(committed);
        let remaining = LIVE.get();
        ACTIVE.set(false);
        assert_eq!(remaining, 0);
        assert_eq!(success, status == OK);
        if success {
            reached_success = true;
            break;
        }
        assert_eq!(status, paperpaw_native::host_glue::ERR_NO_MEMORY);
    }
    assert!(reached_success);
}
