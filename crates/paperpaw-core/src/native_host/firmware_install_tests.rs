//! The firmware install machine against the scripted device
//! (`paperpaw_platform::firmware_fake`): the whole install, what the check
//! refuses, the always-asked confirmation, a failure at every stage, the
//! restart's gates and the check results.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use paperpaw_platform::{
    NoStorage,
    firmware::{Signer, Text, WRITE_BLOCK},
    firmware_fake::{Call, FakeFirmware, Stage, paperpaw_facts},
};

const OWNER: u64 = 7;
const OTHER: u64 = 8;

/// Built into the running image: stats, weather, clock.
struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        3
    }
    fn index_of(&self, name: &str) -> Option<usize> {
        ["stats", "weather", "clock"]
            .iter()
            .position(|known| *known == name)
    }
}

fn fake() -> FakeFirmware {
    let mut fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 3 * WRITE_BLOCK),
    );
    fake.built_ins = std::vec!["stats".into()];
    fake
}

/// The question of an install's confirmation.
fn install_q(owner: u64, request: u32) -> Question {
    Question {
        owner,
        request,
        restart: false,
    }
}

/// The question of a restart's confirmation.
fn restart_q(owner: u64, request: u32) -> Question {
    Question {
        owner,
        request,
        restart: true,
    }
}

fn install(owner: u64, request: u32, flags: u32) -> Job {
    Job::install(owner, request, "update.ppu", None, flags).unwrap()
}

/// Steps until something other than `Quiet` or `Progress` happens.
fn run(machine: &mut FirmwareInstall, fw: &mut FakeFirmware) -> Event {
    let mut now = 0;
    for _ in 0..64 {
        now += 100;
        match machine.step(fw, &NoStorage, &Names, now) {
            Event::Quiet | Event::Progress { .. } => {}
            other => return other,
        }
    }
    panic!("never ended");
}

/// A whole install, answered yes, up to its end.
fn installed(machine: &mut FirmwareInstall, fw: &mut FakeFirmware, owner: u64) -> Outcome {
    assert_eq!(machine.start(install(owner, 1, 0), fw), None);
    assert!(matches!(run(machine, fw), Event::Ask(_)));
    assert_eq!(
        machine.answer(install_q(owner, 1), Answer::Yes, fw),
        Event::Quiet
    );
    match run(machine, fw) {
        Event::Ended(outcome) => outcome,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_whole_install_asks_first_binds_the_digest_and_waits_for_its_restart() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    assert_eq!(machine.start(install(OWNER, 3, 0), &mut fw), None);
    assert_eq!(machine.state(), State::Installing);
    let Event::Ask(ask) = run(&mut machine, &mut fw) else {
        panic!("not asked");
    };
    assert_eq!((ask.owner, ask.request), (OWNER, 3));
    assert_eq!(ask.version.as_str(), "0.6.0");
    assert!(!ask.foreign && !ask.older && !ask.other_key && !ask.restart);
    assert_eq!(machine.state(), State::WaitingForUser);
    // Nothing was written before the yes.
    assert!(
        !fw.log
            .iter()
            .any(|call| matches!(call, Call::BeginWrite { .. }))
    );
    assert_eq!(
        machine.answer(ask.question(), Answer::Yes, &mut fw),
        Event::Quiet
    );
    assert_eq!(machine.state(), State::Installing);
    let Event::Ended(outcome) = run(&mut machine, &mut fw) else {
        panic!("not ended");
    };
    assert_eq!(
        outcome,
        Outcome {
            owner: OWNER,
            request: 3,
            kind: KIND_FIRMWARE,
            reason: 0,
            flags: 0,
            id: None,
        }
    );
    assert_eq!(machine.state(), State::Installed);
    assert_eq!(machine.last(OWNER), (3, 0));
    // Written to the digest of the check, committed, never abandoned.
    let digest = paperpaw_facts("0.6.0", "p", 1).sha256;
    assert!(fw.log.iter().any(|call| matches!(
        call,
        Call::BeginWrite { digest: d, foreign: false, .. } if *d == digest
    )));
    assert!(fw.log.contains(&Call::Commit));
    assert!(!fw.log.contains(&Call::Abandon));
    assert!(!machine.busy());
}

#[test]
fn progress_is_told_at_most_every_250_ms() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    machine.start(install(OWNER, 1, 0), &mut fw);
    run(&mut machine, &mut fw);
    machine.answer(install_q(OWNER, 1), Answer::Yes, &mut fw);
    // 3 blocks, a step every 100 ms: the first is due, the next two are not.
    let mut told = std::vec::Vec::new();
    for step in 1..=3 {
        if let Event::Progress { done, total, .. } =
            machine.step(&mut fw, &NoStorage, &Names, step * 100)
        {
            told.push((done, total));
        }
    }
    assert_eq!(told, [(WRITE_BLOCK, 3 * WRITE_BLOCK)]);
}

