//! The end-of-book screen with the real kernel and the library: what the
//! rows say, what a press of Select does, and what the library keeps.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::library::{self, Provided};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, FormatSpec, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, PixelFormat, Plugin, RegisterError, Registrar, Version,
};

const X4: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

/// Knows text files as books (named only: the library's own notes of them
/// stand), so the library has them.
struct Text;

impl Plugin for Text {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("text"), "Text", Version::new(1, 0, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_format(FormatSpec::new(
            Id::new_static("text.txt"),
            "TXT",
            vec![std::borrow::Cow::Borrowed("txt")],
            Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
    }
}

const ONE: &str = "books/Moor 1.txt";
const TWO: &str = "books/Moor 2.txt";
const THREE: &str = "books/Moor 3.txt";
const LONE: &str = "books/Lone.txt";

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    /// A card with a trilogy and a book of its own; `config` is added to
    /// the user's settings.
    fn new(config: &str) -> Self {
        Self::with_series(config, 3)
    }

    /// [`Device::new`] with `n` books in the series.
    fn with_series(config: &str, n: usize) -> Self {
        let mut storage = MemoryStorage::new().with(
            LONE,
            "A Quiet Year. The wind came over the hills.\n".repeat(40),
        );
        let titles = ["The Salt Marsh", "The Grey Tide", "The Last Light"];
        for i in 1..=n {
            let path = format!("books/Moor {i}.txt");
            let title = titles
                .get(i - 1)
                .map_or(format!("Moor Book {i}"), |t| (*t).to_string());
            storage = storage.with(
                &path,
                format!("{title}. The wind came over the hills.\n").repeat(40),
            );
            library::provide(
                &storage,
                &path,
                Provided::default()
                    .title(title)
                    .series("The Moor Trilogy".to_string())
                    .series_index(i.to_string()),
            );
        }
        let user = format!("[core]\nlanguage = en\n[help]\nshow = false\n{config}\n");
        let kernel = Kernel::builder(X4)
            .storage(storage)
            .user_config(user)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_files::Files)
            .plugin(Text)
            .plugin(paperpaw_plugin_end_of_book::EndOfBook)
            .build()
            .unwrap();
        let mut device = Self { kernel, now: 0 };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        // The library read once: its catalog has the books.
        device.now += 100;
        let wanted = events::CatalogWanted::new(false).to_params();
        let effects = device.kernel.send_event(
            Instant(device.now),
            events::CATALOG_WANTED.id().clone(),
            wanted,
        );
        device.settle(effects);
        device.settle_for(3000);
        device
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            match effects.wake_at() {
                Some(at) if at.0 <= self.now + 1000 => {
                    self.now = self.now.max(at.0);
                    effects = self.kernel.tick(Instant(self.now));
                }
                _ => return,
            }
        }
    }

    fn settle_for(&mut self, millis: u64) {
        self.now += millis;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
    }

    fn run(&mut self, action: paperpaw_sdk::Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn send(
        &mut self,
        event: &paperpaw_sdk::event::Event<events::Reading>,
        path: &str,
        percent: f32,
    ) {
        self.now += 100;
        let reading =
            events::Reading::new(path.into(), String::new(), percent, 0, 0, String::new());
        let effects =
            self.kernel
                .send_event(Instant(self.now), event.id().clone(), reading.to_params());
        self.settle(effects);
    }

    fn finish(&mut self, path: &str) {
        self.send(&events::BOOK_OPENED, path, 0.0);
        self.send(&events::BOOK_FINISHED, path, 100.0);
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .iter()
            .map(|p| format!("{}\n", p.label))
            .collect()
    }

    fn book(&self, path: &str) -> library::Book {
        library::book(self.kernel.storage(), path).expect("book")
    }
}

const RATING: &str = "end-of-book.rating";

impl Device {
    fn press(&mut self, action: paperpaw_sdk::Id) {
        self.run(action);
    }

    fn progress(&self, path: &str) -> Option<u8> {
        self.book(path).progress
    }

