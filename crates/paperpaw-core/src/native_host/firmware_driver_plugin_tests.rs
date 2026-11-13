//! Plugin jobs in the firmware queue: the seam of `firmware_driver` with
//! `plugin_job`, against a job that takes a scripted number of steps.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::firmware_install::{FirmwareInstall, KIND_CHECK};
use crate::native_host::plugin_job::{
    Answer, Finished, KIND_INSTALL, KIND_REMOVAL, PluginFacts, PluginQuestion, Relation,
    USES_SHOWN, Uses, reason as job_reason,
};
use crate::native_host::plugin_jobs::PluginJobs;
use core::cell::RefCell;
use paperpaw_native::{
    header5::{Host as Loader, X4_PROFILE},
    host_glue::{ERR_FULL, ERR_UNSUPPORTED},
    metadata::Host as Offered,
};
use paperpaw_platform::{
    NoStorage,
    firmware::{Text, WRITE_BLOCK},
    firmware_fake::{FakeFirmware, paperpaw_facts},
};

struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        0
    }
    fn index_of(&self, _: &str) -> Option<usize> {
        None
    }
}

fn host() -> PluginHost {
    PluginHost {
        loader: Some(Loader::x4(1)),
        offered: Offered {
            abi_major: 7,
            minor: 1,
            profile: X4_PROFILE,
            services: &[],
            features: &[],
        },
        signers: &[],
    }
}

fn facts(id: &str) -> PluginFacts {
    PluginFacts {
        id: Text::new(id),
        version: Text::new("1.2.0"),
        bytes: 4096,
        signer: Text::new("PaperPaw release"),
        key: Some([5; 32]),
        sha256: Some([6; 32]),
        flags: 0b010,
    }
}

/// What a scripted job did, seen from the test.
#[derive(Default)]
pub(crate) struct Seen {
    pub(crate) steps: Cell<u32>,
    aborted: Cell<Option<u8>>,
    pub(crate) dropped: Cell<bool>,
}

pub(crate) struct Script {
    owner: u64,
    request: u32,
    kind: u32,
    steps: u32,
    done: Finished,
    seen: Rc<Seen>,
    must_finish: bool,
    keep: bool,
    /// Asks the user once and ends with the answer's reason.
    asks: bool,
    answered: Option<Answer>,
}

impl PluginJob for Script {
    fn owner(&self) -> u64 {
        self.owner
    }
    fn request(&self) -> u32 {
        self.request
    }
    fn kind(&self) -> u32 {
        self.kind
    }
    fn step(&mut self, _: &JobEnv<'_>) -> JobStep {
        self.seen.steps.set(self.seen.steps.get() + 1);
        if self.asks {
            return match self.answered {
                None => JobStep::Ask(question(self.owner, self.request)),
                Some(Answer::Yes) => JobStep::Done(Finished::refused(job_reason::OK)),
                Some(Answer::No) => JobStep::Done(Finished::refused(2)),
                Some(_) => JobStep::Done(Finished::refused(3)),
            };
        }
        if self.seen.steps.get() >= self.steps {
            JobStep::Done(Finished {
                needs_draft: self.done.needs_draft,
                ..Finished::checked(self.done.reason, self.done.facts)
            })
        } else {
            JobStep::More
        }
    }
    fn answer(&mut self, answer: Answer) {
        self.answered = Some(answer);
    }
    fn abort(&mut self, reason: u8) {
        self.seen.aborted.set(Some(reason));
    }
    fn must_finish(&self) -> bool {
        self.must_finish
    }
    fn keep_without_sender(&self) -> bool {
        self.keep
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        self.seen.dropped.set(true);
    }
}

fn question(owner: u64, request: u32) -> PluginQuestion {
    PluginQuestion {
        owner,
        request,
        kind: KIND_INSTALL,
        id: Text::new("dials"),
        name: Text::new("Dials"),
        version: Text::new("1.0.0"),
        signer: Text::new(""),
        key: None,
        sideload: false,
        relation: Relation::Newer,
        uses: Uses {
            keys: [Text::new(""); USES_SHOWN],
            count: 0,
            more: false,
        },
    }
}

