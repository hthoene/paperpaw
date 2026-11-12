//! Input to native screens through the real kernel and the retained owner
//! (`docs/abi/screens-input.md` section 2, `ui.md` sections 2 and 4.3): keys,
//! the key map layer, pointers and gestures reach the screen on top, nothing
//! reaches a screen below, and nothing reaches one below a guard.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use alloc::string::String;
use paperpaw_platform::{ButtonState, InputEvent, Instant, Point, PointerEvent, PointerPhase};
use paperpaw_sdk::Id;

/// The key-bind mode (see `key_records`): `button.confirm` on the screen's
/// layer is `factory.action`.
pub(super) const BOUND: u8 = 113;

/// What the screen's `input` callback saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Seen {
    Key(String, u32),
    Pointer(i32, i32, u32, Option<u32>),
    Gesture(u32),
    Action(String),
}

std::thread_local! {
    /// The owner (host id) and the record of each call, in order.
    static SEEN: RefCell<Vec<(u64, Seen)>> = const { RefCell::new(Vec::new()) };
    /// Whether a raw key is answered as handled.
    static TAKES_KEYS: Cell<bool> = const { Cell::new(false) };
    /// Whether a pointer contact is answered as handled.
    static TAKES_POINTERS: Cell<bool> = const { Cell::new(false) };
    /// A batch of commands the screen submits when it hears an action (and, as
    /// `on_shown`, when it hears it is shown), and what its `input` answers
    /// then (see `native_screen_command_tests.rs`).
    pub(super) static STAGED: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    pub(super) static STAGED_ON_SHOWN: Cell<bool> = const { Cell::new(false) };
    pub(super) static STAGED_ANSWER: Cell<i32> = const { Cell::new(1) };
    /// The features the next installed plugin is admitted with.
    pub(super) static FEATURES: Cell<crate::native_host::admitted_features::AdmittedFeatures> =
        const { Cell::new(crate::native_host::admitted_features::AdmittedFeatures::NONE) };
    /// With a store, the callback makes a hand-over in it first (`page = 3`) and
    /// submits an `open` of `factory.screen` naming it, as the staged batch
    /// (the second: the id it names instead of the made one, 0 for the made one).
    pub(super) static STAGED_HANDOVER: RefCell<
        Option<(crate::native_host::shared::Shared<crate::native_host::ui_store::UiStore>, u32)>,
    > = const { RefCell::new(None) };
}

/// Submits the staged batch from the callback `state` is the screen's of.
pub(super) fn submit_staged(state: *mut c_void) {
    let Some(batch) = STAGED.with(|batch| batch.borrow().clone()) else {
        return;
    };
    let batch = match STAGED_HANDOVER.with(|made| {
        made.borrow()
            .as_ref()
            .map(|(store, wrong)| (store.try_clone().unwrap(), *wrong))
    }) {
        Some((store, wrong)) => handover_batch(state, &store, wrong),
        None => batch,
    };
    // SAFETY: the owner retains the initialized fixture; its host table and
    // pinned context are live for the callback.
    let (host, context) = unsafe {
        let state = &*state.cast::<ScreenState>();
        (&*state.host, state.context)
    };
    // SAFETY: the batch is readable for its length.
    let _ = unsafe { (host.commands_submit)(context, borrowed(&batch)) };
}

/// The batch of an `open` naming a hand-over the callback of `state` makes now
/// (or `wrong`, an id nobody made, when it is not 0).
fn handover_batch(
    state: *mut c_void,
    store: &crate::native_host::ui_store::UiStore,
    wrong: u32,
) -> Vec<u8> {
    use crate::native_host::commands::{WireCommand, commands_encode};
    use crate::native_host::ui_store::{Param, Stamp};
    // SAFETY: the owner retains the initialized fixture and its pinned context.
    let context = unsafe {
        &*(*state.cast::<ScreenState>())
            .context
            .cast::<Context<Heap>>()
    };
    let epoch = context
        .environment
        .with_frame(|frame| frame.callback_epoch())
        .unwrap()
        .unwrap();
    let stamp = Stamp::new(context.owner_key(), epoch).unwrap();
    let made = store
        .handover_make(
            stamp,
            &[Param {
                key: "page",
                value: "3",
            }],
        )
        .unwrap();
    let mut bytes = [0; 256];
    let len = commands_encode(
        &[WireCommand::Open {
            target: "factory.screen",
            mode: 0,
            slot: "",
            handover: if wrong == 0 { made } else { wrong },
        }],
        &mut bytes,
    )
    .unwrap();
    bytes[..len].to_vec()
}

