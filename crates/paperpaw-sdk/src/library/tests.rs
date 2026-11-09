use super::*;
use crate::MemoryStorage;
use crate::memory_storage::Unreadable;

#[test]
fn new_books_are_added_lately_and_never_opened() {
    let book = Book {
        added: Some(100),
        ..Book::default()
    };
    assert!(is_new_on(&book, false, Some(100 + NEW_DAYS)));
    assert!(!is_new_on(&book, false, Some(101 + NEW_DAYS)), "too old");
    assert!(!is_new_on(&book, true, Some(100)), "opened");
    let begun = Book {
        progress: Some(0),
        ..book.clone()
    };
    assert!(!is_new_on(&begun, false, Some(100)), "begun");
    assert!(!is_new_on(&book, false, None), "no date");
    assert!(
        !is_new_on(&Book::default(), false, Some(100)),
        "never dated"
    );
}

#[test]
fn details_belong_to_one_file_version() {
    let storage = MemoryStorage::new().with("books/a.txt", "hello");
    let book = Book {
        size: 5,
        indexed: true,
        title: Some("A = \"title\"".into()),
        author: Some("Ann".into()),
        series: Some("Tales".into()),
        series_index: Some("2".into()),
        tags: alloc::vec!["Fantasy".into(), "To read".into()],
        cover: Some(ImageRef {
            path: "x.gray".into(),
            width: 30,
            height: 40,
        }),
        progress: Some(12),
        added: Some(20_000),
        fields: alloc::vec![("stars.rating".into(), "4".into())],
    };
    save(&storage, "books/a.txt", &book).unwrap();
    let read = super::book(&storage, "books/a.txt");
    assert_eq!(read, Some(book));
    assert_eq!(read.unwrap().field("stars.rating"), Some("4"));
    set_field(&storage, "books/a.txt", "stars.rating", "");
    assert!(
        super::book(&storage, "books/a.txt")
            .unwrap()
            .fields
            .is_empty()
    );
    // Replaced by a file of another size: forgotten.
    let storage = storage.with("books/a.txt", "hello!");
    assert_eq!(super::book(&storage, "books/a.txt"), None);
}

#[test]
fn covers_made_small_are_kept_for_the_book() {
    use paperpaw_image::{GrayWriter, ImageSink};
    let storage = MemoryStorage::new().with("books/a.epub", "12345");
    let mut writer = GrayWriter::new(&storage, "cache/cover.gray");
    writer.start(40, 60).unwrap();
    for y in 0..60u8 {
        writer.row(&[y * 4; 40]).unwrap();
    }
    writer.finish().unwrap();
    let mut book = Book {
        size: 5,
        cover: Some(ImageRef {
            path: "cache/cover.gray".into(),
            width: 40,
            height: 60,
        }),
        ..Book::default()
    };
    let made = cover(
        &storage,
        "books/a.epub",
        &book,
        20,
        20,
        paperpaw_platform::PixelFormat::Gray8,
    )
    .unwrap();
    assert_eq!((made.width(), made.height()), (13, 20));
    // Kept: read again without the picture.
    storage.remove("cache/cover.gray").unwrap();
    let kept = cover(
        &storage,
        "books/a.epub",
        &book,
        20,
        20,
        paperpaw_platform::PixelFormat::Gray8,
    )
    .unwrap();
    assert_eq!((kept.width(), kept.height()), (13, 20));
    // Moved with the book.
    moved(&storage, "books/a.epub", "books/b.epub");
    assert!(
        cover(
            &storage,
            "books/b.epub",
            &book,
            20,
            20,
            paperpaw_platform::PixelFormat::Gray8
        )
        .is_some()
    );
    // Another size asked, another book of that name: made anew (here
    // it cannot be, its picture gone).
    assert!(
        cover(
            &storage,
            "books/b.epub",
            &book,
            30,
            30,
            paperpaw_platform::PixelFormat::Gray8
        )
        .is_none()
    );
    book.size = 6;
    assert!(
        cover(
            &storage,
            "books/b.epub",
            &book,
            20,
            20,
            paperpaw_platform::PixelFormat::Gray8
        )
        .is_none()
    );
}

