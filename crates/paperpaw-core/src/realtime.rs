//! Screens that run in real time ([`paperpaw_sdk::realtime`], ADR 0030):
//! the mode of the screen on top, raw keys and their way out, the
//! buttons held, and how frames went.

use core::time::Duration;

use paperpaw_platform::{ButtonState, Instant, Rect, RefreshMode, buttons};
use paperpaw_sdk::realtime::{ESCAPE_HOLD, FrameReport, KeyEvent, ScreenMode};
use paperpaw_sdk::{Command, Handled, ScreenKey};

use crate::Kernel;

/// What the kernel keeps for raw keys.
#[derive(Debug, Default)]
pub(crate) struct RawKeys {
    /// Presses the screen on top took: their releases go to it too.
    taken: alloc::vec::Vec<&'static str>,
    /// When *Back* went down while taken, for the way out.
    back_since: Option<Instant>,
    /// The finger held in the top left corner over such a screen, and
    /// since when: the same way out by touch.
    corner: Option<(u8, Instant)>,
    /// Frames asked for since the last one shown.
    frames_asked: u16,
}

/// Kept once the way out of a screen that takes every key was shown.
const ESCAPE_HINTED: &str = ".paperpaw/core/escape-hinted";

impl Kernel {
    /// The first time a screen that takes every key shows, a notice says
    /// how to leave it (*Back* held, or a corner held on touch screens);
    /// never again. A native screen takes every key only to hand it to its
    /// plugin, which leaves on *Back* as any screen does: no notice over it,
    /// as Beta 3 showed none over the screens native plugins now draw.
    pub(crate) fn hint_escape(&mut self) {
        let native = self
            .stack
            .len()
            .checked_sub(1)
            .is_some_and(|top| self.is_native_at(top));
        if native || !self.top_mode().takes_raw_keys() || self.shared.storage.exists(ESCAPE_HINTED)
        {
            return;
        }
        let key = match self.shared.device.input.touch {
            true => "core.escape.touch",
            false => "core.escape.buttons",
        };
        let text = alloc::string::String::from(self.shared.translator.tr(key));
        // Kept before it shows: a card that takes nothing shows it once a
        // start at most.
        let _ = self.shared.storage.write(ESCAPE_HINTED, b"");
        self.notify(text);
    }

    /// How the screen on top runs; a screen of pages where there is none.
    pub(crate) fn top_mode(&self) -> ScreenMode {
        self.stack
            .last()
            .map_or_else(ScreenMode::new, |top| top.screen.mode())
    }

    /// Whether open screen `index` has all of the panel: no bars over it.
    pub(crate) fn full_screen(&self, index: usize) -> bool {
        self.stack
            .get(index)
            .is_some_and(|s| s.screen.mode().is_full_screen())
    }

    /// Keeps which buttons are held down, as they physically are.
    pub(crate) fn note_held(&mut self, name: &'static str, state: ButtonState) {
        let held = &mut self.shared.keys_held;
        held.retain(|n| *n != name);
        if state == ButtonState::Pressed {
            held.push(name);
        }
    }

    /// Offers button `name` to the screen input goes to (the one on top,
    /// or the list beside it) where it takes raw keys;
    /// whether it took it (then nothing else sees it). A release goes
    /// where its press went; the power button is never offered.
    pub(crate) fn raw_key(&mut self, name: &'static str, state: ButtonState, now: Instant) -> bool {
        let pressed = state == ButtonState::Pressed;
        if pressed {
            let input = self
                .input_index()
                .map(|index| self.stack[index].screen.mode());
            if name == buttons::POWER || !input.is_some_and(|mode| mode.takes_raw_keys()) {
                return false;
            }
            let key = KeyEvent::pressed(name, now);
            if self.with_input(|screen, cx| screen.on_key(&key, cx)) != Handled::Yes {
                return false;
            }
            self.raw.taken.push(name);
            if name == buttons::BACK {
                self.raw.back_since = Some(now);
            }
            return true;
        }
        let Some(at) = self.raw.taken.iter().position(|n| *n == name) else {
            return false;
        };
        self.raw.taken.remove(at);
        if name == buttons::BACK {
            self.raw.back_since = None;
        }
        let key = KeyEvent::released(name, now);
        self.with_input(|screen, cx| screen.on_key(&key, cx));
        true
    }

