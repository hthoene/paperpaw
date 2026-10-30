use super::*;
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::{DirEntry, MemoryStorage};
use paperpaw_zip::testing::ZipBuilder;

const BOOK: &str = "books/alice.epub";
const SIDECAR: &str = "books/alice.sdr/metadata.epub.lua";
const EXCHANGE: &str = "books/alice.sdr/alice.epub.annotations.lua";
const NOW: i64 = 1_792_000_000;

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Alice</dc:title></metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#;

const NAV: &str = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body><nav epub:type="toc"><ol><li><a href="ch1.xhtml">Down the Rabbit Hole</a></li></ol></nav></body></html>"#;

const CH1: &str = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body>\
    <p>Alice was beginning to get very tired.</p>\
    <p>So she was considering in her own mind.</p></body></html>";

/// What KOReader keeps: a highlight of "Alice" with a note, a bookmark,
/// and a highlight PaperPaw cannot place.
const METADATA: &str = r#"-- we can read Lua syntax here!
return {
    ["annotations"] = {
        [1] = {
            ["datetime"] = "2026-10-01 10:00:00",
            ["drawer"] = "lighten",
            ["note"] = "Her name",
            ["page"] = "/body/DocFragment[1]/body/p[1]/text().0",
            ["pos0"] = "/body/DocFragment[1]/body/p[1]/text().0",
            ["pos1"] = "/body/DocFragment[1]/body/p[1]/text().5",
            ["text"] = "Alice",
        },
        [2] = {
            ["datetime"] = "2026-10-01 10:05:00",
            ["page"] = "/body/DocFragment[1]/body/p[2]/text().0",
        },
        [3] = {
            ["datetime"] = "2026-10-01 10:06:00",
            ["drawer"] = "lighten",
            ["page"] = "/body/DocFragment[7]/body/p[1]/text().0",
            ["pos0"] = "/body/DocFragment[7]/body/p[1]/text().0",
            ["pos1"] = "/body/DocFragment[7]/body/p[1]/text().3",
            ["text"] = "Elsewhere",
        },
    },
    ["percent_finished"] = 0.5,
}
"#;

fn storage() -> MemoryStorage {
    let zip = ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", CONTAINER)
        .deflated("OEBPS/content.opf", OPF)
        .deflated("OEBPS/nav.xhtml", NAV)
        .deflated("OEBPS/ch1.xhtml", CH1)
        .build();
    MemoryStorage::new()
        .with(BOOK, zip)
        .with(SIDECAR, METADATA.as_bytes().to_vec())
}

/// An exchange at `now`, with KOReader's files read up to `limit`:
/// whether ours changed, `None` if KOReader has no file.
fn try_run(storage: &dyn Storage, now: i64, limit: usize) -> Result<Option<bool>, Unreadable> {
    try_run_as(storage, now, limit, Removal::Ask)
}

/// An exchange, `removal` saying what to do where KOReader's file has
/// none any more of what both had.
fn try_run_as(
    storage: &dyn Storage,
    now: i64,
    limit: usize,
    removal: Removal,
) -> Result<Option<bool>, Unreadable> {
    let Some(theirs) = read_theirs(storage, BOOK, limit)? else {
        return Ok(None);
    };
    let doc = EpubDocument::open(storage, BOOK).unwrap();
    let local = |unix| unix;
    let x = Exchange {
        storage,
        removal,
        doc: &doc,
        now,
        local: &local,
        drawer: "lighten",
    };
    exchange(&x, BOOK, theirs).map(Some)
}

fn run(storage: &dyn Storage, now: i64) -> bool {
    try_run(storage, now, MAX_FILE).unwrap().unwrap()
}

fn items(storage: &MemoryStorage, path: &str) -> Vec<Table> {
    read_items(storage, path, MAX_FILE).unwrap().unwrap().0
}

fn text(storage: &MemoryStorage, path: &str) -> String {
    String::from_utf8(paperpaw_sdk::read_file(storage, path, 1 << 20).unwrap()).unwrap()
}