#[test]
fn what_is_kept_for_a_book_follows_it() {
    let (a, b) = ("books/a.epub", "books/b.epub");
    let (ka, kb) = (key(a), key(b));
    let storage = MemoryStorage::new()
        .with(a, "book")
        .with(&format!("cache/{ka}/0.txt"), "chapter")
        .with(&format!("cache/cbz-{ka}/source"), "x")
        .with(&format!("cache/{ka}.ini"), "x")
        .with("cache/other.txt", "not a book's")
        .with(&format!("cache/{ka}x"), "not a key");
    let data = [
        BookData {
            folder: "cache".into(),
            prefix: "".into(),
            ..BookData::default()
        },
        BookData {
            folder: "cache".into(),
            prefix: "cbz-".into(),
            ..BookData::default()
        },
    ];
    assert_eq!(
        data[0].key_of(&format!("{ka}.ini")),
        Some((ka.as_str(), ".ini"))
    );
    assert_eq!(data[0].key_of("other.txt"), None);
    // Moved: every entry of its key, by the new one.
    move_data(&storage, &data, a, b);
    assert!(storage.exists(&format!("cache/{kb}/0.txt")));
    assert!(storage.exists(&format!("cache/cbz-{kb}/source")));
    assert!(storage.exists(&format!("cache/{kb}.ini")));
    assert!(!storage.exists(&format!("cache/{ka}.ini")));
    // Removed: all of it; the rest stays.
    remove_data(&storage, &data, b);
    assert!(!storage.exists(&format!("cache/{kb}/0.txt")));
    assert!(!storage.exists(&format!("cache/{kb}.ini")));
    assert!(storage.exists("cache/other.txt"));
    assert!(storage.exists(&format!("cache/{ka}x")));
    // Cleaned up: what is kept for books no longer there.
    let c = "books/c.txt";
    let storage = storage
        .with(c, "text")
        .with(&format!("cache/{}.ini", key(c)), "x");
    let _ = storage.write(&format!("cache/{ka}.ini"), b"left over");
    let mut formats = crate::Formats::new();
    formats
        .add(crate::FormatSpec {
            id: crate::Id::new_static("test.txt"),
            title: alloc::borrow::Cow::Borrowed("TXT"),
            extensions: alloc::vec![
                alloc::borrow::Cow::Borrowed("txt"),
                alloc::borrow::Cow::Borrowed("epub")
            ],
            open: alloc::boxed::Box::new(|_, _, _| {
                Err(crate::document::DocumentError::UnsupportedFormat)
            }),
            prepared: None,
            sniff: None,
        })
        .unwrap();
    let books = book_keys(&storage, &formats);
    let keep = |k: &str| u64::from_str_radix(k, 16).is_ok_and(|k| books.binary_search(&k).is_ok());
    // The epub at a is there, c too: nothing goes; a gone, its data goes.
    assert_eq!(clean_up(&storage, &data, &keep), 0);
    storage.remove(a).unwrap();
    let books = book_keys(&storage, &formats);
    let keep = |k: &str| u64::from_str_radix(k, 16).is_ok_and(|k| books.binary_search(&k).is_ok());
    assert_eq!(clean_up(&storage, &data, &keep), 1);
    assert!(storage.exists(&format!("cache/{}.ini", key(c))));
    // The books in a folder, or the book itself.
    let mut found = Vec::new();
    books_in(&storage, &formats, "books", &mut |p| {
        found.push(String::from(p))
    });
    assert_eq!(found, [c]);
    found.clear();
    books_in(&storage, &formats, c, &mut |p| found.push(String::from(p)));
    assert_eq!(found, [c]);
}

