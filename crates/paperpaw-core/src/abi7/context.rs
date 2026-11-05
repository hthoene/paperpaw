//! Scoped core views inside a stable host-owned context.
//!
//! The owner context may outlive an event's Env or Canvas. It must not retain
//! their Rust references. This slot admits a frame only for one synchronous
//! dispatch, clears it on every exit, and refuses nested installation.

use core::{cell::Cell, marker::PhantomPinned, pin::Pin, ptr::NonNull};
use paperpaw_i18n::Translator;
use paperpaw_sdk::Settings;

pub(crate) trait Frame {
    fn settings(&self) -> &Settings;
    fn translator(&self) -> &Translator;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Busy,
    NoFrame,
}

pub(crate) struct FrameSlot {
    current: Cell<Option<NonNull<dyn Frame>>>,
    _pin: PhantomPinned,
}

struct Reset<'a>(&'a FrameSlot);

impl Drop for Reset<'_> {
    fn drop(&mut self) {
        self.0.current.set(None);
    }
}

impl FrameSlot {
    pub(crate) const fn new() -> Self {
        Self {
            current: Cell::new(None),
            _pin: PhantomPinned,
        }
    }

    /// The closure is synchronous. Neither it nor the raw ABI may keep a frame
    /// view after it returns. Pinning also keeps a surrounding owner context's
    /// address stable while foreign code holds its pointer.
    pub(crate) fn scope<R>(
        self: Pin<&Self>,
        frame: &dyn Frame,
        call: impl FnOnce() -> R,
    ) -> Result<R, Error> {
        if self.current.get().is_some() {
            return Err(Error::Busy);
        }
        let borrowed = NonNull::from(frame);
        // SAFETY: the erased lifetime is restored only inside `with`. The
        // Reset guard clears the pointer before this borrow ends, including
        // unwinding. Nested scope replacement is refused and Cell prevents
        // cross-thread sharing. No pointer or borrowed frame is returned.
        let erased = unsafe {
            core::mem::transmute::<NonNull<dyn Frame + '_>, NonNull<dyn Frame + 'static>>(borrowed)
        };
        self.current.set(Some(erased));
        let _reset = Reset(self.get_ref());
        Ok(call())
    }

    pub(crate) fn with<R>(&self, read: impl FnOnce(&dyn Frame) -> R) -> Result<R, Error> {
        let pointer = self.current.get().ok_or(Error::NoFrame)?;
        // SAFETY: a present pointer belongs to a live scope. Its shared frame
        // reference is limited to `read`; R cannot borrow that reference.
        Ok(read(unsafe { pointer.as_ref() }))
    }
}
