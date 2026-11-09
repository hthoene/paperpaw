//! The `core.input-filter` chain on the kernel's input path: an event as the
//! stage's record and back, and what a swallowing, replacing, failing filter
//! (and Power) makes of an input.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native_host::codec;
use alloc::vec;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_sdk::{Direction, Gesture, GestureKind};

fn all(_: u64) -> bool {
    true
}
fn none(_: u64, _: &str) -> bool {
    false
}

fn filter(plugin: u64) -> Subscription<'static> {
    Subscription {
        stage: stage::KEY,
        plugin,
        token: 1,
        order: 0,
        index: 0,
        signature: 0,
        enabled: true,
        style_granted: false,
    }
}

/// A filter that answers what it was told to, one answer per call.
struct Scripted {
    answers: Vec<Result<stage::FilterVerdict<'static>, i32>>,
    called: usize,
}

impl FilterRunner for Scripted {
    fn call(&mut self, _: &Subscription<'_>, _: &[u8], reply: &mut [u8]) -> Result<usize, i32> {
        self.called += 1;
        match self.answers.remove(0) {
            Ok(verdict) => verdict.encode(reply),
            Err(status) => Err(status),
        }
    }
}

fn pass() -> Result<stage::FilterVerdict<'static>, i32> {
    Ok(stage::FilterVerdict::new(0, "", &[], ""))
}

