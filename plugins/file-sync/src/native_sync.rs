//! The sync of one pair: the card's folder and the server's, folder by folder.
//! Each folder is listed on the server (`PROPFIND`, saved to a file and read as
//! it streams) and on the card, set against the state of the last sync
//! ([`crate::native_plan`]), and each file that needs it is fetched, sent or
//! deleted, one request at a time. The state is kept after every file, so a
//! cut or a lost network is picked up where it stopped, and every file is
//! written whole or not at all (the host saves a fetched file under another
//! name and renames it).
//!
//! The sync is driven by the end of each request ([`Sync::done`]).
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    dav::{
        listing::{LISTING, read_each},
        url::segment,
    },
    mem::push,
    text::{concat, copy},
};

use crate::native_link::{Entry, Link};
use crate::native_names as names;
use crate::native_plan::{Action, Kept, Mode, Side, decide};
use crate::native_state::{self as state, MOST};

/// The most folders of a pair and how deep they go.
pub const MOST_DIRS: usize = 64;
pub const MAX_DEPTH: usize = 6;
/// The largest file fetched (the host's limit for a saved answer).
pub const DOWNLOAD_MAX: u64 = 64 << 20;
/// Files the trash holds; older ones are removed.
pub const TRASH_MAX: usize = 100;
/// Where the card's files that were deleted on the server go: a folder of
/// the plugin's data folder (`.paperpaw/plugins/file-sync/trash`).
pub const TRASH_DIR: &str = "trash";
/// Deletions in a folder held back when they are this many and more than half
/// of the files the state knows: a server that answers with an empty folder,
/// or a card that lost its files, deletes nothing.
const GUARD_MIN: usize = 5;

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// An answer came (any status).
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
    pub end: End,
    pub status: u32,
    pub saved: bool,
    /// A number made of the `ETag` of the answer; 0 for none.
    pub version: u64,
}

/// What a sync did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub down: u32,
    pub up: u32,
    pub deleted: u32,
    pub conflicts: u32,
    pub errors: u32,
    /// Deletions held back.
    pub held: u32,
}

impl Totals {
    /// These and `other` together.
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            down: self.down + other.down,
            up: self.up + other.up,
            deleted: self.deleted + other.deleted,
            conflicts: self.conflicts + other.conflicts,
            errors: self.errors + other.errors,
            held: self.held + other.held,
        }
    }
}

/// What is synced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The pair's number (names its state files).
    pub pair: usize,
    /// The card's folder, without a `/` at either end.
    pub root: String,
    /// The server's folder, with a `/` at its end.
    pub url: String,
    pub mode: Mode,
    /// The device's name and today's date, for the names of conflicts.
    pub device: String,
    pub date: String,
}

/// How a sync ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finish {
    /// A translation key that says what went wrong first, or `""`.
    pub problem: &'static str,
    /// The network went, the card is full or the sync was cancelled: nothing
    /// more is tried.
    pub stopped: bool,
    pub totals: Totals,
}

/// Where a sync is.
#[derive(Debug)]
pub enum Step {
    /// Waiting for the end of a request.
    Quiet,
    /// Over.
    Ended(Finish),
}

/// One file of the folder being synced.
#[derive(Debug)]
struct Item {
    name: String,
    local: Option<Side>,
    remote: Option<Side>,
}

/// What the request under way is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    Nothing,
    Listing,
    Folder,
    Get(usize),
    Put(usize),
    Delete(usize),
}

/// The folder being synced.
#[derive(Debug, Default)]
struct Dir {
    rel: String,
    url: String,
    items: Vec<Item>,
    at: usize,
    kept: Vec<Kept>,
    /// The server has no such folder yet.
    remote_missing: bool,
    /// The card's folder is known to be there.
    made: bool,
    hold: bool,
}

/// A sync.
#[derive(Debug)]
pub struct Sync {
    job: Option<Job>,
    wait: Wait,
    dirs: Vec<String>,
    dir: Dir,
    totals: Totals,
    problem: &'static str,
}

impl Default for Sync {
    fn default() -> Self {
        Self {
            job: None,
            wait: Wait::Nothing,
            dirs: Vec::new(),
            dir: Dir::default(),
            totals: Totals::default(),
            problem: "",
        }
    }
}

impl Sync {
    /// Whether a sync is under way.
    #[must_use]
    pub const fn running(&self) -> bool {
        self.job.is_some()
    }

