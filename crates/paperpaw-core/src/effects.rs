//! What the shell has to do after a kernel call ([`Effects`]).

use paperpaw_platform::{Instant, Rect, RefreshMode};

#[cfg(doc)]
use crate::{Kernel, KernelBuilder};
#[cfg(doc)]
use paperpaw_sdk::Screen;

/// Something the shell has to do after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Effect {
    /// Pass `region` on to the panel with [`Kernel::render`] and refresh it.
    Refresh { region: Rect, mode: RefreshMode },
    /// Call [`Kernel::tick`] at (or soon after) this time.
    WakeAt(Instant),
    /// Build a new kernel, e.g. to apply switched plugins.
    Restart,
    /// Sleep once the frame is shown (the sleep screen): call
    /// [`Kernel::suspend`], and [`Kernel::resume`] on waking.
    Sleep,
    /// Switch the device off once the frame is shown: call
    /// [`Kernel::suspend`] first. The next start is a cold one.
    PowerOff,
    /// The frame is best shown in grays (see [`Effects::grays`]).
    Grays,
    /// Call [`Kernel::proceed`] while the panel shows the busy mark (see
    /// [`Effects::proceed`]).
    Proceed,
}

/// The effects of one kernel call. Refreshes are already coalesced: there
/// is at most one per call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effects {
    pub(crate) refresh: Option<(Rect, RefreshMode)>,
    pub(crate) grays: bool,
    pub(crate) wake_at: Option<Instant>,
    pub(crate) restart: bool,
    pub(crate) sleep: bool,
    pub(crate) power_off: bool,
    pub(crate) proceed: bool,
    pub(crate) pace: Option<core::time::Duration>,
}

impl Effects {
    /// Whether the kernel showed the busy mark and holds the work back:
    /// the shell starts showing the mark (`refresh`), then calls
    /// [`Kernel::proceed`] while the panel refreshes (see
    /// [`KernelBuilder::busy_mark`]).
    pub fn proceed(&self) -> bool {
        self.proceed
    }

    pub fn refresh(&self) -> Option<(Rect, RefreshMode)> {
        self.refresh
    }

    /// Whether the frame is best shown in grays (the screen on top, with
    /// nothing over it, says so: [`Screen::grays`]):
    /// shells whose panel shows more grays with a refresh of its own draw
    /// the whole screen with [`Kernel::render_gray`] and refresh it so;
    /// others refresh as [`Effects::refresh`] says.
    pub fn grays(&self) -> bool {
        self.grays
    }

    pub fn wake_at(&self) -> Option<Instant> {
        self.wake_at
    }

    /// The pace the screen on top runs at, if it asks for one
    /// ([`ScreenMode::frame_every`](paperpaw_sdk::realtime::ScreenMode::frame_every)):
    /// the shell keeps ready for it (no light sleep longer than a frame)
    /// and, where it can, goes on while the panel refreshes, showing the
    /// next frame once the panel is free.
    pub fn pace(&self) -> Option<core::time::Duration> {
        self.pace
    }

    /// Whether the shell should build a new kernel (see [`Effect::Restart`]).
    pub fn restart(&self) -> bool {
        self.restart
    }

    /// Whether the shell should sleep once the frame is shown (see
    /// [`Effect::Sleep`]).
    pub fn sleep(&self) -> bool {
        self.sleep
    }

    /// Whether the shell should switch the device off once the frame is
    /// shown (see [`Effect::PowerOff`]).
    pub fn power_off(&self) -> bool {
        self.power_off
    }

    /// What these and the `later` effects ask together, for a shell that
    /// shows one frame for both (input that came while the last frame
    /// refreshed): the parts to refresh joined, in the stronger mode; the
    /// earlier time to wake; grays as the later frame says.
    #[must_use]
    pub fn and(self, later: Effects) -> Effects {
        let refresh = match (self.refresh, later.refresh) {
            (Some((a, m)), Some((b, n))) => Some((a.union(&b), m.max(n))),
            (a, b) => a.or(b),
        };
        let wake_at = match (self.wake_at, later.wake_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Effects {
            refresh,
            grays: later.grays,
            wake_at,
            restart: self.restart || later.restart,
            sleep: self.sleep || later.sleep,
            power_off: self.power_off || later.power_off,
            proceed: self.proceed || later.proceed,
            pace: later.pace,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.refresh.is_none()
            && self.wake_at.is_none()
            && !self.restart
            && !self.sleep
            && !self.power_off
            && !self.proceed
    }

    pub fn iter(&self) -> impl Iterator<Item = Effect> {
        let refresh = self
            .refresh
            .map(|(region, mode)| Effect::Refresh { region, mode });
        refresh
            .into_iter()
            .chain(self.wake_at.map(Effect::WakeAt))
            .chain(self.restart.then_some(Effect::Restart))
            .chain(self.sleep.then_some(Effect::Sleep))
            .chain(self.power_off.then_some(Effect::PowerOff))
            .chain(self.grays.then_some(Effect::Grays))
            .chain(self.proceed.then_some(Effect::Proceed))
    }
}
