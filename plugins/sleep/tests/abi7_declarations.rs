//! What the plugin declares: the group, the nine settings (kinds, defaults,
//! choices, titles, help, editors, units, steps, sliders), the two screens (the
//! sleep screen and the picker of the picture and the widget over the page),
//! the slot offer, the icon, the needs, the two places of widgets and the
//! events. The batches are checked by the host's rules, read back and compared
//! with what is written here, and pinned byte for byte by a reviewed golden
//! (`PAPERPAW_BLESS=1` writes it anew). The hidden hints of the sleep screen are
//! not declared yet.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#![allow(
    clippy::redundant_closure,
    reason = "the declaring functions are generic over the sink: a closure fixes it"
)]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_sdk::native::declare::{Declarations, DeclareError};

fn batches(
    declare: impl FnOnce(
        &mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>,
    ) -> Result<(), DeclareError>,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d).unwrap();
    }
    out
}

fn all() -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d)));
    all.extend(batches(|d| declarations::screens(d)));
    all.extend(batches(|d| declarations::navigation(d)));
    all.extend(batches(|d| declarations::events(d)));
    all
}

#[test]
fn the_declarations_are_the_reviewed_golden() {
    let mut framed = Vec::new();
    for batch in all() {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&batch);
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/declarations.bin.v1");
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(path, &framed).unwrap();
    }
    assert!(
        framed == std::fs::read(path).unwrap(),
        "the declarations changed: review, then PAPERPAW_BLESS=1 cargo test"
    );
}

#[test]
fn the_group_and_the_settings_say_what_the_settings_page_shows() {
    let native = declared::native("sleep", &all());
    assert_eq!(native.groups, ["group|sleep.title|62||"]);
    let ids: Vec<&str> = native
        .specs
        .iter()
        .map(|spec| spec.split('|').next().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "show",
            "folder",
            "widgets",
            "overlay-picture",
            "overlay-widget",
            "overlay-position",
            "overlay-size",
            "overlay-see-through",
            "overlay-when"
        ]
    );
    let spec = |at: usize| native.specs[at].as_str();
    assert!(spec(0).starts_with(
        "show|2|0|sleep.group|sleep.setting.show|sleep.setting.show.help|t:sleep.show.cat|"
    ));
    assert!(spec(0).ends_with(
        "sleep.show.cat\\nsleep.show.cover\\nsleep.show.picture\\nsleep.show.overlay\\nsleep.show.widgets\\nsleep.show.message\\nsleep.show.page\""
    ));
    assert!(spec(1).contains("|t:sleep|"));
    assert!(spec(2).contains("|t:home.date, home.continue, stats.widget.today, home.recent|"));
    assert!(spec(3).contains("|sleep.setting.overlay-picture.help|t:|"));
    assert!(spec(4).contains("|sleep.setting.overlay-widget.help|t:|"));
    assert!(spec(5).contains("|t:sleep.position.bottom-right|"));
    assert!(spec(6).contains("|i:35|10|100|"));
    assert!(spec(7).contains("|i:0|0|90|"));
    assert!(spec(8).contains("|t:sleep.when.always|"));
    // The editors: the folder and the widgets each on a slot, the picture and
    // the widget over the page on the screen `pick-overlay`; the units, steps
    // and sliders of the two percentages.
    assert_eq!(
        native.extras,
        [
            "folder||||=|2:core.choose-folder|0|0",
            "widgets||||=|2:core.edit-widgets|0|0",
            "overlay-picture||||=|1:pick-overlay|0|0",
            "overlay-widget||||=|1:pick-overlay|0|0",
            "overlay-size|core.unit.percent|||=|0:|5|1",
            "overlay-see-through|core.unit.percent|||=|0:|5|1",
        ]
    );
}

#[test]
fn the_screens_the_slot_the_places_the_needs_and_the_events_are_declared() {
    let native = declared::native("sleep", &all());
    assert_eq!(
        native.screens,
        [
            "pick-overlay|sleep.setting.overlay-picture",
            "screen|sleep.title"
        ]
    );
    assert_eq!(native.slots, ["core.sleep|sleep.screen|0"]);
    assert_eq!(
        native.widget_places,
        [
            "overlay|sleep.title|sleep.overlay-widget|Stack|sleep.show=sleep.show.overlay",
            "widgets|sleep.title|sleep.widgets|Stack|sleep.show=sleep.show.widgets",
        ]
    );
    assert_eq!(native.icons, ["sleep.group|moon"]);
    assert_eq!(native.needs.len(), 10);
    assert!(native.needs.iter().all(|n| n.ends_with("|Sleep")));
    assert!(native.needs.iter().any(|n| n == "sleep.group|Sleep"));
    // Resume, started, plugins changed (70), a setting changed (64), a picture
    // ready (90), each with its token.
    assert_eq!(native.events, ["32:13", "33:12", "64:14", "70:15", "90:16"]);
    assert!(native.status_places.is_empty());
    assert!(native.keys.is_empty());
    assert!(native.menu_entries.is_empty());
    assert!(native.actions.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
}
