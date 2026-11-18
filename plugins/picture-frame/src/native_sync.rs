//! Fetching the server's photos: the folder is listed once (`PROPFIND`, saved
//! to a file and read as it streams), the plan says which photos of it fit the
//! cache's cap, and the photos not cached yet are fetched one at a time, each
//! saved straight to the cache (the host writes it whole or not at all).
//! Nothing is fetched but by a call of `start`: the plugin decides when (on
//! request, with the charger, or where Wi-Fi is up anyway), never at a wake.
//!
//! The sync is a state machine over the [`Link`] (the network and the data
//! folder), driven by the end of each request ([`Sync::done`]); the tests run
//! it against a fake server.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    files::Volume,
    mem::push,
    text::{concat, copy},
};

use crate::native_cache::{self as cache, Entry, Plan, Remote};
use crate::native_dav::{self as dav, LISTING};
use crate::native_photos::CACHE;

/// The token of the request that lists, and of those that fetch.
pub const LIST_TOKEN: u32 = 1;
pub const GET_TOKEN: u32 = 2;
/// The most a photo is, whatever the cap says (the host's decoder takes 8
/// megapixels; a file this size is more).
pub const PHOTO_MAX: u64 = 24 << 20;

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// An answer came (any status): see `status`.
    Answered,
    /// No network, no name, a failed TLS check, a timeout, a refusal to
    /// connect, power saving or the network turned off.
    Network,
    /// The card is full.
    Full,
    /// Cancelled or replaced.
    Cancelled,
    /// Anything else.
    Other,
}

/// The end of a request, as the sync needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Done {
    pub token: u32,
    pub end: End,
    pub status: u32,
    pub saved: bool,
}

/// The sync's world: the data folder and the network.
pub trait Link {
    /// The plugin's data folder.
    fn volume(&self) -> &dyn Volume;
    /// Starts the listing of the folder at `url`, saved as [`LISTING`], with
    /// the token [`LIST_TOKEN`].
    ///
    /// # Errors
    /// The host's refusal.
    fn propfind(&mut self, url: &str) -> Result<(), Error>;
    /// Starts fetching `url` to the file `save` (at most `max` bytes), with
    /// the token [`GET_TOKEN`].
    ///
    /// # Errors
    /// The host's refusal.
    fn get(&mut self, url: &str, save: &str, max: u64) -> Result<(), Error>;
    /// Cancels the requests of the sync.
    fn cancel(&mut self);
}

/// What the sync is told to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The folder's address, with a `/` at its end.
    pub url: String,
    /// The cache's room in bytes.
    pub cap: u64,
    /// Where the window of photos starts.
    pub window: usize,
}

/// How a sync ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finish {
    /// The text that says it (a translation key).
    pub key: &'static str,
    /// Photos fetched now, photos in the cache, photos on the server.
    pub fetched: usize,
    pub cached: usize,
    pub photos: usize,
    /// Where the next window starts; kept only where the sync went well.
    pub window: usize,
    pub ok: bool,
}

impl Finish {
    /// A sync that ended without a result, saying `key`.
    #[must_use]
    pub fn failed_key(key: &'static str) -> Self {
        Self {
            key,
            fetched: 0,
            cached: 0,
            photos: 0,
            window: 0,
            ok: false,
        }
    }
}

/// Where a sync is.
#[derive(Debug)]
pub enum Step {
    /// Waiting for the end of a request.
    Quiet,
    /// Over.
    Ended(Finish),
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Idle,
    Listing,
    Fetching,
}

/// A sync.
#[derive(Debug, Default)]
pub struct Sync {
    state: State,
    job: Option<Job>,
    remote: Vec<Remote>,
    plan: Plan,
    at: usize,
    /// The cache as it is now: the kept entries and those fetched.
    kept: Vec<Entry>,
    fetched: usize,
    skipped: usize,
}