#[test]
fn removed_books_are_forgotten() {
    let storage = MemoryStorage::new().with("books/a.txt", "hello");
    save(
        &storage,
        "books/a.txt",
        &Book {
            size: 5,
            ..Book::default()
        },
    )
    .unwrap();
    set_favourite(&storage, "books/a.txt", true);
    let _ = storage.write(
        &opened_file(),
        b"books/a.txt
books/b.txt",
    );
    removed(&storage, "books/a.txt");
    assert!(book(&storage, "books/a.txt").is_none());
    assert!(favourites(&storage).is_empty());
    assert_eq!(opened_books(&storage), ["books/b.txt"]);
}

#[test]
fn what_sources_provide_is_kept() {
    let storage = MemoryStorage::new().with("books/a.txt", "hello");
    provide(
        &storage,
        "books/a.txt",
        Provided {
            title: Some("Alice".into()),
            author: Some("Lewis Carroll".into()),
            tags: alloc::vec!["Classics".into()],
            ..Provided::default()
        },
    );
    let book = super::book(&storage, "books/a.txt").unwrap();
    assert_eq!(book.title.as_deref(), Some("Alice"));
    assert_eq!(book.tags, ["Classics"]);
    assert!(!book.indexed);
}

/// A memory storage that says it ignores case, as a memory card does
/// (its own names still count case: enough for paths spelt as listed).
struct Folding(MemoryStorage);

impl Storage for Folding {
    fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, at, buf)
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
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        if dir == "broken" {
            return Err(StorageError::Io);
        }
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
    fn ignores_case(&self) -> bool {
        true
    }
}

#[test]
fn clean_up_keeps_what_any_spelling_or_format_needs() {
    let data = [BookData::new("cache", "", BookDataKind::Cache)];
    let kept = |path: &str| format!("cache/{}.ini", key(path));
    // The card names the folder `Books`; the library's setting says
    // `books`, and the reader opened the book by it.
    let storage = Folding(
        MemoryStorage::new()
            .with("Books/Sub/a.epub", "book")
            .with("Books/notes.xyz", "a format switched off")
            .with(&kept("books/Sub/a.epub"), "x")
            .with(&kept("Books/Sub/a.epub"), "x")
            .with(&kept("Books/notes.xyz"), "x")
            .with(&kept("books/gone.epub"), "x"),
    );
    assert_eq!(
        clean_up_unused(&storage, &data, "books/", BookDataKind::Cache),
        Ok(1)
    );
    assert!(storage.exists(&kept("books/Sub/a.epub")));
    assert!(storage.exists(&kept("Books/Sub/a.epub")));
    assert!(storage.exists(&kept("Books/notes.xyz")));
    assert!(!storage.exists(&kept("books/gone.epub")));
    // A folder that cannot be listed: nothing goes.
    let storage = Folding(
        storage
            .0
            .with("broken/b.epub", "book")
            .with(&kept("books/gone.epub"), "x"),
    );
    assert_eq!(
        clean_up_unused(&storage, &data, "books", BookDataKind::Cache),
        Err(StorageError::Io)
    );
    assert!(storage.exists(&kept("books/gone.epub")));
}

/// A book not found (renamed on a computer, on a card not inserted)
/// keeps the reader's data when caches are cleaned up; it goes only
/// when asked for, and is counted first, once a book.
#[test]
fn caches_go_by_themselves_the_readers_data_only_when_asked() {
    let data = [
        BookData::new("notes", "", BookDataKind::UserData),
        BookData::new("places", "", BookDataKind::UserData),
        BookData::new("cache", "", BookDataKind::Cache),
    ];
    let gone = key("books/renamed.epub");
    let storage = MemoryStorage::new()
        .with("books/new name.epub", "book")
        .with(&format!("notes/{gone}"), "a note")
        .with(&format!("places/{gone}.ini"), "page 30")
        .with(&format!("cache/{gone}/0.txt"), "chapter");
    let cache = BookDataKind::Cache;
    assert_eq!(clean_up_unused(&storage, &data, "books", cache), Ok(1));
    assert!(storage.exists(&format!("notes/{gone}")));
    assert!(storage.exists(&format!("places/{gone}.ini")));
    assert!(!storage.exists(&format!("cache/{gone}/0.txt")));
    let user = BookDataKind::UserData;
    assert_eq!(unused_books(&storage, &data, "books", user), Ok(1));
    assert_eq!(clean_up_unused(&storage, &data, "books", user), Ok(2));
    assert!(!storage.exists(&format!("notes/{gone}")));
    assert_eq!(unused_books(&storage, &data, "books", user), Ok(0));
}

