//! The KOReader plugin in the real kernel: opening a book takes over
//! KOReader's highlights and bookmarks at the right positions, changes
//! here are passed on, a moved book takes its KOReader folder along, and
//! hostile `metadata.epub.lua` files (garbage, deep nesting, wrong types,
//! positions nowhere, huge files) change nothing and break nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_format_epub::EpubFormat;
use paperpaw_plugin_koreader::KoReader;
use paperpaw_plugin_koreader::lua::{self, Value};
use paperpaw_sdk::annotations::{self, Annotation, Kind, Mark};
use paperpaw_sdk::builtin::events::{self, AnnotationsChanged, BookMoved, Reading};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, PixelFormat, Plugin, RegisterError, Registrar, Storage, StorageError, Version,
};
use paperpaw_zip::testing::ZipBuilder;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(300, 400, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

const BOOK: &str = "books/alice.epub";
const SIDECAR: &str = "books/alice.sdr/metadata.epub.lua";
const EXCHANGE: &str = "books/alice.sdr/alice.epub.annotations.lua";

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Alice</dc:title></metadata>
  <manifest>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#;

const CH1: &str = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>\
    <p>Alice was beginning to get very tired.</p>\
    <p>So she was considering in her own mind.</p></body></html>";
const CH2: &str = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>\
    <p>Down, down, down. Would the fall never come to an end?</p></body></html>";

/// A highlight of "Alice" with a note, and a bookmark in chapter 2.
const METADATA: &str = r#"-- we can read Lua syntax here!
return {
    ["annotations"] = {
        [1] = {
            ["datetime"] = "2026-10-01 10:00:00",
            ["drawer"] = "lighten",
            ["note"] = "Her name ✓",
            ["page"] = "/body/DocFragment[1]/body/p[1]/text().0",
            ["pos0"] = "/body/DocFragment[1]/body/p[1]/text().0",
            ["pos1"] = "/body/DocFragment[1]/body/p[1]/text().5",
            ["text"] = "Alice",
        },
        [2] = {
            ["datetime"] = "2026-10-01 10:05:00",
            ["page"] = "/body/DocFragment[2]/body/p[1]/text().6",
        },
    },
}
"#;

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

/// Counts the books whose annotations were said to have changed.
struct Listener(Rc<RefCell<Vec<String>>>);

impl Plugin for Listener {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("listener"),
            "Listener",
            Version::new(0, 1, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let heard = self.0.clone();
        r.on(events::ANNOTATIONS_CHANGED, move |_, changed, _| {
            heard.borrow_mut().push(changed.path.clone())
        })
    }
}

fn epub() -> Vec<u8> {
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", CONTAINER)
        .deflated("OEBPS/content.opf", OPF)
        .deflated("OEBPS/ch1.xhtml", CH1)
        .deflated("OEBPS/ch2.xhtml", CH2)
        .build()
}

struct Device {
    kernel: Kernel,
    storage: Shared,
    changed: Rc<RefCell<Vec<String>>>,
    now: u64,
}

impl Device {
    /// The book, and KOReader's file `metadata` beside it if given.
    fn new(metadata: Option<&[u8]>) -> Self {
        let storage = Shared::default();
        storage.write(BOOK, &epub()).unwrap();
        if let Some(metadata) = metadata {
            storage.write(SIDECAR, metadata).unwrap();
        }
        let changed = Rc::default();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .plugin(EpubFormat)
            .plugin(KoReader)
            .plugin(Listener(Rc::clone(&changed)))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            storage,
            changed,
            now: 0,
        }
    }

    fn send<T: EventData>(&mut self, event: paperpaw_sdk::event::Event<T>, data: &T) {
        self.now += 100;
        self.kernel
            .send_event(Instant(self.now), event.id().clone(), data.to_params());
        // What was read is kept until a frame is drawn.
        self.kernel.render(&mut |_, _| {});
    }

    fn open(&mut self) {
        let reading = Reading::new(
            BOOK.to_string(),
            String::from("Alice"),
            0.0,
            0,
            0,
            String::new(),
        );
        self.send(events::BOOK_OPENED, &reading);
    }

    /// The reader answers the question shown: remove, or keep (where the
    /// focus starts).
    fn answer(&mut self, remove: bool) {
        for action in [
            remove.then_some(paperpaw_sdk::builtin::actions::LEFT),
            Some(paperpaw_sdk::builtin::actions::CONFIRM),
        ]
        .into_iter()
        .flatten()
        {
            self.now += 100;
            self.kernel.run_action(Instant(self.now), action);
            self.kernel.render(&mut |_, _| {});
        }
    }

    /// Whether a question shows (about KOReader's empty file: the only
    /// one this plugin asks).
    fn asked(&mut self) -> bool {
        self.kernel.active_screen().map(Id::as_str) == Some("core.confirm-dialog")
    }

    fn ours(&self) -> Vec<Annotation> {
        annotations::load(&self.storage, BOOK)
    }

    /// The annotations in the file for KOReader.
    fn passed(&self) -> Vec<lua::Table> {
        let Some(data) = self.storage.0.get(EXCHANGE) else {
            return Vec::new();
        };
        let Some(Value::Table(root)) = lua::parse(&String::from_utf8(data).unwrap()) else {
            panic!("not Lua");
        };
        let Some(Value::Table(items)) = root.get("annotations") else {
            panic!("no annotations");
        };
        items
            .list()
            .map(|v| match v {
                Value::Table(t) => t.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    fn locate(&self, xpointer: &str) -> paperpaw_sdk::document::Locator {
        let doc = self.kernel.formats().open(&self.storage, BOOK).unwrap();
        doc.locate_xpointer(&self.storage, xpointer).unwrap()
    }
}

#[test]
fn opening_a_book_takes_over_koreaders_annotations_where_they_are() {
    let mut device = Device::new(Some(METADATA.as_bytes()));
    device.open();
    let ours = device.ours();
    assert_eq!(ours.len(), 2, "{ours:?}");
    let highlight = ours
        .iter()
        .find(|a| a.mark.kind == Kind::Highlight)
        .unwrap();
    assert_eq!(
        highlight.mark.start,
        device.locate("/body/DocFragment[1]/body/p[1]/text().0")
    );
    assert_eq!(
        highlight.mark.end,
        device.locate("/body/DocFragment[1]/body/p[1]/text().5")
    );
    assert_eq!(highlight.text, "Alice");
    assert_eq!(highlight.note, "Her name ✓");
    assert!(highlight.mark.noted);
    let bookmark = ours.iter().find(|a| a.mark.kind == Kind::Bookmark).unwrap();
    assert_eq!(
        bookmark.mark.start,
        device.locate("/body/DocFragment[2]/body/p[1]/text().6")
    );
    assert_eq!(bookmark.mark.start.section, 1);
    // In the second of two chapters of about the same length.
    assert!(bookmark.percent >= 40, "{}", bookmark.percent);
    assert_eq!(device.changed.borrow().as_slice(), [BOOK]);
    // Nothing new for KOReader.
    assert!(!device.storage.exists(EXCHANGE));

    // Opened again: nothing changes, nothing is said.
    device.open();
    assert_eq!(device.ours().len(), 2);
    assert_eq!(device.changed.borrow().len(), 1);
}

#[test]
fn changes_here_are_passed_on_to_koreader() {
    let mut device = Device::new(Some(METADATA.as_bytes()));
    device.open();
    let start = device.locate("/body/DocFragment[1]/body/p[2]/text().3");
    let end = device.locate("/body/DocFragment[1]/body/p[2]/text().6");
    annotations::add(
        &device.storage,
        BOOK,
        Annotation::new(
            Mark::new(Kind::Highlight, start, end, true),
            20,
            1_792_000_000,
            0,
            String::from("she"),
            String::from("Quote \"this\"\\"),
        ),
    )
    .unwrap();
    let changed = AnnotationsChanged::new(BOOK.to_string());
    device.send(events::ANNOTATIONS_CHANGED, &changed);
    let passed = device.passed();
    assert_eq!(passed.len(), 3, "{passed:?}");
    let new = passed
        .iter()
        .find(|i| i.str("text") == Some("she"))
        .unwrap();
    assert_eq!(new.str("note"), Some("Quote \"this\"\\"));
    assert_eq!(new.str("drawer"), Some("lighten"));
    assert_eq!(device.locate(new.str("pos0").unwrap()), start);
    assert_eq!(device.locate(new.str("pos1").unwrap()), end);
}

/// KOReader's file empty again (a fresh one synced in, say), and the
/// reader keeps them: they stay here and go to KOReader again; not asked
/// twice for it.
#[test]
fn kept_they_stay_and_go_to_koreader_again() {
    let mut device = Device::new(Some(METADATA.as_bytes()));
    device.open();
    device
        .storage
        .write(SIDECAR, b"return { [\"annotations\"] = {} }")
        .unwrap();
    device.open();
    assert!(device.asked());
    device.answer(false);
    assert!(!device.asked());
    assert_eq!(device.ours().len(), 2);
    assert_eq!(device.passed().len(), 2);
    device.open();
    assert!(!device.asked());
    assert_eq!(device.ours().len(), 2);
}

/// Every annotation removed on one side goes on the other too: in
/// KOReader, they go here once the reader said so in the book; here,
/// KOReader is told when the reader says so.
#[test]
fn everything_removed_on_one_side_goes_on_the_other() {
    let mut device = Device::new(Some(METADATA.as_bytes()));
    device.open();
    assert_eq!(device.ours().len(), 2);
    device
        .storage
        .write(SIDECAR, b"return { [\"annotations\"] = {} }")
        .unwrap();
    device.open();
    // Asked in the book first: nothing goes meanwhile.
    assert!(device.asked());
    assert_eq!(device.ours().len(), 2);
    device.answer(true);
    assert!(device.ours().is_empty());
    assert_eq!(device.changed.borrow().len(), 2);
    assert!(!device.storage.exists(EXCHANGE));

    let mut device = Device::new(Some(METADATA.as_bytes()));
    device.open();
    assert!(annotations::remove(&device.storage, BOOK, |_| true).unwrap());
    let changed = AnnotationsChanged::new(BOOK.to_string());
    device.send(events::ANNOTATIONS_CHANGED, &changed);
    assert!(device.storage.exists(EXCHANGE));
    assert!(device.passed().is_empty());
    // Opened again before KOReader took it over: nothing comes back.
    device.open();
    assert!(device.ours().is_empty());
}

#[test]
fn books_without_a_koreader_folder_are_left_alone() {
    let mut device = Device::new(None);
    device.open();
    assert!(device.ours().is_empty());
    assert!(device.changed.borrow().is_empty());
    let mut written = Vec::new();
    let _ = device.storage.list(".paperpaw/plugins/koreader", &mut |e| {
        written.push(e.name.to_string())
    });
    assert!(written.is_empty(), "{written:?}");
}

#[test]
fn hostile_koreader_files_change_nothing() {
    let huge = format!(
        "return {{ annotations = {{ {{ text = \"{}\" }} }} }}",
        "x".repeat(5 << 20)
    );
    let files: Vec<Vec<u8>> = vec![
        b"garbage".to_vec(),
        vec![0xff, 0xfe, 0x00, 0x80],
        format!("return {}{}", "{".repeat(10_000), "}".repeat(10_000)).into_bytes(),
        b"return { annotations = \"not a table\" }".to_vec(),
        b"return { annotations = { 1, true, \"x\", 0x7fffffffffffffffff } }".to_vec(),
        // Wrong types where text is expected.
        br#"return { annotations = { { drawer = "lighten", pos0 = 5, pos1 = {}, text = 1 },
            { page = true }, { page = { "/body" } } } }"#
            .to_vec(),
        // Positions that are nowhere, or backwards.
        br#"return { annotations = {
            { page = "/body/DocFragment[99]/body/p[1]/text().0" },
            { page = "not an xpointer \u{0}" },
            { drawer = "lighten", pos0 = "/body/DocFragment[1]/body/p[1]/text().5",
              pos1 = "/body/DocFragment[1]/body/p[1]/text().0" },
            { drawer = "lighten", pos0 = "/body/DocFragment[1]/body/p[1]/text().99999999999",
              pos1 = "/body/DocFragment[1]/body/p[1]/text().999999999999999" } } }"#
            .to_vec(),
        huge.into_bytes(),
    ];
    for file in files {
        let label = String::from_utf8_lossy(&file[..file.len().min(60)]).into_owned();
        let mut device = Device::new(Some(&file));
        device.open();
        assert!(device.ours().is_empty(), "{label}");
        assert!(device.changed.borrow().is_empty(), "{label}");
        assert!(!device.storage.exists(EXCHANGE), "{label}");
        // KOReader's own file is never written.
        assert_eq!(device.storage.0.get(SIDECAR).unwrap(), file, "{label}");
    }
}

