//! The touch lock: while it is on, taps, swipes and the pen do nothing,
//! so that a hand resting on the screen turns no page; the buttons go on
//! working. Off by default.
//!
//! - One switch, `lock.touch`, in the quick panel and the reader's menu
//!   (and Settings › Controls), on touch screens only; *Lock touch*
//!   (`lock.touch-toggle`) switches it from any button or rule.
//! - An input filter takes every finger and pen before screens, gestures
//!   and key maps see it, on every screen; not while the screen lock asks
//!   for its code.
//! - A way out that needs no menu and that a resting hand does not make
//!   (`Escape`): one finger taps twice and stays down on the second tap
//!   (a third touch held after two taps counts too); or, where
//!   PaperPaw is the firmware, the power button held, or the home key (the
//!   X4 Pro's, below the screen) held. A small lock in the status lines
//!   says it is on. Its times allow for a slow screen: a refresh may hold
//!   the touches back that came meanwhile.
//! - Said whenever it turns on or off, however it was switched (the
//!   setting's change); and while it is on, a touch that is no step of
//!   the way out says again how to unlock, at most every [`HINT_MS`], as
//!   a notice (no full refresh): never in the middle of the way out, whose
//!   taps a refresh would hold back.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, menus};
use paperpaw_sdk::filters::{InputFilterSpec, InputVerdict};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::status::{StatusItemSpec, StatusValue};
use paperpaw_sdk::{
    Command, Env, FixedAction, Id, InputEvent, Instant, MenuEntry, MenuNeed, Point, PointerPhase,
    RegisterError, Registrar, SettingSpec,
};

use crate::Shared;

/// Whether touch is locked.
pub const LOCKED: Setting<bool> = Setting::new("lock.touch");
/// Locks or unlocks touch.
pub const TOGGLE: Id = Id::new_static("lock.touch-toggle");
/// The lock in the status lines while touch is locked.
pub const STATUS: Id = Id::new_static("lock.touch-status");
/// Says how to unlock touch.
pub const HINT: Id = Id::new_static("lock.touch-hint");
/// A touch while locked says how to unlock at most this often.
pub const HINT_MS: u64 = 10_000;
const FILTER: Id = Id::new_static("lock.touch-filter");

/// The way out by touch, which a hand resting on the screen does not do:
/// one finger taps, then comes down again at the same spot and stays
/// there for [`HOLD_MS`] (as it says: tap twice and hold the second
/// tap), the tap shorter than [`TAP_MS`] and the next touch within
/// [`GAP_MS`]; after two taps the third touch held counts too. A second
/// finger on the screen starts it over. It unlocks when the finger is
/// lifted, or before, as it moves a little once held long enough.
pub const TAP_MS: u64 = 500;
pub const GAP_MS: u64 = 1_000;
pub const HOLD_MS: u64 = 1_500;
/// How far the finger may stray from the first tap, in pixels.
const SLOP: i32 = 40;

/// How far the way out has come.
#[derive(Debug, Default)]
struct Escape {
    /// The fingers down now, by id.
    down: Vec<u8>,
    /// The finger that may go on with it: its id, when and where it came
    /// down.
    finger: Option<(u8, Instant, Point)>,
    /// Taps so far (two at most), where the first was, when the last
    /// ended.
    taps: u8,
    first: Option<Point>,
    last_up: Option<Instant>,
}

impl Escape {
    fn over(&mut self) {
        self.finger = None;
        self.taps = 0;
        self.first = None;
        self.last_up = None;
    }

    fn near(&self, point: Point) -> bool {
        self.first
            .is_none_or(|at| (point.x - at.x).abs() <= SLOP && (point.y - at.y).abs() <= SLOP)
    }

    /// Follows `pointer` (a finger or the pen) at `now`: whether it
    /// completed the way out.
    fn follow(&mut self, now: Instant, pointer: &paperpaw_sdk::PointerEvent) -> bool {
        let id = pointer.id;
        match pointer.phase {
            PointerPhase::Down => {
                if !self.down.contains(&id) {
                    self.down.push(id);
                }
                let late = self.last_up.is_some_and(|up| now.since(up) > GAP_MS);
                if self.down.len() > 1 || late || !self.near(pointer.point) {
                    self.over();
                }
                if self.down.len() == 1 {
                    self.finger = Some((id, now, pointer.point));
                    self.first.get_or_insert(pointer.point);
                }
            }
            PointerPhase::Move => {
                let Some((f, at, _)) = self.finger else {
                    return false;
                };
                if f != id {
                    return false;
                }
                if !self.near(pointer.point) {
                    self.over();
                } else if self.taps > 0 && now.since(at) >= HOLD_MS {
                    // Held long enough, still down: no need to lift.
                    self.down.clear();
                    self.over();
                    return true;
                }
            }
            PointerPhase::Up => {
                self.down.retain(|d| *d != id);
                let Some((f, at, _)) = self.finger.take() else {
                    return false;
                };
                if f != id {
                    self.over();
                    return false;
                }
                let held = now.since(at);
                if held < TAP_MS && self.taps < 2 {
                    self.taps += 1;
                    self.last_up = Some(now);
                } else if held >= HOLD_MS && self.taps > 0 {
                    self.down.clear();
                    self.over();
                    return true;
                } else {
                    self.over();
                }
            }
            // A cancel ends every touch: the Android shell sends it for one
            // pointer alone, and the others left would count as a second
            // finger from then on.
            _ => {
                self.down.clear();
                self.over();
            }
        }
        false
    }
}

