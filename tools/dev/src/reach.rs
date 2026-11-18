//! Buttons alone: every screen a plugin adds can be reached and closed
//! with the device's buttons, and the focus visits all there is on it.
//!
//! It runs on every profile looked at that has buttons and no touch
//! screen (the X4s; `two-buttons` if asked for; once for profiles with
//! the same buttons and screen size), and checks, for every screen the
//! plugins looked at add:
//!
//! - `reach.unreachable`: it could not be reached from the home screen
//!   with the buttons alone. With the emulator's fixtures
//!   (`--network fixtures`), one that asked for data they do not give is
//!   an error: it waits for data, it is no dead end. A screen a setting offers among its choices
//!   (a mode of the sleep screen, shown in that one's place) is reached
//!   through that setting instead: a note (`reach.chosen`) says which.
//! - `reach.no-close`: back (the button, or what stands for it on a
//!   device without one), pressed up to five times (a folder at a time),
//!   neither closes it, nor brings the home screen, nor asks whether to
//!   leave.
//! - `reach.focus-skips`: moving the focus with the buttons that move it
//!   (left, right, the page buttons; each pressed in every place the
//!   focus goes, a keyboard's rows and columns too) leaves a row, button,
//!   switch or slider out (one inside a row counts with the row), or no
//!   focus is shown at all where there are several.
//! - `reach.focus-trap`: the focus stops on a part and none of these
//!   buttons moves it on, while there are other parts.
//! - `reach.undescribed`: a screen describes no parts, but a forward
//!   button changes a small part of it, as a focus moving would: what is
//!   on it cannot be followed (nor read aloud).
//!
//! On touch screens, for the screens that open without parameters, a
//! note (`reach.touch-label`) where a tap by a part's text, as flows and
//! recordings name parts (`tap-on:`), lands on another part.
//!
//! Each finding comes with a picture of the screen.
//!
//! # The search
//!
//! What the device shows is known from the screen shown and the parts it
//! describes (`Kernel::described_active`: rows, buttons, switches,
//! sliders with their texts, and which has the focus). A *view* is a
//! screen with its parts, wherever in the app it is; the search goes
//! breadth first through the views, from the home screen:
//!
//! 1. On a view newly seen every button is pressed once, short and long
//!    (the power button aside, and the backward one of each pair, left
//!    beside right, page back beside page forward: going round reaches
//!    the same).
//! 2. Then the focus is moved through it with each forward button
//!    (right, down, page forward), one press after the other, and on
//!    each part it reaches confirm is pressed (on a device without
//!    confirm, the long presses).
//! 3. What comes up that was not seen is searched in turn: the menus,
//!    the tools, the settings, and screens that others open with
//!    parameters, when confirm on one of their rows opens them.
//!
//! Views are searched in the order they were seen, but a screen that
//! was searched in the same place (over the same screen) already, with
//! other parts (another group of settings), waits until the screens in
//! new places are (the menu over the reader, the tools under the main
//! menu). The search ends when every screen it looks for is found, when
//! [`Limits::idle`] views in a row brought no screen not seen before, or
//! at [`Limits::views`]. To go on from a view the device is brought
//! there the cheapest way: back from what a press opened, the opposite
//! of a press that moved the focus, one press on from the part before,
//! else home and the way pressed again (in a fresh session if what was
//! pressed changed the way, e.g. a setting).
//!
//! It looks for the screens that open without parameters and stay
//! (notices that go by themselves are left out). One not found is a
//! warning, as the search is bounded and some ways need data the sample
//! library has not; its picture shows it opened directly. One that
//! needs parameters and was not found is noted (`reach.not-reached`).
//! Closing and the focus are checked on the screens reached.

use std::collections::{HashMap, HashSet, VecDeque};

use paperpaw_core::sdk::semantics::{self, Part, Role};
use paperpaw_platform::{ButtonState, InputEvent, Rect, buttons};

use crate::cases;
use crate::cli::Context;
use crate::image::Image;
use crate::report::{Finding, Report};
use crate::session::Session;
use crate::variant::Variant;

/// How often a screen is searched in the same place (over the same
/// screen), with other parts each time, before those not searched yet.
pub const SAME_PLACE: usize = 1;

/// How far the search goes, and the walk through a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The screens (with their parts) searched a variant; half as many
    /// again as the screens looked for, if that is more.
    pub views: usize,
    /// Views searched in a row that brought no screen not seen before,
    /// after which the search ends.
    pub idle: usize,
    /// The longest way searched, in presses.
    pub presses: usize,
    /// Presses of each forward button while a screen is searched.
    pub focus: usize,
    /// Presses while the focus is moved through a screen.
    pub walk: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            views: 60,
            idle: 20,
            presses: 30,
            focus: 30,
            walk: 400,
        }
    }
}

/// How long a screen opened alone is watched: one that is gone by then
/// went by itself (a notice).
const PASSING_MS: u64 = 10_000;
/// Time between presses, as in the emulator's scripts.
const STEP_MS: u64 = 100;
/// How long a short press is held.
const PRESS_MS: u64 = 60;
/// How long a long press is held.
const LONG_MS: u64 = 1200;