/// The screen-shown event of the screen: submits the staged batch if asked to.
pub(super) fn events_submit(state: *mut c_void, bytes: &[u8]) {
    if STAGED_ON_SHOWN.with(Cell::get)
        && matches!(
            wire::records_decode_single(bytes),
            Ok(wire::Record::EventScreenShown(_))
        )
    {
        submit_staged(state);
    }
}

/// The `input` callback of the fixture: records the record, answers handled for
/// an action and for a key when `TAKES_KEYS` says so.
pub(super) unsafe extern "C" fn screen_input(
    state: *mut c_void,
    token: u32,
    bytes: PpBytes,
) -> i32 {
    if token != 91 {
        return ERR_INVALID;
    }
    // SAFETY: the owner retains the initialized fixture and its pinned context.
    let context = unsafe {
        &*(*state.cast::<ScreenState>())
            .context
            .cast::<Context<Heap>>()
    };
    // SAFETY: the callback borrows the host's initialized input for this call.
    let bytes = unsafe { core::slice::from_raw_parts(bytes.ptr, bytes.len) };
    let (seen, answer) = match wire::records_decode_single(bytes) {
        Ok(wire::Record::InputKey(key)) => (
            Seen::Key(key.key.into(), key.state),
            i32::from(TAKES_KEYS.with(Cell::get)),
        ),
        Ok(wire::Record::InputPointer(p)) => (
            Seen::Pointer(p.x, p.y, p.phase, p.contact),
            i32::from(TAKES_POINTERS.with(Cell::get)),
        ),
        Ok(wire::Record::InputGesture(g)) => (Seen::Gesture(g.kind), 0),
        Ok(wire::Record::InputAction(a)) => {
            submit_staged(state);
            (Seen::Action(a.action.into()), STAGED_ANSWER.with(Cell::get))
        }
        _ => return ERR_INVALID,
    };
    let owner = context.resources.owner_id();
    SEEN.with(|all| all.borrow_mut().push((owner, seen)));
    answer
}

fn seen() -> Vec<Seen> {
    SEEN.with(|all| all.borrow().iter().map(|(_, seen)| seen.clone()).collect())
}

fn owners() -> Vec<u64> {
    let mut owners: Vec<u64> = SEEN.with(|all| all.borrow().iter().map(|(o, _)| *o).collect());
    owners.dedup();
    owners
}

/// A kernel with `factory.screen` installed (declaring its action when `bound`
/// is the key-bind mode) and opened on top.
fn opened(bound: u8, flags: u32) -> crate::Kernel {
    ARMS.with(|arms| arms.set(u8::from(bound != 0)));
    MENUS.with(|menus| menus.set(bound));
    let mut kernel = kernel(flags);
    MENUS.with(|menus| menus.set(0));
    ARMS.with(|arms| arms.set(0));
    SEEN.with(|all| all.borrow_mut().clear());
    TAKES_KEYS.with(|takes| takes.set(false));
    TAKES_POINTERS.with(|takes| takes.set(false));
    open(&mut kernel, Params::new());
    kernel
}

fn key(kernel: &mut crate::Kernel, at: u64, name: &'static str) {
    for (state, millis) in [(ButtonState::Pressed, at), (ButtonState::Released, at + 40)] {
        kernel.handle(Instant(millis), InputEvent::Button { name, state });
    }
}

fn touch(kernel: &mut crate::Kernel, at: u64, phase: PointerPhase, x: i32, y: i32) {
    kernel.handle(
        Instant(at),
        InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            paperpaw_platform::PointerTool::Finger,
            1000,
        )),
    );
}

