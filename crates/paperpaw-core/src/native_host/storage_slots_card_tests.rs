//! Root 1, the user's files from the card's top, on the storage slots 20-26
//! (`files.md` 1.1, decision 125): open with `uses.storage`, `ERR_DENIED`
//! without it, `ERR_UNSUPPORTED` where the host offers no card files, never
//! the kernel's or the plugins' files, and busy where `core.files` is.
extern crate std;
use super::tests::{Backend, bytes, output, scoped};
use super::*;
use crate::native_host::{
    files_service::{Access, FilesHost},
    format_scope::FormatScope,
};
use paperpaw_native::{
    host_glue::{ERR_BUSY, ERR_DENIED, ERR_INVALID},
    metadata::Feature,
};
use paperpaw_platform::Storage;

const OWNER: &str = "vocabulary";
const HOST: &[Feature<'static>] = &[Feature {
    key: "uses.storage",
    revision: 1,
}];

/// The plugin's features: `uses.storage` asked for (`asked`), and offered
/// by the host (`offered`).
fn features(asked: bool, offered: bool) -> AdmittedFeatures {
    AdmittedFeatures::test(u32::from(asked), if offered { HOST } else { &[] })
}

fn card() -> Backend {
    let backend = Backend::default();
    backend.files.write("words/cards.txt", b"CARDS").unwrap();
    backend.files.write("paperpaw.ini", b"[firmware]").unwrap();
    backend.files.write(".paperpaw/settings", b"S").unwrap();
    backend.files.write("plugins/sleep/a.png", b"PNG").unwrap();
    // The FAT name of a long one: on a card it may be `.paperpaw`.
    backend.files.write("PAPERP~1/settings", b"S").unwrap();
    backend
}