/// An install-like job that asks the user once (`question`) and ends with
/// reason 0 for a yes, 2 for a no and 3 for a dismissal.
pub(crate) fn asking_job(owner: u64, request: u32) -> PluginJobs {
    PluginJobs::Script(Script {
        owner,
        request,
        kind: KIND_INSTALL,
        steps: 1,
        done: Finished::refused(job_reason::OK),
        seen: Rc::default(),
        must_finish: false,
        keep: false,
        asks: true,
        answered: None,
    })
}

/// The plugin host the kernel tests give `Kernel::builder`.
pub(crate) fn kernel_host() -> (Loader, Offered<'static>) {
    let host = host();
    (Loader::x4(1), host.offered)
}

fn check_job(owner: u64, request: u32, steps: u32) -> (PluginJobs, Rc<Seen>) {
    let seen = Rc::new(Seen::default());
    let job = Script {
        owner,
        request,
        kind: KIND_CHECK,
        steps,
        done: Finished::checked(0, Some(facts("dials"))),
        seen: Rc::clone(&seen),
        must_finish: false,
        keep: false,
        asks: false,
        answered: None,
    };
    (PluginJobs::Script(job), seen)
}

fn job_of_kind(owner: u64, request: u32, kind: u32) -> PluginJobs {
    PluginJobs::Script(Script {
        owner,
        request,
        kind,
        steps: 1,
        done: Finished::refused(job_reason::OK),
        seen: Rc::default(),
        must_finish: false,
        keep: false,
        asks: false,
        answered: None,
    })
}

/// An install-like job of `steps` steps, with what it says of stopping.
pub(crate) fn install_job(
    owner: u64,
    request: u32,
    steps: u32,
    must_finish: bool,
    keep: bool,
) -> (PluginJobs, Rc<Seen>) {
    let seen = Rc::new(Seen::default());
    let job = Script {
        owner,
        request,
        kind: KIND_INSTALL,
        steps,
        done: Finished::refused(job_reason::OK),
        seen: Rc::clone(&seen),
        must_finish,
        keep,
        asks: false,
        answered: None,
    };
    (PluginJobs::Script(job), seen)
}

fn driver(with_host: bool) -> Driver {
    let fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    let mut d = Driver::new(
        Box::new(fake),
        crate::native_host::firmware_cell::test_shared(RefCell::new(FirmwareInstall::new())),
    );
    if with_host {
        d.set_plugin_host(host());
    }
    d
}

/// Pumps until a job ends.
fn run(d: &mut Driver) -> Event {
    for now in (100..20_000).step_by(100) {
        match d.pump(&NoStorage, &Names, now) {
            Event::Quiet | Event::Progress { .. } => {}
            other => return other,
        }
    }
    panic!("never ended");
}

#[test]
fn without_a_plugin_host_or_with_another_kind_nothing_is_accepted() {
    let mut d = driver(false);
    assert!(!d.plugins_accepted());
    let (job, seen) = check_job(1, 1, 1);
    assert_eq!(d.submit_plugin(job), Err(ERR_UNSUPPORTED));
    assert!(seen.dropped.get());
    assert_eq!(d.waiting(), 0);
    let mut d = driver(true);
    assert!(d.plugins_accepted());
    // A firmware kind is not a plugin job's.
    assert_eq!(
        d.submit_plugin(job_of_kind(1, 1, KIND_FIRMWARE)),
        Err(ERR_UNSUPPORTED)
    );
    assert_eq!(d.waiting(), 0);
}

