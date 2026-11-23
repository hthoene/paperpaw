//! Fetching the list and a plugin's page: the list over `core.net` into a file of
//! the data folder, taken into the index kept on the card, and a plugin's page
//! into the plugin it describes, with the addresses of its files and pictures
//! made absolute. No screen: the plugin (`native_plugin.rs`) and the screens ask
//! [`Loader`] what it holds and tell it when a request ended. The decisions are
//! those of interface 6 (`list.rs` `fetch`/`loaded`, `page.rs` `fetch`/`fetched`).
//!
//! The host is [`Wire`]: the data folder, the device and what the settings say,
//! the network, the kernel's trust list; a stand-in in tests.
//!
//! **Differences from interface 6, on purpose:**
//! - There is one request of each kind at a time (the page a screen shows
//!   and the one the queue installs from are two kinds), so "another screen
//!   took the same answer first" cannot happen; a list that is no list leaves
//!   the index there is.
//! - The revocations of the list go to the host as they are, for the plugins
//!   that are here (`Hooks::host_checks`): the host checks them against the
//!   keys it pinned (`device.md` 5.2).
//! - A card that refuses a write while the index is built is a failed fetch
//!   (the one kept stays).
use alloc::{boxed::Box, string::String};
use paperpaw_sdk::{
    Storage,
    install::is_plugin_name,
    native::{
        Error,
        net::{Reason, RequestId},
    },
};
use paperpaw_ui_kit::{
    mem::buffer,
    text::{concat, copy},
};

use crate::native_device::Facts;
use crate::native_index::{self, Index, MAX_LIST_FILE};
use crate::native_item::HaveFn;
use crate::native_list::{Hooks, Page, PageReader};
use crate::native_model::{Revoked, join_url, list_url, plugin_url};
use crate::native_queue::read_text;
use crate::native_sources::{
    MAX_SIGNATURE_FILE, MAX_SIGNATURE_TEXT, MAX_SOURCE_LIST, SIGNATURE_PART, Signed, Trust,
    Verdict, digest, judge, signature_url,
};

/// The plugin's own number of the request for the list, in its `http_done`.
pub const LIST_TOKEN: u32 = 0x5301;
/// The plugin's own number of the request for the page a screen shows.
pub const PAGE_TOKEN: u32 = 0x5302;
/// The plugin's own number of the request for a community source's signature.
pub const SIGNATURE_TOKEN: u32 = 0x5303;
/// The plugin's own number of the request for the page the queue installs
/// from.
pub const QUEUE_PAGE_TOKEN: u32 = 0x5304;
/// Where the list is fetched to, in the data folder.
pub const LIST_PART: &str = "list.part";
/// The longest page of a plugin read (bytes).
pub const MAX_PAGE_FILE: u64 = 512 * 1024;
/// A page is fed to the reader in pieces of this many bytes.
const PIECE: usize = 1024;
/// On a small device only screenshots up to this many pixels on a side are kept
/// (the decoder's limit), and up to this many bytes.
const SMALL_SHOT_SIDE: u32 = 1280;
const SMALL_SHOT_BYTES: u64 = 300 * 1000;

/// Why there is no list to show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Trouble {
    #[default]
    None,
    Loading,
    Offline,
    Failed,
    /// The device had no memory for the connection (a restart gives it back).
    Memory,
    /// The libraries have nothing made for this device (no list is asked for).
    Unsupported,
    /// A community source's list has no signature that holds: it is not read.
    Unsigned,
    /// A community source's list is signed by another key than the one kept
    /// of the source: it is not read.
    KeyChanged,
    /// A community source's list is longer than the store reads.
    TooLarge,
}

/// How far a plugin's page is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Details {
    /// Nothing asked.
    #[default]
    None,
    /// Asked; the answer is awaited.
    Asked,
    /// The page is here ([`Loader::page`]).
    Here,
    /// No answer at all.
    Offline,
    /// An answer that is no page of this plugin, or none can be asked for
    /// (a name that is none, a library that does not know the device).
    Failed,
}