#[test]
fn odd_dates_and_fields_are_read_without_harm() {
    let file = r#"return { annotations = {
        { datetime = "99999-99-99 99:99:99", drawer = "lighten",
          pos0 = "/body/DocFragment[1]/body/p[1]/text().0",
          pos1 = "/body/DocFragment[1]/body/p[1]/text().5",
          note = "line one\nline two", text = "Ålice ✓",
          unknown_field = { nested = { deeper = 1 } } },
        { datetime = "-001-01-01", page = "/body/DocFragment[1]/body/p[2]/text().0" },
        { datetime = "", datetime_updated = "2026-02-30 25:61:61",
          page = "/body/DocFragment[2]/body/p[1]/text().0" },
    } }"#;
    let mut device = Device::new(Some(file.as_bytes()));
    device.open();
    let ours = device.ours();
    assert_eq!(ours.len(), 3, "{ours:?}");
    let highlight = ours
        .iter()
        .find(|a| a.mark.kind == Kind::Highlight)
        .unwrap();
    assert_eq!(highlight.text, "Ålice ✓");
    // Notes are one line here.
    assert!(!highlight.note.contains('\n'), "{:?}", highlight.note);
    assert!(highlight.note.contains("line one") && highlight.note.contains("line two"));
}

#[test]
fn what_koreader_has_and_paperpaw_does_not_know_is_passed_on_unchanged() {
    // An annotation PaperPaw cannot place, with fields of its own, beside
    // a new one here: written back exactly as KOReader had it.
    let file = r#"return { annotations = {
        { drawer = "underscore", color = "yellow", pos0 = "/body/DocFragment[9]/p[1]/text().0",
          pos1 = "/body/DocFragment[9]/p[1]/text().3", text = "Elsewhere",
          extra = { 1, 2, { three = true } }, weight = -3.5e-2 },
    } }"#;
    let Some(Value::Table(root)) = lua::parse(file) else {
        panic!("not Lua");
    };
    let Some(Value::Table(items)) = root.get("annotations") else {
        panic!("no annotations");
    };
    let theirs = items.list().next().unwrap().clone();
    let mut device = Device::new(Some(file.as_bytes()));
    let start = device.locate("/body/DocFragment[1]/body/p[1]/text().0");
    annotations::add(
        &device.storage,
        BOOK,
        Annotation::new(
            Mark::new(Kind::Bookmark, start, start, false),
            0,
            1_792_000_000,
            0,
            String::new(),
            String::new(),
        ),
    )
    .unwrap();
    device.open();
    let passed = device.passed();
    assert_eq!(passed.len(), 2);
    assert_eq!(Value::Table(passed[0].clone()), theirs);
    assert!(passed[1].get("drawer").is_none());
}