#[test]
fn a_plugin_job_shares_the_limits_of_the_request_table() {
    let mut d = driver(true);
    d.submit_plugin(job_of_kind(1, 1, KIND_INSTALL)).unwrap();
    d.submit_plugin(job_of_kind(1, 2, KIND_REMOVAL)).unwrap();
    // Two for one plugin, four device-wide, a number that waits is invalid.
    assert_eq!(
        d.submit_plugin(job_of_kind(1, 3, KIND_INSTALL)),
        Err(ERR_FULL)
    );
    assert_eq!(d.submit_plugin(job_of_kind(2, 1, KIND_INSTALL)), Ok(()));
    assert_eq!(
        d.submit_plugin(job_of_kind(2, 1, KIND_INSTALL)),
        Err(ERR_INVALID)
    );
    d.submit_check(3, 1, "a.ppu").unwrap();
    assert_eq!(d.waiting(), 4);
    assert_eq!(d.submit_check(4, 1, "b.ppu"), Err(ERR_FULL));
}

#[test]
fn plugin_and_firmware_jobs_run_one_at_a_time_in_queue_order() {
    let mut d = driver(true);
    let (job, seen) = check_job(1, 1, 3);
    d.submit_plugin(job).unwrap();
    d.submit_check(2, 1, "a.ppu").unwrap();
    // The plugin job is the head: it runs its three steps, the firmware
    // check behind it does not start (the machine stays idle).
    let ended = run(&mut d);
    assert!(matches!(ended, Event::Ended(o) if (o.owner, o.kind, o.reason) == (1, KIND_CHECK, 0)));
    assert_eq!(seen.steps.get(), 3);
    assert!(seen.dropped.get());
    assert!(!d.machine().unwrap().borrow().busy());
    assert!(d.holds(2, 1));
    // Then the firmware check, which ends in its turn.
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 2));
    // And a firmware job in front keeps the plugin job waiting.
    let mut d = driver(true);
    d.submit_check(1, 1, "a.ppu").unwrap();
    let (job, seen) = check_job(2, 1, 1);
    d.submit_plugin(job).unwrap();
    d.pump(&NoStorage, &Names, 100);
    assert!(d.machine().unwrap().borrow().busy());
    assert_eq!(seen.steps.get(), 0);
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 1));
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 2));
    assert_eq!(seen.steps.get(), 1);
}

#[test]
fn a_plugin_checks_facts_wait_in_its_owners_result_slot_for_op_6() {
    let mut d = driver(true);
    let (job, _) = check_job(1, 7, 2);
    d.submit_plugin(job).unwrap();
    let machine = d.machine().unwrap();
    // Queued and running: the slot is reserved, empty (`ERR_BUSY`).
    assert_eq!(machine.borrow().checked(1).err(), Some(ERR_BUSY));
    d.pump(&NoStorage, &Names, 100);
    assert_eq!(machine.borrow().checked(1).err(), Some(ERR_BUSY));
    assert!(matches!(run(&mut d), Event::Ended(_)));
    let borrowed = machine.borrow();
    let checked = borrowed.checked(1).unwrap();
    assert_eq!((checked.request, checked.reason), (7, 0));
    assert!(checked.is_plugin());
    assert!(checked.facts().is_none());
    assert_eq!(checked.plugin_facts(), Some(&facts("dials")));
    // Op 16 is a firmware check's: a plugin's result is not one.
    assert_eq!(
        borrowed.leaves_out_of(1).err(),
        Some(paperpaw_native::host_glue::ERR_NOT_FOUND)
    );
}

#[test]
fn a_refused_plugin_check_leaves_its_reason_and_no_facts() {
    let mut d = driver(true);
    d.submit_plugin(PluginJobs::Script(Script {
        owner: 1,
        request: 1,
        kind: KIND_CHECK,
        steps: 1,
        done: Finished::refused(job_reason::UNSIGNED),
        seen: Rc::default(),
        must_finish: false,
        keep: false,
        asks: false,
        answered: None,
    }))
    .unwrap();
    assert!(matches!(run(&mut d), Event::Ended(o) if o.reason == job_reason::UNSIGNED));
    let machine = d.machine().unwrap();
    let checked = machine.borrow().checked(1).unwrap();
    assert_eq!(checked.reason, job_reason::UNSIGNED);
    assert!(checked.plugin_facts().is_none() && checked.facts().is_none());
}