#[test]
fn a_key_reaches_the_screen_on_top_raw_and_then_by_the_key_map() {
    let mut kernel = opened(BOUND, 0);
    key(&mut kernel, 100, "confirm");
    let all = seen();
    // The raw press first (the screen did not take it, so no release follows),
    // then what the screen's own layer bound the button to, handled by the
    // screen itself.
    assert_eq!(
        all,
        [
            Seen::Key("confirm".into(), 0),
            Seen::Action("factory.action".into()),
        ]
    );
}

#[test]
fn a_key_the_screen_takes_never_reaches_the_key_map() {
    let mut kernel = opened(BOUND, 0);
    TAKES_KEYS.with(|takes| takes.set(true));
    key(&mut kernel, 100, "confirm");
    assert_eq!(
        seen(),
        [
            Seen::Key("confirm".into(), 0),
            Seen::Key("confirm".into(), 1)
        ]
    );
}

#[test]
fn power_is_never_sent_and_an_unbound_key_is_only_raw() {
    let mut kernel = opened(BOUND, 0);
    key(&mut kernel, 100, "power");
    // Power is no raw key: only the action the key map made of it is offered.
    assert!(
        seen().iter().all(|s| !matches!(s, Seen::Key(..))),
        "{:?}",
        seen()
    );
    SEEN.with(|all| all.borrow_mut().clear());
    key(&mut kernel, 300, "back");
    // Not taken, so only the press was offered raw.
    let raw: Vec<Seen> = seen()
        .into_iter()
        .filter(|s| matches!(s, Seen::Key(..)))
        .collect();
    assert_eq!(raw, [Seen::Key("back".into(), 0)]);
}

#[test]
fn a_finger_reaches_the_screen_on_top_with_its_contact_and_makes_its_gesture() {
    let mut kernel = opened(0, 0);
    touch(&mut kernel, 100, PointerPhase::Down, 30, 40);
    touch(&mut kernel, 120, PointerPhase::Move, 31, 41);
    touch(&mut kernel, 160, PointerPhase::Up, 31, 41);
    kernel.tick(Instant(1_000));
    let all = seen();
    // The contact is offered on down; the screen did not take it, so the rest
    // of the finger is the kernel's, which makes a tap (kind 0) of it.
    assert_eq!(all[0], Seen::Pointer(30, 40, 0, Some(0)));
    assert!(all.contains(&Seen::Gesture(0)), "{all:?}");
}

#[test]
fn a_finger_the_screen_takes_goes_on_reaching_it_to_the_end() {
    let mut kernel = opened(0, 0);
    TAKES_POINTERS.with(|takes| takes.set(true));
    touch(&mut kernel, 100, PointerPhase::Down, 30, 40);
    touch(&mut kernel, 120, PointerPhase::Move, 31, 41);
    touch(&mut kernel, 160, PointerPhase::Up, 31, 41);
    kernel.tick(Instant(1_000));
    // No gesture is made of a finger the screen holds.
    assert_eq!(
        seen(),
        [
            Seen::Pointer(30, 40, 0, Some(0)),
            Seen::Pointer(31, 41, 1, Some(0)),
            Seen::Pointer(31, 41, 2, Some(0)),
        ]
    );
}

#[test]
fn nothing_reaches_a_screen_below_the_top() {
    // The first owner's screen is on the stack, the second's opens above it.
    let mut kernel = opened(0, 0);
    let first = owners();
    assert!(first.is_empty());
    let other = host_named("other", 0);
    kernel
        .register_native(other, &device_depth(&kernel), |_, _, _| {})
        .unwrap();
    kernel.open_screen(Instant(2), Id::new_static("other.screen"), Params::new());
    let top = kernel.stack.len() - 1;
    kernel.with_screen(top, |screen, cx| {
        screen.on_show(cx);
        Handled::Yes
    });
    SEEN.with(|all| all.borrow_mut().clear());
    key(&mut kernel, 100, "back");
    touch(&mut kernel, 300, PointerPhase::Down, 5, 5);
    touch(&mut kernel, 340, PointerPhase::Up, 5, 5);
    // Every record came from one owner: the screen on top.
    assert!(!seen().is_empty());
    assert_eq!(owners().len(), 1, "{:?}", owners());
}