    /// The file being synced and how far the folder is: `(name, done, all)`.
    #[must_use]
    pub fn progress(&self) -> Option<(&str, usize, usize)> {
        self.job.as_ref()?;
        let name = self.dir.items.get(self.dir.at)?.name.as_str();
        Some((name, self.dir.at, self.dir.items.len()))
    }

    /// What was done so far.
    #[must_use]
    pub const fn totals(&self) -> Totals {
        self.totals
    }

    /// Starts: the server's folder is listed.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the sync is idle then.
    pub fn start(&mut self, link: &mut dyn Link, job: Job) -> Result<Step, Error> {
        *self = Self::default();
        self.dirs
            .try_reserve(MOST_DIRS)
            .map_err(|_| Error::NoMemory)?;
        self.dirs.push(String::new());
        self.job = Some(job);
        self.guarded(link, Self::next_dir)
    }

    /// Ends the sync at once (the plugin stops).
    pub fn cancel(&mut self, link: &mut dyn Link) {
        if self.running() {
            link.cancel();
        }
        *self = Self::default();
    }

    /// The end of the request.
    ///
    /// # Errors
    /// [`Error::NoMemory`] (the sync is over).
    pub fn done(&mut self, link: &mut dyn Link, done: &Done) -> Result<Step, Error> {
        if !self.running() {
            return Ok(Step::Quiet);
        }
        let wait = core::mem::replace(&mut self.wait, Wait::Nothing);
        if wait == Wait::Nothing {
            return Ok(Step::Quiet);
        }
        if let Some(finish) = self.stopped(done) {
            *self = Self::default();
            return Ok(Step::Ended(finish));
        }
        self.guarded(link, |this, link| this.ended(link, wait, done))
    }