/// What is seen on the device after a step.
#[derive(Debug, Clone)]
pub struct Seen {
    /// The screens open, the one shown last.
    pub stack: Vec<String>,
    /// The parts of the screen shown, alone.
    pub parts: Vec<Part>,
    /// What the button hints below the screen say (they tell a screen's
    /// modes apart, e.g. the home screen being arranged).
    pub hints: Vec<String>,
}

impl Seen {
    pub fn top(&self) -> &str {
        self.stack.last().map_or("", String::as_str)
    }

    /// The part with the focus.
    pub fn focused(&self) -> Option<&Part> {
        semantics::focused(&self.parts)
    }

    /// The rows, buttons, switches and sliders, with a text.
    pub fn targets(&self) -> impl Iterator<Item = &Part> {
        self.parts.iter().filter(|p| is_target(p))
    }

    /// The same screens, the same focus, the same hints.
    pub fn state(&self) -> String {
        format!(
            "{}|{}|{}",
            self.stack.join(">"),
            self.focused().map(name).unwrap_or_default(),
            self.hints.join(" ")
        )
    }

    /// The state, the targets shown and where the part with the focus
    /// is: parts with the same text told apart, and pages of one screen
    /// (a guide's) whose focus sits on the same button.
    fn spot(&self) -> String {
        format!(
            "{}#{:?}#{}",
            self.state(),
            self.focused().map(|p| (p.area.x, p.area.y)),
            self.view()
        )
    }

    /// The screen shown with its targets (wherever in the app it is).
    fn view(&self) -> String {
        let mut names: Vec<String> = self.targets().map(name).collect();
        names.sort();
        format!(
            "{}|{}|{}",
            self.top(),
            names.join("|"),
            self.hints.join(" ")
        )
    }
}

/// Whether `part` is something to act on.
pub fn is_target(part: &Part) -> bool {
    matches!(
        part.role,
        Role::Row | Role::Button | Role::Switch | Role::Slider
    ) && part.label.chars().any(char::is_alphanumeric)
}

/// Whether `part` is a target the focus can go to: one that does nothing
/// now (a row of text, a switched-off row) is passed over.
pub fn takes_focus(part: &Part) -> bool {
    is_target(part) && !part.disabled
}

/// What a part is called, whatever value it shows: its role and its text
/// up to the first comma (a row's text is the texts in it, its title
/// first, joined with commas).
pub fn name(part: &Part) -> String {
    let title = part.label.split(", ").next().unwrap_or_default().trim();
    format!("{:?} \"{title}\"", part.role)
}

/// Whether `inner` lies inside `outer`.
fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

/// The buttons of `variant`'s profile pressed while searching: short
/// presses but the backward one of a pair and power, and long presses
/// of all but power.
pub fn keys(variant: &Variant) -> Vec<String> {
    let has = |b: &str| variant.profile.buttons.contains(&b);
    let backward = |b: &str| {
        (b == buttons::LEFT && has(buttons::RIGHT))
            || (b == buttons::PAGE_BACK && has(buttons::PAGE_FORWARD))
            || (b == buttons::UP && has(buttons::DOWN))
    };
    let pressed: Vec<&str> = variant
        .profile
        .buttons
        .iter()
        .copied()
        .filter(|b| *b != buttons::POWER)
        .collect();
    let mut keys: Vec<String> = pressed
        .iter()
        .filter(|b| !backward(b))
        .map(|b| b.to_string())
        .collect();
    keys.extend(pressed.iter().map(|b| format!("long:{b}")));
    keys
}

/// The buttons that move the focus, of those `variant`'s profile has.
pub fn movers(variant: &Variant) -> Vec<String> {
    [
        buttons::RIGHT,
        buttons::LEFT,
        buttons::DOWN,
        buttons::UP,
        buttons::PAGE_FORWARD,
        buttons::PAGE_BACK,
    ]
    .iter()
    .filter(|b| variant.profile.buttons.contains(b))
    .map(|b| b.to_string())
    .collect()
}

/// What goes back: the back button, else a long press of page back (as
/// the two-button key map has it).
pub fn back_key(variant: &Variant) -> Option<String> {
    let has = |b: &str| variant.profile.buttons.contains(&b);
    if has(buttons::BACK) {
        Some(buttons::BACK.to_string())
    } else if has(buttons::PAGE_BACK) {
        Some(format!("long:{}", buttons::PAGE_BACK))
    } else {
        None
    }
}