/// KOReader opens the book: it takes over the exchange file.
fn koreader_takes_over(storage: &MemoryStorage) {
    storage
        .write(SIDECAR, text(storage, EXCHANGE).as_bytes())
        .unwrap();
    storage.remove(EXCHANGE).unwrap();
}

/// A card on which the file `broken` cannot be read.
struct Broken<'a> {
    storage: &'a MemoryStorage,
    broken: String,
}

impl Broken<'_> {
    fn check(&self, path: &str) -> Result<(), StorageError> {
        if path == self.broken {
            Err(StorageError::Io)
        } else {
            Ok(())
        }
    }
}

impl Storage for Broken<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.check(path)?;
        self.storage.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.check(path)?;
        self.storage.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.storage.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.storage.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.storage.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.storage.remove(path)
    }
}

/// A card on which the file `path` cannot be written (it is full).
struct Unwritable<'a> {
    storage: &'a MemoryStorage,
    path: String,
}

impl Storage for Unwritable<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.storage.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.storage.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path == self.path {
            return Err(StorageError::Full);
        }
        self.storage.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.storage.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.storage.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.storage.remove(path)
    }
}

/// Both sides after an exchange that KOReader took over: three
/// annotations here, four there (one not placed here).
fn exchanged() -> MemoryStorage {
    let storage = storage();
    let tired = highlight(
        &storage,
        "/body/DocFragment[1]/body/p[1]/text().32",
        "/body/DocFragment[1]/body/p[1]/text().37",
        "",
    );
    annotations::add(&storage, BOOK, tired).unwrap();
    assert!(run(&storage, NOW));
    koreader_takes_over(&storage);
    assert!(!run(&storage, NOW + 10));
    assert!(!storage.exists(EXCHANGE));
    storage
}

/// What both sides keep, to see that nothing changed.
fn state(storage: &MemoryStorage) -> (Vec<Annotation>, Option<String>, String) {
    let theirs = paperpaw_sdk::read_file(storage, SIDECAR, MAX_FILE)
        .ok()
        .map(|data| String::from_utf8_lossy(&data).into_owned());
    let known = text(storage, &known_file(BOOK));
    (annotations::load(storage, BOOK), theirs, known)
}

/// A highlight here of the text at `pos0..pos1`.
fn highlight(storage: &MemoryStorage, pos0: &str, pos1: &str, note: &str) -> Annotation {
    let doc = EpubDocument::open(storage, BOOK).unwrap();
    let start = doc.locate_xpointer(storage, pos0).unwrap();
    let end = doc.locate_xpointer(storage, pos1).unwrap();
    Annotation::new(
        Mark::new(Kind::Highlight, start, end, !note.is_empty()),
        1,
        NOW - 100,
        0,
        String::from("tired"),
        String::from(note),
    )
}

#[test]
fn file_names_as_koreader_has_them() {
    assert_eq!(sidecar(BOOK), "books/alice.sdr");
    assert_eq!(
        metadata_file("a/b.Book.EPUB"),
        "a/b.Book.sdr/metadata.EPUB.lua"
    );
    assert_eq!(exchange_file(BOOK), EXCHANGE);
    let local = |unix| unix + 7200;
    let doc = EpubDocument::open(&storage(), BOOK).unwrap();
    let x = Exchange {
        storage: &MemoryStorage::new(),
        removal: Removal::Ask,
        doc: &doc,
        now: 0,
        local: &local,
        drawer: "lighten",
    };
    // Local times, two hours ahead here.
    assert_eq!(datetime(&x, 1_760_623_200), "2025-10-16 16:00:00");
    assert_eq!(unix_of(&x, "2025-10-16 16:00:00"), 1_760_623_200);
    assert_eq!(unix_of(&x, "nonsense"), 0);
}