/// Books in hidden folders are found: what is kept for them stays.
#[test]
fn clean_up_finds_books_in_hidden_folders() {
    let data = [BookData::new("cache", "", BookDataKind::Cache)];
    let kept = |path: &str| format!("cache/{}.ini", key(path));
    let storage = MemoryStorage::new()
        .with(".hidden/a.epub", "book")
        .with("books/.b.epub", "book")
        .with(&kept(".hidden/a.epub"), "x")
        .with(&kept("books/.b.epub"), "x");
    assert_eq!(
        clean_up_unused(&storage, &data, "books", BookDataKind::Cache),
        Ok(0)
    );
    assert!(storage.exists(&kept(".hidden/a.epub")));
    assert!(storage.exists(&kept("books/.b.epub")));
}

/// A storage whose books cannot tell their size.
struct Sizeless(MemoryStorage);

impl Storage for Sizeless {
    fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, at, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        if path.starts_with("books/") {
            return Err(StorageError::Io);
        }
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
}

/// Details are not made anew where the book's size cannot be read;
/// where it changed (the file replaced), the day it came and the
/// plugins' fields stay.
#[test]
fn details_keep_what_cannot_be_read_again() {
    let kept = MemoryStorage::new().with("books/a.txt", "hello");
    let book = Book {
        size: 5,
        title: Some("Alice".into()),
        added: Some(20_000),
        fields: alloc::vec![("stars.rating".into(), "4".into())],
        ..Book::default()
    };
    save(&kept, "books/a.txt", &book).unwrap();
    let before = kept.get(&book_file("books/a.txt"));
    let storage = Sizeless(kept);
    set_finished(&storage, "books/a.txt", true);
    set_field(&storage, "books/a.txt", "other.x", "1");
    assert_eq!(storage.0.get(&book_file("books/a.txt")), before);
    // Replaced by a file of another size.
    let storage = storage.0.with("books/a.txt", "hello!");
    set_finished(&storage, "books/a.txt", true);
    let read = super::book(&storage, "books/a.txt").unwrap();
    assert_eq!(read.size, 6);
    assert_eq!(read.added, Some(20_000));
    assert_eq!(read.field("stars.rating"), Some("4"));
    assert_eq!(read.title, None, "the old file's");
    assert_eq!(read.progress, Some(100));
}

/// What a newer PaperPaw wrote in a book's details stays when this one
/// writes them.
#[test]
fn details_keep_what_a_newer_version_wrote() {
    let storage = MemoryStorage::new().with("books/a.txt", "hello").with(
        &book_file("books/a.txt"),
        "[book]\nsize = 5\nshelf = Garden\ntitle = Alice\n\
                 [field]\nstars.rating = 4\n[later]\nx = 1\n",
    );
    set_field(&storage, "books/a.txt", "other.x", "1");
    set_finished(&storage, "books/a.txt", true);
    let text = String::from_utf8(storage.get(&book_file("books/a.txt")).unwrap()).unwrap();
    let entries: Vec<(String, String, String)> = config::parse(&text)
        .filter_map(Result::ok)
        .map(|e| {
            (
                e.section.into(),
                e.key.into(),
                e.value.as_str().into_owned(),
            )
        })
        .collect();
    for (section, key, value) in [
        ("book", "shelf", "Garden"),
        ("book", "title", "Alice"),
        ("field", "stars.rating", "4"),
        ("field", "other.x", "1"),
        ("later", "x", "1"),
    ] {
        assert!(
            entries.contains(&(section.into(), key.into(), value.into())),
            "{section}.{key} in {text}"
        );
    }
    assert_eq!(
        super::book(&storage, "books/a.txt").unwrap().progress,
        Some(100)
    );
}

