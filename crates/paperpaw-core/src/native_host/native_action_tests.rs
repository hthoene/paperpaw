//! A native plugin's action reaches its plugin in the call that ran it, with
//! the callback's command stage, whoever ran it: `Command::RunAction` (menu
//! rows, rules, widgets) runs inside `run_commands`, after the pump.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::{
    native_instances::NativeInstance, native_screen::Dispatch, native_values::Callback,
    registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Command, Env, Id, Params};

/// A plugin that notes the token and the record of each `event_staged`.
struct Noting {
    seen: Rc<RefCell<Vec<(u32, String)>>>,
    dropped: Rc<RefCell<Vec<(u32, i32)>>>,
    alive: Cell<bool>,
    status: Result<i32, i32>,
}

impl NativeInstance for Noting {
    fn running(&self) -> bool {
        self.alive.get()
    }
    fn stop(&self) {
        self.alive.set(false);
    }
    fn event_staged(
        &self,
        token: u32,
        bytes: &[u8],
        _: &Env<'_>,
        _: &mut crate::native_host::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        if let Ok(wire::Record::EventActionRun(run)) = wire::records_decode_single(bytes) {
            self.seen
                .borrow_mut()
                .push((token, String::from(run.action)));
        }
        self.status
    }
}

impl Dispatch for Noting {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, token: u32, status: i32) {
        self.dropped.borrow_mut().push((token, status));
    }
}

struct World {
    kernel: crate::Kernel,
    seen: Rc<RefCell<Vec<(u32, String)>>>,
    dropped: Rc<RefCell<Vec<(u32, i32)>>>,
    lease: NativeLease,
    owner: u64,
}

fn world(status: Result<i32, i32>) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let dropped = Rc::new(RefCell::new(Vec::new()));
    let lease = NativeLease::try_new(Noting {
        seen: Rc::clone(&seen),
        dropped: Rc::clone(&dropped),
        alive: Cell::new(true),
        status,
    })
    .unwrap();
    let owner = kernel
        .shared
        .native_instances
        .test_insert(
            Id::try_parse("n").unwrap(),
            Some(lease.try_clone().unwrap()),
            true,
        )
        .unwrap();
    World {
        kernel,
        seen,
        dropped,
        lease,
        owner,
    }
}

/// The action `n.open` (token 7) as an install makes it, in the registry.
fn declare(w: &mut World) {
    let callback = Callback {
        id: Id::try_parse("n.open").unwrap(),
        title: String::from("Open"),
        token: 7,
        flags: 0,
    };
    let spec = spec(
        callback,
        w.lease.try_clone().unwrap(),
        w.owner,
        &w.kernel.shared.native_instances,
    )
    .unwrap();
    w.kernel.shared.actions.insert(spec);
}

#[test]
fn an_action_run_by_a_command_reaches_its_plugin_in_the_same_run() {
    let mut w = world(Ok(0));
    declare(&mut w);
    // What a menu row, a rule or a widget asks for.
    w.kernel.output.commands.push(Command::RunAction(
        Id::try_parse("n.open").unwrap(),
        Params::new(),
    ));
    w.kernel.run_commands();
    assert_eq!(*w.seen.borrow(), [(7, String::from("n.open"))]);
    assert!(w.dropped.borrow().is_empty());
}

#[test]
fn an_action_run_directly_reaches_its_plugin_in_the_same_call() {
    let mut w = world(Ok(0));
    declare(&mut w);
    let _ = w
        .kernel
        .run_action(Instant(10), Id::try_parse("n.open").unwrap());
    assert_eq!(*w.seen.borrow(), [(7, String::from("n.open"))]);
}

#[test]
fn more_actions_than_the_queue_keeps_are_refused_and_said_so() {
    let mut w = world(Ok(0));
    declare(&mut w);
    let queue = w.kernel.shared.native_instances.action_queue().unwrap();
    // Run without a delivery between: the queue keeps `QUEUE_MAX`.
    let action = Id::try_parse("n.open").unwrap();
    for _ in 0..QUEUE_MAX + 2 {
        w.kernel.dispatch(action.clone());
    }
    assert_eq!(queue.refused(), 2);
    // What was kept is delivered.
    w.kernel.run_commands();
    assert_eq!(w.seen.borrow().len(), QUEUE_MAX);
}

#[test]
fn a_stopped_plugin_leaves_its_action_without_effect() {
    let mut w = world(Ok(0));
    declare(&mut w);
    w.lease.stop();
    let _ = w
        .kernel
        .run_action(Instant(10), Id::try_parse("n.open").unwrap());
    assert!(w.seen.borrow().is_empty());
    // Nothing waits for it afterwards.
    let queue = w.kernel.shared.native_instances.action_queue().unwrap();
    assert!(queue.take().is_none());
}

#[test]
fn a_status_other_than_0_or_1_is_reported_to_the_plugin() {
    for status in [Ok(5), Err(-6)] {
        let mut w = world(status);
        declare(&mut w);
        let _ = w
            .kernel
            .run_action(Instant(10), Id::try_parse("n.open").unwrap());
        assert_eq!(w.dropped.borrow().len(), 1, "{status:?}");
    }
}