#[test]
fn annotations_go_back_and_forth() {
    let storage = storage();
    let tired = highlight(
        &storage,
        "/body/DocFragment[1]/body/p[1]/text().32",
        "/body/DocFragment[1]/body/p[1]/text().37",
        "",
    );
    annotations::add(&storage, BOOK, tired.clone()).unwrap();

    // KOReader's come over; ours go to the exchange file with all of
    // KOReader's, also the one not placed here.
    assert!(run(&storage, NOW));
    let ours = annotations::load(&storage, BOOK);
    assert_eq!(ours.len(), 3);
    let alice = ours.iter().find(|a| a.note == "Her name").unwrap();
    assert_eq!(alice.text, "Alice");
    assert_eq!(alice.created, unix_of_utc("2026-10-01 10:00:00"));
    let bookmark = ours.iter().find(|a| a.mark.kind == Kind::Bookmark).unwrap();
    assert!(bookmark.text.starts_with("So she was"));
    let passed = items(&storage, EXCHANGE);
    assert_eq!(passed.len(), 4);
    assert_eq!(passed[2].str("text"), Some("Elsewhere"));
    let new = &passed[3];
    assert_eq!(new.str("text"), Some("tired"));
    assert_eq!(new.str("chapter"), Some("Down the Rabbit Hole"));
    assert_eq!(
        new.str("pos0"),
        Some("/body/DocFragment[1]/body/p[1]/text()[1].32")
    );
    assert_eq!(
        new.str("pos1"),
        Some("/body/DocFragment[1]/body/p[1]/text()[1].37")
    );
    let Some(Value::Table(root)) = lua::parse(&text(&storage, EXCHANGE)) else {
        panic!("not Lua");
    };
    assert_eq!(root.str("device_id"), Some("PaperPaw"));

    // Until KOReader has taken it over, nothing more to do.
    assert!(!run(&storage, NOW + 10));

    // KOReader took it over, then removed "Alice" and noted on "tired".
    storage.remove(EXCHANGE).unwrap();
    let mut theirs = passed.clone();
    theirs.remove(0);
    theirs[2].set("note", lua::string("Very"));
    theirs[2].set("datetime_updated", lua::string("2026-10-20 08:00:00"));
    let file = lua::table([(
        "annotations",
        Value::Table(Table::from_list(theirs.into_iter().map(Value::Table))),
    )]);
    storage
        .write(SIDECAR, lua::write(&file).as_bytes())
        .unwrap();
    assert!(run(&storage, NOW + 20));
    let ours = annotations::load(&storage, BOOK);
    assert_eq!(ours.len(), 2);
    assert!(ours.iter().all(|a| a.note != "Her name"));
    assert!(ours.iter().any(|a| a.note == "Very" && a.mark.noted));
    assert!(!storage.exists(EXCHANGE), "nothing new for KOReader");

    // Removed here: KOReader is told by leaving it out.
    assert!(annotations::remove(&storage, BOOK, |m| m.kind == Kind::Bookmark).unwrap());
    assert!(!run(&storage, NOW + 30));
    let passed = items(&storage, EXCHANGE);
    assert_eq!(passed.len(), 2);
    assert!(passed.iter().all(|i| i.get("drawer").is_some()));
}

#[test]
fn later_notes_win() {
    let storage = storage();
    // The same highlight as KOReader's, with a note made later here.
    let mut alice = highlight(
        &storage,
        "/body/DocFragment[1]/body/p[1]/text().0",
        "/body/DocFragment[1]/body/p[1]/text().5",
        "Later note",
    );
    alice.updated = unix_of_utc("2026-10-05 12:00:00");
    annotations::add(&storage, BOOK, alice).unwrap();
    run(&storage, NOW);
    let ours = annotations::load(&storage, BOOK);
    assert!(ours.iter().any(|a| a.note == "Later note"));
    let passed = items(&storage, EXCHANGE);
    assert_eq!(passed[0].str("note"), Some("Later note"));
    assert_eq!(
        passed[0].str("datetime_updated"),
        Some("2026-10-05 12:00:00")
    );
}

