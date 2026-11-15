//! A whole screen with pictures is shown at once in black and white, its
//! pictures dithered; the grays follow by themselves once nothing happens for
//! [`CALM_MS`] (Decision 196). Without this the pictures rule drew the screen
//! twice before it showed (once to count the pictures, once in grays).
//!
//! The wait belongs to the screen that asked for it: it is dropped when that
//! screen is gone and held while another is on top (a dialog). A key or a
//! touch begins the calm again. A screen that has had its grays and draws
//! itself again with no key since (Home each minute) keeps them: grays at
//! once, as before.

use paperpaw_platform::Instant;

use crate::Kernel;

/// How long nothing may happen before the gray pass: no key, no other frame.
const CALM_MS: u64 = 3_000;

/// Where a screen's gray pass stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Phase {
    /// None is due.
    #[default]
    Nothing,
    /// A frame of the pictures rule is on its way, in black and white.
    Asked,
    /// It was drawn and has pictures: the calm begins when it is shown (and
    /// its screen is on top).
    Pictured,
    /// The gray pass is due at this time if nothing has happened.
    At(Instant),
    /// The gray pass is the frame of this call.
    Pass,
    /// The pass was shown: the screen has its grays.
    Done,
}

/// The wait, and the serial of the screen it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct GrayLater {
    pub(crate) phase: Phase,
    pub(crate) screen: u32,
}

/// What a frame that was made says to the wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Frame {
    /// The gray pass.
    Pass,
    /// A frame in black and white for the pictures rule: grays follow.
    Later,
    /// The screen that has its grays drew itself again, in grays: it keeps them.
    Kept,
    /// Most of the screen with no pictures to show in grays (or shown in
    /// grays by the screen itself, or at once before sleep): no pass.
    Whole,
    /// Anything else (a part of the screen, a refresh set exactly, a frame
    /// drawn again after grays): the wait is as it was.
    Other,
}

/// What decides how a frame is shown, as `finish_frame` knows it.
pub(crate) struct GrayRule {
    /// The screen on top is shown in grays by itself.
    pub(crate) grays: bool,
    /// The gray pass is this frame.
    pub(crate) pass: bool,
    /// The frame is drawn again after grays (the X4's shell asks for it).
    pub(crate) again: bool,
    /// The screen on top is the whole screen.
    pub(crate) whole: bool,
    /// The frame is most of the screen, shown as the screen asks (not set
    /// exactly).
    pub(crate) big: bool,
    /// The screen on top has pictures to show in grays.
    pub(crate) pictures: bool,
    /// The device goes to sleep (or off) on this frame.
    pub(crate) sleeping: bool,
}

impl Kernel {
    /// Whether the frame is shown in grays, and what it says to the wait.
    pub(crate) fn gray_decide(&mut self, rule: GrayRule) -> (bool, Frame) {
        if rule.pass {
            return (true, Frame::Pass);
        }
        let screen_grays = rule.whole && rule.grays;
        let big_whole = rule.whole && rule.big;
        if rule.again || !big_whole {
            return (screen_grays, Frame::Other);
        }
        let eligible = rule.pictures
            && self.shared.device.display.format.gray_levels() == Some(2)
            && self
                .shared
                .settings
                .bool(paperpaw_sdk::builtin::settings::PICTURES_IN_GRAYS.as_str());
        if screen_grays || !eligible {
            return (screen_grays, Frame::Whole);
        }
        // A book page keeps the rule as it was: its grays at once where it
        // has pictures (Decision 196 is about Home and the other screens).
        let reading = self
            .stack
            .last()
            .and_then(|top| top.slot.as_ref())
            .is_some_and(|slot| *slot == paperpaw_sdk::builtin::slots::READER);
        if reading {
            return (self.pictures_drawn(), Frame::Whole);
        }
        if self.gray_kept() {
            return (true, Frame::Kept);
        }
        // The screen a device goes to sleep on has no calm to wait for:
        // in grays at once, where it has the pictures.
        if rule.sleeping && self.pictures_drawn() {
            return (true, Frame::Whole);
        }
        (false, Frame::Later)
    }

