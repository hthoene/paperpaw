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

/// The UI kit's list of screens, which keeps its views without their type
/// (#501): each view is found, replaced, moved and dropped exactly once.
#[test]
fn the_screens_list_keeps_and_drops_each_view_once() {
    use std::{cell::RefCell, rc::Rc};
    #[derive(Debug)]
    struct Counted(u32, Rc<RefCell<Vec<u32>>>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    let dropped = Rc::new(RefCell::new(Vec::new()));
    let view = |n| Counted(n, dropped.clone());
    let mut screens = Screens::default();
    for n in 1..=20 {
        screens.open(instance(n), view(n)).unwrap();
    }
    screens.open(instance(5), view(105)).unwrap();
    assert_eq!(*dropped.borrow(), [5], "the replaced view");
    screens.shown(1, instance(7)).unwrap();
    screens.close(instance(3));
    screens.close(instance(30));
    assert_eq!(
        *dropped.borrow(),
        [5, 3],
        "the closed view, nothing for one not open"
    );
    assert_eq!(screens.top(1).map(|v| v.0), Some(7));
    screens.close(instance(7));
    assert!(
        screens.top(1).is_none(),
        "a closed instance is no longer on top"
    );
    assert_eq!(screens.get(instance(5)).map(|v| v.0), Some(105));
    assert_eq!(screens.get(instance(20)).map(|v| v.0), Some(20));
    let left: Vec<u32> = screens
        .all_with_instance()
        .map(|(i, v)| (i.get() * 1000) + v.0)
        .collect();
    let mut expected: Vec<u32> = (1..=20)
        .filter(|n| ![3, 7].contains(n))
        .map(|n| n * 1000 + n)
        .collect();
    expected[3] = 5105;
    assert_eq!(
        left, expected,
        "the views after a closed one moved down in order"
    );
    drop(screens);
    let mut all = dropped.borrow().clone();
    all.sort_unstable();
    let mut want: Vec<u32> = (1..=20).chain([105]).collect();
    want.sort_unstable();
    assert_eq!(all, want, "every view dropped once when the list goes");
}