#[test]
fn a_moved_book_takes_its_koreader_folder_along() {
    let mut device = Device::new(Some(METADATA.as_bytes()));
    device
        .storage
        .write(EXCHANGE, b"return { annotations = {} }")
        .unwrap();
    device
        .storage
        .write("books/new/wonder.epub", &epub())
        .unwrap();
    device.storage.remove(BOOK).unwrap();
    let moved = BookMoved::new(BOOK.to_string(), String::from("books/new/wonder.epub"));
    device.send(events::BOOK_MOVED, &moved);
    assert!(!device.storage.exists(SIDECAR));
    assert_eq!(
        device
            .storage
            .0
            .get("books/new/wonder.sdr/metadata.epub.lua")
            .unwrap(),
        METADATA.as_bytes()
    );
    // KOReader's pending file is named after the book: renamed too.
    assert!(
        device
            .storage
            .exists("books/new/wonder.sdr/wonder.epub.annotations.lua")
    );
    assert!(
        !device
            .storage
            .exists("books/new/wonder.sdr/alice.epub.annotations.lua")
    );
}

/// The book with styles of its own before the highlighted words: with
/// them, its text holds runs the conversion without them lacks, so the
/// words are at other offsets in each.
fn styled_epub() -> Vec<u8> {
    let styled = r#"<span style="font-weight: bold">was</span>"#;
    let ch1 = format!(
        "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>\
         <p style=\"text-align: center\">Alice {styled} beginning {styled} tired.</p>\
         <p>So she was considering in her own mind.</p></body></html>"
    );
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", CONTAINER)
        .deflated("OEBPS/content.opf", OPF)
        .deflated("OEBPS/ch1.xhtml", ch1)
        .deflated("OEBPS/ch2.xhtml", CH2)
        .build()
}