#[test]
fn unreadable_details_are_not_written_over() {
    let kept = MemoryStorage::new().with("books/a.txt", "hello");
    set_field(&kept, "books/a.txt", "stars.rating", "4");
    let before = kept.get(&book_file("books/a.txt"));
    let storage = Unreadable(kept);
    set_field(&storage, "books/a.txt", "other.x", "1");
    set_finished(&storage, "books/a.txt", true);
    provide(&storage, "books/a.txt", Provided::default());
    assert_eq!(storage.0.get(&book_file("books/a.txt")), before);
}

#[test]
fn details_of_many_tags_are_read_back() {
    let storage = MemoryStorage::new().with("books/a.txt", "hello");
    let book = Book {
        size: 5,
        indexed: true,
        title: Some("t".repeat(5000)),
        tags: (0..200).map(|i| format!("Subject number {i}")).collect(),
        fields: alloc::vec![("stars.rating".into(), "4".into())],
        ..Book::default()
    };
    assert!(save(&storage, "books/a.txt", &book).is_ok());
    let read = super::book(&storage, "books/a.txt").expect("read back");
    assert_eq!(read.tags.len(), MAX_TAGS);
    assert_eq!(read.title.as_deref().map(str::len), Some(MAX_VALUE));
    assert_eq!(read.field("stars.rating"), Some("4"));
    // A field set later keeps what other plugins set.
    set_field(&storage, "books/a.txt", "other.x", "1");
    let read = super::book(&storage, "books/a.txt").unwrap();
    assert_eq!(read.field("stars.rating"), Some("4"));
    assert!(read.indexed);
}

#[test]
fn unreadable_lists_are_not_written_over() {
    let kept = MemoryStorage::new();
    set_favourite(&kept, "a.epub", true);
    set_favourite(&kept, "b.epub", true);
    let before = kept.get(&favourites_file());
    let storage = Unreadable(kept);
    set_favourite(&storage, "c.epub", true);
    moved(&storage, "a.epub", "d.epub");
    removed(&storage, "b.epub");
    assert_eq!(storage.0.get(&favourites_file()), before);
    // Saved in another encoding (here Latin-1): kept as it is.
    let storage = MemoryStorage::new().with(&favourites_file(), b"b\xFCcher/a.epub".to_vec());
    set_favourite(&storage, "c.epub", true);
    assert_eq!(
        storage.get(&favourites_file()).as_deref(),
        Some(&b"b\xFCcher/a.epub"[..])
    );
    // Written by a newer PaperPaw (after going back to an older
    // update): left as it is.
    let newer = "# format 2\nbooks/a.epub";
    let storage = MemoryStorage::new().with(&favourites_file(), newer);
    set_favourite(&storage, "c.epub", true);
    assert_eq!(
        storage.get(&favourites_file()).as_deref(),
        Some(newer.as_bytes())
    );
    let storage = MemoryStorage::new()
        .with("books/a.txt", "hello")
        .with(&book_file("books/a.txt"), "# format 2\n[book]\nsize = 5\n");
    set_finished(&storage, "books/a.txt", true);
    assert!(
        storage
            .get(&book_file("books/a.txt"))
            .unwrap()
            .starts_with(b"# format 2")
    );
    // A byte order mark first, as Notepad writes: read past.
    let storage = MemoryStorage::new().with(&favourites_file(), "\u{feff}a.epub\r\nb.epub");
    assert_eq!(favourites(&storage), ["a.epub", "b.epub"]);
}

#[test]
fn favourites_newest_first() {
    let storage = MemoryStorage::new();
    set_favourite(&storage, "a.epub", true);
    set_favourite(&storage, "b.epub", true);
    set_favourite(&storage, "a.epub", true);
    assert_eq!(favourites(&storage), ["a.epub", "b.epub"]);
    set_favourite(&storage, "a.epub", false);
    assert_eq!(favourites(&storage), ["b.epub"]);
    moved(&storage, "b.epub", "c.epub");
    assert_eq!(favourites(&storage), ["c.epub"]);
}

