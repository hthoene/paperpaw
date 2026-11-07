//! Storage helpers: which virtual paths are valid ([`is_valid_path`]),
//! how [`Mounted`] routes every operation to the base or the mounted
//! storage (including nested mounts, renames across the mount point and
//! paths that try to climb out), what [`NoStorage`] and the trait's
//! default methods do, and the errors' messages.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use paperpaw_platform::storage::{is_reserved_name, is_valid_path};
use paperpaw_platform::{DirEntry, Mounted, NoStorage, Storage, StorageError};
use proptest::prelude::*;

type Calls = Rc<RefCell<Vec<String>>>;

/// A storage that writes what was asked of it, as `name: call`, to a log
/// shared with the test, and answers with `result`.
struct Log {
    name: &'static str,
    calls: Calls,
    result: Result<(), StorageError>,
    entries: &'static [&'static str],
}

impl Log {
    fn new(name: &'static str, calls: &Calls) -> Self {
        Self {
            name,
            calls: calls.clone(),
            result: Ok(()),
            entries: &[],
        }
    }

    fn failing(mut self, error: StorageError) -> Self {
        self.result = Err(error);
        self
    }

    fn listing(mut self, entries: &'static [&'static str]) -> Self {
        self.entries = entries;
        self
    }

    fn note(&self, call: String) -> Result<(), StorageError> {
        self.calls
            .borrow_mut()
            .push(format!("{}: {call}", self.name));
        self.result
    }
}

impl Storage for Log {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.note(format!("read_at {path} {offset}"))
            .map(|()| buf.len().min(3))
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.note(format!("size {path}")).map(|()| 42)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note(format!("write {path} {}", data.len()))
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note(format!("append {path} {}", data.len()))
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.note(String::from("sync"))
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.note(format!("list {dir}"))?;
        for name in self.entries {
            visit(DirEntry::new(name, false, 1));
        }
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.note(format!("remove {path}"))
    }

    fn exists(&self, path: &str) -> bool {
        self.note(format!("exists {path}")).is_ok()
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.note(format!("rename {from} {to}"))
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.note(format!("create_dir {path}"))
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.note(format!("remove_dir_all {path}"))
    }

    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        self.note(format!("remove_tree_step {root} {keep_root} {place}"))?;
        Ok(paperpaw_platform::tree::TreeStep {
            removed: 0,
            done: true,
            next: 0,
        })
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        let _ = self.note(format!("local_path {path}"));
        with(&format!("/{}/{path}", self.name));
    }
}

/// A base with `books` mounted on it, and their shared log.
fn mounted() -> (Mounted<Log, Log>, Calls) {
    let calls = Calls::default();
    let storage = Mounted::new(Log::new("base", &calls), "books", Log::new("books", &calls));
    (storage, calls)
}

/// What one call on a fresh mounted storage asked of its parts.
fn routed(call: impl FnOnce(&Mounted<Log, Log>)) -> Vec<String> {
    let (storage, calls) = mounted();
    call(&storage);
    calls.take()
}

#[test]
fn every_operation_goes_to_the_storage_its_path_is_on() {
    let mut buf = [0u8; 8];
    assert_eq!(
        routed(|s| assert_eq!(s.read_at("books/a.epub", 5, &mut buf), Ok(3))),
        ["books: read_at a.epub 5"]
    );
    assert_eq!(
        routed(|s| assert_eq!(s.size("settings.ini"), Ok(42))),
        ["base: size settings.ini"]
    );
    assert_eq!(
        routed(|s| s.write("books/new/b.txt", b"abc").unwrap()),
        ["books: write new/b.txt 3"]
    );
    assert_eq!(
        routed(|s| s.append(".paperpaw/log", b"x").unwrap()),
        ["base: append .paperpaw/log 1"]
    );
    assert_eq!(
        routed(|s| s.remove("books/old.epub").unwrap()),
        ["books: remove old.epub"]
    );
    assert_eq!(routed(|s| assert!(s.exists("books"))), ["books: exists "]);
    assert_eq!(
        routed(|s| s.create_dir("books/series/one").unwrap()),
        ["books: create_dir series/one"]
    );
    assert_eq!(
        routed(|s| s.remove_dir_all("books/series").unwrap()),
        ["books: remove_dir_all series"]
    );
    assert_eq!(
        routed(|s| s.list("books/series", &mut |_| {}).unwrap()),
        ["books: list series"]
    );
    let mut local = Vec::new();
    assert_eq!(
        routed(|s| s.local_path("books/a.pdf", &mut |p| local.push(p.to_string()))),
        ["books: local_path a.pdf"]
    );
    assert_eq!(local, ["/books/a.pdf"]);
}