/// What the host gives the fetch.
pub trait Wire {
    /// The plugin's data folder.
    fn storage(&self) -> &dyn Storage;
    /// The device and what is installed on it; `None` where plugin libraries do
    /// not know the device.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals.
    fn facts(&mut self) -> Result<Option<Facts>, Error>;
    /// The library's address and whether community plugins are listed too.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals.
    fn catalog(&mut self) -> Result<(String, bool), Error>;
    /// Asks for `url`, its answer saved to `file` in the data folder (at most
    /// `max` bytes), the end told with `token`.
    ///
    /// # Errors
    /// `Unsupported` or `Denied` without the network; what the host says.
    fn get(&mut self, url: &str, file: &str, token: u32, max: u64) -> Result<RequestId, Error>;
    /// Ends the requests of `token`; one that cannot be ended ends by itself and
    /// is ignored.
    fn cancel(&mut self, token: u32);
    /// The address the answer of `id` came from, after redirects.
    fn final_url(&mut self, id: RequestId) -> Option<String>;
    /// Whether the device has little memory (the X4): fewer pictures are kept.
    fn small_memory(&self) -> bool;
    /// Hands the revocations of the list to the host.
    ///
    /// # Errors
    /// What the host says; only `NoMemory` stops the fetch.
    fn revoke(&mut self, revoked: &[Revoked]) -> Result<(), Error>;
    /// Whose word the list of [`catalog`](Self::catalog) is read on: PaperPaw's
    /// own library (the default), or a community source with the key kept of
    /// it (#214).
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals.
    fn trust(&mut self) -> Result<Trust, Error> {
        Ok(Trust::Official)
    }
    /// Keeps `key` as the key of the community source listed, whose first
    /// list it signed (trust on first use).
    ///
    /// # Errors
    /// `NoMemory`; `Io` where the card refuses; `Denied` where the source has
    /// another key or the key is PaperPaw's own: the list is not read then.
    fn pin(&mut self, key: &[u8; 32]) -> Result<(), Error> {
        let _ = key;
        Err(Error::Denied)
    }
}

/// A request that ended, as far as the fetch reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Done {
    /// The request.
    pub id: RequestId,
    /// The plugin's own number of it.
    pub token: u32,
    /// Why it ended.
    pub reason: Reason,
    /// The HTTP status, 0 where none came.
    pub status: u32,
}

impl Done {
    /// The end of the request `id`.
    #[must_use]
    pub const fn new(id: RequestId, token: u32, reason: Reason, status: u32) -> Self {
        Self {
            id,
            token,
            reason,
            status,
        }
    }

    /// A 2xx answer came and its body is in the file.
    #[must_use]
    pub const fn saved(&self) -> bool {
        matches!(self.reason, Reason::Ok) && self.status >= 200 && self.status < 300
    }
}

/// What the end of a request came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ended {
    /// Not a request under way (cancelled, replaced, not ours).
    Ignored,
    /// A fresh list took the place of the one kept: what was picked from the
    /// old one is picked again.
    ListChanged,
    /// The list is as it was ([`Loader::trouble`] says why there is none).
    ListUnchanged,
    /// The page a screen asked for is here or is not ([`Loader::details`]).
    Page,
    /// The page the queue asked for is here or is not
    /// ([`Loader::queue_details`]).
    QueuePage,
}

/// The list and the page the store holds.
#[derive(Debug, Default)]
pub struct Loader {
    trouble: Trouble,
    /// The last list asked for was held back by Offline mode (the host ended
    /// it `Offline`): the start offers to go online (#459, as Beta 3).
    held: bool,
    list: Option<RequestId>,
    /// The request for a community source's signature, and where it is.
    signature: Option<RequestId>,
    signature_url: String,
    index: Option<Index>,
    /// The page a screen shows.
    page: Option<PageSlot>,
    /// The page the queue installs from: apart from the screens', so neither
    /// takes the other's place. It is let go once its install started (on
    /// the heap: no room is kept for it while the queue asks for nothing).
    queued: Option<Box<PageSlot>>,
}

/// The page of one plugin and how far it is.
#[derive(Debug)]
struct PageSlot {
    id: String,
    /// The address asked for, which relative addresses are joined to where the
    /// answer says no other.
    url: String,
    request: Option<RequestId>,
    details: Details,
    plugin: Option<Box<Page>>,
}

