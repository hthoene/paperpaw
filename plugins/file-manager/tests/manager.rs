//! The file manager with the real kernel: large folders a page at a time,
//! what it tells when folders move, settings that follow a renamed
//! folder, and what was picked going with a deleted folder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::{Confirmed, Typed};
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass, MemoryStorage, Params,
    PixelFormat, Plugin, RegisterError, Registrar, Storage, StorageError, Version,
};

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

/// The books said to have moved: from where, to where.
type Moved = Rc<RefCell<Vec<(String, String)>>>;

/// Hears which books were said to have moved.
struct Spy(Moved);

impl Plugin for Spy {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("spy"), "Spy", Version::new(0, 1, 0))
            .description("Hears books moving.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let moved = Rc::clone(&self.0);
        r.on(events::BOOK_MOVED, move |_, m, _| {
            moved.borrow_mut().push((m.from.clone(), m.path.clone()));
        })
    }
}

/// The file manager in folder `dir` of `storage`; and the books said to
/// have moved.
fn manager(storage: impl Storage + 'static, dir: &str) -> (Kernel, Moved) {
    let moved = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[core]\nlanguage = en\n")
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(paperpaw_plugin_files::Files)
        .plugin(paperpaw_plugin_file_manager::FileManager)
        .plugin(Spy(Rc::clone(&moved)))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    settle(&mut kernel, effects);
    let screen = Id::new_static("file-manager.browser");
    let effects = kernel.open_screen(Instant(0), screen, Params::new().with(params::PATH, dir));
    settle(&mut kernel, effects);
    (kernel, moved)
}

fn settle(kernel: &mut Kernel, mut effects: Effects) {
    let mut now = 0;
    for _ in 0..200 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 10_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    kernel.render(&mut |_, _| {});
}

fn shown(kernel: &mut Kernel) -> Vec<String> {
    kernel.render(&mut |_, _| {});
    kernel.describe().into_iter().map(|p| p.label).collect()
}

/// What has the focus: on the screen on top, where one is over another.
fn focused(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    let parts = kernel.describe();
    paperpaw_sdk::semantics::focused(&parts)
        .map(|p| p.label.clone())
        .unwrap_or_default()
}

fn rename(kernel: &mut Kernel, path: &str, name: &str) {
    let typed = Typed::new(name, Params::new().with(params::PATH, path));
    let event = Id::new_static("file-manager.renamed");
    let effects = kernel.send_event(Instant(0), event, typed.to_params());
    settle(kernel, effects);
}

fn choose(kernel: &mut Kernel, path: &str, option: &str) {
    let picked = Picked::new(path.into(), option.into());
    let event = Id::new_static("file-manager.chosen");
    let effects = kernel.send_event(Instant(0), event, picked.to_params());
    settle(kernel, effects);
}

/// Answers the question whether to delete `path`: yes, as if asked.
fn delete(kernel: &mut Kernel, path: &str) {
    let answer = Confirmed::new(true, Params::new().with(params::PATH, path));
    let event = Id::new_static("file-manager.delete");
    let effects = kernel.send_event(Instant(0), event, answer.to_params());
    settle(kernel, effects);
}

/// Presses down until the button `label` has the focus, then confirm.
fn press(kernel: &mut Kernel, label: &str) {
    for _ in 0..40 {
        if focused(kernel) == label {
            let effects = kernel.run_action(Instant(0), Id::new_static("core.confirm"));
            return settle(kernel, effects);
        }
        let effects = kernel.run_action(Instant(0), Id::new_static("core.down"));
        settle(kernel, effects);
    }
    panic!("no button {label}: {:?}", shown(kernel));
}

/// A folder of many files goes a page at a time, the focus where the
/// buttons take it.
#[test]
fn a_large_folder_page_by_page() {
    let storage = (0..60).fold(MemoryStorage::new(), |card, i| {
        card.with(&format!("big/file {i:02}.dat"), "x")
    });
    let (mut kernel, _) = manager(storage, "big");
    for _ in 0..30 {
        let effects = kernel.run_action(Instant(0), Id::new_static("core.down"));
        settle(&mut kernel, effects);
    }
    assert!(
        focused(&mut kernel).contains("file 30.dat"),
        "{}",
        focused(&mut kernel)
    );
    for _ in 0..3 {
        let effects = kernel.run_action(Instant(0), Id::new_static("core.up"));
        settle(&mut kernel, effects);
    }
    assert!(
        focused(&mut kernel).contains("file 27.dat"),
        "{}",
        focused(&mut kernel)
    );
}

