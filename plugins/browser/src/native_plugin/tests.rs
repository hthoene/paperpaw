//! What the plugin keeps of a request while its screens come and go.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;

/// The instance number `n` the host issues: read from an event as the plugin
/// reads it.
fn instance(n: u32) -> Instance {
    use paperpaw_core::native_host::registrations::{EventScreenOpened, Record, records_encode};
    let record = Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen: "browser.screen",
        instance: n,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    });
    let mut bytes = [0_u8; 256];
    let len = records_encode(core::slice::from_ref(&record), &mut bytes).unwrap();
    match Incoming::new(&bytes[..len]).unwrap().decode().unwrap() {
        Event::ScreenOpened { instance, .. } => instance,
        other => panic!("{other:?}"),
    }
}

fn loading(browser: &mut NativeBrowser, token: u32) {
    browser.loading = Some(Loading {
        token,
        url: copy("https://a.example/").unwrap(),
    });
}

#[test]
fn closing_the_last_screen_ends_the_request() {
    let mut browser = NativeBrowser::default();
    browser.screens.open(instance(1), View::default()).unwrap();
    loading(&mut browser, 7);
    assert!(browser.wanted());
    assert_eq!(browser.closed(instance(1)), Some(7));
    assert!(browser.loading.is_none(), "its end is not waited for");
    assert!(!browser.wanted(), "an answer that still comes is discarded");
}

#[test]
fn closing_a_screen_while_another_is_open_keeps_the_request() {
    let mut browser = NativeBrowser::default();
    browser.screens.open(instance(1), View::default()).unwrap();
    browser.screens.open(instance(2), View::default()).unwrap();
    loading(&mut browser, 3);
    assert_eq!(browser.closed(instance(2)), None);
    assert_eq!(browser.loading.as_ref().map(|l| l.token), Some(3));
    assert!(browser.wanted());
}

#[test]
fn closing_without_a_request_ends_nothing() {
    let mut browser = NativeBrowser::default();
    browser.screens.open(instance(1), View::default()).unwrap();
    assert_eq!(browser.closed(instance(1)), None);
}