#[test]
fn file_names_are_made_readable() {
    assert_eq!(readable_name("alice-in-wonderland"), "Alice In Wonderland");
    assert_eq!(readable_name("die_verwandlung"), "Die Verwandlung");
    assert_eq!(readable_name("Alice in Wonderland"), "Alice in Wonderland");
    assert_eq!(readable_name("McCarthy-The-Road"), "McCarthy-The-Road");
    assert_eq!(readable_name("2024-10-01"), "2024-10-01");
    assert_eq!(readable_name("plain"), "plain");
    let book = Book::default();
    assert_eq!(
        book.shown_title("books/die-verwandlung.txt"),
        "Die Verwandlung"
    );
}

#[test]
fn titles_fall_back_to_the_file_name() {
    let book = Book::default();
    assert_eq!(
        book.title_or("books/Alice in Wonderland.epub"),
        "Alice in Wonderland"
    );
    let book = Book {
        title: Some("Alice".into()),
        ..Book::default()
    };
    assert_eq!(book.title_or("books/x.epub"), "Alice");
}

#[test]
fn what_the_library_plugin_kept_is_taken_over() {
    let storage = MemoryStorage::new()
        .with(".paperpaw/plugins/files/opened.txt", "books/a.epub")
        .with(".paperpaw/plugins/files/books/0000000000000001.ini", "x");
    take_over_old(&storage);
    assert_eq!(opened_books(&storage), ["books/a.epub"]);
    assert!(storage.exists(".paperpaw/library/books/0000000000000001.ini"));
}

#[test]
fn books_without_an_extension_are_found_by_their_first_bytes() {
    let storage = crate::MemoryStorage::new()
        .with("books/a.txt", "text")
        .with("books/download", "BOOK and more")
        .with("books/notes", "plain")
        .with("books/photo.jpg", "BOOK too");
    let mut formats = crate::Formats::new();
    formats
        .add(
            crate::FormatSpec::new(
                crate::Id::new_static("test.book"),
                "book",
                alloc::vec![alloc::borrow::Cow::Borrowed("txt")],
                alloc::boxed::Box::new(|_, _, _| {
                    Err(crate::document::DocumentError::UnsupportedFormat)
                }),
            )
            .sniff(|head| head.starts_with(b"BOOK")),
        )
        .unwrap();
    let mut found = alloc::vec::Vec::new();
    books_in(&storage, &formats, "books", &mut |path| {
        found.push(String::from(path))
    });
    found.sort();
    // Not a file with another extension, whatever it holds.
    assert_eq!(found, ["books/a.txt", "books/download"]);
}

