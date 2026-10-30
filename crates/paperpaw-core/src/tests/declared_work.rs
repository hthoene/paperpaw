//! Declared work (ADR 0025): moments, deadlines, work that waits for a
//! cheap moment, jobs in steps, periods, cancelling.

use super::*;
use core::cell::Cell;
use core::time::Duration;
use paperpaw_sdk::Params;
use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::wifi::WifiState;
use paperpaw_sdk::work::{Moments, Progress, Step, Work, WorkSpec, WorkState};

use crate::work::IDLE_MS;
use crate::{Effects, WifiEvent};
use paperpaw_platform::SystemStatus;

/// Logs every event of its own, with why it came; its job counts three
/// steps, then is done.
struct WorkPlugin {
    log: Log,
}

const RAN: [&str; 4] = ["wk.closed", "wk.push", "wk.weather", "wk.repeat"];
const JOB: Id = Id::new_static("wk.job");

struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for WorkPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("wk"), "Work", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("wk.home"),
            "home",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("wk.home"), Priority(i16::MAX))?;
        for event in RAN {
            let log = self.log.clone();
            r.subscribe(
                Id::new_static(event),
                Box::new(move |_, params, _| {
                    let why = params.get(paperpaw_sdk::work::params::WHY).unwrap_or("-");
                    log.borrow_mut().push(alloc::format!("{event} {why}"));
                }),
            )?;
        }
        let steps = Rc::new(Cell::new(0u32));
        let log = self.log.clone();
        r.add_work(WorkSpec::new(JOB, "job", move |_, _, _| {
            steps.set(steps.get() + 1);
            log.borrow_mut()
                .push(alloc::format!("step {}", steps.get()));
            if steps.get() < 3 {
                Step::Progress(Progress::new(steps.get(), 3))
            } else {
                Step::Done
            }
        }))?;
        let log = self.log.clone();
        r.subscribe(
            events::WORK_CHANGED.id().clone(),
            Box::new(move |_, params, _| {
                let id = params.get(params::ID).unwrap_or("");
                log.borrow_mut().push(alloc::format!("changed {id}"));
            }),
        )?;
        // Says whose doing it is handling, run as an action.
        let log = self.log.clone();
        r.add_action(ActionSpec::new(Id::new_static("wk.who"), "who").handler(
            move |env, _, _| {
                log.borrow_mut()
                    .push(alloc::format!("reader {}", env.reader()));
            },
        ))?;
        r.subscribe(
            Id::new_static("wk.fire"),
            Box::new(|_, _, out| out.push(Command::run(Id::new_static("wk.who")))),
        )?;
        // Work that fetches: its request goes out as it runs.
        r.subscribe(
            Id::new_static("wk.fetch"),
            Box::new(|_, _, out| {
                out.push(Command::http(paperpaw_sdk::HttpRequest::get(
                    "https://example.org/",
                    Id::new_static("wk.answered"),
                )));
            }),
        )?;
        let log = self.log.clone();
        r.subscribe(
            Id::new_static("wk.answered"),
            Box::new(move |_, params, _| {
                let refused = params.get(paperpaw_sdk::work::params::REFUSED);
                log.borrow_mut()
                    .push(alloc::format!("answered {}", refused.unwrap_or("-")));
            }),
        )?;
        for (event, name) in [
            (events::NETWORK_WANTED.id(), "wanted"),
            (events::WORK_ALLOWED.id(), "allowed"),
            (events::WORK_REFUSED.id(), "refused"),
        ] {
            let log = self.log.clone();
            r.subscribe(
                event.clone(),
                Box::new(move |_, _, _| log.borrow_mut().push(String::from(name))),
            )?;
        }
        Ok(())
    }
}

fn kernel(log: &Log, network: bool) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(WorkPlugin { log: log.clone() })
        .network(network)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    kernel
}

fn declare(kernel: &mut Kernel, at: u64, work: Work) -> Effects {
    kernel.now = Instant(at);
    kernel.apply(Command::work(work));
    kernel.finish()
}