/// A session driven by steps: presses (`confirm`, `long:confirm`) and
/// screens opened directly (`open:<screen>`), able to go back to where a
/// way of steps leads.
#[derive(Debug)]
pub struct Driver<'a> {
    context: &'a Context,
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
        }
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
    }

    /// Back to the home screen, as it was at the start where it can be.
    pub fn home(&mut self) {
        cases::home(&mut self.session);
        self.last = None;
    }

    /// Goes where `way` leads from the home screen and says what is seen,
    /// if that is `state` (any, if `None`); with a fresh session if the
    /// way led elsewhere (what the steps before changed, e.g. settings).
    pub fn go(&mut self, way: &[String], state: Option<&str>) -> Option<Seen> {
        for fresh in [false, true] {
            if fresh {
                self.fresh += 1;
                match Session::new(self.context, &self.variant) {
                    Ok(session) => self.session = session,
                    Err(_) => return None,
                }
                self.last = None;
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

/// What the search found.
#[derive(Debug, Default)]
pub struct Found {
    /// Each screen seen, with the shortest way to it and the state it
    /// was seen in (see [`Seen::state`]).
    pub ways: HashMap<String, (Vec<String>, String)>,
    /// Screens (with their parts) searched.
    pub views: usize,
    /// Whether every one there was within the bounds was searched.
    pub complete: bool,
    /// Screens that describe no parts but show a focus moving: the way
    /// to each, the button that moved it, and the picture then.
    pub undescribed: HashMap<String, (Vec<String>, String, Image)>,
}

/// The buttons that move the focus forward, of those `variant`'s
/// profile has: to walk through a screen's rows.
pub fn forward(variant: &Variant) -> Vec<String> {
    [buttons::RIGHT, buttons::DOWN, buttons::PAGE_FORWARD]
        .iter()
        .filter(|b| variant.profile.buttons.contains(b))
        .map(|b| b.to_string())
        .collect()
}

/// The presses that act on the part with the focus (rather than move
/// it, or go back): confirm; on a device without a confirm button, the
/// long presses (but back's).
pub fn actions(variant: &Variant) -> Vec<String> {
    let confirm = variant.profile.buttons.contains(&buttons::CONFIRM);
    let movers = movers(variant);
    let back = back_key(variant);
    if confirm {
        return vec![buttons::CONFIRM.to_string()];
    }
    keys(variant)
        .into_iter()
        .filter(|k| !movers.iter().any(|m| m == k) && Some(k) != back.as_ref())
        .collect()
}

/// Searches the ways from the home screen until `wanted` screens are
/// all found (or the bounds are reached), breadth first: on each screen
/// newly seen every button is pressed once, then the focus is walked
/// forward through it and on each part the focus reaches what acts is
/// pressed (see [`actions`]).
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn search(driver: &mut Driver<'_>, wanted: &HashSet<String>, limits: &Limits) -> Found {
    let variant = driver.variant.clone();
    let (keys, forward, actions) = (keys(&variant), forward(&variant), actions(&variant));
    let back = back_key(&variant);
    let mut found = Found::default();
    let Some(start) = driver.go(&[], None) else {
        return found;
    };
    let mut views: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(Vec<String>, Seen)> = VecDeque::new();
    let mut note = |seen: &Seen,
                    way: Vec<String>,
                    found: &mut Found,
                    queue: &mut VecDeque<(Vec<String>, Seen)>| {
        found
            .ways
            .entry(seen.top().to_string())
            .or_insert_with(|| (way.clone(), seen.state()));
        if way.len() < limits.presses && views.insert(seen.view()) {
            queue.push_back((way, seen.clone()));
        }
    };
    note(&start, Vec::new(), &mut found, &mut queue);
    let mut current = start.clone();
    found.complete = true;
    let longer = |way: &[String], step: &str| {
        let mut longer = way.to_vec();
        longer.push(step.to_string());
        longer
    };
    // In the order seen, but a screen searched [`SAME_PLACE`] times in
    // the same place (over the same screen) with other parts each time
    // (another group of settings) after the others.
    let mut places: HashMap<String, usize> = HashMap::new();
    let mut idle = 0;
    let place = |seen: &Seen| seen.stack[seen.stack.len().saturating_sub(2)..].join(">");
    loop {
        let next = queue
            .iter()
            .enumerate()
            .min_by_key(|(i, (_, seen))| {
                let here = places.get(&place(seen)).copied().unwrap_or(0);
                (here >= SAME_PLACE, *i)
            })
            .map(|(i, _)| i);
        let Some((way, seen)) = next.and_then(|i| queue.remove(i)) else {
            break;
        };
        *places.entry(place(&seen)).or_insert(0) += 1;
        if wanted.iter().all(|w| found.ways.contains_key(w)) {
            found.complete = queue.is_empty();
            break;
        }
        if found.views >= limits.views.max(wanted.len() * 3 / 2) || idle >= limits.idle {
            found.complete = false;
            break;
        }
        found.views += 1;
        let known = found.ways.len();
        let state = seen.state();
        // Every button once.
        for key in &keys {
            let Some(_) = driver.arrive(&current, &way, &state, None, back.as_deref()) else {
                break;
            };
            driver.step(key);
            current = driver.observe();
            note(&current, longer(&way, key), &mut found, &mut queue);
        }
        // From the home screen, sleep: what it shows then was reached
        // (the device is woken again at once).
        if way.is_empty()
            && variant.profile.buttons.contains(&buttons::POWER)
            && driver
                .arrive(&current, &way, &state, None, back.as_deref())
                .is_some()
        {
            driver.step(buttons::POWER);
            current = driver.observe();
            for screen in &driver.asleep {
                found
                    .ways
                    .entry(screen.clone())
                    .or_insert_with(|| (vec![buttons::POWER.to_string()], String::new()));
            }
        }
        // Nothing described, but something like a focus moves: the
        // search cannot follow it.
        if seen.targets().next().is_none()
            && !found.undescribed.contains_key(seen.top())
            && let Some(mover) = forward.first()
            && driver
                .arrive(&current, &way, &state, None, back.as_deref())
                .is_some()
        {
            let before = driver.session.image();
            driver.step(mover);
            current = driver.observe();
            let after = driver.session.image();
            if current.stack == seen.stack
                && current.targets().next().is_none()
                && focus_like(&before, &after)
            {
                found
                    .undescribed
                    .insert(seen.top().to_string(), (way.clone(), mover.clone(), after));
            }
        }
        // The parts the focus reaches, one press of a forward button
        // after the other, and what acts on each.
        let mut focused: HashSet<Option<String>> = HashSet::new();
        focused.insert(seen.focused().map(name));
        for mover in &forward {
            let mut before = state.clone();
            let mut at = way.clone();
            for _ in 0..limits.focus {
                let Some(_) = driver.arrive(&current, &at, &before, None, back.as_deref()) else {
                    break;
                };
                driver.step(mover);
                at.push(mover.clone());
                current = driver.observe();
                if current.stack != seen.stack {
                    note(&current, at.clone(), &mut found, &mut queue);
                    break;
                }
                if !focused.insert(current.focused().map(name)) {
                    break;
                }
                let here = current.state();
                for action in &actions {
                    let Some(_) =
                        driver.arrive(&current, &at, &here, Some(&before), back.as_deref())
                    else {
                        break;
                    };
                    driver.step(action);
                    current = driver.observe();
                    note(&current, longer(&at, action), &mut found, &mut queue);
                }
                before = here;
            }
        }
        idle = if found.ways.len() > known {
            0
        } else {
            idle + 1
        };
    }
    found
}

/// Whether what changed from `before` to `after` looks like a focus
/// that moved: something changed, within less than a third of the screen
/// (a page turned changes nearly all of it).
fn focus_like(before: &Image, after: &Image) -> bool {
    if (before.width, before.height) != (after.width, after.height) {
        return false;
    }
    let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..before.height {
        for x in 0..before.width {
            if before.pixel(x, y) != after.pixel(x, y) {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    if left > right {
        return false;
    }
    let changed = u64::from(right - left + 1) * u64::from(bottom - top + 1);
    changed * 3 < u64::from(before.width) * u64::from(before.height)
}

/// How moving the focus through a screen went.
#[derive(Debug, Default)]
pub struct Walked {
    /// Every target seen, by [`name`], with where it was last seen.
    pub targets: HashMap<String, Rect>,
    /// Targets that had the focus.
    pub focused: HashSet<String>,
    /// Targets inside others, with those they are in.
    pub inside: HashMap<String, HashSet<String>>,
    /// Where the focus stopped and no button moved it on: the part, and
    /// the picture then.
    pub trap: Option<(Part, Image)>,
    /// A picture of the screen while the focus was moved.
    pub picture: Option<Image>,
    /// Whether every place the focus can go was found (within
    /// [`Limits::walk`] presses).
    pub complete: bool,
}

impl Walked {
    fn saw(&mut self, seen: &Seen) {
        let targets: Vec<&Part> = seen.targets().filter(|p| takes_focus(p)).collect();
        for target in &targets {
            self.targets.insert(name(target), target.area);
            for outer in &targets {
                if outer.area != target.area && inside(target.area, outer.area) {
                    self.inside
                        .entry(name(target))
                        .or_default()
                        .insert(name(outer));
                }
            }
        }
        if let Some(part) = seen.focused()
            && is_target(part)
        {
            self.focused.insert(name(part));
        }
    }

    /// The targets the focus never reached, nor one they are in.
    pub fn skipped(&self) -> Vec<String> {
        let mut skipped: Vec<String> = self
            .targets
            .keys()
            .filter(|t| {
                !self.focused.contains(*t)
                    && !self
                        .inside
                        .get(*t)
                        .is_some_and(|outer| outer.iter().any(|o| self.focused.contains(o)))
            })
            .cloned()
            .collect();
        skipped.sort();
        skipped
    }
}

/// Moves the focus through the screen `way` leads to (seen as `state`)
/// with `movers`, until every place it can go is found: each
/// mover is pressed in each place once, the device going along the moves
/// learnt to the nearest place with one not pressed yet (a keyboard's
/// keys are reached with left and right in a row, the page buttons
/// between rows).
pub fn walk(
    driver: &mut Driver<'_>,
    way: &[String],
    state: &str,
    movers: &[String],
    limits: &Limits,
) -> Option<Walked> {
    let mut walked = Walked {
        complete: true,
        ..Walked::default()
    };
    let start = driver.go(way, Some(state))?;
    let screen = start.stack.clone();
    walked.saw(&start);
    // Where each mover leads from each place; `None`: off the screen.
    let mut moves: HashMap<String, HashMap<String, Option<String>>> = HashMap::new();
    let mut current = start.clone();
    let mut presses = 0;
    loop {
        if presses >= limits.walk {
            walked.complete = false;
            break;
        }
        let here = current.spot();
        let tried = moves.entry(here.clone()).or_default();
        if let Some(mover) = movers.iter().find(|m| !tried.contains_key(*m)).cloned() {
            driver.step(&mover);
            presses += 1;
            let next = driver.observe();
            let left = next.stack != screen;
            moves
                .entry(here)
                .or_default()
                .insert(mover, (!left).then(|| next.spot()));
            if left {
                current = driver.go(way, Some(&start.state()))?;
                continue;
            }
            walked.saw(&next);
            if walked.picture.is_none() && next.focused().is_some() {
                walked.picture = Some(driver.session.image());
            }
            current = next;
            continue;
        }
        // Every mover pressed here: caught, if none moved the focus.
        if walked.trap.is_none()
            && current.targets().filter(|p| takes_focus(p)).count() > 1
            && let Some(part) = current.focused().filter(|p| is_target(p))
            && moves[&here]
                .values()
                .all(|to| to.as_deref() == Some(here.as_str()))
        {
            walked.trap = Some((part.clone(), driver.session.image()));
        }
        let Some(path) = nearest_untried(&moves, &here, movers) else {
            break;
        };
        for mover in &path {
            driver.step(mover);
            presses += 1;
        }
        current = driver.observe();
        if current.stack != screen {
            current = driver.go(way, Some(&start.state()))?;
        }
    }
    Some(walked)
}

/// The movers that lead, along the moves learnt, from `from` to the
/// nearest place where one of `movers` was not pressed yet.
fn nearest_untried(
    moves: &HashMap<String, HashMap<String, Option<String>>>,
    from: &str,
    movers: &[String],
) -> Option<Vec<String>> {
    let untried = |place: &str| {
        moves
            .get(place)
            .is_none_or(|tried| tried.len() < movers.len())
    };
    let mut queue: VecDeque<(String, Vec<String>)> =
        VecDeque::from([(from.to_string(), Vec::new())]);
    let mut known: HashSet<String> = HashSet::from([from.to_string()]);
    while let Some((place, path)) = queue.pop_front() {
        if !path.is_empty() && untried(&place) {
            return Some(path);
        }
        let Some(tried) = moves.get(&place) else {
            continue;
        };
        for mover in movers {
            if let Some(Some(to)) = tried.get(mover)
                && known.insert(to.clone())
            {
                let mut longer = path.clone();
                longer.push(mover.clone());
                queue.push_back((to.clone(), longer));
            }
        }
    }
    None
}

/// The plugin a screen belongs to: the one looked at, else its namespace.
fn plugin_of(context: &Context, screen: &str) -> String {
    context
        .target
        .owner(screen)
        .map(String::from)
        .unwrap_or_else(|| screen.split('.').next().unwrap_or(screen).to_string())
}

/// Saves `image` for a finding about `screen`; its path in the report.
fn picture(
    context: &Context,
    variant: &Variant,
    screen: &str,
    what: &str,
    image: &Image,
) -> Option<String> {
    if context.in_memory {
        return None;
    }
    let path = format!("reach/{}/{screen}-{what}.png", variant.slug());
    image.save_png(&context.path(&path)).ok()?;
    Some(path)
}

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let variants = context.variants_per_profile();
    let buttons_only: Vec<&Variant> = variants
        .iter()
        .filter(|v| !v.has_touch() && v.profile.buttons.iter().any(|b| *b != buttons::POWER))
        .collect();
    if buttons_only.is_empty() {
        report.add(Finding::info(
            "reach.none",
            "reach",
            "None of the profiles looked at has buttons and no touch screen: buttons alone \
             are not checked.",
        ));
    }
    for variant in alike(&buttons_only, report) {
        check(context, variant, &Limits::default(), report)?;
    }
    let touch: Vec<&Variant> = variants.iter().filter(|v| v.has_touch()).collect();
    for variant in alike(&touch, report) {
        taps(context, variant, report)?;
    }
    Ok(())
}

/// What makes devices go the same ways: their buttons, their screen's
/// size.
type Alike = (&'static [&'static str], u16, u16);

/// `variants` but those with the same buttons and screen size as one
/// before them, which go the same ways (noted).
fn alike<'v>(variants: &[&'v Variant], report: &mut Report) -> Vec<&'v Variant> {
    let mut checked: Vec<(&Variant, Alike)> = Vec::new();
    for &variant in variants {
        let device = &variant.profile.device;
        let alike = (
            variant.profile.buttons,
            device.display.width,
            device.display.height,
        );
        if let Some((first, _)) = checked.iter().find(|(_, a)| *a == alike) {
            report.add(
                Finding::info(
                    "reach.alike",
                    "reach",
                    format!(
                        "Same buttons and screen as {}: checked there.",
                        first.profile.device.id
                    ),
                )
                .variant(variant.name()),
            );
            continue;
        }
        checked.push((variant, alike));
    }
    checked.into_iter().map(|(v, _)| v).collect()
}

/// On a touch screen: for every screen of the target that opens alone,
/// a tap where `tap-on:<text>` taps (as flows and recordings name parts)
/// lands on the row, button, switch or slider with that text, for each
/// of them (those inside a row go with it).
pub fn taps(context: &Context, variant: &Variant, report: &mut Report) -> Result<(), String> {
    let mut session = Session::new(context, variant)?;
    for screen in cases::screens(&session, &context.target) {
        cases::home(&mut session);
        session.open(&screen, &[])?;
        if session.screen().as_deref() != Some(screen.as_str()) {
            continue;
        }
        let own = semantics::parts(session.device.kernel.described_active());
        let all = session.parts();
        let mut elsewhere = Vec::new();
        let mut first = None;
        for target in own.iter().filter(|p| is_target(p)) {
            let nested = own
                .iter()
                .any(|o| is_target(o) && o.area != target.area && inside(target.area, o.area));
            if nested {
                continue;
            }
            let text = target.label.split(", ").next().unwrap_or_default().trim();
            let lands = semantics::find(&all, text).is_some_and(|found| {
                let a = found.area;
                let x = u32::from(a.x) + u32::from(a.width) / 2;
                let y = u32::from(a.y) + u32::from(a.height) / 2;
                let t = target.area;
                x >= u32::from(t.x) && y >= u32::from(t.y) && x < t.right() && y < t.bottom()
            });
            if !lands && !elsewhere.contains(&format!("\"{text}\"")) {
                elsewhere.push(format!("\"{text}\""));
                first.get_or_insert(target.area);
            }
        }
        if elsewhere.is_empty() {
            continue;
        }
        // Not wrong for who taps, but for flows and recordings.
        let mut finding = Finding::info(
            "reach.touch-label",
            screen.clone(),
            format!(
                "A tap by the text of {} (`tap-on:`, as flows and recordings name it) lands on \
                 another part, one with the same text or beginning so: give each a text of its \
                 own.",
                elsewhere.join(", ")
            ),
        )
        .plugin(plugin_of(context, &screen))
        .variant(variant.name());
        if let Some(path) = picture(context, variant, &screen, "touch-label", &session.image()) {
            finding = finding.picture(path);
            if let Some(area) = first {
                finding = finding.area(area);
            }
        }
        report.add(finding);
    }
    Ok(())
}

/// Checks every screen of the target on `variant`, within `limits`.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn check(
    context: &Context,
    variant: &Variant,
    limits: &Limits,
    report: &mut Report,
) -> Result<(), String> {
    // The emulator's clock: a browser's too (std's has none there).
    let started = paperpaw_emulator::host::micros();
    let mut driver = Driver::new(context, variant)?;
    let screens = cases::screens(&driver.session, &context.target);
    let home = driver.session.stack().first().cloned().unwrap_or_default();
    // Those that open alone are searched until found; those that need
    // parameters only come along; those that go by themselves (notices)
    // are not looked for.
    let mut alone: HashSet<String> = HashSet::new();
    let mut passing: HashSet<String> = HashSet::new();
    for screen in &screens {
        // Alone, with nothing to show, it is waiting for its parameters.
        if driver
            .go(&[format!("open:{screen}")], None)
            .is_some_and(|s| s.top() == screen && !s.parts.is_empty())
        {
            driver.step(&format!("wait:{PASSING_MS}"));
            if driver.observe().top() == screen {
                alone.insert(screen.clone());
            } else {
                passing.insert(screen.clone());
            }
        }
    }
    let found = search(&mut driver, &alone, limits);
    let searched = driver.steps;
    let mut undescribed: Vec<_> = found
        .undescribed
        .iter()
        .filter(|(screen, _)| context.target.ours(screen.as_str()))
        .collect();
    undescribed.sort_by_key(|(screen, _)| screen.as_str());
    for (screen, (way, mover, image)) in undescribed {
        let way = if way.is_empty() {
            String::from("on the home screen")
        } else {
            format!("after {}", way.join(", "))
        };
        let mut finding = Finding::warning(
            "reach.undescribed",
            screen.clone(),
            format!(
                "Pressing `{mover}` ({way}) moves something like a focus on it, but it \
                 describes no rows or buttons, so its parts cannot be told apart, read aloud \
                 or followed: draw them with the ui helpers, or tell them with \
                 canvas.describe."
            ),
        )
        .plugin(plugin_of(context, screen))
        .variant(variant.name());
        if let Some(path) = picture(context, variant, screen, "undescribed", image) {
            finding = finding.picture(path);
        }
        report.add(finding);
    }
    // What the search changed (settings, files) is left behind.
    let asleep = driver.asleep.clone();
    let mut driver = Driver::new(context, variant)?;
    let place = |finding: Finding, screen: &str| {
        finding
            .plugin(plugin_of(context, screen))
            .variant(variant.name())
    };
    for screen in &screens {
        if asleep.contains(screen) {
            // Shown while the device sleeps: power closes it.
            continue;
        }
        if let Some(setting) = chosen_by(&driver.session, screen) {
            let note = Finding::info(
                "reach.chosen",
                screen.clone(),
                format!(
                    "Shown in another screen's place where the setting `{setting}` chooses \
                     it: reached through that setting."
                ),
            );
            report.add(place(note, screen));
            continue;
        }
        let (way, state) = match found.ways.get(screen) {
            Some((way, state)) => (way.clone(), state.clone()),
            None if passing.contains(screen) => {
                let note = Finding::info(
                    "reach.not-reached",
                    screen.clone(),
                    "It goes by itself (a notice): not looked for.",
                );
                report.add(place(note, screen));
                continue;
            }
            None if !alone.contains(screen) => {
                let note = Finding::info(
                    "reach.not-reached",
                    screen.clone(),
                    "It was not reached with the buttons alone: it needs parameters, and no \
                     screen found in the sample library opened it.",
                );
                report.add(place(note, screen));
                continue;
            }
            None => {
                // Pictured opened directly, as a plugin would; with the
                // fixtures, what it asked for that they did not answer.
                let open = vec![format!("open:{screen}")];
                let _ = paperpaw_emulator::offline::unanswered();
                let Some(_) = driver.go(&open, None) else {
                    continue;
                };
                let wanted = paperpaw_emulator::offline::unanswered();
                let mut unreachable = if context.fixtures && !wanted.is_empty() {
                    Finding::error(
                        "reach.unreachable",
                        screen.clone(),
                        format!(
                            "It could not be reached from the home screen with the buttons \
                             alone, and it asked for data the emulator's fixtures do not give \
                             ({}): give it an answer in shells/desktop/src/offline.rs, or a way \
                             in without.",
                            wanted.join(", ")
                        ),
                    )
                } else {
                    Finding::warning(
                        "reach.unreachable",
                        screen.clone(),
                        format!(
                            "It could not be reached from the home screen with the buttons \
                             alone ({} screens searched, up to {} presses): give it a way in, \
                             such as a menu entry or a row of another screen.",
                            found.views, limits.presses
                        ),
                    )
                };
                if let Some(path) = picture(
                    context,
                    variant,
                    screen,
                    "unreachable",
                    &driver.session.image(),
                ) {
                    unreachable = unreachable.picture(path);
                }
                // Closing and the focus are looked at where it is reached
                // (opened without what its way gives it, it may show
                // little of itself).
                report.add(place(unreachable, screen));
                continue;
            }
        };
        if let Some(finding) = closes(&mut driver, &way, &state, screen, &home) {
            report.add(place(finding, screen));
        }
        let movers = movers(variant);
        let Some(walked) = walk(&mut driver, &way, &state, &movers, limits) else {
            continue;
        };
        for finding in focus(context, variant, screen, &walked, &movers, &driver) {
            report.add(place(finding, screen));
        }
    }
    report.add(
        Finding::info(
            "reach.searched",
            "reach",
            format!(
                "{} screens checked; the search looked at {} screens{} in {:.1} s ({} steps \
                 searching, {} checking, {} fresh sessions).",
                screens.len(),
                found.views,
                if found.complete {
                    ", all there were"
                } else {
                    ""
                },
                (paperpaw_emulator::host::micros() - started) as f32 / 1e6,
                searched,
                driver.steps,
                driver.fresh
            ),
        )
        .variant(variant.name()),
    );
    Ok(())
}

