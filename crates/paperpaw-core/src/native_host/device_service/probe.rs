//! The memory probe's cache (`device.md` 2, op 7): a probe walks the stack on
//! the X4, so the host does one real probe per 250 ms and answers the cached
//! one inside that window.
use core::cell::Cell;

/// The window in which a probe's answer is reused, in milliseconds.
pub const PROBE_WINDOW_MS: u64 = 250;

/// One cached probe result with the time it was taken. The clock is injected
/// (`now_ms`, a monotonic millisecond count), so the window is testable.
///
/// A probe that read nothing (`None`) is cached for the window as well: the
/// cost is the walk, not its outcome. A clock that runs backwards counts as
/// outside the window (the next call probes again).
///
/// Memory: `T` plus 16 bytes; for the memory probe 32 bytes in all.
#[derive(Debug)]
pub struct ProbeCache<T: Copy> {
    last: Cell<Option<(u64, Option<T>)>>,
}

impl<T: Copy> Default for ProbeCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy> ProbeCache<T> {
    /// An empty cache: the first call probes.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: Cell::new(None),
        }
    }

    /// The probe's result and whether it is a cached one. `probe` runs only
    /// when the last real probe is [`PROBE_WINDOW_MS`] old or older (or none).
    pub fn get(&self, now_ms: u64, probe: impl FnOnce() -> Option<T>) -> (Option<T>, bool) {
        if let Some((at, value)) = self.last.get()
            && now_ms >= at
            && now_ms - at < PROBE_WINDOW_MS
        {
            return (value, true);
        }
        let value = probe();
        self.last.set(Some((now_ms, value)));
        (value, false)
    }
}
