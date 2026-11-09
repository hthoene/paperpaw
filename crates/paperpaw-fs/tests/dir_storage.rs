//! `DirStorage` on a real folder: paths that try to leave the root are
//! refused and touch nothing outside it; reads past the end; the file
//! kept open follows writes, appends, moves and removals; random
//! operations against a model in memory.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_fs::DirStorage;
use paperpaw_platform::{Storage, StorageError};
use proptest::prelude::*;

/// A folder of its own with the storage's root in it and, beside the
/// root, a folder `outside` with a file; removed when dropped.
struct Place {
    base: PathBuf,
    storage: DirStorage,
}

impl Place {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("paperpaw-fs-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(base.join("outside/secret.txt"), b"secret").unwrap();
        let storage = DirStorage::new(base.join("root")).unwrap();
        Self { base, storage }
    }

    /// The names in the base folder and in `outside`: what a path that
    /// escaped would have changed.
    fn around(&self) -> Vec<String> {
        let names = |dir: &Path| -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        let mut all = names(&self.base);
        all.extend(names(&self.base.join("outside")));
        all
    }

    fn untouched(&self) {
        assert_eq!(self.around(), ["outside", "root", "secret.txt"]);
        assert_eq!(
            std::fs::read(self.base.join("outside/secret.txt")).unwrap(),
            b"secret"
        );
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn read(storage: &DirStorage, path: &str) -> Vec<u8> {
    let mut data = vec![0u8; storage.size(path).unwrap() as usize];
    assert_eq!(storage.read_at(path, 0, &mut data), Ok(data.len()));
    data
}

fn names(storage: &DirStorage, dir: &str) -> Vec<String> {
    let mut names = Vec::new();
    storage
        .list(dir, &mut |e| names.push(e.name.to_string()))
        .unwrap();
    names.sort();
    names
}

const BAD_PATHS: [&str; 16] = [
    "..",
    "../outside/secret.txt",
    "a/../../outside/secret.txt",
    "a/../b",
    ".",
    "./a",
    "/outside/secret.txt",
    "/",
    "..\\outside\\secret.txt",
    "a\\..\\..\\outside",
    "C:/Windows",
    "C:\\Windows",
    "c:x",
    "a//b",
    "x/",
    "a\0b",
];

#[test]
fn paths_that_leave_the_root_are_refused() {
    let place = Place::new();
    let s = &place.storage;
    s.write("a/b.txt", b"inside").unwrap();
    let invalid = StorageError::InvalidPath;
    let mut buf = [0u8; 8];
    for path in BAD_PATHS {
        assert_eq!(s.read_at(path, 0, &mut buf), Err(invalid), "{path:?}");
        assert_eq!(s.size(path), Err(invalid), "{path:?}");
        assert!(!s.exists(path), "{path:?}");
        assert_eq!(s.write(path, b"x"), Err(invalid), "{path:?}");
        assert_eq!(s.append(path, b"x"), Err(invalid), "{path:?}");
        assert_eq!(s.remove(path), Err(invalid), "{path:?}");
        assert_eq!(s.rename("a/b.txt", path), Err(invalid), "{path:?}");
        assert_eq!(s.rename(path, "a/c.txt"), Err(invalid), "{path:?}");
        assert_eq!(s.create_dir(path), Err(invalid), "{path:?}");
        assert_eq!(s.remove_dir_all(path), Err(invalid), "{path:?}");
        assert_eq!(s.list(path, &mut |_| {}), Err(invalid), "{path:?}");
        let mut called = false;
        s.local_path(path, &mut |_| called = true);
        assert!(!called, "{path:?}");
    }
    // An absolute path of this very machine.
    let absolute = place.base.join("outside").join("secret.txt");
    let absolute = absolute.to_str().unwrap();
    assert_eq!(s.write(absolute, b"x"), Err(invalid));
    assert_eq!(s.remove(absolute), Err(invalid));
    place.untouched();
    assert_eq!(read(s, "a/b.txt"), b"inside");
}

/// The empty path is the root, for listing it; a file cannot be there.
#[test]
fn writing_the_empty_path_touches_nothing_outside() {
    let place = Place::new();
    assert!(place.storage.write("", b"x").is_err());
    assert!(place.storage.append("", b"x").is_err());
    place.untouched();
}

/// A file beside the root named like the root's temporary is outside the
/// storage: writing the empty path must not open it.
#[test]
fn writing_the_empty_path_keeps_a_file_beside_the_root() {
    let place = Place::new();
    let beside = place.base.join("root.paperpaw-tmp");
    std::fs::write(&beside, b"not the storage's").unwrap();
    assert!(place.storage.write("", b"x").is_err());
    assert_eq!(
        std::fs::read(&beside).ok().as_deref(),
        Some(&b"not the storage's"[..])
    );
}

/// Removing "everything below the empty path" would remove the root.
#[test]
fn removing_the_empty_path_keeps_the_root() {
    let place = Place::new();
    place.storage.write("books/a.epub", b"a book").unwrap();
    let _ = place.storage.remove_dir_all("");
    let _ = place.storage.remove("");
    assert_eq!(read(&place.storage, "books/a.epub"), b"a book");
}

/// Windows drops dots and spaces at the end of a name, so `...` is the
/// folder it is in: a name of dots must never stand for its parent.
#[test]
fn names_of_dots_do_not_stand_for_their_folder() {
    let place = Place::new();
    let s = &place.storage;
    s.write("books/a.epub", b"a book").unwrap();
    let _ = s.remove_dir_all("books/...");
    let _ = s.remove_dir_all("...");
    let _ = s.write("books/. .", b"x");
    assert_eq!(read(s, "books/a.epub"), b"a book");
    assert_eq!(names(s, "books"), ["a.epub"]);
    place.untouched();
}

#[test]
fn reads_past_the_end_read_nothing() {
    let place = Place::new();
    let s = &place.storage;
    s.write("a.bin", &[7u8; 3000]).unwrap();
    let mut buf = [0u8; 100];
    assert_eq!(s.read_at("a.bin", 2950, &mut buf), Ok(50));
    assert_eq!(s.read_at("a.bin", 3000, &mut buf), Ok(0));
    assert_eq!(s.read_at("a.bin", 1 << 40, &mut buf), Ok(0));
    assert_eq!(s.read_at("a.bin", 0, &mut []), Ok(0));
    assert_eq!(
        s.read_at("none.bin", 0, &mut buf),
        Err(StorageError::NotFound)
    );
    s.write("empty", b"").unwrap();
    assert_eq!(s.read_at("empty", 0, &mut buf), Ok(0));
}

/// The largest offset is past the end as well.
#[test]
fn reads_at_the_largest_offset_read_nothing() {
    let place = Place::new();
    place.storage.write("a.bin", &[7u8; 30]).unwrap();
    let mut buf = [0u8; 10];
    assert_eq!(place.storage.read_at("a.bin", u64::MAX, &mut buf), Ok(0));
}

/// The file read last stays open; whatever happens to it after, a read
/// sees the file as it is now.
#[test]
fn the_open_file_follows_changes() {
    let place = Place::new();
    let s = &place.storage;
    let mut buf = [0u8; 32];
    s.write("a.txt", b"first content").unwrap();
    assert_eq!(read(s, "a.txt"), b"first content");
    s.write("a.txt", b"second").unwrap();
    assert_eq!(read(s, "a.txt"), b"second");
    s.append("a.txt", b" and more").unwrap();
    assert_eq!(read(s, "a.txt"), b"second and more");
    // Replaced by a move, while open.
    s.write("b.txt", b"from b").unwrap();
    assert_eq!(read(s, "a.txt"), b"second and more");
    s.rename("b.txt", "a.txt").unwrap();
    assert_eq!(read(s, "a.txt"), b"from b");
    assert!(!s.exists("b.txt"));
    // Removed while open.
    s.remove("a.txt").unwrap();
    assert_eq!(s.read_at("a.txt", 0, &mut buf), Err(StorageError::NotFound));
    // Its folder removed while open.
    s.write("d/c.txt", b"c").unwrap();
    assert_eq!(read(s, "d/c.txt"), b"c");
    s.remove_dir_all("d").unwrap();
    assert_eq!(
        s.read_at("d/c.txt", 0, &mut buf),
        Err(StorageError::NotFound)
    );
}

/// Writes go through a temporary file: none is left, also when a write
/// replaces a file.
#[test]
fn writes_leave_no_temporary_files() {
    let place = Place::new();
    let s = &place.storage;
    s.write("notes/a.txt", b"one").unwrap();
    s.write("notes/a.txt", b"two").unwrap();
    s.write("notes/b", b"no extension").unwrap();
    s.write("notes/c.tar.gz", b"two extensions").unwrap();
    assert_eq!(names(s, "notes"), ["a.txt", "b", "c.tar.gz"]);
}

#[test]
fn folders_and_files_are_told_apart() {
    let place = Place::new();
    let s = &place.storage;
    s.write("dir/file.txt", b"x").unwrap();
    // A folder has no size and is no file.
    assert_eq!(s.size("dir"), Err(StorageError::NotFound));
    assert!(!s.exists("dir"));
    assert!(s.list("dir/file.txt", &mut |_| {}).is_err());
    assert_eq!(s.list("none", &mut |_| {}), Err(StorageError::NotFound));
    // Listed with their sizes; folders with none.
    s.create_dir("dir/sub").unwrap();
    s.create_dir("dir/sub").unwrap();
    let mut entries = Vec::new();
    s.list("dir", &mut |e| {
        entries.push((e.name.to_string(), e.is_dir, e.size))
    })
    .unwrap();
    entries.sort();
    assert_eq!(
        entries,
        [
            ("file.txt".to_string(), false, 1),
            ("sub".to_string(), true, 0)
        ]
    );
    // Missing things are removed without complaint.
    assert_eq!(s.remove("none.txt"), Ok(()));
    assert_eq!(s.remove_dir_all("none"), Ok(()));
    // Moves create the folders they need and take folders along.
    s.rename("dir", "moved/here/dir").unwrap();
    assert_eq!(read(s, "moved/here/dir/file.txt"), b"x");
    assert_eq!(s.rename("none.txt", "x.txt"), Err(StorageError::NotFound));
    // The file's own path in the host's file system, inside the root.
    let mut local = String::new();
    s.local_path("moved/here/dir/file.txt", &mut |p| local = p.to_string());
    assert!(
        Path::new(&local).starts_with(place.base.join("root")),
        "{local}"
    );
    assert_eq!(std::fs::read(&local).unwrap(), b"x");
}

const PATHS: [&str; 5] = ["a.txt", "b.bin", "books/c.epub", "books/deep/d.txt", "e"];

#[derive(Debug, Clone)]
enum Op {
    Write(usize, Vec<u8>),
    Append(usize, Vec<u8>),
    Read(usize, u64, usize),
    Remove(usize),
    Rename(usize, usize),
}

fn op() -> impl Strategy<Value = Op> {
    let path = 0..PATHS.len();
    let data = prop::collection::vec(any::<u8>(), 0..3000);
    prop_oneof![
        (path.clone(), data.clone()).prop_map(|(p, d)| Op::Write(p, d)),
        (path.clone(), data).prop_map(|(p, d)| Op::Append(p, d)),
        (path.clone(), 0u64..7000, 0usize..2000).prop_map(|(p, o, n)| Op::Read(p, o, n)),
        path.clone().prop_map(Op::Remove),
        (path.clone(), path).prop_map(|(a, b)| Op::Rename(a, b)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(32)))]

    #[test]
    fn storage_follows_the_model(ops in prop::collection::vec(op(), 1..30)) {
        let place = Place::new();
        let s = &place.storage;
        let mut model: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
        for op in ops {
            match op {
                Op::Write(p, d) => {
                    s.write(PATHS[p], &d).unwrap();
                    model.insert(PATHS[p], d);
                }
                Op::Append(p, d) => {
                    s.append(PATHS[p], &d).unwrap();
                    model.entry(PATHS[p]).or_default().extend_from_slice(&d);
                }
                Op::Read(p, offset, len) => {
                    let mut buf = vec![0u8; len];
                    let result = s.read_at(PATHS[p], offset, &mut buf);
                    match model.get(PATHS[p]) {
                        Some(data) => {
                            let start = (offset as usize).min(data.len());
                            let end = (start + len).min(data.len());
                            prop_assert_eq!(result, Ok(end - start));
                            prop_assert!(buf[..end - start] == data[start..end]);
                        }
                        None => prop_assert_eq!(result, Err(StorageError::NotFound)),
                    }
                }
                Op::Remove(p) => {
                    s.remove(PATHS[p]).unwrap();
                    model.remove(PATHS[p]);
                }
                Op::Rename(a, b) => {
                    let result = s.rename(PATHS[a], PATHS[b]);
                    match model.remove(PATHS[a]) {
                        Some(data) => {
                            prop_assert_eq!(result, Ok(()));
                            model.insert(PATHS[b], data);
                        }
                        None => prop_assert!(result.is_err()),
                    }
                }
            }
            for path in PATHS {
                match model.get(path) {
                    Some(data) => prop_assert!(read(s, path) == *data, "{}", path),
                    None => prop_assert!(!s.exists(path), "{}", path),
                }
            }
        }
        place.untouched();
    }
}

/// `remove_tree_step` on the host's file system: a big folder goes in pieces
/// within the limits, a tree 256 levels deep is removed, one level more is
/// refused (`InvalidData`, what the service answers `ERR_STATE`), the root is
/// kept when asked, and nothing outside the tree is touched.
#[test]
fn trees_are_removed_in_bounded_pieces_to_the_depth_limit() {
    use paperpaw_platform::tree::{MAX_READS, MAX_REMOVALS};
    let place = Place::new();
    let s = &place.storage;
    for n in 0..300 {
        s.write(&format!("Big/f{n:03}.txt"), b"x").unwrap();
    }
    s.write("Keep/k.txt", b"keep").unwrap();
    let mut calls = 0;
    loop {
        let step = s.remove_tree_step("Big", true, 1).unwrap();
        assert!(step.removed <= MAX_REMOVALS && MAX_READS >= 256);
        calls += 1;
        assert!(calls < 100);
        if step.done {
            break;
        }
    }
    assert!(calls >= 300 / 64, "{calls}");
    let mut left = 0;
    s.list("Big", &mut |_| left += 1).unwrap();
    assert_eq!(left, 0, "kept root, empty");
    assert_eq!(s.size("Keep/k.txt"), Ok(4));
    s.remove_tree_step("Big", false, 0).unwrap();
    assert_eq!(s.size("Big/x"), Err(StorageError::NotFound));

    let mut deep = place.base.join("root");
    for levels in [256usize, 257] {
        let mut dir = deep.join(format!("T{levels}"));
        for _ in 0..levels {
            dir.push("a");
        }
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("leaf.txt"), b"leaf").unwrap();
        let name = format!("T{levels}");
        let mut outcome = Ok(());
        for _ in 0..5000 {
            match s.remove_tree_step(&name, false, 1) {
                Ok(step) if step.done => break,
                Ok(_) => {}
                Err(e) => {
                    outcome = Err(e);
                    break;
                }
            }
        }
        match levels {
            256 => {
                assert_eq!(outcome, Ok(()));
                assert!(!deep.join(&name).exists());
            }
            _ => assert_eq!(outcome, Err(StorageError::InvalidData)),
        }
    }
    deep.push("x");
    assert_eq!(s.size("Keep/k.txt"), Ok(4));
}

