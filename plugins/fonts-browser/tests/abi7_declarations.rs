//! What the native plugin declares: the screens, the catalog setting, the actions with
//! their menu places and icons, and the events, as the host decodes them. (They were
//! compared with the interface-6 registration before it was removed; the expectations
//! below are what that comparison showed.)
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
use paperpaw_core::native_host::registrations::records_decode;
use paperpaw_sdk::native::declare::Declarations;

/// The records of every batch `declare` submits, as text.
fn shown(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<String> {
    let mut batches: Vec<Vec<u8>> = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            batches.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    batches
        .iter()
        .flat_map(|b| {
            records_decode(b)
                .unwrap()
                .iter()
                .map(|r| format!("{r:?}"))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn screens_match() {
    assert_eq!(
        shown(|d| declarations::screens(d).unwrap()),
        [
            r#"ScreenSpec(ScreenSpec { name: "font", title: "fonts-browser.title", token: 10, flags: 0 })"#,
            // Flags 64: a single instance.
            r#"ScreenSpec(ScreenSpec { name: "list", title: "fonts-browser.title", token: 11, flags: 64 })"#,
        ]
    );
}

#[test]
fn the_catalog_setting_matches() {
    let shown = shown(|d| declarations::settings(d).unwrap());
    assert_eq!(shown.len(), 1);
    // A text in the developer group, with its help; the default is the library's address.
    for part in [
        r#"name: "catalog", kind: 3, flags: 0, group: "core.group.developer""#,
        r#"title: "fonts-browser.setting.catalog", help: "fonts-browser.setting.catalog.help""#,
    ] {
        assert!(shown[0].contains(part), "{}", shown[0]);
    }
    let address = b"https://paperpaw.org/api/v1/fonts.json";
    let bytes: Vec<String> = address.iter().map(|b| b.to_string()).collect();
    assert!(shown[0].contains(&bytes.join(", ")), "{}", shown[0]);
}

#[test]
fn actions_menus_and_icons_match() {
    assert_eq!(
        shown(|d| declarations::navigation(d).unwrap()),
        [
            r#"ActionSpec(ActionSpec { name: "open", title: "fonts-browser.more", token: 1, flags: 0 })"#,
            r#"ActionSpec(ActionSpec { name: "own", title: "fonts-browser.own", token: 2, flags: 0 })"#,
            r#"MenuEntry(MenuEntry { menu: "core.reading-font", item: "fonts-browser.open", order: 0, place: 0, anchor: "" })"#,
            r#"MenuEntry(MenuEntry { menu: "core.reading-font", item: "fonts-browser.own", order: 0, place: 0, anchor: "" })"#,
            r#"MenuEntry(MenuEntry { menu: "core.status-font", item: "fonts-browser.open", order: 0, place: 0, anchor: "" })"#,
            r#"MenuEntry(MenuEntry { menu: "core.status-font", item: "fonts-browser.own", order: 0, place: 0, anchor: "" })"#,
            r#"IconSpec(IconSpec { item: "fonts-browser.open", icon: "typography" })"#,
            r#"IconSpec(IconSpec { item: "fonts-browser.own", icon: "folder-open" })"#,
        ]
    );
}

#[test]
fn the_events_are_the_download() {
    // Interface 6 had the download's end (and `started`, which cancelled a download cut off
    // by a restart: `make` clears the staging folder now, `PORTING-7.md`): `http_done`, 99.
    assert_eq!(
        shown(|d| declarations::events(d).unwrap()),
        [
            "Registration(Event { event_tag: 99, token: 3 })",
            // The setting and plugin-list changes the activation added (the settings
            // table and the list of what is installed are read again on them).
            "Registration(Event { event_tag: 64, token: 5 })",
            "Registration(Event { event_tag: 70, token: 6 })",
        ]
    );
}

#[test]
fn the_end_of_an_install_is_heard() {
    // Not in interface 6 (the kernel said it was done): `install_done` (97) ends the install
    // and the removal the plugin asked the host for.
    assert_eq!(
        shown(|d| declarations::installs(d).unwrap()),
        ["Registration(Event { event_tag: 97, token: 4 })"]
    );
}
