//! The native declarations name what the interface-6 registration named (pinned
//! here from that code, which is gone): the screens, the catalog setting, the
//! actions with their menu places and icons, and the events; and what interface
//! 6 did not have (the host's end of an install, the settings and the plugins
//! that change).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_sdk::native::declare::Declarations;

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
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    all.extend(batches(|d| declarations::installs(d).unwrap()));
    all
}

#[test]
fn the_decoded_batches_say_what_the_registration_said() {
    let native = declared::native("fonts-browser", &all());
    // The catalog setting: text, in the developer group, with its help.
    assert_eq!(
        native.specs,
        [
            "catalog|3|0|core.group.developer|fonts-browser.setting.catalog|fonts-browser.setting.catalog.help|t:https://paperpaw.org/api/v1/fonts.json|0|0|\"\""
        ]
    );
    assert_eq!(
        native.screens,
        ["font|fonts-browser.title", "list|fonts-browser.title"]
    );
    assert_eq!(
        native.actions,
        ["open|fonts-browser.more", "own|fonts-browser.own"]
    );
    // After the fonts to choose from, where the reading font and the status
    // lines' font are chosen.
    assert_eq!(
        native.menu_entries,
        [
            "core.reading-font|fonts-browser.open|0",
            "core.reading-font|fonts-browser.own|0",
            "core.status-font|fonts-browser.open|0",
            "core.status-font|fonts-browser.own|0",
        ]
    );
    assert_eq!(
        native.icons,
        [
            "fonts-browser.open|typography",
            "fonts-browser.own|folder-open"
        ]
    );
    // The events (tag:token): a file or the catalog arrived (64 is the
    // setting that changed, 70 the plugins that changed, 97 the end of an
    // install or removal, 99 an answer of `core.net`).
    assert_eq!(native.events, ["64:5", "70:6", "97:4", "99:3"]);
    assert!(native.needs.is_empty());
    assert!(native.groups.is_empty() && native.extras.is_empty());
    assert!(native.slots.is_empty() && native.keys.is_empty() && native.others.is_empty());
    assert!(native.widget_places.is_empty() && native.status_places.is_empty());
}

/// Every text the declarations ask for exists in both languages.
#[test]
fn the_declared_texts_have_words() {
    let (en, de) = (
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
    );
    for key in [
        "fonts-browser.title",
        "fonts-browser.more",
        "fonts-browser.own",
        "fonts-browser.setting.catalog",
        "fonts-browser.setting.catalog.help",
    ] {
        for (name, catalog) in [("en", en), ("de", de)] {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{key} = "))),
                "{key} missing in {name}"
            );
        }
    }
}
