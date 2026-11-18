//! Screens over their life: the times they ask for, a book's own settings
//! while it is open.

use super::*;
use crate::Staging;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{MemoryStorage, Params};

const READER: Id = Id::new_static("book.reader");
const MENU: Id = Id::new_static("book.menu");
const FULL: Id = Id::new_static("book.full");
const CLOSED: Id = Id::new_static("book.closed");
const BOOK_SETTINGS: &str = "books/a/settings.ini";

struct ReaderScreen {
    log: Log,
}

impl crate::testing::Fake for ReaderScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Turn<'_, '_>) {
        cx.command(Ask::SetScope(Some(String::from(BOOK_SETTINGS))));
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Turn<'_, '_>) {
        if *event == CLOSED {
            self.log.borrow_mut().push(String::from("closed said"));
        }
        if *event == events::SETTING_CHANGED {
            let at = cx.env().now().add_millis(10);
            cx.wake_at(at);
        }
    }

    fn on_close(&mut self, cx: &mut Turn<'_, '_>) {
        self.log.borrow_mut().push(String::from("reader close"));
        cx.command(Ask::Emit(CLOSED, Params::new()));
    }

    fn on_timer(&mut self, _: &mut Turn<'_, '_>) {
        self.log.borrow_mut().push(String::from("reader timer"));
    }
}

/// A screen that says when its timer comes, over `region` (all of the
/// screen without).
struct Other {
    name: &'static str,
    region: Option<Rect>,
    log: Log,
}

impl crate::testing::Fake for Other {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn region(&self, _: &Place<'_>) -> Option<Rect> {
        self.region
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_timer(&mut self, _: &mut Turn<'_, '_>) {
        self.log
            .borrow_mut()
            .push(alloc::format!("{} timer", self.name));
    }
}

/// A reader that keeps a book's own settings while it is open and asks
/// for a timer when a setting changes; a menu over the lower half of the
/// screen and a full screen, each saying when its timer comes.
fn book_plugin(kept0: Log) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let log = kept0.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            READER,
            "reader",
            Box::new(move |_, _| Box::new(ReaderScreen { log: log.clone() })),
        ))?;
        r.bind_slot(slots::HOME, READER, Priority::DEFAULT)?;
        for (id, name, region) in [
            (MENU, "menu", Some(Rect::new(0, 45, 90, 45))),
            (FULL, "full", None),
        ] {
            let log = kept0.clone();
            r.add_fake(crate::testing::FakeSpec::new(
                id,
                "other",
                Box::new(move |_, _| {
                    Box::new(Other {
                        name,
                        region,
                        log: log.clone(),
                    })
                }),
            ))?;
        }
        Ok(())
    }
}

/// What [`book_plugin`] registers under.
fn book_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("book"), "Book", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn book_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(book_plugin_manifest(), book_plugin(log.clone()))
        .storage(MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn text(kernel: &Kernel, path: &str) -> String {
    kernel.shared.recall.clear();
    let data = paperpaw_sdk::read_file(kernel.storage(), path, 1 << 20).unwrap_or_default();
    String::from_utf8(data).unwrap()
}

/// With "only this book" on, a book's settings stay its own while it is
/// open, also once its screen was made anew (the screen turned, the row
/// of hints changed in a menu over it).
#[test]
fn a_books_own_settings_last_while_it_is_open() {
    let log = log();
    let mut kernel = book_kernel(&log);
    kernel
        .set_setting(Instant(1), "core.scope-only", "on")
        .unwrap();
    kernel.open_screen(Instant(2), MENU, Params::new());
    kernel.resize(Instant(3), 80, 90);
    kernel
        .set_setting(Instant(4), "core.font-size", "14")
        .unwrap();
    assert!(text(&kernel, BOOK_SETTINGS).contains("font-size = 14"));
    assert!(!text(&kernel, paperpaw_sdk::builtin::CONFIG_FILE).contains("font-size"));
    // A full screen over it, and the menu that changes the setting over
    // that: still the book's.
    kernel.open_screen(Instant(5), FULL, Params::new());
    kernel.open_screen(Instant(6), MENU, Params::new());
    kernel
        .set_setting(Instant(7), "core.font-size", "15")
        .unwrap();
    assert!(text(&kernel, BOOK_SETTINGS).contains("font-size = 15"));
}

/// A timer is the screen's that asked for it, and it runs only while
/// that screen is on top: below a menu (a quick panel, a keyboard) or a
/// full screen it waits until the screen is on top again, also where the
/// screen was made anew meanwhile; it never comes to the screen over it.
#[test]
fn timers_come_to_the_screen_that_asked_once_it_is_on_top() {
    let log = log();
    let mut kernel = book_kernel(&log);
    for (at, over) in [(0, MENU), (100, FULL)] {
        kernel.open_screen(Instant(at + 1), over, Params::new());
        // A setting changed over it: the reader below asks for a timer.
        let value = if at == 0 { "on" } else { "off" };
        kernel
            .set_setting(Instant(at + 2), "core.night-mode", value)
            .unwrap();
        let effects = kernel.tick(Instant(at + 12));
        assert!(log.borrow().is_empty(), "{:?}", log.borrow());
        assert_eq!(effects.wake_at(), None);
        let effects = kernel.run_action(Instant(at + 20), actions::BACK);
        assert_eq!(effects.wake_at(), Some(Instant(at + 12)));
        kernel.tick(Instant(at + 20));
        assert_eq!(*log.borrow(), ["reader timer"]);
        log.borrow_mut().clear();
    }
    // Made anew below a menu (the screen turned there): still asked for.
    kernel.open_screen(Instant(201), MENU, Params::new());
    kernel
        .set_setting(Instant(202), "core.night-mode", "on")
        .unwrap();
    kernel.resize(Instant(203), 80, 90);
    log.borrow_mut().clear();
    kernel.run_action(Instant(220), actions::BACK);
    kernel.tick(Instant(220));
    assert_eq!(*log.borrow(), ["reader timer"]);
}

/// Values shown without being kept (a setting tried in a menu) last
/// while the screen that asked shows, also made anew, and end with it.
#[test]
fn values_tried_end_with_the_screen_that_tried_them() {
    let log = log();
    let mut kernel = book_kernel(&log);
    kernel.open_screen(Instant(1), MENU, Params::new());
    let tried = alloc::vec![(
        Id::new_static("core.night-mode"),
        Some(SettingValue::Bool(true))
    )];
    kernel.apply(Ask::PreviewSettings(tried));
    kernel.finish();
    assert!(kernel.settings().bool("core.night-mode"));
    kernel.resize(Instant(2), 80, 90);
    assert!(kernel.settings().bool("core.night-mode"), "still tried");
    kernel.run_action(Instant(3), actions::BACK);
    assert!(!kernel.settings().bool("core.night-mode"));
}