impl Sync {
    /// Whether a sync is under way.
    #[must_use]
    pub const fn running(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    /// Starts: the folder is listed.
    ///
    /// # Errors
    /// The host's refusal of the request; the sync is idle then.
    pub fn start(&mut self, link: &mut dyn Link, job: Job) -> Result<(), Error> {
        *self = Self::default();
        link.volume().remove(LISTING).ok();
        link.propfind(&job.url)?;
        self.job = Some(job);
        self.state = State::Listing;
        Ok(())
    }

    /// Ends the sync at once (the plugin stops).
    pub fn cancel(&mut self, link: &mut dyn Link) {
        if self.running() {
            link.cancel();
        }
        *self = Self::default();
    }

    /// The end of a request.
    ///
    /// # Errors
    /// [`Error::NoMemory`] where a plan could not be made (the sync is over).
    pub fn done(&mut self, link: &mut dyn Link, done: &Done) -> Result<Step, Error> {
        let step = match (&self.state, done.token) {
            (State::Listing, LIST_TOKEN) => self.listed(link, done),
            (State::Fetching, GET_TOKEN) => self.got(link, done),
            _ => return Ok(Step::Quiet),
        };
        match step {
            Ok(Step::Ended(finish)) => {
                *self = Self::default();
                Ok(Step::Ended(finish))
            }
            Ok(step) => Ok(step),
            Err(error) => {
                link.cancel();
                *self = Self::default();
                Err(error)
            }
        }
    }

    fn refused(done: &Done) -> Option<Finish> {
        match (done.end, done.status) {
            (End::Cancelled, _) => Some(Finish::failed_key("picture-frame.cancelled")),
            (End::Network, _) => Some(Finish::failed_key("picture-frame.no-network")),
            (End::Full, _) => Some(Finish::failed_key("picture-frame.full")),
            (End::Other, _) => Some(Finish::failed_key("picture-frame.failed")),
            (End::Answered, 401 | 403) => Some(Finish::failed_key("picture-frame.denied")),
            (End::Answered, 404 | 410) => Some(Finish::failed_key("picture-frame.not-found")),
            (End::Answered, 200..=299) => None,
            (End::Answered, _) => Some(Finish::failed_key("picture-frame.failed")),
        }
    }

    fn listed(&mut self, link: &mut dyn Link, done: &Done) -> Result<Step, Error> {
        if let Some(finish) = Self::refused(done) {
            return Ok(Step::Ended(finish));
        }
        let Some(job) = self.job.clone() else {
            return Ok(Step::Ended(Finish::failed_key("picture-frame.failed")));
        };
        let volume = link.volume();
        let (remote, _more) = match dav::read(volume, LISTING, &job.url) {
            Ok(read) => read,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Step::Ended(Finish::failed_key("picture-frame.failed"))),
        };
        volume.remove(LISTING).ok();
        let cached = cache::read(volume);
        self.plan = cache::plan(&remote, &cached, job.cap, job.window)?;
        self.remote = remote;
        if self.remote.is_empty() {
            // Nothing to show of the server: what was cached goes.
            self.plan.drop = cached
                .iter()
                .map(|e| copy(&e.name))
                .collect::<Result<_, _>>()?;
            self.plan.keep.clear();
        }
        for name in &self.plan.drop {
            let path = concat(&[CACHE, "/", name])?;
            volume.remove(&path).ok();
        }
        self.kept = core::mem::take(&mut self.plan.keep);
        cache::write(volume, &self.kept)?;
        if self.remote.is_empty() {
            return Ok(Step::Ended(Finish {
                key: "picture-frame.empty-server",
                ok: true,
                ..Finish::failed_key("")
            }));
        }
        self.state = State::Fetching;
        self.next(link, &job)
    }

    /// Asks for the next photo to fetch, or ends.
    fn next(&mut self, link: &mut dyn Link, job: &Job) -> Result<Step, Error> {
        while let Some((index, name)) = self.plan.fetch.get(self.at) {
            let (index, name) = (*index, copy(name)?);
            let Some(photo) = self.remote.get(index) else {
                self.at += 1;
                continue;
            };
            let room = job.cap.saturating_sub(cache::total(&self.kept));
            let path = concat(&[CACHE, "/", &name])?;
            match link.get(&photo.href, &path, room.min(PHOTO_MAX)) {
                Ok(()) => return Ok(Step::Quiet),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // A request the host refuses is left out; the next is tried.
                Err(_) => {
                    self.skipped += 1;
                    self.at += 1;
                }
            }
        }
        Ok(Step::Ended(self.finish()))
    }

    fn finish(&self) -> Finish {
        let key = if self.fetched == 0 && self.skipped > 0 {
            "picture-frame.failed"
        } else {
            "picture-frame.synced"
        };
        Finish {
            key,
            fetched: self.fetched,
            cached: self.kept.len(),
            photos: self.plan.photos,
            window: self.plan.window,
            ok: key == "picture-frame.synced",
        }
    }

    fn got(&mut self, link: &mut dyn Link, done: &Done) -> Result<Step, Error> {
        let Some(job) = self.job.clone() else {
            return Ok(Step::Ended(Finish::failed_key("picture-frame.failed")));
        };
        let (index, name) = match self.plan.fetch.get(self.at) {
            Some((index, name)) => (*index, copy(name)?),
            None => return Ok(Step::Ended(self.finish())),
        };
        match Self::refused(done) {
            // A network that went, a full card or a cancel end the sync;
            // what was fetched so far stays and is listed.
            Some(finish) if matches!(done.end, End::Network | End::Full | End::Cancelled) => {
                return Ok(Step::Ended(Finish {
                    fetched: self.fetched,
                    cached: self.kept.len(),
                    photos: self.plan.photos,
                    ..finish
                }));
            }
            // This photo is not there or too large: left out.
            Some(_) => self.skipped += 1,
            None if done.saved => {
                let path = concat(&[CACHE, "/", &name])?;
                let size = link.volume().size(&path).unwrap_or(0);
                if let Some(photo) = self.remote.get(index) {
                    push(
                        &mut self.kept,
                        Entry {
                            name,
                            size,
                            href: copy(&photo.href)?,
                        },
                    )?;
                    self.fetched += 1;
                    cache::write(link.volume(), &self.kept)?;
                }
            }
            None => self.skipped += 1,
        }
        self.at += 1;
        self.next(link, &job)
    }
}

#[cfg(test)]
#[path = "native_sync_tests.rs"]
mod tests;