#[test]
fn work_runs_at_its_moment() {
    let log = log();
    let mut kernel = kernel(&log, false);
    let closed = Work::new(Id::new_static("wk.closed")).at_moments(Moments::BOOK_CLOSED);
    declare(&mut kernel, 10, closed);
    kernel.tick(Instant(20));
    assert!(log.borrow().is_empty(), "no book closed yet");
    kernel.send_event(Instant(30), events::BOOK_CLOSED.id().clone(), Params::new());
    assert_eq!(*log.borrow(), ["wk.closed book-closed"]);
    // Done: not again.
    kernel.send_event(Instant(40), events::BOOK_CLOSED.id().clone(), Params::new());
    assert_eq!(log.borrow().len(), 1);
}

#[test]
fn a_restart_is_no_moment_of_sleep() {
    let log = log();
    let mut kernel = kernel(&log, false);
    let push = Work::new(Id::new_static("wk.push")).at_moments(Moments::SLEEP);
    declare(&mut kernel, 10, push);
    kernel.stop(Instant(20));
    assert!(log.borrow().is_empty(), "no sleep yet");
    kernel.suspend(Instant(30));
    assert_eq!(*log.borrow(), ["wk.push sleep"]);
}

#[test]
fn a_deadline_runs_work_that_no_moment_did() {
    let log = log();
    let mut kernel = kernel(&log, false);
    let push = Work::network(Id::new_static("wk.push"))
        .within(Duration::from_secs(60))
        .at_moments(Moments::SLEEP);
    let effects = declare(&mut kernel, 1000, push);
    assert_eq!(effects.wake_at(), Some(Instant(61_000)));
    kernel.tick(Instant(60_999));
    assert!(log.borrow().is_empty());
    kernel.tick(Instant(61_000));
    assert_eq!(*log.borrow(), ["wk.push deadline"]);
}

#[test]
fn optional_network_work_waits_until_the_radio_is_on() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(WorkPlugin { log: log.clone() })
        .network(true)
        .wifi(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    let weather = Work::network(Id::new_static("wk.weather")).optional();
    declare(&mut kernel, 10, weather);
    // Waking, a book closed: moments, but the radio is off.
    kernel.send_event(Instant(20), events::RESUME.id().clone(), Params::new());
    kernel.send_event(Instant(30), events::BOOK_CLOSED.id().clone(), Params::new());
    assert!(log.borrow().is_empty());
    assert!(matches!(
        kernel.shared.work_states.get(&Id::new_static("wk.weather")),
        Some(WorkState::Waiting { due: None })
    ));
    // The radio came on for something else.
    kernel.wifi_event(
        Instant(40),
        WifiEvent::State(WifiState::Online {
            ssid: "Home".into(),
            address: "10.0.0.2".into(),
        }),
    );
    assert_eq!(*log.borrow(), ["wk.weather online"]);
}

#[test]
fn where_the_system_keeps_the_network_waking_is_cheap() {
    let log = log();
    let mut kernel = kernel(&log, true);
    let weather = Work::network(Id::new_static("wk.weather")).optional();
    declare(&mut kernel, 10, weather);
    kernel.send_event(Instant(20), events::RESUME.id().clone(), Params::new());
    assert_eq!(*log.borrow(), ["wk.weather wake,online"]);
}

#[test]
fn work_with_a_period_comes_again() {
    let log = log();
    let mut kernel = kernel(&log, false);
    let repeat = Work::new(Id::new_static("wk.repeat"))
        .within(Duration::from_secs(1))
        .every(Duration::from_secs(5))
        .at_moments(Moments::NONE);
    declare(&mut kernel, 0, repeat);
    kernel.tick(Instant(1000));
    let effects = kernel.tick(Instant(1001));
    assert_eq!(effects.wake_at(), Some(Instant(7000)));
    kernel.tick(Instant(7000));
    assert_eq!(*log.borrow(), ["wk.repeat deadline", "wk.repeat deadline"]);
}