/// The details written in one pass are, byte for byte, what setting
/// their keys one by one wrote.
#[test]
fn details_are_written_as_key_by_key() {
    fn key_by_key(book: &Book, before: &str) -> String {
        let mut ini = String::new();
        let mut set = |key: &str, value: &str| ini = config::set(&ini, "book", key, fit(value));
        set("size", &book.size.to_string());
        set("indexed", if book.indexed { "on" } else { "off" });
        set("version", &DETAILS.to_string());
        for (key, value) in [
            ("title", &book.title),
            ("author", &book.author),
            ("series", &book.series),
            ("series-index", &book.series_index),
        ] {
            if let Some(value) = value {
                set(key, value);
            }
        }
        for (i, tag) in book.tags.iter().take(MAX_TAGS).enumerate() {
            set(&format!("tag.{}", i + 1), tag);
        }
        if let Some(cover) = &book.cover {
            set("cover", &cover.path);
            set("cover-width", &cover.width.to_string());
            set("cover-height", &cover.height.to_string());
        }
        if let Some(progress) = book.progress {
            set("progress", &progress.to_string());
        }
        if let Some(added) = book.added {
            set("added", &added.to_string());
        }
        for (id, value) in &book.fields {
            ini = config::set(&ini, "field", id, fit(value));
        }
        for entry in config::parse(before).filter_map(Result::ok) {
            if !known_detail(entry.section, entry.key) {
                ini = config::set(&ini, entry.section, entry.key, &entry.value.as_str());
            }
        }
        ini
    }
    let long = "x".repeat(MAX_VALUE + 10);
    let full = Book {
        size: 5,
        indexed: true,
        title: Some("A = \"title\"".into()),
        author: Some(" Ann\nLee ".into()),
        series: Some(long.clone()),
        series_index: Some(String::new()),
        tags: (0..MAX_TAGS + 3).map(|i| format!("tag {i}")).collect(),
        cover: Some(ImageRef {
            path: "x.gray".into(),
            width: 30,
            height: 40,
        }),
        progress: Some(12),
        added: Some(20_000),
        fields: alloc::vec![
            ("stars.rating".into(), "4".into()),
            ("notes.text".into(), long),
            ("stars.rating".into(), "5".into()),
            ("bad key".into(), "1".into()),
        ],
    };
    let befores = [
        "",
        "[book]\nsize = 5\nshelf = Garden\ntitle = Alice\n[field]\nstars.rating = 4\n[later]\nx = 1\n",
        "top = 1\nother = \"a\\nb\"\n[later]\nx = 1\ny = 2\n[book]\nshelf = A\nshelf = B\n[more]\nz = 3\n",
    ];
    for book in [Book::default(), full] {
        for before in befores {
            assert_eq!(
                details::try_details_text(&book, before).unwrap(),
                key_by_key(&book, before),
                "{before}"
            );
        }
    }
}

#[test]
fn only_a_catalog_whose_header_ends_with_nothing_after_it_is_empty() {
    let path = ".paperpaw/library/catalog.txt";
    // Cannot be told: none, unreadable, empty, another version, no header.
    assert!(!emptiness::is_empty(&MemoryStorage::new()));
    let header = MemoryStorage::new().with(path, "\tcatalog\t2\t1\n");
    assert!(!emptiness::is_empty(&Unreadable(header)));
    for text in [
        "",
        "\tcatalog\t",
        "\tcatalog\t3\t1\n",
        "books/a.txt\tA\tB\n",
        "x",
    ] {
        let storage = MemoryStorage::new().with(path, text);
        assert!(!emptiness::is_empty(&storage), "{text:?}");
    }
    // A header that does not end in what is read, with or without more
    // after it.
    let long = format!("\tcatalog\t2\t{}", "1".repeat(200));
    assert!(!emptiness::is_empty(&MemoryStorage::new().with(path, long)));
    let long = format!("\tcatalog\t2\t1\n{}", " ".repeat(200));
    assert!(!emptiness::is_empty(&MemoryStorage::new().with(path, long)));
    // The header alone (a walk of a library with nothing in it), with or
    // without its newline, then blank lines.
    for text in [
        "\tcatalog\t2\t1",
        "\tcatalog\t2\t1\n",
        "\tcatalog\t2\t7\n\n \n",
    ] {
        let storage = MemoryStorage::new().with(path, text);
        assert!(emptiness::is_empty(&storage), "{text:?}");
    }
    // The first book, with or without a blank line before it.
    for text in [
        "\tcatalog\t2\t1\nbooks/a.txt\tA\tB\n",
        "\tcatalog\t2\t1\n\nbooks/a.txt\tA\n",
    ] {
        let storage = MemoryStorage::new().with(path, text);
        assert!(!emptiness::is_empty(&storage), "{text:?}");
    }
}

#[test]
fn the_log_counts_as_entries_when_it_holds_anything() {
    let log = ".paperpaw/library/status.log";
    assert!(!emptiness::log_has_entries(&MemoryStorage::new()));
    // Cannot be read: as if it held some.
    assert!(emptiness::log_has_entries(&Unreadable(
        MemoryStorage::new().with(log, "1\n")
    )));
    assert!(!emptiness::log_has_entries(
        &MemoryStorage::new().with(log, "")
    ));
    assert!(emptiness::log_has_entries(
        &MemoryStorage::new().with(log, "1\t2\n")
    ));
}