#[test]
fn op_13_sees_a_plugin_job_waiting_running_and_ended() {
    let mut d = driver(true);
    let (job, _) = check_job(1, 1, 2);
    d.submit_plugin(job).unwrap();
    assert_eq!(
        d.lookup(1, 1).map(|l| (l.kind, l.state)),
        Some((KIND_CHECK, 0))
    );
    assert!(d.has_work(0));
    d.pump(&NoStorage, &Names, 100);
    assert_eq!(d.lookup(1, 1).map(|l| l.state), Some(1));
    assert!(d.has_work(0));
    assert!(matches!(run(&mut d), Event::Ended(_)));
    assert_eq!(d.lookup(1, 1).map(|l| l.state), Some(2));
    d.take_ready().unwrap();
    assert!(!d.has_work(0));
}

#[test]
fn a_stop_ends_the_running_plugin_job_and_the_waiting_ones_with_reason_twenty() {
    let mut d = driver(true);
    let (job, seen) = check_job(1, 1, 5);
    d.submit_plugin(job).unwrap();
    let (waiting, waited) = check_job(2, 1, 1);
    d.submit_plugin(waiting).unwrap();
    d.pump(&NoStorage, &Names, 100);
    d.pump(&NoStorage, &Names, 200);
    d.abort_all();
    assert_eq!(seen.aborted.get(), Some(20));
    assert!(seen.dropped.get() && waited.dropped.get());
    assert_eq!(d.waiting(), 0);
    let first = d.take_ready().unwrap();
    let second = d.take_ready().unwrap();
    assert_eq!((first.owner, first.kind, first.reason), (1, KIND_CHECK, 20));
    assert_eq!((second.owner, second.reason), (2, 20));
    // Their check slots say why, not busy.
    let machine = d.machine().unwrap();
    assert_eq!(machine.borrow().checked(1).unwrap().reason, 20);
    assert_eq!(machine.borrow().checked(2).unwrap().reason, 20);
}

#[test]
fn a_sender_that_stopped_takes_its_running_plugin_job_and_leaves_the_others() {
    let mut d = driver(true);
    let (job, seen) = check_job(1, 1, 5);
    d.submit_plugin(job).unwrap();
    d.submit_check(2, 1, "a.ppu").unwrap();
    d.pump(&NoStorage, &Names, 100);
    d.sender_stopped(1);
    assert_eq!(seen.aborted.get(), Some(20));
    assert!(seen.dropped.get());
    assert!(!d.holds(1, 1));
    assert!(d.take_ready().is_none());
    assert!(d.machine().unwrap().borrow().checked(1).is_err());
    // The firmware check of another plugin still runs (the machine was not
    // aborted for the plugin job's sake).
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 2 && o.reason == 0));
}

#[test]
fn a_cancelled_plugin_job_leaves_the_queue_and_gives_its_places_back() {
    let mut d = driver(true);
    for owner in 1..=4 {
        let (job, seen) = check_job(owner, 1, 1);
        d.submit_plugin(job).unwrap();
        d.cancel(owner, 1);
        assert!(seen.dropped.get());
    }
    assert_eq!(d.waiting(), 0);
    // Four cancelled checks keep no result place.
    d.submit_check(5, 1, "a.ppu").unwrap();
}