#[test]
fn only_whole_segments_match_the_mount_point() {
    for path in ["booksx/a", "book/a", "Books/a", "x/books/a", "booksbooks"] {
        assert_eq!(
            routed(|s| {
                let _ = s.size(path);
            }),
            [format!("base: size {path}")]
        );
    }
}

#[test]
fn renames_stay_within_one_storage() {
    assert_eq!(
        routed(|s| s.rename("books/a.epub", "books/b/a.epub").unwrap()),
        ["books: rename a.epub b/a.epub"]
    );
    assert_eq!(
        routed(|s| s.rename("a.ini", "b.ini").unwrap()),
        ["base: rename a.ini b.ini"]
    );
    for (from, to) in [
        ("books/a.epub", "a.epub"),
        ("a.epub", "books/a.epub"),
        ("books", "shelf"),
    ] {
        assert_eq!(
            routed(|s| assert_eq!(s.rename(from, to), Err(StorageError::Unsupported))),
            Vec::<String>::new(),
            "{from} to {to}"
        );
    }
}

/// A tree below the mount point goes to the mounted storage's own step, by
/// its path in it and with its place; the root is the base's own tree (the
/// mounted storage is not walked into); the mount point is never removed.
#[test]
fn tree_steps_go_to_the_storage_that_holds_the_tree() {
    assert_eq!(
        routed(|s| {
            s.remove_tree_step("books/series", false, 7).unwrap();
        }),
        ["books: remove_tree_step series false 7"]
    );
    assert_eq!(
        routed(|s| {
            s.remove_tree_step("data/x", true, 3).unwrap();
        }),
        ["base: remove_tree_step data/x true 3"]
    );
    assert_eq!(
        routed(|s| {
            s.remove_tree_step("", true, 0).unwrap();
        }),
        ["base: remove_tree_step  true 0"]
    );
    // The mount point's contents may be emptied, the mount point stays.
    assert_eq!(
        routed(|s| {
            s.remove_tree_step("books", true, 0).unwrap();
        }),
        ["books: remove_tree_step  true 0"]
    );
    assert_eq!(
        routed(|s| assert_eq!(
            s.remove_tree_step("books", false, 0).map(|_| ()),
            Err(StorageError::Unsupported)
        )),
        Vec::<String>::new()
    );
}

#[test]
fn the_mount_point_itself_is_never_removed_whole() {
    assert_eq!(
        routed(|s| assert_eq!(s.remove_dir_all("books"), Err(StorageError::Unsupported))),
        Vec::<String>::new()
    );
}

/// The base's error is told too: a clean-up must not take what is
/// there for gone.
#[test]
fn the_root_lists_the_mount_point_even_when_the_base_cannot() {
    let calls = Calls::default();
    let storage = Mounted::new(
        Log::new("base", &calls).failing(StorageError::NotFound),
        "books",
        Log::new("books", &calls),
    );
    let mut seen = Vec::new();
    let listed = storage.list("", &mut |e| seen.push((e.name.to_string(), e.is_dir)));
    assert_eq!(listed, Err(StorageError::NotFound));
    assert_eq!(seen, [(String::from("books"), true)]);
    assert_eq!(calls.take(), ["base: list "]);
}

#[test]
fn the_mount_point_lists_the_mounted_root() {
    let calls = Calls::default();
    let storage = Mounted::new(
        Log::new("base", &calls).listing(&["settings.ini"]),
        "books",
        Log::new("books", &calls).listing(&["a.epub", "b.epub"]),
    );
    let mut names = Vec::new();
    storage
        .list("books", &mut |e| names.push(e.name.to_string()))
        .unwrap();
    assert_eq!(names, ["a.epub", "b.epub"]);
    assert_eq!(calls.take(), ["books: list "]);
}

