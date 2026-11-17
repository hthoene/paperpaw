//! A kernel handler's turn (#300, D209).

use paperpaw_platform::{Instant, Rect};
use paperpaw_sdk::{Command, Id, Output, Params, RefreshMode, ScreenKey, StackEntry};

use crate::place::Place;

/// No params, for a turn without an action's.
const NO_PARAMS: &Params = &Params::new();

/// A kernel handler's turn: where it runs ([`Place`]) and what it asks of
/// the kernel when it is done (#300, D209; the SDK's old `Cx`).
#[derive(Debug)]
pub struct Turn<'a, 'e> {
    env: &'a Place<'e>,
    output: &'a mut Output,
    /// The screen whose handler runs, if it is one of the stack's.
    key: Option<ScreenKey>,
    /// The params of the action that runs ([`Turn::params`]).
    params: &'a Params,
}

impl<'a, 'e> Turn<'a, 'e> {
    /// Used by the kernel and by tests.
    pub fn new(env: &'a Place<'e>, output: &'a mut Output) -> Self {
        Self {
            env,
            output,
            key: None,
            params: NO_PARAMS,
        }
    }

    /// For an action run with `params` (the kernel's, or a test that
    /// plays it).
    pub fn with_params(mut self, params: &'a Params) -> Self {
        self.params = params;
        self
    }

    /// The params the action that `on_action` gets was run
    /// with ([`Command::RunAction`]); none from an input, and none outside
    /// `on_action`.
    pub fn params(&self) -> &Params {
        self.params
    }

    /// For the handler of the open screen `key` (the kernel's, or a test
    /// that plays it).
    pub fn for_screen(mut self, key: ScreenKey) -> Self {
        self.key = Some(key);
        self
    }

    /// Which opening of a screen this handler runs for: `None` outside
    /// the stack (a screen made aside, a notice).
    pub fn key(&self) -> Option<ScreenKey> {
        self.key
    }

    /// Whether the screen this handler runs for is on top, nothing over
    /// it: its time ([`Turn::wake_at`]) comes otherwise only below overlays
    /// alone where it asks (`times_under_overlays`), and what it
    /// does only on top then waits. Outside the stack (a screen made
    /// aside, a notice) it is not.
    pub fn is_on_top(&self) -> bool {
        let top = self.env().stack().last().map(|entry| entry.key);
        self.key.is_some_and(|key| top == Some(key))
    }

    /// This screen as the stack lists it ([`Place::stack`]).
    pub fn entry(&self) -> Option<&StackEntry> {
        let key = self.key?;
        self.env.stack().iter().find(|entry| entry.key == key)
    }

    /// Which screen this is.
    pub fn screen_id(&self) -> Option<&Id> {
        self.entry().map(|entry| &entry.screen)
    }

    /// The slot this screen was opened for, if any.
    pub fn slot(&self) -> Option<&Id> {
        self.entry().and_then(|entry| entry.slot.as_ref())
    }

    /// Closes this screen, wherever it is in the stack ([`Command::Close`]):
    /// the screens above it stay. Nothing outside the stack.
    pub fn close(&mut self) {
        if let Some(key) = self.key {
            self.output.commands.push(Command::Close(key));
        }
    }

    pub fn env(&self) -> &Place<'e> {
        self.env
    }

    /// Queues a command for the kernel.
    pub fn command(&mut self, command: Command) {
        self.output.commands.push(command);
    }

    /// Requests a refresh mode for what was drawn; the strongest request of
    /// an event wins.
    pub fn request_refresh(&mut self, mode: RefreshMode) {
        self.output.request_refresh(mode);
    }

    /// Sets the refresh mode for what was drawn exactly, over the requests
    /// of the event (the last one set wins): e.g. [`RefreshMode::Fast`]
    /// for an overlay opening, where the kernel would choose more. Refresh
    /// filters still see it.
    pub fn set_refresh(&mut self, mode: RefreshMode) {
        self.output.refresh_set = Some(mode);
    }

    /// Puts key map layer `layer` in front of the slot's for this screen
    /// while it is on top (see [`Command::SetKeyLayer`]); `None` takes it
    /// away. Nothing for a screen not of the stack.
    pub fn set_key_layer(&mut self, layer: Option<Id>) {
        if let Some(key) = self.key {
            self.output.commands.push(Command::SetKeyLayer(key, layer));
        }
    }

    /// Requests a complete redraw via `draw`.
    pub fn request_redraw(&mut self) {
        self.output.redraw = true;
    }

    /// Requests a redraw of `area` only, from the corner of the part of
    /// the screen the screen has ([`Place::content`]), as it draws; e.g. a
    /// status line or a focused item: `draw` runs clipped to it.
    pub fn request_redraw_area(&mut self, area: Rect) {
        self.output.redraw_area = Some(self.output.redraw_area.map_or(area, |a| a.union(&area)));
    }

    /// Requests a call to `on_timer` of this screen at `at` (the
    /// earliest request wins). Use `cx.env().now()` for "as soon as
    /// possible". It comes while the screen is on top: covered by another
    /// (a menu, a keyboard), the time waits until it is on top again,
    /// unless overlays alone cover it and it asks for it
    /// (`times_under_overlays`). It is kept when the screen is
    /// made anew for another size.
    /// Keeps the device from going to sleep by itself while this screen
    /// is on top, for as long as `hold` lasts (renew it before it ends):
    /// a clock, a timer, a slideshow. It ends when the screen closes.
    pub fn keep_awake(&mut self, hold: paperpaw_sdk::power::WakeHold) {
        let hold = hold.of_screen(self.key);
        self.output.commands.push(Command::KeepAwake(hold));
    }

    /// Lets go of the hold of `id`.
    pub fn let_sleep(&mut self, id: impl Into<Id>) {
        self.output.commands.push(Command::LetSleep(id.into()));
    }

    pub fn wake_at(&mut self, at: Instant) {
        self.output.wake_at = Some(self.output.wake_at.map_or(at, |w| w.min(at)));
    }
}
