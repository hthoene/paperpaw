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
use core::cell::{Cell, RefCell};
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
    /// Its name and the log of who was called, in order (fairness).
    order: Option<(char, Rc<RefCell<Vec<char>>>)>,
    /// A setting of its own, for the command kind 3.
    setting: &'static str,
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
        if let Some((name, log)) = &self.order {
            log.borrow_mut().push(*name);
        }
        if self.submits {
            let mut bytes = [0; 64];
            let command = match self.command.get() {
                0 => WireCommand::Redraw { mode: 0 },
                // Not on the list of a run's commands (closing its screen).
                2 => WireCommand::CloseScreen,
                3 => WireCommand::SetSetting {
                    key: self.setting,
                    value: crate::abi7::settings_wire::SettingValueRef::Bool(true),
                },
                // A notify with the announce bit, and a plain one.
                4 => WireCommand::Notify {
                    text: "from the run",
                    flags: 1,
                },
                5 => WireCommand::Notify {
                    text: "plain",
                    flags: 0,
                },
                _ => WireCommand::Open {
                    target: "a.screen",
                    mode: 0,
                    slot: "",
                    handover: 0,
                },
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

/// A plugin that binds the notice slot to a plain overlay, so that a notice
/// can show.
struct NoticeSlot;

struct Overlay;

impl paperpaw_sdk::Screen for Overlay {
    fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

impl paperpaw_sdk::Plugin for NoticeSlot {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("nt"),
            "Notice",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("nt.notice"),
            "notice",
            alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Overlay)),
        ))?;
        r.bind_slot(
            paperpaw_sdk::builtin::slots::NOTICE,
            Id::new_static("nt.notice"),
            paperpaw_sdk::Priority::DEFAULT,
        )
    }
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
    world_with(submits, None)
}

fn world_with(submits: bool, order: Option<Rc<RefCell<Vec<char>>>>) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(NoticeSlot)
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
        order: order.clone().map(|log| ('a', log)),
        setting: "a.toggle",
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
    fn subscribed(&self, _: u64, _: u32) -> bool {
        true
    }
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

// ---- the outbox at the start of a pump (no event is lost) ----

#[test]
fn a_batch_left_waiting_runs_before_a_pump_calls_anything() {
    let mut w = world(true);
    // A callback outside the pumps left its batch.
    assert_eq!(w.call(), Ok(OK));
    assert!(w.kernel.shared.native_instances.outbox.full());
    run_work(&mut w);
    // The work callback was called (its entry was not taken for a refused
    // call), and the waiting batch ran first.
    assert_eq!(w.calls.get(), 2);
    assert!(w.kernel.output.redraw);
    // The service pump does the same, even with no service wired.
    let mut w = world(true);
    assert_eq!(w.call(), Ok(OK));
    w.kernel.native_service_pump();
    assert!(!w.kernel.shared.native_instances.outbox.full());
    assert!(w.kernel.output.redraw);
}

// ---- the service pump through the real wrapper ----

use crate::native_host::service_events::ServiceEvents;
use crate::native_host::service_pump::pump;

/// Events queued per owner; each is one byte; `run` the work run they belong to.
#[derive(Default)]
struct Svc {
    queue: RefCell<Vec<u64>>,
    run: Cell<Option<u64>>,
}
impl ServiceEvents for Svc {
    fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<(usize, Option<u64>)>, i32> {
        let mut queue = self.queue.borrow_mut();
        let Some(at) = queue.iter().position(|o| *o == owner) else {
            return Ok(None);
        };
        queue.remove(at);
        out[0] = 1;
        Ok(Some((1, self.run.get())))
    }
    fn event_delivered(&self, _: u64) -> Result<(), i32> {
        Ok(())
    }
    fn screen_closed(&self, _: u64, _: u64) -> Result<(), i32> {
        Ok(())
    }
    fn work_ended(&self, _: u64, _: u64) -> Result<(), i32> {
        Ok(())
    }
    fn tick(&self) -> Result<(), i32> {
        Ok(())
    }
    fn next_deadline(&self) -> Option<u64> {
        None
    }
}

/// One pass of the real pump with the real wrapper, then the kernel runs the
/// batch (as `native_service_pump` does).
fn pass(kernel: &mut crate::Kernel, svc: &Svc) -> bool {
    let more = {
        let env = kernel.shared.env(kernel.now);
        let k = &*kernel;
        pump(
            &k.shared.native_instances,
            &[Some(svc), None, None],
            &env,
            &|owner, lease, bytes| {
                let _ = k.native_event_call(owner, lease, bytes);
            },
        )
    };
    kernel.native_outbox_run();
    more
}