#[test]
fn sync_reaches_both_and_reports_a_failure_of_either() {
    for (base_fails, books_fails, expected) in [
        (false, false, Ok(())),
        (true, false, Err(StorageError::Io)),
        (false, true, Err(StorageError::Full)),
        (true, true, Err(StorageError::Full)),
    ] {
        let calls = Calls::default();
        let mut base = Log::new("base", &calls);
        let mut books = Log::new("books", &calls);
        if base_fails {
            base = base.failing(StorageError::Io);
        }
        if books_fails {
            books = books.failing(StorageError::Full);
        }
        let storage = Mounted::new(base, "books", books);
        assert_eq!(storage.sync(), expected);
        assert_eq!(calls.take(), ["base: sync", "books: sync"], "both synced");
    }
}

#[test]
fn nested_mounts_route_to_the_innermost() {
    let calls = Calls::default();
    let storage = Mounted::new(
        Mounted::new(Log::new("base", &calls), "books", Log::new("books", &calls)),
        "fonts",
        Log::new("fonts", &calls),
    );
    storage.size("fonts/serif.ttf").unwrap();
    storage.size("books/a.epub").unwrap();
    storage.size("settings.ini").unwrap();
    assert_eq!(
        calls.take(),
        [
            "fonts: size serif.ttf",
            "books: size a.epub",
            "base: size settings.ini"
        ]
    );
    let mut root = Vec::new();
    storage
        .list("", &mut |e| root.push(e.name.to_string()))
        .unwrap();
    root.sort();
    assert_eq!(root, ["books", "fonts"]);
    assert_eq!(
        storage.rename("books/a.epub", "fonts/a.epub"),
        Err(StorageError::Unsupported)
    );
    assert_eq!(
        storage.rename("books/a", "settings.ini"),
        Err(StorageError::Unsupported)
    );
}

/// Storages without any state of their own (e.g. one on a global file
/// system): two of them, each logging to its own static log.
struct GlobalA;
struct GlobalB;

static GLOBAL_CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

macro_rules! global_storage {
    ($ty:ident, $name:literal) => {
        impl Storage for $ty {
            fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
                Ok(0)
            }
            fn size(&self, _: &str) -> Result<u64, StorageError> {
                Ok(0)
            }
            fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
                Ok(())
            }
            fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
                Ok(())
            }
            fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
                Ok(())
            }
            fn remove(&self, _: &str) -> Result<(), StorageError> {
                Ok(())
            }
            fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
                GLOBAL_CALLS
                    .lock()
                    .unwrap()
                    .push(format!("{}: rename {from} {to}", $name));
                Ok(())
            }
        }
    };
}

global_storage!(GlobalA, "a");
global_storage!(GlobalB, "b");

#[test]
fn renames_across_the_mount_point_are_refused_for_stateless_storages_too() {
    let storage = Mounted::new(GlobalA, "books", GlobalB);
    let result = storage.rename("settings.ini", "books/settings.ini");
    let calls = std::mem::take(&mut *GLOBAL_CALLS.lock().unwrap());
    assert_eq!(result, Err(StorageError::Unsupported), "{calls:?}");
    assert!(calls.is_empty(), "{calls:?}");
}

#[test]
fn no_storage_is_empty_and_read_only() {
    let storage = NoStorage;
    let mut buf = [0u8; 4];
    assert_eq!(
        storage.read_at("a", 0, &mut buf),
        Err(StorageError::NotFound)
    );
    assert_eq!(storage.size("a"), Err(StorageError::NotFound));
    assert!(!storage.exists("a"));
    assert_eq!(storage.write("a", b"x"), Err(StorageError::Unsupported));
    assert_eq!(storage.append("a", b"x"), Err(StorageError::Unsupported));
    assert_eq!(
        storage.list("", &mut |_| panic!("nothing to list")),
        Err(StorageError::NotFound)
    );
    assert_eq!(storage.remove("a"), Ok(()), "removing what is not there");
    assert_eq!(storage.sync(), Ok(()));
    assert_eq!(storage.rename("a", "b"), Err(StorageError::Unsupported));
    assert_eq!(storage.create_dir("d"), Err(StorageError::Unsupported));
    assert_eq!(storage.remove_dir_all("d"), Err(StorageError::Unsupported));
    storage.local_path("a", &mut |_| panic!("no local path"));
}

