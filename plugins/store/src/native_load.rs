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
//! - There is one request of each kind at a time, so "another screen took the
//!   same answer first" cannot happen; a list that is no list leaves the index
//!   there is.
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
    mem::{boxed, buffer},
    text::{concat, copy},
};

use crate::native_device::Facts;
use crate::native_index::{self, Index, MAX_LIST_FILE};
use crate::native_item::HaveFn;
use crate::native_list::{Hooks, Page, PageReader};
use crate::native_model::{Revoked, join_url, list_url, plugin_url};

/// The plugin's own number of the request for the list, in its `http_done`.
pub const LIST_TOKEN: u32 = 0x5301;
/// The plugin's own number of the request for a plugin's page.
pub const PAGE_TOKEN: u32 = 0x5302;
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
    /// The libraries have nothing made for this device (no list is asked for).
    Unsupported,
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
    /// An answer that is no page of this plugin.
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
    /// The page asked for is here or is not ([`Loader::details`]).
    Page,
}

/// The list and the page the store holds.
#[derive(Debug, Default)]
pub struct Loader {
    trouble: Trouble,
    list: Option<RequestId>,
    /// Boxed, as the page: the plugin keeps only a pointer while the store is
    /// not open (#248).
    index: Option<Box<Index>>,
    page: Option<Box<PageSlot>>,
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
            self.index = match Index::kept(storage)? {
                Some(index) => Some(boxed(index)?),
                None => None,
            };
        }
        Ok(())
    }

    /// Why there is no list (or that one is being fetched).
    #[must_use]
    pub const fn trouble(&self) -> Trouble {
        self.trouble
    }

    /// The list read, `None` until one was.
    #[must_use]
    pub fn index(&self) -> Option<&Index> {
        self.index.as_deref()
    }

    /// How far the page of the plugin last asked for is, and whose it is.
    #[must_use]
    pub fn details(&self) -> (Option<&str>, Details) {
        match &self.page {
            Some(slot) => (Some(slot.id.as_str()), slot.details),
            None => (None, Details::None),
        }
    }

    /// The page of plugin `id`, where it is here.
    #[must_use]
    pub fn page(&self, id: &str) -> Option<&Page> {
        self.page
            .as_ref()
            .filter(|slot| slot.id == id)
            .and_then(|slot| slot.plugin.as_deref())
    }

    /// Lets go of the page held (the screen of the plugin is left).
    pub fn forget_page(&mut self) {
        self.page = None;
    }

    /// Fetches the list anew: in the library's fair order, with what it picks
    /// for a start; a library that knows neither sends its list as before. Where
    /// the library does not know the device nothing is asked.
    ///
    /// # Errors
    /// `NoMemory` (nothing changes then) and the host's refusals of the facts.
    #[inline(never)]
    pub fn start_list(&mut self, wire: &mut dyn Wire) -> Result<(), Error> {
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
        wire.cancel(LIST_TOKEN);
        self.list = None;
        match wire.get(&url, LIST_PART, LIST_TOKEN, MAX_LIST_FILE) {
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

    /// Fetches the page of plugin `id`; the one under way, of another plugin or
    /// this, is given up.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals of the facts.
    #[inline(never)]
    pub fn start_page(&mut self, wire: &mut dyn Wire, id: &str) -> Result<(), Error> {
        wire.cancel(PAGE_TOKEN);
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
        if let Some(facts) = facts {
            let (catalog, community) = wire.catalog()?;
            let url = plugin_url(
                &catalog,
                facts.name(),
                facts.language(),
                id,
                community,
                None,
            );
            slot.details = match url {
                Ok(url) => {
                    let file = part_of(id)?;
                    let asked = wire.get(&url, &file, PAGE_TOKEN, MAX_PAGE_FILE);
                    slot.url = url;
                    match asked {
                        Ok(request) => {
                            slot.request = Some(request);
                            Details::Asked
                        }
                        Err(Error::NoMemory) => return Err(Error::NoMemory),
                        Err(Error::Unsupported | Error::Denied) => Details::Offline,
                        Err(_) => Details::Failed,
                    }
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => Details::Failed,
            };
        }
        self.page = Some(boxed(slot)?);
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
                if ended.is_err() {
                    // A fetch that cannot go on leaves no part behind and no
                    // "loading" that never ends.
                    let _ = wire.storage().remove(LIST_PART);
                    self.trouble = if self.index.is_some() {
                        Trouble::None
                    } else {
                        Trouble::Failed
                    };
                }
                ended
            }
            PAGE_TOKEN
                if self
                    .page
                    .as_ref()
                    .is_some_and(|slot| slot.request == Some(done.id)) =>
            {
                let ended = self.page_ended(wire, done);
                if let (Err(_), Some(slot)) = (&ended, self.page.as_mut()) {
                    slot.details = Details::Failed;
                }
                ended
            }
            _ => Ok(Ended::Ignored),
        }
    }

    #[inline(never)]
    fn list_ended(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Ended, Error> {
        let none = |wire: &mut dyn Wire, this: &mut Self| {
            let _ = wire.storage().remove(LIST_PART);
            // The list kept still serves; no answer at all is no connection.
            this.trouble = match (this.index.is_some(), done.status) {
                (true, _) => Trouble::None,
                (false, 0) => Trouble::Offline,
                (false, _) => Trouble::Failed,
            };
            Ok(Ended::ListUnchanged)
        };
        if !done.saved() {
            return none(wire, self);
        }
        let Some(facts) = wire.facts()? else {
            return none(wire, self);
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
                self.index = Some(boxed(index)?);
                self.trouble = Trouble::None;
                if let Err(Error::NoMemory) = wire.revoke(&list.revoked) {
                    return Err(Error::NoMemory);
                }
                Ok(Ended::ListChanged)
            }
            Ok(None) | Err(Error::Io) => none(wire, self),
            Err(error) => {
                let _ = wire.storage().remove(LIST_PART);
                Err(error)
            }
        }
    }

    #[inline(never)]
    fn page_ended(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Ended, Error> {
        let Some(slot) = self.page.as_mut() else {
            return Ok(Ended::Ignored);
        };
        slot.request = None;
        let file = part_of(&slot.id)?;
        let outcome = if done.saved() {
            let address = wire.final_url(done.id);
            let base = address.as_deref().unwrap_or(&slot.url);
            read_page(wire, &file, base).map(|page| page.filter(|page| page.entry.id == slot.id))
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
                slot.plugin = Some(Box::new(page));
                slot.details = Details::Here;
            }
            Ok(None) | Err(Error::Io) => {
                // No answer at all is no connection (the lists say so too).
                slot.details = if done.status == 0 {
                    Details::Offline
                } else {
                    Details::Failed
                };
            }
            Err(error) => return Err(error),
        }
        Ok(Ended::Page)
    }
}

/// Where the page of `id` is fetched to: a file of its own, so that pages fetched
/// at once do not write into each other.
fn part_of(id: &str) -> Result<String, Error> {
    concat(&["plugin-", id, ".part"])
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