    /// A step; a failure of memory ends the sync.
    fn guarded(
        &mut self,
        link: &mut dyn Link,
        step: impl FnOnce(&mut Self, &mut dyn Link) -> Result<Step, Error>,
    ) -> Result<Step, Error> {
        match step(self, link) {
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

    /// A network that went, a full card or a cancel end the sync; what was
    /// done stays and is picked up by the next.
    fn stopped(&self, done: &Done) -> Option<Finish> {
        let problem = match done.end {
            End::Network => "file-sync.no-network",
            End::Full => "file-sync.full",
            End::Cancelled => "file-sync.cancelled",
            End::Answered | End::Other => return None,
        };
        Some(Finish {
            problem,
            stopped: true,
            totals: self.totals,
        })
    }

    fn job(&self) -> Result<&Job, Error> {
        self.job.as_ref().ok_or(Error::State)
    }

    fn fail(&mut self, key: &'static str) {
        self.totals.errors += 1;
        if self.problem.is_empty() {
            self.problem = key;
        }
    }

    fn finish(&self) -> Finish {
        Finish {
            problem: self.problem,
            stopped: false,
            totals: self.totals,
        }
    }

    // --- folders -----------------------------------------------------------

    /// Lists the next folder on the server, or ends.
    fn next_dir(&mut self, link: &mut dyn Link) -> Result<Step, Error> {
        while let Some(rel) = self.dirs.pop() {
            let url = self.dir_url(&rel)?;
            self.dir = Dir {
                rel,
                url,
                ..Dir::default()
            };
            link.data().remove(LISTING).ok();
            match link.propfind(&self.dir.url) {
                Ok(()) => {
                    self.wait = Wait::Listing;
                    return Ok(Step::Quiet);
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => self.fail("file-sync.failed"),
            }
        }
        self.dir = Dir::default();
        Ok(Step::Ended(self.finish()))
    }

    fn dir_url(&self, rel: &str) -> Result<String, Error> {
        let mut url = copy(&self.job()?.url)?;
        for part in rel.split('/').filter(|p| !p.is_empty()) {
            url.push_str(&segment(part)?);
            url.push('/');
        }
        Ok(url)
    }

    fn ended(&mut self, link: &mut dyn Link, wait: Wait, done: &Done) -> Result<Step, Error> {
        match wait {
            Wait::Listing => self.listed(link, done),
            Wait::Folder => {
                if matches!(done.status, 200..=299 | 405) {
                    self.dir.remote_missing = false;
                    self.run(link)
                } else {
                    self.skip_dir("file-sync.failed");
                    self.next_dir(link)
                }
            }
            Wait::Get(at) => {
                self.got(link, at, done)?;
                self.dir.at = at + 1;
                self.run(link)
            }
            Wait::Put(at) => {
                self.put_done(link, at, done)?;
                self.dir.at = at + 1;
                self.run(link)
            }
            Wait::Delete(at) => {
                self.deleted(link, at, done)?;
                self.dir.at = at + 1;
                self.run(link)
            }
            Wait::Nothing => Ok(Step::Quiet),
        }
    }

    /// A folder that cannot be synced: said, and its files are left as they are.
    fn skip_dir(&mut self, key: &'static str) {
        self.fail(key);
        self.dir.items.clear();
        self.dir.at = 0;
    }

    fn listed(&mut self, link: &mut dyn Link, done: &Done) -> Result<Step, Error> {
        let two = self.job()?.mode == Mode::TwoWay;
        match (done.end, done.status) {
            (End::Answered, 200..=299) => {}
            (End::Answered, 404 | 410) => {
                // The top folder gone that was synced before is no folder to
                // empty the card by: only a pair never synced makes it.
                let fresh = state::read(link.data(), self.job()?.pair, &self.dir.rel)?.is_empty();
                if !(two && (fresh || !self.dir.rel.is_empty())) {
                    self.skip_dir("file-sync.not-found");
                    return self.next_dir(link);
                }
                self.dir.remote_missing = true;
            }
            (End::Answered, 401 | 403) => {
                self.skip_dir("file-sync.denied");
                self.dirs.clear();
                return self.next_dir(link);
            }
            _ => {
                self.skip_dir("file-sync.failed");
                return self.next_dir(link);
            }
        }
        if !self.build(link)? {
            return self.next_dir(link);
        }
        self.run(link)
    }

    /// Sets the listings and the state side by side. `false`: the folder
    /// cannot be synced (said).
    fn build(&mut self, link: &mut dyn Link) -> Result<bool, Error> {
        let mut remote: Vec<(String, bool, Side)> = Vec::new();
        if !self.dir.remote_missing {
            let more = read_each(link.data(), LISTING, &self.dir.url, &mut |item| {
                if names::left_out(&item.name) {
                    return Ok(());
                }
                if remote.len() > MOST {
                    return Err(Error::NoSpace);
                }
                push(
                    &mut remote,
                    (
                        item.name,
                        item.folder,
                        Side {
                            size: item.size,
                            stamp: item.version,
                        },
                    ),
                )
            });
            link.data().remove(LISTING).ok();
            match more {
                Ok(false) => {}
                Ok(true) | Err(Error::NoSpace) => return Ok(self.too_many()),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {
                    self.skip_dir("file-sync.failed");
                    return Ok(false);
                }
            }
        }
        let mut local: Vec<Entry> = Vec::new();
        let root = copy(&self.job()?.root)?;
        let folder = if self.dir.rel.is_empty() {
            root
        } else {
            concat(&[&root, "/", &self.dir.rel])?
        };
        match link.list(&folder, &mut local, MOST + 1) {
            Ok(()) => {}
            Err(Error::NotFound) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                self.skip_dir("file-sync.failed");
                return Ok(false);
            }
        }
        local.retain(|e| !names::left_out(&e.name));
        if local.len() > MOST {
            return Ok(self.too_many());
        }
        self.dir.made = !local.is_empty();
        self.dir.kept = state::read(link.data(), self.job()?.pair, &self.dir.rel)?;
        self.merge(remote, local)?;
        self.dir.hold = self.holds()?;
        Ok(true)
    }

    fn too_many(&mut self) -> bool {
        self.skip_dir("file-sync.too-many");
        false
    }

    /// The files of both sides as one list by name; the folders of either go
    /// to the folders still to do.
    fn merge(
        &mut self,
        mut remote: Vec<(String, bool, Side)>,
        mut local: Vec<Entry>,
    ) -> Result<(), Error> {
        paperpaw_sdk::sort::sort_by(&mut remote, |a, b| a.0.cmp(&b.0));
        paperpaw_sdk::sort::sort_by(&mut local, |a, b| a.name.cmp(&b.name));
        let (mut r, mut l) = (remote.into_iter().peekable(), local.into_iter().peekable());
        loop {
            let order = match (r.peek(), l.peek()) {
                (None, None) => break,
                (Some(_), None) => core::cmp::Ordering::Less,
                (None, Some(_)) => core::cmp::Ordering::Greater,
                (Some(a), Some(b)) => a.0.cmp(&b.name),
            };
            let (name, rem, loc) = match order {
                core::cmp::Ordering::Less => {
                    let (n, f, s) = r.next().ok_or(Error::State)?;
                    (n, Some((f, s)), None)
                }
                core::cmp::Ordering::Greater => {
                    let e = l.next().ok_or(Error::State)?;
                    (e.name, None, Some((e.folder, e.side)))
                }
                core::cmp::Ordering::Equal => {
                    let (n, f, s) = r.next().ok_or(Error::State)?;
                    let e = l.next().ok_or(Error::State)?;
                    (n, Some((f, s)), Some((e.folder, e.side)))
                }
            };
            let folder = rem.is_some_and(|(f, _)| f) || loc.is_some_and(|(f, _)| f);
            let file = rem.is_some_and(|(f, _)| !f) || loc.is_some_and(|(f, _)| !f);
            if folder && file {
                // A file on one side, a folder on the other: neither is touched.
                self.fail("file-sync.failed");
            } else if folder {
                self.add_dir(&name)?;
            } else {
                push(
                    &mut self.dir.items,
                    Item {
                        name,
                        local: loc.map(|(_, s)| s),
                        remote: rem.map(|(_, s)| s),
                    },
                )?;
            }
        }
        Ok(())
    }

    fn add_dir(&mut self, name: &str) -> Result<(), Error> {
        let child = names::child(&self.dir.rel, name)?;
        let depth = child.split('/').count();
        if depth > MAX_DEPTH || self.dirs.len() >= MOST_DIRS {
            self.fail("file-sync.too-many");
            return Ok(());
        }
        self.dirs.push(child);
        Ok(())
    }

    /// Whether the deletions of the folder are held back.
    fn holds(&self) -> Result<bool, Error> {
        let mode = self.job()?.mode;
        let deletes = self
            .dir
            .items
            .iter()
            .filter(|i| {
                matches!(
                    decide(mode, i.local, i.remote, self.kept_of(&i.name)),
                    Action::DeleteLocal | Action::DeleteRemote
                )
            })
            .count();
        Ok(deletes >= GUARD_MIN && deletes * 2 > self.dir.kept.len())
    }

    fn kept_of(&self, name: &str) -> Option<&Kept> {
        let at = self
            .dir
            .kept
            .binary_search_by(|k| k.name.as_str().cmp(name))
            .ok()?;
        self.dir.kept.get(at)
    }

    // --- files -------------------------------------------------------------

    /// Goes on with the files of the folder until a request is under way.
    fn run(&mut self, link: &mut dyn Link) -> Result<Step, Error> {
        while self.dir.at < self.dir.items.len() {
            if self.item(link, self.dir.at)? {
                return Ok(Step::Quiet);
            }
            self.dir.at += 1;
        }
        self.next_dir(link)
    }

    /// Does what the file `at` needs. `true`: a request is under way.
    fn item(&mut self, link: &mut dyn Link, at: usize) -> Result<bool, Error> {
        let mode = self.job()?.mode;
        let Some(item) = self.dir.items.get(at) else {
            return Ok(false);
        };
        let (name, local, remote) = (copy(&item.name)?, item.local, item.remote);
        let mut action = decide(mode, local, remote, self.kept_of(&name));
        if self.dir.hold && matches!(action, Action::DeleteLocal | Action::DeleteRemote) {
            // Held back: what was to be deleted stays, and goes where it is missing.
            self.totals.held += 1;
            action = match (action, mode) {
                (Action::DeleteRemote, _) => Action::Download,
                (_, Mode::TwoWay) => Action::Upload,
                _ => Action::Forget,
            };
        }
        match action {
            Action::Nothing => self.refresh(link, &name, local, remote),
            Action::Adopt => match (local, remote) {
                (Some(l), Some(r)) => self.keep(link, &name, l, r),
                _ => Ok(false),
            },
            Action::Forget => self.forget(link, &name),
            Action::DeleteLocal => self.delete_local(link, &name),
            Action::DeleteRemote => self.delete_remote(link, at, &name),
            Action::Upload => self.upload(link, at, &name),
            Action::Download => self.download(link, at, &name, remote),
            Action::Conflict => self.conflict(link, at, &name, local, remote),
        }
        .or_else(|error| match error {
            Error::NoMemory => Err(Error::NoMemory),
            _ => {
                self.fail("file-sync.failed");
                Ok(false)
            }
        })
    }

    fn local_path(&self, name: &str) -> Result<String, Error> {
        names::path(&self.job()?.root, &self.dir.rel, name)
    }

    fn remote_url(&self, name: &str) -> Result<String, Error> {
        concat(&[&self.dir.url, &segment(name)?])
    }

    /// The state of the folder kept, after a change.
    fn persist(&mut self, link: &mut dyn Link) -> Result<(), Error> {
        let pair = self.job()?.pair;
        match state::write(link.data(), pair, &self.dir.rel, &self.dir.kept) {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => {
                // A state that cannot be kept only makes the next sync ask more.
                self.fail("file-sync.failed");
                Ok(())
            }
        }
    }

    /// `name` is now as `local` and `remote` say.
    fn keep(
        &mut self,
        link: &mut dyn Link,
        name: &str,
        local: Side,
        remote: Side,
    ) -> Result<bool, Error> {
        let kept = Kept {
            name: copy(name)?,
            local,
            remote,
        };
        match self
            .dir
            .kept
            .binary_search_by(|k| k.name.as_str().cmp(name))
        {
            Ok(at) => {
                if let Some(slot) = self.dir.kept.get_mut(at) {
                    *slot = kept;
                }
            }
            Err(at) => {
                self.dir.kept.try_reserve(1).map_err(|_| Error::NoMemory)?;
                self.dir.kept.insert(at, kept);
            }
        }
        self.persist(link)?;
        Ok(false)
    }

    fn forget(&mut self, link: &mut dyn Link, name: &str) -> Result<bool, Error> {
        if let Ok(at) = self
            .dir
            .kept
            .binary_search_by(|k| k.name.as_str().cmp(name))
        {
            self.dir.kept.remove(at);
            self.persist(link)?;
        }
        Ok(false)
    }

    /// An `ETag` that the state did not know is taken in.
    fn refresh(
        &mut self,
        link: &mut dyn Link,
        name: &str,
        local: Option<Side>,
        remote: Option<Side>,
    ) -> Result<bool, Error> {
        if let (Some(l), Some(r), Some(k)) = (local, remote, self.kept_of(name))
            && k.remote.stamp == 0
            && r.stamp != 0
        {
            return self.keep(link, name, l, r);
        }
        Ok(false)
    }

    fn download(
        &mut self,
        link: &mut dyn Link,
        at: usize,
        name: &str,
        remote: Option<Side>,
    ) -> Result<bool, Error> {
        let size = remote.map_or(0, |r| r.size);
        if size > DOWNLOAD_MAX {
            self.fail("file-sync.too-large");
            return Ok(false);
        }
        if !self.dir.made {
            let folder = names::path(&self.job()?.root, &self.dir.rel, "")?;
            link.make_dir(folder.trim_end_matches('/'))?;
            self.dir.made = true;
        }
        let (url, path) = (self.remote_url(name)?, self.local_path(name)?);
        link.get(&url, &path, DOWNLOAD_MAX)?;
        self.wait = Wait::Get(at);
        Ok(true)
    }

    fn got(&mut self, link: &mut dyn Link, at: usize, done: &Done) -> Result<(), Error> {
        let Some(item) = self.dir.items.get(at) else {
            return Ok(());
        };
        let (name, remote) = (copy(&item.name)?, item.remote);
        if !(done.end == End::Answered && (200..300).contains(&done.status) && done.saved) {
            self.fail(status_key(done));
            return Ok(());
        }
        let path = self.local_path(&name)?;
        match (link.stat(&path), remote) {
            (Ok(Some(l)), Some(r)) => {
                self.totals.down += 1;
                self.keep(link, &name, l, r)?;
            }
            _ => self.fail("file-sync.failed"),
        }
        Ok(())
    }

    fn upload(&mut self, link: &mut dyn Link, at: usize, name: &str) -> Result<bool, Error> {
        let (url, path) = (self.remote_url(name)?, self.local_path(name)?);
        if self.dir.remote_missing {
            // The folder is made on the server first, then the file goes.
            link.mkcol(&self.dir.url)?;
            self.wait = Wait::Folder;
            return Ok(true);
        }
        link.put(&url, &path)?;
        self.wait = Wait::Put(at);
        Ok(true)
    }

    fn put_done(&mut self, link: &mut dyn Link, at: usize, done: &Done) -> Result<(), Error> {
        let Some(item) = self.dir.items.get(at) else {
            return Ok(());
        };
        let (name, local) = (copy(&item.name)?, item.local);
        let ok = done.end == End::Answered && matches!(done.status, 200 | 201 | 204);
        let Some(l) = local.filter(|_| ok) else {
            self.fail(status_key(done));
            return Ok(());
        };
        self.totals.up += 1;
        let remote = Side {
            size: l.size,
            stamp: done.version,
        };
        self.keep(link, &name, l, remote)?;
        Ok(())
    }

    fn delete_remote(&mut self, link: &mut dyn Link, at: usize, name: &str) -> Result<bool, Error> {
        let url = self.remote_url(name)?;
        link.delete(&url)?;
        self.wait = Wait::Delete(at);
        Ok(true)
    }

    fn deleted(&mut self, link: &mut dyn Link, at: usize, done: &Done) -> Result<(), Error> {
        let Some(item) = self.dir.items.get(at) else {
            return Ok(());
        };
        let name = copy(&item.name)?;
        if done.end == End::Answered && matches!(done.status, 200..=299 | 404) {
            self.totals.deleted += 1;
            self.forget(link, &name)?;
        } else {
            self.fail(status_key(done));
        }
        Ok(())
    }

    /// The card's copy goes to the trash; the trash keeps the last
    /// [`TRASH_MAX`].
    fn delete_local(&mut self, link: &mut dyn Link, name: &str) -> Result<bool, Error> {
        let path = self.local_path(name)?;
        self.trash(link, &path, name)?;
        self.totals.deleted += 1;
        self.forget(link, name)
    }

    fn trash(&mut self, link: &mut dyn Link, path: &str, name: &str) -> Result<(), Error> {
        let mut held = state::trash(link.data())?;
        let mut target = String::new();
        for n in 1..=99u32 {
            let candidate = paperpaw_ui_kit::text::format(format_args!(
                "{TRASH_DIR}/{}-{n}-{name}",
                self.job()?.date
            ))?;
            if link.data().size(&candidate).is_err() {
                target = candidate;
                break;
            }
        }
        if target.is_empty() {
            return Err(Error::State);
        }
        link.trash(path, &target)?;
        push(&mut held, target)?;
        while held.len() > TRASH_MAX {
            let oldest = held.remove(0);
            link.data().remove(&oldest).ok();
        }
        state::write_trash(link.data(), &held)
    }

    /// Both changed: the card's copy is kept under another name (and goes to
    /// the server on its own turn), then the server's file comes.
    fn conflict(
        &mut self,
        link: &mut dyn Link,
        at: usize,
        name: &str,
        local: Option<Side>,
        remote: Option<Side>,
    ) -> Result<bool, Error> {
        let (device, date) = (copy(&self.job()?.device)?, copy(&self.job()?.date)?);
        let from = self.local_path(name)?;
        let mut copy_name = String::new();
        for n in 1..=9u32 {
            let candidate = names::conflict(name, &device, &date, n)?;
            if link.stat(&self.local_path(&candidate)?)?.is_none()
                && !self.dir.items.iter().any(|i| i.name == candidate)
            {
                copy_name = candidate;
                break;
            }
        }
        if copy_name.is_empty() {
            return Err(Error::State);
        }
        link.rename(&from, &self.local_path(&copy_name)?)?;
        self.totals.conflicts += 1;
        self.forget(link, name)?;
        if self.job()?.mode == Mode::TwoWay
            && let Some(side) = local
        {
            push(
                &mut self.dir.items,
                Item {
                    name: copy_name,
                    local: Some(side),
                    remote: None,
                },
            )?;
        }
        self.download(link, at, name, remote)
    }
}

/// The text that says why a request did not do what it was for.
fn status_key(done: &Done) -> &'static str {
    match (done.end, done.status) {
        (End::Answered, 401 | 403) => "file-sync.denied",
        (End::Answered, 404 | 410) => "file-sync.not-found",
        _ => "file-sync.failed",
    }
}

#[cfg(test)]
#[path = "native_sync_tests.rs"]
mod tests;