/// The file id: kept by a rename and a move, another for another entry, an
/// error for nothing; a link answers its own, never its target's.
#[cfg(unix)]
#[test]
fn entry_identity_is_the_file_id_kept_by_a_move() {
    let place = Place::new();
    let s = &place.storage;
    s.write("Books/a.epub", b"a book").unwrap();
    s.write("Books/b.epub", b"another").unwrap();
    s.write("empty.txt", b"").unwrap();
    let book = s.entry_identity("Books/a.epub").unwrap().unwrap();
    assert_ne!(Some(book), s.entry_identity("Books/b.epub").unwrap());
    // Unlike FAT, an empty file has one here.
    assert!(s.entry_identity("empty.txt").unwrap().is_some());
    s.rename("Books/a.epub", "Inbox/a.epub").unwrap();
    assert_eq!(s.entry_identity("Inbox/a.epub"), Ok(Some(book)));
    assert_eq!(
        s.entry_identity("Books/a.epub"),
        Err(StorageError::NotFound)
    );
    std::os::unix::fs::symlink(
        place.base.join("root/Inbox/a.epub"),
        place.base.join("root/link.epub"),
    )
    .unwrap();
    assert_ne!(s.entry_identity("link.epub"), Ok(Some(book)));
    // Two hard links share the id: neither has one to tell a move by.
    std::fs::hard_link(
        place.base.join("root/Inbox/a.epub"),
        place.base.join("root/twin.epub"),
    )
    .unwrap();
    assert_eq!(s.entry_identity("twin.epub"), Ok(None));
    assert_eq!(s.entry_identity("Inbox/a.epub"), Ok(None));
    // A folder keeps its own whatever subfolders it holds.
    s.write("Inbox/Sub/c.epub", b"c").unwrap();
    assert!(s.entry_identity("Inbox").unwrap().is_some());
}

