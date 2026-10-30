//! Recording reading, with the real kernel and a stand-in reader that
//! says where it is by the standard reading events alone: the stays on
//! pages as the statistics log them, with and without the time of day,
//! and the views while the device does not know it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::{Rotation, SystemStatus, WallClock};
use paperpaw_sdk::builtin::{self, events, slots};
use paperpaw_sdk::{
    ActionSpec, Canvas, Commands, Cx, DeviceInfo, DisplayInfo, Env, Handled, Id, InputCaps,
    Instant, Manifest, MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, Priority,
    RegisterError, Registrar, SDK_VERSION, Screen, ScreenSpec, Storage, StorageError,
};

/// An X4: black and white, buttons.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// 14 October 2026, 10:41 UTC.
const NOW: i64 = 1_791_967_260;
const DIR: &str = ".paperpaw/plugins/stats";
const ALICE: &str = "books/alice.epub";
const EFFI: &str = "books/effi.epub";
const OPEN: Id = Id::new_static("stand-in.open");
const TURN: Id = Id::new_static("stand-in.turn");
const CLOSE: Id = Id::new_static("stand-in.close");
const NOTICE: Id = Id::new_static("stand-in.notice");

/// A reader that only says where it is: the book and page in `0`; and
/// the notices shown, kept in `1`.
struct StandIn(Rc<RefCell<events::Reading>>, Notices);

type Notices = Rc<RefCell<Vec<String>>>;

/// A notice, shown as nothing.
struct Notice;

impl Screen for Notice {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for StandIn {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("stand-in"), "Stand-in", SDK_VERSION)
            .description("Says where a book is read.")
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (id, event) in [
            (OPEN, events::BOOK_OPENED),
            (TURN, events::PAGE_TURNED),
            (CLOSE, events::BOOK_CLOSED),
        ] {
            let reading = self.0.clone();
            r.add_action(ActionSpec::new(id, "stand-in.action").handler(
                move |_: &Env<'_>, _, out: &mut Commands| {
                    out.push(event.emit(reading.borrow().clone()));
                },
            ))?;
        }
        let notices = self.1.clone();
        r.add_screen(ScreenSpec::new(
            NOTICE,
            "stand-in.notice",
            Box::new(move |_, params| {
                let text = params.get(builtin::params::TEXT).unwrap_or_default();
                notices.borrow_mut().push(text.to_string());
                Box::new(Notice)
            }),
        ))?;
        r.bind_slot(slots::NOTICE, NOTICE, Priority::DEFAULT)?;
        Ok(())
    }
}

/// Page `page` of 300 of the book at `path`.
fn page(path: &str, page: u32) -> events::Reading {
    events::Reading::new(
        path.into(),
        String::new(),
        page as f32 / 3.0,
        0,
        page * 1000,
        String::new(),
    )
    .page(page)
    .pages(300)
}

/// A stay as logged: book, page, start (seconds after [`NOW`]) and
/// duration.
type Stay = (u16, u32, i64, u16);

/// A card that fills up when told to: writing then fails.
struct Card {
    files: MemoryStorage,
    full: Rc<Cell<bool>>,
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.full.get() {
            return Err(StorageError::Full);
        }
        self.files.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.full.get() {
            return Err(StorageError::Full);
        }
        self.files.append(path, data)
    }

    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }
}

struct Device {
    kernel: Kernel,
    /// Whether the card is full.
    full: Rc<Cell<bool>>,
    notices: Notices,
    reading: Rc<RefCell<events::Reading>>,
    /// Milliseconds since the start.
    now: u64,
}

impl Device {
    /// A device with two books on it, knowing the time (at [`NOW`] at
    /// the start) or not, with the home screen and `page_1` as its first
    /// page.
    fn new(clock: bool, page_1: &str) -> Self {
        let full = Rc::new(Cell::new(false));
        let storage = Card {
            files: MemoryStorage::new()
                .with(ALICE, "Alice's Adventures in Wonderland")
                .with(EFFI, "Effi Briest"),
            full: full.clone(),
        };
        let user = format!(
            "[core]
language = en
[home]
page-1 = \"{page_1}\"
"
        );
        let reading = Rc::new(RefCell::new(page(ALICE, 1)));
        let notices = Notices::default();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(user)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(paperpaw_plugin_stats::Stats)
            .plugin(StandIn(reading.clone(), notices.clone()))
            .build()
            .unwrap();
        if clock {
            kernel.set_status(Instant(0), status(0));
        }
        kernel.start(Instant(0));
        Self {
            kernel,
            full,
            notices,
            reading,
            now: 0,
        }
    }

