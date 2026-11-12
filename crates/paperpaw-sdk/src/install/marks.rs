//! What an installer keeps for a plugin once it is in place: its code
//! named as installed ([`native::remember_code`]), its author, where it
//! came from ([`origin`](super::origin)). Kept before the kernel installs
//! the plugin, each would be wrong where the install fails or the power
//! is cut first: the version before would be refused at the next start,
//! or lose its store. So the installer writes them into the folder it
//! hands over ([`write()`]), and the kernel keeps them once the folder is
//! the plugin's ([`settle`]): right after the install, or at the next
//! start where that was cut short. The file is hidden: neither the Wi-Fi
//! transfer nor the cable reach it.

use alloc::format;
use alloc::string::String;

use paperpaw_platform::Storage;

use crate::Version;
use crate::native;

/// The file in a plugin's folder that holds what is to be kept.
pub const FILE: &str = ".install-marks";

/// Where a plugin being installed came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum From {
    /// A store, at this version: it offers its updates.
    Store(Version),
    /// Any other way: its own, no store's.
    Local,
}

/// What is kept once the plugin is in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Marks {
    pub from: From,
    /// Its code, as installers name it (the SHA-256 its file's header
    /// holds), where the reader or a store let it run.
    pub code: Option<[u8; 32]>,
    /// The key of its code's author, to remember (a store's).
    pub author: Option<[u8; 32]>,
    /// The author's recovery key (a store's library names it), pinned
    /// with `author` where this is the plugin's first install, and never
    /// otherwise ([`keys::remember_first_install`](native::keys::remember_first_install)).
    pub recovery: Option<[u8; 32]>,
}

impl Marks {
    pub fn new(from: From) -> Self {
        Self {
            from,
            code: None,
            author: None,
            recovery: None,
        }
    }

    #[must_use]
    pub fn code(mut self, sha256: [u8; 32]) -> Self {
        self.code = Some(sha256);
        self
    }

    #[must_use]
    pub fn author(mut self, key: [u8; 32]) -> Self {
        self.author = Some(key);
        self
    }

    #[must_use]
    pub fn recovery(mut self, key: [u8; 32]) -> Self {
        self.recovery = Some(key);
        self
    }
}

/// Writes `marks` into `folder`, the plugin about to be installed; an
/// error if they could not be written (then do not install it).
pub fn write(storage: &dyn Storage, folder: &str, marks: &Marks) -> Result<(), crate::data::Error> {
    let hex = paperpaw_digest::hex::encode;
    let mut text = match marks.from {
        From::Store(version) => format!("from store {version}\n"),
        From::Local => String::from("from local\n"),
    };
    if let Some(code) = marks.code {
        text.push_str(&format!("code {}\n", hex(&code)));
    }
    if let Some(recovery) = marks.recovery {
        text.push_str(&format!("recovery {}\n", hex(&recovery)));
    }
    if let Some(author) = marks.author {
        text.push_str(&format!("author {}\n", hex(&author)));
    }
    let path = format!("{}/{FILE}", folder.trim_end_matches('/'));
    Ok(storage.write(&path, text.as_bytes())?)
}

/// Keeps what an installer wrote into the folder of plugin `id`, now in
/// place ([`write()`]), and removes the file; `true` if there was any. The
/// code is named only where the file in the folder is that code.
pub fn settle(storage: &dyn Storage, id: &str, folder: &str) -> bool {
    let path = format!("{}/{FILE}", folder.trim_end_matches('/'));
    let Ok(bytes) = crate::read_file(storage, &path, 1024) else {
        return false;
    };
    let text = String::from_utf8_lossy(&bytes);
    let bytes32 =
        |hex: &str| -> Option<[u8; 32]> { paperpaw_digest::hex::decode_array(hex.trim()) };
    // Pinned with the author's key, where that is the first one.
    let recovery = text
        .lines()
        .find_map(|line| line.strip_prefix("recovery "))
        .and_then(bytes32);
    // A known author stays: only a rotation moves it to another key
    // (`keys::accept_rotation`), never a mark. Code is named as installed
    // only where its author's key signed it (any, where none is known).
    let pinned = native::known_author(storage, id);
    let author = pinned.or_else(|| {
        text.lines()
            .find_map(|line| line.strip_prefix("author "))
            .and_then(bytes32)
    });
    let mut kept = true;
    for line in text.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        kept &= match key {
            "from" => match value.split_once(' ') {
                // As its plugin.ini writes it, where that is the version
                // marked (a beta's pre-release with it).
                Some(("store", version)) => Version::parse(version.trim()).is_none_or(|v| {
                    let written = super::folder_release(storage, folder)
                        .filter(|written| Version::parse(written) == Some(v))
                        .unwrap_or_else(|| format!("{v}"));
                    super::remember_store_install(storage, id, written)
                }),
                _ => super::forget_store_install(storage, id),
            },
            "code" => match bytes32(value)
                .map(|sha256| (sha256, code_is(storage, folder, &sha256, author)))
            {
                Some((sha256, Some(true))) => native::remember_code(storage, id, &sha256),
                // No memory to check it now: not judged, the next start does.
                Some((_, None)) => false,
                _ => true,
            },
            "author" => bytes32(value).is_none_or(|key| match (pinned, recovery) {
                (Some(_), _) => true,
                (None, Some(recovery)) => {
                    native::keys::remember_first_install(storage, id, &key, &recovery)
                }
                (None, None) => native::remember_author(storage, id, &key),
            }),
            _ => true,
        };
    }
    // Kept for the next start where something could not be kept now.
    if kept {
        let _ = storage.remove(&path);
    }
    true
}

