//! Experiment (#218, X4 with an SSD1677, `gray-clock` builds only): the
//! home screen is in grays and its clock changes; the grays stay and only
//! the clock's rectangle is driven, instead of the change waiting for the
//! next page or the whole screen being driven again.
//!
//! After a gray pass the controller's two memories hold gray codes, not
//! the frame shown. Here, right after the pass, both are made the frame in
//! black and white (`Kernel::capture`; a gray pixel is black or white by
//! its brightness). A change of the clock then writes its rectangle into
//! the memory of the new frame only, and a fast refresh drives just the
//! pixels whose two bits differ: the grays elsewhere are left alone. The
//! next change of another kind drives every pixel, as after any gray pass
//! (`Display::start_refresh_keeping_grays`).
//!
//! Which variant, by the first character of `gray-clock.txt` on the card
//! (read at every gray pass and change; no file or `0`: as today, the
//! change waits):
//!
//! - `1` the difference only: the pixels of the clock's old and new digits;
//! - `2` the whole rectangle: its pixels all driven (a clean patch of black
//!   and white around the clock, grays gone in it);
//! - `3` as `1`, with the panel's own fast waveform;
//! - `4` as `1`, with both memories written whole again after each change.
//!
//! Without the feature every call here does nothing.

use paperpaw_platform::{Rect, RefreshMode};
use paperpaw_x4::display::{Error, Refresh};

use super::Firmware;

/// What the experiment keeps.
#[derive(Debug, Default)]
pub struct Clock {
    /// Both memories hold the frame shown in black and white.
    #[cfg(feature = "gray-clock")]
    ready: bool,
    /// The next refresh keeps the grays.
    #[cfg(feature = "gray-clock")]
    keep: bool,
}

#[cfg(feature = "gray-clock")]
mod on {
    use paperpaw_x4::display::Planes;

    use super::*;
    use crate::{now, power};
    use log::{info, warn};

    /// The file on the card that chooses the variant.
    const FILE: &str = "gray-clock.txt";

    impl Firmware {
        /// The variant (0 as today); only on a panel that can.
        fn gray_clock_variant(&self) -> u8 {
            let mut first = [0u8; 1];
            match self.kernel.storage().read_at(FILE, 0, &mut first) {
                Ok(1) if self.panel.can_change_in_grays() => first[0].saturating_sub(b'0'),
                _ => 0,
            }
        }

        /// A gray pass begins.
        pub(in crate::firmware) fn gray_begin(&mut self) {
            self.state.gray_clock.ready = false;
        }

        /// A gray pass is shown: both memories become the frame in black
        /// and white, so that a later refresh can compare.
        pub(in crate::firmware) fn gray_synced(&mut self) {
            if self.gray_clock_variant() == 0 {
                return;
            }
            let started = now();
            let panel = &mut self.panel;
            let mut failed = None;
            let drawn = self.kernel.capture(&mut |top, rows| {
                if let Err(e) = panel.write_rows(top, rows, Planes::BOTH) {
                    failed = Some(e);
                }
            });
            self.state.gray_clock.ready = drawn && failed.is_none();
            info!(
                "gray clock: memories made the frame in black and white in {} ms (ready {}, {failed:?})",
                now().since(started),
                self.state.gray_clock.ready
            );
        }

        /// The clock (or another small part, `area`) changed over a gray
        /// screen: shown alone if the memories are ready; else it waits, as
        /// it did.
        pub(in crate::firmware) fn clock_in_grays(&mut self, area: Rect, mode: RefreshMode) {
            let variant = self.gray_clock_variant();
            if variant == 0 || !self.state.gray_clock.ready {
                return;
            }
            power::mark(power::at::SHOWING);
            let rect = (
                area.x,
                area.y,
                area.x.saturating_add(area.width),
                area.y.saturating_add(area.height),
            );
            let started = now();
            let mut failed = None;
            let panel = &mut self.panel;
            self.kernel.render(&mut |top, rows| {
                let mut write = |planes, invert| {
                    if let Err(e) = panel.write_rect(top, rows, rect, planes, invert) {
                        failed = Some(e);
                    }
                };
                if variant == 2 {
                    // The old frame is the other way round in the rectangle.
                    write(Planes::SHOWN, true);
                }
                write(Planes::NEW, false);
            });
            let drawn = now();
            self.kernel.showing(drawn);
            self.set_waveform(Refresh::Fast, mode);
            if variant == 3 {
                self.panel.set_experiment(None);
            }
            self.state.gray_clock.keep = true;
            if let Err(e) = self.refresh(Refresh::Fast) {
                failed = Some(e);
            }
            let shown = now();
            self.kernel.panel_busy(shown.since(drawn));
            self.kernel.shown(shown);
            let ms = core::time::Duration::from_millis;
            self.kernel.frame_shown(
                area,
                mode,
                ms(drawn.since(started)),
                ms(shown.since(drawn)),
                shown,
            );
            // Both memories hold the frame shown again.
            let panel = &mut self.panel;
            if variant == 4 {
                self.kernel.capture(&mut |top, rows| {
                    if let Err(e) = panel.write_rows(top, rows, Planes::BOTH) {
                        failed = Some(e);
                    }
                });
            } else {
                self.kernel.rerender(&mut |top, rows| {
                    if let Err(e) = panel.write_rect(top, rows, rect, Planes::BOTH, false) {
                        failed = Some(e);
                    }
                });
            }
            self.state.panel_idle_at = Some(now().0 + self.panel_idle_ms());
            if let Some(e) = failed {
                warn!("display: {e:?}");
                self.state.gray_clock.ready = false;
            }
            info!(
                "gray clock {variant}: {}x{} at {},{}: drawn in {} ms, shown in {} ms, memories again in {} ms",
                area.width,
                area.height,
                area.x,
                area.y,
                drawn.since(started),
                shown.since(drawn),
                now().since(shown)
            );
        }

        /// Starts the refresh, keeping the grays where it was asked.
        pub(in crate::firmware) fn start_panel_refresh(
            &mut self,
            refresh: Refresh,
        ) -> Result<(), Error> {
            if core::mem::take(&mut self.state.gray_clock.keep) {
                self.panel.start_refresh_keeping_grays(false)
            } else {
                self.panel.start_refresh(refresh, false)
            }
        }
    }
}

#[cfg(not(feature = "gray-clock"))]
mod off {
    use super::*;

    impl Firmware {
        #[inline(always)]
        pub(in crate::firmware) fn gray_begin(&mut self) {
            // Nothing is kept without the feature.
            let _ = &self.state.gray_clock;
        }

        #[inline(always)]
        pub(in crate::firmware) fn gray_synced(&mut self) {}

        #[inline(always)]
        pub(in crate::firmware) fn clock_in_grays(&mut self, _: Rect, _: RefreshMode) {}

        #[inline(always)]
        pub(in crate::firmware) fn start_panel_refresh(
            &mut self,
            refresh: Refresh,
        ) -> Result<(), Error> {
            self.panel.start_refresh(refresh, false)
        }
    }
}
