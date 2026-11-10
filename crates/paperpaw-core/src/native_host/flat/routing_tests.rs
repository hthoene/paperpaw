//! Recipient restrictions are inherited from the kernel's existing router.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use alloc::{rc::Rc, vec::Vec};
use core::cell::Cell;
use paperpaw_sdk::{
    Cx, Handled, Screen, ScreenSpec, Version,
    builtin::{actions, events as builtin},
};

struct Instance {
    scope: Pin<Box<EnvironmentScope>>,
    calls: Rc<Cell<usize>>,
}
impl FlatInstance for Instance {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        visit(FlatRegistration::Event {
            event_tag: 32,
            index: 0,
        });
    }
    fn environment(&self) -> Pin<&EnvironmentScope> {
        self.scope.as_ref()
    }
    fn dispatch_event(
        &self,
        _: usize,
        _: &[u8],
        _: &mut [u8],
        _: &DispatchDepth,
    ) -> Result<i32, Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(0)
    }
    fn event_dropped(&self, _: u32, why: DropReason) {
        panic!("unexpected drop {why:?}");
    }
}
fn flat(calls: &Rc<Cell<usize>>, host: &FlatHost) -> FlatPlugin {
    FlatPlugin::new(
        Manifest::new(Id::new_static("flat"), "Flat", Version::new(1, 0, 0)),
        Box::new(Instance {
            scope: Box::pin(EnvironmentScope::new()),
            calls: Rc::clone(calls),
        }),
        crate::native_host::flat::FlatDepth::try_new().unwrap(),
        host,
    )
    .unwrap()
}
fn load_flat(builder: crate::KernelBuilder, calls: &Rc<Cell<usize>>) -> crate::Kernel {
    let calls = Rc::clone(calls);
    test_loader::load(builder, &["flat"], move |code| {
        Ok(Box::new(flat(&calls, code.host)))
    })
}
struct RustListener(Rc<Cell<usize>>);
impl Plugin for RustListener {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("rust"), "Rust", Version::new(1, 0, 0))
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let calls = Rc::clone(&self.0);
        r.subscribe(
            builtin::STARTED,
            Box::new(move |_, _, _| calls.set(calls.get() + 1)),
        )
    }
}

#[test]
fn only_and_password_restrictions_apply_to_flat_and_rust_handlers() {
    let a = Rc::new(Cell::new(0));
    let b = Rc::new(Cell::new(0));
    let mut kernel = load_flat(
        crate::Kernel::builder(crate::tests::DEVICE).plugin(RustListener(Rc::clone(&b))),
        &a,
    );
    kernel.emit_to(builtin::STARTED.id(), &Params::new(), Some("rust"));
    assert_eq!((a.get(), b.get()), (0, 1));
    kernel.emit_to(builtin::STARTED.id(), &Params::new(), Some("flat"));
    assert_eq!((a.get(), b.get()), (1, 1));
    // Core owns these event names, so neither other namespace is their asker.
    kernel.emit(
        builtin::STARTED.id(),
        &Params::new().with("mode", "password"),
    );
    assert_eq!((a.get(), b.get()), (1, 1));
    kernel.emit(builtin::STARTED.id(), &Params::new());
    assert_eq!((a.get(), b.get()), (2, 2));
}

struct Guard;
impl Screen for Guard {
    fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &Env<'_>) {}
    fn guards(&self) -> bool {
        true
    }
    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action != actions::CONFIRM {
            return Handled::No;
        }
        cx.command(builtin::STARTED.emit(()));
        Handled::Yes
    }
}
struct GuardPlugin;
impl Plugin for GuardPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("guard"), "Guard", Version::new(1, 0, 0))
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("guard.screen"),
            "guard",
            Box::new(|_, _| Box::new(Guard)),
        ))
    }
}
#[test]
fn guard_owned_events_filter_flat_and_rust_handlers_identically() {
    let a = Rc::new(Cell::new(0));
    let b = Rc::new(Cell::new(0));
    let mut kernel = load_flat(
        crate::Kernel::builder(crate::tests::DEVICE)
            .plugin(RustListener(Rc::clone(&b)))
            .plugin(GuardPlugin),
        &a,
    );
    kernel.open_screen(
        paperpaw_platform::Instant(1),
        Id::new_static("guard.screen"),
        Params::new(),
    );
    kernel.run_action(paperpaw_platform::Instant(2), actions::CONFIRM);
    assert_eq!((a.get(), b.get()), (0, 0));
    kernel.send_event(
        paperpaw_platform::Instant(3),
        builtin::STARTED.id().clone(),
        Params::new(),
    );
    assert_eq!((a.get(), b.get()), (1, 1));
}

