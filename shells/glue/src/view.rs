//! What the shell tells `core.work` and the catalog.

use std::cell::Cell;

use paperpaw_core::native_host::shell_wiring::{CatalogEnv, WorkDevice};

/// What the device tells `core.work` (`WorkDevice`): the kernel's clock as the
/// device last read it and whether the network is up. The device writes both
/// where it reads its clock ([`DeviceView::set`]), so the scheduler and the
/// kernel's tick agree.
#[derive(Debug, Default)]
pub struct DeviceView {
    now_ms: Cell<u64>,
    online: Cell<bool>,
}

impl DeviceView {
    /// The clock and the network as they are now.
    pub fn set(&self, now_ms: u64, online: bool) {
        self.now_ms.set(now_ms);
        self.online.set(online);
    }
}

impl WorkDevice for DeviceView {
    fn now_ms(&self) -> u64 {
        self.now_ms.get()
    }
    fn network_up(&self) -> bool {
        self.online.get()
    }
    /// The pretend radio is on when the network is up.
    fn radio_on(&self) -> bool {
        self.online.get()
    }
}

/// What the card cannot tell the catalog reads, on the desktop: no walk
/// shown, today's date from the computer's clock (none on simulated time, so
/// no book is new in a run that must not depend on the day).
pub struct Catalog {
    /// A run on simulated time (tests, scripts).
    pub simulated: bool,
}

impl CatalogEnv for Catalog {
    fn walking(&self) -> Option<u32> {
        None
    }
    fn today(&self) -> Option<u32> {
        // A browser's `SystemTime` cannot be read: no day there.
        if self.simulated || cfg!(target_arch = "wasm32") {
            return None;
        }
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        u32::try_from(seconds / 86_400).ok()
    }
    /// The wall clock in milliseconds: the idle time of an annotation
    /// transaction and the life of its receipt. A simulated clock is none.
    fn now_ms(&self) -> u64 {
        if self.simulated {
            return 0;
        }
        // A browser's `SystemTime` cannot be read: the shell's clock is the
        // page's (no wall clock there).
        if cfg!(target_arch = "wasm32") {
            return crate::clock::micros() / 1000;
        }
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(0))
    }
}