/// A name, the change to the device, the install flags, the reason.
type Case = (&'static str, fn(&mut FakeFirmware), u32, u8);

#[test]
fn what_the_check_refuses_is_the_reason_an_install_would_end_with() {
    let cases: [Case; 6] = [
        ("other chip", |f| f.facts.chip_ok = false, 0, 12),
        ("no room", |f| f.room = WRITE_BLOCK as u64, 0, 13),
        ("not here", |f| f.room = 0, 0, 19),
        ("unsigned", |f| f.facts.signer = Signer::Unsigned, 0, 8),
        (
            "an unaccepted key",
            |f| f.facts.signer = Signer::Other { key: [1; 32] },
            0,
            9,
        ),
        (
            "the same version",
            |f| f.facts.version = Text::new("0.5.0"),
            0,
            14,
        ),
    ];
    for (name, change, flags, reason) in cases {
        let mut machine = FirmwareInstall::new();
        let mut fw = fake();
        change(&mut fw);
        machine.start(install(OWNER, 1, flags), &mut fw);
        let Event::Ended(outcome) = run(&mut machine, &mut fw) else {
            panic!("{name}: asked");
        };
        assert_eq!(outcome.reason, reason, "{name}");
        assert_eq!(machine.state(), State::Failed, "{name}");
        assert!(!fw.log.iter().any(|c| matches!(c, Call::BeginWrite { .. })));
    }
}

#[test]
fn an_older_version_installs_only_with_the_flag_and_is_named_older() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    fw.facts.version = Text::new("0.4.0");
    machine.start(install(OWNER, 1, FLAG_OLDER), &mut fw);
    let Event::Ask(ask) = run(&mut machine, &mut fw) else {
        panic!("not asked");
    };
    assert!(ask.older);
}

#[test]
fn a_key_that_is_not_paperpaws_installs_only_when_the_plugin_names_it_and_the_user_accepts() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    fw.facts.signer = Signer::Other { key: [1; 32] };
    let job = Job::install(OWNER, 1, "u.ppu", Some([1; 32]), 0).unwrap();
    machine.start(job, &mut fw);
    let Event::Ask(ask) = run(&mut machine, &mut fw) else {
        panic!("not asked");
    };
    assert!(ask.other_key, "the confirmation shows the key");
    machine.answer(install_q(OWNER, 1), Answer::Yes, &mut fw);
    assert!(fw.log.iter().any(|call| matches!(
        call,
        Call::BeginWrite { key: Some(key), .. } if *key == [1; 32]
    )));
}

#[test]
fn a_foreign_image_is_asked_too_and_is_checked_as_foreign() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    fw.facts.foreign = true;
    fw.facts.signer = Signer::Unsigned;
    machine.start(install(OWNER, 1, FLAG_FOREIGN), &mut fw);
    let Event::Ask(ask) = run(&mut machine, &mut fw) else {
        panic!("not asked");
    };
    assert!(ask.foreign);
    // An image that is not the kind the request named is refused.
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    machine.start(install(OWNER, 1, FLAG_FOREIGN), &mut fw);
    assert!(matches!(run(&mut machine, &mut fw), Event::Ended(o) if o.reason == 4));
    // And a foreign image without flag 1 is refused before any question.
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    fw.facts.foreign = true;
    fw.facts.signer = Signer::Unsigned;
    machine.start(install(OWNER, 1, 0), &mut fw);
    assert!(matches!(run(&mut machine, &mut fw), Event::Ended(o) if o.reason == 4));
    assert!(
        !fw.log
            .iter()
            .any(|call| matches!(call, Call::BeginWrite { .. }))
    );
}