#[test]
fn errors_say_what_went_wrong() {
    let messages: Vec<String> = [
        StorageError::NotFound,
        StorageError::InvalidPath,
        StorageError::Unsupported,
        StorageError::Full,
        StorageError::Io,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        messages,
        [
            "not found",
            "invalid path",
            "not supported",
            "storage full",
            "input/output error"
        ]
    );
    let error: &dyn std::error::Error = &StorageError::Full;
    assert!(error.source().is_none());
}

#[test]
fn paths_that_escape_or_are_not_relative_are_invalid() {
    for bad in [
        "",
        "/",
        "//",
        "/etc/passwd",
        "../x",
        "a/..",
        "a/../../b",
        "./a",
        "a/.",
        "a//b",
        "a/",
        "C:/x",
        "C:x",
        "\\\\server\\share",
        "a\\..\\b",
        "a\0b",
        "books/\0",
        "file:x",
    ] {
        assert!(!is_valid_path(bad), "{bad:?}");
    }
}

#[test]
fn names_that_fat_or_windows_read_as_another_are_invalid() {
    // A dot or space at the end is dropped, device names reach a device,
    // control characters have no place in a name.
    for bad in [
        "...",
        "a.",
        "books/a /b",
        "con",
        "books/NUL.txt",
        "com1.epub",
        "a\tb",
        "a\u{7f}",
    ] {
        assert!(!is_valid_path(bad), "{bad:?}");
    }
}

#[test]
fn names_with_dots_inside_and_unusual_characters_are_valid() {
    for ok in [
        "...a",
        ".a",
        "a..b",
        "..a",
        "a/.hidden/b",
        "b\u{fc}cher/stra\u{df}e.epub",
        "a b/c d.txt",
        "%2e%2e/x",
        "\u{1f43e}",
    ] {
        assert!(is_valid_path(ok), "{ok:?}");
    }
}

/// A valid path segment: no `/`, `\`, `:` or control character, no dot
/// or space at the end, not a Windows device name.
fn segment() -> impl Strategy<Value = String> {
    "[^/\\\\:\\p{Cc}]{1,8}".prop_filter("no dot or space at the end, no device", |s| {
        !s.ends_with(['.', ' ']) && !is_reserved_name(s)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn paths_of_valid_segments_are_valid(segments in proptest::collection::vec(segment(), 1..6)) {
        prop_assert!(is_valid_path(&segments.join("/")));
    }

    #[test]
    fn one_bad_segment_makes_a_path_invalid(
        segments in proptest::collection::vec(segment(), 0..5),
        bad in prop_oneof![Just(""), Just("."), Just(".."), Just("a\\b"), Just("c:"), Just("\0")],
        at in any::<usize>(),
    ) {
        let mut segments = segments;
        let at = at % (segments.len() + 1);
        segments.insert(at, bad.to_string());
        prop_assert!(!is_valid_path(&segments.join("/")));
    }

    #[test]
    fn arbitrary_text_never_validates_as_escaping(path in "\\PC{0,24}") {
        if is_valid_path(&path) {
            prop_assert!(!path.starts_with('/'));
            prop_assert!(!path.ends_with('/'));
            prop_assert!(!path.split('/').any(|s| s == ".." || s == "." || s.is_empty()));
            prop_assert!(!path.contains(['\\', ':', '\0']));
        }
    }

    #[test]
    fn routing_never_turns_an_invalid_path_valid(
        path in prop_oneof![
            "\\PC{0,16}",
            "books(/[a-z.]{0,3}){0,3}",
            "(\\.\\./|/)?books/\\.\\./[a-z]{1,4}",
        ],
    ) {
        let calls = routed(|s| {
            let _ = s.size(&path);
        });
        prop_assert_eq!(calls.len(), 1);
        let (storage, rest) = calls[0].split_once(": size ").unwrap();
        if is_valid_path(&path) {
            prop_assert!(is_valid_path(rest) || (storage == "books" && path == "books"));
        } else {
            prop_assert!(!is_valid_path(rest), "{path:?} became {rest:?} on {storage}");
        }
        if storage == "base" {
            prop_assert_eq!(rest, path.as_str(), "the base sees the path unchanged");
        }
    }
}