/// A second plugin `b`, with its own screen on the stack.
fn add_b(w: &mut World, order: &Rc<RefCell<Vec<char>>>) -> u64 {
    w.kernel.shared.stack.push(StackEntry::new(
        Id::new_static("b.screen"),
        None,
        false,
        ScreenKey::new(8),
    ));
    let lease = NativeLease::try_new(Probe {
        answer: Rc::new(Cell::new(Ok(OK))),
        calls: Rc::new(Cell::new(0)),
        submits: true,
        command: Rc::new(Cell::new(3)),
        last: Rc::new(Cell::new(OK)),
        order: Some(('b', Rc::clone(order))),
        setting: "b.toggle",
    })
    .unwrap();
    w.kernel
        .shared
        .native_instances
        .test_insert(Id::try_parse("b").unwrap(), Some(lease), true)
        .unwrap()
}

#[test]
fn a_plugin_whose_events_all_stage_does_not_keep_the_others_waiting() {
    let order = Rc::new(RefCell::new(Vec::new()));
    let mut w = world_with(true, Some(Rc::clone(&order)));
    let b = add_b(&mut w, &order);
    // Each stages a setting of its own: no screen needs to be on top.
    for key in ["a.toggle", "b.toggle"] {
        w.kernel
            .shared
            .settings
            .define(paperpaw_sdk::SettingSpec::switch(
                key,
                "toggle",
                false,
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
    }
    w.command.set(3);
    let svc = Svc::default();
    svc.queue
        .borrow_mut()
        .extend([w.owner, w.owner, w.owner, b, b, b]);
    let mut passes = 0;
    while !svc.queue.borrow().is_empty() && passes < 12 {
        pass(&mut w.kernel, &svc);
        passes += 1;
    }
    // Every event was delivered, one a pass (its batch ran before the next
    // callback), and the pass went on at the next plugin: they alternate.
    assert!(svc.queue.borrow().is_empty());
    let log: alloc::string::String = order.borrow().iter().collect();
    assert_eq!(log, "ababab");
}

#[test]
fn a_runs_network_events_are_quiet_too() {
    use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE};
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
    w.command.set(1);
    let svc = Svc::default();
    // An event of a run's request: the callback is quiet.
    svc.queue.borrow_mut().push(w.owner);
    svc.run.set(Some(1));
    pass(&mut w.kernel, &svc);
    assert_eq!(w.last.get(), ERR_STATE);
    assert!(!w.kernel.shared.native_instances.outbox.full());
    // The same event outside a run is judged as any callback's.
    svc.queue.borrow_mut().push(w.owner);
    svc.run.set(None);
    pass(&mut w.kernel, &svc);
    assert_eq!(w.last.get(), ERR_NOT_FOUND);
}

#[test]
fn a_run_may_announce_and_the_notice_shows_after_its_callback_but_may_not_notify_plainly() {
    use paperpaw_native::host_glue::ERR_STATE;
    // A plain notify from a run is refused at submit: nothing shows.
    let mut w = world(true);
    w.kernel.end_notice();
    w.command.set(5);
    run_work(&mut w);
    assert_eq!(w.calls.get(), 1);
    assert_eq!(w.last.get(), ERR_STATE);
    assert!(w.kernel.notice.is_none());
    assert!(!w.kernel.shared.native_instances.outbox.full());
    // With the announce bit it is staged (`OK`), and the outbox's run, which
    // the pump does between callbacks, shows it: the plugin shows no screen,
    // and an announce does not need one.
    let mut w = world(true);
    w.kernel.shared.stack.clear();
    w.command.set(4);
    run_work(&mut w);
    assert_eq!(w.last.get(), OK);
    assert!(w.kernel.notice.is_some());
}

#[test]
fn a_run_may_only_stage_what_is_listed() {
    use paperpaw_native::host_glue::ERR_STATE;
    // A command not named for runs (here `close_screen`) is refused there by
    // default, and staged anywhere else.
    let mut w = world(true);
    w.command.set(2);
    run_work(&mut w);
    assert_eq!(w.last.get(), ERR_STATE);
    let w = world(true);
    w.command.set(2);
    w.call().unwrap();
    assert_eq!(w.last.get(), OK);
}

#[test]
fn the_service_pump_does_not_run_a_batch_inside_a_callback() {
    let mut w = world(true);
    assert_eq!(w.call(), Ok(OK));
    // A callback is on the stack: what it left waits until it returns.
    w.kernel.shared.native_instances.test_enter_callback();
    w.kernel.native_service_pump();
    assert!(w.kernel.shared.native_instances.outbox.full());
    assert!(!w.kernel.output.redraw);
}
