//! A plugin install in the firmware queue, end to end: the question waits for
//! the user's answer, a yes runs it, the asking plugin going away stops
//! nothing after the yes, and the outcome carries the installed plugin's id.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::firmware_install::FirmwareInstall;
use crate::native_host::plugin_install::PluginInstall;
use crate::native_host::plugin_job::{InstallGrants, KIND_INSTALL};
use crate::native_host::plugin_jobs::PluginJobs;
use crate::native_host::plugin_rules::Evidence;
use core::cell::RefCell;
use paperpaw_native::{
    header5::{Host as Loader, X4_PROFILE},
    metadata::Host as Offered,
};
use paperpaw_platform::firmware::WRITE_BLOCK;
use paperpaw_platform::firmware_fake::{FakeFirmware, paperpaw_facts};
use paperpaw_sdk::MemoryStorage;
use std::{format, string::ToString};

const OWNER: u64 = 7;
const SOURCE: &str = "Download/dials";

struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        0
    }
    fn index_of(&self, _: &str) -> Option<usize> {
        None
    }
}

struct Grant(bool);
impl InstallGrants for Grant {
    fn granted(&self, _: u64) -> bool {
        self.0
    }
}

fn driver() -> Driver {
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
    d.set_plugin_host(PluginHost {
        loader: Loader::x4(1),
        offered: Offered {
            abi_major: 7,
            minor: 1,
            profile: X4_PROFILE,
            services: &[],
            features: &[],
        },
        signers: &[],
    });
    d
}

fn source() -> MemoryStorage {
    let card = MemoryStorage::new();
    card.write(
        &format!("{SOURCE}/plugin.ini"),
        b"[plugin]\nid = dials\nname = Dials\nversion = 1.0.0\n",
    )
    .unwrap();
    card.write(&format!("{SOURCE}/strings.ini"), b"hello\n")
        .unwrap();
    card
}

fn submit(d: &mut Driver) {
    let job = PluginInstall::new(OWNER, 3, SOURCE, 0, Evidence::NONE).unwrap();
    d.submit_plugin(PluginJobs::Install(job)).unwrap();
}

/// Pumps until the job asks or an outcome is ready.
fn until_asked(d: &mut Driver, card: &MemoryStorage) {
    for now in (100..60_000).step_by(100) {
        d.pump(card, &Names, now);
        if d.plugin_question().is_some() || d.peek_ready().is_some() {
            return;
        }
    }
    panic!("never asked");
}

fn until_ready(d: &mut Driver, card: &MemoryStorage) -> Outcome {
    for now in (100..60_000).step_by(100) {
        d.pump(card, &Names, now);
        if let Some(outcome) = d.take_ready() {
            return outcome;
        }
    }
    panic!("never ended");
}

#[test]
fn the_install_waits_for_the_answer_and_a_yes_runs_it_with_the_plugins_id_in_the_outcome() {
    let card = source();
    let mut d = driver();
    submit(&mut d);
    until_asked(&mut d, &card);
    let question = *d.plugin_question().expect("a question");
    assert_eq!((question.owner, question.request), (OWNER, 3));
    assert_eq!(question.id.as_str(), "dials");
    // Nothing runs while it waits; an answer for another request is none.
    for now in (100_000..101_000).step_by(100) {
        d.pump(&card, &Names, now);
    }
    assert!(d.plugin_question().is_some() && d.peek_ready().is_none());
    assert!(!d.answer_plugin(OWNER, 99, Answer::Yes));
    assert!(!d.answer_plugin(OWNER + 1, 3, Answer::Yes));
    assert!(d.answer_plugin(OWNER, 3, Answer::Yes));
    assert!(d.plugin_question().is_none());
    let done = until_ready(&mut d, &card);
    assert_eq!((done.kind, done.reason), (KIND_INSTALL, 0));
    // The installed plugin's id, not the asker's.
    assert_eq!(
        done.id.map(|id| id.as_str().to_string()).as_deref(),
        Some("dials")
    );
    assert!(card.exists("plugins/dials/plugin.ini"));
}

#[test]
fn a_no_removes_the_stage_and_ends_with_reason_2() {
    let card = source();
    let mut d = driver();
    submit(&mut d);
    until_asked(&mut d, &card);
    assert!(d.answer_plugin(OWNER, 3, Answer::No));
    let done = until_ready(&mut d, &card);
    assert_eq!(done.reason, 2);
    assert!(!card.exists("plugins/dials/plugin.ini"));
    assert!(!card.exists("plugins/.dials.part/plugin.ini"));
}