/// The bytes of a `replace` verdict's input: a one-record batch.
fn batch(record: &Record<'_>) -> &'static [u8] {
    let mut bytes = [0; 128];
    let length = codec::encode_batch(core::slice::from_ref(record), &mut bytes).unwrap();
    alloc::boxed::Box::leak(bytes[..length].to_vec().into_boxed_slice())
}

const DISPLAY: (u16, u16) = (480, 800);
const KEYS: [&str; 1] = ["extra"];

fn run(
    event: InputEvent,
    answers: Vec<Result<stage::FilterVerdict<'static>, i32>>,
) -> (Native, usize, usize) {
    run_declaring(event, answers, &none)
}

/// [`run`] where `declared` says what the filter's plugin declared.
fn run_declaring(
    event: InputEvent,
    answers: Vec<Result<stage::FilterVerdict<'static>, i32>>,
    declared: &dyn Fn(u64, &str) -> bool,
) -> (Native, usize, usize) {
    let mut chain = InputFilters::new();
    let mut runner = Scripted { answers, called: 0 };
    let policy = Policy {
        touch: true,
        secret: false,
        runs: &all,
        declared,
    };
    let (native, refused) = run_chain(
        &mut chain,
        &mut runner,
        &[filter(1)],
        &policy,
        &event,
        (5, DISPLAY, &KEYS),
    );
    (native, refused, runner.called)
}

fn press(name: &'static str) -> InputEvent {
    InputEvent::Button {
        name,
        state: ButtonState::Pressed,
    }
}

fn finger(phase: PointerPhase, x: i32, y: i32) -> InputEvent {
    InputEvent::Pointer(PointerEvent::new(
        3,
        phase,
        Point::new(x, y),
        PointerTool::Finger,
        1000,
    ))
}

#[test]
fn an_event_is_the_stages_record_and_power_and_hovering_are_not() {
    assert_eq!(
        record_of(&press("up"), 9),
        Some(Record::InputKey(wire::InputKey {
            now_ms: 9,
            key: "up",
            state: 0
        }))
    );
    let released = InputEvent::Button {
        name: "up",
        state: ButtonState::Released,
    };
    assert!(matches!(
        record_of(&released, 9),
        Some(Record::InputKey(wire::InputKey { state: 1, .. }))
    ));
    assert_eq!(record_of(&press(buttons::POWER), 9), None);
    for (phase, number) in [
        (PointerPhase::Down, 0),
        (PointerPhase::Move, 1),
        (PointerPhase::Up, 2),
        (PointerPhase::Cancel, 3),
    ] {
        assert_eq!(
            record_of(&finger(phase, 10, 20), 9),
            Some(Record::InputPointer(wire::InputPointer {
                now_ms: 9,
                x: 10,
                y: 20,
                phase: number,
                contact: Some(3)
            }))
        );
    }
    assert_eq!(record_of(&finger(PointerPhase::Hover, 1, 1), 9), None);
}

#[test]
fn a_replaced_event_is_known_or_it_is_not() {
    let key = |name, state| {
        Record::InputKey(wire::InputKey {
            now_ms: 1,
            key: name,
            state,
        })
    };
    let was = press("up");
    let make = |record: &Record<'_>| event_of(record, &was, DISPLAY, &KEYS);
    // A platform button, and one of the device's.
    assert_eq!(
        make(&key("down", 1)),
        Some(InputEvent::Button {
            name: "down",
            state: ButtonState::Released
        })
    );
    assert_eq!(make(&key("extra", 0)), Some(press("extra")));
    // Not a name anyone knows, Power, a repeat: no event.
    assert_eq!(make(&key("nonsense", 0)), None);
    assert_eq!(make(&key(buttons::POWER, 0)), None);
    assert_eq!(make(&key("up", 2)), None);
    // A pointer is clamped to the display and keeps the original's tool.
    let pen = InputEvent::Pointer(PointerEvent::new(
        1,
        PointerPhase::Move,
        Point::new(0, 0),
        PointerTool::Pen,
        1000,
    ));
    let far = Record::InputPointer(wire::InputPointer {
        now_ms: 1,
        x: 9000,
        y: -5,
        phase: 0,
        contact: Some(2),
    });
    let Some(InputEvent::Pointer(made)) = event_of(&far, &pen, DISPLAY, &KEYS) else {
        panic!("a pointer");
    };
    assert_eq!(
        (made.point.x, made.point.y, made.id, made.tool),
        (479, 0, 2, PointerTool::Pen)
    );
    assert_eq!(made.phase, PointerPhase::Down);
    // A gesture is raised, never routed as an event.
    let gesture = Record::InputGesture(wire::InputGesture {
        now_ms: 1,
        kind: 0,
        x: 0,
        y: 0,
    });
    assert_eq!(make(&gesture), None);
}

#[test]
fn a_filter_swallows_a_key() {
    let take = Ok(stage::FilterVerdict::new(1, "", &[], ""));
    assert_eq!(run(press("up"), vec![take]), (Native::Taken, 0, 1));
}

#[test]
fn a_filter_replaces_a_key_with_a_known_one_and_an_unknown_one_passes() {
    let replace = |name| {
        let record = Record::InputKey(wire::InputKey {
            now_ms: 5,
            key: name,
            state: 0,
        });
        Ok(stage::FilterVerdict::new(4, "", batch(&record), ""))
    };
    assert_eq!(
        run(press("up"), vec![replace("down")]),
        (Native::Replaced(press("down")), 0, 1)
    );
    // The key path takes known names only: the original goes on, counted.
    assert_eq!(
        run(press("up"), vec![replace("nonsense")]),
        (Native::Pass, 1, 1)
    );
    // A filter cannot make Power.
    assert_eq!(
        run(press("up"), vec![replace(buttons::POWER)]),
        (Native::Pass, 1, 1)
    );
}

#[test]
fn a_filter_replaces_a_pointer_and_it_is_clamped() {
    let record = Record::InputPointer(wire::InputPointer {
        now_ms: 5,
        x: 5000,
        y: 20,
        phase: 0,
        contact: Some(3),
    });
    let replace = Ok(stage::FilterVerdict::new(4, "", batch(&record), ""));
    let (native, refused, _) = run(finger(PointerPhase::Down, 10, 20), vec![replace]);
    assert_eq!(refused, 0);
    assert_eq!(
        native,
        Native::Replaced(finger(PointerPhase::Down, 479, 20))
    );
}

#[test]
fn a_filter_raises_an_input_it_declared_and_nothing_else() {
    let raise = |name| Ok(stage::FilterVerdict::new(5, "", &[], name));
    let own = |plugin: u64, name: &str| plugin == 1 && name == "gesture.demo.flick";
    // Declared: the input is raised, no event goes on.
    assert_eq!(
        run_declaring(press("up"), vec![raise("gesture.demo.flick")], &own),
        (Native::Raise("gesture.demo.flick".into()), 0, 1)
    );
    // Not declared (another's, or none): the original passes, counted.
    for name in ["gesture.other.flick", "input.demo.flick", "button.menu", ""] {
        assert_eq!(
            run_declaring(press("up"), vec![raise(name)], &own),
            (Native::Pass, 1, 1),
            "{name}"
        );
    }
}

#[test]
fn a_filter_replaces_an_input_with_a_gesture() {
    let gesture = |kind, x, y| {
        let record = Record::InputGesture(wire::InputGesture {
            now_ms: 5,
            kind,
            x,
            y,
        });
        Ok(stage::FilterVerdict::new(4, "", batch(&record), ""))
    };
    let made = |kind, x, y, tool| {
        let point = Point::new(x, y);
        Native::Gesture(Gesture::new(kind, point, point, tool))
    };
    let tap = finger(PointerPhase::Down, 10, 20);
    for (number, kind) in [
        (0, GestureKind::Tap),
        (1, GestureKind::LongPress),
        (2, GestureKind::Swipe(Direction::Left)),
        (3, GestureKind::Swipe(Direction::Right)),
        (4, GestureKind::Swipe(Direction::Up)),
        (5, GestureKind::Swipe(Direction::Down)),
    ] {
        assert_eq!(
            run(tap, vec![gesture(number, 30, 40)]),
            (made(kind, 30, 40, PointerTool::Finger), 0, 1)
        );
    }
    // Clamped to the display; a key replaced makes a finger's gesture.
    assert_eq!(
        run(press("up"), vec![gesture(0, 9000, -3)]),
        (made(GestureKind::Tap, 479, 0, PointerTool::Finger), 0, 1)
    );
    // A kind that is none of ours: the original passes, counted.
    assert_eq!(run(tap, vec![gesture(6, 1, 1)]), (Native::Pass, 1, 1));
}

#[test]
fn power_is_never_filtered() {
    // The runner has no answer for it: it is never called.
    assert_eq!(run(press(buttons::POWER), vec![]), (Native::Pass, 0, 0));
}

#[test]
fn a_filter_that_fails_or_answers_badly_passes_the_input() {
    assert_eq!(run(press("up"), vec![Err(ERR_STATE)]), (Native::Pass, 1, 1));
    // A verdict the host does not know.
    let strange = Ok(stage::FilterVerdict::new(99, "", &[], ""));
    assert_eq!(run(press("up"), vec![strange]), (Native::Pass, 1, 1));
    assert_eq!(run(press("up"), vec![pass()]), (Native::Pass, 0, 1));
}

#[test]
fn cancel_and_run_are_told_as_they_are() {
    let cancel = Ok(stage::FilterVerdict::new(3, "", &[], ""));
    assert_eq!(run(press("up"), vec![cancel]).0, Native::Cancelled);
    let action = Ok(stage::FilterVerdict::new(2, "lock.open", &[], ""));
    assert_eq!(
        run(press("up"), vec![action]).0,
        Native::Run("lock.open".into())
    );
}

#[test]
fn a_display_of_no_size_clamps_to_the_origin_and_does_not_panic() {
    let record = Record::InputPointer(wire::InputPointer {
        now_ms: 1,
        x: 300,
        y: -7,
        phase: 1,
        contact: Some(0),
    });
    let was = finger(PointerPhase::Down, 1, 1);
    for display in [(0, 0), (0, 800), (480, 0)] {
        let Some(InputEvent::Pointer(made)) = event_of(&record, &was, display, &KEYS) else {
            panic!("a pointer");
        };
        let (x, y) = (made.point.x, made.point.y);
        assert_eq!((x == 0, y == 0), (display.0 == 0, true), "{display:?}");
    }
}

/// The real `Kernel::handle`: a native filter, counted, under the interface-6
/// lock and PIN screens of another plugin.
mod kernel {
    use super::*;
    use crate::native_host::{
        declaration_store::Allocations,
        flat::FlatDepth,
        native_declarations::{self, NativeDeclarations},
        native_instances::{NativeInstance, NativeLease},
        native_screen::Dispatch,
        raw_declarations::RawDeclarations,
    };
    use alloc::{boxed::Box, rc::Rc};
    use core::cell::{Cell, RefCell};
    use paperpaw_graphics::Canvas;
    use paperpaw_native::host_glue::ERR_BUSY;
    use paperpaw_sdk::{
        ActionSpec, Cx, Env, Handled, Id, Manifest, Params, Plugin, RegisterError, Registrar,
        Screen, ScreenSpec, Uses, Version,
        filters::{InputFilterSpec, InputVerdict},
        keys::InputSpec as PluginInput,
    };

    extern crate std;
    std::thread_local! {
        /// What the native filter answers at its next call (one answer; then
        /// pass): [`PASS`] and the modes below.
        static MODE: Cell<u8> = const { Cell::new(PASS) };
        /// How often the action `f.chord` ran, and a screen got a gesture.
        static RAN: Cell<u32> = const { Cell::new(0) };
        static GESTURES: Cell<u32> = const { Cell::new(0) };
        /// Who saw an input, in order: `native` or `legacy`.
        static ORDER: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }
    const PASS: u8 = 0;
    /// Raise `input.f.chord` (declared), `input.f.nothing` (not).
    const RAISE: u8 = 1;
    const RAISE_UNDECLARED: u8 = 2;
    /// Replace with a tap at (100, 100).
    const GESTURE: u8 = 3;
    /// The callback fails.
    const FAIL: u8 = 4;
    const TAKE: u8 = 5;

    fn verdict(mode: u8, out: &mut [u8]) -> Result<usize, i32> {
        let mut bytes = [0; 128];
        let record = Record::InputGesture(wire::InputGesture {
            now_ms: 1,
            kind: 0,
            x: 100,
            y: 100,
        });
        let length = codec::encode_batch(core::slice::from_ref(&record), &mut bytes).unwrap();
        match mode {
            RAISE => stage::FilterVerdict::new(5, "", &[], "input.f.chord").encode(out),
            RAISE_UNDECLARED => {
                stage::FilterVerdict::new(5, "", &[], "input.f.nothing").encode(out)
            }
            GESTURE => stage::FilterVerdict::new(4, "", &bytes[..length], "").encode(out),
            FAIL => Err(ERR_STATE),
            TAKE => stage::FilterVerdict::new(1, "", &[], "").encode(out),
            _ => stage::FilterVerdict::new(0, "", &[], "").encode(out),
        }
    }

    const LOCK: Id = Id::new_static("g.lock");
    const PIN: Id = Id::new_static("g.pin");
    const OWN_PIN: Id = Id::new_static("f.pin");

    /// A screen that guards the device, one that takes a PIN, and the filter
    /// plugin's own PIN screen.
    struct Seen(bool, bool);
    impl Screen for Seen {
        fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
        fn guards(&self) -> bool {
            self.0
        }
        fn secret(&self) -> bool {
            self.1
        }
        fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
            Handled::No
        }
        fn on_gesture(&mut self, _: Gesture, _: &mut Cx<'_, '_>) -> Handled {
            GESTURES.with(|count| count.set(count.get() + 1));
            Handled::Yes
        }
    }

    struct Screens(&'static str);
    impl Plugin for Screens {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static(self.0), "Screens", Version::new(1, 0, 0))
                .uses(Uses::NOTHING)
        }
        fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
            if self.0 == "g" {
                r.add_screen(ScreenSpec::new(
                    LOCK,
                    "lock",
                    Box::new(|_, _| Box::new(Seen(true, false))),
                ))?;
                r.add_screen(ScreenSpec::new(
                    PIN,
                    "pin",
                    Box::new(|_, _| Box::new(Seen(false, true))),
                ))
            } else {
                r.add_screen(ScreenSpec::new(
                    OWN_PIN,
                    "pin",
                    Box::new(|_, _| Box::new(Seen(false, true))),
                ))?;
                // An input of its own the native filter can raise, the action
                // it runs, and an interface-6 filter that watches.
                r.add_input(PluginInput::new(Id::new_static("f.chord"), "f.chord"))?;
                r.add_action(
                    ActionSpec::new(Id::new_static("f.chord"), "f.chord")
                        .handler(|_, _, _| RAN.with(|count| count.set(count.get() + 1))),
                )?;
                r.add_keymap(alloc::borrow::Cow::Borrowed(
                    "[keymap.global]\ninput.f.chord = f.chord\n",
                ))?;
                r.add_input_filter(InputFilterSpec::new(
                    Id::new_static("f.legacy"),
                    "",
                    Box::new(|_, _| {
                        ORDER.with(|order| order.borrow_mut().push("legacy"));
                        InputVerdict::Pass
                    }),
                ))
            }
        }
    }

    /// The native filter: counts its calls and passes every input.
    struct Counting {
        depth: FlatDepth,
        calls: Rc<Cell<u32>>,
        guard: crate::abi7::calls::Instance,
        live: Cell<bool>,
    }
    impl Dispatch for Counting {
        fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
            unreachable!()
        }
        fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
            unreachable!()
        }
        fn draw(
            &self,
            _: u32,
            _: &wire::DrawFrame<'_>,
            _: &Env<'_>,
            _: &mut paperpaw_graphics::Canvas<'_>,
        ) -> Result<i32, i32> {
            unreachable!()
        }
        fn dropped(&self, _: u32, _: i32) {}
        fn stage(&self, _: u32, _: &[u8], out: &mut [u8], _: &Env<'_>) -> Result<i32, i32> {
            let _guard = self
                .guard
                .enter(&self.depth.as_depth().0)
                .map_err(|_| ERR_BUSY)?;
            self.calls.set(self.calls.get() + 1);
            ORDER.with(|order| order.borrow_mut().push("native"));
            let length = verdict(MODE.replace(PASS), out)?;
            Ok(i32::try_from(length).unwrap())
        }
    }
    impl NativeInstance for Counting {
        fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
            visit(Some(&[]));
        }
        fn running(&self) -> bool {
            self.live.get()
        }
        fn stop(&self) {
            self.live.set(false);
        }
        fn callback_depth(&self) -> u8 {
            self.depth.as_depth().0.current()
        }
    }

    /// A kernel with the interface-6 plugin `g` (lock and PIN screens), `f`
    /// (a PIN screen) and the native plugin `f` filtering `core.input-filter`.
    fn world() -> (crate::Kernel, Rc<Cell<u32>>) {
        MODE.set(PASS);
        RAN.set(0);
        GESTURES.set(0);
        ORDER.with(|order| order.borrow_mut().clear());
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .plugin(Screens("g"))
            .plugin(Screens("f"))
            .build()
            .unwrap();
        kernel.start(paperpaw_platform::Instant(0));
        let depth = FlatDepth::try_new().unwrap();
        kernel.shared.native_depth = Some(depth.try_clone().unwrap());
        let directory =
            crate::native_host::shared::Shared::try_new(NativeDeclarations::default()).unwrap();
        let calls = Rc::new(Cell::new(0));
        let lease = NativeLease::try_new(Counting {
            depth,
            calls: Rc::clone(&calls),
            guard: crate::abi7::calls::Instance::new(),
            live: Cell::new(true),
        })
        .unwrap();
        let key = kernel
            .shared
            .native_instances
            .test_insert(Id::new_static("f"), Some(lease.try_clone().unwrap()), true)
            .unwrap();
        let rows = [
            Record::RegistrationStage(wire::RegistrationStage {
                stage: stage::KEY,
                token: 1,
                order: 0,
                signature: 0,
            }),
            Record::InputSpec(wire::InputSpec {
                name: "chord",
                title: "f.chord",
                kind: 0,
                need: 0,
                capability: 0,
                flags: 0,
            }),
            Record::InputSpec(wire::InputSpec {
                name: "flick",
                title: "f.flick",
                kind: 1,
                need: 0,
                capability: 0,
                flags: 0,
            }),
        ];
        let mut bytes = [0; 128];
        let length = wire::records_encode(&rows, &mut bytes).unwrap();
        let bytes = &bytes[..length];
        let decoded = wire::records_decode(bytes).unwrap();
        let mut raw = RawDeclarations::default();
        let prepared = raw.prepare(None, Some(bytes), &decoded).unwrap();
        raw.commit(prepared);
        let mut allocations = Allocations::default();
        let pending =
            native_declarations::pending(&Id::new_static("f"), key, lease, raw, &mut allocations)
                .unwrap();
        directory
            .stage(pending, key, &mut allocations)
            .unwrap()
            .commit();
        kernel.shared.registry.native_declarations = Some(directory);
        (kernel, calls)
    }

    fn key(kernel: &mut crate::Kernel, at: u64) {
        let now = paperpaw_platform::Instant(at);
        kernel.handle(now, press("up"));
        kernel.handle(
            now,
            InputEvent::Button {
                name: "up",
                state: ButtonState::Released,
            },
        );
    }

    #[test]
    fn a_native_filter_is_called_on_every_key_and_listed_once() {
        let (mut kernel, calls) = world();
        key(&mut kernel, 10);
        assert_eq!(calls.get(), 2);
        // The list is kept while the declarations stay as they are.
        let revision = kernel.shared.native_filters.as_ref().map(|c| c.revision);
        assert!(revision.is_some());
        key(&mut kernel, 20);
        assert_eq!(calls.get(), 4);
        assert_eq!(
            kernel.shared.native_filters.as_ref().map(|c| c.revision),
            revision
        );
    }

    #[test]
    fn the_list_is_read_again_when_the_declarations_change_or_a_filter_failed() {
        let revision = |kernel: &crate::Kernel| {
            let directory = kernel.shared.registry.native_declarations.as_ref().unwrap();
            directory.store.borrow().revision()
        };
        // A filter that stopped is not called: the input passes.
        let (mut kernel, calls) = world();
        let owner = kernel.shared.native_instances.find("f").unwrap();
        key(&mut kernel, 10);
        assert!(kernel.shared.native_filters.is_some());
        kernel.shared.native_instances.stop(owner).unwrap();
        let before = calls.get();
        key(&mut kernel, 20);
        assert_eq!(calls.get(), before);
        // An owner taken out of the store changes its revision: the list kept
        // is not good any more.
        let (mut kernel, calls) = world();
        let owner = kernel.shared.native_instances.find("f").unwrap();
        key(&mut kernel, 10);
        let kept = revision(&kernel);
        kernel
            .shared
            .registry
            .native_declarations
            .as_ref()
            .unwrap()
            .store
            .borrow_mut()
            .remove_host_owner(owner);
        assert_ne!(revision(&kernel), kept);
        let before = calls.get();
        key(&mut kernel, 20);
        assert_eq!(calls.get(), before);
        assert_eq!(
            kernel
                .shared
                .native_filters
                .as_ref()
                .map(|c| c.filters.len()),
            Some(0)
        );
    }

    #[test]
    fn another_plugins_filter_is_not_called_under_a_lock_or_a_pin_screen() {
        for screen in [LOCK, PIN] {
            let (mut kernel, calls) = world();
            kernel.open_screen(paperpaw_platform::Instant(5), screen.clone(), Params::new());
            key(&mut kernel, 10);
            assert_eq!(calls.get(), 0, "{screen}");
        }
    }

    #[test]
    fn the_filter_of_the_plugin_whose_screen_it_is_still_runs() {
        let (mut kernel, calls) = world();
        kernel.open_screen(paperpaw_platform::Instant(5), OWN_PIN, Params::new());
        key(&mut kernel, 10);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_raised_input_runs_what_the_key_maps_bind_and_the_key_goes_nowhere_else() {
        let (mut kernel, calls) = world();
        MODE.set(RAISE);
        kernel.handle(paperpaw_platform::Instant(10), press("up"));
        assert_eq!((calls.get(), RAN.get()), (1, 1));
        // The interface-6 filters did not see the raised key: the chain ended.
        assert_eq!(ORDER.with(|order| order.borrow().clone()), ["native"]);
        // An input the plugin did not declare: the key goes on, nothing runs.
        MODE.set(RAISE_UNDECLARED);
        kernel.handle(paperpaw_platform::Instant(20), press("up"));
        assert_eq!(RAN.get(), 1);
        assert_eq!(
            ORDER.with(|order| order.borrow().clone()),
            ["native", "native", "legacy"]
        );
    }

    #[test]
    fn a_replaced_gesture_is_the_kernels_own_gesture_for_the_screen_on_top() {
        let (mut kernel, _) = world();
        kernel.open_screen(paperpaw_platform::Instant(5), OWN_PIN, Params::new());
        MODE.set(GESTURE);
        kernel.handle(paperpaw_platform::Instant(10), press("up"));
        assert_eq!(GESTURES.get(), 1);
        // Only that: the key itself did not go on.
        assert_eq!(ORDER.with(|order| order.borrow().clone()), ["native"]);
    }

    #[test]
    fn a_made_gesture_belongs_to_the_screen_on_top_not_to_the_last_one_touched() {
        let (mut kernel, _) = world();
        kernel.open_screen(paperpaw_platform::Instant(5), OWN_PIN, Params::new());
        // A stale serial from an earlier touch of a screen that is gone.
        kernel.input.pressed_on = Some(u32::MAX);
        MODE.set(GESTURE);
        kernel.handle(paperpaw_platform::Instant(10), press("up"));
        let top = kernel.stack.last().map(|top| top.serial);
        assert!(top.is_some());
        assert_eq!(kernel.input.pressed_on, top);
    }

    #[test]
    fn the_native_chain_comes_first_and_a_take_ends_it() {
        let (mut kernel, calls) = world();
        kernel.handle(paperpaw_platform::Instant(10), press("up"));
        assert_eq!(
            ORDER.with(|order| order.borrow().clone()),
            ["native", "legacy"]
        );
        ORDER.with(|order| order.borrow_mut().clear());
        MODE.set(TAKE);
        kernel.handle(paperpaw_platform::Instant(20), press("down"));
        assert_eq!(ORDER.with(|order| order.borrow().clone()), ["native"]);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn power_is_not_filtered_and_a_failing_or_stopped_filter_passes_the_key_logged_once() {
        let (mut kernel, calls) = world();
        kernel.handle(paperpaw_platform::Instant(10), press(buttons::POWER));
        assert_eq!(calls.get(), 0);
        ORDER.with(|order| order.borrow_mut().clear());
        // A failing callback: the key passes (the interface-6 filter sees it),
        // and the warning comes once while it keeps failing.
        let told = |kernel: &mut crate::Kernel| {
            kernel
                .take_warnings()
                .iter()
                .filter(|warning| warning.message.contains("native input filter"))
                .count()
        };
        MODE.set(FAIL);
        kernel.handle(paperpaw_platform::Instant(20), press("up"));
        assert_eq!(
            ORDER.with(|order| order.borrow().clone()),
            ["native", "legacy"]
        );
        assert_eq!(told(&mut kernel), 1);
        MODE.set(FAIL);
        kernel.handle(paperpaw_platform::Instant(30), press("up"));
        assert_eq!(told(&mut kernel), 0);
        // A stopped plugin is a failed call: told again once the filter had
        // answered well in between.
        kernel.handle(paperpaw_platform::Instant(40), press("up"));
        let owner = kernel.shared.native_instances.find("f").unwrap();
        kernel.shared.native_instances.stop(owner).unwrap();
        ORDER.with(|order| order.borrow_mut().clear());
        let before = calls.get();
        kernel.handle(paperpaw_platform::Instant(50), press("up"));
        assert_eq!(calls.get(), before);
        assert_eq!(ORDER.with(|order| order.borrow().clone()), ["legacy"]);
        assert_eq!(told(&mut kernel), 1);
    }

    #[test]
    fn only_an_input_of_the_right_kind_the_plugin_declared_is_declared() {
        let (kernel, _) = world();
        let store = &kernel
            .shared
            .registry
            .native_declarations
            .as_ref()
            .unwrap()
            .store;
        let has = |plugin, name| declared_input(store, plugin, name);
        assert!(has("f", "input.f.chord"));
        assert!(has("f", "gesture.f.flick"));
        // The wrong kind, another plugin's, an unknown name, no prefix.
        assert!(!has("f", "gesture.f.chord"));
        assert!(!has("f", "input.f.flick"));
        assert!(!has("f", "input.g.chord"));
        assert!(!has("g", "input.f.chord"));
        assert!(!has("f", "input.f.nothing"));
        assert!(!has("f", "f.chord"));
        assert!(!has("f", "button.menu"));
    }
}
