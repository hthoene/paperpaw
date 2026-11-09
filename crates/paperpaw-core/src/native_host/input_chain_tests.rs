//! The `core.input-filter` chain on the kernel's input path: an event as the
//! stage's record and back, and what a swallowing, replacing, failing filter
//! (and Power) makes of an input.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native_host::codec;
use alloc::vec;
use paperpaw_native::host_glue::ERR_STATE;

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
    let mut chain = InputFilters::new();
    let mut runner = Scripted { answers, called: 0 };
    let policy = Policy {
        touch: true,
        secret: false,
        runs: &all,
        declared: &none,
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
    use core::cell::Cell;
    use paperpaw_graphics::Canvas;
    use paperpaw_native::host_glue::ERR_BUSY;
    use paperpaw_sdk::{
        Cx, Env, Handled, Id, Manifest, Params, Plugin, RegisterError, Registrar, Screen,
        ScreenSpec, Uses, Version,
    };

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
            let length = stage::FilterVerdict::new(0, "", &[], "").encode(out)?;
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
        let rows = [Record::RegistrationStage(wire::RegistrationStage {
            stage: stage::KEY,
            token: 1,
            order: 0,
            signature: 0,
        })];
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
            let revision = directory.store.borrow().revision();
            revision
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
}