/// The same name in the same folder is one entry; two entries are two;
/// what names nothing, or is refused, is none of them (`Some(false)`).
#[cfg(unix)]
#[test]
fn same_entry_tells_one_entry_from_two() {
    let place = Place::new();
    let s = &place.storage;
    s.write("Books/a.txt", b"a").unwrap();
    s.write("Books/b.txt", b"b").unwrap();
    s.write(".paperpaw/secrets", b"s").unwrap();
    let same = |a, b| s.same_entry(a, b);
    assert_eq!(same("Books/a.txt", "Books/a.txt"), Some(true));
    assert_eq!(same("Books", "Books"), Some(true));
    assert_eq!(same(".paperpaw/secrets", ".paperpaw/secrets"), Some(true));
    assert_eq!(same("Books/a.txt", "Books/b.txt"), Some(false));
    assert_eq!(same("Books", "Books/a.txt"), Some(false));
    // The secrets file is its own entry, whatever is beside it.
    assert_eq!(same(".paperpaw/secrets", "Books/a.txt"), Some(false));
    // Names nothing.
    assert_eq!(same("Books/a.txt", "Books/none.txt"), Some(false));
    assert_eq!(same("none", "none"), Some(false));
    // Refused paths: not an entry of this storage.
    assert_eq!(same("../outside/secret.txt", "Books/a.txt"), Some(false));
    assert_eq!(
        same("../outside/secret.txt", "../outside/secret.txt"),
        Some(false)
    );
    assert_eq!(same("", ""), Some(false));
    place.untouched();
}