struct Reentrant<'a, 'env> {
    scope: Pin<Box<EnvironmentScope>>,
    env: &'a Env<'env>,
    calls: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
    host: FlatHost,
}
impl FlatInstance for Reentrant<'_, '_> {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        visit(FlatRegistration::Event {
            event_tag: 32,
            index: 0,
        });
    }
    fn environment(&self) -> Pin<&EnvironmentScope> {
        self.scope.as_ref()
    }
    fn dispatch_event(
        &self,
        _: usize,
        _: &[u8],
        _: &mut [u8],
        depth: &DispatchDepth,
    ) -> Result<i32, Error> {
        self.calls.set(self.calls.get() + 1);
        // Re-enter the exact handler used by FlatPlugin while its scope is live.
        // Admission must fail before dispatch_event can see plugin state again.
        deliver(self, depth, &self.host, 32, 0, self.env, &Params::new());
        Ok(0)
    }
    fn event_dropped(&self, tag: u32, why: DropReason) {
        assert_eq!((tag, why), (32, DropReason::Busy));
        self.drops.set(self.drops.get() + 1);
    }
}
struct ReentryDriver {
    host: FlatHost,
    calls: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
}
impl Plugin for ReentryDriver {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("flat"), "Reentry", Version::new(1, 0, 0))
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let calls = Rc::clone(&self.calls);
        let drops = Rc::clone(&self.drops);
        let meter = Rc::clone(&self.host.meter);
        let font_removals = Rc::clone(&self.host.font_removals);
        let registry_generation = Rc::clone(&self.host.registry_generation);
        r.subscribe(
            builtin::STARTED,
            Box::new(move |env, _, _| {
                let instance = Reentrant {
                    scope: Box::pin(EnvironmentScope::new()),
                    env,
                    calls: Rc::clone(&calls),
                    drops: Rc::clone(&drops),
                    host: FlatHost {
                        log: Default::default(),
                        meter: Rc::clone(&meter),
                        directory: None,
                        stack_rows: alloc::rc::Rc::new(Default::default()),
                        font_removals: Rc::clone(&font_removals),
                        registry_generation: Rc::clone(&registry_generation),
                    },
                };
                deliver(
                    &instance,
                    &DispatchDepth::new(),
                    &instance.host,
                    32,
                    0,
                    env,
                    &Params::new(),
                );
            }),
        )
    }
}
#[test]
fn callback_reentry_is_busy_before_plugin_callback_runs_twice() {
    let calls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let observed_calls = Rc::clone(&calls);
    let observed_drops = Rc::clone(&drops);
    let mut kernel = test_loader::load(
        crate::Kernel::builder(crate::tests::DEVICE),
        &["flat"],
        move |code| {
            Ok(Box::new(ReentryDriver {
                calls: Rc::clone(&observed_calls),
                drops: Rc::clone(&observed_drops),
                host: FlatHost {
                    log: Default::default(),
                    meter: Rc::clone(&code.host.meter),
                    directory: None,
                    stack_rows: Rc::clone(&code.host.stack_rows),
                    font_removals: Rc::clone(&code.host.font_removals),
                    registry_generation: Rc::clone(&code.host.registry_generation),
                },
            }))
        },
    );
    kernel.start(paperpaw_platform::Instant(1));
    assert_eq!((calls.get(), drops.get()), (1, 1));
}

#[test]
fn reply_lengths_are_checked_before_any_reply_can_be_read() {
    use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE};
    assert_eq!(reply_length(0, 0), Ok(0));
    assert_eq!(reply_length(1, 0), Err(DropReason::Plugin(ERR_INVALID)));
    assert_eq!(
        reply_length(i32::MAX, 0),
        Err(DropReason::Plugin(ERR_INVALID))
    );
    assert_eq!(
        reply_length(ERR_STATE, 8),
        Err(DropReason::Plugin(ERR_STATE))
    );
    assert_eq!(reply_length(0, 8), Ok(0));
    assert_eq!(reply_length(8, 8), Ok(8));
    assert_eq!(reply_length(9, 8), Err(DropReason::Plugin(ERR_INVALID)));
}

struct TickInstance {
    scope: Pin<Box<EnvironmentScope>>,
    ticks: Rc<Cell<Vec<u64>>>,
}
impl FlatInstance for TickInstance {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        visit(FlatRegistration::Event {
            event_tag: crate::native_host::registrations::EVENT_TICK_TAG,
            index: 0,
        });
        // Registered for another event too: it gets no tick.
        visit(FlatRegistration::Event {
            event_tag: 32,
            index: 1,
        });
    }
    fn environment(&self) -> Pin<&EnvironmentScope> {
        self.scope.as_ref()
    }
    fn dispatch_event(
        &self,
        index: usize,
        bytes: &[u8],
        _: &mut [u8],
        _: &DispatchDepth,
    ) -> Result<i32, Error> {
        let mut seen = self.ticks.take();
        if index == 0 {
            seen.push(crate::native_host::registrations::tick_decode(bytes).unwrap());
        } else {
            seen.push(u64::MAX);
        }
        self.ticks.set(seen);
        Ok(0)
    }
    fn event_dropped(&self, _: u32, why: DropReason) {
        panic!("unexpected drop {why:?}");
    }
}