/// `call` runs in a callback of [`OWNER`] with `features` and `files`.
fn with(
    backend: &Backend,
    features: AdmittedFeatures,
    files: Option<&dyn FilesHost>,
    call: impl FnOnce(&EnvironmentScope, Option<Format<'_>>),
) {
    let scope = FormatScope::new();
    let linked = LinkedPlugins::default();
    let format = Some(Format {
        scope: &scope,
        owner: 1,
        features,
        files,
        linked: &linked,
    });
    scoped(backend, |env, kernel| {
        let _running = kernel.shared.meter.enter(OWNER);
        call(env, format);
    });
}

/// The status of each of slots 20-25 on root 1 `path` (`to` for a rename),
/// in slot order, with `in_use` the host writers' paths.
fn statuses(
    env: &EnvironmentScope,
    format: Option<Format<'_>>,
    in_use: &PathsInUse,
    path: &str,
    to: &str,
) -> [i32; 6] {
    let path = bytes(path.as_bytes());
    let (mut len, mut out) = (0_u64, [0_u8; 8]);
    // SAFETY: live inputs, aligned and exclusive outputs.
    unsafe {
        [
            size(env, OWNER, format, 1, path, &mut len),
            read_at(env, OWNER, format, 1, path, 0, output(&mut out)).min(OK),
            write(env, OWNER, format, in_use, 1, path, bytes(b"new")),
            append(env, OWNER, format, in_use, 1, path, bytes(b"+")),
            rename(env, OWNER, format, in_use, 1, path, bytes(to.as_bytes())),
            remove(env, OWNER, format, in_use, 1, path),
        ]
    }
}

#[test]
fn root_one_serves_the_users_files_with_uses_storage() {
    let backend = card();
    with(&backend, features(true, true), None, |env, format| {
        let none = PathsInUse::default();
        let (mut len, mut out) = (0_u64, [0_u8; 8]);
        // SAFETY: live inputs, aligned and exclusive outputs.
        unsafe {
            let path = bytes(b"words/cards.txt");
            assert_eq!(size(env, OWNER, format, 1, path, &mut len), OK);
            assert_eq!(len, 5);
            assert_eq!(read_at(env, OWNER, format, 1, path, 1, output(&mut out)), 4);
            assert_eq!(&out[..4], b"ARDS");
            let new = bytes(b"words/new.txt");
            assert_eq!(write(env, OWNER, format, &none, 1, new, bytes(b"ab")), OK);
            assert_eq!(append(env, OWNER, format, &none, 1, new, bytes(b"c")), OK);
            let moved = bytes(b"old/moved.txt");
            assert_eq!(rename(env, OWNER, format, &none, 1, new, moved), OK);
            assert_eq!(remove(env, OWNER, format, &none, 1, path), OK);
            assert_eq!(sync(env, OWNER), OK);
        }
    });
    // The card's own paths, not the plugin's data folder.
    let mut out = [0_u8; 8];
    assert_eq!(backend.files.read_at("old/moved.txt", 0, &mut out), Ok(3));
    assert_eq!(&out[..3], b"abc");
    assert!(!backend.files.exists("words/cards.txt"));
    assert!(
        !backend
            .files
            .exists(".paperpaw/plugins/vocabulary/old/moved.txt")
    );
}

#[test]
fn root_one_is_denied_without_uses_storage_and_unsupported_where_the_host_has_none() {
    for (features, expected) in [
        (features(false, true), ERR_DENIED),
        // A host without card files offers no `uses.storage`.
        (features(true, false), ERR_UNSUPPORTED),
        (features(false, false), ERR_UNSUPPORTED),
    ] {
        let backend = card();
        with(&backend, features, None, |env, format| {
            let none = PathsInUse::default();
            assert_eq!(
                statuses(env, format, &none, "words/cards.txt", "b.txt"),
                [expected; 6]
            );
            // Before the path: a bad one is refused for the root first.
            assert_eq!(statuses(env, format, &none, "../x", "b.txt"), [expected; 6]);
        });
        assert!(backend.files.exists("words/cards.txt"));
        assert!(!backend.files.exists("b.txt"));
    }
    // A slot call that tells nothing of the plugin has no root 1.
    let backend = card();
    with(&backend, features(true, true), None, |env, _| {
        let none = PathsInUse::default();
        assert_eq!(
            statuses(env, None, &none, "words/cards.txt", "b.txt"),
            [ERR_UNSUPPORTED; 6]
        );
    });
}

#[test]
fn root_one_never_reaches_the_kernels_or_the_plugins_files() {
    let backend = card();
    with(&backend, features(true, true), None, |env, format| {
        let none = PathsInUse::default();
        for path in [
            "paperpaw.ini",
            "PaperPaw.INI",
            ".paperpaw/settings",
            ".PAPERPAW/plugins/vocabulary/x",
            "plugins/sleep/a.png",
            "Plugins/x.ppn",
            "PAPERP~1/settings",
        ] {
            assert_eq!(
                statuses(env, format, &none, path, "words/b.txt"),
                [ERR_INVALID; 6],
                "{path}"
            );
            // Nor as the target of a rename.
            // SAFETY: live inputs.
            let moved = unsafe {
                rename(
                    env,
                    OWNER,
                    format,
                    &none,
                    1,
                    bytes(b"words/cards.txt"),
                    bytes(path.as_bytes()),
                )
            };
            assert_eq!(moved, ERR_INVALID, "{path}");
        }
        // The path rules of every root.
        for path in ["", "/words", "words/../paperpaw.ini", "a//b"] {
            assert_eq!(
                statuses(env, format, &none, path, "x"),
                [ERR_INVALID; 6],
                "{path}"
            );
        }
    });
    let mut out = [0_u8; 16];
    assert_eq!(backend.files.read_at("paperpaw.ini", 0, &mut out), Ok(10));
    assert_eq!(&out[..10], b"[firmware]");
    assert!(backend.files.exists("words/cards.txt"));
    assert!(backend.files.exists("plugins/sleep/a.png"));
    assert!(backend.files.exists("PAPERP~1/settings"));
}

/// `core.files`'s host: `held` is busy for every change.
struct Holds(&'static str);

impl FilesHost for Holds {
    fn excluded(&self, _: u32, _: &str, _: &str) -> bool {
        false
    }
    fn built_in(&self, _: &str) -> bool {
        false
    }
    fn busy(&self, path: &str, access: Access) -> bool {
        // Reading is never asked about (`files.md` 3).
        assert!(matches!(
            access,
            Access::Write | Access::Remove | Access::Rename
        ));
        path == self.0
    }
}

#[test]
fn root_one_changes_wait_for_what_the_host_holds_and_reads_never_do() {
    let backend = card();
    let open = Holds("words/cards.txt");
    with(
        &backend,
        features(true, true),
        Some(&open),
        |env, format| {
            let none = PathsInUse::default();
            let busy = [OK, OK, ERR_BUSY, ERR_BUSY, ERR_BUSY, ERR_BUSY];
            assert_eq!(
                statuses(env, format, &none, "words/cards.txt", "b.txt"),
                busy
            );
            // Neither end of a rename may be held.
            // SAFETY: live inputs.
            unsafe {
                backend.files.write("other.txt", b"o").unwrap();
                let onto = rename(
                    env,
                    OWNER,
                    format,
                    &none,
                    1,
                    bytes(b"other.txt"),
                    bytes(b"words/cards.txt"),
                );
                assert_eq!(onto, ERR_BUSY);
            }
            // A host writer's path (a download's file) answers busy too.
            let saving = PathsInUse::default();
            saving.claim(9, "news/feed.xml").unwrap();
            backend.files.write("news/feed.xml", b"x").unwrap();
            assert_eq!(
                statuses(env, format, &saving, "news/feed.xml", "b.txt"),
                busy
            );
        },
    );
    let mut out = [0_u8; 8];
    assert_eq!(backend.files.read_at("words/cards.txt", 0, &mut out), Ok(5));
    assert!(backend.files.exists("other.txt") && backend.files.exists("news/feed.xml"));
}

/// One card in memory, shared by the kernel, the slots and the library's move
/// job (as on a device), with the entry identities a move journals.
#[derive(Clone)]
struct Shared(alloc::rc::Rc<paperpaw_sdk::MemoryStorage>);

impl Storage for Shared {
    fn read_at(
        &self,
        p: &str,
        o: u64,
        b: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.0.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.0.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(p, d)
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.0.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.0.create_dir(p)
    }
    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }
    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.0.same_entry(a, b)
    }
    fn entry_identity(&self, p: &str) -> Result<Option<u64>, paperpaw_platform::StorageError> {
        self.0.entry_identity(p)
    }
}

