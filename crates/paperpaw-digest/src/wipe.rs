//! Clearing what a digest held of what it hashed (a password for
//! KOReader's sync key, a secret for Calibre): its state when it goes, and
//! the block copies and message words each compression makes. Not
//! covered: registers and spilled locals of the rounds, and the digest
//! the caller gets.

/// Writes `zero` over `value` so that the store is not left out as dead.
pub(crate) fn wipe<T: Copy>(value: &mut T, zero: T) {
    // SAFETY: `value` is live, aligned and exclusively borrowed; a volatile
    // store of a `Copy` value over it is a plain write the compiler keeps.
    unsafe { core::ptr::write_volatile(value, zero) };
}

/// Keeps the wipes before what follows (the memory's reuse).
pub(crate) fn fence() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// `Drop` for a digest state of `words` state words: its state, block,
/// fill and length wiped.
macro_rules! wiped_on_drop {
    ($digest:ty, $words:literal) => {
        impl $digest {
            /// Every field zero, kept by the compiler.
            fn wipe(&mut self) {
                crate::wipe::wipe(&mut self.state, [0; $words]);
                crate::wipe::wipe(&mut self.block, [0; 64]);
                crate::wipe::wipe(&mut self.used, 0);
                crate::wipe::wipe(&mut self.length, 0);
                crate::wipe::fence();
            }
        }

        /// The state is wiped when it goes, also after `finish`, `hex` and
        /// their kind, which take it (see the module for what is not).
        impl Drop for $digest {
            fn drop(&mut self) {
                self.wipe();
            }
        }
    };
}
pub(crate) use wiped_on_drop;