    fn at(&mut self, seconds: u64) -> Instant {
        assert!(seconds * 1000 >= self.now, "time runs on");
        self.now = seconds * 1000;
        Instant(self.now)
    }

    /// At `seconds`, the reader shows `reading` (as it opens a book).
    fn open(&mut self, seconds: u64, reading: events::Reading) {
        *self.reading.borrow_mut() = reading;
        let at = self.at(seconds);
        self.kernel.run_action(at, OPEN);
    }

    /// At `seconds`, the reader turns to `reading`.
    fn turn(&mut self, seconds: u64, reading: events::Reading) {
        *self.reading.borrow_mut() = reading;
        let at = self.at(seconds);
        self.kernel.run_action(at, TURN);
    }

    /// At `seconds`, the reader closes its book.
    fn close(&mut self, seconds: u64) {
        let at = self.at(seconds);
        self.kernel.run_action(at, CLOSE);
    }

    fn suspend(&mut self, seconds: u64) {
        let at = self.at(seconds);
        self.kernel.suspend(at);
    }

    fn resume(&mut self, seconds: u64) {
        let at = self.at(seconds);
        self.kernel.resume(at);
    }

    /// At `seconds`, the device learns the time (as Wi-Fi sets it).
    fn set_clock(&mut self, seconds: u64) {
        let at = self.at(seconds);
        self.kernel.set_status(at, status(self.now));
    }

    /// The stays logged so far.
    fn stays(&self) -> Vec<Stay> {
        let storage = self.kernel.storage();
        let path = format!("{DIR}/pages.bin");
        let size = storage.size(&path).unwrap_or(0) as usize;
        let mut log = vec![0u8; size];
        if size > 0 {
            storage.read_at(&path, 0, &mut log).unwrap();
        }
        log.as_chunks::<16>()
            .0
            .iter()
            .map(|r| {
                let u32_at = |i: usize| u32::from_le_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]);
                (
                    u16::from_le_bytes([r[0], r[1]]),
                    u32_at(2),
                    i64::from(u32_at(6)) - NOW,
                    u16::from_le_bytes([r[10], r[11]]),
                )
            })
            .collect()
    }

    /// The books recorded, as `books.ini` has them.
    fn books(&self) -> String {
        let storage = self.kernel.storage();
        let path = format!("{DIR}/books.ini");
        let size = storage.size(&path).unwrap_or(0) as usize;
        let mut text = vec![0u8; size];
        if size > 0 {
            storage.read_at(&path, 0, &mut text).unwrap();
        }
        String::from_utf8(text).unwrap()
    }

    /// The texts of `screen` of the statistics, opened now.
    fn shown(&mut self, screen: &'static str) -> String {
        let at = Instant(self.now + 10);
        self.kernel
            .open_screen(at, Id::new_static(screen), Params::new());
        self.kernel.render(&mut |_, _| {});
        let mut text = String::new();
        for part in self.kernel.describe() {
            text.push_str(&part.label);
            text.push('\n');
        }
        text
    }
}

/// The wall clock: [`NOW`] at the start, read at `millis`.
fn status(millis: u64) -> SystemStatus {
    SystemStatus::new(
        Some(WallClock::new(
            Instant(millis),
            NOW * 1000 + millis as i64,
            Some(0),
        )),
        None,
    )
}

#[test]
fn pages_are_recorded_as_they_are_read() {
    let mut device = Device::new(true, "");
    device.open(1, page(ALICE, 1));
    device.turn(31, page(ALICE, 2));
    // Turned on after 3 seconds: not counted.
    device.turn(34, page(ALICE, 3));
    // An hour on one page counts as two minutes.
    device.turn(3634, page(ALICE, 4));
    device.close(3644);
    assert_eq!(
        device.stays(),
        [(1, 1, 1, 30), (1, 3, 34, 120), (1, 4, 3634, 10)]
    );
    let books = device.books();
    assert!(books.contains("path = books/alice.epub"), "{books}");
}

#[test]
fn time_outside_the_book_is_not_reading() {
    let mut device = Device::new(true, "");
    device.open(0, page(ALICE, 1));
    // Asleep while reading: the page pauses, and goes on after.
    device.suspend(20);
    device.resume(600);
    device.turn(640, page(ALICE, 2));
    device.close(660);
    // On the home screen, the book closed: nothing is read.
    device.suspend(700);
    device.resume(5000);
    device.suspend(5100);
    assert_eq!(
        device.stays(),
        [(1, 1, 0, 20), (1, 1, 600, 40), (1, 2, 640, 20)]
    );
}

