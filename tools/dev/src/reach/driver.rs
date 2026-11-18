//! The device the searches drive (see [`super`]): a session taken by
//! steps, able to go back to where a way of steps leads, with the
//! settings kept as at the start where a search asks for that.

use std::collections::{BTreeMap, HashSet};

use paperpaw_core::sdk::semantics;
use paperpaw_platform::{ButtonState, InputEvent, buttons};

use super::Seen;
use crate::cases;
use crate::cli::Context;
use crate::session::Session;
use crate::variant::Variant;

/// Time between presses, as in the emulator's scripts.
const STEP_MS: u64 = 100;
/// How long a short press is held.
const PRESS_MS: u64 = 60;
/// How long a long press is held.
const LONG_MS: u64 = 1200;

/// A session driven by steps: presses (`confirm`, `long:confirm`) and
/// screens opened directly (`open:<screen>`), able to go back to where a
/// way of steps leads.
#[derive(Debug)]
pub struct Driver<'a> {
    pub(super) context: &'a Context,
    pub variant: Variant,
    pub session: Session,
    /// Steps taken.
    pub steps: usize,
    /// The last step taken.
    last: Option<String>,
    /// Fresh sessions started.
    pub fresh: usize,
    /// The screens shown while the device slept (power woke it).
    pub asleep: HashSet<String>,
    /// The settings as at the start, where they are kept so
    /// ([`Driver::keep_settings`]).
    kept: Option<BTreeMap<String, String>>,
    /// The settings as they were after the last step.
    settings: BTreeMap<String, String>,
    /// The settings a press changed, each with the screen it was pressed
    /// on.
    changed: Vec<(String, String)>,
}

/// The settings of `session` with their values, but the secrets.
fn settings_of(session: &Session) -> BTreeMap<String, String> {
    let settings = session.device.kernel.settings();
    settings
        .specs()
        .filter(|spec| !spec.extra().secret)
        .filter_map(|spec| {
            let value = settings.get(spec.id.as_str())?;
            Some((spec.id.to_string(), value.to_config_text()))
        })
        .collect()
}

impl<'a> Driver<'a> {
    pub fn new(context: &'a Context, variant: &Variant) -> Result<Self, String> {
        Ok(Self::with(context, Session::new(context, variant)?))
    }

    /// Drives `session`, started as a check wants it.
    pub fn with(context: &'a Context, session: Session) -> Self {
        Driver {
            context,
            variant: session.variant.clone(),
            session,
            steps: 0,
            last: None,
            fresh: 0,
            asleep: HashSet::new(),
            kept: None,
            settings: BTreeMap::new(),
            changed: Vec::new(),
        }
    }

    /// Keeps the settings as they are now from here on: one a press
    /// changes is set back once the screen it was pressed on is closed
    /// (while that is open, what the setting brings there can be seen).
    /// So what a search finds does not depend on what it pressed before:
    /// the language, the size of the text.
    pub fn keep_settings(&mut self) {
        self.settings = settings_of(&self.session);
        self.kept = Some(self.settings.clone());
        self.changed.clear();
    }

    /// Sets back the settings kept that changed, where the screen they
    /// were changed on (`on`, for those the last step changed) is closed.
    fn mind_settings(&mut self, on: &str) {
        let Some(kept) = &self.kept else {
            return;
        };
        let now = settings_of(&self.session);
        if now == self.settings && self.changed.is_empty() {
            return;
        }
        for (id, value) in &now {
            if self.settings.get(id) != Some(value) {
                self.changed.retain(|(changed, _)| changed != id);
                if kept.get(id).is_some_and(|kept| kept != value) {
                    self.changed.push((id.clone(), on.to_string()));
                }
            }
        }
        let stack = self.session.stack();
        let back: Vec<(String, String)> = self
            .changed
            .iter()
            .filter(|(_, screen)| !stack.contains(screen))
            .filter_map(|(id, _)| Some((id.clone(), kept.get(id)?.clone())))
            .collect();
        self.changed.retain(|(_, screen)| stack.contains(screen));
        if back.is_empty() {
            self.settings = now;
            return;
        }
        for (id, value) in &back {
            // One that cannot be set so stays as it is.
            let _ = self.session.set(id, value);
        }
        self.settings = settings_of(&self.session);
    }

    /// A device as at the start; `false` where none could be started.
    pub fn fresh(&mut self) -> bool {
        self.fresh += 1;
        match Session::new(self.context, &self.variant) {
            Ok(session) => self.session = session,
            Err(_) => return false,
        }
        if let Some(kept) = &self.kept {
            self.settings = kept.clone();
        }
        self.changed.clear();
        self.last = None;
        true
    }

    pub fn observe(&mut self) -> Seen {
        let kernel = &mut self.session.device.kernel;
        let content = kernel.device().display.height;
        let hints = kernel
            .described()
            .into_iter()
            .filter(|d| d.clip.y >= content && !d.label.trim().is_empty())
            .map(|d| d.label)
            .collect();
        let described = kernel.described_active();
        Seen {
            stack: self.session.stack(),
            parts: semantics::parts(described),
            hints,
        }
    }

