//! SDK entry regression: both export symbols must build and pack for the X4.
#![no_std]
#![no_main]

use core::alloc::{GlobalAlloc, Layout};
use paperpaw_sdk::native::{Error, Host, Plugin, clock};

struct Example;
impl Plugin for Example {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        clock::micros(host)?;
        Ok(Self)
    }
}
// `heap = none`: no heap word, allocator or panic handler from the SDK; the
// sample brings its own below.
paperpaw_sdk::native_plugin!(
    Example,
    metadata = include_bytes!("../metadata.bin"),
    heap = none
);

// The sample allocates no plugin-owned data. Entry state uses the host allocator
// directly; a null allocator catches accidental use of the plugin's own heap.
struct NoHeap;
// SAFETY: allocations always fail; no non-null allocation can be deallocated.
unsafe impl GlobalAlloc for NoHeap {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static HEAP: NoHeap = NoHeap;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
