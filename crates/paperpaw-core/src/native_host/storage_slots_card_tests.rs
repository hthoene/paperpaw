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
    let format = Some(Format {
        scope: &scope,
        owner: 1,
        features,
        files,
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