#[test]
fn a_missing_koreader_file_changes_nothing() {
    // A sync has brought `alice.sdr`, but not yet the file in it: that
    // is not KOReader having removed everything.
    let storage = exchanged();
    let (ours, _, known) = state(&storage);
    assert_eq!(ours.len(), 3);
    storage.remove(SIDECAR).unwrap();
    assert_eq!(try_run(&storage, NOW + 20, MAX_FILE), Ok(None));
    assert_eq!(state(&storage), (ours, None, known));
    assert!(!storage.exists(EXCHANGE));
}

#[test]
fn unreadable_koreader_files_change_nothing() {
    let taken = text(&exchanged(), SIDECAR);
    let mut invalid_utf8 = taken.clone().into_bytes();
    invalid_utf8.insert(taken.find("Alice").unwrap(), 0xff);
    let mut large = taken.clone();
    large.push_str(&"-- padding\n".repeat(MAX_FILE_SMALL / 10));
    let broken: [(&str, Vec<u8>, usize); 7] = [
        (
            "cut short",
            taken.as_bytes()[..taken.len() / 2].to_vec(),
            MAX_FILE,
        ),
        ("not UTF-8", invalid_utf8, MAX_FILE),
        (
            "not a list",
            b"return { [\"annotations\"] = \"none\" }".to_vec(),
            MAX_FILE,
        ),
        (
            "not a table in the list",
            b"return { [\"annotations\"] = { [1] = \"Alice\" } }".to_vec(),
            MAX_FILE,
        ),
        (
            "an older KOReader",
            b"return { [\"bookmarks\"] = {}, [\"highlight\"] = {} }".to_vec(),
            MAX_FILE,
        ),
        ("too large here", large.clone().into_bytes(), MAX_FILE_SMALL),
        ("cannot be read", taken.clone().into_bytes(), MAX_FILE),
    ];
    for (what, file, limit) in broken {
        let storage = exchanged();
        storage.write(SIDECAR, &file).unwrap();
        let before = state(&storage);
        let card = Broken {
            storage: &storage,
            broken: String::from(if what == "cannot be read" {
                SIDECAR
            } else {
                ""
            }),
        };
        assert_eq!(try_run(&card, NOW + 20, limit), Err(Unreadable), "{what}");
        assert_eq!(state(&storage), before, "{what}");
        assert!(!storage.exists(EXCHANGE), "{what}");
    }
    // Where there is room, the large file is read.
    let storage = exchanged();
    storage.write(SIDECAR, large.as_bytes()).unwrap();
    assert_eq!(try_run(&storage, NOW + 20, MAX_FILE), Ok(Some(false)));
}

#[test]
fn a_koreader_file_without_annotations_has_none() {
    let storage =
        MemoryStorage::new().with(SIDECAR, b"return { [\"percent_finished\"] = 0.5 }".to_vec());
    assert_eq!(
        read_items(&storage, SIDECAR, MAX_FILE),
        Ok(Some((Vec::new(), false)))
    );
    assert_eq!(read_items(&storage, EXCHANGE, MAX_FILE), Ok(None));
}

#[test]
fn small_devices_read_small_files_only() {
    // A KOReader file of 40 KiB: too much for the X4, fine elsewhere.
    let mut file = String::from(METADATA);
    file.push_str(&"-- padding\n".repeat(4096));
    let storage = MemoryStorage::new().with(SIDECAR, file.into_bytes());
    let small = max_file(MemoryClass::Small);
    assert_eq!(read_items(&storage, SIDECAR, small), Err(Unreadable));
    let large = max_file(MemoryClass::Large);
    assert_eq!(
        read_items(&storage, SIDECAR, large)
            .unwrap()
            .unwrap()
            .0
            .len(),
        3
    );
}