/// A highlight both sides have stays one highlight when the book's own
/// styles are switched off: ours is found by its XPointer at the places
/// of the other conversion, where KOReader's is, so neither is taken for
/// removed or new, and nothing changes on either side.
#[test]
fn switching_the_books_styles_changes_nothing_on_either_side() {
    let metadata = r#"return { annotations = {
        { datetime = "2026-10-01 10:00:00", drawer = "lighten", note = "Mine",
          page = "/body/DocFragment[1]/body/p[2]/text().3",
          pos0 = "/body/DocFragment[1]/body/p[2]/text().3",
          pos1 = "/body/DocFragment[1]/body/p[2]/text().6", text = "she" },
    } }"#;
    let mut device = Device::new(Some(metadata.as_bytes()));
    device.storage.write(BOOK, &styled_epub()).unwrap();
    device.open();
    let before = device.ours();
    assert_eq!(before.len(), 1, "{before:?}");
    assert_eq!(device.changed.borrow().len(), 1);
    device.now += 100;
    device
        .kernel
        .set_setting(Instant(device.now), "core.publisher-styles", "off")
        .unwrap();
    device.open();
    let ours = device.ours();
    assert_eq!(ours.len(), 1, "{ours:?}");
    let plain = device.kernel.formats().options().publisher_styles(false);
    let doc = device
        .kernel
        .formats()
        .open_with(&device.storage, BOOK, &plain)
        .unwrap();
    let at = |x: &str| doc.locate_xpointer(&device.storage, x).unwrap();
    assert_eq!(
        (ours[0].mark.start, ours[0].mark.end),
        (
            at("/body/DocFragment[1]/body/p[2]/text().3"),
            at("/body/DocFragment[1]/body/p[2]/text().6")
        )
    );
    assert_ne!(ours[0].mark.start, before[0].mark.start, "the text moved");
    assert_eq!(
        (&ours[0].note, ours[0].created),
        (&before[0].note, before[0].created)
    );
    // Neither side changed.
    assert_eq!(device.changed.borrow().len(), 1);
    assert!(!device.storage.exists(EXCHANGE));
}