    fn rating(&self, path: &str) -> Option<String> {
        self.book(path).field(RATING).map(String::from)
    }
}

#[test]
fn the_screen_offers_the_next_book_of_the_series() {
    let mut device = Device::new("");
    device.finish(ONE);
    let shown = device.shown();
    assert!(shown.contains("The end"), "{shown}");
    assert!(shown.contains("Read the next book"), "{shown}");
    assert!(shown.contains("The Grey Tide"), "{shown}");
    assert!(shown.contains("(2 of 3)"), "{shown}");
    assert!(shown.contains("Not rated"), "{shown}");
}

#[test]
fn a_book_without_a_series_and_the_last_of_a_series_offer_the_library() {
    for path in [LONE, THREE] {
        let mut device = Device::new("");
        device.finish(path);
        let shown = device.shown();
        assert!(shown.contains("Choose another book"), "{path}: {shown}");
        assert!(!shown.contains("Read the next book"), "{path}: {shown}");
    }
}

#[test]
fn the_next_book_is_not_one_read_already_or_gone_from_the_card() {
    let mut device = Device::new("");
    // Book 2 is read already: book 3 is next.
    library::set_finished(device.kernel.storage(), TWO, true);
    device.finish(ONE);
    assert!(
        device.shown().contains("The Last Light"),
        "{}",
        device.shown()
    );
    // Book 3 is deleted: nothing is next.
    let mut device = Device::new("");
    library::set_finished(device.kernel.storage(), TWO, true);
    device.kernel.storage().remove(THREE).unwrap();
    device.finish(ONE);
    let shown = device.shown();
    assert!(shown.contains("Choose another book"), "{shown}");
}

#[test]
fn a_book_that_is_gone_has_no_screen() {
    let mut device = Device::new("");
    device.kernel.storage().remove(ONE).unwrap();
    device.finish(ONE);
    assert!(!device.shown().contains("The end"));
    assert!(library::book(device.kernel.storage(), ONE).is_none());
}

#[test]
fn the_book_counts_as_finished_and_stays_so_when_it_closes() {
    let mut device = Device::new("");
    device.finish(ONE);
    assert_eq!(device.progress(ONE), Some(100));
    assert!(
        device.shown().contains("Finished, Yes"),
        "{}",
        device.shown()
    );
    // The reader's closing records how far the page's start is.
    device.send(&events::BOOK_CLOSED, ONE, 96.0);
    assert_eq!(device.progress(ONE), Some(100));
}

#[test]
fn the_third_row_undoes_finished_and_the_book_stays_unfinished_when_it_closes() {
    let mut device = Device::new("");
    device.finish(ONE);
    device.press(actions::RIGHT);
    device.press(actions::RIGHT);
    device.press(actions::CONFIRM);
    assert_eq!(device.progress(ONE), None);
    assert!(
        device.shown().contains("Finished, No"),
        "{}",
        device.shown()
    );
    device.send(&events::BOOK_CLOSED, ONE, 96.0);
    assert_eq!(device.progress(ONE), None);
    // And again: Yes.
    let mut device = Device::new("");
    device.finish(ONE);
    for _ in 0..2 {
        device.press(actions::RIGHT);
    }
    device.press(actions::CONFIRM);
    device.press(actions::CONFIRM);
    assert_eq!(device.progress(ONE), Some(100));
    device.send(&events::BOOK_CLOSED, ONE, 96.0);
    assert_eq!(device.progress(ONE), Some(100));
}

#[test]
fn a_rating_goes_from_one_to_five_and_is_not_rated_again_and_is_kept() {
    let mut device = Device::new("");
    device.finish(ONE);
    device.press(actions::RIGHT);
    for stars in 1..=5 {
        device.press(actions::CONFIRM);
        assert_eq!(device.rating(ONE), Some(stars.to_string()));
        assert!(
            device.shown().contains(&format!("{stars} / 5")),
            "{}",
            device.shown()
        );
    }
    device.press(actions::CONFIRM);
    assert_eq!(device.rating(ONE), None);
    assert!(device.shown().contains("Not rated"));
    // Four, closed, and the book finished again later: it is still four.
    for _ in 0..4 {
        device.press(actions::CONFIRM);
    }
    device.send(&events::BOOK_CLOSED, ONE, 96.0);
    assert_eq!(device.rating(ONE), Some("4".into()));
    device.finish(ONE);
    assert!(device.shown().contains("4 / 5"), "{}", device.shown());
}

