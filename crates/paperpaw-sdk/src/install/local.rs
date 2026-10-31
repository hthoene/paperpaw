//! Installing a plugin that comes from this side of the device: from a
//! computer over the Wi-Fi transfer or USB, e.g. one's own build. The
//! same steps whichever way it comes, and the same rules as a store's
//! install where data is at stake:
//!
//! 1. Each file goes into a folder PaperPaw does not load
//!    ([`Staging::folder`]), checked by its SHA-256 before it is taken
//!    ([`Staging::put`]): a file cut short or changed on the way is
//!    refused, and nothing installed changes.
//! 2. Once all are there, [`Staging::commit`] checks the folder holds the
//!    plugin it says and, for a plugin with code, that its code is whole
//!    and built for this PaperPaw. Code runs only when the reader agreed
//!    on the device to that very code ([`Commit::approved`], by its
//!    SHA-256: code put in its place after the question is refused); the
//!    code is then named as installed, as a store names what it installs,
//!    once the plugin is in place ([`marks`](super::marks)).
//! 3. The kernel installs it in one step ([`Command::InstallPlugin`]):
//!    the version before stays until the new one is all there, the
//!    plugin's own data (in `.paperpaw/plugins/<id>`) is not touched, and
//!    it is loaded at once (a plugin with code: after a restart, as an
//!    update from a store).
//!
//! It is the reader's own from then on: no store updates it
//! ([`forget_store_install`](super::forget_store_install)).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;

use crate::builtin::PLUGIN_DIR;
use crate::native::{self, CODE_FILES};
use crate::{Command, Env, Id, PluginKind};

/// The longest `plugin.ini` read for the plugin's id.
const MAX_MANIFEST: usize = 16 * 1024;

/// Why a plugin was not installed (and nothing installed changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// Not a name a plugin can have, or a path out of its folder.
    Name,
    /// A file came otherwise than said: its SHA-256 differs.
    Damaged,
    /// Its storage could not take it (full, failing).
    Storage,
    /// The folder holds no `plugin.ini`, or one of another plugin.
    NotThePlugin,
    /// A plugin of that id is built into PaperPaw.
    BuiltIn,
    /// Its code is damaged, or not a plugin's.
    BadCode,
    /// This device runs no plugins with code, or not this kind of file.
    NoCode,
    /// Its code is built for another version of PaperPaw.
    OtherBuild,
    /// It has code, and the reader has not agreed to run it yet.
    NeedsApproval,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

/// A plugin being put together from files sent one by one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staging {
    plugin: String,
    /// Whose folder it is (see [`Staging::apart`]).
    place: &'static str,
}

/// What [`Staging::commit`] is told.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Commit {
    /// The code the reader agreed on the device to install and run, by
    /// its SHA-256 as installers name it ([`Staging::code`]).
    pub approved: Option<[u8; 32]>,
}

impl Commit {
    /// The reader agreed to run the code named `sha256` ([`Staging::code`]).
    pub fn approved(sha256: [u8; 32]) -> Self {
        Self {
            approved: Some(sha256),
        }
    }
}

/// What a commit found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Committed {
    pub plugin: String,
    /// It has code (named as installed, running after a restart).
    pub code: bool,
}

impl Staging {
    /// The staging of plugin `id`; `None` for a name no plugin can have.
    pub fn new(id: &str) -> Option<Self> {
        Self::apart(id, "local")
    }

    /// The staging of plugin `id` in a folder of its own, named by
    /// `place` (a word of small letters): one way a plugin comes (the
    /// cable, say) never takes or changes the files another sends. `None`
    /// for a name no plugin can have, or a `place` not such a word.
    pub fn apart(id: &str, place: &'static str) -> Option<Self> {
        let word = !place.is_empty() && place.bytes().all(|b| b.is_ascii_lowercase());
        (word && super::is_plugin_name(id)).then(|| Self {
            plugin: String::from(id),
            place,
        })
    }

    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// The folder the files go into, which PaperPaw does not load.
    pub fn folder(&self) -> String {
        format!("{PLUGIN_DIR}/.{}.{}", self.plugin, self.place)
    }