#[test]
fn a_job_that_must_finish_is_stepped_on_after_a_stop_and_ends_with_its_own_reason() {
    let mut d = driver(true);
    let (job, seen) = install_job(1, 1, 5, true, false);
    d.submit_plugin(job).unwrap();
    d.submit_check(2, 1, "a.ppu").unwrap();
    d.pump(&NoStorage, &Names, 100);
    d.pump(&NoStorage, &Names, 200);
    d.abort_all();
    // Aborted, not abandoned: still running, its entry kept, the waiting
    // check ended with reason 20.
    assert_eq!(seen.aborted.get(), Some(20));
    assert!(!seen.dropped.get());
    assert!(d.holds(1, 1) && !d.holds(2, 1));
    let waiting = d.take_ready().unwrap();
    assert_eq!((waiting.owner, waiting.reason), (2, 20));
    // The bounded finish takes it to its end: its own reason (0), told.
    d.finish_stopped(&NoStorage, &Names);
    assert!(seen.dropped.get());
    assert_eq!(seen.steps.get(), 5);
    let done = d.take_ready().unwrap();
    assert_eq!((done.owner, done.kind, done.reason), (1, KIND_INSTALL, 0));
    assert_eq!(d.waiting(), 0);
}

#[test]
fn a_job_that_must_finish_but_cannot_in_one_go_is_left_to_the_pump() {
    let mut d = driver(true);
    let (job, seen) = install_job(1, 1, FINISH_STEPS + 10, true, false);
    d.submit_plugin(job).unwrap();
    d.pump(&NoStorage, &Names, 100);
    d.abort_all();
    d.finish_stopped(&NoStorage, &Names);
    assert_eq!(seen.steps.get(), FINISH_STEPS);
    assert!(d.has_work(0), "the pump goes on");
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 1 && o.reason == 0));
}

#[test]
fn a_sender_that_stops_leaves_a_confirmed_install_running_untold() {
    let mut d = driver(true);
    let (job, seen) = install_job(1, 1, 3, false, true);
    d.submit_plugin(job).unwrap();
    d.pump(&NoStorage, &Names, 100);
    d.pump(&NoStorage, &Names, 200);
    d.sender_stopped(1);
    // Not aborted, no entry for its sender, still running.
    assert_eq!(seen.aborted.get(), None);
    assert!(!d.holds(1, 1));
    // A stop does not step it (it may take long): the pump does.
    d.finish_stopped(&NoStorage, &Names);
    assert_eq!(seen.steps.get(), 1);
    for now in (300..2_000).step_by(100) {
        d.pump(&NoStorage, &Names, now);
    }
    assert_eq!(seen.steps.get(), 3);
    assert!(seen.dropped.get());
    // Nobody is told, nothing is kept for it, and the queue is free.
    assert!(d.take_ready().is_none());
    assert_eq!(d.kept(1, KIND_INSTALL), None);
    assert!(!d.has_work(0));
    d.submit_plugin(job_of_kind(2, 1, KIND_INSTALL)).unwrap();
}

#[test]
fn a_stop_after_the_sender_stopped_leaves_no_stale_mark_for_the_next_job() {
    // The sender stops (the job runs on, untold); then everything stops and
    // the job, which may be abandoned, goes: nothing is told for it, and the
    // next job's end is told as any.
    let mut d = driver(true);
    let (job, seen) = install_job(1, 1, 9, false, true);
    d.submit_plugin(job).unwrap();
    d.pump(&NoStorage, &Names, 100);
    d.pump(&NoStorage, &Names, 200);
    d.sender_stopped(1);
    d.abort_all();
    assert_eq!(seen.aborted.get(), Some(20));
    assert!(seen.dropped.get());
    assert!(d.take_ready().is_none(), "nobody is told for the orphan");
    assert_eq!(d.waiting(), 0);
    let (next, _) = check_job(2, 1, 1);
    d.submit_plugin(next).unwrap();
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 2 && o.reason == 0));
    assert_eq!(d.take_ready().map(|o| o.owner), Some(2));
    assert_eq!(d.waiting(), 0);
}

