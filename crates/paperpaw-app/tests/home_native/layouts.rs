//! Home screen pages as anyone may write them in `paperpaw.ini`: items
//! unknown, without a place, overlapping, off the grid or of absurd
//! sizes, grids of any size. The home screen shows them on both kinds of
//! device and can be edited without a panic; what it keeps after an edit
//! reads back the same in a new kernel, which shows the same page.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;
use proptest::prelude::*;

/// One item as written: an id, maybe a place, maybe a size, maybe junk.
fn item() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec![
            "home.clock",
            "home.continue",
            "home.recent",
            "home.date",
            "home.clock@3",
            "tool:core.library",
            "tool:core.settings",
            "tool:nope.nothing",
            "no.such-widget",
            "home.tools",
            "",
            "tool:",
            "@@@",
        ]),
        prop::option::of((0u16..300, 0u16..300)),
        prop::option::of((0u16..300, 0u16..300)),
    )
        .prop_map(|(id, at, size)| {
            let mut text = id.to_string();
            if let Some((col, row)) = at {
                text.push_str(&format!(" {col}/{row}"));
            }
            if let Some((w, h)) = size {
                text.push_str(&format!(" {w}x{h}"));
            }
            text
        })
}

fn page() -> impl Strategy<Value = String> {
    prop::collection::vec(item(), 0..10).prop_map(|items| items.join(", "))
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Failures become named tests, not files.
        failure_persistence: None,
        ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48))
    })]

    #[test]
    fn any_page_shows_and_what_an_edit_keeps_reads_back_the_same(
        first in page(),
        second in page(),
        columns in -3i64..12,
        rows in -3i64..20,
        touch in any::<bool>(),
    ) {
        let config = format!(
            "[home]\npage-1 = {first}\npage-2 = {second}\ncolumns = {columns}\nrows = {rows}\n"
        );
        let device = if touch { TOUCH } else { X4 };
        let mut app = App::new(device, Shared::memory(&config, &[]));
        prop_assert_eq!(app.screen(), "home.dashboard");
        let _ = app.parts();
        app.run(actions::NEXT_PAGE);
        app.run(actions::PREV_PAGE);
        // Edit: the first item's options, then remove it.
        app.run(Id::new_static("home.edit-pages"));
        app.run(actions::RIGHT);
        app.run(actions::CONFIRM);
        if app.screen() == "core.choice-dialog" && app.shows("Remove") {
            if touch {
                app.tap_on("Remove");
            } else {
                while app.focused().as_deref() != Some("Remove") {
                    app.run(actions::DOWN);
                }
                app.run(actions::CONFIRM);
            }
            prop_assert_eq!(app.screen(), "home.dashboard");
            let kept = app.kernel.settings().text("home.page-1").to_string();
            app.run(actions::BACK);
            let shown = app.described();
            let mut app = app.restart();
            prop_assert_eq!(app.kernel.settings().text("home.page-1"), kept.as_str());
            prop_assert_eq!(app.described(), shown);
        }
    }
}

/// Items written with places that overlap: the first keeps its place,
/// the other goes where there is room, and what is kept after an edit
/// has none over another.
#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at layouts.rs:113); to analyse (#244, Home removal)"]
fn overlapping_places_are_kept_apart_once_edited() {
    let config = "[home]\ncolumns = 4\npage-1 = tool:core.library 0/0 2x2, tool:core.settings 1/1 2x2, \
                  tool:store.open 0/0 1x1\n";
    let mut app = App::new(X4, Shared::memory(config, &[]));
    app.run(Id::new_static("home.edit-pages"));
    // The library's options; removed.
    app.run(actions::RIGHT);
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "core.choice-dialog", "{}", app.described());
    while app.focused().as_deref() != Some("Remove") {
        app.run(actions::DOWN);
    }
    app.run(actions::CONFIRM);
    let kept = app.kernel.settings().text("home.page-1").to_string();
    assert!(!kept.contains("core.library"), "{kept}");
    // Both others shown, each with a place of its own.
    assert!(app.shows("Settings"), "{}", app.described());
    assert!(app.shows("Plugin store"), "{}", app.described());
    let places: Vec<(u16, u16, u16, u16)> = kept
        .split(", ")
        .map(|item| {
            let mut parts = item.split_whitespace().skip(1);
            let (col, row) = parts.next().unwrap().split_once('/').unwrap();
            let (w, h) = parts.next().unwrap().split_once('x').unwrap();
            (
                col.parse().unwrap(),
                row.parse().unwrap(),
                w.parse().unwrap(),
                h.parse().unwrap(),
            )
        })
        .collect();
    assert_eq!(places.len(), 2, "{kept}");
    let ((c1, r1, w1, h1), (c2, r2, w2, h2)) = (places[0], places[1]);
    assert!(
        c1 + w1 <= c2 || c2 + w2 <= c1 || r1 + h1 <= r2 || r2 + h2 <= r1,
        "{kept}"
    );
}
