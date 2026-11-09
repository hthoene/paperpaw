//! A service event's callback has the command stage: what it submits runs after
//! it returns, only on 0 or 1; an error drops the whole batch; and each batch
//! runs before the next callback is called.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::{
    commands::{WireCommand, commands_decode, commands_encode},
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    work_sched::{Limits, Power, Spec, WorkEnv, WorkSched},
    work_service::WorkServices,
};
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Id, ScreenKey, StackEntry};

/// A plugin that submits a redraw in every callback and returns `answer`.
struct Probe {
    answer: Rc<Cell<Result<i32, i32>>>,
    calls: Rc<Cell<u32>>,
    submits: bool,
    /// What it submits: 0 a redraw, 1 an `open` of its screen.
    command: Rc<Cell<u8>>,
    /// The status of its last `submit`.
    last: Rc<Cell<i32>>,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        tag == 103 || tag == wire::EVENT_IMAGE_READY_TAG
    }
    fn service_event(&self, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        self.calls.set(self.calls.get() + 1);
        self.answer.get()
    }
    fn service_event_staged(
        &self,
        _: &[u8],
        _: &paperpaw_sdk::Env<'_>,
        stage: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.calls.set(self.calls.get() + 1);
        if self.submits {
            let mut bytes = [0; 64];
            let command = if self.command.get() == 0 {
                WireCommand::Redraw { mode: 0 }
            } else {
                WireCommand::Open {
                    target: "a.screen",
                    mode: 0,
                    slot: "",
                    handover: 0,
                }
            };
            let len = commands_encode(&[command], &mut bytes).unwrap();
            let batch = commands_decode(&bytes[..len]).unwrap();
            self.last.set(stage.stage(&batch));
        }
        self.answer.get()
    }
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

struct World {
    kernel: crate::Kernel,
    owner: u64,
    lease: NativeLease,
    answer: Rc<Cell<Result<i32, i32>>>,
    calls: Rc<Cell<u32>>,
    command: Rc<Cell<u8>>,
    last: Rc<Cell<i32>>,
}

fn world(submits: bool) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    // The plugin's screen is on the stack: a redraw is for it.
    kernel.shared.stack.push(StackEntry::new(
        Id::new_static("a.screen"),
        None,
        false,
        ScreenKey::new(7),
    ));
    let answer = Rc::new(Cell::new(Ok(OK)));
    let calls = Rc::new(Cell::new(0));
    let command = Rc::new(Cell::new(0));
    let last = Rc::new(Cell::new(OK));
    let probe = || Probe {
        answer: Rc::clone(&answer),
        calls: Rc::clone(&calls),
        submits,
        command: Rc::clone(&command),
        last: Rc::clone(&last),
    };
    let lease = NativeLease::try_new(probe()).unwrap();
    let owner = kernel
        .shared
        .native_instances
        .test_insert(
            Id::try_parse("a").unwrap(),
            Some(NativeLease::try_new(probe()).unwrap()),
            true,
        )
        .unwrap();
    World {
        kernel,
        owner,
        lease,
        answer,
        calls,
        command,
        last,
    }
}

impl World {
    fn call(&self) -> Result<i32, i32> {
        self.kernel.native_event_call(self.owner, &self.lease, &[])
    }
}

#[test]
fn what_a_service_callback_submits_runs_after_it_returned() {
    let mut w = world(true);
    assert_eq!(w.call(), Ok(OK));
    // Kept, not run inside the callback.
    assert!(!w.kernel.output.redraw);
    assert!(w.kernel.shared.native_instances.outbox.full());
    w.kernel.native_outbox_run();
    assert!(w.kernel.output.redraw, "its redraw ran");
    assert!(!w.kernel.shared.native_instances.outbox.full());
}

#[test]
fn a_callback_that_returns_an_error_has_its_batch_dropped() {
    let mut w = world(true);
    w.answer.set(Err(ERR_INVALID));
    assert_eq!(w.call(), Err(ERR_INVALID));
    assert!(!w.kernel.shared.native_instances.outbox.full());
    w.kernel.native_outbox_run();
    assert!(!w.kernel.output.redraw);
    // A status other than 0 or 1 is no success either.
    w.answer.set(Ok(2));
    assert_eq!(w.call(), Ok(2));
    assert!(!w.kernel.shared.native_instances.outbox.full());
}

