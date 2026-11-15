//! A whole screen with pictures is shown at once in black and white, its
//! pictures dithered; the grays follow by themselves once nothing happens for
//! [`CALM_MS`] (Decision 196). Without this the pictures rule drew the screen
//! twice before it showed (once to count the pictures, once in grays).

use paperpaw_platform::Instant;

use crate::Kernel;

/// How long nothing may happen before the gray pass: no key, no other frame.
const CALM_MS: u64 = 3_000;

/// Where a screen's gray pass stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum GrayLater {
    /// None is due.
    #[default]
    Nothing,
    /// A frame of the pictures rule is on its way, in black and white.
    Asked,
    /// It was drawn and has pictures: the calm begins when it is shown.
    Pictured,
    /// The gray pass is due at this time if nothing has happened.
    At(Instant),
    /// The gray pass is the frame of this call.
    Pass,
}

impl Kernel {
    /// A key or a touch: the calm begins again from this call (a pass not
    /// yet due, or one waiting for the calm to begin, is not dropped).
    pub(crate) fn gray_postpone(&mut self) {
        if matches!(self.frame.gray_later, GrayLater::At(_)) {
            self.frame.gray_later = GrayLater::Pictured;
        }
    }

    /// At the start of a call's frame: the calm begins once the frame is
    /// shown (this call is after it), and the pass is the frame when the calm
    /// is over; whether it is. Only where no other frame waits.
    pub(crate) fn gray_step(&mut self) -> bool {
        match self.frame.gray_later {
            GrayLater::Pictured => {
                self.frame.gray_later = GrayLater::At(self.now.add_millis(CALM_MS));
            }
            GrayLater::At(at)
                if at <= self.now
                    && self.frame.pending.is_none()
                    && self.frame.unrendered.is_none()
                    && self.frame.deferred.is_none() =>
            {
                self.frame.gray_later = GrayLater::Pass;
                self.invalidate(self.framebuffer.bounds());
            }
            _ => {}
        }
        self.frame.gray_later == GrayLater::Pass
    }

    /// After a refresh was made: a frame of the whole screen asks for the
    /// pass if the pictures rule holds for it and ends the wait if not (what
    /// it shows has no pictures, or is shown in grays already); a part of the
    /// screen changing leaves it as it was.
    pub(crate) fn gray_asked(&mut self, whole: bool, rule: bool) {
        if whole {
            self.frame.gray_later = if rule {
                GrayLater::Asked
            } else {
                GrayLater::Nothing
            };
        } else if self.frame.gray_later == GrayLater::Pass {
            self.frame.gray_later = GrayLater::Nothing;
        }
    }

    /// The frame is drawn, `pictured` pixels of it pictures: grays follow if
    /// there are enough of them ([`crate::PICTURES_GRAY_PERCENT`] of it).
    pub(crate) fn gray_seen(&mut self, pictured: u32, screen: u32) {
        if self.frame.gray_later == GrayLater::Asked {
            let enough =
                pictured.saturating_mul(100) >= screen.saturating_mul(crate::PICTURES_GRAY_PERCENT);
            self.frame.gray_later = if enough {
                GrayLater::Pictured
            } else {
                GrayLater::Nothing
            };
        }
    }

    /// When the shell is to call next for it: at once after the frame (to
    /// begin the calm there), then when the calm is over.
    pub(crate) fn gray_wake(&self, refreshing: bool) -> Option<Instant> {
        match self.frame.gray_later {
            GrayLater::Asked if refreshing => Some(self.now),
            GrayLater::At(at) => Some(at),
            _ => None,
        }
    }
}
