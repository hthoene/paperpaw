//! The status lines while pages are turned: what changes on them a
//! moment after a press waits for the next page, or until no press came
//! for a while, instead of a refresh of its own (on the X4 such a
//! refresh holds up the next press half a second).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_platform::{Battery, SystemStatus};
use paperpaw_sdk::Instant;
use paperpaw_sdk::builtin::actions as core_actions;

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// The battery alone at the bottom right.
const CONFIG: &str = "[reader]\nstatus.bottom-left =\nstatus.bottom-center =\nstatus.bottom-right = core.status.battery\n";

fn battery(percent: u8) -> SystemStatus {
    SystemStatus::new(None, Some(Battery::new(percent, false, None)))
}

#[test]
fn the_status_lines_wait_while_pages_are_turned() {
    let mut reading = Reading::new(
        BUTTONS,
        Shared::default(),
        "books/alice.epub",
        ALICE,
        CONFIG,
    );
    reading.kernel.set_status(Instant(reading.now), battery(80));
    reading.open();
    reading.kernel.render(&mut |_, _| {});
    reading.now += 6000;
    let turned = reading.now;
    let mut effects = reading
        .kernel
        .run_action(Instant(turned), core_actions::NEXT_PAGE);
    if effects.proceed() {
        effects = reading.kernel.proceed(Instant(turned));
    }
    assert!(effects.refresh().is_some(), "the page turned");
    reading.kernel.render(&mut |_, _| {});
    // The battery changes a moment after the press: not shown on its own
    // while the next press may come...
    let effects = reading
        .kernel
        .set_status(Instant(turned + 300), battery(79));
    assert!(effects.refresh().is_none(), "{:?}", effects.refresh());
    let mut at = turned + 300;
    while at < turned + 1_900 {
        at += 100;
        let effects = reading.kernel.tick(Instant(at));
        assert!(
            effects.refresh().is_none(),
            "{} ms after the press",
            at - turned
        );
    }
    // ...but once none came for a while.
    let effects = reading.kernel.tick(Instant(turned + 2_000));
    let (area, _) = effects.refresh().expect("the status line refreshed");
    assert!(area.height < 100, "{area:?}");
}
