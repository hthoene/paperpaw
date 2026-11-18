//! What the native declarations say, and that the host's rules accept them:
//! the settings, the three screens and their instance policy, the actions,
//! the place in the tools, the sleep screen offered to `core.sleep`, the five
//! widgets and the events the plugin reads; and that every text the code asks
//! for is in both languages.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_i18n::{Catalog, check};
use paperpaw_plugin_calendar::declarations;
use paperpaw_sdk::native::declare::Declarations;

const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

fn all() -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::widgets(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    all
}

#[test]
fn the_decoded_batches_say_what_the_plugin_declares() {
    let native = declared::native("calendar", &all());
    for name in ["sources|", "sync|", "days|"] {
        assert!(native.specs.iter().any(|s| s.starts_with(name)), "{name}");
    }
    // The calendars are edited on their own screen.
    assert!(native.extras.iter().any(|e| e.contains("1:sources")));
    assert_eq!(
        native.screens,
        [
            "screen|calendar.title",
            "sources|calendar.sources.title",
            "sleep|calendar.sleep"
        ]
    );
    assert_eq!(
        native.actions,
        ["open|calendar.title", "sync|calendar.action.sync"]
    );
    assert_eq!(native.menu_entries, ["core.tools|calendar.open|82"]);
    // The sleep screen is offered to the slot, below the sleep plugin's own.
    assert!(
        native
            .slots
            .iter()
            .any(|s| s.starts_with("core.sleep|calendar.sleep|")),
        "{:?}",
        native.slots
    );
    // Five widgets, each with the shared look and its own settings.
    for widget in ["today", "next", "agenda", "month", "birthdays"] {
        let group = format!("|calendar.{widget}|");
        assert!(
            native.specs.iter().filter(|s| s.contains(&group)).count() >= 6,
            "{widget}: {:?}",
            native.specs
        );
    }
}

#[test]
fn the_screens_open_one_at_a_time() {
    let mut new = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((s.name.to_string(), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new.len(), 3);
    assert!(new.iter().all(|(_, policy)| *policy == 1), "{new:?}");
}

#[test]
fn the_plugin_reads_the_events_it_needs_and_no_others() {
    let native = declared::native("calendar", &all());
    // The start, the wake, a setting, the work that is due, the end of a request.
    assert_eq!(native.events.len(), 5, "{:?}", native.events);
}

fn has(catalog: &str, key: &str) -> bool {
    [
        format!("{key} = "),
        format!("{key}.one = "),
        format!("{key}.other = "),
    ]
    .iter()
    .any(|line| catalog.lines().any(|l| l.starts_with(line.as_str())))
}

#[test]
fn the_catalogs_agree() {
    let en = Catalog::parse(EN).unwrap();
    let de = Catalog::parse(DE).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn every_text_the_code_asks_for_is_in_both_languages() {
    // Ids of screens, actions, settings, files and work are no texts.
    let not_texts = [
        "calendar.screen",
        "calendar.sources",
        "calendar.open",
        "calendar.sync",
        "calendar.refresh",
        "calendar.day",
        "calendar.days",
        "calendar.today",
        "calendar.next",
        "calendar.agenda",
        "calendar.month",
        "calendar.birthdays",
        "calendar.txt",
        "calendar.txt.new",
        "calendar.damaged.txt",
        "calendar.sync.open",
        "calendar.sync.online",
        "calendar.sync.charging",
    ];
    let mut asked = 0;
    for source in [
        include_str!("../src/native_plugin.rs"),
        include_str!("../src/native_asks.rs"),
        include_str!("../src/native_screens.rs"),
        include_str!("../src/native_widgets.rs"),
        include_str!("../src/native_text.rs"),
        include_str!("../declarations.rs"),
    ] {
        for part in source.split('"').skip(1).step_by(2) {
            let key = part.starts_with("calendar.")
                && !part.contains(' ')
                && !part.ends_with('.')
                && !part.ends_with(".txt")
                && !not_texts.contains(&part);
            if key {
                asked += 1;
                for (name, catalog) in [("en", EN), ("de", DE)] {
                    assert!(has(catalog, part), "{part} missing in {name}");
                }
            }
        }
    }
    assert!(asked >= 100, "{asked}");
}

#[test]
fn the_how_it_works_texts_say_what_the_plugin_does() {
    for (text, words) in [
        (
            include_str!("../how/en.txt"),
            ["CalDAV", "iCal", "CardDAV", "sleep screen"],
        ),
        (
            include_str!("../how/de.txt"),
            ["CalDAV", "iCal", "CardDAV", "Ruhebildschirm"],
        ),
    ] {
        for word in words {
            assert!(text.contains(word), "{word}");
        }
    }
}