    /// Takes the file at `from` (on the storage, e.g. where a request's
    /// body was saved) as `path` in the plugin's folder, if its SHA-256
    /// is `sha256`; it replaces a file of that path sent before. `from`
    /// is gone afterwards either way.
    pub fn put(
        &self,
        storage: &dyn Storage,
        path: &str,
        from: &str,
        sha256: &[u8; 32],
    ) -> Result<(), Refusal> {
        let taken = self.take(storage, path, from, sha256);
        let _ = storage.remove(from);
        taken
    }

    fn take(
        &self,
        storage: &dyn Storage,
        path: &str,
        from: &str,
        sha256: &[u8; 32],
    ) -> Result<(), Refusal> {
        if !super::is_safe_path(path) {
            return Err(Refusal::Name);
        }
        if file_sha256(storage, from).ok_or(Refusal::Storage)? != *sha256 {
            return Err(Refusal::Damaged);
        }
        let target = format!("{}/{path}", self.folder());
        if let Some((dir, _)) = target.rsplit_once('/') {
            storage.create_dir(dir).map_err(|_| Refusal::Storage)?;
        }
        if storage.exists(&target) {
            storage.remove(&target).map_err(|_| Refusal::Storage)?;
        }
        storage.rename(from, &target).map_err(|_| Refusal::Storage)
    }

    /// The files sent so far, by their paths in the plugin's folder.
    pub fn files(&self, storage: &dyn Storage) -> Vec<String> {
        let mut files = Vec::new();
        list_files(storage, &self.folder(), "", &mut files);
        files
    }

    /// Installs the plugin from the files sent (see the [module
    /// documentation](self)), or says why not; the files stay for another
    /// try only where the reader has not agreed to its code yet.
    pub fn commit(
        &self,
        env: &Env<'_>,
        commit: Commit,
        out: &mut crate::Commands,
    ) -> Result<Committed, Refusal> {
        let checked = self.check(env, commit);
        if !matches!(checked, Ok(_) | Err(Refusal::NeedsApproval)) {
            self.abort(env.storage());
        }
        let code = checked?;
        let storage = env.storage();
        // Those the store installed before origins were kept stay its.
        super::adopt(env);
        // Named as installed, and no store's, once it is in place.
        let mut marks = super::marks::Marks::new(super::marks::From::Local);
        if let Some(sha256) = code {
            marks = marks.code(sha256);
        }
        if super::marks::write(storage, &self.folder(), &marks).is_err() {
            self.abort(storage);
            return Err(Refusal::Storage);
        }
        let id = Id::parse(&self.plugin).map_err(|_| Refusal::Name)?;
        out.push(Command::InstallPlugin(id, self.folder()));
        Ok(Committed {
            plugin: self.plugin.clone(),
            code: code.is_some(),
        })
    }

    /// Whether the files sent make the plugin; its code's SHA-256 (the
    /// header's, as installers name it) where it has code.
    fn check(&self, env: &Env<'_>, commit: Commit) -> Result<Option<[u8; 32]>, Refusal> {
        let storage = env.storage();
        let folder = self.folder();
        if folder_plugin_id(storage, &folder).as_deref() != Some(self.plugin.as_str()) {
            return Err(Refusal::NotThePlugin);
        }
        if env
            .plugins
            .get(&self.plugin)
            .is_some_and(|info| info.kind == PluginKind::BuiltIn)
        {
            return Err(Refusal::BuiltIn);
        }
        let Some(code) = CODE_FILES
            .iter()
            .map(|name| format!("{folder}/{name}"))
            .find(|path| storage.exists(path))
        else {
            return Ok(None);
        };
        let native = env
            .native
            .filter(|native| code.ends_with(&*native.file))
            .ok_or(Refusal::NoCode)?;
        let (header, _) = native::check_file(storage, &code).map_err(|_| Refusal::BadCode)?;
        if header.fingerprint != native.fingerprint.0 {
            return Err(Refusal::OtherBuild);
        }
        if commit.approved != Some(header.sha256) {
            return Err(Refusal::NeedsApproval);
        }
        Ok(Some(header.sha256))
    }

    /// The code among the files sent, as installers name it (the SHA-256
    /// its header holds, checked whole): what the reader is asked to let
    /// run ([`Commit::approved`]). `None` where there is none, or it is
    /// damaged.
    pub fn code(&self, storage: &dyn Storage) -> Option<[u8; 32]> {
        let folder = self.folder();
        CODE_FILES
            .iter()
            .map(|name| format!("{folder}/{name}"))
            .find(|path| storage.exists(path))
            .and_then(|path| native::check_file(storage, &path).ok())
            .map(|(header, _)| header.sha256)
    }

