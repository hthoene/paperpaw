//! Installing what is downloaded (slice 8, host-free). The host does the
//! installing (`docs/abi/device.md` 5.2): it copies the staged folder into its
//! own stage, checks the code and the trust rules, asks the reader and swaps; the
//! store carries the library's word (the author's key, the recovery key, the
//! rotations) in the same batch as `install_plugin` ([`Wire::install`]) and says
//! what became of it when `install_done` comes ([`Installer::done`], whose
//! reasons are `native_refusal`'s). The interface-6 `trust.rs` (`verdict`,
//! `follow`, `check_file`, the room for code) is the host's now.
//!
//! What stays the store's: the evidence it hands over, the rule that the
//! version installed is not replaced by an older or the same one (the host's
//! reason 14, unless the entry is a repair of a build that does not run), the
//! revocations the library lists, and the record of what the store installed
//! ([`Installs`]), which says whether it may offer a plugin's updates and as
//! which release (interface 6 `install::origin`, `plugin-origin.txt`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::install::is_plugin_name;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

use crate::native_download::{Download, Failure, Staged, Step};
use crate::native_item::Rotation;
use crate::native_list::Page;
use crate::native_model::Revoked;
use crate::native_queue::Queue;
use crate::native_refusal::{self as words, Outcome as Said};

/// The most rotation steps the host takes in one install (`device.md` 3): a
/// longer chain ends with reason 6 where the signing key is not within them.
pub const MAX_STEPS: usize = 6;
/// The most revocations handed over in one go (the SDK's bound).
pub const MAX_REVOKED: usize = 16;
/// The file of the versions the store installed (lines `<id>\t<release>`): the
/// interface-6 file (`paperpaw_sdk::data_path!("plugin-origin.txt")`), so what
/// Beta 3 kept is read as it was.
pub const INSTALLS_FILE: &str = ".paperpaw/plugin-origin.txt";
/// The most of it read, and the versions kept of one plugin: the one
/// installed, and those before it in case an install did not complete.
const MAX_INSTALLS: usize = 64 * 1024;
const VERSIONS_KEPT: usize = 4;

/// The library's word about a code plugin, carried with its install.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Proof {
    /// The key the library names for the plugin's author.
    pub author: Option<[u8; 32]>,
    /// The recovery key it names, pinned at a first install only.
    pub recovery: Option<[u8; 32]>,
    /// The moves to other keys, in the author's order, at most [`MAX_STEPS`]:
    /// the last ones where there are more, the host skipping those before the
    /// key it has pinned.
    pub rotations: Vec<Rotation>,
}

impl Proof {
    /// What the library says of `page`'s plugin; nothing for a data plugin.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn of(page: &Page) -> Result<Self, Error> {
        let mut proof = Self::default();
        if !page.code {
            return Ok(proof);
        }
        proof.author = page.author_key;
        proof.recovery = page.recovery_key;
        let from = page.rotations.len().saturating_sub(MAX_STEPS);
        for rotation in page.rotations.iter().skip(from) {
            push(&mut proof.rotations, *rotation)?;
        }
        Ok(proof)
    }
}

/// What the store sends the host: the network of the install, as the plugin
/// has it (`CommandSender::install_plugin_with`, `trust_revoked`). A stand-in in
/// tests.
pub trait Wire {
    /// The plugin's data folder.
    fn storage(&self) -> &dyn Storage;
    /// Sends `install_plugin` for the staged `folder` of the data folder with
    /// its `proof`: `update` replaces an installed plugin of the id,
    /// `allow_older` takes an older or the same version too. The end comes as
    /// `install_done` with `request`.
    ///
    /// # Errors
    /// `Denied` without `uses.install`, `Full` while four requests wait, what the
    /// host says.
    fn install(
        &mut self,
        request: u32,
        folder: &str,
        update: bool,
        allow_older: bool,
        proof: &Proof,
    ) -> Result<(), Error>;
    /// Hands over the library's revocations, which the host checks itself.
    ///
    /// # Errors
    /// What the host says.
    fn revoke(&mut self, revoked: &[Revoked]) -> Result<(), Error>;
}

/// What the device has of the plugin being installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Have<'a> {
    /// The version installed, as `plugin.ini` says; `None` where none is.
    pub version: Option<&'a str>,
    /// What is installed does not run on this PaperPaw (a build for an older
    /// interface or another build), so the same version is a repair
    /// (interface 6 `Entry::repairs`).
    pub repairs: bool,
}

/// What an `install_done` came to, for the store to say.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The plugin is installed (or updated); `restart`: a code plugin that
    /// starts at the next start.
    Installed {
        plugin: String,
        name: String,
        updated: bool,
        restart: bool,
    },
    /// It is not installed: the text to say, taking `{name}`.
    Refused { name: String, key: &'static str },
    /// The reader declined, or the host stopped it: nothing to say.
    Silent,
    /// Not an install of this store's (another request, or none going on).
    Other,
}

/// The installs of one plugin at a time.
#[derive(Debug, Default)]
pub struct Installer {
    /// The request of the install going on, and what it is of.
    going: Option<Going>,
    next: u32,
}