/// Presses of back that may be needed to close a screen (one that goes
/// back a folder at a time).
const BACKS: usize = 5;

/// Whether the screen `way` leads to closes with back: the stack
/// shrinks, the home screen comes back, or a question comes up (whether
/// to leave), within [`BACKS`] presses. A finding if not.
fn closes(
    driver: &mut Driver<'_>,
    way: &[String],
    state: &str,
    screen: &str,
    home: &str,
) -> Option<Finding> {
    let back = back_key(&driver.variant)?;
    let before = driver.go(way, Some(state))?;
    if before.top() != screen || before.stack.len() < 2 {
        return None;
    }
    let mut after = before.clone();
    for _ in 0..BACKS {
        driver.step(&back);
        after = driver.observe();
        let closed = after.stack.len() < before.stack.len()
            || after.top() == home
            || (after.stack.len() > before.stack.len() && after.stack.starts_with(&before.stack));
        if closed {
            return None;
        }
        if after.stack != before.stack {
            break;
        }
    }
    let mut finding = Finding::error(
        "reach.no-close",
        screen.to_string(),
        format!(
            "Pressing `{back}` does not close it (the screen shown then is `{}`): back must \
             close every screen, or go home.",
            after.top()
        ),
    );
    if let Some(path) = picture(
        driver.context,
        &driver.variant,
        screen,
        "no-close",
        &driver.session.image(),
    ) {
        finding = finding.picture(path);
    }
    Some(finding)
}

