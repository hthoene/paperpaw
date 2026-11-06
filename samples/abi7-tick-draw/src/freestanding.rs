//! Symbols a freestanding cdylib must define itself, so that loading it
//! needs nothing from the host process but the PaperPaw host table.
//!
//! Without them the linked library imports `rust_eh_personality` (core is
//! built with unwinding tables even under `-Cpanic=abort`) and `memcpy`
//! (lowered from struct copies), and a real `dlopen` refuses it.
//! `verify.sh` checks that the artifact has no undefined symbol.

use core::ffi::c_void;

/// Never called: with `panic = "abort"` no frame is unwound. The symbol only
/// satisfies the reference from core's unwinding tables.
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}

/// Byte copy for non-overlapping regions.
///
/// The writes are volatile so that the optimizer cannot recognize the loop
/// and turn it back into a call to `memcpy`, which would recurse.
///
/// # Safety
///
/// `dst` and `src` must be valid for `n` bytes and must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let (to, from) = (dst.cast::<u8>(), src.cast::<u8>());
    let mut i = 0;
    while i < n {
        // SAFETY: the caller guarantees both regions are valid for `n` bytes.
        unsafe { to.add(i).write_volatile(from.add(i).read()) };
        i += 1;
    }
    dst
}