#[test]
fn nothing_reaches_a_screen_below_a_guard() {
    // `guards` is bit 1 of the screen flags.
    let mut kernel = opened(0, 0);
    let guard = host_named("guard", 1 << 1);
    kernel
        .register_native(guard, &device_depth(&kernel), |_, _, _| {})
        .unwrap();
    kernel.open_screen(Instant(2), Id::new_static("guard.screen"), Params::new());
    let top = kernel.stack.len() - 1;
    kernel.with_screen(top, |screen, cx| {
        screen.on_show(cx);
        Handled::Yes
    });
    // Another plugin's screen opened while the guard is up waits below it.
    kernel.open_screen(Instant(3), Id::new_static("factory.screen"), Params::new());
    assert_eq!(
        kernel.active_screen(),
        Some(&Id::new_static("guard.screen"))
    );
    SEEN.with(|all| all.borrow_mut().clear());
    key(&mut kernel, 100, "back");
    touch(&mut kernel, 300, PointerPhase::Down, 5, 5);
    touch(&mut kernel, 340, PointerPhase::Up, 5, 5);
    // Only the guard's callback ran (one owner), and it saw the input.
    assert!(!seen().is_empty());
    assert_eq!(owners().len(), 1, "{:?}", owners());
}

#[path = "native_dev_screen_tests.rs"]
mod dev_commands;
#[path = "native_screen_command_tests.rs"]
mod screen_commands;

/// A native screen takes every key only to pass it to its plugin: it is not a
/// real-time screen (#2671). Each of these held before the fix.
mod not_realtime {
    use super::*;
    use crate::native_host::work_sched::{Power, WorkEnv};

    #[test]
    fn a_native_screen_on_top_shows_no_way_out_hint() {
        let mut kernel = opened(0, 0);
        // The notice itself, not the file that keeps it shown (the test
        // kernel's storage is read-only): none at the opening, none asked.
        assert!(kernel.notice.is_none() && kernel.waiting_notices.is_empty());
        kernel.hint_escape();
        assert!(kernel.notice.is_none() && kernel.waiting_notices.is_empty());
    }

    #[test]
    fn back_held_on_a_native_screen_does_not_leave_it() {
        let mut kernel = opened(0, 0);
        TAKES_KEYS.with(|takes| takes.set(true));
        kernel.handle(
            Instant(100),
            InputEvent::Button {
                name: "back",
                state: ButtonState::Pressed,
            },
        );
        assert_eq!(seen(), [Seen::Key("back".into(), 0)], "the plugin has it");
        assert_eq!(kernel.escape_due(), None);
    }

    #[test]
    fn a_finger_held_in_the_corner_of_a_native_screen_does_not_leave_it() {
        let mut kernel = opened(0, 0);
        touch(&mut kernel, 100, PointerPhase::Down, 1, 1);
        assert_eq!(kernel.escape_due(), None);
    }

    /// Records whether the kernel said a real-time screen holds the device.
    #[derive(Default)]
    struct Held(Cell<Option<bool>>);
    impl WorkEnv for Held {
        fn now_ms(&self) -> u64 {
            0
        }
        fn power(&self) -> Power {
            Power {
                level: 0,
                stretch: 1,
                radio_on: true,
            }
        }
        fn screen_hold(&self) -> bool {
            self.0.get().unwrap_or(false)
        }
        fn network_up(&self) -> bool {
            true
        }
        fn network_allowed(&self, _: u64) -> bool {
            true
        }
        fn saving_manual(&self, _: u64) -> bool {
            false
        }
        fn subscribed(&self, _: u64, _: u32) -> bool {
            false
        }
        fn kernel_state(&self, _: u32, _: u32, screen_hold: bool) {
            self.0.set(Some(screen_hold));
        }
    }

    #[test]
    fn a_native_screen_on_top_holds_no_background_work() {
        let kernel = opened(0, 0);
        let env = Held::default();
        kernel.tell_work_env(&env);
        assert_eq!(env.0.get(), Some(false));
    }
}
