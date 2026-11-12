//! The firmware request table, queue and pump against the scripted device.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::firmware_install::{Answer, FirmwareInstall};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_STATE};
use paperpaw_platform::{
    NoStorage,
    firmware::{Reason, WRITE_BLOCK},
    firmware_fake::{FakeFirmware, Stage, paperpaw_facts},
};

struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        1
    }
    fn index_of(&self, _: &str) -> Option<usize> {
        None
    }
}

fn driver() -> Driver {
    driver_sized(2)
}

/// A driver over an image of `blocks` write blocks.
fn driver_sized(blocks: u32) -> Driver {
    let fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", blocks * WRITE_BLOCK),
    );
    Driver::new(
        Box::new(fake),
        crate::native_host::firmware_cell::test_shared(RefCell::new(FirmwareInstall::new())),
    )
}

/// Pumps until something other than a quiet step or progress comes.
fn run(driver: &mut Driver) -> Event {
    for now in (100..20_000).step_by(100) {
        match driver.pump(&NoStorage, &Names, now) {
            Event::Quiet | Event::Progress { .. } => {}
            other => return other,
        }
    }
    panic!("never ended");
}

#[test]
fn a_request_is_accepted_whole_or_refused_with_nothing_changed() {
    let mut d = driver();
    // Request 0, a flag beyond bits 0 and 1, and a request that waits.
    assert_eq!(d.submit_install(1, 0, "a.ppu", None, 0), Err(ERR_INVALID));
    assert_eq!(d.submit_install(1, 1, "a.ppu", None, 4), Err(ERR_INVALID));
    assert_eq!(d.waiting(), 0);
    d.submit_install(1, 1, "a.ppu", None, 0).unwrap();
    assert_eq!(d.submit_install(1, 1, "b.ppu", None, 0), Err(ERR_INVALID));
    assert_eq!(d.waiting(), 1);
    assert!(d.holds(1, 1));
}

#[test]
fn four_requests_wait_device_wide_and_the_fifth_is_full() {
    let mut d = driver();
    d.submit_install(1, 1, "a.ppu", None, 0).unwrap();
    d.submit_check(2, 1, "b.ppu").unwrap();
    d.submit_check(3, 1, "c.ppu").unwrap();
    d.submit_install(4, 1, "d.ppu", None, 0).unwrap();
    assert_eq!(d.submit_install(5, 1, "e.ppu", None, 0), Err(ERR_FULL));
    assert_eq!(d.submit_check(5, 2, "e.ppu"), Err(ERR_FULL));
    assert_eq!(d.waiting(), 4);
}

#[test]
fn a_second_firmware_request_waits_in_the_queue_and_runs_after_the_first() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    d.submit_check(2, 1, "b.ppu").unwrap();
    // The first check ends; the second one has not started.
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 1));
    assert!(d.holds(2, 1));
    assert!(matches!(run(&mut d), Event::Ended(o) if o.owner == 2));
    assert_eq!(d.waiting(), 0);
    assert_eq!(d.take_ready().unwrap().owner, 1);
    assert_eq!(d.take_ready().unwrap().owner, 2);
    assert!(d.take_ready().is_none());
}

#[test]
fn an_outcome_is_kept_for_op_13_once_delivered_until_the_next_request_of_its_kind() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    let Event::Ended(outcome) = run(&mut d) else {
        panic!("a check ends");
    };
    assert_eq!(
        d.kept(1, KIND_CHECK),
        None,
        "not before the edge reached it"
    );
    assert_eq!(d.take_ready(), Some(outcome));
    d.delivered(outcome);
    assert_eq!(d.kept(1, KIND_CHECK), Some(outcome));
    // Another kind is another slot; the next check replaces this one.
    assert_eq!(d.kept(1, KIND_FIRMWARE), None);
    d.submit_check(1, 2, "a.ppu").unwrap();
    assert_eq!(d.kept(1, KIND_CHECK), None);
}

