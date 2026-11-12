//! The picker of what goes over the page (`sleep.pick-overlay`), its model
//! without the host: the rows in interface 6's order (the pictures folder, the
//! presets, the widgets), the chosen one marked, what a choice sets, a picker
//! with nothing but the folder, and why the last sleep showed no picture.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;

use paperpaw_plugin_sleep::native_pick::{
    Choice, FAILED_MAX, Failure, KIND, Now, PICTURE, Rows, Source, WIDGET, failed, kept,
};
use paperpaw_sdk::native::image::ImageStatus;
use paperpaw_ui_kit::items::Item;

#[global_allocator]
static ALLOCATOR: fail_alloc::Failing = fail_alloc::Failing;

const ON: &str = "On";

fn now<'a>(picture: &'a str, widget: &'a str) -> Now<'a> {
    Now { picture, widget }
}

/// The picker of interface 6's example: two presets (the second applied) and
/// two widgets.
fn picker(now: Now<'_>, owl_applied: bool) -> Rows {
    let mut rows = Rows::new("From the pictures folder", now, ON).unwrap();
    rows.preset("cats.overlay.cat", "A cat", false).unwrap();
    rows.preset("owls.overlay.owl", "An owl", owl_applied)
        .unwrap();
    rows.widget("home.clock", "Widget: Clock").unwrap();
    rows.widget("stats.widget.today", "Widget: Today").unwrap();
    rows
}

/// Each row's title and what it says at its right.
fn shown(rows: &mut Rows) -> Vec<(String, String)> {
    rows.take_items()
        .into_iter()
        .map(|item| match item {
            Item::Row(row) => (row.title, row.value),
            Item::Heading(text) => (text, "heading".to_string()),
        })
        .collect()
}

fn marked(rows: &mut Rows) -> Vec<bool> {
    shown(rows)
        .into_iter()
        .map(|(_, value)| value == ON)
        .collect()
}

#[test]
fn the_settings_are_the_two_interface_6_picked() {
    assert_eq!(PICTURE, "sleep.overlay-picture");
    assert_eq!(WIDGET, "sleep.overlay-widget");
    assert_eq!(KIND, "sleep.overlay");
}

#[test]
fn the_rows_are_the_folder_the_presets_and_the_widgets_in_order() {
    let mut rows = picker(now("", ""), false);
    assert_eq!(
        rows.sources(),
        [
            Source::Folder,
            Source::Preset {
                id: "cats.overlay.cat".to_string(),
                applied: false
            },
            Source::Preset {
                id: "owls.overlay.owl".to_string(),
                applied: false
            },
            Source::Widget("home.clock".to_string()),
            Source::Widget("stats.widget.today".to_string()),
        ]
    );
    let titles: Vec<String> = shown(&mut rows).into_iter().map(|(t, _)| t).collect();
    assert_eq!(
        titles,
        [
            "From the pictures folder",
            "A cat",
            "An owl",
            "Widget: Clock",
            "Widget: Today"
        ]
    );
}

#[test]
fn the_chosen_row_says_on() {
    // Nothing set: the folder.
    assert_eq!(
        marked(&mut picker(now("", ""), false)),
        [true, false, false, false, false]
    );
    // A picture set that is a preset's: that preset, not the folder.
    assert_eq!(
        marked(&mut picker(now("plugins/owls/owl.png", ""), true)),
        [false, false, true, false, false]
    );
    // A picture set that no preset gives: no row.
    assert_eq!(
        marked(&mut picker(now("my/own.png", ""), false)),
        [false; 5]
    );
    // A widget set: only it, whatever the picture and the presets say.
    assert_eq!(
        marked(&mut picker(
            now("plugins/owls/owl.png", " home.clock "),
            true
        )),
        [false, false, false, true, false]
    );
    // A widget that is gone: no row.
    assert_eq!(
        marked(&mut picker(now("", "gone.widget"), false)),
        [false; 5]
    );
}