/// The switch, with the controls.
pub(crate) static SETTINGS: [SettingSpec; 1] =
    [
        SettingSpec::switch(LOCKED.static_str(), "lock.touch", false, crate::GROUP)
            .description("lock.touch.help"),
    ];

/// While touch is locked, the power button held unlocks it, and the home
/// key held (the X4 Pro's touch key below the screen).
const ESCAPE: &str =
    "[keymap.global]\nlong.power = lock.touch-toggle\nlong.home = lock.touch-toggle\n";

/// What the filter does with `event` while touch is locked: taken, the
/// toggle once the way out is done, or how to unlock said where it was
/// not said for [`HINT_MS`] and the touch is no step of the way out: one
/// that ends without being one of its taps (a swipe, a hand resting, a
/// hold too short), or one that comes long after a tap (the way out given
/// up). Never between its steps: the refresh would hold the next back.
fn locked(
    now: Instant,
    event: &InputEvent,
    escape: &mut Escape,
    hinted: &mut Option<Instant>,
) -> InputVerdict {
    let InputEvent::Pointer(pointer) = event else {
        return InputVerdict::Pass;
    };
    let taps = escape.taps;
    let given_up = escape.last_up.is_some_and(|up| now.since(up) > GAP_MS);
    if escape.follow(now, pointer) {
        return InputVerdict::Run(TOGGLE);
    }
    let no_step = match pointer.phase {
        PointerPhase::Up => escape.taps <= taps && escape.finger.is_none(),
        PointerPhase::Down => taps > 0 && given_up,
        _ => false,
    };
    if no_step && hinted.is_none_or(|at| now.since(at) >= HINT_MS) {
        *hinted = Some(now);
        return InputVerdict::Run(HINT);
    }
    InputVerdict::Take
}

