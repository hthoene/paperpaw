//! The notes overview with the real kernel and library: which books it
//! lists, what a row says, and the `My Clippings.txt` it writes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::annotations::{self, Annotation, Kind, Mark};
use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::document::{DocumentError, Locator};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::library::{self, Provided};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, FormatSpec, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, PixelFormat, Plugin, RegisterError, Registrar, Storage, Version,
};

const X4: DeviceInfo = DeviceInfo::new(
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

/// Knows text files as books (named only).
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

const TIDE: &str = "books/The Grey Tide.txt";
const MARSH: &str = "books/The Salt Marsh.txt";
const BARE: &str = "books/Bare.txt";
const SEEN: i64 = 1_793_527_500; // Sunday, 1 November 2026 10:05:00 UTC

fn note(kind: Kind, section: u32, percent: u8, text: &str, words: &str) -> Annotation {
    let start = Locator::new(section, 5);
    let end = Locator::new(section, 5 + text.len() as u32);
    Annotation::new(
        Mark::new(kind, start, end, !words.is_empty()),
        percent,
        SEEN,
        SEEN,
        text,
        words,
    )
}

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    /// A card with three books: two with notes, one without.
    fn new(with_notes: bool) -> Self {
        let storage = MemoryStorage::new();
        for (path, title, author) in [
            (TIDE, "The Grey Tide", "Ann Reed"),
            (MARSH, "The Salt Marsh", ""),
            (BARE, "Bare Book", "Bo Bare"),
        ] {
            let text = format!("{title}. The wind came over the hills.\n").repeat(40);
            storage.write(path, text.as_bytes()).unwrap();
            library::provide(
                &storage,
                path,
                Provided::default()
                    .title(title.to_string())
                    .author((!author.is_empty()).then(|| author.to_string())),
            );
        }
        if with_notes {
            annotations::add(
                &storage,
                TIDE,
                note(
                    Kind::Highlight,
                    1,
                    42,
                    "The wind came over\nthe hills",
                    "so true",
                ),
            )
            .unwrap();
            annotations::add(
                &storage,
                TIDE,
                note(Kind::Bookmark, 2, 77, "Chapter two", ""),
            )
            .unwrap();
            annotations::add(
                &storage,
                TIDE,
                note(Kind::Highlight, 0, 10, "Grey water", ""),
            )
            .unwrap();
            annotations::add(&storage, MARSH, note(Kind::Bookmark, 0, 5, "Start", "")).unwrap();
            // The notes of a book that is not on the card any more.
            annotations::add(
                &storage,
                "books/Gone.txt",
                note(Kind::Highlight, 0, 1, "Lost", ""),
            )
            .unwrap();
        }
        let user = "[core]\nlanguage = en\n".to_string();
        let kernel = Kernel::builder(X4)
            .storage(storage)
            .user_config(user)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_files::Files)
            .plugin(Text)
            .plugin(paperpaw_plugin_notes_overview::NotesOverview)
            .build()
            .unwrap();
        let mut device = Self { kernel, now: 0 };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
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

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .iter()
            .map(|p| format!("{}\n", p.label))
            .collect()
    }

    fn file(&self, path: &str) -> Option<Vec<u8>> {
        let storage = self.kernel.storage();
        let size = storage.size(path).ok()?;
        let mut data = vec![0; size as usize];
        let n = storage.read_at(path, 0, &mut data).ok()?;
        data.truncate(n);
        Some(data)
    }

    /// Opens the list and moves to the export row.
    fn export(&mut self) {
        self.run(paperpaw_plugin_notes_overview::OPEN);
        // Two books, then the export row.
        self.run(actions::DOWN);
        self.run(actions::DOWN);
        self.run(actions::CONFIRM);
    }
}

#[test]
fn the_books_with_notes_are_listed_by_title_with_their_counts() {
    let mut device = Device::new(true);
    device.run(paperpaw_plugin_notes_overview::OPEN);
    let shown = device.shown();
    assert!(shown.contains("All notes"), "{shown}");
    let tide = shown.find("The Grey Tide").expect(&shown);
    let marsh = shown.find("The Salt Marsh").expect(&shown);
    assert!(tide < marsh, "{shown}");
    assert!(shown.contains("2 highlights · 1 bookmark"), "{shown}");
    assert!(shown.contains("1 bookmark"), "{shown}");
    assert!(shown.contains("Export as My Clippings.txt"), "{shown}");
    // No notes, or the book is gone: not listed.
    assert!(!shown.contains("Bare Book"), "{shown}");
    assert!(!shown.contains("Gone"), "{shown}");
}

#[test]
fn without_notes_the_list_says_so_and_offers_no_export() {
    let mut device = Device::new(false);
    device.run(paperpaw_plugin_notes_overview::OPEN);
    let shown = device.shown();
    assert!(shown.contains("No notes yet"), "{shown}");
    assert!(!shown.contains("Export"), "{shown}");
}

#[test]
fn a_book_shows_its_notes() {
    let mut device = Device::new(true);
    device.run(paperpaw_plugin_notes_overview::OPEN);
    device.run(actions::CONFIRM);
    let shown = device.shown();
    assert!(shown.contains("The Grey Tide"), "{shown}");
    assert!(shown.contains("Grey water"), "{shown}");
    assert!(shown.contains("so true"), "{shown}");
    assert!(shown.contains("Chapter two"), "{shown}");
    assert!(shown.contains("77 %"), "{shown}");
}

#[test]
fn the_export_is_a_kindle_clippings_file() {
    let mut device = Device::new(true);
    device.export();
    let bytes = device.file("Exports/My Clippings.txt").expect("file");
    assert_eq!(&bytes[..3], [0xEF, 0xBB, 0xBF], "a byte order mark");
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    assert!(text.contains("\r\n"));
    assert!(!text.replace("\r\n", "").contains('\n'), "{text:?}");
    // Library order: each book's notes by position, a note on a highlight
    // as an entry of its own.
    let entries: Vec<&str> = text.split("==========\r\n").collect();
    assert_eq!(entries.len(), 6, "{text:?}");
    assert_eq!(entries[5], "");
    let tide: Vec<&&str> = entries
        .iter()
        .filter(|e| e.starts_with("The Grey Tide (Ann Reed)\r\n"))
        .collect();
    assert_eq!(tide.len(), 4, "{text:?}");
    assert!(
        text.contains(
            "The Grey Tide (Ann Reed)\r\n- Your Highlight at location 42-42 | Added on Sunday, 1 November 2026 10:05:00\r\n\r\nThe wind came over the hills\r\n"
        ),
        "{text:?}"
    );
    assert!(
        text.contains("- Your Note at location 42 | Added on"),
        "{text:?}"
    );
    assert!(text.contains("so true\r\n"), "{text:?}");
    assert!(
        text.contains("The Salt Marsh\r\n- Your Bookmark at location 5 |"),
        "{text:?}"
    );
    assert!(!text.contains("Lost"), "{text:?}");
    assert!(!text.contains("Bare Book"), "{text:?}");
    // The part file is gone.
    assert!(device.file("Exports/My Clippings.txt.part").is_none());
}

#[test]
fn exporting_again_replaces_the_file() {
    let mut device = Device::new(true);
    device.export();
    let first = device.file("Exports/My Clippings.txt").unwrap();
    device.export();
    // The list is open twice now: the file is the same all the same.
    assert_eq!(device.file("Exports/My Clippings.txt").unwrap(), first);
}
