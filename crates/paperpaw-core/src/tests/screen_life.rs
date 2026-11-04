//! Screens over their life: made anew for another size, the times they
//! ask for (below overlays too, where they ask), a book's own settings
//! while it is open.

use super::*;
use core::cell::Cell;
use paperpaw_sdk::SCREEN_REVISION;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{MemoryStorage, Params};

const READER: Id = Id::new_static("book.reader");
const MENU: Id = Id::new_static("book.menu");
const FULL: Id = Id::new_static("book.full");
const CLOSED: Id = Id::new_static("book.closed");
const BOOK_SETTINGS: &str = "books/a/settings.ini";

/// A reader that keeps a book's own settings while it is open and asks
/// for a timer when a setting changes; a menu over the lower half of the
/// screen and a full screen, each saying when its timer comes.
struct BookPlugin(Log, Asks);

/// What the reader says of its time below overlays
/// ([`Screen::times_under_overlays`]) and its revision.
type Asks = Rc<Cell<(bool, u16)>>;

struct ReaderScreen {
    log: Log,
    asks: Asks,
}

impl Screen for ReaderScreen {
    fn revision(&self) -> u16 {
        self.asks.get().1
    }

    fn times_under_overlays(&self) -> bool {
        self.asks.get().0
    }

    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::SetScope(Some(String::from(BOOK_SETTINGS))));
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        if *event == CLOSED {
            self.log.borrow_mut().push(String::from("closed said"));
        }
        if *event == events::SETTING_CHANGED {
            let at = cx.env().now().add_millis(10);
            cx.wake_at(at);
        }
    }

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        self.log.borrow_mut().push(String::from("reader close"));
        cx.command(Command::Emit(CLOSED, Params::new()));
    }

    fn on_timer(&mut self, _: &mut Cx<'_, '_>) {
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

impl Screen for Other {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        self.region
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_timer(&mut self, _: &mut Cx<'_, '_>) {
        self.log
            .borrow_mut()
            .push(alloc::format!("{} timer", self.name));
    }
}

impl Plugin for BookPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("book"), "Book", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (log, asks) = (self.0.clone(), self.1.clone());
        r.add_screen(ScreenSpec::new(
            READER,
            "reader",
            Box::new(move |_, _| {
                Box::new(ReaderScreen {
                    log: log.clone(),
                    asks: asks.clone(),
                })
            }),
        ))?;
        r.bind_slot(slots::HOME, READER, Priority::DEFAULT)?;
        for (id, name, region) in [
            (MENU, "menu", Some(Rect::new(0, 45, 90, 45))),
            (FULL, "full", None),
        ] {
            let log = self.0.clone();
            r.add_screen(ScreenSpec::new(
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

fn book_kernel(log: &Log) -> Kernel {
    book_kernel_asking(log, &Rc::new(Cell::new((false, SCREEN_REVISION))))
}

fn book_kernel_asking(log: &Log, asks: &Asks) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(BookPlugin(log.clone(), asks.clone()))
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

/// A screen made anew for another size is told first, as when it is
/// closed, so it keeps what it would lose (the reader its position); what
/// it asks for then is not done, as it stays open.
#[test]
fn screens_made_anew_keep_what_they_would_lose() {
    let log = log();
    let mut kernel = book_kernel(&log);
    kernel.resize(Instant(1), 80, 90);
    assert_eq!(*log.borrow(), ["reader close"]);
    kernel.open_screen(Instant(2), FULL, Params::new());
    log.borrow_mut().clear();
    kernel.resize(Instant(3), 90, 90);
    assert_eq!(*log.borrow(), ["reader close"], "the one below too");
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

/// Below overlays alone (the menu bar over a page), a screen that asks
/// for it has its time (the reader counting its pages, #117); one that
/// does not, one built before the item (revision 2), and any below a full
/// screen wait until they are on top again.
#[test]
fn timers_come_below_overlays_alone_to_a_screen_that_asks() {
    let log = log();
    let asks: Asks = Rc::new(Cell::new((true, SCREEN_REVISION)));
    let mut kernel = book_kernel_asking(&log, &asks);
    kernel.open_screen(Instant(1), MENU, Params::new());
    kernel
        .set_setting(Instant(2), "core.night-mode", "on")
        .unwrap();
    assert_eq!(kernel.tick(Instant(5)).wake_at(), Some(Instant(12)));
    kernel.tick(Instant(12));
    assert_eq!(*log.borrow(), ["reader timer"]);
    log.borrow_mut().clear();
    // Below a full screen, also with a menu over that: it waits.
    kernel.run_action(Instant(15), actions::BACK);
    kernel.open_screen(Instant(20), FULL, Params::new());
    kernel.open_screen(Instant(21), MENU, Params::new());
    kernel
        .set_setting(Instant(22), "core.night-mode", "off")
        .unwrap();
    assert_eq!(kernel.tick(Instant(32)).wake_at(), None);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    kernel.run_action(Instant(40), actions::BACK);
    kernel.run_action(Instant(41), actions::BACK);
    kernel.tick(Instant(41));
    assert_eq!(*log.borrow(), ["reader timer"]);
    // Below the menu again, not asking, or asking but built before.
    for (at, said) in [(100, (false, SCREEN_REVISION)), (200, (true, 2))] {
        log.borrow_mut().clear();
        asks.set(said);
        kernel.open_screen(Instant(at), MENU, Params::new());
        let value = if at == 100 { "on" } else { "off" };
        kernel
            .set_setting(Instant(at + 1), "core.night-mode", value)
            .unwrap();
        assert_eq!(kernel.tick(Instant(at + 11)).wake_at(), None, "{said:?}");
        assert!(log.borrow().is_empty(), "{said:?}: {:?}", log.borrow());
        kernel.run_action(Instant(at + 20), actions::BACK);
        kernel.tick(Instant(at + 20));
        assert_eq!(*log.borrow(), ["reader timer"], "{said:?}");
    }
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
    kernel.apply(Command::PreviewSettings(tried));
    kernel.finish();
    assert!(kernel.settings().bool("core.night-mode"));
    kernel.resize(Instant(2), 80, 90);
    assert!(kernel.settings().bool("core.night-mode"), "still tried");
    kernel.run_action(Instant(3), actions::BACK);
    assert!(!kernel.settings().bool("core.night-mode"));
}