#[test]
fn unreadable_files_here_change_nothing() {
    for broken in [annotations::file(BOOK), known_file(BOOK)] {
        // Removed here: without what is known, it would come back.
        let storage = exchanged();
        assert!(annotations::remove(&storage, BOOK, |m| m.kind == Kind::Bookmark).unwrap());
        let before = state(&storage);
        let card = Broken {
            storage: &storage,
            broken: broken.clone(),
        };
        assert_eq!(
            try_run(&card, NOW + 20, MAX_FILE),
            Err(Unreadable),
            "{broken}"
        );
        assert_eq!(state(&storage), before, "{broken}");
        assert!(!storage.exists(EXCHANGE), "{broken}");
    }
}

#[test]
fn what_cannot_be_kept_here_is_not_known() {
    let storage = storage();
    let card = Unwritable {
        storage: &storage,
        path: annotations::file(BOOK),
    };
    assert_eq!(try_run(&card, NOW, MAX_FILE), Err(Unreadable));
    assert!(!storage.exists(&known_file(BOOK)));
    assert!(!storage.exists(EXCHANGE));
    // With room again, KOReader's come over and stay there: they were
    // never known, so they are not taken for removed here.
    assert!(run(&storage, NOW + 10));
    assert_eq!(annotations::load(&storage, BOOK).len(), 2);
    assert!(!storage.exists(EXCHANGE));
    assert_eq!(items(&storage, SIDECAR).len(), 3);
}

#[test]
fn without_a_clock_nothing_is_dated_1970() {
    let storage = storage();
    let mut tired = highlight(
        &storage,
        "/body/DocFragment[1]/body/p[1]/text().32",
        "/body/DocFragment[1]/body/p[1]/text().37",
        "",
    );
    tired.created = 0;
    annotations::add(&storage, BOOK, tired).unwrap();

    // KOReader's come over; ours waits for a time, and stays.
    for _ in 0..2 {
        run(&storage, 0);
        assert!(!storage.exists(EXCHANGE));
        let ours = annotations::load(&storage, BOOK);
        assert_eq!(ours.len(), 3);
        assert!(ours.iter().any(|a| a.text == "tired"));
    }

    // With the clock set, it goes over, made then.
    assert!(!run(&storage, NOW));
    let passed = items(&storage, EXCHANGE);
    let tired = passed
        .iter()
        .find(|i| i.str("text") == Some("tired"))
        .unwrap();
    assert_eq!(unix_of_utc(tired.str("datetime").unwrap()), NOW);
    assert!(!text(&storage, EXCHANGE).contains("1970"));
}

#[test]
fn notes_changed_on_one_side_go_over_without_a_clock() {
    let storage = storage();
    // The same highlight as KOReader's, with the same note, made while
    // the clock was not set.
    let mut alice = highlight(
        &storage,
        "/body/DocFragment[1]/body/p[1]/text().0",
        "/body/DocFragment[1]/body/p[1]/text().5",
        "Her name",
    );
    alice.created = 0;
    let at = alice.mark.start;
    annotations::add(&storage, BOOK, alice).unwrap();
    run(&storage, 0);
    assert!(!storage.exists(EXCHANGE));
    let note = |storage: &MemoryStorage| annotations::note_of(storage, BOOK, at);

    // Changed here only: it goes to KOReader, though KOReader's is dated
    // and ours is not.
    annotations::set_note(&storage, BOOK, at, "Mine", 0).unwrap();
    assert!(!run(&storage, 0));
    assert_eq!(note(&storage), "Mine");
    assert_eq!(items(&storage, EXCHANGE)[0].str("note"), Some("Mine"));
    assert!(!text(&storage, EXCHANGE).contains("1970"));
    koreader_takes_over(&storage);

    // Changed there only: it comes over.
    let mut theirs = items(&storage, SIDECAR);
    theirs[0].set("note", lua::string("Theirs"));
    let file = lua::table([(
        "annotations",
        Value::Table(Table::from_list(theirs.iter().cloned().map(Value::Table))),
    )]);
    storage
        .write(SIDECAR, lua::write(&file).as_bytes())
        .unwrap();
    assert!(run(&storage, 0));
    assert_eq!(note(&storage), "Theirs");
    assert!(!storage.exists(EXCHANGE));

    // Changed on both, with no time here: both stay as they are.
    annotations::set_note(&storage, BOOK, at, "Again", 0).unwrap();
    theirs[0].set("note", lua::string("Other"));
    let file = lua::table([(
        "annotations",
        Value::Table(Table::from_list(theirs.into_iter().map(Value::Table))),
    )]);
    storage
        .write(SIDECAR, lua::write(&file).as_bytes())
        .unwrap();
    assert!(!run(&storage, 0));
    assert_eq!(note(&storage), "Again");
    assert!(!storage.exists(EXCHANGE));
    assert_eq!(items(&storage, SIDECAR)[0].str("note"), Some("Other"));
}