#[test]
fn a_root_one_rename_is_the_librarys_move_and_its_books_keep_their_place() {
    use crate::native_host::{
        host_services::HostServices, lib_moves_service::Pump, storage_files::StorageFiles,
    };
    use alloc::{borrow::Cow, rc::Rc};
    use paperpaw_sdk::{
        FormatSpec, Id, MemoryStorage,
        document::{DocumentError, Locator},
        positions::{self, Position},
    };
    let card = Rc::new(MemoryStorage::new());
    card.write("Books/old/a.epub", b"book").unwrap();
    let place = Position::new(Locator::new(7, 1234), 41.5);
    positions::save(&*card, "Books/old/a.epub", &place).unwrap();
    let storage: Rc<dyn Storage> = Rc::new(Shared(card.clone()));
    let hosts = HostServices::default();
    let moves = hosts.wire_library_moves(storage.clone()).unwrap();
    let files = StorageFiles::new(storage, hosts.busy_sources().clone(), Rc::default())
        .with_moves(moves.clone());
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(Shared(card.clone()))
        .build()
        .unwrap();
    kernel
        .shared
        .formats
        .add(FormatSpec::new(
            Id::new_static("test.epub"),
            "EPUB",
            alloc::vec![Cow::Borrowed("epub")],
            alloc::boxed::Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let scope = core::pin::pin!(EnvironmentScope::new());
    let linked = LinkedPlugins::default();
    let formats = FormatScope::new();
    let format = Some(Format {
        scope: &formats,
        owner: 1,
        features: features(true, true),
        files: Some(&files),
        linked: &linked,
    });
    let none = PathsInUse::default();
    scope
        .as_ref()
        .scope(&env, || {
            let _running = kernel.shared.meter.enter(OWNER);
            // SAFETY: live inputs.
            let renamed = unsafe {
                rename(
                    &scope,
                    OWNER,
                    format,
                    &none,
                    1,
                    bytes(b"Books/old"),
                    bytes(b"Books/new"),
                )
            };
            assert_eq!(renamed, OK);
            assert!(moves.moves().active(), "the library's move job");
            // One move at a time: another rename waits for it.
            card.write("notes.txt", b"n").unwrap();
            // SAFETY: live inputs.
            let second = unsafe {
                rename(
                    &scope,
                    OWNER,
                    format,
                    &none,
                    1,
                    bytes(b"notes.txt"),
                    bytes(b"notes2.txt"),
                )
            };
            assert_eq!(second, ERR_BUSY);
        })
        .unwrap();
    for _ in 0..100 {
        if moves.pump(&env, &mut |_| ()) == Pump::Idle {
            break;
        }
    }
    assert!(!moves.moves().active(), "carried");
    assert!(card.exists("Books/new/a.epub"));
    let kept = positions::load(&*card, "Books/new/a.epub").expect("the place followed");
    assert_eq!((kept.at.section, kept.at.offset), (7, 1234));
}

/// A callback's frame that has the kernel's `Env`, as core's own frame.
struct KernelFrame<'a>(&'a paperpaw_sdk::Env<'a>);

impl crate::abi7::context::Frame for KernelFrame<'_> {
    fn settings(&self) -> &paperpaw_sdk::Settings {
        self.0.settings()
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        self.0.i18n()
    }
    fn storage(&self) -> Option<&dyn Storage> {
        Some(self.0.storage())
    }
    fn kernel_env(&self) -> Option<&paperpaw_sdk::Env<'_>> {
        Some(self.0)
    }
}