#[derive(Debug)]
struct Going {
    request: u32,
    plugin: String,
    name: String,
    folder: String,
    updated: bool,
}

impl Installer {
    /// Nothing going on.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            going: None,
            next: 0,
        }
    }

    /// Whether an install is going on (its end is not come).
    #[must_use]
    pub const fn busy(&self) -> bool {
        self.going.is_some()
    }

    /// Has the host install what `staged` holds, with the library's word of
    /// `page`. The step is `Installing`; a refusal at once is `Failed`, and the
    /// files go.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(
        &mut self,
        wire: &mut dyn Wire,
        download: &mut Download,
        staged: Staged,
        page: &Page,
        have: Have<'_>,
    ) -> Result<Step, Error> {
        if !is_plugin_name(&staged.plugin) || staged.plugin != page.entry.id {
            discard(wire.storage(), &staged.folder);
            download.set_step(Step::Failed(Failure::Damaged));
            return Ok(Step::Failed(Failure::Damaged));
        }
        let proof = Proof::of(page)?;
        // Nothing is asked of the host for a request already waiting: the
        // plugin's own numbers count up from 1, 0 is never sent.
        self.next = self.next.wrapping_add(1).max(1);
        let request = self.next;
        let update = have.version.is_some();
        let allow_older = have.repairs;
        if wire
            .install(request, &staged.folder, update, allow_older, &proof)
            .is_err()
        {
            discard(wire.storage(), &staged.folder);
            download.set_step(Step::Failed(Failure::Card));
            return Ok(Step::Failed(Failure::Card));
        }
        self.going = Some(Going {
            request,
            plugin: staged.plugin,
            name: staged.name,
            folder: staged.folder,
            updated: update,
        });
        download.set_step(Step::Installing);
        Ok(Step::Installing)
    }

    /// The end of an install request (`event.install_done`, kind 0): what the
    /// store says of it. An install that ended with reason 0 is remembered as
    /// the store's (`release` as the library names the version, a pre-release
    /// with it); the staged files go whatever it was.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn done(
        &mut self,
        wire: &mut dyn Wire,
        download: &mut Download,
        end: &End,
        release: &str,
        flash_hint: bool,
    ) -> Result<Outcome, Error> {
        match &self.going {
            Some(going) if going.request == end.request => {}
            _ => return Ok(Outcome::Other),
        }
        let Some(going) = self.going.take() else {
            return Ok(Outcome::Other);
        };
        discard(wire.storage(), &going.folder);
        match words::outcome(end.reason, flash_hint) {
            Said::Done => {
                download.set_step(Step::Installed);
                // A record that cannot be kept is no reason to say it failed:
                // the plugin is installed, the store only counts it as put in
                // place another way until it is kept again.
                let _ = Installs::remember(wire.storage(), &going.plugin, release);
                Ok(Outcome::Installed {
                    plugin: going.plugin,
                    name: going.name,
                    updated: going.updated,
                    restart: words::after_restart(end.flags),
                })
            }
            Said::Silent => {
                download.set_step(Step::Idle);
                Ok(Outcome::Silent)
            }
            Said::Said(key) => {
                download.set_step(Step::Idle);
                Ok(Outcome::Refused {
                    name: going.name,
                    key,
                })
            }
        }
    }

    /// Forgets the install going on (the screen asked to stop; the host's
    /// request still ends, and is passed over).
    pub fn forget(&mut self, wire: &mut dyn Wire) {
        if let Some(going) = self.going.take() {
            discard(wire.storage(), &going.folder);
        }
    }
}

/// An `install_done` of a plugin install, as far as the store reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct End {
    /// The request it answers.
    pub request: u32,
    /// The host's reason (`device.md` 5.5).
    pub reason: u32,
    /// Bit 1: takes effect at the next start.
    pub flags: u32,
}

impl End {
    /// The end of `request`.
    #[must_use]
    pub const fn new(request: u32, reason: u32, flags: u32) -> Self {
        Self {
            request,
            reason,
            flags,
        }
    }
}

/// Hands the revocations the library lists to the host, which checks each and
/// keeps those that hold; nothing where there are none. At most
/// [`MAX_REVOKED`] are handed over.
///
/// # Errors
/// What the host says.
pub fn hand_over(wire: &mut dyn Wire, revoked: &[Revoked]) -> Result<(), Error> {
    let take = revoked.len().min(MAX_REVOKED);
    match revoked.get(..take) {
        Some([]) | None => Ok(()),
        Some(some) => wire.revoke(some),
    }
}

/// What the queue asks for next.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Next {
    /// The queue is empty: nothing more to install.
    Idle,
    /// The plugin whose turn it is: its page is fetched, then its files.
    FetchPage(String),
}

/// Goes on with the queue (the plugin before is done, stopped or set aside):
/// gives the next plugin its turn and keeps the queue on the card, so a restart
/// finds it where it was.
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses (the turn is given anyway).
pub fn next(queue: &mut Queue, storage: &dyn Storage) -> Result<Next, Error> {
    let id = match queue.advance() {
        Some(id) => Some(copy(id)?),
        None => None,
    };
    queue.keep(storage)?;
    Ok(id.map_or(Next::Idle, Next::FetchPage))
}