    fn gray_screen(&self) -> Option<u32> {
        self.stack.last().map(|s| s.serial)
    }

    /// A key or a touch: the calm is counted from this call, and a screen
    /// that had its grays is asked for them again (a frame it draws is black
    /// and white first).
    pub(crate) fn gray_postpone(&mut self) {
        match self.frame.gray_later.phase {
            Phase::At(_) => self.frame.gray_later.phase = Phase::Pictured,
            Phase::Done => self.frame.gray_later.phase = Phase::Nothing,
            _ => {}
        }
    }

    /// Whether the screen on top had its grays, and draws itself again.
    pub(crate) fn gray_kept(&self) -> bool {
        let gray = self.frame.gray_later;
        gray.phase == Phase::Done && self.gray_screen() == Some(gray.screen)
    }

    /// At the start of a call's frame: the calm begins once the frame is
    /// shown (this call is after it) and the screen is on top; the pass is
    /// the frame when the calm is over, where no other frame waits.
    pub(crate) fn gray_step(&mut self) -> bool {
        let gray = self.frame.gray_later;
        if gray.phase == Phase::Nothing {
            return false;
        }
        if !self.stack.iter().any(|s| s.serial == gray.screen) {
            self.frame.gray_later = GrayLater::default();
            return false;
        }
        let on_top = self.gray_screen() == Some(gray.screen);
        match gray.phase {
            Phase::Pictured if on_top => {
                self.frame.gray_later.phase = Phase::At(self.now.add_millis(CALM_MS));
            }
            // Something came over it: the calm begins when it is on top again.
            Phase::At(_) if !on_top => self.frame.gray_later.phase = Phase::Pictured,
            Phase::At(at)
                if at <= self.now
                    && self.frame.pending.is_none()
                    && self.frame.unrendered.is_none()
                    && self.frame.deferred.is_none() =>
            {
                self.frame.gray_later.phase = Phase::Pass;
                self.invalidate(self.framebuffer.bounds());
            }
            _ => {}
        }
        self.frame.gray_later.phase == Phase::Pass
    }

    /// After a refresh was made.
    pub(crate) fn gray_frame(&mut self, frame: Frame) {
        let screen = self.gray_screen().unwrap_or(0);
        match frame {
            Frame::Pass => {
                self.frame.gray_later = GrayLater {
                    phase: Phase::Done,
                    screen,
                }
            }
            Frame::Later => {
                self.frame.gray_later = GrayLater {
                    phase: Phase::Asked,
                    screen,
                }
            }
            Frame::Whole => self.frame.gray_later = GrayLater::default(),
            Frame::Kept | Frame::Other => {
                if self.frame.gray_later.phase == Phase::Pass {
                    self.frame.gray_later = GrayLater::default();
                }
            }
        }
    }

    /// The frame is drawn, `pictured` pixels of it pictures: grays follow if
    /// there are enough of them ([`crate::PICTURES_GRAY_PERCENT`] of it).
    pub(crate) fn gray_seen(&mut self, pictured: u32, screen: u32) {
        if self.frame.gray_later.phase == Phase::Asked {
            let enough =
                pictured.saturating_mul(100) >= screen.saturating_mul(crate::PICTURES_GRAY_PERCENT);
            // A page whose pictures come in the frame after it (a follow-up)
            // is counted in that one.
            self.frame.gray_later.phase = if enough {
                Phase::Pictured
            } else if self.frame.followup.is_some() {
                Phase::Asked
            } else {
                Phase::Nothing
            };
        }
    }

    /// When the shell is to call next for it: at once after the frame (to
    /// begin the calm there), then when the calm is over; not while another
    /// screen is on top of it.
    pub(crate) fn gray_wake(&self, refreshing: bool) -> Option<Instant> {
        let gray = self.frame.gray_later;
        match gray.phase {
            Phase::Asked if refreshing => Some(self.now),
            Phase::At(at) if self.gray_screen() == Some(gray.screen) => Some(at),
            _ => None,
        }
    }
}