#[test]
fn a_no_or_a_dismissal_writes_nothing_and_ends_with_reason_2_or_3() {
    for (answer, reason) in [(Answer::No, 2), (Answer::Dismissed, 3)] {
        let mut machine = FirmwareInstall::new();
        let mut fw = fake();
        machine.start(install(OWNER, 1, 0), &mut fw);
        run(&mut machine, &mut fw);
        let Event::Ended(outcome) = machine.answer(install_q(OWNER, 1), answer, &mut fw) else {
            panic!("not ended");
        };
        assert_eq!(outcome.reason, reason);
        assert!(!fw.log.iter().any(|c| matches!(c, Call::BeginWrite { .. })));
        assert!(!machine.busy());
    }
}

#[test]
fn a_failure_at_any_stage_ends_the_install_and_leaves_no_state_3() {
    let stages = [
        (Stage::BeginCheck, 0, 16),
        (Stage::Check, 0, 16),
        (Stage::BeginWrite, 0, 19),
        (Stage::Write, 1, 18),
        (Stage::Readback, 0, 18),
        (Stage::Commit, 0, 1),
    ];
    for (stage, nth, reason) in stages {
        let mut machine = FirmwareInstall::new();
        let mut fw = fake();
        fw.fail = Some((stage, nth, Reason(reason)));
        let first = machine.start(install(OWNER, 1, 0), &mut fw);
        let outcome = match first {
            Some(outcome) => outcome,
            None => match run(&mut machine, &mut fw) {
                Event::Ask(_) => {
                    let mut event = machine.answer(install_q(OWNER, 1), Answer::Yes, &mut fw);
                    if event == Event::Quiet {
                        event = run(&mut machine, &mut fw);
                    }
                    match event {
                        Event::Ended(outcome) => outcome,
                        other => panic!("{stage:?}: {other:?}"),
                    }
                }
                Event::Ended(outcome) => outcome,
                other => panic!("{stage:?}: {other:?}"),
            },
        };
        assert_eq!(outcome.reason, reason, "{stage:?}");
        assert_eq!(machine.state(), State::Failed, "{stage:?}");
        assert_eq!(machine.last(OWNER), (1, reason), "{stage:?}");
        assert!(!machine.busy(), "{stage:?}: the job is closed");
        assert!(!fw.committed || stage == Stage::Restart, "{stage:?}");
    }
}

#[test]
fn only_the_plugin_whose_install_waits_may_restart_and_the_host_asks_again() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    assert_eq!(machine.request_restart(OWNER, 9).unwrap_err(), ERR_STATE);
    assert_eq!(installed(&mut machine, &mut fw, OWNER).reason, 0);
    assert_eq!(machine.request_restart(OTHER, 9).unwrap_err(), ERR_DENIED);
    let ask = machine.request_restart(OWNER, 9).unwrap();
    assert!(ask.restart);
    assert_eq!(ask.version.as_str(), "0.6.0");
    assert_eq!(machine.state(), State::WaitingForUser);
    // No: ended with reason 2, the image keeps waiting.
    let Event::Ended(outcome) = machine.answer(restart_q(OWNER, 9), Answer::No, &mut fw) else {
        panic!("not ended");
    };
    assert_eq!(
        (outcome.kind, outcome.request, outcome.reason),
        (KIND_FIRMWARE, 9, 2)
    );
    assert_eq!(machine.state(), State::Installed);
    // Dismissed: reason 3.
    machine.request_restart(OWNER, 9).unwrap();
    assert!(matches!(
        machine.answer(restart_q(OWNER, 9), Answer::Dismissed, &mut fw),
        Event::Ended(o) if o.reason == 3
    ));
    assert!(!fw.log.contains(&Call::Restart));
    // Yes: the caller saves what plugins keep, then restarts.
    machine.request_restart(OWNER, 9).unwrap();
    assert_eq!(
        machine.answer(restart_q(OWNER, 9), Answer::Yes, &mut fw),
        Event::Restart
    );
    assert_eq!(machine.restart(&mut fw), None);
    assert!(fw.log.contains(&Call::Restart));
}

