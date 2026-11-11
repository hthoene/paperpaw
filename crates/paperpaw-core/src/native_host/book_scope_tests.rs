//! A native plugin's book scope (#257): the host picks Beta 3's file for the
//! book, only the submitter's own top screen asks, a path that names no book
//! is refused, the scoped settings take the book's values over the global
//! ones until the screen goes, and every plugin's file follows its book.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native_host::commands::{
    CommandScope, CommandStage, WireCommand as Wire, commands_encode,
};
use crate::{Command, Id};
use alloc::{vec, vec::Vec};
use paperpaw_native::host_glue::{ERR_STATE, OK};
use paperpaw_platform::{DirEntry, Instant, Storage, StorageError};
use paperpaw_sdk::{MemoryStorage, Params, SettingValue};

const BOOK: &str = "books/a.epub";

/// The folder Beta 3's reader kept a book's data in (`book_dir`: FNV-1a 64 of
/// the path, 16 hex digits), worked out here on its own.
fn beta3_dir(plugin: &str, path: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in path.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    alloc::format!(
        "{}/books/{hash:016x}",
        paperpaw_sdk::Env::plugin_dir(plugin)
    )
}

#[test]
fn the_file_is_where_beta_3_kept_a_books_own_settings() {
    let file = file("reader", BOOK).unwrap();
    assert_eq!(
        file,
        alloc::format!("{}/settings.ini", beta3_dir("reader", BOOK))
    );
    // Each plugin has its own; each book its own.
    assert_ne!(file, super::file("comics", BOOK).unwrap());
    assert_ne!(file, super::file("reader", "books/b.epub").unwrap());
}

#[test]
fn a_path_that_names_no_book_is_refused() {
    for path in [
        "",
        "/books/a.epub",
        ".paperpaw/plugins/reader/books/x",
        "plugins/reader/main.ppn",
        "paperpaw.ini",
        "books/a\nb.epub",
    ] {
        assert_eq!(super::file("reader", path), Err(ERR_INVALID), "{path:?}");
    }
    let long = "b".repeat(513);
    assert!(!book_path_ok(&long));
    assert!(book_path_ok(&long[..512]));
}

#[test]
fn a_book_key_is_sixteen_hex_digits() {
    assert!(could_be_key("0123456789abcdef"));
    for name in [
        "0123456789abcde",
        "0123456789abcdef0",
        "0123456789abcdeg",
        "notes",
    ] {
        assert!(!could_be_key(name), "{name}");
    }
}

/// A kernel with the screens of the plugin `probe`, its `probe.main` on top.
fn kernel() -> crate::Kernel {
    use paperpaw_sdk::{Manifest, Plugin, Registrar, Screen, ScreenSpec, Version};
    struct Empty;
    impl Screen for Empty {
        fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
        fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
            paperpaw_sdk::Handled::No
        }
    }
    struct Probe;
    impl Plugin for Probe {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static("probe"), "Probe", Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING)
        }
        fn register(&self, r: &mut Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
            for id in ["probe.home", "probe.main"] {
                r.add_screen(ScreenSpec::new(
                    Id::new_static(id),
                    "probe.title",
                    alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Empty)),
                ))?;
            }
            Ok(())
        }
    }
    let beta3 = alloc::format!("{}/settings.ini", beta3_dir("probe", BOOK));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(
            MemoryStorage::new()
                .with(
                    &beta3,
                    b"[core]\nscope-only = on\nfont-size = 14\n".to_vec(),
                )
                .with(BOOK, b"x".to_vec()),
        )
        .plugin(Probe)
        .build()
        .unwrap();
    // `probe.main` over another screen of the plugin, so it can close.
    for id in ["probe.home", "probe.main"] {
        kernel.apply(Command::OpenScreen(Id::new_static(id), Params::new()));
    }
    kernel
}