/// #612: a flash card save is two renames in one callback (the copy kept, then
/// the new file). The first one's move job, stepped only after the callback,
/// made the second `ERR_BUSY`: the list "could not be written".
#[test]
fn a_file_that_is_no_book_is_renamed_without_the_librarys_job() {
    use crate::native_host::{host_services::HostServices, storage_files::StorageFiles};
    use alloc::{borrow::Cow, rc::Rc};
    use paperpaw_native::host_glue::ERR_STATE;
    use paperpaw_sdk::{FormatSpec, Id, MemoryStorage, document::DocumentError};
    let card = Rc::new(MemoryStorage::new());
    for (path, data) in [
        ("Flashcards/Spanish.csv", "old"),
        ("Flashcards/_Spanish.bak.csv.part", "old"),
        ("Flashcards/Spanish.csv.part", "new"),
        ("Books/a.epub", "book"),
    ] {
        card.write(path, data.as_bytes()).unwrap();
    }
    let storage: Rc<dyn Storage> = Rc::new(Shared(card.clone()));
    let hosts = HostServices::default();
    let moves = hosts.wire_library_moves(storage.clone()).unwrap();
    let files = StorageFiles::new(storage, hosts.busy_sources().clone(), Rc::default())
        .with_moves(moves.clone());
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(Shared(card.clone()))
        .build()
        .unwrap();
    kernel
        .shared
        .formats
        .add(FormatSpec::new(
            Id::new_static("test.epub"),
            "EPUB",
            alloc::vec![Cow::Borrowed("epub")],
            alloc::boxed::Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let scope = core::pin::pin!(EnvironmentScope::new());
    let linked = LinkedPlugins::default();
    let formats = FormatScope::new();
    let format = Some(Format {
        scope: &formats,
        owner: 1,
        features: features(true, true),
        files: Some(&files),
        linked: &linked,
    });
    let none = PathsInUse::default();
    let frame = KernelFrame(&env);
    scope
        .as_ref()
        .scope_frame(&frame, || {
            let _running = kernel.shared.meter.enter(OWNER);
            let mv = |from: &str, to: &str| {
                // SAFETY: live inputs.
                unsafe {
                    rename(
                        &scope,
                        OWNER,
                        format,
                        &none,
                        1,
                        bytes(from.as_bytes()),
                        bytes(to.as_bytes()),
                    )
                }
            };
            for (from, to) in [
                (
                    "Flashcards/_Spanish.bak.csv.part",
                    "Flashcards/_Spanish.bak.csv",
                ),
                ("Flashcards/Spanish.csv.part", "Flashcards/Spanish.csv"),
            ] {
                assert_eq!(mv(from, to), OK, "{from}");
                assert!(!moves.moves().active(), "no job after {from}");
                assert!(!card.exists(from) && card.exists(to), "{from}");
            }
            assert_eq!(card.get("Flashcards/Spanish.csv").unwrap(), b"new");
            assert_eq!(card.get("Flashcards/_Spanish.bak.csv").unwrap(), b"old");
            // Refused as the job refuses: onto a folder, below itself.
            assert_eq!(mv("Flashcards/Spanish.csv", "Books"), ERR_STATE);
            assert_eq!(
                mv("Flashcards/Spanish.csv", "Flashcards/Spanish.csv/x"),
                ERR_INVALID
            );
            assert!(card.exists("Flashcards/Spanish.csv"));
            // A book is the library's move as before, under either name.
            assert_eq!(mv("Books/a.epub", "Books/a.old"), OK);
            assert!(moves.moves().active(), "the library's move job");
            // One move at a time: while it runs, any other rename waits.
            assert_eq!(
                mv("Flashcards/Spanish.csv", "Flashcards/Spanish2.csv"),
                ERR_BUSY
            );
        })
        .unwrap();
}