#[test]
fn a_job_steps_in_ticks_until_done() {
    let log = log();
    let mut kernel = kernel(&log, false);
    let effects = declare(&mut kernel, 5, Work::new(JOB).within(Duration::ZERO));
    // Marked to run, but no step while handling a call other than a tick.
    assert!(!log.borrow().iter().any(|l| l.starts_with("step")));
    assert_eq!(effects.wake_at(), Some(Instant(5)));
    kernel.tick(Instant(6));
    assert!(matches!(
        kernel.shared.work_states.get(&JOB),
        Some(WorkState::Running {
            progress: Some(p)
        }) if *p == Progress::new(1, 3)
    ));
    kernel.tick(Instant(7));
    kernel.tick(Instant(2000));
    let steps: Vec<_> = log
        .borrow()
        .iter()
        .filter(|l| l.starts_with("step"))
        .cloned()
        .collect();
    assert_eq!(steps, ["step 1", "step 2", "step 3"]);
    assert!(log.borrow().iter().any(|l| l == "changed wk.job"));
    assert_eq!(kernel.shared.work_states.get(&JOB), None, "done");
}

#[test]
fn work_taken_back_does_not_run() {
    let log = log();
    let mut kernel = kernel(&log, false);
    let push = Work::new(Id::new_static("wk.push")).within(Duration::from_secs(1));
    declare(&mut kernel, 0, push);
    kernel.apply(Command::CancelWork(Id::new_static("wk.push"), None));
    kernel.tick(Instant(5000));
    assert!(log.borrow().is_empty());
}

#[test]
fn offline_holds_network_work_back() {
    let log = log();
    let mut kernel = kernel(&log, true);
    kernel.shared.power = paperpaw_sdk::power::Power::new(
        paperpaw_sdk::power::PowerLevel::Offline,
        paperpaw_sdk::power::OFFLINE,
        false,
        4,
    );
    let push = Work::network(Id::new_static("wk.push")).within(Duration::from_secs(1));
    declare(&mut kernel, 0, push);
    kernel.tick(Instant(5000));
    kernel.send_event(Instant(5001), events::RESUME.id().clone(), Params::new());
    assert!(log.borrow().is_empty());
    assert!(matches!(
        kernel.shared.work_states.get(&Id::new_static("wk.push")),
        Some(WorkState::Held(_))
    ));
}

/// Says something when its events come, or its action runs.
struct Sayer;