#[test]
fn the_asking_plugin_stopping_before_the_answer_ends_the_install_untold() {
    let card = source();
    let mut d = driver();
    submit(&mut d);
    until_asked(&mut d, &card);
    d.sender_stopped(OWNER);
    assert!(d.plugin_question().is_none());
    for now in (100_000..102_000).step_by(100) {
        d.pump(&card, &Names, now);
    }
    assert!(d.take_ready().is_none(), "nobody is told");
    assert!(!card.exists("plugins/dials/plugin.ini"));
}

#[test]
fn after_the_yes_the_install_runs_on_when_its_plugin_stops() {
    let card = source();
    let mut d = driver();
    submit(&mut d);
    until_asked(&mut d, &card);
    assert!(d.answer_plugin(OWNER, 3, Answer::Yes));
    d.sender_stopped(OWNER);
    d.finish_stopped(&card, &Names);
    for now in (100_000..110_000).step_by(100) {
        d.pump(&card, &Names, now);
    }
    // Done, and nobody told.
    assert!(card.exists("plugins/dials/plugin.ini"));
    assert!(d.take_ready().is_none());
}

#[test]
fn only_the_hosts_grant_skips_the_question_and_never_for_a_sideload() {
    // A data plugin's first install is asked about even with the grant.
    let card = source();
    let mut d = driver();
    d.set_install_grants(Box::new(Grant(true)));
    submit(&mut d);
    until_asked(&mut d, &card);
    assert!(d.plugin_question().is_some());
    // An update of the installed plugin under the grant goes without one.
    let card = source();
    card.write(
        "plugins/dials/plugin.ini",
        b"[plugin]\nid = dials\nversion = 0.9.0\n",
    )
    .unwrap();
    let mut d = driver();
    d.set_install_grants(Box::new(Grant(true)));
    let job = PluginInstall::new(OWNER, 3, SOURCE, 1, Evidence::NONE).unwrap();
    d.submit_plugin(PluginJobs::Install(job)).unwrap();
    let done = until_ready(&mut d, &card);
    assert_eq!(done.reason, 0);
    assert!(d.plugin_question().is_none());
    // Without the grant the same update asks.
    let card = source();
    card.write(
        "plugins/dials/plugin.ini",
        b"[plugin]\nid = dials\nversion = 0.9.0\n",
    )
    .unwrap();
    let mut d = driver();
    let job = PluginInstall::new(OWNER, 3, SOURCE, 1, Evidence::NONE).unwrap();
    d.submit_plugin(PluginJobs::Install(job)).unwrap();
    until_asked(&mut d, &card);
    assert!(d.plugin_question().is_some());
}

/// An installed update of the plugin, the card the job reads.
fn update_source() -> MemoryStorage {
    let card = source();
    card.write(
        "plugins/dials/plugin.ini",
        b"[plugin]\nid = dials\nversion = 0.9.0\n",
    )
    .unwrap();
    card
}

fn grants(owners: &[u64]) -> crate::native_host::install_grants::Granted {
    let mut granted = crate::native_host::install_grants::Granted::NONE;
    for owner in owners {
        granted.add(*owner);
    }
    granted
}

#[test]
fn a_revoke_after_the_decision_does_not_end_the_job_and_the_next_job_asks() {
    // Revoke the grant after `steps` pump steps, for every point of the job:
    // either it had not decided yet (it asks) or it runs on and finishes.
    let mut finished_after_revoke = 0;
    for steps in 1..40 {
        let card = update_source();
        let mut d = driver();
        d.feed_install_grants(grants(&[OWNER]));
        let job = PluginInstall::new(OWNER, 3, SOURCE, 1, Evidence::NONE).unwrap();
        d.submit_plugin(PluginJobs::Install(job)).unwrap();
        let mut now = 100;
        for _ in 0..steps {
            d.pump(&card, &Names, now);
            now += 100;
        }
        if d.peek_ready().is_some() || d.plugin_question().is_some() {
            continue;
        }
        d.feed_install_grants(grants(&[]));
        until_asked(&mut d, &card);
        if d.plugin_question().is_some() {
            // It had not decided yet: the revoke came first, so it asks.
            continue;
        }
        let done = d.take_ready().expect("an outcome");
        assert_eq!(
            done.reason, 0,
            "a revoke ends nothing (after {steps} steps)"
        );
        finished_after_revoke += 1;
    }
    assert!(finished_after_revoke > 0);
    // The next job, after the revoke, asks.
    let card = update_source();
    let mut d = driver();
    d.feed_install_grants(grants(&[]));
    let job = PluginInstall::new(OWNER, 4, SOURCE, 1, Evidence::NONE).unwrap();
    d.submit_plugin(PluginJobs::Install(job)).unwrap();
    until_asked(&mut d, &card);
    assert!(d.plugin_question().is_some());
}