#[test]
fn no_callback_is_called_while_a_batch_waits() {
    let mut w = world(true);
    assert_eq!(w.call(), Ok(OK));
    assert_eq!(w.calls.get(), 1);
    assert_eq!(w.call(), Err(paperpaw_native::host_glue::ERR_BUSY));
    assert_eq!(w.calls.get(), 1, "refused before the plugin was called");
    w.kernel.native_outbox_run();
    assert_eq!(w.call(), Ok(OK));
    assert_eq!(w.calls.get(), 2);
}

#[test]
fn a_callback_that_stages_nothing_leaves_the_outbox_empty() {
    let w = world(false);
    assert_eq!(w.call(), Ok(OK));
    assert!(!w.kernel.shared.native_instances.outbox.full());
}

struct Env;
impl WorkEnv for Env {
    fn now_ms(&self) -> u64 {
        1_000
    }
    fn power(&self) -> Power {
        Power {
            level: 0,
            stretch: 1,
            radio_on: true,
        }
    }
    fn screen_hold(&self) -> bool {
        false
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
}

#[test]
fn two_callbacks_in_one_pump_step_each_run_their_batch_before_the_next() {
    let mut w = world(true);
    let sched = Rc::new(WorkSched::new(Limits::OTHER));
    w.kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_work(WorkServices {
            sched: Rc::clone(&sched),
            env: Rc::new(Env),
        })
        .unwrap();
    for (token, key) in [(1, "one"), (2, "two")] {
        sched
            .declare(
                w.owner,
                &Spec {
                    token,
                    needs: 0,
                    flags: 0,
                    moments: 0,
                    after_ms: 0,
                    within_ms: 0,
                    every_ms: 0,
                    at_ms: 0,
                    key,
                },
                1_000,
            )
            .unwrap();
    }
    w.kernel.now = Instant(1_000);
    w.kernel.native_work_pump();
    // The pump itself ran the first batch before it called the second
    // callback: a waiting batch refuses the next call (`ERR_BUSY`, the
    // plugin is not called), so both were called.
    assert_eq!(w.calls.get(), 2);
    assert!(w.kernel.output.redraw);
    assert!(!w.kernel.shared.native_instances.outbox.full());
}

/// A pump with one plugin whose work is due.
fn run_work(w: &mut World) {
    let sched = Rc::new(WorkSched::new(Limits::OTHER));
    w.kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_work(WorkServices {
            sched: Rc::clone(&sched),
            env: Rc::new(Env),
        })
        .unwrap();
    sched
        .declare(
            w.owner,
            &Spec {
                token: 1,
                needs: 0,
                flags: 0,
                moments: 0,
                after_ms: 0,
                within_ms: 0,
                every_ms: 0,
                at_ms: 0,
                key: "job",
            },
            1_000,
        )
        .unwrap();
    w.kernel.now = Instant(1_000);
    w.kernel.native_work_pump();
}

#[test]
fn a_runs_callback_may_not_open_a_screen_but_may_redraw() {
    use paperpaw_native::host_glue::ERR_STATE;
    // `open` from `work_due`: refused at submit, nothing is opened or kept.
    let mut w = world(true);
    w.command.set(1);
    run_work(&mut w);
    assert_eq!(w.calls.get(), 1);
    assert_eq!(w.last.get(), ERR_STATE);
    assert!(!w.kernel.shared.native_instances.outbox.full());
    assert!(!w.kernel.output.redraw);
    // A redraw of its own screen from the same callback runs.
    let mut w = world(true);
    run_work(&mut w);
    assert_eq!(w.last.get(), OK);
    assert!(w.kernel.output.redraw);
    // Outside a run (an `image_ready`, say) the same `open` is not refused
    // for being quiet: it is judged as any callback's (here the screen is not
    // registered, so `ERR_NOT_FOUND`).
    let w = world(true);
    w.command.set(1);
    w.call().unwrap();
    assert_eq!(w.last.get(), paperpaw_native::host_glue::ERR_NOT_FOUND);
}