/// A folder renamed tells of the books in it moving, not of other files.
#[test]
fn only_books_are_told_moving() {
    let storage = MemoryStorage::new()
        .with("books/Series/a.txt", "A")
        .with("books/Series/b.txt", "B")
        .with("books/Series/cover.jpg", "picture");
    let (mut kernel, moved) = manager(storage, "books");
    rename(&mut kernel, "books/Series", "Saga");
    let mut moved = moved.borrow().clone();
    moved.sort();
    assert_eq!(
        moved,
        [
            ("books/Series/a.txt".into(), "books/Saga/a.txt".into()),
            ("books/Series/b.txt".into(), "books/Saga/b.txt".into()),
        ]
    );
}

/// The library folder renamed: the settings naming it, or a folder in it,
/// follow.
#[test]
fn folder_settings_follow_a_rename() {
    let storage = MemoryStorage::new().with("books/a.txt", "A");
    let (mut kernel, _) = manager(storage, "");
    rename(&mut kernel, "books", "Library");
    assert_eq!(kernel.settings().text("core.library-folder"), "Library");
    assert_eq!(kernel.settings().text("files.archive"), "Library/Archive");
}

/// The reading app chosen for a folder follows it renamed, also for the
/// folders in it.
#[test]
fn reading_apps_of_folders_follow_a_rename() {
    use paperpaw_sdk::readers::{Choice, remember, remembered};
    let storage = MemoryStorage::new().with("books/Comics/Old/a.txt", "A");
    let app = Id::new_static("comics.app");
    remember(&storage, Choice::Folder("books/Comics"), Some(&app)).unwrap();
    remember(&storage, Choice::Folder("books/Comics/Old"), Some(&app)).unwrap();
    let (mut kernel, _) = manager(storage, "books");
    rename(&mut kernel, "books/Comics", "Manga");
    for folder in ["books/Manga", "books/Manga/Old"] {
        let kept = remembered(kernel.storage(), Choice::Folder(folder));
        assert_eq!(kept.as_deref(), Some("comics.app"), "{folder}");
    }
    assert_eq!(
        remembered(kernel.storage(), Choice::Folder("books/Comics")),
        None
    );
}

/// A file picked to be moved goes with its folder when that is deleted:
/// nothing is left to paste.
#[test]
fn deleting_a_folder_drops_what_was_picked_in_it() {
    let storage = MemoryStorage::new()
        .with("books/Old/a.txt", "A")
        .with("books/b.txt", "B");
    let (mut kernel, _) = manager(storage, "books");
    choose(&mut kernel, "books/Old/a.txt", "move");
    assert!(
        shown(&mut kernel).iter().any(|l| l == "Paste here"),
        "{:?}",
        shown(&mut kernel)
    );
    delete(&mut kernel, "books/Old");
    let after = shown(&mut kernel);
    assert!(!after.iter().any(|l| l == "Paste here"), "{after:?}");
}

/// A folder of more books than can be told moving at once is not moved:
/// the data kept by their paths would stay behind.
#[test]
fn a_folder_of_too_many_books_is_not_moved() {
    let storage = (0..=2000).fold(MemoryStorage::new(), |card, i| {
        card.with(&format!("books/All/{i:04}.txt"), "x")
    });
    let (mut kernel, moved) = manager(storage, "books");
    rename(&mut kernel, "books/All", "Every");
    assert!(kernel.storage().exists("books/All/0000.txt"));
    assert!(!kernel.storage().exists("books/Every/0000.txt"));
    assert!(moved.borrow().is_empty());
    let said = shown(&mut kernel);
    assert!(
        said.iter().any(|l| l.contains("more than 2000 books")),
        "{said:?}"
    );
}

/// A book renamed to a name no format opens is told moving all the same:
/// renamed back, its notes come back with it.
#[test]
fn a_file_renamed_is_told_moving_whatever_its_name() {
    let storage = MemoryStorage::new().with("books/a.txt", "A");
    let (mut kernel, moved) = manager(storage, "books");
    rename(&mut kernel, "books/a.txt", "a.txt.bak");
    assert_eq!(
        *moved.borrow(),
        [("books/a.txt".into(), "books/a.txt.bak".into())]
    );
}

/// A storage that moves between folders by copying, and deletes only the
/// first file of a folder moved (`books/Old`) before failing.
struct HalfMoving(MemoryStorage);

impl Storage for HalfMoving {
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
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if from.starts_with("books/Old") {
            return Err(StorageError::Unsupported);
        }
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        if path == "books/Old" {
            self.0.remove("books/Old/a.txt")?;
            return Err(StorageError::Io);
        }
        self.0.remove_dir_all(path)
    }
}