#[test]
fn own_announcements_need_no_exchange() {
    let announced = Announced::default();
    assert!(!announced.is_own(BOOK));
    announced.set(BOOK);
    assert!(!announced.is_own("books/other.epub"));
    assert!(announced.is_own(BOOK));
    // Only once: a change after it is exchanged.
    assert!(!announced.is_own(BOOK));
}

fn unix_of_utc(text: &str) -> i64 {
    let doc = EpubDocument::open(&storage(), BOOK).unwrap();
    let local = |unix| unix;
    let x = Exchange {
        storage: &MemoryStorage::new(),
        removal: Removal::Ask,
        doc: &doc,
        now: 0,
        local: &local,
        drawer: "lighten",
    };
    unix_of(&x, text)
}

#[test]
fn translations_are_complete() {
    use paperpaw_i18n::{Catalog, check};
    let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}

/// The exchange file cannot be written (the card is full, its name too
/// long): what was to go to KOReader is not known to both, so the next
/// exchange does not take it for removed there.
#[test]
fn what_cannot_be_passed_on_is_not_known() {
    let storage = storage();
    let tired = highlight(
        &storage,
        "/body/DocFragment[1]/body/p[1]/text().32",
        "/body/DocFragment[1]/body/p[1]/text().37",
        "my own note",
    );
    assert!(annotations::add(&storage, BOOK, tired).is_ok());
    let card = Unwritable {
        storage: &storage,
        path: String::from(EXCHANGE),
    };
    // KOReader's come over all the same.
    assert_eq!(try_run(&card, NOW, MAX_FILE), Ok(Some(true)));
    assert!(!storage.exists(EXCHANGE));
    // With room again, it is passed on, not removed here.
    run(&storage, NOW + 10);
    let ours = annotations::load(&storage, BOOK);
    assert!(ours.iter().any(|a| a.note == "my own note"), "{ours:?}");
    assert!(
        items(&storage, EXCHANGE)
            .iter()
            .any(|i| i.str("note") == Some("my own note"))
    );
}

/// A note of KOReader longer than PaperPaw keeps is cut here; that is no
/// change here to pass back, which would cut KOReader's note too.
#[test]
fn a_long_note_of_koreader_is_not_cut_there() {
    let storage = storage();
    let long = "x".repeat(5000);
    storage
        .write(SIDECAR, METADATA.replace("Her name", &long).as_bytes())
        .unwrap();
    assert!(run(&storage, NOW));
    let ours = annotations::load(&storage, BOOK);
    assert!(
        ours.iter()
            .any(|a| a.note.len() < long.len() && a.note.starts_with("xxx"))
    );
    for later in [10, 20] {
        assert!(!run(&storage, NOW + later));
        assert!(!storage.exists(EXCHANGE));
    }
    assert_eq!(items(&storage, SIDECAR)[0].str("note"), Some(long.as_str()));
}