/// What moving the focus through `screen` found: no focus shown, parts
/// it never reaches (if every place it goes was found), a trap.
fn focus(
    context: &Context,
    variant: &Variant,
    screen: &str,
    walked: &Walked,
    movers: &[String],
    driver: &Driver<'_>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    // One part alone is what confirm acts on, focus shown or not.
    if walked.targets.len() < 2 {
        return findings;
    }
    let buttons = movers.join(", ");
    let image = walked
        .picture
        .clone()
        .unwrap_or_else(|| driver.session.image());
    if walked.focused.is_empty() {
        let mut none = Finding::warning(
            "reach.focus-skips",
            screen.to_string(),
            format!(
                "The buttons ({buttons}) never show a focus on its {} rows, buttons and \
                 switches: move a visible focus with them.",
                walked.targets.len()
            ),
        );
        if let Some(path) = picture(context, variant, screen, "no-focus", &image) {
            none = none.picture(path);
        }
        findings.push(none);
    } else if walked.complete {
        let skipped = walked.skipped();
        if !skipped.is_empty() {
            let mut skips = Finding::warning(
                "reach.focus-skips",
                screen.to_string(),
                format!(
                    "Moving the focus with the buttons ({buttons}) never reaches {}: every \
                     part that acts must take the focus.",
                    skipped.join(", ")
                ),
            );
            if let Some(path) = picture(context, variant, screen, "focus-skips", &image) {
                skips = skips.picture(path);
                if let Some(area) = walked.targets.get(&skipped[0]) {
                    skips = skips.area(*area);
                }
            }
            findings.push(skips);
        }
    }
    if let Some((part, image)) = &walked.trap {
        let mut trap = Finding::error(
            "reach.focus-trap",
            screen.to_string(),
            format!(
                "The focus stops on {} and none of the buttons ({buttons}) moves it on.",
                name(part)
            ),
        )
        .area(part.area);
        if let Some(path) = picture(context, variant, screen, "focus-trap", image) {
            trap = trap.picture(path);
        }
        findings.push(trap);
    }
    findings
}

