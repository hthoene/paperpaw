//! A native plugin's action reaches its plugin in the call that ran it, with
//! the callback's command stage, whoever ran it: `Command::RunAction` (menu
//! rows, rules, widgets) runs inside `run_commands`, after the pump.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::{
    native_instances::NativeInstance, native_screen::Dispatch, registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Command, Env, Id, Params};

struct Again {
    queue: crate::native_host::shared::Shared<ActionQueue>,
    lease: NativeLease,
    owner: u64,
}

/// A plugin that notes the token and the record of each `event_staged`.
struct Noting {
    /// Runs the action again from its own callback (as a staged `RunAction`
    /// of itself does): the queue, the lease and the key to do it with.
    again: Rc<RefCell<Option<Again>>>,
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
        if let Some(again) = self.again.borrow().as_ref() {
            let _ = again.queue.push(Pending {
                owner: again.owner,
                lease: again.lease.try_clone().unwrap(),
                token,
                bytes: bytes.to_vec(),
            });
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
    again: Rc<RefCell<Option<Again>>>,
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
    let again = Rc::new(RefCell::new(None));
    let lease = NativeLease::try_new(Noting {
        again: Rc::clone(&again),
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
        again,
        kernel,
        seen,
        dropped,
        lease,
        owner,
    }
}

/// The action `n.open` (token 7) as an install makes it, in the registry:
/// without a handler, run by the plugin's key and token (#248).
fn declare(w: &mut World) {
    let id = Id::try_parse("n.open").unwrap();
    let action = paperpaw_sdk::NativeAction::new(id, "Open".into(), w.owner, 7);
    w.kernel.shared.actions.insert_native(action);
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
    // A waiting action keeps its plugin from sleeping again (#248, D3).
    let instances = &w.kernel.shared.native_instances;
    assert!(instances.waits_for(w.owner) && !instances.waits_for(w.owner + 1));
    // What was kept is delivered.
    w.kernel.run_commands();
    assert_eq!(w.seen.borrow().len(), QUEUE_MAX);
    assert!(!w.kernel.shared.native_instances.waits_for(w.owner));
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

#[test]
fn an_action_that_runs_itself_does_not_hang_the_run() {
    let mut w = world(Ok(0));
    declare(&mut w);
    *w.again.borrow_mut() = Some(Again {
        queue: w.kernel.shared.native_instances.action_queue().unwrap(),
        lease: w.lease.try_clone().unwrap(),
        owner: w.owner,
    });
    w.kernel.output.commands.push(Command::RunAction(
        Id::try_parse("n.open").unwrap(),
        Params::new(),
    ));
    // Each delivery queues the next: a round delivers what waited when it
    // began, so the rounds of `run_commands` bound the chain.
    w.kernel.run_commands();
    let delivered = w.seen.borrow().len();
    assert!(delivered >= 1, "{delivered}");
    assert!(delivered <= crate::MAX_COMMAND_ROUNDS, "{delivered}");
    // Not a cycle past the test.
    *w.again.borrow_mut() = None;
}

/// The action of a plugin that is no longer there finds no plugin to run:
/// nothing waits, nothing breaks, and a warning says so (Code Reviewer, #1177).
#[test]
fn the_action_of_a_plugin_not_installed_does_nothing() {
    let mut w = world(Ok(0));
    let id = Id::try_parse("gone.open").unwrap();
    let action = paperpaw_sdk::NativeAction::new(id.clone(), "Open".into(), w.owner + 1, 7);
    w.kernel.shared.actions.insert_native(action);
    let _ = w.kernel.run_action(Instant(10), id);
    assert!(w.seen.borrow().is_empty());
    // Said once, not dropped silently.
    let said = w
        .kernel
        .take_warnings()
        .into_iter()
        .filter(|warning| alloc::format!("{warning:?}").contains("its plugin is not running"))
        .count();
    assert_eq!(said, 1);
    let queue = w.kernel.shared.native_instances.action_queue().unwrap();
    assert!(queue.take().is_none());
}
