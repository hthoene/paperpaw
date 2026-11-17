use super::*;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;

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

/// A storage whose files cannot be read, as on a failing card: what is
/// there says [`StorageError::Io`] where it is read or its size asked
/// for. For tests of what must not be written over.
struct Unreadable(pub MemoryStorage);

impl Storage for Unreadable {
    fn read_at(&self, path: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
        self.size(path).map(|_| 0)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path).and(Err(StorageError::Io))
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
}