/// How to unlock, where PaperPaw is the firmware (the power button) or not.
fn how_to_unlock(env: &Env<'_>) -> &'static str {
    if env.caps().firmware {
        "lock.touch.locked.buttons"
    } else {
        "lock.touch.locked"
    }
}

static ACTIONS: [FixedAction; 2] = [
    FixedAction::new(TOGGLE, "lock.touch-toggle").handler(|env, _, out| {
        // Said as the setting changes (below), as for any switch.
        out.push(Command::set(&LOCKED, !env.settings().read(&LOCKED)));
    }),
    FixedAction::new(HINT, "lock.touch-hint")
        .handler(|env, _, out| out.push(Command::Notify(env.i18n().tr(how_to_unlock(env)).into()))),
];
/// Beside night mode in the quick panel.
static ENTRIES: [MenuEntry; 2] = [
    MenuEntry::new(menus::QUICK, Id::new_static(LOCKED.static_str())).order(11),
    MenuEntry::new(menus::READER, Id::new_static(LOCKED.static_str())).order(60),
];
static ICONS: [(Id, &str); 2] = [
    (TOGGLE, "lock"),
    (Id::new_static(LOCKED.static_str()), "lock"),
];

/// Adds the touch lock; `state` is the screen lock's, whose code entry
/// it leaves alone.
pub(crate) fn register(r: &mut Registrar, state: Shared) -> Result<(), RegisterError> {
    r.add_settings(&SETTINGS)?;
    r.add_actions(&ACTIONS)?;
    r.add_icons(&ICONS)?;
    // However it was switched: the quick panel, the reader's menu,
    // Settings, the toggle, a rule.
    r.on(events::SETTING_CHANGED, |env, changed, out| {
        if changed.id != *LOCKED.id() {
            return;
        }
        let told = if env.settings().read(&LOCKED) {
            how_to_unlock(env)
        } else {
            "lock.touch.unlocked"
        };
        out.push(Command::Notify(env.i18n().tr(told).into()));
    })?;
    let escape: Rc<RefCell<(Escape, Option<Instant>)>> = Rc::default();
    r.add_input_filter(InputFilterSpec::new(
        FILTER,
        "lock.touch",
        Box::new(move |env, event| {
            let (escape, hinted) = &mut *escape.borrow_mut();
            if !env.settings().read(&LOCKED) || state.locked.get() {
                *escape = Escape::default();
                *hinted = None;
                return InputVerdict::Pass;
            }
            locked(env.now(), event, escape, hinted)
        }),
    ))?;
    r.add_keymap_when(&LOCKED, Cow::Borrowed(ESCAPE))?;
    r.add_status_item(StatusItemSpec::new(
        STATUS,
        "lock.touch-status",
        Box::new(|env, _| {
            env.settings()
                .read(&LOCKED)
                .then_some(StatusValue::Icon(Cow::Borrowed("lock")))
        }),
    ))?;
    r.add_menu_entries(&ENTRIES)?;
    // Only where there is touch to lock.
    r.set_menu_need(LOCKED.id().clone(), MenuNeed::Touch)?;
    r.set_menu_need(TOGGLE, MenuNeed::Touch)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{PointerEvent, PointerTool};

    fn finger(id: u8, phase: PointerPhase, (x, y): (i32, i32)) -> InputEvent {
        InputEvent::Pointer(PointerEvent::new(
            id,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ))
    }

    /// Runs `steps` (when, which finger, what, where): the verdicts, the
    /// hints left out (as taken: they change nothing else).
    fn run(steps: &[(u64, u8, PointerPhase, (i32, i32))]) -> Vec<InputVerdict> {
        run_with_hints(steps)
            .into_iter()
            .map(|v| match v {
                InputVerdict::Run(id) if id == HINT => InputVerdict::Take,
                v => v,
            })
            .collect()
    }

    fn run_with_hints(steps: &[(u64, u8, PointerPhase, (i32, i32))]) -> Vec<InputVerdict> {
        let mut escape = Escape::default();
        let mut hinted = None;
        steps
            .iter()
            .map(|&(at, id, phase, point)| {
                locked(
                    Instant(at),
                    &finger(id, phase, point),
                    &mut escape,
                    &mut hinted,
                )
            })
            .collect()
    }

    use PointerPhase::{Cancel, Down, Move, Up};
    const AT: (i32, i32) = (200, 300);

    #[test]
    fn two_taps_then_a_hold_unlock() {
        let verdicts = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, (210, 305)),
            (500, 0, Up, (210, 305)),
            (800, 0, Down, AT),
            (800 + HOLD_MS, 0, Up, AT),
        ]);
        assert_eq!(verdicts.last(), Some(&InputVerdict::Run(TOGGLE)));
        assert!(verdicts[..5].iter().all(|v| *v == InputVerdict::Take));
    }

    /// As the notice says: tap twice, and hold the second tap.
    #[test]
    fn a_tap_then_a_held_tap_unlock() {
        let verdicts = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, (205, 302)),
            (400 + HOLD_MS, 0, Up, (205, 302)),
        ]);
        assert_eq!(verdicts.last(), Some(&InputVerdict::Run(TOGGLE)));
        assert!(verdicts[..3].iter().all(|v| *v == InputVerdict::Take));
        // Still down, once held long enough: a little movement unlocks.
        let held = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, AT),
            (400 + HOLD_MS + 10, 0, Move, (201, 301)),
        ]);
        assert_eq!(held.last(), Some(&InputVerdict::Run(TOGGLE)));
    }

    #[test]
    fn a_resting_hand_does_not_unlock() {
        // A palm still for a long while, then lifted.
        assert!(
            !run(&[(0, 0, Down, AT), (10_000, 0, Up, AT)]).contains(&InputVerdict::Run(TOGGLE))
        );
        // A palm and a thumb: one lifts after the other's taps and hold.
        let verdicts = run(&[
            (0, 0, Down, AT),
            (100, 1, Down, (300, 600)),
            (150, 1, Up, (300, 600)),
            (400, 1, Down, (300, 600)),
            (450, 1, Up, (300, 600)),
            (700, 1, Down, (300, 600)),
            (700 + HOLD_MS, 1, Up, (300, 600)),
            (5_000, 0, Up, AT),
        ]);
        assert!(
            !verdicts.contains(&InputVerdict::Run(TOGGLE)),
            "{verdicts:?}"
        );
    }

    #[test]
    fn taps_too_slow_or_too_far_apart_do_not_unlock() {
        // The held tap coming too late after the tap: a hand resting.
        let slow = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (100 + GAP_MS + 1, 0, Down, AT),
            (100 + GAP_MS + 1 + HOLD_MS, 0, Up, AT),
        ]);
        assert!(!slow.contains(&InputVerdict::Run(TOGGLE)));
        // The held tap far from the tap.
        let far = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, (400, 700)),
            (400 + HOLD_MS, 0, Up, (400, 700)),
        ]);
        assert!(!far.contains(&InputVerdict::Run(TOGGLE)));
        let moved = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, AT),
            (500, 0, Up, AT),
            (800, 0, Down, AT),
            (900, 0, Move, (350, 300)),
            (800 + HOLD_MS, 0, Up, (350, 300)),
        ]);
        assert!(!moved.contains(&InputVerdict::Run(TOGGLE)));
    }

    #[test]
    fn a_cancel_for_one_finger_ends_them_all() {
        // Two fingers down, the gesture cancelled for one of them alone:
        // the way out works afterwards all the same.
        let verdicts = run(&[
            (0, 0, Down, AT),
            (10, 1, Down, (300, 600)),
            (20, 0, Cancel, AT),
            (1_000, 2, Down, AT),
            (1_100, 2, Up, AT),
            (1_400, 2, Down, AT),
            (1_500, 2, Up, AT),
            (1_800, 2, Down, AT),
            (1_800 + HOLD_MS, 2, Up, AT),
        ]);
        assert_eq!(verdicts.last(), Some(&InputVerdict::Run(TOGGLE)));
    }

    #[test]
    fn buttons_pass() {
        let mut escape = Escape::default();
        let press = InputEvent::Button {
            name: "page-forward",
            state: paperpaw_sdk::ButtonState::Pressed,
        };
        let verdict = locked(Instant(0), &press, &mut escape, &mut None);
        assert_eq!(verdict, InputVerdict::Pass);
    }

    fn hints(verdicts: &[InputVerdict]) -> usize {
        verdicts
            .iter()
            .filter(|v| **v == InputVerdict::Run(HINT))
            .count()
    }

    /// Touches that are no step of the way out say how to unlock, at most
    /// every HINT_MS; the way out itself is never interrupted by it.
    #[test]
    fn touches_say_how_to_unlock_now_and_then() {
        // Single taps two seconds apart, for forty seconds: said every
        // ten seconds, the first time at the second tap.
        let taps: Vec<_> = (0..20u64)
            .flat_map(|n| [(n * 2_000, 0, Down, AT), (n * 2_000 + 50, 0, Up, AT)])
            .collect();
        let verdicts = run_with_hints(&taps);
        assert_eq!(verdicts[2], InputVerdict::Run(HINT));
        assert_eq!(hints(&verdicts), 4);
        // A hand resting, then lifted: said once, when it is lifted.
        let rest = run_with_hints(&[(0, 0, Down, AT), (10_000, 0, Up, AT)]);
        assert_eq!(rest, [InputVerdict::Take, InputVerdict::Run(HINT)]);
        // A swipe.
        let swipe = run_with_hints(&[
            (0, 0, Down, AT),
            (50, 0, Move, (300, 300)),
            (80, 0, Up, (400, 300)),
        ]);
        assert_eq!(hints(&swipe), 1);
        // The way out: nothing said between its steps.
        let verdicts = run_with_hints(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, AT),
            (500, 0, Up, AT),
            (800, 0, Down, AT),
            (800 + HOLD_MS, 0, Up, AT),
        ]);
        assert_eq!(hints(&verdicts), 0);
        assert_eq!(verdicts.last(), Some(&InputVerdict::Run(TOGGLE)));
        // Held too short after the taps: then it is said.
        let short = run_with_hints(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, AT),
            (500, 0, Up, AT),
            (800, 0, Down, AT),
            (1_200, 0, Up, AT),
        ]);
        assert_eq!(short.last(), Some(&InputVerdict::Run(HINT)));
    }

    /// A slow screen holds touches back: taps up to TAP_MS long, GAP_MS
    /// apart, still count; and once held long enough, a little movement
    /// of the finger unlocks without lifting it.
    #[test]
    fn the_way_out_allows_for_a_slow_screen_and_needs_no_lift() {
        let slow = run(&[
            (0, 0, Down, AT),
            (TAP_MS - 50, 0, Up, AT),
            (TAP_MS + GAP_MS - 100, 0, Down, AT),
            (2 * TAP_MS + GAP_MS - 150, 0, Up, AT),
            (2 * TAP_MS + 2 * GAP_MS - 200, 0, Down, AT),
            (2 * TAP_MS + 2 * GAP_MS - 200 + HOLD_MS, 0, Up, AT),
        ]);
        assert_eq!(slow.last(), Some(&InputVerdict::Run(TOGGLE)));
        let held = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, AT),
            (500, 0, Up, AT),
            (800, 0, Down, AT),
            (1_000, 0, Move, (201, 301)),
            (800 + HOLD_MS + 10, 0, Move, (202, 300)),
        ]);
        assert_eq!(held.last(), Some(&InputVerdict::Run(TOGGLE)));
        assert!(held[..6].iter().all(|v| *v == InputVerdict::Take));
    }
}