#[test]
fn choosing_sets_the_widget_then_the_picture_or_the_preset() {
    let rows = picker(now("a.png", "home.clock"), false);
    assert_eq!(
        rows.choice(0),
        Some(Choice {
            widget: "",
            picture: Some(""),
            preset: None
        })
    );
    assert_eq!(
        rows.choice(2),
        Some(Choice {
            widget: "",
            picture: None,
            preset: Some("owls.overlay.owl")
        })
    );
    assert_eq!(
        rows.choice(4),
        Some(Choice {
            widget: "stats.widget.today",
            picture: None,
            preset: None
        })
    );
    // A row there is not chooses nothing.
    assert_eq!(rows.choice(5), None);
    assert_eq!(rows.choice(usize::MAX), None);
}

#[test]
fn with_no_presets_and_no_widgets_only_the_folder_is_there() {
    let mut rows = Rows::new("From the pictures folder", now("", ""), ON).unwrap();
    assert_eq!(rows.sources(), [Source::Folder]);
    assert_eq!(
        shown(&mut rows),
        [("From the pictures folder".to_string(), ON.to_string())]
    );
    assert!(rows.choice(0).is_some());
    assert_eq!(rows.choice(1), None);
    // The items are taken once; the rows still choose.
    assert!(rows.take_items().is_empty());
    assert!(rows.choice(0).is_some());
}

#[test]
fn an_empty_folder_and_a_widget_that_is_gone_say_why() {
    assert_eq!(Failure::EmptyFolder.key(), "sleep.overlay.empty-folder");
    assert_eq!(Failure::EmptyFolder.args(), (None, None));
    let gone = Failure::NoWidget("home.clock".to_string());
    assert_eq!(gone.key(), "sleep.overlay.no-widget");
    assert_eq!(gone.args(), (Some("home.clock"), None));
}

#[test]
fn a_picture_that_failed_says_why_as_interface_6_did() {
    let path = "sleep/owl.png";
    assert_eq!(Failure::of(path, ImageStatus::Ready).unwrap(), None);
    let missing = Failure::of(path, ImageStatus::NotFound).unwrap().unwrap();
    assert_eq!(missing, Failure::Missing(path.to_string()));
    assert_eq!(missing.key(), "sleep.overlay.missing");
    assert_eq!(missing.args(), (Some(path), None));
    for (status, reason) in [
        (ImageStatus::Unsupported, "unsupported image: format"),
        (ImageStatus::Damaged, "invalid image: damaged"),
        (ImageStatus::TooLarge, "image too large for this decoder"),
        (
            ImageStatus::CardFailure,
            "image could not be read or written",
        ),
        (ImageStatus::NoMemory, "not enough memory for the image"),
    ] {
        let why = Failure::of(path, status).unwrap().unwrap();
        assert_eq!(why.key(), "sleep.overlay.unreadable");
        assert_eq!(why.args(), (Some(path), Some(reason)));
    }
}

#[test]
fn the_note_kept_is_read_back_and_cut_where_a_character_ends() {
    assert_eq!(failed(None), None);
    assert_eq!(failed(Some(b"")), None);
    assert_eq!(failed(Some(b" \n")), None);
    assert_eq!(failed(Some(&[0xff, 0xfe])), None);
    assert_eq!(
        failed(Some("There is no widget clock.\n".as_bytes())),
        Some("There is no widget clock.")
    );
    assert_eq!(kept("short"), "short");
    let long = "ä".repeat(FAILED_MAX);
    let cut = kept(&long);
    assert!(cut.len() <= FAILED_MAX && cut.len() >= FAILED_MAX - 1);
    assert!(cut.chars().all(|c| c == 'ä'));
}

#[test]
fn refused_allocations_are_errors() {
    fail_alloc::sweep(|| {
        let mut rows = Rows::new("From the pictures folder", now("", "home.clock"), ON)?;
        rows.preset("owls.overlay.owl", "An owl", true)?;
        rows.widget("home.clock", "Widget: Clock")?;
        assert_eq!(rows.sources().len(), 3);
        assert!(Failure::of("a.png", ImageStatus::Damaged)?.is_some());
        Ok(())
    });
}
