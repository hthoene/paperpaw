//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_sdk::native::declare::Declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/store_declarations.rs"));
}

type Batches = Vec<Vec<u8>>;

macro_rules! batches_of {
    ($declare:path) => {{
        let mut out: Batches = Vec::new();
        {
            let mut d = Declarations::new(|batch: &[u8]| {
                out.push(batch.to_vec());
                Ok(())
            });
            $declare(&mut d).unwrap();
        }
        out
    }};
}

fn batches() -> Batches {
    batches_of!(declarations::settings)
}

#[test]
fn the_image_is_what_the_builder_makes() {
    let built = batches();
    assert_eq!(built.len(), image::SETTINGS.len());
    for (built, image) in built.iter().zip(image::SETTINGS) {
        assert_eq!(built.as_slice(), *image);
    }
}

#[test]
fn the_host_admits_the_two_settings() {
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        assert_eq!(records.iter().count(), 2);
        validate_batch("store", &records).unwrap();
    }
}

/// The two settings: kinds, defaults, the group, the titles and helps, and the
/// guarded flag of the community switch, as pinned (made while the interface-6
/// registration was compared with them line for line).
#[test]
fn the_decoded_batches_say_what_was_pinned() {
    let new = declared::native("store", &batches());
    assert_eq!(new.specs.len(), 2);
    declared::assert_pinned("settings", &new);
    // The guard is on the community switch alone.
    let guarded = |line: &String| {
        let flags: u32 = line.split('|').nth(2).unwrap().parse().unwrap();
        flags & 2 != 0
    };
    let on: Vec<_> = new.specs.iter().filter(|l| guarded(l)).collect();
    assert_eq!(on.len(), 1);
    assert!(on[0].starts_with("community|"));
}

#[test]
fn the_action_menu_entries_and_event_are_what_the_image_holds_and_the_host_admits_them() {
    let navigation = batches_of!(declarations::navigation);
    assert_eq!(navigation.len(), image::NAVIGATION.len());
    for (built, image) in navigation.iter().zip(image::NAVIGATION) {
        assert_eq!(built.as_slice(), *image);
        validate_batch("store", &records_decode(built).unwrap()).unwrap();
    }
    let events = batches_of!(declarations::events);
    assert_eq!(events.len(), image::EVENTS.len());
    for (built, image) in events.iter().zip(image::EVENTS) {
        assert_eq!(built.as_slice(), *image);
        validate_batch("store", &records_decode(built).unwrap()).unwrap();
    }
}

/// The nine screens are what the image holds, the host admits them, and each
/// has the id and the title pinned (those of the interface-6 registration).
#[test]
fn the_screens_are_what_the_image_holds_and_were_pinned() {
    let screens = batches_of!(declarations::screens);
    assert_eq!(screens.len(), image::SCREENS.len());
    // A batch holds at most eight records: the ninth screen is a batch of its own.
    let mut declared_screens = 0;
    for (built, image) in screens.iter().zip(image::SCREENS) {
        assert_eq!(built.as_slice(), *image);
        let records = records_decode(built).unwrap();
        declared_screens += records.iter().count();
        validate_batch("store", &records).unwrap();
    }
    assert_eq!(declared_screens, 9);
    let new = declared::native("store", &screens);
    assert_eq!(new.screens.len(), 9);
    declared::assert_pinned("screens", &new);
}

/// The setup step is what the image holds, the host admits it, and it is what
/// the interface-6 registration had, pinned: its title, help and order, opening the page of
/// the first-run offer.
#[test]
fn the_setup_step_is_what_the_image_holds_and_the_host_admits_it() {
    let setup = batches_of!(declarations::setup);
    assert_eq!(setup.len(), image::SETUP.len());
    for (built, image) in setup.iter().zip(image::SETUP) {
        assert_eq!(built.as_slice(), *image);
        let records = records_decode(built).unwrap();
        assert_eq!(records.iter().count(), 2);
        validate_batch("store", &records).unwrap();
    }
    let new = declared::native("store", &setup);
    assert_eq!(new.others.len(), 2, "{:?}", new.others);
    declared::assert_pinned("setup", &new);
}

/// The store hears the Wi-Fi coming up (#72): the plugins a setup template
/// queued while offline are released then, not only at the start.
#[test]
fn the_store_subscribes_to_the_wifi_changing() {
    use paperpaw_core::native_host::registrations::{Record, Registration};
    use paperpaw_sdk::native::declare::EventKind;
    // The tag of the wifi event, as the SDK declares it.
    let mut one = Vec::new();
    Declarations::new(|batch: &[u8]| {
        one.push(batch.to_vec());
        Ok(())
    })
    .subscribe_event(EventKind::WifiChanged, 99)
    .unwrap()
    .submit()
    .unwrap();
    let Some(Record::Registration(Registration::Event {
        event_tag: wifi, ..
    })) = records_decode(&one[0]).unwrap().iter().next()
    else {
        panic!("no event");
    };
    let mut tokens = Vec::new();
    for batch in batches_of!(declarations::events) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::Registration(Registration::Event { event_tag, token }) = record
                && event_tag == wifi
            {
                tokens.push(token);
            }
        }
    }
    assert_eq!(tokens, [declarations::WIFI_TOKEN]);
}
