//! The device's one callback-depth counter (`FlatDepth`).
//!
//! Every interface-7 plugin of a device brings the same counter: a host stage
//! that runs a plugin enters it, so a second counter would let nested dispatch
//! past the budget, and the kernel's install check refuses a plugin that brings
//! a different one (`a native plugin brought its own callback depth counter`).
//! The X4 has two ways to get plugins in: the loader of the card's files and
//! the probe of the plugins linked into the firmware. Both take their counter
//! from one [`DeviceDepth`], which makes it at the first use and hands out
//! counted shares of that one.

use core::cell::RefCell;

use paperpaw_core::native_host::flat::FlatDepth;

/// The one callback-depth counter of the device, made at the first use.
#[derive(Debug, Default)]
pub struct DeviceDepth(RefCell<Option<FlatDepth>>);

impl DeviceDepth {
    /// An empty holder: nothing is allocated until [`share`](Self::share).
    #[must_use]
    pub const fn new() -> Self {
        Self(RefCell::new(None))
    }

    /// A counted share of the device's counter, made if it is not there yet.
    /// `None` where it could not be allocated, or where the holder is in use
    /// (never a panic).
    #[must_use]
    pub fn share(&self) -> Option<FlatDepth> {
        let mut held = self.0.try_borrow_mut().ok()?;
        if held.is_none() {
            *held = FlatDepth::try_new().ok();
        }
        held.as_ref()?.try_clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two takers (the card's loader and the linked probe) hold the same
    /// counter, not two.
    #[test]
    fn every_share_is_the_same_counter() {
        let depth = DeviceDepth::new();
        let loader = depth.share().unwrap();
        let probe = depth.share().unwrap();
        assert!(core::ptr::eq(loader.as_depth(), probe.as_depth()));
        let later = depth.share().unwrap();
        assert!(core::ptr::eq(loader.as_depth(), later.as_depth()));
    }

    /// A holder of another device has its own counter.
    #[test]
    fn another_holder_has_another_counter() {
        let one = DeviceDepth::new().share().unwrap();
        let two = DeviceDepth::new().share().unwrap();
        assert!(!core::ptr::eq(one.as_depth(), two.as_depth()));
    }

    /// The holder outlives a share: the counter stays the device's.
    #[test]
    fn a_dropped_share_leaves_the_counter() {
        let depth = DeviceDepth::new();
        let first = depth.share().unwrap();
        let address = core::ptr::from_ref(first.as_depth());
        drop(first);
        let second = depth.share().unwrap();
        assert!(core::ptr::eq(address, second.as_depth()));
    }
}