#[test]
fn a_restart_that_cannot_happen_says_why_and_the_image_keeps_waiting() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    installed(&mut machine, &mut fw, OWNER);
    fw.can_restart = false;
    machine.request_restart(OWNER, 9).unwrap();
    assert_eq!(
        machine.answer(restart_q(OWNER, 9), Answer::Yes, &mut fw),
        Event::Restart
    );
    let outcome = machine.restart(&mut fw).unwrap();
    assert_eq!(outcome.reason, 19);
    assert_eq!(machine.state(), State::Installed);
    assert!(!machine.busy());
}

#[test]
fn a_firmware_request_while_an_image_waits_ends_with_reason_21() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    installed(&mut machine, &mut fw, OWNER);
    let outcome = machine.start(install(OTHER, 2, 0), &mut fw).unwrap();
    assert_eq!((outcome.owner, outcome.reason), (OTHER, 21));
    assert_eq!(machine.state(), State::Installed, "the waiting image stays");
    assert!(
        !fw.log.iter().any(|c| matches!(c, Call::BeginCheck { .. }))
            || fw
                .log
                .iter()
                .filter(|c| matches!(c, Call::BeginCheck { .. }))
                .count()
                == 1
    );
}

#[test]
fn flag_bits_other_than_0_and_1_are_invalid() {
    assert_eq!(
        Job::install(OWNER, 1, "u.ppu", None, 4).err(),
        Some(ERR_INVALID)
    );
    assert_eq!(
        Job::install(OWNER, 1, "u.ppu", None, 1 << 31).err(),
        Some(ERR_INVALID)
    );
    assert!(Job::install(OWNER, 1, "u.ppu", None, 3).is_ok());
}

#[test]
fn the_host_stopping_a_job_abandons_the_device_and_tells_reason_20() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    machine.start(install(OWNER, 1, 0), &mut fw);
    run(&mut machine, &mut fw);
    machine.answer(install_q(OWNER, 1), Answer::Yes, &mut fw);
    machine.step(&mut fw, &NoStorage, &Names, 100);
    let outcome = machine.abort(&mut fw).unwrap();
    assert_eq!(outcome.reason, 20);
    assert!(fw.log.contains(&Call::Abandon));
    assert!(!machine.busy());
    assert_eq!(machine.abort(&mut fw), None, "nothing open");
}

fn check_job(owner: u64, request: u32) -> Job {
    Job::check(owner, request, "update.ppu").unwrap()
}

#[test]
fn a_check_keeps_one_result_per_plugin_with_a_bit_per_built_in_it_has() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    machine.reserve_check(OWNER).unwrap();
    assert_eq!(
        machine.checked(OWNER).unwrap_err(),
        ERR_BUSY,
        "reserved: running"
    );
    assert_eq!(machine.start(check_job(OWNER, 5), &mut fw), None);
    assert_eq!(machine.state(), State::Idle, "a check is no install");
    let Event::Ended(outcome) = run(&mut machine, &mut fw) else {
        panic!("not ended");
    };
    assert_eq!((outcome.kind, outcome.reason), (KIND_CHECK, 0));
    let checked = machine.checked(OWNER).unwrap();
    assert_eq!((checked.request, checked.reason), (5, 0));
    assert_eq!(checked.facts().unwrap().size, 3 * WRITE_BLOCK);
    // The image has `stats`; `weather` and `clock` are left out.
    assert!(!checked.leaves_out(0) && checked.leaves_out(1) && checked.leaves_out(2));
    assert!(checked.leaves_out_some());
    assert!(!checked.leaves_out(3), "past the running built-ins");
    assert_eq!(machine.checked(OTHER).unwrap_err(), ERR_NOT_FOUND);
    assert_eq!(machine.leaves_out_of(OWNER).unwrap().request, 5);
}

#[test]
fn an_image_with_every_built_in_leaves_none_out() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    fw.built_ins = std::vec!["stats".into(), "weather".into(), "clock".into()];
    machine.reserve_check(OWNER).unwrap();
    machine.start(check_job(OWNER, 1), &mut fw);
    run(&mut machine, &mut fw);
    assert!(!machine.checked(OWNER).unwrap().leaves_out_some());
}