    /// When *Back* held down and taken, or a finger held in the top left
    /// corner, leaves the screen.
    pub(crate) fn escape_due(&self) -> Option<Instant> {
        let hold = u64::try_from(ESCAPE_HOLD.as_millis()).unwrap_or(u64::MAX);
        let since = match (self.raw.back_since, self.raw.corner) {
            (Some(back), Some((_, corner))) => Some(back.min(corner)),
            (back, corner) => back.or(corner.map(|(_, at)| at)),
        };
        since.map(|since| since.add_millis(hold))
    }

    /// Follows a finger in the top left corner of a screen that has all
    /// of the panel or takes raw keys: held there for [`ESCAPE_HOLD`], it
    /// leaves the screen, as *Back* held does (touch screens may have no
    /// *Back*). It is offered to the screen as ever.
    pub(crate) fn note_corner(&mut self, pointer: &paperpaw_platform::PointerEvent, now: Instant) {
        use paperpaw_platform::PointerPhase;
        let mode = self.top_mode();
        if !(mode.is_full_screen() || mode.takes_raw_keys()) {
            self.raw.corner = None;
            return;
        }
        let panel = self.framebuffer.bounds();
        let size = i32::from(panel.width.min(panel.height) / 8);
        let inside = pointer.point.x < size && pointer.point.y < size;
        self.raw.corner = match (pointer.phase, self.raw.corner) {
            (PointerPhase::Down, _) if inside => Some((pointer.id, now)),
            (PointerPhase::Move, Some((id, since))) if id == pointer.id && inside => {
                Some((id, since))
            }
            (_, Some((id, since))) if id != pointer.id => Some((id, since)),
            _ => None,
        };
    }

    /// The way out of a screen that takes raw keys: *Back* held long
    /// enough closes it, its release told first, whatever it does with
    /// *Back* otherwise.
    pub(crate) fn escape(&mut self, now: Instant) {
        if !self.escape_due().is_some_and(|due| due <= now) {
            return;
        }
        self.raw.corner = None;
        if self.raw.back_since.take().is_some() {
            self.raw.taken.retain(|n| *n != buttons::BACK);
            let key = KeyEvent::released(buttons::BACK, now);
            self.with_top(|screen, cx| screen.on_key(&key, cx));
        }
        if let Some(top) = self.stack.last() {
            let key = ScreenKey::new(top.serial);
            self.output.commands.push(Command::Close(key));
        }
    }

    /// A screen came or went: presses taken by the one before are let go
    /// for nobody (the new one did not see them go down).
    pub(crate) fn forget_raw(&mut self) {
        self.raw.taken.clear();
        self.raw.back_since = None;
        self.raw.corner = None;
    }

    /// The refresh mode for changes where the screen asked for the fastest
    /// and set none exactly: black and white where that is enough.
    pub(crate) fn fastest_mode(mode: &ScreenMode, requested: Option<RefreshMode>) -> RefreshMode {
        match requested {
            Some(strong) if strong > RefreshMode::Partial => strong,
            _ if mode.grays_enough() == Some(2) => RefreshMode::Mono,
            _ => RefreshMode::Fast,
        }
    }

    /// Notes that a frame went to the panel.
    pub(crate) fn frame_asked(&mut self) {
        self.raw.frames_asked = self.raw.frames_asked.saturating_add(1);
    }

    /// The frame last rendered was shown: `area` in `mode`, drawn and
    /// sent in `drawn`, on the panel for `shown`, done at `at`. Plugins
    /// read it ([`Env::panel`](paperpaw_sdk::Env::panel)), and the mode's
    /// time counts as measured.
    pub fn frame_shown(
        &mut self,
        area: Rect,
        mode: RefreshMode,
        drawn: Duration,
        shown: Duration,
        at: Instant,
    ) {
        let merged = self.raw.frames_asked.saturating_sub(1);
        self.raw.frames_asked = 0;
        let panel = &mut self.shared.panel;
        panel.measured(mode, shown);
        panel.set_last(FrameReport::new(area, mode, at, drawn, shown, merged));
    }
}
