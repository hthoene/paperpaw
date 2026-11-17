//! What the device did since PaperPaw started, as the kernel counts it
//! (`Env::usage`): for plugins that relate the
//! battery's use to it. Refreshing the screen takes much of an e-reader's
//! power; the processor being awake much of the rest.

use paperpaw_platform::{Rect, RefreshMode};

use crate::id::Id;

/// Refreshes of the screen, by mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Refreshes {
    pub fast: u32,
    /// Partial ones, and modes to come between fast and full.
    pub partial: u32,
    pub full: u32,
}

impl Refreshes {
    pub const fn new(fast: u32, partial: u32, full: u32) -> Self {
        Self {
            fast,
            partial,
            full,
        }
    }

    /// Refreshes of every mode together.
    pub fn total(&self) -> u32 {
        self.fast
            .saturating_add(self.partial)
            .saturating_add(self.full)
    }

    /// Counts a refresh in `mode`.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn count(&mut self, mode: RefreshMode) {
        let count = match mode {
            RefreshMode::Fast => &mut self.fast,
            RefreshMode::Full => &mut self.full,
            _ => &mut self.partial,
        };
        *count = count.saturating_add(1);
    }
}

/// Counts since PaperPaw started (on a device whose sleep cuts the power,
/// since it last woke).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Usage {
    /// Refreshes of the screen asked for, by mode.
    pub refreshes: Refreshes,
    /// Pixels refreshed, all refreshes together.
    pub pixels: u64,
    /// Button presses and releases, taps and swipes handled.
    pub inputs: u32,
    /// Times the device went to sleep (or the app to the background).
    pub sleeps: u32,
    /// Time the panel took for its refreshes, where the shell says
    /// (the kernel's `panel_busy`).
    pub panel_ms: u64,
    /// Time the Bluetooth radio was on.
    pub bluetooth_ms: u64,
}

/// What a plugin took (`Env::plugin_usage`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginUsage {
    pub plugin: Id,
    /// Time its code ran (its screens drawing and handling input, its
    /// handlers), in microseconds, where the device has a clock for it.
    pub busy_us: u64,
    /// Times its code was called.
    pub calls: u32,
    /// Time one of its screens was on top, in milliseconds (counts stop
    /// at their largest value; a row is kept for every plugin, so they
    /// are as narrow as they can be).
    pub shown_ms: u32,
    /// Refreshes while one of its screens was on top.
    pub refreshes: Refreshes,
    /// Pixels refreshed then.
    pub pixels: u64,
    /// Bytes of storage its code read and wrote.
    pub read: u32,
    pub written: u32,
    /// Its requests, servers and connections that went out to the network,
    /// and those refused (Offline, its network off, not declared; see
    /// [`work`](crate::work)).
    pub requests: u32,
    pub refused: u32,
    /// Time its holds kept the device from going to sleep by itself
    /// ([`WakeHold`](crate::power::WakeHold)), in milliseconds, and the
    /// reason of its last hold.
    pub awake_ms: u32,
    pub awake_reason: alloc::borrow::Cow<'static, str>,
}

impl Default for PluginUsage {
    fn default() -> Self {
        Self::new(Id::new_static("core"))
    }
}

impl PluginUsage {
    /// Nothing taken yet by `plugin`.
    pub fn new(plugin: Id) -> Self {
        Self {
            plugin,
            busy_us: 0,
            calls: 0,
            shown_ms: 0,
            refreshes: Refreshes::default(),
            pixels: 0,
            read: 0,
            written: 0,
            requests: 0,
            refused: 0,
            awake_ms: 0,
            awake_reason: alloc::borrow::Cow::Borrowed(""),
        }
    }
}

impl Usage {
    /// Counts a refresh of `area` in `mode`.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn refreshed(&mut self, mode: RefreshMode, area: Rect) {
        self.refreshes.count(mode);
        self.pixels = self
            .pixels
            .saturating_add(u64::from(area.width) * u64::from(area.height));
    }

    /// Refreshes of every mode together.
    pub fn total_refreshes(&self) -> u32 {
        self.refreshes.total()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_counted_by_mode_and_area() {
        let mut usage = Usage::default();
        usage.refreshed(RefreshMode::Fast, Rect::new(0, 0, 10, 10));
        usage.refreshed(RefreshMode::Full, Rect::new(5, 5, 480, 800));
        usage.refreshed(RefreshMode::Full, Rect::new(0, 0, 1, 1));
        assert_eq!(usage.refreshes, Refreshes::new(1, 0, 2));
        assert_eq!(usage.pixels, 100 + 480 * 800 + 1);
        assert_eq!(usage.total_refreshes(), 3);
    }
}