#[test]
fn a_check_that_fails_after_streaming_built_ins_keeps_no_bits() {
    let mut fw = fake();
    fw.check_steps = 3;
    // The first step names `stats`; the second fails.
    fw.fail = Some((Stage::Check, 1, Reason(18)));
    let mut machine = FirmwareInstall::new();
    machine.reserve_check(OWNER).unwrap();
    machine.start(check_job(OWNER, 1), &mut fw);
    let Event::Ended(outcome) = run(&mut machine, &mut fw) else {
        panic!("not ended");
    };
    assert_eq!(outcome.reason, 18);
    let checked = machine.checked(OWNER).unwrap();
    assert_eq!(checked.reason, 18);
    assert!(!checked.leaves_out_some() && !checked.leaves_out(1));
    assert!(checked.facts().is_none());
}

#[test]
fn a_foreign_image_has_no_list_of_built_ins() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    fw.facts.foreign = true;
    fw.facts.signer = Signer::Unsigned;
    machine.reserve_check(OWNER).unwrap();
    machine.start(Job::check(OWNER, 1, "o.bin").unwrap(), &mut fw);
    run(&mut machine, &mut fw);
    assert_eq!(machine.leaves_out_of(OWNER).unwrap_err(), ERR_UNSUPPORTED);
    assert!(machine.checked(OWNER).is_ok(), "op 6 still reads it");
}

#[test]
fn four_plugins_keep_a_result_a_fifth_is_full_and_a_repeat_replaces_its_own() {
    let mut machine = FirmwareInstall::new();
    for owner in 1..=4 {
        machine.reserve_check(owner).unwrap();
    }
    assert_eq!(machine.reserve_check(5), Err(ERR_FULL));
    // The next check of a plugin that has one takes its place, no more.
    machine.reserve_check(3).unwrap();
    machine.forget(2);
    machine.reserve_check(5).unwrap();
    assert_eq!(machine.checked(2).unwrap_err(), ERR_NOT_FOUND);
    machine.clear_results();
    assert_eq!(machine.checked(5).unwrap_err(), ERR_NOT_FOUND);
}

#[test]
fn a_failed_check_keeps_its_reason_and_a_check_does_not_disturb_a_waiting_image() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    installed(&mut machine, &mut fw, OWNER);
    fw.facts.chip_ok = false;
    machine.reserve_check(OTHER).unwrap();
    machine.start(check_job(OTHER, 9), &mut fw);
    let Event::Ended(outcome) = run(&mut machine, &mut fw) else {
        panic!("not ended");
    };
    assert_eq!(outcome.reason, 12);
    assert_eq!(machine.checked(OTHER).unwrap().reason, 12);
    assert_eq!(machine.state(), State::Installed);
    // The asker can still restart.
    assert!(machine.request_restart(OWNER, 9).is_ok());
}

#[test]
fn a_late_answer_to_an_aborted_jobs_question_never_starts_the_next_write() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    machine.start(install(OWNER, 1, 0), &mut fw);
    assert!(matches!(run(&mut machine, &mut fw), Event::Ask(_)));
    // The host stops the job; the next install of another plugin asks.
    assert_eq!(machine.abort(&mut fw).unwrap().reason, 20);
    machine.start(install(OTHER, 2, 0), &mut fw);
    assert!(matches!(run(&mut machine, &mut fw), Event::Ask(_)));
    // The first dialog's yes (its owner and request) is ignored ...
    for stale in [
        install_q(OWNER, 1),
        install_q(OTHER, 1),
        install_q(OWNER, 2),
    ] {
        assert_eq!(machine.answer(stale, Answer::Yes, &mut fw), Event::Quiet);
        assert_eq!(machine.state(), State::WaitingForUser, "still asking");
    }
    // ... as is an answer of the restart's kind to an install's question.
    assert_eq!(
        machine.answer(restart_q(OTHER, 2), Answer::Yes, &mut fw),
        Event::Quiet
    );
    assert!(
        !fw.log
            .iter()
            .any(|call| matches!(call, Call::BeginWrite { .. }))
    );
    // Only the open question's own answer is applied.
    assert_eq!(
        machine.answer(install_q(OTHER, 2), Answer::Yes, &mut fw),
        Event::Quiet
    );
    assert_eq!(machine.state(), State::Installing);
    assert!(
        fw.log
            .iter()
            .any(|call| matches!(call, Call::BeginWrite { .. }))
    );
}

