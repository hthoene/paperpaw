//! Symbols a freestanding cdylib must define itself, so that loading it
//! needs nothing from the host process but the PaperPaw host table.
//!
//! Without them the linked library imports `rust_eh_personality` (core is
//! built with unwinding tables even under `-Cpanic=abort`) and `memcpy`
//! (lowered from struct copies), and a real `dlopen` refuses it. They are
//! defined here as *hidden* symbols: the linker resolves them inside the
//! library, and they never reach the dynamic symbol table, which holds only
//! `PAPERPAW_ENTRY` and `PAPERPAW_METADATA`. `verify.sh` checks the library:
//! no undefined symbol, no needed library, exactly those two exports.

use core::ffi::c_void;

/// Never called: with `panic = "abort"` no frame is unwound, and nothing in
/// this library catches a panic. The symbol only satisfies the reference from
/// core's unwinding tables, which name `rust_eh_personality` even under
/// `-Cpanic=abort`.
///
/// It deliberately does nothing. A real personality routine tells the unwinder
/// whether a frame has a handler; this one would claim "no handler, keep going"
/// to an unwinder that never runs here. If a future change enabled unwinding in
/// this library, this stub would have to be replaced, not kept, which is why
/// `Cargo.toml` and `verify.sh` pin `panic = "abort"`.
extern "C" fn eh_personality() {}

/// Byte copy for non-overlapping regions.
///
/// The writes are volatile so that the optimizer cannot recognize the loop
/// and turn it back into a call to `memcpy`, which would recurse. The function
/// stays a symbol of its own (`inline(never)`) so that `check_artifact.py`
/// can find and test it.
///
/// # Safety
///
/// `dst` and `src` must be valid for `n` bytes and must not overlap.
#[inline(never)]
unsafe extern "C" fn copy_bytes(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (to, from) = (dst.cast::<u8>(), src.cast::<u8>());
    let mut i = 0;
    while i < n {
        // SAFETY: the caller guarantees both regions are valid for `n` bytes.
        unsafe { to.add(i).write_volatile(from.add(i).read()) };
        i += 1;
    }
    dst
}

// x86-64 only, which `build.rs` enforces. The jumps keep the arguments in
// place, so the C calling convention passes straight through.
core::arch::global_asm!(
    ".globl rust_eh_personality",
    ".hidden rust_eh_personality",
    ".type rust_eh_personality,@function",
    "rust_eh_personality: jmp {eh}",
    ".globl memcpy",
    ".hidden memcpy",
    ".type memcpy,@function",
    "memcpy: jmp {copy}",
    eh = sym eh_personality,
    copy = sym copy_bytes,
);