#[test]
fn a_stop_keeps_the_untold_mark_of_a_job_that_must_finish() {
    // Its sender stopped (`Some(false)`), then a stop: the job must finish,
    // and still nobody is told (the mark is not overwritten with "told").
    let mut d = driver(true);
    let (job, seen) = install_job(1, 1, 5, true, false);
    d.submit_plugin(job).unwrap();
    d.pump(&NoStorage, &Names, 100);
    d.pump(&NoStorage, &Names, 200);
    d.sender_stopped(1);
    d.abort_all();
    d.finish_stopped(&NoStorage, &Names);
    assert_eq!(seen.steps.get(), 5);
    assert!(seen.dropped.get());
    assert!(d.take_ready().is_none());
    assert_eq!(d.waiting(), 0);
    assert!(!d.has_work(0));
}

#[test]
fn a_sender_that_stops_takes_an_install_that_may_be_abandoned() {
    let mut d = driver(true);
    let (job, seen) = install_job(1, 1, 3, false, false);
    d.submit_plugin(job).unwrap();
    d.pump(&NoStorage, &Names, 100);
    d.sender_stopped(1);
    assert_eq!(seen.aborted.get(), Some(20));
    assert!(seen.dropped.get());
    assert!(!d.has_work(0));
}

#[test]
fn a_question_waits_for_the_host_and_one_it_cannot_show_is_a_dismissal_never_a_yes() {
    let mut d = driver(true);
    let seen = Rc::new(Seen::default());
    d.submit_plugin(PluginJobs::Script(Script {
        owner: 1,
        request: 1,
        kind: KIND_INSTALL,
        steps: 1,
        done: Finished::refused(job_reason::OK),
        seen: Rc::clone(&seen),
        must_finish: false,
        keep: false,
        asks: true,
        answered: None,
    }))
    .unwrap();
    for now in 0..10 {
        d.pump(&NoStorage, &Names, now * 100);
    }
    // It asked once and waits: nothing steps it, and it is no work.
    assert_eq!(seen.steps.get(), 1);
    let asked = d.peek_plugin_question().expect("a question waits");
    assert!(!d.has_work(1_000));
    // The host shows it (nonce 7): it is no longer waiting to be shown, and
    // only the screen's nonce answers.
    d.plugin_question_shown(7, &asked);
    assert!(d.peek_plugin_question().is_none());
    assert!(d.answer_shown(8, Answer::Yes).is_none());
    assert_eq!(seen.steps.get(), 1);
    // The screen cannot be kept: dismissed (reason 3), never a yes.
    assert!(d.answer_shown(7, Answer::Dismissed).is_some());
    let ended = run(&mut d);
    assert!(
        matches!(ended, Event::Ended(o) if (o.owner, o.kind, o.reason) == (1, KIND_INSTALL, 3))
    );
    assert_eq!(seen.steps.get(), 2, "asked, then ended with the answer");
    assert!(seen.dropped.get());
}

#[test]
fn a_plugin_checks_needs_draft_waits_in_the_result_slot_for_op_6() {
    let mut d = driver(true);
    d.submit_plugin(PluginJobs::Script(Script {
        owner: 1,
        request: 1,
        kind: KIND_CHECK,
        steps: 1,
        done: Finished::needs_newer(7),
        seen: Rc::default(),
        must_finish: false,
        keep: false,
        asks: false,
        answered: None,
    }))
    .unwrap();
    assert!(matches!(run(&mut d), Event::Ended(o) if o.reason == job_reason::NEEDS_NEWER));
    let machine = d.machine().unwrap();
    let checked = machine.borrow().checked(1).unwrap();
    assert_eq!(
        (checked.reason, checked.needs_draft()),
        (job_reason::NEEDS_NEWER, 7)
    );
    // The next check that fits says 0 again.
    let (job, _) = check_job(1, 2, 1);
    d.submit_plugin(job).unwrap();
    assert!(matches!(run(&mut d), Event::Ended(_)));
    assert_eq!(machine.borrow().checked(1).unwrap().needs_draft(), 0);
}
