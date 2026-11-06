//! Recipient restrictions are inherited from the kernel's existing router.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use alloc::rc::Rc;
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
    fn dispatch_event(&self, _: usize, _: &[u8], _: &DispatchDepth) -> Result<i32, Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(0)
    }
    fn event_dropped(&self, _: u32, why: DropReason) {
        panic!("unexpected drop {why:?}");
    }
}
fn flat(calls: &Rc<Cell<usize>>) -> FlatPlugin {
    FlatPlugin::new(
        Manifest::new(Id::new_static("flat"), "Flat", Version::new(1, 0, 0)),
        Box::new(Instance {
            scope: Box::pin(EnvironmentScope::new()),
            calls: Rc::clone(calls),
        }),
        Rc::new(DispatchDepth::new()),
    )
    .unwrap()
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
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(flat(&a))
        .plugin(RustListener(Rc::clone(&b)))
        .build()
        .unwrap();
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
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(flat(&a))
        .plugin(RustListener(Rc::clone(&b)))
        .plugin(GuardPlugin)
        .build()
        .unwrap();
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
    fn dispatch_event(&self, _: usize, _: &[u8], depth: &DispatchDepth) -> Result<i32, Error> {
        self.calls.set(self.calls.get() + 1);
        // Re-enter the exact handler used by FlatPlugin while its scope is live.
        // Admission must fail before dispatch_event can see plugin state again.
        deliver(self, depth, 32, 0, self.env, &Params::new());
        Ok(0)
    }
    fn event_dropped(&self, tag: u32, why: DropReason) {
        assert_eq!((tag, why), (32, DropReason::Busy));
        self.drops.set(self.drops.get() + 1);
    }
}
struct ReentryDriver {
    calls: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
}
impl Plugin for ReentryDriver {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("reentry"), "Reentry", Version::new(1, 0, 0))
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let calls = Rc::clone(&self.calls);
        let drops = Rc::clone(&self.drops);
        r.subscribe(
            builtin::STARTED,
            Box::new(move |env, _, _| {
                let instance = Reentrant {
                    scope: Box::pin(EnvironmentScope::new()),
                    env,
                    calls: Rc::clone(&calls),
                    drops: Rc::clone(&drops),
                };
                deliver(&instance, &DispatchDepth::new(), 32, 0, env, &Params::new());
            }),
        )
    }
}
#[test]
fn callback_reentry_is_busy_before_plugin_callback_runs_twice() {
    let calls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(ReentryDriver {
            calls: Rc::clone(&calls),
            drops: Rc::clone(&drops),
        })
        .build()
        .unwrap();
    kernel.start(paperpaw_platform::Instant(1));
    assert_eq!((calls.get(), drops.get()), (1, 1));
}