/// `commands` submitted by `owner` with `top` as the screen it draws.
fn submit(
    kernel: &crate::Kernel,
    owner: &str,
    top: bool,
    commands: &[Wire<'_>],
) -> (i32, Vec<Command>) {
    let env = kernel.shared.env(Instant(0));
    let owner = Id::try_parse(owner).unwrap();
    let screen = top.then(|| kernel.shared.stack.last().unwrap().key);
    let mut stage =
        CommandStage::new(&owner, env.settings(), None, 8, 4096).with_ui(&env, screen, 0, 8);
    let scope = core::pin::pin!(CommandScope::new());
    let mut bytes = [0; 1024];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let status = scope
        .as_ref()
        .scope(&mut stage, || {
            // SAFETY: the encoded input stays readable and apart from the
            // stage until the synchronous call ends.
            unsafe { scope.submit(bytes.as_ptr(), len) }
        })
        .unwrap();
    (status, stage.into_commands())
}

#[test]
fn the_top_screens_plugin_names_the_book_and_the_host_the_file() {
    let kernel = kernel();
    let (status, staged) = submit(&kernel, "probe", true, &[Wire::SetScope { path: BOOK }]);
    assert_eq!(status, OK);
    assert_eq!(
        staged,
        [Command::SetScope(Some(file("probe", BOOK).unwrap()))]
    );
    // Empty ends the scope.
    let (status, staged) = submit(&kernel, "probe", true, &[Wire::SetScope { path: "" }]);
    assert_eq!((status, staged), (OK, vec![Command::SetScope(None)]));
    // A path that names no book: the batch is refused, nothing staged.
    let (status, staged) = submit(
        &kernel,
        "probe",
        true,
        &[Wire::SetScope {
            path: "paperpaw.ini",
        }],
    );
    assert_eq!((status, staged.len()), (ERR_INVALID, 0));
    // Not from the plugin's own top screen.
    for (owner, top) in [("probe", false), ("other", true)] {
        let (status, staged) = submit(&kernel, owner, top, &[Wire::SetScope { path: BOOK }]);
        assert_eq!((status, staged.len()), (ERR_STATE, 0), "{owner} {top}");
    }
}

#[test]
fn a_books_own_value_is_read_until_the_scope_ends_then_the_global_one() {
    let mut kernel = kernel();
    let global = kernel.shared.settings.get("core.font-size").cloned();
    assert_ne!(global, Some(SettingValue::Int(14)));
    let (_, staged) = submit(&kernel, "probe", true, &[Wire::SetScope { path: BOOK }]);
    for command in staged {
        kernel.apply(command);
    }
    // Beta 3's value for the book.
    assert_eq!(
        kernel.shared.settings.get("core.font-size"),
        Some(&SettingValue::Int(14))
    );
    let (_, staged) = submit(&kernel, "probe", true, &[Wire::SetScope { path: "" }]);
    for command in staged {
        kernel.apply(command);
    }
    assert_eq!(
        kernel.shared.settings.get("core.font-size").cloned(),
        global
    );
}

#[test]
fn the_scope_ends_when_its_screen_goes() {
    // A plugin that stops or is parked has its screens closed: the same.
    let mut kernel = kernel();
    let global = kernel.shared.settings.get("core.font-size").cloned();
    let (_, staged) = submit(&kernel, "probe", true, &[Wire::SetScope { path: BOOK }]);
    for command in staged {
        kernel.apply(command);
    }
    assert_eq!(
        kernel.shared.settings.get("core.font-size"),
        Some(&SettingValue::Int(14))
    );
    let top = kernel.shared.stack.last().unwrap().key;
    kernel.apply(Command::Close(top));
    kernel.check_scope_owner();
    assert!(kernel.scope.is_none());
    assert_eq!(
        kernel.shared.settings.get("core.font-size").cloned(),
        global
    );
}

const KEPT: &[u8] = b"[core]\nscope-only = on\nfont-size = 14\n";

#[test]
fn a_move_carries_every_plugins_file_and_a_removal_removes_it() {
    use crate::native_host::book_data_steps::{Progress, move_data, remove_data_within};
    let storage = MemoryStorage::new();
    let (old, new) = (
        file("reader", BOOK).unwrap(),
        file("reader", "books/b.epub").unwrap(),
    );
    let other = file("comics", BOOK).unwrap();
    storage.write(&old, KEPT).unwrap();
    storage.write(&other, KEPT).unwrap();
    // A plugin with a folder but no file for the book: nothing to do.
    storage.create_dir(".paperpaw/plugins/clock").unwrap();
    // Found on the card, with no `book_data` declared at all.
    move_data(&storage, &[], BOOK, "books/b.epub").unwrap();
    let read = |path: &str| paperpaw_sdk::read_file(&storage, path, 64);
    assert!(read(&old).is_err(), "gone from the old key");
    assert_eq!(read(&new).unwrap(), KEPT);
    assert_eq!(
        read(&file("comics", "books/b.epub").unwrap()).unwrap(),
        KEPT
    );
    // The old folders went with their only file.
    assert!(!paperpaw_sdk::files::is_dir(
        &storage,
        &old[..old.len() - 13]
    ));
    // Again (a repeat after a cut): nothing more, no error.
    move_data(&storage, &[], BOOK, "books/b.epub").unwrap();
    let mut budget = 8;
    let done = remove_data_within(&storage, &[], "books/b.epub", &mut budget).unwrap();
    assert_eq!((done, budget), (Progress::Done, 6));
    assert!(read(&new).is_err(), "removed with the book");
    // Nothing kept, or no plugins folder at all: no error.
    assert_eq!(
        remove_data_within(&storage, &[], "books/c.epub", &mut budget),
        Ok(Progress::Done)
    );
    assert_eq!(
        remove_data_within(&MemoryStorage::new(), &[], BOOK, &mut budget),
        Ok(Progress::Done)
    );
    move_data(&MemoryStorage::new(), &[], BOOK, "books/b.epub").unwrap();
}

#[test]
fn a_move_never_replaces_and_beta_3s_other_data_keeps_its_folder() {
    use crate::native_host::book_data_steps::{Progress, move_data, remove_data_within};
    let storage = MemoryStorage::new();
    let (old, new) = (
        file("reader", BOOK).unwrap(),
        file("reader", "books/b.epub").unwrap(),
    );
    storage.write(&old, KEPT).unwrap();
    storage.write(&new, b"[core]\n").unwrap();
    move_data(&storage, &[], BOOK, "books/b.epub").unwrap();
    let read = |path: &str| paperpaw_sdk::read_file(&storage, path, 64);
    assert_eq!(read(&old).unwrap(), KEPT, "stays under the old key");
    assert_eq!(read(&new).unwrap(), b"[core]\n");
    // Beta 3 kept more in the book's folder: the folder stays for it.
    let notes = alloc::format!("{}/notes.txt", &old[..old.len() - 13]);
    storage.write(&notes, b"n").unwrap();
    let mut budget = 1;
    assert_eq!(
        remove_data_within(&storage, &[], BOOK, &mut budget),
        Ok(Progress::Done)
    );
    assert!(read(&old).is_err());
    assert_eq!(read(&notes).unwrap(), b"n");
    // A budget spent first: more to do, nothing removed.
    let mut budget = 0;
    assert_eq!(
        remove_data_within(&storage, &[], "books/b.epub", &mut budget),
        Ok(Progress::More)
    );
    assert!(read(&new).is_ok());
}

/// A card as FAT is: a rename into a folder that is not there is refused
/// (`NotFound`), and `size` of a path holding `flaky` fails for now (`Io`).
struct Fat<'a> {
    card: &'a MemoryStorage,
    flaky: &'static str,
}

