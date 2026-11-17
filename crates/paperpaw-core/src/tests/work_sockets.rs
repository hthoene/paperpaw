//! The sockets of background work belong to its run, not to the screens
//! open: they stay while screens open and close, and close when the run
//! ends (a job done or failed, the work taken back, run again, or its
//! plugin gone). A socket a plugin opens for the reader goes on as
//! before, tied to the screens.

extern crate std;

use super::*;
use crate::Items;
use crate::WorkSpec;
use core::cell::Cell;
use paperpaw_sdk::Params;
use paperpaw_sdk::work::{Step, Work};

use crate::io::SocketCall;

const JOB: Id = Id::new_static("ws.job");
const FETCH: Id = Id::new_static("ws.fetch");
const GO: Id = Id::new_static("ws.go");
/// A job that connects only once woken (its second step).
const LATE: Id = Id::new_static("ws.late");

struct Blank;

impl crate::testing::Fake for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

std::thread_local! {
    /// The plugin's jobs in the test running: whether the first fails, its
    /// steps and the late one's (the steps are plain functions and hold
    /// nothing).
    static STATE: core::cell::RefCell<(Rc<Cell<bool>>, u32, u32)> =
        core::cell::RefCell::new((Rc::new(Cell::new(false)), 0, 0));
}

/// Fetches over a socket as an event's work and as a job (its first step
/// connects and waits for `ws.go`, the next is done, or fails as `fail`
/// says); opens one for the reader on `ws.open`.
struct SocketWork {
    fail: Rc<Cell<bool>>,
}

fn connect(socket: &'static str) -> Command {
    let events = Id::new_static("ws.heard");
    Command::connect(Id::new_static(socket), "mail.example.org", 993, events)
}

fn socket_work(kept: SocketWork) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        for id in ["ws.home", "ws.page"] {
            r.add_fake(crate::testing::FakeSpec::new(
                Id::new_static(id),
                id,
                Box::new(|_, _| Box::new(Blank)),
            ))?;
        }
        r.bind_slot(slots::HOME, Id::new_static("ws.home"), Priority(i16::MAX))?;
        r.subscribe(FETCH, |k, _| {
            k.with_env(|_, out| out.push(connect("ws.mail")));
        })?;
        r.subscribe(Id::new_static("ws.open"), |k, _| {
            k.with_env(|_, out| out.push(connect("ws.own")));
        })?;
        STATE.with(|state| {
            *state.borrow_mut() = (Rc::clone(&kept.fail), 0, 0);
        });
        r.add_work(WorkSpec::new(JOB, "job", |k, _| {
            let (fail, steps) = STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.1 += 1;
                (Rc::clone(&state.0), state.1)
            });
            if steps == 1 {
                k.with_env(|_, out| out.push(connect("ws.box")));
                return Step::WaitFor(GO);
            }
            STATE.with(|state| state.borrow_mut().1 = 0);
            if fail.get() {
                Step::Failed(String::from("no answer"))
            } else {
                Step::Done
            }
        }))?;
        r.add_work(WorkSpec::new(LATE, "late", |k, _| {
            let late = STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.2 += 1;
                state.2
            });
            match late {
                1 => Step::WaitFor(GO),
                2 => {
                    k.with_env(|_, out| out.push(connect("ws.late")));
                    Step::WaitFor(GO)
                }
                _ => Step::Done,
            }
        }))?;
        Ok(())
    }
}

/// What [`socket_work`] registers under.
fn socket_work_manifest() -> Manifest {
    Manifest::new(Id::new_static("ws"), "Socket work", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NETWORK)
}

fn kernel(fail: bool) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            socket_work_manifest(),
            socket_work(SocketWork {
                fail: Rc::new(Cell::new(fail)),
            }),
        )
        .network(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.take_io();
    kernel
}

/// What the kernel asked of the shell's sockets: `connect n`, `close n`.
fn calls(kernel: &mut Kernel) -> Vec<String> {
    kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Socket(SocketCall::Connect { socket, .. }) => {
                Some(alloc::format!("connect {socket}"))
            }
            Io::Socket(SocketCall::Close { socket }) => Some(alloc::format!("close {socket}")),
            _ => None,
        })
        .collect()
}

/// Runs the work `work` at `at`.
fn run(kernel: &mut Kernel, at: u64, work: Work) {
    kernel.shared.now = Instant(at - 1);
    kernel.apply(Command::work(work));
    kernel.finish();
    kernel.tick(Instant(at));
}

/// Opens a screen over the home screen and closes it again.
fn open_and_close_a_screen(kernel: &mut Kernel, at: u64) {
    kernel.apply(Command::OpenScreen(
        Id::new_static("ws.page"),
        Params::new(),
    ));
    kernel.finish();
    assert_eq!(kernel.stack.len(), 2);
    kernel.run_action(Instant(at), actions::BACK);
    assert_eq!(kernel.stack.len(), 1);
}

