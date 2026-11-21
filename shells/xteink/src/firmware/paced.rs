//! Frames of a screen that runs at a pace (`Effects::pace`, ADR 0030):
//! the refresh is started and the main loop goes on, so the screen's
//! timers run and the buttons are read while the panel works; a frame
//! asked for meanwhile waits until the panel is free, then shows what is
//! current. Other screens are shown waiting, as before
//! ([`Firmware::show`]).

use core::time::Duration;

use log::{debug, warn};
use paperpaw_platform::{FrameView, Instant, Rect, RefreshMode};
use paperpaw_x4::display::{Planes, Refresh};
use paperpaw_x4::paced::Ask;
use paperpaw_x4::sent::{Check, Sent};

use super::{Firmware, heap_free};
use crate::{now, power};

/// A frame on its way to the panel while the main loop goes on.
#[derive(Debug)]
pub(super) struct InFlight {
    area: Rect,
    mode: RefreshMode,
    refresh: Refresh,
    /// When it began to be drawn, and when its refresh started.
    started: Instant,
    drawn: Instant,
    /// Whether what was sent goes again into both memories once it is
    /// shown: not where the panel's driver keeps the frame.
    send_again: bool,
    /// What was sent, for both memories once it is shown.
    sent: Sent,
    written: Check,
}

impl Firmware {
    /// Whether `area` in `mode` goes as a paced frame: the screen runs at
    /// a pace, the refresh is no cleaning one, nothing else is asked.
    pub(super) fn paced(&self, mode: RefreshMode) -> bool {
        self.state.pace.is_some() && !mode.cleans() && !self.state.gray_shown
    }

    /// Shows `area` in `mode` without waiting for the panel: at once if it
    /// is free, else once the frame on its way is shown.
    pub(super) fn queue_frame(&mut self, area: Rect, mode: RefreshMode) {
        if self.state.frames.ask(area, mode) == Ask::Start {
            self.start_frame(area, mode);
        }
    }

    /// Draws and sends what changed and starts its refresh, not waiting
    /// for it. After a frame whose memories could not be made the same,
    /// the panel is driven whole once.
    fn start_frame(&mut self, area: Rect, mode: RefreshMode) {
        power::mark(power::at::SHOWING);
        let stale = core::mem::take(&mut self.state.frame_stale);
        let refresh = if stale { Refresh::Half } else { Refresh::Fast };
        let planes = if stale { Planes::BOTH } else { Planes::NEW };
        let started = now();
        // A driver that keeps the frame needs nothing sent again.
        let send_again = !self.panel.keeps_frame();
        let mut sent = Sent::new(heap_free);
        let mut written = Check::default();
        let mut failed = None;
        let panel = &mut self.panel;
        self.kernel.render(&mut |top, rows| {
            if let Err(e) = panel.write_rows(top, rows, planes) {
                failed = Some(e);
            }
            if send_again {
                written.add(top, &rows);
                sent.keep(top, &rows);
            }
        });
        let drawn = now();
        self.kernel.showing(drawn);
        // The waveform of the frame's mode, as for any frame: it stays set
        // from the last one, so a screen at a pace would else show with
        // whatever a frame before it left (the panel's own, 500 ms).
        self.set_waveform(refresh, mode);
        if let Err(e) = self.panel.start_refresh(refresh, false) {
            failed = Some(e);
        }
        if let Some(e) = failed {
            warn!("display: {e:?}");
        }
        self.state.in_flight = Some(InFlight {
            area,
            mode,
            refresh,
            started,
            drawn,
            send_again,
            sent,
            written,
        });
    }

    /// Finishes the frame on its way once the panel is done with it, and
    /// starts the one that waited.
    pub(super) fn poll_frame(&mut self) {
        if self.state.in_flight.is_none() || self.panel.refreshing().unwrap_or(false) {
            return;
        }
        self.finish_frame();
        if let Some((area, mode)) = self.state.frames.shown() {
            self.start_frame(area, mode);
        }
    }

    /// Waits for the frame on its way and the one that waited, if any:
    /// before anything else uses the panel, or the device sleeps.
    pub(super) fn settle_frame(&mut self) {
        while self.state.in_flight.is_some() {
            self.finish_frame();
            if let Some((area, mode)) = self.state.frames.shown() {
                self.start_frame(area, mode);
            }
        }
    }

    /// When the frame on its way should be done, as long as the last
    /// refresh of its kind took: the main loop rests no longer.
    pub(super) fn frame_due(&self) -> Option<u64> {
        let frame = self.state.in_flight.as_ref()?;
        let kind = super::kind(frame.refresh);
        Some(frame.drawn.0 + self.state.refresh_ms[kind].max(1))
    }

    /// The panel is done (or waited for): both memories made to hold the
    /// frame, the times noted for the kernel and its plugins.
    fn finish_frame(&mut self) {
        let Some(mut frame) = self.state.in_flight.take() else {
            return;
        };
        if let Err(e) = self.panel_wait() {
            warn!("display: {e:?}");
        }
        let shown = now();
        let took = shown.since(frame.drawn);
        let slot = &mut self.state.refresh_ms[super::kind(frame.refresh)];
        *slot = if *slot == 0 { took } else { (*slot).min(took) };
        self.state.panel_idle_at = Some(shown.0 + self.panel_idle_ms());
        self.kernel.panel_busy(took);
        self.kernel.shown(shown);
        let ms = |millis: u64| Duration::from_millis(millis);
        self.kernel.frame_shown(
            frame.area,
            frame.mode,
            ms(frame.drawn.since(frame.started)),
            ms(took),
            shown,
        );
        if frame.send_again {
            self.send_again(&mut frame);
        }
        debug!(
            "paced frame {},{} {}x{}: drawn in {} ms, shown in {took} ms",
            frame.area.x,
            frame.area.y,
            frame.area.width,
            frame.area.height,
            frame.drawn.since(frame.started)
        );
    }

    /// As after every refresh: both memories hold the frame shown, sent
    /// again as kept, or drawn anew where nothing changed since.
    fn send_again(&mut self, frame: &mut InFlight) {
        let mut again_written = Check::default();
        let panel = &mut self.panel;
        let mut again = |top: u16, rows: FrameView<'_>| {
            if let Err(e) = panel.write_rows(top, rows, Planes::BOTH) {
                warn!("display: {e:?}");
            }
            again_written.add(top, &rows);
        };
        if !frame.sent.replay(&mut again) {
            self.kernel.rerender(&mut again);
        }
        if again_written != frame.written {
            // Changes came meanwhile and nothing was kept: the next frame
            // drives every pixel.
            self.state.frame_stale = true;
            warn!("paced frame not kept: the next one is shown whole");
        }
    }
}