impl Storage for Fat<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        if !self.flaky.is_empty() && p.contains(self.flaky) {
            return Err(StorageError::Io);
        }
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        let parent = t.rsplit_once('/').map_or("", |(dir, _)| dir);
        if !paperpaw_sdk::files::is_dir(self.card, parent) {
            return Err(StorageError::NotFound);
        }
        self.card.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.card.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.card.remove_dir_all(p)
    }
}

#[test]
fn a_move_makes_the_new_keys_folder_first_and_a_card_error_is_no_none() {
    use crate::native_host::book_data_steps::{move_data, remove_data_within};
    let card = MemoryStorage::new();
    let (old, new) = (
        file("reader", BOOK).unwrap(),
        file("reader", "books/b.epub").unwrap(),
    );
    card.write(&old, KEPT).unwrap();
    // A card error asking whether the file is there: a status, the file
    // where it was, and a repeat moves it.
    let flaky = Fat {
        card: &card,
        flaky: "settings.ini",
    };
    assert_eq!(
        move_data(&flaky, &[], BOOK, "books/b.epub"),
        Err(super::status(StorageError::Io))
    );
    let mut budget = 4;
    assert!(remove_data_within(&flaky, &[], BOOK, &mut budget).is_err());
    assert_eq!(budget, 4);
    let read = |path: &str| paperpaw_sdk::read_file(&card, path, 64);
    assert_eq!(read(&old).unwrap(), KEPT);
    // A card that renames into no folder: the new key's is made first.
    let fat = Fat {
        card: &card,
        flaky: "",
    };
    move_data(&fat, &[], BOOK, "books/b.epub").unwrap();
    assert!(read(&old).is_err(), "gone from the old key");
    assert_eq!(read(&new).unwrap(), KEPT);
}

#[test]
fn a_move_never_goes_into_a_folders_or_a_files_place() {
    use crate::native_host::book_data_steps::move_data;
    let card = MemoryStorage::new();
    let fat = Fat {
        card: &card,
        flaky: "",
    };
    let old = file("reader", BOOK).unwrap();
    card.write(&old, KEPT).unwrap();
    let read = |path: &str| paperpaw_sdk::read_file(&card, path, 64);
    // A folder where the new file goes: the file stays.
    let new = file("reader", "books/b.epub").unwrap();
    card.create_dir(&new).unwrap();
    move_data(&fat, &[], BOOK, "books/b.epub").unwrap();
    assert_eq!(read(&old).unwrap(), KEPT);
    // A file where the new key's folder goes: the file stays.
    let new = file("reader", "books/c.epub").unwrap();
    card.write(&new[..new.len() - 13], b"x").unwrap();
    move_data(&fat, &[], BOOK, "books/c.epub").unwrap();
    assert_eq!(read(&old).unwrap(), KEPT);
    assert_eq!(read(&new[..new.len() - 13]).unwrap(), b"x");
}
