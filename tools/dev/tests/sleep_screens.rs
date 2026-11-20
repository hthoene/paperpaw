//! The sleep screens of the plugins that offer one (#421) drawn on the emulated
//! X4 from the data in each plugin's `tests/snapshots/data` and compared with
//! the pictures there. A plugin without its screen fails: the case cannot open
//! it. After a change meant to alter what is drawn, write the pictures anew
//! (`paperpaw-dev snapshots --plugin <id> --snapshots plugins/<id>/tests/snapshots
//! --bless`) and look at each before committing it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::Path;

fn check(plugin: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(plugin)
        .join("tests/snapshots");
    if let Err(problems) = paperpaw_dev::snapshot::check_dir(|app| app, &dir) {
        panic!("{plugin}: {problems}");
    }
}

#[test]
fn calendar() {
    check("calendar");
}

#[test]
fn tasks() {
    check("tasks");
}

#[test]
fn stats() {
    check("stats");
}

#[test]
fn reading_goal() {
    check("reading-goal");
}

#[test]
fn notes_overview() {
    check("notes-overview");
}

#[test]
fn news() {
    check("news");
}

#[test]
fn smart_home() {
    check("smart-home");
}