/// The setting that offers `screen` among its choices, if one does: a
/// mode of another screen, shown in its place (the sleep screen's).
pub fn chosen_by(session: &Session, screen: &str) -> Option<String> {
    use paperpaw_core::sdk::SettingKind;
    session
        .device
        .kernel
        .settings()
        .specs()
        .find(|spec| match &spec.kind {
            SettingKind::Choice(choices) => choices.iter().any(|value| value == screen),
            _ => false,
        })
        .map(|spec| spec.id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(role: Role, label: &str, area: Rect, focused: bool) -> Part {
        Part::new(role, label.to_string(), area).focused(focused)
    }

    #[test]
    fn buttons_are_chosen_by_profile() {
        let x4 = Variant::x4();
        let pressed = keys(&x4);
        assert!(pressed.contains(&"right".to_string()));
        assert!(!pressed.contains(&"left".to_string()), "{pressed:?}");
        assert!(pressed.contains(&"long:confirm".to_string()));
        assert!(!pressed.iter().any(|k| k.contains("power")));
        assert_eq!(back_key(&x4).as_deref(), Some("back"));
        assert!(movers(&x4).contains(&"page-back".to_string()));
        let two = Variant::of(paperpaw_emulator::profiles::find("two-buttons").unwrap());
        assert_eq!(
            keys(&two),
            ["page-forward", "long:page-back", "long:page-forward"]
        );
        assert_eq!(back_key(&two).as_deref(), Some("long:page-back"));
    }

    #[test]
    fn parts_inside_rows_count_with_them() {
        let row = Rect::new(0, 100, 400, 60);
        let seen = |focus: usize| Seen {
            stack: vec!["a.list".into()],
            hints: vec!["Back".into()],
            parts: vec![
                part(Role::Heading, "Title", Rect::new(0, 0, 400, 60), false),
                part(Role::Row, "Light, 4", row, focus == 1),
                part(Role::Button, "+", Rect::new(340, 110, 40, 40), false),
                part(
                    Role::Row,
                    "Size, 12",
                    Rect::new(0, 160, 400, 60),
                    focus == 2,
                ),
                part(Role::Button, "Done", Rect::new(0, 300, 100, 60), false),
            ],
        };
        let mut walked = Walked::default();
        walked.saw(&seen(0));
        walked.saw(&seen(1));
        // The light's value changed: still the same row.
        let mut changed = seen(2);
        changed.parts[1].label = "Light, 5".into();
        walked.saw(&changed);
        assert_eq!(walked.skipped(), ["Button \"Done\""]);
        assert_eq!(seen(1).state(), "a.list|Row \"Light\"|Back");
        assert_eq!(seen(1).view(), seen(2).view());
    }
}