#[test]
fn a_jobs_socket_stays_while_screens_close_and_closes_when_it_is_done() {
    let mut kernel = kernel(false);
    // Opened while a screen was over the home screen.
    kernel.apply(Command::OpenScreen(
        Id::new_static("ws.page"),
        Params::new(),
    ));
    kernel.finish();
    run(&mut kernel, 100, Work::at(JOB, Instant(100)));
    assert_eq!(calls(&mut kernel), ["connect 1"]);
    kernel.run_action(Instant(110), actions::BACK);
    open_and_close_a_screen(&mut kernel, 120);
    assert!(calls(&mut kernel).is_empty(), "the job's socket stays");
    assert!(kernel.sockets_open());
    // Told to go on: done, its socket closed.
    kernel.send_event(Instant(130), GO, Params::new());
    kernel.tick(Instant(140));
    assert_eq!(calls(&mut kernel), ["close 1"]);
    assert!(!kernel.sockets_open());
}

#[test]
fn a_failed_job_closes_its_socket() {
    let mut kernel = kernel(true);
    run(&mut kernel, 100, Work::at(JOB, Instant(100)));
    assert_eq!(calls(&mut kernel), ["connect 1"]);
    kernel.send_event(Instant(130), GO, Params::new());
    kernel.tick(Instant(140));
    assert_eq!(calls(&mut kernel), ["close 1"]);
}

#[test]
fn work_taken_back_closes_its_socket() {
    let mut kernel = kernel(false);
    run(&mut kernel, 100, Work::at(JOB, Instant(100)));
    assert_eq!(calls(&mut kernel), ["connect 1"]);
    kernel.apply(Command::CancelWork(JOB, None));
    kernel.finish();
    assert_eq!(calls(&mut kernel), ["close 1"]);
}

#[test]
fn an_events_socket_closes_when_the_work_runs_again_and_none_is_left_over() {
    let mut kernel = kernel(false);
    for n in 0..10u64 {
        let at = 100 + n * 100;
        run(&mut kernel, at, Work::at(FETCH, Instant(at)));
        open_and_close_a_screen(&mut kernel, at + 50);
        let expected: Vec<String> = match n {
            0 => alloc::vec![String::from("connect 1")],
            // The run before ends: its socket closes.
            _ => alloc::vec![
                alloc::format!("close {n}"),
                alloc::format!("connect {}", n + 1)
            ],
        };
        let mut got = calls(&mut kernel);
        got.sort();
        assert_eq!(got, expected, "run {n}");
        // The same socket, connected anew, is the new run's: the end of
        // the one before leaves it open.
        assert!(kernel.sockets_open(), "run {n}");
        assert!(kernel.works.runs.count() <= 1, "run {n}");
    }
    // Taken back (it is not declared any more): its last socket closes.
    kernel.apply(Command::CancelWork(FETCH, None));
    kernel.finish();
    assert_eq!(calls(&mut kernel), ["close 10"]);
    assert_eq!(kernel.works.runs.count(), 0);
    assert!(!kernel.sockets_open());
}

#[test]
fn work_whose_plugin_goes_closes_its_socket() {
    let mut kernel = kernel(false);
    run(&mut kernel, 100, Work::at(JOB, Instant(100)));
    assert_eq!(calls(&mut kernel), ["connect 1"]);
    // As unloading the plugin does.
    kernel.works.runs.end_plugin("ws");
    kernel.close_ended_runs();
    assert_eq!(calls(&mut kernel), ["close 1"]);
}

#[test]
fn a_socket_for_the_reader_is_the_screens_and_no_work_closes_it() {
    let mut kernel = kernel(false);
    kernel.apply(Command::OpenScreen(
        Id::new_static("ws.page"),
        Params::new(),
    ));
    kernel.finish();
    kernel.send_event(Instant(50), Id::new_static("ws.open"), Params::new());
    assert_eq!(calls(&mut kernel), ["connect 1"]);
    // Work runs and ends beside it: it stays.
    run(&mut kernel, 100, Work::at(JOB, Instant(100)));
    kernel.send_event(Instant(130), GO, Params::new());
    kernel.tick(Instant(140));
    assert_eq!(calls(&mut kernel), ["connect 2", "close 2"]);
    // Its screen closed: it closes, as before.
    kernel.run_action(Instant(150), actions::BACK);
    assert_eq!(calls(&mut kernel), ["close 1"]);
}

#[test]
fn a_socket_a_job_connects_once_woken_is_the_runs() {
    let mut kernel = kernel(false);
    run(&mut kernel, 100, Work::at(LATE, Instant(100)));
    assert!(calls(&mut kernel).is_empty());
    kernel.apply(Command::OpenScreen(
        Id::new_static("ws.page"),
        Params::new(),
    ));
    kernel.finish();
    // Woken by the event it waits for: its step connects.
    kernel.send_event(Instant(130), GO, Params::new());
    kernel.tick(Instant(140));
    assert_eq!(calls(&mut kernel), ["connect 1"]);
    kernel.run_action(Instant(150), actions::BACK);
    assert!(calls(&mut kernel).is_empty(), "the job's socket stays");
    kernel.send_event(Instant(160), GO, Params::new());
    kernel.tick(Instant(170));
    assert_eq!(calls(&mut kernel), ["close 1"]);
}
