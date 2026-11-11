//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! pack is pinned by `tests/drawing.rs` on a kernel; each behaviour needs the
//! same run through the production loader with the native plugin. These cases
//! never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 12] = [
    // The day widget on the home screen in each style, with the frame and
    // look settings applied, at the sizes a cell takes.
    Case {
        name: "widgets-day-draws-in-each-style",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // A widget whose content changes at the minute tells its maker.
    Case {
        name: "widgets-live-minute-update",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // With no time zone chosen the times follow the shell's UTC offset (the
    // snapshot's `utc_offset_minutes`, draft 25); until then they read as UTC.
    Case {
        name: "widgets-system-utc-offset",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // The greeting by the hour, its sun or moon, the name, the date line.
    Case {
        name: "widgets-greeting-draws-by-time-of-day",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // A tap on the note asks for its text on the keyboard, the answer is kept
    // as the setting (the request and its text are owner resources, H2).
    Case {
        name: "widgets-note-edits-on-tap",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // The logo asks the host for its picture (the picture set, else PaperPaw's
    // own from the install folder), draws it beside the name and asks again
    // when `core.logo` changes: it needs the host's image service (H1, H3).
    Case {
        name: "widgets-logo-asks-for-and-draws-its-picture",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    Case {
        name: "widgets-quote-turns-on-tap",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    Case {
        name: "widgets-todo-ticks-on-tap",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // The battery lying, standing and as a ring, a low charge shaded, the bolt
    // while charging, the caption.
    Case {
        name: "widgets-battery-draws-each-style",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    Case {
        name: "widgets-todo-screen-adds-ticks-and-moves",
        golden: None,
        blocked_by: &["H1", "H2", "H4"],
    },
    // The library in numbers: each number above its name, the bar of the share
    // read, the numbers beside their names where they do not fit above, the
    // tap that opens the library.
    Case {
        name: "widgets-library-draws-its-numbers",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
    // The highlight: the text in quotes with a large mark, the note, the book
    // below, shortened where it does not fit; a tap opens the book at it.
    Case {
        name: "widgets-highlight-draws-and-opens-the-book",
        golden: None,
        blocked_by: &["H1", "H3", "H4"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED H1/"), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED H1/H3: native screen commands and composition are not qualified"]
fn widgets_day_draws_in_each_style() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: native screen commands and composition are not qualified"]
fn widgets_live_minute_update() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3 and draft 25: the snapshot has no UTC offset yet, so an unset zone reads as UTC"]
fn widgets_system_utc_offset() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: native screen commands and composition are not qualified"]
fn widgets_greeting_draws_by_time_of_day() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and widget input are not qualified"]
fn widgets_note_edits_on_tap() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and widget input are not qualified"]
fn widgets_quote_turns_on_tap() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and widget input are not qualified"]
fn widgets_todo_ticks_on_tap() {
    CASES[6].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: native screen commands and composition are not qualified"]
fn widgets_battery_draws_each_style() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screen commands, dialogs and input are not qualified"]
fn widgets_todo_screen_adds_ticks_and_moves() {
    CASES[8].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and widget input are not qualified"]
fn widgets_library_draws_its_numbers() {
    CASES[9].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3/H4: native screen commands, composition and widget input are not qualified"]
fn widgets_highlight_draws_and_opens_the_book() {
    CASES[10].refuse().unwrap();
}