#[test]
fn an_install_asks_then_a_yes_writes_and_state_three_waits_for_its_asker() {
    let mut d = driver();
    d.submit_install(1, 5, "a.ppu", None, 0).unwrap();
    let Event::Ask(ask) = run(&mut d) else {
        panic!("always asked");
    };
    assert_eq!((ask.owner, ask.request), (1, 5));
    d.answer(ask.question(), Answer::Yes);
    assert!(matches!(run(&mut d), Event::Ended(o) if o.reason == 0 && o.kind == KIND_FIRMWARE));
    // Only the plugin whose install waits may restart; the host asks again.
    assert_eq!(d.request_restart(2).err(), Some(ERR_DENIED));
    let ask = d.request_restart(1).unwrap();
    assert!(ask.restart);
    let mut idle = driver();
    assert_eq!(idle.request_restart(1).err(), Some(ERR_STATE));
}

#[test]
fn a_no_ends_the_job_with_reason_two_and_the_slot_never_starts() {
    let mut d = driver();
    d.submit_install(1, 5, "a.ppu", None, 0).unwrap();
    let Event::Ask(ask) = run(&mut d) else {
        panic!("always asked");
    };
    let Event::Ended(outcome) = d.answer(ask.question(), Answer::No) else {
        panic!("a no ends it");
    };
    assert_eq!(outcome.reason, 2);
    assert_eq!(d.waiting(), 0);
}

#[test]
fn a_stop_ends_the_running_and_every_waiting_job_with_reason_twenty() {
    let mut d = driver();
    d.submit_install(1, 1, "a.ppu", None, 0).unwrap();
    d.submit_check(2, 1, "b.ppu").unwrap();
    // The first job runs (pass 1) before the stop.
    assert_eq!(d.pump(&NoStorage, &Names, 100), Event::Quiet);
    d.abort_all();
    assert_eq!(d.waiting(), 0);
    let first = d.take_ready().unwrap();
    let second = d.take_ready().unwrap();
    assert_eq!((first.owner, first.reason), (1, 20));
    assert_eq!(
        (second.owner, second.kind, second.reason),
        (2, KIND_CHECK, 20)
    );
}

#[test]
fn a_sender_that_stopped_leaves_no_request_outcome_or_result() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    d.submit_check(2, 1, "b.ppu").unwrap();
    assert!(matches!(run(&mut d), Event::Ended(_)));
    d.sender_stopped(1);
    d.sender_stopped(2);
    assert_eq!(d.waiting(), 0);
    assert!(d.take_ready().is_none());
    assert_eq!(d.kept(1, KIND_CHECK), None);
    // Their places are free again.
    for owner in 1..=4 {
        d.submit_check(owner, 1, "a.ppu").unwrap();
    }
}

#[test]
fn a_failed_start_ends_the_request_at_once_with_the_devices_reason() {
    let mut fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", WRITE_BLOCK),
    );
    fake.fail = Some((Stage::BeginCheck, 0, Reason::OTHER));
    let mut d = Driver::new(
        Box::new(fake),
        crate::native_host::firmware_cell::test_shared(RefCell::new(FirmwareInstall::new())),
    );
    d.submit_install(1, 1, "a.ppu", None, 0).unwrap();
    let event = d.pump(&NoStorage, &Names, 100);
    assert!(matches!(event, Event::Ended(o) if o.reason == Reason::OTHER.0));
    assert_eq!(d.waiting(), 0);
}

/// Runs plugin 1's install to state 3 (request `request`).
fn installed(d: &mut Driver, request: u32) {
    d.submit_install(1, request, "a.ppu", None, 0).unwrap();
    let Event::Ask(ask) = run(d) else {
        panic!("always asked");
    };
    d.answer(ask.question(), Answer::Yes);
    assert!(matches!(run(d), Event::Ended(o) if o.reason == 0));
    // Its outcome is told and kept.
    let outcome = d.take_ready().unwrap();
    d.delivered(outcome);
}

