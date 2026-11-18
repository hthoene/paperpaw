//! Root 2, the plugin's own install folder `plugins/<id>/`, on the storage
//! slots (`files.md` 1.1): `size` and `read_at` read it, every change is
//! `ERR_DENIED`, and a plugin built into the firmware has none.
extern crate std;
use super::tests::{Backend, bytes, output, scoped};
use super::*;
use crate::native_host::format_scope::FormatScope;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_NOT_FOUND};
use paperpaw_platform::Storage;

const OWNER: &str = "vocabulary";

fn card() -> Backend {
    let backend = Backend::default();
    backend
        .files
        .write("plugins/vocabulary/data/phone.txt", b"PHONE")
        .unwrap();
    backend
        .files
        .write("plugins/other/data/x.txt", b"X")
        .unwrap();
    backend
        .files
        .write(".paperpaw/plugins/vocabulary/mine.txt", b"M")
        .unwrap();
    backend
}

/// `call` runs in a callback of [`OWNER`]; `built_in`: it is linked.
fn with(
    backend: &Backend,
    built_in: bool,
    call: impl FnOnce(&EnvironmentScope, Option<Format<'_>>),
) {
    let scope = FormatScope::new();
    let linked = LinkedPlugins::default();
    if built_in {
        linked.add(OWNER).unwrap();
    }
    let format = Some(Format {
        scope: &scope,
        owner: 1,
        features: AdmittedFeatures::default(),
        files: None,
        linked: &linked,
    });
    scoped(backend, |env, kernel| {
        let _running = kernel.shared.meter.enter(OWNER);
        call(env, format);
    });
}

/// Slot 20 and 21 on root 2 `path`: the size and what a read gives.
fn read(
    env: &EnvironmentScope,
    format: Option<Format<'_>>,
    path: &str,
) -> Result<(u64, std::vec::Vec<u8>), i32> {
    let (mut len, mut out) = (0_u64, [0_u8; 8]);
    let path = bytes(path.as_bytes());
    // SAFETY: live input, aligned and exclusive outputs.
    let (sized, count) = unsafe {
        (
            size(env, OWNER, format, 2, path, &mut len),
            read_at(env, OWNER, format, 2, path, 0, output(&mut out)),
        )
    };
    if sized != OK {
        return Err(sized);
    }
    let count = usize::try_from(count).map_err(|_| count)?;
    Ok((len, out[..count].to_vec()))
}

#[test]
fn root_two_reads_the_plugins_own_install_folder_only() {
    with(&card(), false, |env, format| {
        assert_eq!(
            read(env, format, "data/phone.txt"),
            Ok((5, b"PHONE".to_vec()))
        );
        assert_eq!(read(env, format, "data/none.txt"), Err(ERR_NOT_FOUND));
        // Not another plugin's folder, nor its own data folder.
        assert_eq!(read(env, format, "../other/data/x.txt"), Err(ERR_INVALID));
        assert_eq!(read(env, format, "mine.txt"), Err(ERR_NOT_FOUND));
        assert_eq!(read(env, format, ""), Err(ERR_INVALID));
    });
}

#[test]
fn root_two_is_never_changed() {
    let backend = card();
    with(&backend, false, |env, format| {
        let none = PathsInUse::default();
        for path in ["data/phone.txt", "data/new.txt", "../x"] {
            let path = bytes(path.as_bytes());
            // SAFETY: live inputs.
            unsafe {
                assert_eq!(
                    write(env, OWNER, format, &none, 2, path, bytes(b"w")),
                    ERR_DENIED
                );
                assert_eq!(
                    append(env, OWNER, format, &none, 2, path, bytes(b"a")),
                    ERR_DENIED
                );
                assert_eq!(
                    rename(env, OWNER, format, &none, 2, path, bytes(b"data/b.txt")),
                    ERR_DENIED
                );
                assert_eq!(remove(env, OWNER, format, &none, 2, path), ERR_DENIED);
            }
        }
    });
    let mut out = [0_u8; 8];
    let shipped = "plugins/vocabulary/data/phone.txt";
    assert_eq!(backend.files.read_at(shipped, 0, &mut out), Ok(5));
    assert_eq!(&out[..5], b"PHONE");
    assert!(!backend.files.exists("plugins/vocabulary/data/new.txt"));
    assert!(!backend.files.exists("plugins/vocabulary/data/b.txt"));
}

#[test]
fn a_built_in_plugin_has_an_empty_install_folder() {
    // What the card holds at `plugins/<id>/` is a leftover of an older install.
    with(&card(), true, |env, format| {
        assert_eq!(read(env, format, "data/phone.txt"), Err(ERR_NOT_FOUND));
        assert_eq!(read(env, format, "../other/data/x.txt"), Err(ERR_INVALID));
    });
}
