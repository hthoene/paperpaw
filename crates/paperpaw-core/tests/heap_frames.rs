#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! The frame owner is per thread (`abi/heap-word.md` section 4): plugin code
//! of several threads of one process (desktop and test binaries) allocates
//! through the `PpHeap` table without seeing each other's frames.
use paperpaw_core::native_host::resources::HostResources;
use paperpaw_core::native_host::shell::{DeclaredServices, Hosted, handed_heap};
use paperpaw_native::host_glue::{OK, PpHost, PpPlugin, Services};
use std::alloc::Layout;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Barrier};

struct Heap;

impl Services for Heap {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        Layout::from_size_align(size, align)
            // SAFETY: a valid, non-empty layout, as the caller promises.
            .map_or(ptr::null_mut(), |layout| unsafe {
                std::alloc::alloc(layout)
            })
    }
    unsafe fn dealloc(&self, block: *mut u8, size: usize, align: usize) {
        // SAFETY: the trait's contract: this allocator's block, freed once.
        unsafe { std::alloc::dealloc(block, Layout::from_size_align(size, align).unwrap()) };
    }
    unsafe fn realloc(&self, block: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: as `dealloc`.
        unsafe { std::alloc::realloc(block, Layout::from_size_align(old, align).unwrap(), new) }
    }
    fn micros(&self) -> u64 {
        0
    }
}

/// A plugin table with a destructor and no callbacks.
#[repr(C)]
struct State {
    table: PpPlugin,
}

/// Whether the table allocated inside the plugin's own code (`make`) and
/// whether it is refused after `make` returned.
unsafe extern "C" fn make(_: *const PpHost, _: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    let heap = handed_heap::<Heap>();
    // SAFETY: the table's own entries, a block given back with its layout.
    unsafe {
        let block = (heap.alloc.unwrap())(64, 16);
        if block.is_null() {
            return -1;
        }
        (heap.dealloc.unwrap())(block, 64, 16);
    }
    let state = Box::into_raw(Box::new(State {
        table: PpPlugin {
            size: std::mem::offset_of!(PpPlugin, dispatch_event) as u32,
            state: ptr::null_mut(),
            drop: Some(destroy),
            dispatch_event: None,
            draw: None,
            input: None,
        },
    }));
    // SAFETY: a fresh allocation and a writable output.
    unsafe {
        (*state).table.state = state.cast();
        out.write(&raw const (*state).table);
    }
    OK
}

unsafe extern "C" fn destroy(state: *mut c_void) {
    // SAFETY: the `State` that `make` boxed, destroyed once.
    drop(unsafe { Box::from_raw(state.cast::<State>()) });
}

#[test]
fn threads_hosting_plugins_side_by_side_each_allocate_in_their_own_frame() {
    const THREADS: usize = 4;
    let barrier = Arc::new(Barrier::new(THREADS));
    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..200 {
                    // SAFETY: `make` is this file's plugin, always callable.
                    let hosted = unsafe {
                        Hosted::new(
                            Heap,
                            (),
                            make,
                            "frames",
                            (ptr::null(), 0),
                            |_, _, _, _| {},
                            HostResources::try_new().unwrap(),
                            DeclaredServices::none(),
                        )
                    }
                    .unwrap();
                    let batch = hosted
                        .construct_registered(hosted.begin().unwrap())
                        .expect("make allocated inside its frame");
                    // Outside the frame again: no heap for the table.
                    // SAFETY: nothing is allocated.
                    let outside = unsafe { (handed_heap::<Heap>().alloc.unwrap())(8, 8) };
                    assert!(outside.is_null());
                    drop(batch);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}