    /// Removes the files sent: nothing installed changes.
    pub fn abort(&self, storage: &dyn Storage) {
        let _ = storage.remove_dir_all(&self.folder());
    }
}

/// The id `plugin.ini` in `folder` names.
fn folder_plugin_id(storage: &dyn Storage, folder: &str) -> Option<String> {
    let bytes = crate::read_file(storage, &format!("{folder}/plugin.ini"), MAX_MANIFEST).ok()?;
    let source = core::str::from_utf8(&bytes).ok()?;
    paperpaw_config::parse(source)
        .filter_map(Result::ok)
        .find(|entry| entry.section == "plugin" && entry.key == "id")
        .map(|entry| String::from(entry.value.as_str().trim()))
}

/// The SHA-256 of the file at `path`, read a part at a time.
pub fn file_sha256(storage: &dyn Storage, path: &str) -> Option<[u8; 32]> {
    let size = storage.size(path).ok()?;
    let mut digest = paperpaw_digest::sha256::Sha256::new();
    let mut buf = [0u8; 512];
    let mut at = 0u64;
    while at < size {
        let n = storage.read_at(path, at, &mut buf).ok()?;
        if n == 0 {
            return None;
        }
        digest.update(&buf[..n]);
        at += n as u64;
    }
    Some(digest.finish())
}

/// The files below `dir`, by their paths from it (`prefix` before them).
fn list_files(storage: &dyn Storage, dir: &str, prefix: &str, out: &mut Vec<String>) {
    let mut entries: Vec<(String, bool)> = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        entries.push((String::from(entry.name), entry.is_dir));
    });
    for (name, is_dir) in entries {
        let path = format!("{prefix}{name}");
        if is_dir {
            list_files(storage, &format!("{dir}/{name}"), &format!("{path}/"), out);
        } else {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    fn sha(data: &[u8]) -> [u8; 32] {
        paperpaw_digest::sha256::digest(data)
    }

    /// A file is taken only as it was said to be; the one sent before in
    /// its place stays where the new one is refused.
    /// Stagings apart never share their files.
    #[test]
    fn stagings_apart_keep_their_files_apart() {
        assert_eq!(
            Staging::new("dark").unwrap().folder(),
            "plugins/.dark.local"
        );
        let cable = Staging::apart("dark", "cable").unwrap();
        assert_eq!(cable.folder(), "plugins/.dark.cable");
        assert!(Staging::apart("dark", "").is_none());
        assert!(Staging::apart("dark", "Cable").is_none());
        assert!(Staging::apart("dark", "a/b").is_none());
        assert!(Staging::apart("../x", "cable").is_none());
    }

    #[test]
    fn files_are_taken_by_their_digest() {
        let storage = MemoryStorage::new();
        let staging = Staging::new("dark").unwrap();
        storage.write("in/a", b"[plugin]\nid = dark\n").unwrap();
        staging
            .put(
                &storage,
                "plugin.ini",
                "in/a",
                &sha(b"[plugin]\nid = dark\n"),
            )
            .unwrap();
        assert!(!storage.exists("in/a"));
        storage.write("in/b", b"changed on the way").unwrap();
        assert_eq!(
            staging.put(&storage, "plugin.ini", "in/b", &sha(b"what was sent")),
            Err(Refusal::Damaged)
        );
        assert!(!storage.exists("in/b"));
        assert_eq!(
            crate::read_file(&storage, "plugins/.dark.local/plugin.ini", 100).unwrap(),
            b"[plugin]\nid = dark\n"
        );
        storage.write("in/c", b"x").unwrap();
        assert_eq!(
            staging.put(&storage, "../escape", "in/c", &sha(b"x")),
            Err(Refusal::Name)
        );
        storage.write("in/d", b"de").unwrap();
        staging
            .put(&storage, "i18n/de.ini", "in/d", &sha(b"de"))
            .unwrap();
        let mut files = staging.files(&storage);
        files.sort();
        assert_eq!(files, ["i18n/de.ini", "plugin.ini"]);
        assert!(Staging::new("../x").is_none());
        assert!(Staging::new("Dark").is_none());
    }
}