/// One side has none at all of what both had (book data moved halfway, a
/// synced folder not complete yet): nothing is taken for removed.
#[test]
fn all_gone_on_one_side_changes_nothing() {
    // None here.
    let storage = exchanged();
    storage.remove(&annotations::file(BOOK)).unwrap();
    let before = state(&storage);
    assert_eq!(try_run(&storage, NOW + 20, MAX_FILE), Err(Unreadable));
    assert_eq!(state(&storage), before);
    assert!(!storage.exists(EXCHANGE));
    // KOReader's file without a list of annotations.
    let storage = exchanged();
    storage
        .write(SIDECAR, b"return { [\"percent_finished\"] = 0.5 }")
        .unwrap();
    let before = state(&storage);
    assert_eq!(try_run(&storage, NOW + 20, MAX_FILE), Err(Unreadable));
    assert_eq!(state(&storage), before);
    assert!(!storage.exists(EXCHANGE));
    // None here after a change the reader did not tell of.
    let storage = exchanged();
    storage.remove(&annotations::file(BOOK)).unwrap();
    note_emptied(&storage, "books/other.epub");
    assert_eq!(try_run(&storage, NOW + 20, MAX_FILE), Err(Unreadable));
    // None here, and KOReader's file cannot be read.
    let storage = exchanged();
    assert!(annotations::remove(&storage, BOOK, |_| true).unwrap());
    note_emptied(&storage, BOOK);
    storage.write(SIDECAR, b"return { garbage").unwrap();
    let before = state(&storage);
    assert_eq!(try_run(&storage, NOW + 20, MAX_FILE), Err(Unreadable));
    assert_eq!(state(&storage), before);
    assert!(!storage.exists(EXCHANGE));
}

/// Every annotation removed in KOReader: its list there is empty. The
/// reader is asked first, nothing changing on either side meanwhile;
/// told to remove them, they go here too. Those it makes after come over
/// again.
#[test]
fn everything_removed_in_koreader_goes_here_too_when_the_reader_says_so() {
    let storage = exchanged();
    storage
        .write(SIDECAR, b"return { [\"annotations\"] = {} }")
        .unwrap();
    let before = text(&storage, &annotations::file(BOOK));
    let theirs = read_theirs(&storage, BOOK, MAX_FILE).unwrap().unwrap();
    assert_eq!(removed_there(&storage, BOOK, &theirs), Ok(true));
    let known = state(&storage);
    assert_eq!(try_run(&storage, NOW + 20, MAX_FILE), Ok(Some(false)));
    assert_eq!(text(&storage, &annotations::file(BOOK)), before);
    assert_eq!(state(&storage), known);
    // Kept aside before, and not over one kept aside earlier.
    let earlier = format!("{}.removed", annotations::file(BOOK));
    storage.write(&earlier, b"earlier").unwrap();
    assert_eq!(
        try_run_as(&storage, NOW + 20, MAX_FILE, Removal::Remove),
        Ok(Some(true))
    );
    assert!(annotations::load(&storage, BOOK).is_empty());
    assert!(!storage.exists(EXCHANGE), "nothing to tell KOReader");
    assert_eq!(text(&storage, &earlier), "earlier");
    let aside = annotations::file(BOOK).replace(".txt", ".txt (2).removed");
    assert_eq!(text(&storage, &aside), before);
    // A new one in KOReader comes over.
    storage.write(SIDECAR, METADATA.as_bytes()).unwrap();
    assert_eq!(try_run(&storage, NOW + 30, MAX_FILE), Ok(Some(true)));
    assert_eq!(annotations::load(&storage, BOOK).len(), 2);
}

