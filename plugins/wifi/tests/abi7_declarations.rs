//! What the Wi-Fi plugin declares: the group and its settings, the screens,
//! the action, the status item, the icons, the needs and the first-run step,
//! as the image holds them, and the subscriptions to the radio's events.
//! (The interface-6 registration these once matched is gone; the ids and
//! numbers below are what it registered.)
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};
use paperpaw_sdk::native::declare::Declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/wifi_declarations.rs"));
}

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

/// Every record of the batches, as the host decodes it, in text.
fn text(batches: &[Vec<u8>]) -> String {
    let mut all = String::new();
    for batch in batches {
        for record in records_decode(batch).unwrap().iter() {
            all.push_str(&format!("{record:?}\n"));
        }
    }
    all
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (name, built, image) in [
        (
            "settings",
            batches(|d| declarations::settings(d).unwrap()),
            image::SETTINGS,
        ),
        (
            "screens",
            batches(|d| declarations::screens(d).unwrap()),
            image::SCREENS,
        ),
        (
            "navigation",
            batches(|d| declarations::navigation(d).unwrap()),
            image::NAVIGATION,
        ),
        (
            "events",
            batches(|d| declarations::events(d).unwrap()),
            image::EVENTS,
        ),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

/// The records of the batches, one text line each: what the interface-6
/// registration declared (Beta 3), pinned exactly, so that a changed default,
/// order, key, icon, need or flag fails here.
fn lines(batches: &[Vec<u8>]) -> Vec<String> {
    text(batches).lines().map(String::from).collect()
}

/// A setting record as its text: `default` is the encoded value.
fn setting(name: &str, kind: u8, default: &str, choices: &str) -> String {
    format!(
        "SettingSpec(SettingSpec {{ name: \"{name}\", kind: {kind}, flags: 0, group: \"wifi.group\", \
         title: \"wifi.setting.{name}\", help: \"wifi.setting.{name}.help\", default: [{default}], \
         min: 0, max: 0, choices: \"{choices}\" }})"
    )
}

#[test]
fn the_group_and_the_settings_are_declared() {
    let settings = batches(|d| declarations::settings(d).unwrap());
    assert_eq!(settings.len(), 1);
    // The group (order 85, its page) and the four settings: on demand on,
    // idle off after a minute, while charging off, join the picked network
    // first; the choices in order; every one with its help text.
    assert_eq!(
        lines(&settings),
        [
            "SettingGroup(SettingGroup { name: \"group\", title: \"wifi.title\", order: 85, page: \"page\", parent: \"\" })".to_string(),
            setting("on-demand", 0, "1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1", ""),
            setting(
                "idle-off",
                2,
                "1, 0, 0, 0, 3, 0, 0, 0, 16, 0, 0, 0, 119, 105, 102, 105, 46, 105, 100, 108, 101, 45, 111, 102, 102, 46, 49, 109",
                "wifi.idle-off.30s\\nwifi.idle-off.1m\\nwifi.idle-off.2m\\nwifi.idle-off.5m\\nwifi.idle-off.10m",
            ),
            setting("while-charging", 0, "1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0", ""),
            setting(
                "join-first",
                2,
                "1, 0, 0, 0, 3, 0, 0, 0, 22, 0, 0, 0, 119, 105, 102, 105, 46, 106, 111, 105, 110, 45, 102, 105, 114, 115, 116, 46, 112, 105, 99, 107, 101, 100",
                "wifi.join-first.picked\\nwifi.join-first.strongest",
            ),
        ]
    );
    for id in declarations::IDLE_OFF {
        assert!(text(&settings).contains(id), "{id}");
    }
    for batch in &settings {
        // The host admits the group and the settings as far as it wires them
        // (others it refuses as unsupported until it does).
        let _ = validate_batch("wifi", &records_decode(batch).unwrap());
    }
}

#[test]
fn the_screens_action_status_item_icons_needs_and_step_are_declared() {
    // Three screens, one instance each (flag 64), tokens 10 to 12.
    let screens = batches(|d| declarations::screens(d).unwrap());
    assert_eq!(screens.len(), 1);
    assert_eq!(
        lines(&screens),
        [
            "ScreenSpec(ScreenSpec { name: \"page\", title: \"wifi.title\", token: 10, flags: 64 })",
            "ScreenSpec(ScreenSpec { name: \"networks\", title: \"wifi.networks.title\", token: 11, flags: 64 })",
            "ScreenSpec(ScreenSpec { name: \"phone\", title: \"wifi.phone.title\", token: 12, flags: 64 })",
        ]
    );
    for batch in &screens {
        validate_batch("wifi", &records_decode(batch).unwrap()).unwrap();
    }
    // The action, the icons (the group's and the action's, both `wifi`), the
    // status item (its tap opens the page), the `Wifi` need (7) of the four
    // settings and the first-run step (order 50, the page).
    let navigation = batches(|d| declarations::navigation(d).unwrap());
    assert_eq!(
        lines(&navigation),
        [
            "ActionSpec(ActionSpec { name: \"open\", title: \"wifi.title\", token: 1, flags: 0 })",
            "IconSpec(IconSpec { item: \"wifi.group\", icon: \"wifi\" })",
            "IconSpec(IconSpec { item: \"wifi.open\", icon: \"wifi\" })",
            "StatusItem(StatusItem { name: \"status\", title: \"wifi.status.item\", tap: \"open\", priority: 0, min_width: 0, token: 2, flags: 0 })",
            "MenuNeed(MenuNeed { item: \"wifi.on-demand\", need: 7, capability: 0 })",
            "MenuNeed(MenuNeed { item: \"wifi.idle-off\", need: 7, capability: 0 })",
            "MenuNeed(MenuNeed { item: \"wifi.while-charging\", need: 7, capability: 0 })",
            "MenuNeed(MenuNeed { item: \"wifi.join-first\", need: 7, capability: 0 })",
            "SetupStep(SetupStep { name: \"setup\", title: \"wifi.title\", help: \"wifi.setup.help\", order: 50, page_kind: 1, page: \"wifi.page\", applies_key: \"\", applies_value: \"\", need: 7, capability: 0, flags: 3 })",
        ]
    );
    // Ten subscriptions (the radio's four and the timers' work among them) in two batches.
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 2);
}

/// The subscriptions, decoded: the radio's own events 92-95 go to their
/// callbacks, with the five the plugin had before.
#[test]
fn the_decoded_subscriptions_name_every_event_the_plugin_handles() {
    use paperpaw_core::native_host::registrations::{Record, Registration, records_decode};
    let mut found = Vec::new();
    for batch in batches(|d| declarations::events(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::Registration(Registration::Event { event_tag, token }) => {
                    found.push((event_tag, token));
                }
                other => panic!("{other:?}"),
            }
        }
    }
    // The device's five, as before, then the radio's four: scanned 92, done 93,
    // network wanted 94 and idle 95 (`docs/abi/device.md`), then the work's and the served page's (server, request).
    let tokens: Vec<u32> = found.iter().map(|(_, token)| *token).collect();
    assert_eq!(
        tokens,
        [
            declarations::STARTED,
            declarations::STATUS_CHANGED,
            declarations::SCREEN_SHOWN,
            declarations::WIFI_CHANGED,
            declarations::POWER_CHANGED,
            declarations::WIFI_SCANNED,
            declarations::WIFI_DONE,
            declarations::NETWORK_WANTED,
            declarations::NETWORK_IDLE,
            declarations::WORK,
            declarations::SERVER_EVENT,
            declarations::SERVED_REQUEST,
        ]
    );
    let radio: Vec<u32> = found[5..9].iter().map(|(tag, _)| *tag).collect();
    assert_eq!(radio, [92, 93, 94, 95]);
    // The timers (idle off, retry) come due as work (`event.work_due`).
    assert_eq!(found[9].0, 103, "work_due");
    assert_eq!(found[0].0, 32, "started");
}
