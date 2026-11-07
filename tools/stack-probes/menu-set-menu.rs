#![no_std]
#![no_main]
use core::alloc::{GlobalAlloc, Layout};
use paperpaw_sdk::native::{Host, commands::CommandSender};
core::arch::global_asm!(
    ".global probe_allocate",
    "probe_allocate:",
    "li a0, 0",
    "ret",
    ".global probe_free",
    "probe_free:",
    "ret"
);
unsafe extern "C" {
    fn probe_allocate(size: usize, align: usize) -> *mut u8;
    fn probe_free(ptr: *mut u8, size: usize, align: usize);
}
struct Heap;
unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { probe_allocate(layout.size(), layout.align()) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { probe_free(ptr, layout.size(), layout.align()) }
    }
}
#[global_allocator]
static HEAP: Heap = Heap;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn menu_stack_probe(host: &Host<'_>, scratch: &mut [u8], items: &[&str]) -> i32 {
    match CommandSender::new(host, scratch).set_menu("core.main", Some(items)) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    core::hint::black_box(menu_stack_probe as *const ());
    loop {
        core::hint::spin_loop();
    }
}