/// Another case of a name is the same entry exactly where the file system
/// ignores case (a case-only rename); where it does not, it is two.
#[cfg(unix)]
#[test]
fn same_entry_follows_the_file_systems_case() {
    let place = Place::new();
    let s = &place.storage;
    s.write("Books/A.txt", b"a").unwrap();
    assert_eq!(
        s.same_entry("Books/A.txt", "Books/a.TXT"),
        Some(s.ignores_case())
    );
    // The folder in another case is the same folder on such a file system.
    assert_eq!(
        s.same_entry("Books/A.txt", "books/A.txt"),
        Some(s.ignores_case())
    );
}

/// Nothing is followed at the last name: a second name for the same file
/// (a hard link, a symlink) is another entry, so a move onto it still
/// checks that the target exists and never replaces the book.
#[cfg(unix)]
#[test]
fn same_entry_does_not_follow_links() {
    let place = Place::new();
    let s = &place.storage;
    s.write("a.txt", b"a").unwrap();
    s.write("other.txt", b"o").unwrap();
    let root = place.base.join("root");
    std::fs::hard_link(root.join("a.txt"), root.join("hard.txt")).unwrap();
    std::os::unix::fs::symlink("a.txt", root.join("soft.txt")).unwrap();
    assert_eq!(s.same_entry("a.txt", "hard.txt"), Some(false));
    assert_eq!(s.same_entry("hard.txt", "a.txt"), Some(false));
    assert_eq!(s.same_entry("a.txt", "soft.txt"), Some(false));
    assert_eq!(s.same_entry("soft.txt", "a.txt"), Some(false));
    assert_eq!(s.same_entry("other.txt", "soft.txt"), Some(false));
    // A link is itself, by its own name.
    assert_eq!(s.same_entry("soft.txt", "soft.txt"), Some(true));
    // A link to nothing is an entry too, and itself.
    std::os::unix::fs::symlink("gone", root.join("dangling")).unwrap();
    assert_eq!(s.same_entry("dangling", "dangling"), Some(true));
    // A link out of the root is no entry inside by another name.
    std::os::unix::fs::symlink(place.base.join("outside/secret.txt"), root.join("out")).unwrap();
    assert_eq!(s.same_entry("out", "a.txt"), Some(false));
    // Hard links in two folders under one name are two entries.
    s.write("one/x.txt", b"x").unwrap();
    std::fs::create_dir_all(root.join("two")).unwrap();
    std::fs::hard_link(root.join("one/x.txt"), root.join("two/x.txt")).unwrap();
    assert_eq!(s.same_entry("one/x.txt", "two/x.txt"), Some(false));
    // A folder reached by a link is the folder: the same entry of it.
    std::os::unix::fs::symlink("one", root.join("dirlink")).unwrap();
    assert_eq!(s.same_entry("dirlink/x.txt", "one/x.txt"), Some(true));
}

/// A root that is gone cannot tell.
#[test]
fn same_entry_cannot_tell_without_the_root() {
    let place = Place::new();
    place.storage.write("a.txt", b"a").unwrap();
    std::fs::remove_dir_all(place.base.join("root")).unwrap();
    assert_eq!(place.storage.same_entry("a.txt", "a.txt"), None);
}

/// Where the path has not been run, there is no answer to give.
#[cfg(windows)]
#[test]
fn same_entry_is_not_answered_on_windows() {
    let place = Place::new();
    place.storage.write("a.txt", b"a").unwrap();
    assert_eq!(place.storage.same_entry("a.txt", "a.txt"), None);
    assert_eq!(place.storage.same_entry("a.txt", "A.TXT"), None);
}
