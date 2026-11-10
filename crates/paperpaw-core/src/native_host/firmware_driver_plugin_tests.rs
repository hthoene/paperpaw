//! Plugin jobs in the firmware queue: the seam of `firmware_driver` with
//! `plugin_job`, against a job that takes a scripted number of steps.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::firmware_install::KIND_CHECK;
use crate::native_host::plugin_job::{
    Finished, KIND_INSTALL, KIND_REMOVAL, PluginFacts, reason as job_reason,
};
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
        loader: Loader::x4(1),
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
struct Seen {
    steps: Cell<u32>,
    aborted: Cell<Option<u8>>,
    dropped: Cell<bool>,
}

struct Script {
    owner: u64,
    request: u32,
    kind: u32,
    steps: u32,
    done: Finished,
    seen: Rc<Seen>,
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
        if self.seen.steps.get() >= self.steps {
            JobStep::Done(Finished {
                reason: self.done.reason,
                facts: self.done.facts,
            })
        } else {
            JobStep::More
        }
    }
    fn abort(&mut self, reason: u8) {
        self.seen.aborted.set(Some(reason));
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        self.seen.dropped.set(true);
    }
}

fn check_job(owner: u64, request: u32, steps: u32) -> (Box<dyn PluginJob>, Rc<Seen>) {
    let seen = Rc::new(Seen::default());
    let job = Script {
        owner,
        request,
        kind: KIND_CHECK,
        steps,
        done: Finished {
            reason: 0,
            facts: Some(facts("dials")),
        },
        seen: Rc::clone(&seen),
    };
    (Box::new(job), seen)
}

fn job_of_kind(owner: u64, request: u32, kind: u32) -> Box<dyn PluginJob> {
    Box::new(Script {
        owner,
        request,
        kind,
        steps: 1,
        done: Finished::refused(job_reason::OK),
        seen: Rc::default(),
    })
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
        Rc::new(RefCell::new(FirmwareInstall::new())),
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
    assert!(!d.machine().borrow().busy());
    assert!(d.holds(2, 1));
    // Then the firmware check, which ends in its turn.
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 2));
    // And a firmware job in front keeps the plugin job waiting.
    let mut d = driver(true);
    d.submit_check(1, 1, "a.ppu").unwrap();
    let (job, seen) = check_job(2, 1, 1);
    d.submit_plugin(job).unwrap();
    d.pump(&NoStorage, &Names, 100);
    assert!(d.machine().borrow().busy());
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
    let machine = d.machine();
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
    d.submit_plugin(Box::new(Script {
        owner: 1,
        request: 1,
        kind: KIND_CHECK,
        steps: 1,
        done: Finished::refused(job_reason::UNSIGNED),
        seen: Rc::default(),
    }))
    .unwrap();
    assert!(matches!(run(&mut d), Event::Ended(o) if o.reason == job_reason::UNSIGNED));
    let machine = d.machine();
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
    let machine = d.machine();
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
    assert!(d.machine().borrow().checked(1).is_err());
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