#[test]
fn the_settings_say_whether_a_screen_opens_and_whether_the_book_is_marked() {
    // No screen, the book marked all the same.
    let mut device = Device::new("[end-of-book]\nshow = false");
    device.finish(ONE);
    assert!(!device.shown().contains("The end"));
    assert_eq!(device.progress(ONE), Some(100));
    // Not marked: the row says No, and the book stays unfinished.
    let mut device = Device::new("[end-of-book]\nmark = false");
    device.finish(ONE);
    assert!(
        device.shown().contains("Finished, No"),
        "{}",
        device.shown()
    );
    device.send(&events::BOOK_CLOSED, ONE, 96.0);
    assert_eq!(device.progress(ONE), None);
}

#[test]
fn german_texts() {
    let mut device = Device::new("[core]\nlanguage = de");
    device.finish(ONE);
    let shown = device.shown();
    assert!(shown.contains("Das Ende"), "{shown}");
    assert!(shown.contains("Nächstes Buch lesen"), "{shown}");
    assert!(shown.contains("(2 von 3)"), "{shown}");
}

mod heap {
    //! What a thread asks of the allocator, counted while it is on.
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub struct Counting;

    std::thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
        static LIVE: Cell<isize> = const { Cell::new(0) };
        static PEAK: Cell<isize> = const { Cell::new(0) };
    }

    fn note(size: usize, grow: bool) {
        let _ = ON.try_with(|on| {
            if !on.get() {
                return;
            }
            let _ = LIVE.try_with(|live| {
                let now = live.get()
                    + if grow {
                        size as isize
                    } else {
                        -(size as isize)
                    };
                live.set(now);
                let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
            });
        });
    }

    // SAFETY: only counts, and passes every call to the system allocator.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc(layout) };
            if !p.is_null() {
                note(layout.size(), true);
            }
            p
        }
        unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
            unsafe { System.dealloc(p, layout) };
            note(layout.size(), false);
        }
        unsafe fn realloc(&self, p: *mut u8, layout: Layout, new: usize) -> *mut u8 {
            let q = unsafe { System.realloc(p, layout, new) };
            if !q.is_null() {
                note(layout.size(), false);
                note(new, true);
            }
            q
        }
    }

    /// The most held at once, above what was held at the start, while
    /// `work` runs.
    pub fn peak(work: impl FnOnce()) -> usize {
        ON.with(|on| on.set(true));
        LIVE.with(|v| v.set(0));
        PEAK.with(|v| v.set(0));
        work();
        ON.with(|on| on.set(false));
        usize::try_from(PEAK.with(Cell::get)).unwrap_or(0)
    }
}

#[global_allocator]
static COUNTING: heap::Counting = heap::Counting;

#[test]
fn the_screen_takes_a_small_heap_of_a_long_series() {
    // 200 books in the series, this the first: the next book is found on
    // the first page of the library's answer; this the 150th, on the
    // fourth.
    for (here, label) in [(1, "first"), (150, "150th")] {
        let mut device = Device::with_series("", 200);
        let path = format!("books/Moor {here}.txt");
        device.send(&events::BOOK_OPENED, &path, 0.0);
        let peak = heap::peak(|| device.send(&events::BOOK_FINISHED, &path, 100.0));
        let shown = device.shown();
        assert!(shown.contains("Read the next book"), "{shown}");
        std::eprintln!("end of book, the {label} of 200: {peak} bytes at most at once");
        assert!(peak <= 24 * 1024, "{peak} bytes at once");
    }
}