/// An empty list in a KOReader file never seen with annotations is no
/// sign of anything removed there: KOReader writes one for every book it
/// opened. A fresh file (the book opened in KOReader elsewhere, or reset
/// there) that came by a sync, which also took the file for KOReader
/// away, removes nothing here: what both had is offered to KOReader
/// again.
#[test]
fn a_fresh_koreader_file_removes_nothing() {
    let storage = storage();
    let fresh = b"return { [\"annotations\"] = {}, [\"percent_finished\"] = 0 }";
    storage.write(SIDECAR, fresh).unwrap();
    let tired = highlight(
        &storage,
        "/body/DocFragment[1]/body/p[1]/text().32",
        "/body/DocFragment[1]/body/p[1]/text().37",
        "mine",
    );
    annotations::add(&storage, BOOK, tired).unwrap();
    assert!(!run(&storage, NOW));
    assert_eq!(items(&storage, EXCHANGE).len(), 1);
    // The sync: KOReader's fresh file stays, the file for it is gone.
    storage.remove(EXCHANGE).unwrap();
    assert!(!run(&storage, NOW + 10));
    let ours = annotations::load(&storage, BOOK);
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert_eq!(ours[0].note, "mine");
    assert_eq!(items(&storage, EXCHANGE)[0].str("note"), Some("mine"));
    // KOReader takes it over; then its file is empty again (removed
    // there, or a fresh one synced in): asked, nothing goes meanwhile.
    koreader_takes_over(&storage);
    assert!(!run(&storage, NOW + 20));
    storage.write(SIDECAR, fresh).unwrap();
    assert!(!run(&storage, NOW + 30));
    assert_eq!(annotations::load(&storage, BOOK).len(), 1);
    // Kept: here as they were, and offered to KOReader again; not asked
    // again for that file.
    assert_eq!(
        try_run_as(&storage, NOW + 40, MAX_FILE, Removal::Keep),
        Ok(Some(false))
    );
    assert_eq!(annotations::load(&storage, BOOK).len(), 1);
    assert_eq!(items(&storage, EXCHANGE)[0].str("note"), Some("mine"));
    let theirs = read_theirs(&storage, BOOK, MAX_FILE).unwrap().unwrap();
    assert_eq!(removed_there(&storage, BOOK, &theirs), Ok(false));
    // Told to remove them: they go, a copy kept aside.
    koreader_takes_over(&storage);
    assert!(!run(&storage, NOW + 50));
    storage.write(SIDECAR, fresh).unwrap();
    assert!(
        try_run_as(&storage, NOW + 60, MAX_FILE, Removal::Remove)
            .unwrap()
            .unwrap()
    );
    assert!(annotations::load(&storage, BOOK).is_empty());
}

/// Every annotation removed here, as the reader tells: KOReader is told
/// by an empty list (with only what is not placed here), and nothing
/// comes back.
#[test]
fn everything_removed_here_goes_to_koreader() {
    let storage = exchanged();
    assert!(annotations::remove(&storage, BOOK, |_| true).unwrap());
    note_emptied(&storage, BOOK);
    assert_eq!(try_run(&storage, NOW + 20, MAX_FILE), Ok(Some(false)));
    let passed = items(&storage, EXCHANGE);
    assert_eq!(passed.len(), 1, "{passed:?}");
    assert_eq!(passed[0].str("text"), Some("Elsewhere"));
    koreader_takes_over(&storage);
    assert_eq!(try_run(&storage, NOW + 30, MAX_FILE), Ok(Some(false)));
    assert!(annotations::load(&storage, BOOK).is_empty());
    assert!(!storage.exists(EXCHANGE));
}

/// A book moved whose folder KOReader shares with another format of it:
/// only its own files go along.
#[test]
fn a_shared_folder_stays_with_the_other_format() {
    let storage = exchanged();
    let pdf_metadata = "books/alice.sdr/metadata.pdf.lua";
    storage.write("books/alice.pdf", b"%PDF-1.4").unwrap();
    storage.write(pdf_metadata, b"return {}").unwrap();
    let theirs = text(&storage, SIDECAR);
    let to = "moved/alice.epub";
    storage.rename(BOOK, to).unwrap();
    moved_book(&storage, BOOK, to);
    assert_eq!(text(&storage, pdf_metadata), "return {}");
    assert!(!storage.exists(SIDECAR));
    assert_eq!(text(&storage, "moved/alice.sdr/metadata.epub.lua"), theirs);

    // Alone in it, the whole folder goes.
    let storage = exchanged();
    storage.rename(BOOK, to).unwrap();
    moved_book(&storage, BOOK, to);
    assert!(!storage.exists(SIDECAR));
    assert_eq!(text(&storage, "moved/alice.sdr/metadata.epub.lua"), theirs);
}