/// A move that fails half-way tells of the books it moved: those at the
/// new place and gone from the old one.
#[test]
fn a_move_cut_short_tells_of_the_books_it_moved() {
    let storage = HalfMoving(
        MemoryStorage::new()
            .with("books/Old/a.txt", "A")
            .with("books/Old/b.txt", "B")
            .with("New/keep.dat", "x"),
    );
    let (mut kernel, moved) = manager(storage, "New");
    choose(&mut kernel, "books/Old", "move");
    // Up from the first entry to the bar: *Cancel*, then *Paste here*.
    for action in ["core.up", "core.up", "core.confirm"] {
        let effects = kernel.run_action(Instant(0), Id::new_static(action));
        settle(&mut kernel, effects);
    }
    assert!(kernel.storage().exists("New/Old/a.txt"));
    assert!(kernel.storage().exists("books/Old/b.txt"));
    assert_eq!(
        *moved.borrow(),
        [("books/Old/a.txt".into(), "New/Old/a.txt".into())]
    );
}

/// Deleting says what goes: a folder with everything in it, a book with
/// its place and notes.
#[test]
fn deleting_says_what_goes() {
    let storage = MemoryStorage::new()
        .with("books/Old/a.txt", "A")
        .with("books/b.txt", "B");
    let (mut kernel, _) = manager(storage, "books");
    choose(&mut kernel, "books/Old", "delete");
    let said = shown(&mut kernel).join(" ");
    assert!(said.contains("everything in it"), "{said}");
    let effects = kernel.run_action(Instant(0), Id::new_static("core.back"));
    settle(&mut kernel, effects);
    choose(&mut kernel, "books/b.txt", "delete");
    let said = shown(&mut kernel).join(" ");
    assert!(said.contains("bookmarks and notes"), "{said}");
}

/// PaperPaw's own folder, shown with hidden files, is not deleted: it
/// holds every setting, note and statistic.
#[test]
fn paperpaws_own_folder_is_not_deleted() {
    let storage = MemoryStorage::new()
        .with(".paperpaw/annotations/0123456789abcdef", "a note")
        .with("books/b.txt", "B");
    let (mut kernel, _) = manager(storage, "");
    delete(&mut kernel, ".paperpaw");
    assert!(
        kernel
            .storage()
            .exists(".paperpaw/annotations/0123456789abcdef")
    );
    let said = shown(&mut kernel).join(" ");
    assert!(said.contains("PaperPaw"), "{said}");
}

/// The question whether to delete starts on the safe answer, as every yes
/// or no question does: confirm pressed at once deletes nothing.
#[test]
fn deleting_starts_on_the_safe_answer() {
    let storage = MemoryStorage::new().with("books/b.txt", "B");
    let (mut kernel, _) = manager(storage, "books");
    choose(&mut kernel, "books/b.txt", "delete");
    assert_eq!(focused(&mut kernel), "Cancel", "{:?}", shown(&mut kernel));
    let effects = kernel.run_action(Instant(0), Id::new_static("core.confirm"));
    settle(&mut kernel, effects);
    assert!(kernel.storage().exists("books/b.txt"));
}

/// Nothing in PaperPaw's own folder, however deep, nor in the one of its
/// old name (what was not taken over stays there) is deleted, renamed or
/// moved: each is refused with the reason.
#[test]
fn nothing_in_paperpaws_own_folders_is_deleted_renamed_or_moved() {
    let paths = [
        ".paperpaw/plugins/stats/x.txt",
        ".paperpaw/plugins",
        ".quireink",
        ".quireink/plugins/stats/x.txt",
    ];
    for path in paths {
        for how in ["ask", "delete", "rename", "move"] {
            let storage = MemoryStorage::new()
                .with(".paperpaw/plugins/stats/x.txt", "new")
                // Kept under the old name: the new one has it too.
                .with(".quireink/plugins/stats/x.txt", "old")
                .with("books/b.txt", "B");
            let (mut kernel, _) = manager(storage, "books");
            match how {
                "ask" => choose(&mut kernel, path, "delete"),
                "delete" => delete(&mut kernel, path),
                "rename" => rename(&mut kernel, path, "gone"),
                _ => {
                    choose(&mut kernel, path, "move");
                    press(&mut kernel, "Paste here");
                }
            }
            let storage = kernel.storage();
            let text = |path: &str| paperpaw_sdk::read_file(storage, path, 8).unwrap();
            assert_eq!(
                text(".paperpaw/plugins/stats/x.txt"),
                b"new",
                "{how} {path}"
            );
            assert_eq!(
                text(".quireink/plugins/stats/x.txt"),
                b"old",
                "{how} {path}"
            );
            let gone = ["books/x.txt", "books/plugins", "books/gone"];
            assert!(!gone.iter().any(|p| storage.exists(p)), "{how} {path}");
            let said = shown(&mut kernel).join(" ");
            assert!(said.contains("PaperPaw’s own"), "{how} {path}: {said}");
        }
    }
}