impl Plugin for Sayer {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("sy"), "Sayer", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // Notices need a screen to show them.
        r.add_screen(ScreenSpec::new(
            Id::new_static("sy.notice"),
            "notice",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.bind_slot(
            slots::NOTICE,
            Id::new_static("sy.notice"),
            Priority(i16::MAX),
        )?;
        r.subscribe(
            Id::new_static("sy.say"),
            Box::new(|_, _, out| out.push(Command::Notify(String::from("synced")))),
        )?;
        r.subscribe(
            Id::new_static("sy.foreign"),
            Box::new(|_, _, out| {
                let work = Work::new(Id::new_static("wk.push")).at_moments(Moments::NONE);
                out.push(Command::work(work));
            }),
        )?;
        r.subscribe(
            Id::new_static("sy.remind"),
            Box::new(|_, _, out| out.push(Command::Announce(String::from("unplug")))),
        )?;
        r.add_action(
            ActionSpec::new(Id::new_static("sy.act"), "act")
                .handler(|_, _, out| out.push(Command::Notify(String::from("done")))),
        )
    }
}

fn sayer() -> Kernel {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(WorkPlugin { log })
        .plugin(Sayer)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.notice = None;
    kernel
}

#[test]
fn background_work_says_nothing_over_reading_but_keeps_a_note() {
    let mut kernel = sayer();
    // A timer's event: background work, the home screen (another
    // plugin's) on top.
    declare(
        &mut kernel,
        5,
        Work::at(Id::new_static("sy.say"), Instant(10)),
    );
    kernel.tick(Instant(10));
    assert!(kernel.notice.is_none(), "no notice over reading");
    let note = kernel
        .shared
        .work_states
        .note(&Id::new_static("sy"))
        .unwrap();
    assert_eq!((note.at, note.text.as_str()), (Instant(10), "synced"));
}

#[test]
fn what_the_reader_runs_may_say_so_and_an_announcement_always_may() {
    let mut kernel = sayer();
    kernel.run_action(Instant(10), Id::new_static("sy.act"));
    assert!(kernel.notice.is_some(), "the reader's own action");
    kernel.notice = None;
    declare(
        &mut kernel,
        15,
        Work::at(Id::new_static("sy.remind"), Instant(20)),
    );
    kernel.tick(Instant(20));
    assert!(kernel.notice.is_some(), "a message the reader set up");
    assert!(
        kernel
            .shared
            .work_states
            .note(&Id::new_static("sy"))
            .is_none()
    );
}

fn saving() -> paperpaw_sdk::power::Power {
    paperpaw_sdk::power::Power::new(
        paperpaw_sdk::power::PowerLevel::Saving,
        paperpaw_sdk::power::SAVING,
        false,
        4,
    )
}

#[test]
fn a_plugin_whose_network_is_off_gets_none() {
    let log = log();
    let mut kernel = kernel(&log, true);
    kernel
        .set_setting(Instant(1), "core.network.wk", "off")
        .unwrap();
    let push = Work::network(Id::new_static("wk.push")).within(Duration::from_secs(1));
    declare(&mut kernel, 2, push);
    kernel.tick(Instant(5000));
    assert_eq!(*log.borrow(), ["refused"], "its network work never runs");
    assert_eq!(
        kernel.shared.work_states.get(&Id::new_static("wk.push")),
        None
    );
    kernel.apply(Command::http(paperpaw_sdk::HttpRequest::get(
        "https://example.org/",
        Id::new_static("wk.closed"),
    )));
    kernel.finish();
    assert!(kernel.take_io().is_empty(), "nor does a request go out");
    assert_eq!(log.borrow()[1], "wk.closed -", "it is answered at once");
}

#[test]
fn an_exempt_plugin_keeps_normal_limits_in_saving() {
    let log = log();
    let mut kernel = kernel(&log, true);
    kernel.shared.power = saving();
    let push = || {
        Work::network(Id::new_static("wk.push"))
            .within(Duration::from_secs(60))
            .at_moments(Moments::ONLINE)
    };
    let effects = declare(&mut kernel, 0, push());
    assert_eq!(
        effects.wake_at(),
        Some(Instant(240_000)),
        "four times as long"
    );
    kernel
        .set_setting(Instant(1), "core.saving.wk", "core.saving.normal")
        .unwrap();
    // Declared again: counted from its first declaring, as in Normal now.
    let effects = declare(&mut kernel, 2, push());
    assert_eq!(effects.wake_at(), Some(Instant(60_000)), "as in Normal");
    kernel
        .set_setting(Instant(3), "core.saving.wk", "core.saving.manual")
        .unwrap();
    declare(&mut kernel, 4, push());
    kernel.tick(Instant(1_000_000));
    assert!(log.borrow().is_empty(), "only when asked");
    assert!(matches!(
        kernel.shared.work_states.get(&Id::new_static("wk.push")),
        Some(WorkState::Held(_))
    ));
}

fn wifi_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(WorkPlugin { log: log.clone() })
        .network(true)
        .wifi(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    kernel
}

#[test]
fn work_due_at_a_tick_wants_the_network_in_that_call() {
    let log = log();
    let mut kernel = wifi_kernel(&log);
    declare(
        &mut kernel,
        0,
        Work::at(Id::new_static("wk.fetch"), Instant(7000)),
    );
    let effects = kernel.tick(Instant(7000));
    assert_eq!(*log.borrow(), ["wanted"], "the radio is asked for now");
    // And the wait for it ends in time, not at the next button press.
    assert_eq!(effects.wake_at(), Some(Instant(37_000)));
}

#[test]
fn work_that_cannot_run_wakes_nothing() {
    let log = log();
    let mut kernel = wifi_kernel(&log);
    // Nice to have, the radio off: past its deadline it waits for the
    // radio, without the device waking for it again and again.
    let weather = Work::network(Id::new_static("wk.weather"))
        .optional()
        .within(Duration::from_secs(1))
        .at_moments(Moments::ONLINE);
    declare(&mut kernel, 0, weather);
    let effects = kernel.tick(Instant(5000));
    assert_eq!(effects.wake_at(), None);
    let charging = Work::new(Id::new_static("wk.push"))
        .charging()
        .within(Duration::from_secs(1))
        .at_moments(Moments::NONE);
    declare(&mut kernel, 5000, charging);
    let effects = kernel.tick(Instant(9000));
    assert_eq!(effects.wake_at(), None);
    assert!(log.borrow().is_empty());
}

#[test]
fn a_running_job_declared_again_goes_on() {
    let log = log();
    let mut kernel = kernel(&log, false);
    declare(&mut kernel, 0, Work::new(JOB).within(Duration::ZERO));
    kernel.tick(Instant(1));
    declare(&mut kernel, 2, Work::new(JOB).within(Duration::ZERO));
    kernel.tick(Instant(3));
    let steps: Vec<_> = log
        .borrow()
        .iter()
        .filter(|l| l.starts_with("step"))
        .cloned()
        .collect();
    assert_eq!(steps, ["step 1", "step 2"]);
}

#[test]
fn a_repeating_timer_comes_again() {
    let log = log();
    let mut kernel = kernel(&log, false);
    let timer = Work::at(Id::new_static("wk.repeat"), Instant(100)).every(Duration::from_secs(1));
    declare(&mut kernel, 0, timer);
    kernel.tick(Instant(100));
    let effects = kernel.tick(Instant(101));
    assert_eq!(effects.wake_at(), Some(Instant(1100)));
    kernel.tick(Instant(1100));
    assert_eq!(*log.borrow(), ["wk.repeat -", "wk.repeat -"]);
}

#[test]
fn work_brought_due_by_the_readers_call_is_still_background() {
    let mut kernel = sayer();
    let say = Work::new(Id::new_static("sy.say")).at_moments(Moments::BOOK_CLOSED);
    declare(&mut kernel, 1, say);
    // The reader closes the book (a call of theirs): the work it brought
    // due says nothing over reading all the same.
    kernel.send_event(Instant(5), events::BOOK_CLOSED.id().clone(), Params::new());
    assert!(kernel.notice.is_none());
    assert!(
        kernel
            .shared
            .work_states
            .note(&Id::new_static("sy"))
            .is_some()
    );
}

#[test]
fn a_held_button_is_the_readers_input() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(WorkPlugin { log })
        .plugin(Sayer)
        .user_config("[keymap.global]\nlong.menu = sy.act\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.notice = None;
    let button = |state| InputEvent::Button {
        name: "menu",
        state,
    };
    kernel.handle(Instant(100), button(ButtonState::Pressed));
    kernel.tick(Instant(3000));
    assert!(kernel.notice.is_some(), "the long press's answer shows");
    kernel.handle(Instant(3100), button(ButtonState::Released));
}

#[test]
fn moments_of_idling_and_charging() {
    let log = log();
    let mut kernel = kernel(&log, false);
    let idle = Work::new(Id::new_static("wk.closed")).at_moments(Moments::IDLE);
    let effects = declare(&mut kernel, 0, idle);
    assert_eq!(effects.wake_at(), Some(Instant(IDLE_MS)));
    kernel.tick(Instant(IDLE_MS));
    assert_eq!(*log.borrow(), ["wk.closed idle"]);
    let charging = Work::new(Id::new_static("wk.push")).at_moments(Moments::CHARGING);
    declare(&mut kernel, IDLE_MS + 1, charging);
    kernel.set_status(
        Instant(IDLE_MS + 2),
        SystemStatus::new(None, Some(paperpaw_platform::Battery::new(50, true, None))),
    );
    assert_eq!(log.borrow()[1], "wk.push charging");
}

#[test]
fn a_full_queue_refuses_and_then_allows_again() {
    let log = log();
    let mut kernel = kernel(&log, false);
    for n in 0..33 {
        let work = Work::new(Id::new_static("wk.push"))
            .key(alloc::format!("{n}"))
            .at_moments(Moments::NONE);
        declare(&mut kernel, 1, work);
    }
    assert_eq!(*log.borrow(), ["refused"]);
    kernel.apply(Command::CancelWork(
        Id::new_static("wk.push"),
        Some(String::from("0")),
    ));
    kernel.finish();
    assert_eq!(*log.borrow(), ["refused", "allowed"]);
}

#[test]
fn work_under_another_plugins_name_is_dropped() {
    let mut kernel = sayer();
    kernel.take_warnings();
    // Sayer's handler of `sy.foreign` declares work as `wk`.
    kernel.send_event(Instant(5), Id::new_static("sy.foreign"), Params::new());
    assert_eq!(
        kernel.shared.work_states.get(&Id::new_static("wk.push")),
        None
    );
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains("another plugin's name"))
    );
}

