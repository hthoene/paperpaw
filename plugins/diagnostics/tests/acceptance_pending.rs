//! What the interface-6 tests (`screen.rs`, `pages.rs`, `nav.rs`, `check.rs`)
//! proved with a whole kernel and the plugin in it, drawing real pixels. The
//! layout of the test screen and the steps' decisions are tested without a host
//! (`src/test_screen/tests.rs`, `src/steps.rs`); these need the host to run a
//! native plugin end to end, and stay ignored until it does. Each names what is missing.

#[test]
#[ignore = "needs a host that runs native screens and replays their draw ops (Core H1), with the plugin offered as the home screen"]
fn the_test_screen_draws_the_ramp_the_boxes_and_the_progress_bar() {
    unreachable!("pixel golden of diagnostics.screen on 480x800 Gray4, en and de");
}

#[test]
#[ignore = "needs key bindings admitted by the host (record key.bind is refused today)"]
fn a_held_select_resets_the_test_screen() {
    unreachable!("long.confirm on diagnostics.screen runs diagnostics.reset");
}

#[test]
#[ignore = "needs core.device ops 1, 2, 7, 8, 9, 15 on the host (PR #311) and Tick delivery to a plugin with a screen"]
fn the_check_walks_every_step_and_writes_its_report() {
    unreachable!(
        "answers, suggested focus, light levels restored, card speed, Wi-Fi scan, sleep and wake, memory"
    );
}

#[test]
#[ignore = "needs DEV commands with uses.power and uses.wifi on the host (PR #312 merged; native dispatch pending)"]
fn sleep_now_marks_asleep_and_the_wake_passes_the_step() {
    unreachable!(
        "command.sleep, suspend, resume, report line `sleep: asleep` then `sleep: pass (woke)`"
    );
}

#[test]
#[ignore = "needs the host to deliver raw key records with repeat to a screen on top"]
fn every_button_counts_as_the_one_asked_for_and_a_held_other_gives_up() {
    unreachable!("Input::Key press, repeat and release on the buttons page");
}
