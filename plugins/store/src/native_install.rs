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
//! reason 14, unless the entry is a repair of a build that does not run) and
//! the revocations the library lists. Its installs say they come from the
//! catalog (`install_plugin` `flags` bit 2): the host keeps the record of what
//! the store installed (interface 6 `install::origin`, `plugin-origin.txt`)
//! and the registry names those versions (`reg_plugin.flags` bit 5,
//! `native_installs`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::install::is_plugin_name;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::copy};

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
    #[inline(never)]
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
        let proof = match Proof::of(page) {
            Ok(proof) => proof,
            Err(error) => {
                discard(wire.storage(), &staged.folder);
                download.set_step(Step::Failed(Failure::Card));
                return Err(error);
            }
        };
        // Nothing is asked of the host for a request already waiting: the
        // plugin's own numbers count up from 1, 0 is never sent.
        self.next = self.next.wrapping_add(1).max(1);
        let request = self.next;
        let update = have.version.is_some();
        let allow_older = have.repairs;
        if let Err(error) = wire.install(request, &staged.folder, update, allow_older, &proof) {
            discard(wire.storage(), &staged.folder);
            download.set_step(Step::Failed(Failure::Card));
            // No memory is the caller's to hear; any other refusal is the
            // host's word that the install cannot start.
            return if error == Error::NoMemory {
                Err(error)
            } else {
                Ok(Step::Failed(Failure::Card))
            };
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
    /// store says of it. The staged files go whatever it was.
    ///
    /// # Errors
    /// `NoMemory`.
    #[inline(never)]
    pub fn done(
        &mut self,
        wire: &mut dyn Wire,
        download: &mut Download,
        end: &End,
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

    /// The plugin of the install going on as `request`: the host's end of a
    /// declined or refused install does not name it.
    #[must_use]
    pub fn going_plugin(&self, request: u32) -> Option<&str> {
        let going = self
            .going
            .as_ref()
            .filter(|going| going.request == request)?;
        Some(going.plugin.as_str())
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
/// `NoMemory`. A card that refuses the queue file is not an error: the turn is
/// given anyway.
pub fn next(queue: &mut Queue, storage: &dyn Storage) -> Result<Next, Error> {
    let id = match queue.advance() {
        Some(id) => Some(copy(id)?),
        None => None,
    };
    if queue.keep(storage) == Err(Error::NoMemory) {
        return Err(Error::NoMemory);
    }
    Ok(id.map_or(Next::Idle, Next::FetchPage))
}

fn discard(storage: &dyn Storage, folder: &str) {
    let _ = storage.remove_dir_all(folder);
}

#[cfg(test)]
mod tests;