/// Whether the code file in `folder` is the code named `sha256` and,
/// where an `author` is known, signed by that author's key (checked
/// whole, as the signature is over all of it). `None` where the author's
/// check had no memory: neither said, so a good plugin is not taken for a
/// badly signed one (#250).
fn code_is(
    storage: &dyn Storage,
    folder: &str,
    sha256: &[u8; 32],
    author: Option<[u8; 32]>,
) -> Option<bool> {
    let mut unjudged = false;
    let is = native::CODE_FILES.iter().any(|name| {
        let path = format!("{}/{name}", folder.trim_end_matches('/'));
        native::read_header(storage, &path).is_some_and(|header| header.sha256 == *sha256)
            && author.is_none_or(|author| match native::checked_file(storage, &path) {
                Ok((_, signature)) => signature.is_some_and(|s| s.key == author),
                Err(native::Problem::Other(native::NO_MEMORY)) => {
                    unjudged = true;
                    false
                }
                Err(_) => false,
            })
    });
    (is || !unjudged).then_some(is)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use crate::install::{Origin, origin};

    /// Kept once settled, the file gone; a local one forgets the store.
    #[test]
    fn marks_are_kept_once_the_plugin_is_in_place() {
        let storage = MemoryStorage::new();
        let v = Version::new(1, 2, 0);
        assert!(write(&storage, "plugins/dark/", &Marks::new(From::Store(v))).is_ok());
        assert_eq!(origin(&storage, "dark", v), Origin::Local);
        assert!(settle(&storage, "dark", "plugins/dark"));
        assert_eq!(origin(&storage, "dark", v), Origin::Store);
        assert!(!storage.exists("plugins/dark/.install-marks"));
        assert!(!settle(&storage, "dark", "plugins/dark"));
        assert!(write(&storage, "plugins/dark", &Marks::new(From::Local)).is_ok());
        settle(&storage, "dark", "plugins/dark");
        assert_eq!(origin(&storage, "dark", v), Origin::Local);
    }

    /// Code is named only where the folder holds that code: marks put
    /// beside other code name nothing.
    #[test]
    fn code_is_named_only_for_the_code_in_the_folder() {
        let storage = MemoryStorage::new();
        let marks = Marks::new(From::Local).code([7; 32]);
        assert!(write(&storage, "plugins/clock", &marks).is_ok());
        settle(&storage, "clock", "plugins/clock");
        assert_eq!(native::installed_code(&storage, "clock"), None);
    }

    /// A recovery key handed over with the first install is pinned with
    /// the author's; with an update (the author known) it is not (#20).
    #[test]
    fn a_recovery_key_is_pinned_only_with_the_first_author() {
        let storage = MemoryStorage::new();
        let marks = Marks::new(From::Local).author([1; 32]).recovery([7; 32]);
        assert!(write(&storage, "plugins/clock", &marks).is_ok());
        assert!(settle(&storage, "clock", "plugins/clock"));
        assert_eq!(native::known_author(&storage, "clock"), Some([1; 32]));
        assert_eq!(
            native::keys::known_recovery(&storage, "clock"),
            Some([7; 32])
        );
        // An update that brings another: not taken.
        let update = Marks::new(From::Local).author([1; 32]).recovery([9; 32]);
        assert!(write(&storage, "plugins/clock", &update).is_ok());
        assert!(settle(&storage, "clock", "plugins/clock"));
        assert_eq!(
            native::keys::known_recovery(&storage, "clock"),
            Some([7; 32])
        );
        // A plugin installed before without one: none later either.
        let storage = MemoryStorage::new();
        assert!(native::remember_author(&storage, "weather", &[2; 32]));
        let update = Marks::new(From::Local).author([2; 32]).recovery([9; 32]);
        assert!(write(&storage, "plugins/weather", &update).is_ok());
        assert!(settle(&storage, "weather", "plugins/weather"));
        assert_eq!(native::keys::known_recovery(&storage, "weather"), None);
        assert!(!storage.exists("plugins/weather/.install-marks"));
    }

    /// The plugin file of `native`'s tests in `folder`, signed with `seed`
    /// (unsigned where `None`); its code's sha256.
    fn code_in(storage: &MemoryStorage, folder: &str, seed: Option<&[u8; 32]>) -> [u8; 32] {
        let mut file = crate::native::tests::file();
        let sha256 = native::read_header(&MemoryStorage::new().with("f", file.clone()), "f")
            .unwrap()
            .sha256;
        if let Some(seed) = seed {
            let signature = paperpaw_native::signature::sign(&file, seed);
            file.extend_from_slice(&signature.to_bytes());
        }
        storage.write(&format!("{folder}/code.ppn"), &file).unwrap();
        sha256
    }

    /// Marks never move a known author to another key, and name code as
    /// installed only where the author pinned then signed it: after a
    /// rotation, the old key's code is refused, the new key's taken (#20,
    /// review of #277).
    #[test]
    fn code_is_named_only_where_the_pinned_author_signed_it() {
        use paperpaw_native::signature::{public_key, sign_message};
        let (old, new, recovery) = ([1; 32], [2; 32], [3; 32]);
        let storage = MemoryStorage::new();
        // The first install pins its signer.
        let sha = code_in(&storage, "plugins/clock", Some(&old));
        let marks = Marks::new(From::Local)
            .code(sha)
            .author(public_key(&old))
            .recovery(public_key(&recovery));
        assert!(write(&storage, "plugins/clock", &marks).is_ok());
        assert!(settle(&storage, "clock", "plugins/clock"));
        assert_eq!(native::installed_code(&storage, "clock"), Some(sha));
        // A mark of another author: the known one stays.
        let marks = Marks::new(From::Local).author(public_key(&new));
        assert!(write(&storage, "plugins/clock", &marks).is_ok());
        assert!(settle(&storage, "clock", "plugins/clock"));
        assert_eq!(
            native::known_author(&storage, "clock"),
            Some(public_key(&old))
        );
        // Moved to the new key: code the old key signed, and unsigned
        // code, are named no more.
        let (from, to) = (public_key(&old), public_key(&new));
        let message = native::keys::rotation_message("clock", &from, &to);
        let signature = sign_message(message.as_bytes(), &recovery);
        assert_eq!(
            native::keys::accept_rotation(&storage, "clock", &to, &signature),
            Ok(())
        );
        for seed in [Some(&old), None] {
            let sha = code_in(&storage, "plugins/clock", seed);
            assert!(
                write(
                    &storage,
                    "plugins/clock",
                    &Marks::new(From::Local).code(sha)
                )
                .is_ok()
            );
            assert!(settle(&storage, "clock", "plugins/clock"));
            assert_eq!(native::installed_code(&storage, "clock"), None);
        }
        let sha = code_in(&storage, "plugins/clock", Some(&new));
        assert!(
            write(
                &storage,
                "plugins/clock",
                &Marks::new(From::Local).code(sha)
            )
            .is_ok()
        );
        assert!(settle(&storage, "clock", "plugins/clock"));
        assert_eq!(native::installed_code(&storage, "clock"), Some(sha));
        // Unsigned code with no author known (one's own build) is named.
        let sha = code_in(&storage, "plugins/mine", None);
        assert!(write(&storage, "plugins/mine", &Marks::new(From::Local).code(sha)).is_ok());
        assert!(settle(&storage, "mine", "plugins/mine"));
        assert_eq!(native::installed_code(&storage, "mine"), Some(sha));
    }
}