#[test]
fn a_restart_is_a_request_with_room_of_its_own_and_its_outcome_carries_the_number() {
    let mut d = driver();
    installed(&mut d, 5);
    // The restart takes a place in the table and room for its outcome.
    let ask = d.request_restart(1).unwrap();
    assert_eq!(d.waiting(), 1);
    assert!(d.holds(1, 5));
    // Three more fill the table; a fifth would be full.
    d.submit_check(2, 1, "b.ppu").unwrap();
    d.submit_check(3, 1, "c.ppu").unwrap();
    d.submit_check(4, 1, "d.ppu").unwrap();
    assert_eq!(d.submit_check(5, 1, "e.ppu"), Err(ERR_FULL));
    // "No" to the restart and a stop in the same pass: every sender is told
    // exactly once, none is lost, the restart's outcome has its number.
    let Event::Ended(declined) = d.answer(ask.question(), Answer::No) else {
        panic!("a no ends it");
    };
    assert_eq!((declined.request, declined.reason), (5, 2));
    d.abort_all();
    let mut told = Vec::new();
    while let Some(outcome) = d.take_ready() {
        told.push((outcome.owner, outcome.request, outcome.reason));
    }
    told.sort_unstable();
    assert_eq!(told, [(1, 5, 2), (2, 1, 20), (3, 1, 20), (4, 1, 20)]);
    assert_eq!(d.lost, 0);
}

#[test]
fn a_restart_waiting_for_its_answer_ends_with_reason_twenty_on_a_stop() {
    let mut d = driver();
    installed(&mut d, 5);
    d.request_restart(1).unwrap();
    d.abort_all();
    let outcome = d.take_ready().unwrap();
    assert_eq!((outcome.owner, outcome.request, outcome.reason), (1, 5, 20));
    assert_eq!(d.waiting(), 0);
}

#[test]
fn an_outcome_only_fills_the_slot_of_its_own_request() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    let Event::Ended(first) = run(&mut d) else {
        panic!("a check ends");
    };
    // The plugin asks again from the callback that is told of the first one:
    // the late `delivered` of the first does not fill the second's slot.
    d.submit_check(1, 2, "a.ppu").unwrap();
    let first_told = d.take_ready().unwrap();
    assert_eq!(first_told, first);
    d.delivered(first_told);
    assert_eq!(d.kept(1, KIND_CHECK), None);
    let Event::Ended(second) = run(&mut d) else {
        panic!("a check ends");
    };
    d.take_ready();
    d.delivered(second);
    assert_eq!(d.kept(1, KIND_CHECK).map(|o| o.request), Some(2));
}

#[test]
fn slots_whose_outcome_never_came_are_evicted_for_new_plugins() {
    let mut d = driver();
    // Eight plugins each leave a slot with no outcome (their requests were
    // taken back); a ninth is not refused for them.
    for owner in 1..=8 {
        d.submit_install(owner, 1, "a.ppu", None, 0).unwrap();
        d.cancel(owner, 1);
    }
    assert_eq!(d.waiting(), 0);
    d.submit_install(9, 1, "a.ppu", None, 0).unwrap();
    assert!(d.holds(9, 1));
}

#[test]
fn a_cancelled_check_gives_its_result_place_back() {
    let mut d = driver();
    for owner in 1..=4 {
        d.submit_check(owner, 1, "a.ppu").unwrap();
        d.cancel(owner, 1);
    }
    // Four cancelled checks do not keep the four result places.
    d.submit_check(5, 1, "a.ppu").unwrap();
}

#[test]
fn a_check_stopped_before_it_ran_leaves_a_result_not_a_busy_slot() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    d.abort_all();
    let machine = d.machine().unwrap();
    let checked = machine.borrow().checked(1).unwrap();
    assert_eq!((checked.request, checked.reason), (1, 20));
}

#[test]
fn one_plugin_holds_at_most_two_places_so_others_keep_theirs() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    d.submit_check(1, 2, "b.ppu").unwrap();
    assert_eq!(d.submit_check(1, 3, "c.ppu"), Err(ERR_FULL));
    assert_eq!(d.submit_install(1, 4, "c.ppu", None, 0), Err(ERR_FULL));
    // Two places are left device-wide for other plugins.
    d.submit_check(2, 1, "d.ppu").unwrap();
    d.submit_check(3, 1, "e.ppu").unwrap();
    assert_eq!(d.waiting(), 4);
}