fn discard(storage: &dyn Storage, folder: &str) {
    let _ = storage.remove_dir_all(folder);
}

/// The versions the store installed, kept in its data folder: a plugin counts
/// as the store's only while its version is one of them (interface 6
/// `install::origin`). Lines `<id>\t<release>`, at most [`VERSIONS_KEPT`] of a
/// plugin, the newest last.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Installs {
    kept: Vec<(String, String)>,
}

impl Installs {
    /// What the file says; none where it cannot be read.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(storage: &dyn Storage) -> Result<Self, Error> {
        let Some(text) = crate::native_queue::read_text(storage, INSTALLS_FILE, MAX_INSTALLS)?
        else {
            return Ok(Self::default());
        };
        let mut kept = Vec::new();
        for line in text.lines() {
            let Some((id, release)) = line.split_once('\t') else {
                continue;
            };
            if is_plugin_name(id) && !release.trim().is_empty() {
                push(&mut kept, (copy(id)?, copy(release.trim())?))?;
            }
        }
        Ok(Self { kept })
    }

    /// Whether the file is there (once it is, the plugins installed before are
    /// not adopted again).
    #[must_use]
    pub fn exists(storage: &dyn Storage) -> bool {
        storage.exists(INSTALLS_FILE)
    }

    /// Whether the store installed `plugin` at `release` (or a version with the
    /// same numbers: betas of one release share them).
    #[must_use]
    pub fn of(&self, plugin: &str, version: &str) -> bool {
        self.release(plugin, version).is_some()
    }

    /// The version of `plugin` the store installed with the numbers of
    /// `version`, as the store named it (`0.5.0-beta.2`); the last such one.
    #[must_use]
    pub fn release(&self, plugin: &str, version: &str) -> Option<&str> {
        let numbers = numbers_of(version);
        self.kept
            .iter()
            .rev()
            .find(|(id, release)| id == plugin && numbers_of(release) == numbers)
            .map(|(_, release)| release.as_str())
    }

    /// Remembers that the store installed `plugin` at `release`; false where
    /// the file cannot be kept (left as it was).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn remember(storage: &dyn Storage, plugin: &str, release: &str) -> Result<bool, Error> {
        if !is_plugin_name(plugin) || release.is_empty() || release.contains(['\t', '\n', '\r']) {
            return Ok(false);
        }
        let mut now = Self::read(storage)?;
        // The same release once, then room for the new one: the oldest of this
        // plugin's go when there are too many.
        now.kept
            .retain(|(id, kept)| !(id == plugin && kept == release));
        let mut keep = Vec::new();
        keep.try_reserve_exact(now.kept.len())
            .map_err(|_| Error::NoMemory)?;
        let mut seen = 0;
        for (id, _) in now.kept.iter().rev() {
            let kept = if id == plugin {
                seen += 1;
                seen < VERSIONS_KEPT
            } else {
                true
            };
            keep.push(kept);
        }
        let mut flags = keep.into_iter().rev();
        now.kept.retain(|_| flags.next().unwrap_or(true));
        push(&mut now.kept, (copy(plugin)?, copy(release)?))?;
        Ok(now.keep(storage))
    }

    /// Forgets every version of `plugin` (an install of another kind put one
    /// in place, or it was removed).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn forget(storage: &dyn Storage, plugin: &str) -> Result<bool, Error> {
        let mut now = Self::read(storage)?;
        now.kept.retain(|(id, _)| id != plugin);
        Ok(now.keep(storage))
    }

    /// Takes the plugins installed (`installed`: id and version, those built in
    /// left out) for the store's, where no record was ever kept on this device:
    /// before PaperPaw kept them, the plugins the store installed went unnamed
    /// and keep its updates. Once kept, it does nothing.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn adopt<'a>(
        storage: &dyn Storage,
        installed: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<bool, Error> {
        if Self::exists(storage) {
            return Ok(true);
        }
        let mut now = Self::default();
        for (id, version) in installed {
            if is_plugin_name(id) && !version.is_empty() && !version.contains(['\t', '\n', '\r']) {
                push(&mut now.kept, (copy(id)?, copy(version)?))?;
            }
        }
        Ok(now.keep(storage))
    }

    fn keep(&self, storage: &dyn Storage) -> bool {
        let mut text = String::new();
        for (id, release) in &self.kept {
            let Ok(line) = concat(&[id, "\t", release, "\n"]) else {
                return false;
            };
            if text.try_reserve(line.len()).is_err() {
                return false;
            }
            text.push_str(&line);
        }
        if text.is_empty() {
            return storage.remove(INSTALLS_FILE).is_ok() || !storage.exists(INSTALLS_FILE);
        }
        storage.write(INSTALLS_FILE, text.as_bytes()).is_ok()
    }
}

/// The numbers of a version, without a pre-release or build: `0.5.0` of
/// `0.5.0-beta.2+7`.
fn numbers_of(version: &str) -> &str {
    let end = version.find(['-', '+']).unwrap_or(version.len());
    version.get(..end).unwrap_or(version)
}

#[cfg(test)]
mod tests;