    /// Takes `step`; a device that went to sleep is woken.
    pub fn step(&mut self, step: &str) {
        self.steps += 1;
        self.last = Some(step.to_string());
        let on = self.session.screen().unwrap_or_default();
        let device = &mut self.session.device;
        if let Some(screen) = step.strip_prefix("open:") {
            if let Ok(id) = paperpaw_core::sdk::Id::parse(screen) {
                device.open_screen(id, paperpaw_core::sdk::Params::new());
            }
        } else if let Some(ms) = step.strip_prefix("wait:") {
            wait(device, ms.parse().unwrap_or(0));
        } else {
            let (name, hold) = match step.strip_prefix("long:") {
                Some(name) => (name, LONG_MS),
                None => (step, PRESS_MS),
            };
            if let Some(&name) = device.profile.buttons.iter().find(|b| **b == name) {
                press(device, name, hold);
            }
        }
        wait(device, STEP_MS);
        self.session.settle_network();
        if self.session.device.asleep() {
            if let Some(screen) = self.session.screen() {
                self.asleep.insert(screen);
            }
            press(&mut self.session.device, buttons::POWER, PRESS_MS);
            wait(&mut self.session.device, STEP_MS);
        }
        self.mind_settings(&on);
    }

    /// Back to the home screen, as it was at the start where it can be.
    pub fn home(&mut self) {
        cases::home(&mut self.session);
        self.last = None;
        self.mind_settings("");
    }

    /// Goes where `way` leads from the home screen and says what is seen,
    /// if that is `state` (any, if `None`); with a fresh session if the
    /// way led elsewhere (what the steps before changed, e.g. settings).
    pub fn go(&mut self, way: &[String], state: Option<&str>) -> Option<Seen> {
        for fresh in [false, true] {
            if fresh {
                if !self.fresh() {
                    return None;
                }
            } else {
                self.home();
            }
            for step in way {
                self.step(step);
            }
            let seen = self.observe();
            if state.is_none_or(|s| seen.state() == s) {
                return Some(seen);
            }
        }
        None
    }
}

impl Driver<'_> {
    /// The press that moves the focus back where `step` moved it from:
    /// left after right, page back after page forward (also after their
    /// long presses, which move as the short ones where nothing else is
    /// bound to them).
    fn opposite(&self, step: &str) -> Option<String> {
        let button = step.strip_prefix("long:").unwrap_or(step);
        let opposite = [
            (buttons::RIGHT, buttons::LEFT),
            (buttons::DOWN, buttons::UP),
            (buttons::PAGE_FORWARD, buttons::PAGE_BACK),
        ]
        .iter()
        .find_map(|&(a, b)| match button {
            _ if button == a => Some(b),
            _ if button == b => Some(a),
            _ => None,
        })?;
        self.variant
            .profile
            .buttons
            .contains(&opposite)
            .then(|| opposite.to_string())
    }

    /// Gets from `current` to `state`, where `way` leads, the cheapest
    /// way: there already; back from a screen opened there; the opposite
    /// of a press that moved the focus; one step on from `before`, where
    /// the way leads without its last step; else the way again.
    pub fn arrive(
        &mut self,
        current: &Seen,
        way: &[String],
        state: &str,
        before: Option<&str>,
        back: Option<&str>,
    ) -> Option<Seen> {
        if current.state() == state {
            return Some(current.clone());
        }
        let mut current = current.clone();
        let depth = |state: &str| {
            state
                .split('|')
                .next()
                .unwrap_or_default()
                .split('>')
                .count()
        };
        if let Some(back) = back
            && current.stack.len() > depth(state)
        {
            self.step(back);
            current = self.observe();
            if current.state() == state {
                return Some(current);
            }
        }
        if current.stack.join(">") == state.split('|').next().unwrap_or_default()
            && let Some(opposite) = self.last.as_deref().and_then(|l| self.opposite(l))
        {
            self.step(&opposite);
            current = self.observe();
            if current.state() == state {
                return Some(current);
            }
        }
        if let (Some(before), Some(last)) = (before, way.last())
            && current.state() == before
        {
            self.step(last);
            current = self.observe();
            if current.state() == state {
                return Some(current);
            }
        }
        self.go(way, Some(state))
    }
}

fn press(device: &mut paperpaw_emulator::device::Device, name: &'static str, hold: u64) {
    let event = |state| InputEvent::Button { name, state };
    device.input(event(ButtonState::Pressed));
    wait(device, hold);
    device.input(event(ButtonState::Released));
}

/// Lets `millis` of emulated time pass, waking the kernel when it asked.
fn wait(device: &mut paperpaw_emulator::device::Device, millis: u64) {
    let end = device.now().add_millis(millis);
    loop {
        let now = device.now();
        let next = device.next_wake().filter(|&at| at < end).unwrap_or(end);
        device.advance(next.since(now).max(1));
        device.tick_if_due();
        if next >= end {
            return;
        }
    }
}