#[test]
fn reading_before_the_clock_is_set_counts_once_it_is() {
    let mut device = Device::new(false, "");
    device.open(0, page(ALICE, 1));
    device.turn(30, page(ALICE, 2));
    device.close(60);
    device.open(70, page(EFFI, 7));
    device.turn(100, page(EFFI, 8));
    assert_eq!(device.stays(), []);
    assert!(!device.books().contains("alice"));
    // Wi-Fi sets the clock while the second book is read: the stays so
    // far are dated by it, and reading goes on being recorded.
    device.set_clock(110);
    device.turn(130, page(EFFI, 9));
    device.close(140);
    assert_eq!(
        device.stays(),
        [
            (1, 1, 0, 30),
            (1, 2, 30, 30),
            (2, 7, 70, 30),
            (2, 8, 100, 30),
            (2, 9, 130, 10)
        ]
    );
    let books = device.books();
    assert!(books.contains("path = books/alice.epub"), "{books}");
    assert!(books.contains("path = books/effi.epub"), "{books}");
}

#[test]
fn pages_counted_by_the_progress_are_told_apart() {
    // Before the reader counted the pages: a page in a thousand, by the
    // progress, which two pages of a long book may share.
    let estimated = |progress: f32, offset: u32| {
        let mut reading = page(ALICE, 1);
        reading.page = None;
        reading.pages = None;
        reading.progress = progress;
        reading.offset = offset;
        reading
    };
    let mut device = Device::new(true, "");
    device.open(0, estimated(10.0, 0));
    device.turn(30, estimated(10.0, 800));
    device.turn(50, estimated(10.1, 1600));
    assert_eq!(device.stays(), [(1, 101, 0, 30), (1, 101, 30, 20)]);
}

#[test]
fn without_the_clock_the_views_say_so_and_show_no_1970() {
    let mut device = Device::new(true, "");
    device.open(0, page(ALICE, 1));
    device.turn(60, page(ALICE, 2));
    // Switched off and on again: the X4 does not know the time.
    let storage = MemoryStorage::new();
    for name in ["pages.bin", "books.ini"] {
        let path = format!("{DIR}/{name}");
        let source = device.kernel.storage();
        let mut data = vec![0u8; source.size(&path).unwrap() as usize];
        source.read_at(&path, 0, &mut data).unwrap();
        storage.write(&path, &data).unwrap();
    }
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[core]\nlanguage = en\n")
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_stats::Stats)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let mut device = Device {
        kernel,
        full: device.full,
        notices: device.notices,
        reading: device.reading,
        now: 0,
    };
    let overview = device.shown("stats.overview");
    assert!(
        overview.contains("Statistics need the time of day"),
        "{overview}"
    );
    assert!(overview.contains("1 min in all"), "{overview}");
    let calendar = device.shown("stats.calendar");
    assert!(calendar.contains("October 2026"), "{calendar}");
    assert!(!calendar.contains("1970"), "{calendar}");
}

#[test]
fn the_goal_widget_is_offered_once() {
    let mut device = Device::new(true, "home.continue");
    let page_1 = |device: &Device| device.kernel.settings().text("home.page-1").to_string();
    device
        .kernel
        .set_setting(Instant(10), "stats.goal", "stats.goal.30")
        .unwrap();
    assert_eq!(page_1(&device), "home.continue, stats.widget.today");
    // Removed by the user: another goal does not bring it back.
    device
        .kernel
        .set_setting(Instant(20), "home.page-1", "home.continue")
        .unwrap();
    device
        .kernel
        .set_setting(Instant(30), "stats.goal", "stats.goal.45")
        .unwrap();
    assert_eq!(page_1(&device), "home.continue");
}

#[test]
fn reading_is_kept_while_the_card_is_full() {
    let mut device = Device::new(true, "");
    device.open(0, page(ALICE, 1));
    device.turn(30, page(ALICE, 2));
    device.full.set(true);
    device.turn(60, page(ALICE, 3));
    device.turn(90, page(ALICE, 4));
    assert_eq!(device.stays(), [(1, 1, 0, 30)]);
    // Said once, not at every page.
    let notices = device.notices.borrow().clone();
    assert_eq!(
        notices,
        ["The reading statistics could not be saved. Is the card full?"]
    );
    // Room again: what could not be written follows.
    device.full.set(false);
    device.turn(120, page(ALICE, 5));
    assert_eq!(
        device.stays(),
        [
            (1, 1, 0, 30),
            (1, 2, 30, 30),
            (1, 3, 60, 30),
            (1, 4, 90, 30)
        ]
    );
}
