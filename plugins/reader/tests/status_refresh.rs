//! The status lines while pages are turned: what changes on them a
//! moment after a press waits for the next page, or until no press came
//! for a while, instead of a refresh of its own (on the X4 such a
//! refresh holds up the next press half a second). And the menu bar
//! opened before the pages are counted: told the pages once they are
//! (#117).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_platform::{Battery, SystemStatus};
use paperpaw_sdk::Instant;
use paperpaw_sdk::builtin::{actions as core_actions, params};

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// The battery alone at the bottom right.
const CONFIG: &str = "[reader]\nstatus.bottom-left =\nstatus.bottom-center =\nstatus.bottom-right = core.status.battery\n";

fn battery(percent: u8) -> SystemStatus {
    SystemStatus::new(None, Some(Battery::new(percent, false, None)))
}

#[test]
#[ignore = "needs native activation (format removal)"]
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

/// The menu opened before the pages are counted shows the percentage; the
/// count goes on below it (only overlays cover the page) and the bar is
/// told the page and the pages, without closing it (#117).
#[test]
#[ignore = "needs native activation (format removal)"]
fn the_menu_bar_learns_the_pages_counted_below_it() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/alice.epub", ALICE, "");
    reading.open_unsettled();
    reading.run(core_actions::MENU);
    assert_eq!(reading.screen(), "recorder.menu");
    let opened = reading.last_slot("menu").unwrap();
    let shown = opened.get(params::PAGE).unwrap();
    assert!(shown.ends_with('%'), "{shown}");
    // `run` let the time pass: the count ended below the menu.
    let told = reading.last_event("menu-page").expect("the bar was told");
    assert_eq!(told.get(params::MENU), Some("core.reader-menu"));
    let page = told.get(params::PAGE).unwrap();
    let (at, of) = page.split_once(" / ").expect(page);
    assert!(
        at.parse::<u32>().unwrap() >= 1 && of.parse::<u32>().unwrap() > 50,
        "{page}"
    );
    assert_eq!(reading.screen(), "recorder.menu", "still open");
    // Once, where the count ended.
    let times = reading
        .events()
        .iter()
        .filter(|e| **e == "menu-page")
        .count();
    assert_eq!(times, 1);
}