#[test]
fn a_late_yes_never_restarts_and_a_refused_check_keeps_no_facts() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    installed(&mut machine, &mut fw, OWNER);
    machine.request_restart(OWNER, 9).unwrap();
    for stale in [
        restart_q(OWNER, 8),
        restart_q(OTHER, 9),
        install_q(OWNER, 9),
    ] {
        assert_eq!(machine.answer(stale, Answer::Yes, &mut fw), Event::Quiet);
    }
    assert!(!fw.log.contains(&Call::Restart));
    assert_eq!(
        machine.answer(restart_q(OWNER, 9), Answer::Yes, &mut fw),
        Event::Restart
    );
}

/// A name, the change to the device, the reason, whether facts are kept.
type Verdict = (&'static str, fn(&mut FakeFirmware), u8, bool);

#[test]
fn a_finished_check_keeps_its_facts_where_it_refuses_and_a_damaged_or_foreign_chip_keeps_none() {
    let cases: [Verdict; 6] = [
        ("unsigned", |f| f.facts.signer = Signer::Unsigned, 8, true),
        (
            "unknown key: op 6 hands the key out",
            |f| f.facts.signer = Signer::Other { key: [7; 32] },
            9,
            true,
        ),
        (
            "older",
            |f| f.facts = paperpaw_facts("0.4.0", "paperpaw-x4", 3 * WRITE_BLOCK),
            14,
            true,
        ),
        ("no room", |f| f.room = WRITE_BLOCK as u64, 13, true),
        ("other chip", |f| f.facts.chip_ok = false, 12, false),
        (
            "damaged",
            |f| f.facts = paperpaw_facts("not-a-version", "paperpaw-x4", WRITE_BLOCK),
            4,
            false,
        ),
    ];
    for (name, change, reason, kept) in cases {
        let mut fw = fake();
        change(&mut fw);
        let mut machine = FirmwareInstall::new();
        machine.reserve_check(OWNER).unwrap();
        machine.start(check_job(OWNER, 1), &mut fw);
        run(&mut machine, &mut fw);
        let checked = machine.checked(OWNER).unwrap();
        assert_eq!(checked.reason, reason, "{name}");
        assert_eq!(checked.facts().is_some(), kept, "{name}");
        if let Some(facts) = checked.facts() {
            // What op 6 reads: the image's key, version and size.
            assert_eq!(facts.size, fw.facts.size, "{name}");
            assert_eq!(facts.signer, fw.facts.signer, "{name}");
            assert!(checked.leaves_out_some(), "{name}: bits kept");
        } else {
            assert!(!checked.leaves_out_some(), "{name}: no bits");
        }
    }
}

#[test]
fn another_plugins_request_number_is_not_told_and_a_check_is_no_install_request() {
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    machine.start(install(OWNER, 3, 0), &mut fw);
    assert!(matches!(run(&mut machine, &mut fw), Event::Ask(_)));
    assert_eq!(machine.last(OWNER), (3, 0));
    assert_eq!(machine.last(OTHER), (0, 0), "asking");
    machine.answer(install_q(OWNER, 3), Answer::Yes, &mut fw);
    assert!(matches!(run(&mut machine, &mut fw), Event::Ended(_)));
    assert_eq!(machine.last(OWNER), (3, 0), "waiting for its restart");
    assert_eq!(machine.last(OTHER), (0, 0), "waiting");
    // Another plugin's check that runs is neither's request.
    machine.reserve_check(OTHER).unwrap();
    machine.start(check_job(OTHER, 9), &mut fw);
    assert_eq!(machine.last(OWNER), (3, 0));
    assert_eq!(machine.last(OTHER), (0, 0), "its own check is no install");
    run(&mut machine, &mut fw);
    // A failed install is the failing plugin's alone.
    let mut machine = FirmwareInstall::new();
    let mut fw = fake();
    fw.facts.signer = Signer::Unsigned;
    machine.start(install(OWNER, 4, 0), &mut fw);
    run(&mut machine, &mut fw);
    assert_eq!(machine.last(OWNER).0, 4);
    assert_eq!(machine.last(OTHER), (0, 0), "failed");
}