impl Loader {
    /// Reads the index kept from an earlier visit, so that it shows at once while
    /// a fresh list is fetched.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn open(&mut self, storage: &dyn Storage) -> Result<(), Error> {
        if self.index.is_none() {
            self.index = Index::kept(storage)?;
        }
        Ok(())
    }

    /// Why there is no list (or that one is being fetched).
    #[must_use]
    pub const fn trouble(&self) -> Trouble {
        self.trouble
    }

    /// Whether the last list asked for was held back by Offline mode.
    #[must_use]
    pub const fn held_offline(&self) -> bool {
        self.held
    }

    /// The list read, `None` until one was.
    #[must_use]
    pub const fn index(&self) -> Option<&Index> {
        self.index.as_ref()
    }

    /// How far the page a screen last asked for is, and whose it is.
    #[must_use]
    pub fn details(&self) -> (Option<&str>, Details) {
        details_of(self.page.as_ref())
    }

    /// How far the page the queue last asked for is, and whose it is.
    #[must_use]
    pub fn queue_details(&self) -> (Option<&str>, Details) {
        details_of(self.queued.as_deref())
    }

    /// The page of plugin `id` a screen asked for, where it is here.
    #[must_use]
    pub fn page(&self, id: &str) -> Option<&Page> {
        page_of(self.page.as_ref(), id)
    }

    /// The page of plugin `id` the queue asked for, where it is here.
    #[must_use]
    pub fn queue_page(&self, id: &str) -> Option<&Page> {
        page_of(self.queued.as_deref(), id)
    }

    /// Lets go of the list: another source is listed from now on. A fetch
    /// under way is ended; the index on the card stays.
    pub fn forget_list(&mut self, wire: &mut dyn Wire) {
        wire.cancel(LIST_TOKEN);
        wire.cancel(SIGNATURE_TOKEN);
        self.list = None;
        self.signature = None;
        self.index = None;
        let _ = wire.storage().remove(LIST_PART);
        let _ = wire.storage().remove(SIGNATURE_PART);
        self.trouble = Trouble::None;
    }

    /// No index is kept, in memory or on the card (an index is whole where
    /// its head is).
    fn drop_index(&mut self, storage: &dyn Storage) {
        self.index = None;
        let _ = storage.remove(native_index::HEAD);
    }

    /// Lets go of the page a screen asked for (the last screen of a plugin
    /// is left): a request under way is ended and its part goes.
    pub fn forget_page(&mut self, wire: &mut dyn Wire) {
        if let Some(slot) = self.page.take() {
            slot.give_up(wire, PAGE_TOKEN);
        }
    }

    /// Lets go of the page the queue asked for: a request under way is
    /// ended and its part goes.
    pub fn forget_queue_page(&mut self, wire: &mut dyn Wire) {
        if let Some(slot) = self.queued.take().map(|slot| *slot) {
            slot.give_up(wire, QUEUE_PAGE_TOKEN);
        }
    }

    /// Lets go of the queue's page of plugin `id` once its install took what
    /// it needs of it (one asked for another plugin since stays).
    pub fn release_queue_page(&mut self, id: &str) {
        if self
            .queued
            .as_ref()
            .is_some_and(|slot| slot.id == id && slot.request.is_none())
        {
            self.queued = None;
        }
    }

    /// Fetches the list anew: in the library's fair order, with what it picks
    /// for a start; a library that knows neither sends its list as before. Where
    /// the library does not know the device nothing is asked.
    ///
    /// # Errors
    /// `NoMemory` (nothing changes then) and the host's refusals of the facts.
    #[inline(never)]
    pub fn start_list(&mut self, wire: &mut dyn Wire) -> Result<(), Error> {
        self.held = false;
        let Some(facts) = wire.facts()? else {
            // Nothing is asked of a library that does not know the device, and
            // the start says so instead of loading for ever.
            wire.cancel(LIST_TOKEN);
            self.list = None;
            self.trouble = Trouble::Unsupported;
            return Ok(());
        };
        let (catalog, community) = wire.catalog()?;
        let url = concat(&[
            &list_url(&catalog, facts.name(), facts.language(), community, None)?,
            "&order=fair",
        ])?;
        // A community source's list is smaller, and its signature is asked
        // for once it is here.
        let (max, signature) = match wire.trust()? {
            Trust::Official => (MAX_LIST_FILE, String::new()),
            _ => (MAX_SOURCE_LIST, signature_url(&url)?),
        };
        wire.cancel(LIST_TOKEN);
        wire.cancel(SIGNATURE_TOKEN);
        self.list = None;
        self.signature = None;
        self.signature_url = signature;
        match wire.get(&url, LIST_PART, LIST_TOKEN, max) {
            Ok(id) => {
                self.list = Some(id);
                self.trouble = Trouble::Loading;
                Ok(())
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // No network service: no connection.
            Err(Error::Unsupported | Error::Denied) => {
                self.trouble = Trouble::Offline;
                Ok(())
            }
            Err(_) => {
                self.trouble = Trouble::Failed;
                Ok(())
            }
        }
    }

    /// Fetches the page of plugin `id` for a screen; the one a screen asked
    /// for before, of another plugin or this, is let go.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals of the facts.
    pub fn start_page(&mut self, wire: &mut dyn Wire, id: &str) -> Result<(), Error> {
        self.forget_page(wire);
        self.page = Some(ask_page(wire, id, PAGE_TOKEN)?);
        Ok(())
    }

    /// Fetches the page of plugin `id` for the queue; the one the queue
    /// asked for before is let go, a screen's is not.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals of the facts.
    pub fn start_queue_page(&mut self, wire: &mut dyn Wire, id: &str) -> Result<(), Error> {
        self.forget_queue_page(wire);
        self.queued = Some(paperpaw_ui_kit::mem::boxed(ask_page(
            wire,
            id,
            QUEUE_PAGE_TOKEN,
        )?)?);
        Ok(())
    }

    /// A request ended.
    ///
    /// # Errors
    /// `NoMemory`: the fetch is over, what was held stays.
    #[inline(never)]
    pub fn ended(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Ended, Error> {
        match done.token {
            LIST_TOKEN if self.list == Some(done.id) => {
                self.list = None;
                let ended = self.list_ended(wire, done);
                self.settle(wire, ended)
            }
            SIGNATURE_TOKEN if self.signature == Some(done.id) => {
                self.signature = None;
                let ended = self.signature_ended(wire, done);
                self.settle(wire, ended)
            }
            PAGE_TOKEN => match self.page.as_mut() {
                Some(slot) if slot.request == Some(done.id) => {
                    slot.ended(wire, done).map(|()| Ended::Page)
                }
                _ => Ok(Ended::Ignored),
            },
            QUEUE_PAGE_TOKEN => match self.queued.as_mut() {
                Some(slot) if slot.request == Some(done.id) => {
                    slot.ended(wire, done).map(|()| Ended::QueuePage)
                }
                _ => Ok(Ended::Ignored),
            },
            _ => Ok(Ended::Ignored),
        }
    }

    /// A fetch that cannot go on leaves no part behind and no "loading" that
    /// never ends.
    fn settle(&mut self, wire: &mut dyn Wire, ended: Result<Ended, Error>) -> Result<Ended, Error> {
        if ended.is_err() {
            let _ = wire.storage().remove(LIST_PART);
            let _ = wire.storage().remove(SIGNATURE_PART);
            self.trouble = if self.index.is_some() {
                Trouble::None
            } else {
                Trouble::Failed
            };
        }
        ended
    }

    /// No list came: the one kept still serves; no answer at all is no
    /// connection.
    fn none(&mut self, wire: &mut dyn Wire, done: &Done) -> Ended {
        let _ = wire.storage().remove(LIST_PART);
        let _ = wire.storage().remove(SIGNATURE_PART);
        self.held = done.reason == Reason::Offline;
        self.trouble = match (self.index.is_some(), done.status) {
            (true, _) => Trouble::None,
            // The host tells a connection it had no memory for the same
            // way as an answer over the limit.
            (false, 0) if done.reason == Reason::TooLarge => Trouble::Memory,
            (false, 0) => Trouble::Offline,
            (false, _) => Trouble::Failed,
        };
        Ended::ListUnchanged
    }

    /// A community source's list is not read: neither it nor an index kept of
    /// the source serves, and the start says why.
    fn refuse(&mut self, wire: &mut dyn Wire, why: Trouble) -> Ended {
        let _ = wire.storage().remove(LIST_PART);
        let _ = wire.storage().remove(SIGNATURE_PART);
        self.drop_index(wire.storage());
        self.trouble = why;
        Ended::ListUnchanged
    }

    #[inline(never)]
    fn list_ended(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Ended, Error> {
        if wire.trust()? == Trust::Official {
            if !done.saved() {
                return Ok(self.none(wire, done));
            }
            return self.take_list(wire, done);
        }
        // An answer over the limit (the host stops it) or a file that is.
        let too_large = (done.reason == Reason::TooLarge && done.status != 0)
            || wire
                .storage()
                .size(LIST_PART)
                .is_ok_and(|size| size > MAX_SOURCE_LIST);
        if too_large {
            return Ok(self.refuse(wire, Trouble::TooLarge));
        }
        if !done.saved() {
            return Ok(self.none(wire, done));
        }
        // The list waits in its file for the signature that says it is the
        // source's.
        let url = core::mem::take(&mut self.signature_url);
        match wire.get(&url, SIGNATURE_PART, SIGNATURE_TOKEN, MAX_SIGNATURE_FILE) {
            Ok(id) => {
                self.signature = Some(id);
                Ok(Ended::Ignored)
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(self.none(wire, &Done::new(done.id, done.token, Reason::Ok, 0))),
        }
    }

    /// The signature of a community source's list came, or did not: the list
    /// is read where the source's key signed it.
    #[inline(never)]
    fn signature_ended(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Ended, Error> {
        // No answer at all is no connection, not a list without a signature.
        if !done.saved() && done.status == 0 {
            return Ok(self.none(wire, done));
        }
        let trust = wire.trust()?;
        if trust == Trust::Official {
            // PaperPaw's library is listed again: this answer is nobody's.
            return Ok(Ended::Ignored);
        }
        let signed = if done.saved() {
            read_text(wire.storage(), SIGNATURE_PART, MAX_SIGNATURE_TEXT)?
                .and_then(|text| Signed::read(&text))
        } else {
            None
        };
        let Some(hash) = digest(wire.storage(), LIST_PART) else {
            return Ok(self.none(wire, done));
        };
        match judge(trust, signed.as_ref(), &hash) {
            Verdict::Trusted { key, first } => {
                if first {
                    match wire.pin(&key) {
                        Ok(()) => {}
                        Err(Error::NoMemory) => return Err(Error::NoMemory),
                        // A key that cannot be kept is not trusted.
                        Err(_) => return Ok(self.refuse(wire, Trouble::Failed)),
                    }
                }
                let _ = wire.storage().remove(SIGNATURE_PART);
                self.take_list(wire, done)
            }
            Verdict::KeyChanged => Ok(self.refuse(wire, Trouble::KeyChanged)),
            _ => Ok(self.refuse(wire, Trouble::Unsigned)),
        }
    }

    /// Takes the list fetched into the index.
    #[inline(never)]
    fn take_list(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Ended, Error> {
        let Some(facts) = wire.facts()? else {
            return Ok(self.none(wire, done));
        };
        let have = |id: &str| facts.have(id);
        let device = facts.device(&have as &HaveFn<'_>);
        let nobody = |_: &str| None;
        let hooks = Hooks {
            author_of: &nobody,
            stopped: &|| alloc::vec::Vec::new(),
            host_checks: true,
        };
        let taken = native_index::take(wire.storage(), LIST_PART, device, hooks);
        match taken {
            Ok(Some((index, list))) => {
                self.index = Some(index);
                self.trouble = Trouble::None;
                if let Err(Error::NoMemory) = wire.revoke(&list.revoked) {
                    return Err(Error::NoMemory);
                }
                Ok(Ended::ListChanged)
            }
            Ok(None) | Err(Error::Io) => Ok(self.none(wire, done)),
            Err(error) => {
                let _ = wire.storage().remove(LIST_PART);
                Err(error)
            }
        }
    }
}

impl PageSlot {
    /// The request of this page ended: the page is read, or why not is kept.
    /// Its part goes either way.
    ///
    /// # Errors
    /// `NoMemory`: the page counts as failed.
    #[inline(never)]
    fn ended(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<(), Error> {
        let read = self.read(wire, done);
        if read.is_err() {
            self.details = Details::Failed;
        }
        read
    }

    fn read(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<(), Error> {
        self.request = None;
        let file = part_of(&self.id, done.token)?;
        let outcome = if done.saved() {
            let address = wire.final_url(done.id);
            let base = address.as_deref().unwrap_or(&self.url);
            read_page(wire, &file, base).map(|page| page.filter(|page| page.entry.id == self.id))
        } else {
            Ok(None)
        };
        let _ = wire.storage().remove(&file);
        match outcome {
            Ok(Some(mut page)) => {
                if wire.small_memory() {
                    page.more.shots.retain(|shot| {
                        shot.width <= SMALL_SHOT_SIDE
                            && shot.height <= SMALL_SHOT_SIDE
                            && shot.size <= SMALL_SHOT_BYTES
                    });
                }
                self.plugin = Some(Box::new(page));
                self.details = Details::Here;
            }
            Ok(None) | Err(Error::Io) => {
                // No answer at all is no connection (the lists say so too).
                self.details = if done.status == 0 {
                    Details::Offline
                } else {
                    Details::Failed
                };
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }

    /// The page is let go: its request under way (asked with `token`) is
    /// ended and its part goes.
    fn give_up(self, wire: &mut dyn Wire, token: u32) {
        if self.request.is_some() {
            wire.cancel(token);
            if let Ok(file) = part_of(&self.id, token) {
                let _ = wire.storage().remove(&file);
            }
        }
    }
}

/// How far the page of `slot` is, and whose it is.
fn details_of(slot: Option<&PageSlot>) -> (Option<&str>, Details) {
    match slot {
        Some(slot) => (Some(slot.id.as_str()), slot.details),
        None => (None, Details::None),
    }
}

/// The page of `slot`, where it is plugin `id`'s and here.
fn page_of<'a>(slot: Option<&'a PageSlot>, id: &str) -> Option<&'a Page> {
    slot.filter(|slot| slot.id == id)
        .and_then(|slot| slot.plugin.as_deref())
}

/// Asks for the page of plugin `id` with `token`. A request the host refuses is no connection (no network
/// service, or too many requests at once: opened again, the page may come);
/// a name that is none, a library that does not know the device or an
/// address that cannot be made is a page that is not there.
///
/// # Errors
/// `NoMemory` and the host's refusals of the facts.
#[inline(never)]
fn ask_page(wire: &mut dyn Wire, id: &str, token: u32) -> Result<PageSlot, Error> {
    let mut slot = PageSlot {
        id: copy(id)?,
        url: String::new(),
        request: None,
        details: Details::Failed,
        plugin: None,
    };
    let facts = match wire.facts() {
        Ok(facts) => facts.filter(|_| is_plugin_name(id)),
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => None,
    };
    let Some(facts) = facts else {
        return Ok(slot);
    };
    let (catalog, community) = wire.catalog()?;
    let url = match plugin_url(
        &catalog,
        facts.name(),
        facts.language(),
        id,
        community,
        None,
    ) {
        Ok(url) => url,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(slot),
    };
    let asked = wire.get(&url, &part_of(id, token)?, token, MAX_PAGE_FILE);
    slot.url = url;
    slot.details = match asked {
        Ok(request) => {
            slot.request = Some(request);
            Details::Asked
        }
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => Details::Offline,
    };
    Ok(slot)
}

/// Where the page of `id` asked with `token` is fetched to: a file of its own,
/// so that pages fetched at once (a screen's and the queue's) do not write
/// into each other.
fn part_of(id: &str, token: u32) -> Result<String, Error> {
    let prefix = if token == QUEUE_PAGE_TOKEN {
        "queue-"
    } else {
        "plugin-"
    };
    concat(&[prefix, id, ".part"])
}

/// Reads the page fetched to `file`, its files and pictures made absolute with
/// `base` (where the answer says where it came from).
#[inline(never)]
fn read_page(wire: &mut dyn Wire, file: &str, base: &str) -> Result<Option<Page>, Error> {
    let Some(page) = parse_page(wire, file)? else {
        return Ok(None);
    };
    let mut page = page;
    absolute(&mut page, base)?;
    Ok(Some(page))
}

/// The page in `file`. The reader lives in this frame alone (X4: under 2,048 B).
#[inline(never)]
fn parse_page(wire: &mut dyn Wire, file: &str) -> Result<Option<Page>, Error> {
    let Some(facts) = wire.facts()? else {
        return Ok(None);
    };
    let have = |id: &str| facts.have(id);
    let device = facts.device(&have as &HaveFn<'_>);
    let storage = wire.storage();
    let mut reader = PageReader::new(device);
    let mut piece = buffer(PIECE)?;
    let mut at = 0u64;
    loop {
        let n = storage
            .read_at(file, at, &mut piece)
            .map_err(|_| Error::Io)?;
        if n == 0 {
            break;
        }
        reader.feed(piece.get(..n).unwrap_or_default())?;
        at = at.saturating_add(n as u64);
    }
    reader.finish()
}

/// The addresses of a page's files and pictures, relative to the page's own.
#[inline(never)]
fn absolute(page: &mut Page, base: &str) -> Result<(), Error> {
    for file in &mut page.files {
        file.url = join_url(base, &file.url)?;
    }
    for shot in &mut page.more.shots {
        shot.url = join_url(base, &shot.url)?;
    }
    Ok(())
}