#[test]
fn a_stopped_owner_keeps_its_waiting_image_and_may_restart_it_later() {
    let mut d = driver();
    installed(&mut d, 5);
    d.sender_stopped(1);
    // The image is a fact about the device: another plugin may not restart
    // it, and its installs end with reason 21 until a reboot.
    assert_eq!(d.request_restart(2).err(), Some(ERR_DENIED));
    d.submit_install(2, 1, "b.ppu", None, 0).unwrap();
    assert!(matches!(run(&mut d), Event::Ended(o) if o.reason == 21));
    // The owner starts again (same plugin id): its restart is accepted.
    assert!(d.request_restart(1).is_ok());
}

#[test]
fn a_request_counts_until_its_outcome_is_told() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    d.submit_check(1, 2, "b.ppu").unwrap();
    assert!(matches!(run(&mut d), Event::Ended(_)));
    // Ended but not told: still two open requests.
    assert_eq!(d.submit_check(1, 3, "c.ppu"), Err(ERR_FULL));
    let outcome = d.take_ready().unwrap();
    d.delivered(outcome);
    d.submit_check(1, 3, "c.ppu").unwrap();
}

#[test]
fn an_answer_that_finds_the_machine_borrowed_is_counted() {
    let mut d = driver();
    installed(&mut d, 5);
    let ask = d.request_restart(1).unwrap();
    let machine = d.machine().unwrap();
    let held = machine.borrow_mut();
    assert_eq!(d.answer(ask.question(), Answer::Yes), Event::Quiet);
    drop(held);
    assert_eq!(d.skipped(), 1);
}

#[test]
fn cancelling_one_check_keeps_the_result_place_of_the_others() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    d.submit_check(1, 2, "b.ppu").unwrap();
    d.cancel(1, 2);
    d.abort_all();
    // The first check still has its place: it left a result, not `NOT_FOUND`.
    let machine = d.machine().unwrap();
    let checked = machine.borrow().checked(1).unwrap();
    assert_eq!((checked.request, checked.reason), (1, 20));
}

#[test]
fn a_restart_asked_inside_its_install_done_keeps_its_own_slot() {
    let mut d = driver();
    d.submit_install(1, 5, "a.ppu", None, 0).unwrap();
    let Event::Ask(ask) = run(&mut d) else {
        panic!("always asked");
    };
    d.answer(ask.question(), Answer::Yes);
    assert!(matches!(run(&mut d), Event::Ended(o) if o.reason == 0));
    let outcome = d.take_ready().unwrap();
    // The plugin asks for the restart (same number) from the callback that
    // was told, before the host calls `delivered`.
    d.request_restart(1).unwrap();
    d.delivered(outcome);
    assert_eq!(d.kept(1, KIND_FIRMWARE), None);
}

#[test]
fn a_stop_with_the_machine_borrowed_tells_nobody_twice() {
    let mut d = driver();
    d.submit_install(1, 1, "a.ppu", None, 0).unwrap();
    d.submit_check(2, 1, "b.ppu").unwrap();
    assert_eq!(d.pump(&NoStorage, &Names, 100), Event::Quiet);
    let machine = d.machine().unwrap();
    let held = machine.borrow_mut();
    d.abort_all();
    drop(held);
    // The queued check ended; the running install did not: it ends once, when
    // its job does, not now.
    let told: Vec<_> = core::iter::from_fn(|| d.take_ready())
        .map(|o| (o.owner, o.reason))
        .collect();
    assert_eq!(told, [(2, 20)]);
    assert!(d.holds(1, 1));
}

