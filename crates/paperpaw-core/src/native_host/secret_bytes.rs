//! Non-elidable cleanup for host buffers that may carry secrets.
/// Wipe private host scratch before its allocation can be reused.
pub(crate) fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        // SAFETY: each byte is live and exclusively borrowed. Volatile stores
        // retain the wipe even when the allocation is immediately freed.
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}