/// Native plugins registered for `event.tick` get one tick a minute while the
/// kernel is ticked: the first call starts the minute, nothing comes early,
/// a long sleep gives one tick, and the plugin's other event registrations
/// get none.
#[test]
fn a_native_plugin_gets_one_tick_a_minute() {
    let ticks = Rc::new(Cell::new(Vec::new()));
    let seen = Rc::clone(&ticks);
    let mut kernel = test_loader::load(
        crate::Kernel::builder(crate::tests::DEVICE),
        &["flat"],
        move |code| {
            Ok(Box::new(
                FlatPlugin::new(
                    Manifest::new(Id::new_static("flat"), "Flat", Version::new(1, 0, 0)),
                    Box::new(TickInstance {
                        scope: Box::pin(EnvironmentScope::new()),
                        ticks: Rc::clone(&seen),
                    }),
                    crate::native_host::flat::FlatDepth::try_new().unwrap(),
                    code.host,
                )
                .unwrap(),
            ))
        },
    );
    let log = |kernel: &mut crate::Kernel, at: u64| {
        kernel.tick(paperpaw_platform::Instant(at));
        let seen = ticks.take();
        ticks.set(seen.clone());
        seen
    };
    assert!(
        log(&mut kernel, 1_000).is_empty(),
        "the first call starts the minute"
    );
    assert!(log(&mut kernel, 60_999).is_empty(), "not a minute yet");
    assert_eq!(log(&mut kernel, 61_000), [61_000]);
    assert_eq!(
        log(&mut kernel, 100_000),
        [61_000],
        "none before the next minute"
    );
    // A device that slept for an hour gets one tick when it wakes.
    assert_eq!(log(&mut kernel, 3_700_000), [61_000, 3_700_000]);
    assert_eq!(log(&mut kernel, 3_700_500), [61_000, 3_700_000]);
    // The clock went back: the minute starts again there, no tick stalls
    // until the old time is passed.
    assert_eq!(log(&mut kernel, 10_000), [61_000, 3_700_000]);
    assert_eq!(log(&mut kernel, 69_999), [61_000, 3_700_000]);
    assert_eq!(log(&mut kernel, 70_000), [61_000, 3_700_000, 70_000]);
}

/// A plugin whose tick callback fails (here: refused admission).
struct FailingTick {
    scope: Pin<Box<EnvironmentScope>>,
    drops: Rc<Cell<usize>>,
}
impl FlatInstance for FailingTick {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        visit(FlatRegistration::Event {
            event_tag: crate::native_host::registrations::EVENT_TICK_TAG,
            index: 0,
        });
    }
    fn environment(&self) -> Pin<&EnvironmentScope> {
        self.scope.as_ref()
    }
    fn dispatch_event(
        &self,
        _: usize,
        _: &[u8],
        _: &mut [u8],
        _: &DispatchDepth,
    ) -> Result<i32, Error> {
        Err(Error::State)
    }
    fn event_dropped(&self, _: u32, _: DropReason) {
        self.drops.set(self.drops.get() + 1);
    }
}

/// A tick a plugin refuses is told as a drop for that plugin alone: the
/// healthy plugin beside it still gets every tick.
#[test]
fn a_failing_tick_plugin_does_not_stop_the_healthy_one() {
    let ticks = Rc::new(Cell::new(Vec::new()));
    let drops = Rc::new(Cell::new(0));
    let (seen, dropped) = (Rc::clone(&ticks), Rc::clone(&drops));
    let mut kernel = test_loader::load(
        crate::Kernel::builder(crate::tests::DEVICE),
        &["bad", "good"],
        move |code| {
            let manifest = Manifest::new(code.id.clone(), "Flat", Version::new(1, 0, 0));
            let depth = crate::native_host::flat::FlatDepth::try_new().unwrap();
            let instance: Box<dyn FlatInstance> = if code.id.as_str() == "bad" {
                Box::new(FailingTick {
                    scope: Box::pin(EnvironmentScope::new()),
                    drops: Rc::clone(&dropped),
                })
            } else {
                Box::new(TickInstance {
                    scope: Box::pin(EnvironmentScope::new()),
                    ticks: Rc::clone(&seen),
                })
            };
            Ok(Box::new(
                FlatPlugin::new(manifest, instance, depth, code.host).unwrap(),
            ))
        },
    );
    for at in [1_000, 61_000, 121_000] {
        kernel.tick(paperpaw_platform::Instant(at));
    }
    let seen = ticks.take();
    assert_eq!(seen, [61_000, 121_000]);
    assert_eq!(drops.get(), 2, "the failing plugin was told each time");
}
