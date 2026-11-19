//! The sleep screen shown again while the device sleeps (`command.show_again`,
//! `docs/abi/ui.md` 1.4). A plugin asks for it from the sleep screen (after
//! `core.suspend`); the shell that can wake for a moment
//! ([`Capability::REFRESH_ASLEEP`](paperpaw_sdk::Capability)) asks
//! [`Kernel::show_again_at`] when to, and calls [`Kernel::show_again_now`] at
//! that time: the screen is drawn again, and what changed is refreshed. The
//! plan ends with the wake (`Kernel::woke`) and when the screen is no longer
//! on top.
use super::*;

/// What `command.show_again` asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The instance to draw again.
    pub(crate) screen: paperpaw_sdk::ScreenKey,
    /// When.
    pub(crate) due: Instant,
    /// The time between the shows after the first; none: it is once.
    pub(crate) every: Option<u32>,
}

impl Kernel {
    /// Keeps (or, with `after_ms` 0, drops) the request of `screen`: from the
    /// time the kernel was last given (`now`), only while asleep.
    pub(crate) fn plan_show_again(
        &mut self,
        screen: paperpaw_sdk::ScreenKey,
        after_ms: u32,
        every_ms: u32,
    ) {
        self.shared.show_again = (after_ms != 0 && self.shared.asleep).then(|| Plan {
            screen,
            due: self.shared.now.add_millis(u64::from(after_ms)),
            every: (every_ms != 0).then_some(every_ms),
        });
    }

    /// When the sleep screen is to be shown again, by the shell's clock; `None`
    /// where nothing is asked for (or the device is awake).
    #[must_use]
    pub fn show_again_at(&self) -> Option<Instant> {
        self.shared.show_again.map(|plan| plan.due)
    }

    /// Draws the sleep screen again if its time (`show_again_at`) has come at
    /// `now`: the screen drawn whole, refreshed as a partial update (the
    /// shell shows it, then sleeps on). The next time is kept for a request
    /// to `repeat` (counted from the time asked for, so it does not drift),
    /// else the request is done. `None` when it is not time, or the screen is
    /// no longer on top (the request ends).
    pub fn show_again_now(&mut self, now: Instant) -> Option<Effects> {
        let plan = self.shared.show_again.filter(|plan| plan.due <= now)?;
        self.shared.show_again = plan.every.map(|every| {
            let mut due = plan.due;
            while due <= now {
                due = due.add_millis(u64::from(every));
            }
            Plan { due, ..plan }
        });
        if self
            .shared
            .stack
            .last()
            .is_none_or(|top| top.key != plan.screen)
        {
            self.shared.show_again = None;
            return None;
        }
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.now = now;
        self.output.commands.push(Ask::Redraw(RefreshMode::Partial));
        self.run_commands();
        Some(self.finish())
    }
}