#[test]
fn a_plugin_that_does_not_declare_the_network_gets_none() {
    let mut kernel = sayer();
    kernel.take_warnings();
    kernel.apply(Command::http(paperpaw_sdk::HttpRequest::get(
        "https://example.org/",
        Id::new_static("sy.answer"),
    )));
    kernel.finish();
    assert!(!kernel.io.http.busy());
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains("without declaring it"))
    );
}

#[test]
fn requests_are_taken_back_and_answered_so() {
    let log = log();
    let mut kernel = kernel(&log, true);
    kernel.apply(Command::http(paperpaw_sdk::HttpRequest::get(
        "https://example.org/",
        Id::new_static("wk.answered"),
    )));
    kernel.finish();
    assert!(kernel.io.http.busy());
    kernel.apply(Command::CancelHttp(Id::new_static("wk.answered")));
    kernel.finish();
    assert_eq!(*log.borrow(), ["answered cancelled"]);
    assert!(!kernel.io.http.busy());
}

#[test]
fn offline_lets_out_only_what_the_reader_went_online_for() {
    let log = log();
    let mut kernel = kernel(&log, true);
    kernel.shared.power = paperpaw_sdk::power::Power::new(
        paperpaw_sdk::power::PowerLevel::Offline,
        paperpaw_sdk::power::OFFLINE,
        false,
        4,
    );
    let request = || {
        Command::http(paperpaw_sdk::HttpRequest::get(
            "https://example.org/",
            Id::new_static("wk.answered"),
        ))
    };
    // Background: refused at once.
    kernel.apply(request());
    kernel.finish();
    assert_eq!(*log.borrow(), ["answered offline"]);
    // The reader, without going online: refused too.
    kernel.shared.reader = true;
    kernel.apply(request());
    kernel.finish();
    assert_eq!(log.borrow()[1], "answered offline");
    // Gone online on the screen in front: it goes out.
    kernel.shared.reader = true;
    kernel.apply(Command::GoOnline);
    kernel.apply(request());
    kernel.finish();
    assert!(kernel.io.http.busy(), "{:?}", log.borrow());
}

#[test]
fn plugins_know_whether_the_reader_is_waiting() {
    let log = log();
    let mut kernel = kernel(&log, false);
    kernel.run_action(Instant(5), Id::new_static("wk.who"));
    // The same action run by background work (a timer, as a rule's).
    declare(
        &mut kernel,
        6,
        Work::at(Id::new_static("wk.fire"), Instant(10)),
    );
    kernel.tick(Instant(10));
    assert_eq!(*log.borrow(), ["reader true", "reader false"]);
}
