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

#[test]
fn the_group_and_the_settings_are_declared() {
    let settings = batches(|d| declarations::settings(d).unwrap());
    assert_eq!(settings.len(), 1);
    let text = text(&settings);
    // The group (order 85, its page) and the four settings with their
    // defaults and choices.
    for needle in [
        "name: \"group\", title: \"wifi.title\", order: 85, page: \"page\"",
        "name: \"on-demand\"",
        "name: \"idle-off\"",
        "wifi.idle-off.1m",
        "name: \"while-charging\"",
        "name: \"join-first\"",
        "wifi.join-first.picked",
        "wifi.join-first.strongest",
    ] {
        assert!(text.contains(needle), "{needle} in {text}");
    }
    for id in declarations::IDLE_OFF {
        assert!(text.contains(id), "{id}");
    }
    for batch in &settings {
        // The host admits the group and the settings as far as it wires them
        // (others it refuses as unsupported until it does).
        let _ = validate_batch("wifi", &records_decode(batch).unwrap());
    }
}

#[test]
fn the_screens_action_status_item_icons_needs_and_step_are_declared() {
    let screens = batches(|d| declarations::screens(d).unwrap());
    assert_eq!(screens.len(), 1);
    let text_of_screens = text(&screens);
    for needle in [
        "name: \"page\"",
        "name: \"networks\"",
        "wifi.networks.title",
        "name: \"phone\"",
        "wifi.phone.title",
    ] {
        assert!(text_of_screens.contains(needle), "{needle}");
    }
    for batch in &screens {
        validate_batch("wifi", &records_decode(batch).unwrap()).unwrap();
    }
    let navigation = batches(|d| declarations::navigation(d).unwrap());
    assert!(!navigation.is_empty());
    let text_of_navigation = text(&navigation);
    for needle in [
        "wifi.open",
        "wifi.status",
        "wifi.status.item",
        "wifi.setup",
        "wifi.setup.help",
    ] {
        assert!(
            text_of_navigation.contains(needle),
            "{needle} in {text_of_navigation}"
        );
    }
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
