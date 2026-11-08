#![no_std]
#![no_main]
use core::alloc::{GlobalAlloc, Layout};
use paperpaw_sdk::native::{
    Host,
    files::{Files, INPUT_BYTES, OUTPUT_BYTES},
    storage::Root,
};
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
type Bufs<'a> = (&'a mut [u8; INPUT_BYTES], &'a mut [u8; OUTPUT_BYTES]);
fn code<T>(r: Result<T, paperpaw_sdk::native::Error>) -> i32 {
    if r.is_ok() { 0 } else { -1 }
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn files_list_probe(host: &Host<'_>, files: &Files, cursor: u64, b: Bufs<'_>) -> i32 {
    code(files.list(host, Root::Files, "a", cursor, b.0, b.1).map(|p| p.len()))
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn files_stat_probe(host: &Host<'_>, files: &Files, b: Bufs<'_>) -> i32 {
    code(files.stat(host, Root::Files, "a", b.0, b.1).map(|e| e.size()))
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn files_space_probe(host: &Host<'_>, files: &Files, b: Bufs<'_>) -> i32 {
    code(files.space(host, Root::Files, b.0, b.1).map(|s| s.free()))
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn files_make_dir_probe(host: &Host<'_>, files: &Files, b: Bufs<'_>) -> i32 {
    code(files.make_dir(host, Root::Data, "a", b.0, b.1))
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn files_remove_step_probe(host: &Host<'_>, files: &Files, b: Bufs<'_>) -> i32 {
    code(files.remove_step(host, Root::Data, "a", 0, b.0, b.1).map(|p| p.next()))
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn files_remove_tree_probe(host: &Host<'_>, files: &Files, b: Bufs<'_>) -> i32 {
    code(files.remove_tree(host, Root::Data, "a", b.0, b.1))
}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn files_move_probe(host: &Host<'_>, files: &Files, b: Bufs<'_>) -> i32 {
    code(files.move_entry(host, Root::Data, "a", Root::Files, "b", true, b.0, b.1))
}
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    core::hint::black_box(files_list_probe as *const ());
    core::hint::black_box(files_stat_probe as *const ());
    core::hint::black_box(files_space_probe as *const ());
    core::hint::black_box(files_make_dir_probe as *const ());
    core::hint::black_box(files_remove_step_probe as *const ());
    core::hint::black_box(files_remove_tree_probe as *const ());
    core::hint::black_box(files_move_probe as *const ());
    loop {
        core::hint::spin_loop();
    }
}
