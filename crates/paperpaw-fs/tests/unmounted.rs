//! A folder storage whose root is gone (an SD card pulled, a share revoked)
//! says so: `CARD_GONE` on every operation, also behind a mount point, so
//! that the library never takes the books on it for deleted (#228).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_fs::DirStorage;
use paperpaw_platform::{Mounted, Storage, StorageError};

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-gone-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn router(name: &str) -> (Mounted<DirStorage, DirStorage>, std::path::PathBuf) {
    let (base, card) = (temp(&format!("{name}-base")), temp(&format!("{name}-card")));
    let storage = Mounted::new(
        DirStorage::new(&base).unwrap(),
        "books",
        DirStorage::new(&card).unwrap(),
    );
    (storage, card)
}

#[test]
fn a_mounted_volume_answers_as_before() {
    let (storage, _card) = router("mounted");
    storage.write("books/a.epub", b"abc").unwrap();
    assert_eq!(storage.size("books/a.epub"), Ok(3));
    assert_eq!(storage.size("books/b.epub"), Err(StorageError::NotFound));
    assert!(storage.list_from("books", 0, &mut |_| true).is_ok());
    assert!(storage.list_from("", 0, &mut |_| true).is_ok());
}

#[test]
fn an_unmounted_volume_is_card_gone_not_not_found() {
    let (storage, card) = router("unmounted");
    storage.write("books/a.epub", b"abc").unwrap();
    std::fs::remove_dir_all(&card).unwrap();

    let gone = StorageError::CARD_GONE;
    assert_eq!(storage.size("books/a.epub"), Err(gone));
    assert_eq!(storage.read_at("books/b.epub", 0, &mut [0; 4]), Err(gone));
    // A root probe of the mount point fails, it does not list.
    assert_eq!(storage.list_from("books", 0, &mut |_| true), Err(gone));
    assert_eq!(storage.list("books", &mut |_| {}), Err(gone));
    assert_eq!(
        storage.list_from("books/Fiction", 0, &mut |_| true),
        Err(gone)
    );
    assert!(!storage.exists("books/a.epub"));
    assert_eq!(storage.modified("books/a.epub"), None);
    // Writing does not make the mount point again on the wrong disk.
    assert_eq!(storage.write("books/c.epub", b"x"), Err(gone));
    assert_eq!(storage.create_dir("books/new"), Err(gone));
    assert!(!card.exists());
    // The base is untouched: its own files and the root still answer.
    storage.write("data/x", b"1").unwrap();
    assert_eq!(storage.size("data/x"), Ok(1));
    assert!(storage.list_from("", 0, &mut |_| true).is_ok());
}

#[test]
fn a_path_that_cannot_be_named_is_still_invalid_when_the_volume_is_gone() {
    let (storage, card) = router("invalid");
    std::fs::remove_dir_all(&card).unwrap();
    assert_eq!(storage.size("books/../x"), Err(StorageError::InvalidPath));
}

#[test]
fn a_volume_that_comes_back_works_again() {
    let (storage, card) = router("back");
    std::fs::remove_dir_all(&card).unwrap();
    assert_eq!(storage.size("books/a.epub"), Err(StorageError::CARD_GONE));
    std::fs::create_dir_all(&card).unwrap();
    assert_eq!(storage.size("books/a.epub"), Err(StorageError::NotFound));
    storage.write("books/a.epub", b"abc").unwrap();
    assert_eq!(storage.size("books/a.epub"), Ok(3));
}

#[test]
fn the_file_read_last_is_not_read_from_a_pulled_volume() {
    let (storage, card) = router("cached");
    storage.write("books/a.epub", b"abcdef").unwrap();
    // Reads it, so the handle stays open for the next read.
    let mut buf = [0; 3];
    assert_eq!(storage.read_at("books/a.epub", 0, &mut buf), Ok(3));
    std::fs::remove_dir_all(&card).unwrap();
    // The open file is looked at again after a short while, not every read.
    std::thread::sleep(std::time::Duration::from_millis(80));
    assert_eq!(
        storage.read_at("books/a.epub", 3, &mut buf),
        Err(StorageError::CARD_GONE)
    );
    // And the handle is gone: a volume that is back is read afresh.
    assert_eq!(
        storage.read_at("books/a.epub", 3, &mut buf),
        Err(StorageError::CARD_GONE)
    );
}

#[test]
fn a_root_deleted_while_running_is_not_made_again() {
    let (storage, card) = router("not-made");
    std::fs::remove_dir_all(&card).unwrap();
    assert_eq!(storage.create_dir("books/x"), Err(StorageError::CARD_GONE));
    assert_eq!(
        storage.write("books/x/y", b"1"),
        Err(StorageError::CARD_GONE)
    );
    assert!(!card.exists());
}

/// A root the system refuses to look at (access revoked) is gone too.
#[cfg(unix)]
#[test]
fn a_root_without_permission_is_card_gone() {
    use std::os::unix::fs::PermissionsExt;
    let parent = temp("denied");
    let root = parent.join("card");
    let storage = DirStorage::new(&root).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root (uid 0) looks anyway: nothing to check then.
    let refused = std::fs::metadata(&root).is_err();
    let answer = storage.size("a.epub");
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    if refused {
        assert_eq!(answer, Err(StorageError::CARD_GONE));
    }
}