#[test]
fn many_steps_between_two_looks_leave_one_changed_mark() {
    let mut d = driver_sized(16);
    d.submit_install(1, 1, "a.ppu", None, 0).unwrap();
    let Event::Ask(ask) = run(&mut d) else {
        panic!("always asked");
    };
    d.answer(ask.question(), Answer::Yes);
    assert_eq!(d.peek_changed(), None);
    // Steps (far apart in time, so each one reports progress) mark the one
    // request; nothing queues however many pass before anyone looks.
    let mut marks = 0;
    for now in (100..20_000).step_by(100) {
        if matches!(d.pump(&NoStorage, &Names, now), Event::Progress { .. }) {
            marks += 1;
        }
        if d.peek_changed().is_some() && marks >= 2 {
            break;
        }
    }
    assert!(marks >= 2, "progress came more than once");
    assert_eq!(d.peek_changed(), Some((1, 1)));
    d.clear_changed(1, 1);
    assert_eq!(d.peek_changed(), None);
}

#[test]
fn op_13_state_is_waiting_until_yes_then_running_then_ended() {
    let mut d = driver();
    d.submit_install(1, 1, "a.ppu", None, 0).unwrap();
    d.submit_check(2, 1, "b.ppu").unwrap();
    // Queued.
    assert_eq!(d.lookup(2, 1).unwrap().state, 0);
    let Event::Ask(ask) = run(&mut d) else {
        panic!("always asked");
    };
    // Its confirmation shows: still waiting.
    assert_eq!(d.lookup(1, 1).unwrap().state, 0);
    d.answer(ask.question(), Answer::Yes);
    assert_eq!(d.lookup(1, 1).unwrap().state, 1);
    assert!(matches!(run(&mut d), Event::Ended(_)));
    let ended = d.lookup(1, 1).unwrap();
    assert_eq!((ended.state, ended.outcome.is_some()), (2, true));
    // Kept after its edge is told, too.
    let outcome = d.take_ready().unwrap();
    d.delivered(outcome);
    assert_eq!(d.lookup(1, 1).unwrap().state, 2);
}

#[test]
fn a_request_waiting_for_the_user_is_no_work_and_a_held_back_outcome_waits_for_its_time() {
    let mut d = driver();
    assert!(!d.has_work(0));
    d.submit_install(1, 1, "a.ppu", None, 0).unwrap();
    // Queued: the head can start.
    assert!(d.has_work(0));
    let Event::Ask(ask) = run(&mut d) else {
        panic!("always asked");
    };
    // The confirmation shows (up to ten minutes): the device may idle.
    assert!(!d.has_work(1_000));
    assert_eq!(d.due(), None);
    d.answer(ask.question(), Answer::Yes);
    assert!(d.has_work(1_000), "a write step can be taken");
    assert!(matches!(run(&mut d), Event::Ended(_)));
    // Ended, to be told; its plugin cannot be called now: held back until 5000.
    assert!(d.has_work(0));
    d.hold_back(5_000);
    assert!(!d.has_work(2_000));
    assert_eq!(d.due(), Some(5_000));
    assert!(d.has_work(5_000));
    d.released();
    assert_eq!(d.due(), None);
}

#[test]
fn only_the_outcome_that_was_told_is_taken() {
    let mut d = driver();
    d.submit_check(1, 1, "a.ppu").unwrap();
    d.submit_check(2, 1, "b.ppu").unwrap();
    assert!(matches!(run(&mut d), Event::Ended(_)));
    assert!(matches!(run(&mut d), Event::Ended(_)));
    let first = d.peek_ready().unwrap();
    // The ready list changed during the callback (the plugin stopped).
    d.sender_stopped(first.owner);
    assert_eq!(
        d.take_ready_if(first.owner, first.request, first.kind),
        None
    );
    // The other outcome is not marked delivered in its place.
    assert_eq!(d.peek_ready().unwrap().owner, 2);
}

#[test]
fn a_stop_clears_a_restart_that_waits_for_the_kernel() {
    let mut d = driver();
    installed(&mut d, 5);
    let ask = d.request_restart(1).unwrap();
    assert!(matches!(
        d.answer(ask.question(), Answer::Yes),
        Event::Restart
    ));
    assert!(d.restart_wanted());
    d.abort_all();
    assert!(!d.restart_wanted(), "sleep between the passes: no restart");
}
