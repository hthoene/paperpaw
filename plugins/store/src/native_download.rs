//! Downloading the files of a plugin (slice 8, host-free): a job kept on the
//! storage, the files one after the other over `core.net`, each saved to the
//! staging folder `stage/<plugin>/` in the data folder as it comes (nothing is
//! held in memory), each checked against the size and the digest the library
//! lists before the next is asked for. The rules are those of the interface-6
//! `paperpaw_sdk::install::Installer` (`start`, `received`, `cancel`), made
//! fallible: memory refused is `NoMemory`, a line of the job is read in pieces
//! of 256 bytes, a file is hashed in pieces of 512, and the job's lines are
//! written one at a time (a package of 64 files is never one text in memory).
//!
//! The plugin owns the network: it implements [`Wire`] and tells the end of
//! each request as a [`Done`]; the install (`native_install`) takes over where
//! the last file is here ([`Download::take_ready`]).
use alloc::string::String;
use paperpaw_sdk::Storage;
use paperpaw_sdk::install::{MAX_FILES, is_plugin_name, is_safe_path};
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::text::{concat, copy, format};

use crate::native_item::{Digest, File};
use crate::native_list::Page;

/// The plugin's own number of a file's request, in its `http_done` event.
pub const FILE_REQUEST: u32 = 30;
/// The folder (in the data folder) the files of a plugin are downloaded into.
pub const STAGE_DIR: &str = "stage";
/// The job: its first line says which plugin, then a line a file.
pub const JOB_FILE: &str = "install.txt";
/// How many files are here and checked.
pub const DONE_FILE: &str = "install.done";
/// The longest line of a job read, in bytes.
const MAX_LINE: usize = 2048;
/// The longest address of a file.
const MAX_URL: usize = 1024;
/// The longest path of a file in a plugin's folder.
const MAX_PATH: usize = 100;

/// What the download needs of the world: the data folder and the network. The
/// host's in the plugin; a stand-in in tests.
pub trait Wire {
    /// The plugin's data folder.
    fn storage(&self) -> &dyn Storage;
    /// Asks for `url`, its answer saved to `file` in the data folder (at most
    /// `max_bytes`), the end told as [`Done`] with `token`.
    ///
    /// # Errors
    /// `Unsupported` or `Denied` without the network; what the host says.
    fn get(
        &mut self,
        url: &str,
        file: &str,
        token: u32,
        max_bytes: u64,
    ) -> Result<RequestId, Error>;
    /// Ends the requests of `token`; one that cannot be ended ends by itself and
    /// is ignored.
    fn cancel(&mut self, token: u32);
}

/// A request that ended, as far as the download reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Done {
    /// The request.
    pub id: RequestId,
    /// The plugin's own number of it.
    pub token: u32,
    /// Why it ended.
    pub reason: Reason,
    /// A 2xx answer came and its body is in the file.
    pub saved: bool,
}

impl Done {
    /// The end of the request `id`.
    #[must_use]
    pub const fn new(id: RequestId, token: u32, reason: Reason, saved: bool) -> Self {
        Self {
            id,
            token,
            reason,
            saved,
        }
    }
}

/// An installation going on: which plugin, and how far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Progress {
    pub plugin: String,
    pub name: String,
    /// Files downloaded and checked so far, and in all.
    pub done: usize,
    pub total: usize,
}

/// Why a download ended without the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// A file did not come: no network, the connection went, a refused or
    /// timed out request.
    Network,
    /// A file came damaged: another size or digest than the library lists, or
    /// no 2xx answer.
    Damaged,
    /// The card refused the file or the job.
    Card,
}

/// Where a plugin's installation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Nothing is going on.
    Idle,
    /// Files are coming: `done` of `total` are here and checked.
    Downloading { done: usize, total: usize },
    /// All files are here and checked; [`Download::take_ready`] hands them to
    /// the install.
    Ready,
    /// The host is installing (set by `native_install`).
    Installing,
    /// The plugin is installed (set by `native_install`).
    Installed,
    /// It did not come; what was downloaded is gone and the version installed
    /// before stays.
    Failed(Failure),
}

/// What a start came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Started {
    /// The first request is sent.
    Downloading,
    /// The page does not make a package: a plugin name that does not fit, no
    /// `plugin.ini`, an unsafe or doubled path, a digest that is none, over 64
    /// files.
    Invalid,
    /// The job could not be kept on the card.
    Card,
    /// The first request was refused (no network service).
    NoNetwork,
}

/// The plugin whose files are all here and checked, for the install.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Staged {
    pub plugin: String,
    pub name: String,
    /// The folder of the files, in the data folder (`stage/<plugin>`).
    pub folder: String,
}

/// The downloads of one plugin at a time.
#[derive(Debug)]
pub struct Download {
    /// The request of the file being fetched.
    current: Option<RequestId>,
    step: Step,
}

impl Default for Download {
    fn default() -> Self {
        Self::new()
    }
}

impl Download {
    /// Nothing going on.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: None,
            step: Step::Idle,
        }
    }

    /// Where it is.
    #[must_use]
    pub const fn step(&self) -> Step {
        self.step
    }

    /// Sets the step the install reached (`Installing`, `Installed`), or `Idle`.
    pub const fn set_step(&mut self, step: Step) {
        self.step = step;
    }

    /// Starts downloading the files of `page`: a download going on is stopped
    /// first and what it fetched is forgotten.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn start(&mut self, wire: &mut dyn Wire, page: &Page) -> Result<Started, Error> {
        self.cancel(wire);
        if !is_package(&page.entry.id, &page.files) {
            return Ok(Started::Invalid);
        }
        let storage = wire.storage();
        let plugin = page.entry.id.as_str();
        let name = one_line(&page.entry.name)?;
        let total = page.files.len();
        let header = format(format_args!("{plugin}\t{name}\t{total}\n"))?;
        let _ = storage.create_dir(STAGE_DIR);
        let staging = staging(plugin)?;
        let _ = storage.remove_dir_all(&staging);
        if storage.create_dir(&staging).is_err()
            || storage.write(JOB_FILE, header.as_bytes()).is_err()
        {
            let _ = storage.remove_dir_all(&staging);
            return Ok(Started::Card);
        }
        for file in &page.files {
            let line = format(format_args!(
                "{}\t{}\t{}\t{}\n",
                file.path,
                file.size,
                digest_text(&file.digest)?,
                file.url
            ))?;
            if storage.append(JOB_FILE, line.as_bytes()).is_err() {
                forget(storage, &staging);
                return Ok(Started::Card);
            }
        }
        let _ = storage.write(DONE_FILE, b"0");
        self.step = Step::Downloading { done: 0, total };
        match self.ask(wire, 0) {
            Ok(Asked::Sent) => Ok(Started::Downloading),
            Ok(Asked::Card) => {
                self.cancel(wire);
                Ok(Started::Card)
            }
            Ok(Asked::Refused) => {
                self.cancel(wire);
                Ok(Started::NoNetwork)
            }
            Err(error) => {
                self.cancel(wire);
                Err(error)
            }
        }
    }

    /// The end of a request. An answer to a request that is not the one being
    /// waited for (one of a download stopped before) changes nothing.
    ///
    /// # Errors
    /// `NoMemory`; the download is then stopped.
    pub fn ended(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Step, Error> {
        if done.token != FILE_REQUEST || self.current != Some(done.id) {
            return Ok(self.step);
        }
        self.current = None;
        let result = self.checked(wire, done);
        if result.is_err() {
            self.cancel(wire);
        }
        result
    }

    fn checked(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Step, Error> {
        let storage = wire.storage();
        let Some(job) = read_job(storage)? else {
            self.step = Step::Idle;
            return Ok(self.step);
        };
        let count = counted(storage);
        let Some(file) = read_file_line(storage, count)? else {
            self.step = Step::Idle;
            return Ok(self.step);
        };
        let saved = concat(&[STAGE_DIR, "/", &job.plugin, "/", &file.path])?;
        let whole = done.reason == Reason::Ok
            && done.saved
            && storage.size(&saved).ok() == Some(file.size)
            && holds(storage, &saved, &file.digest);
        if !whole {
            let failure = match done.reason {
                Reason::Ok | Reason::TooLarge => Failure::Damaged,
                Reason::Io | Reason::Full => Failure::Card,
                _ => Failure::Network,
            };
            self.cancel(wire);
            self.step = Step::Failed(failure);
            return Ok(self.step);
        }
        let next = count + 1;
        let text = format(format_args!("{next}"))?;
        if storage.write(DONE_FILE, text.as_bytes()).is_err() {
            self.cancel(wire);
            self.step = Step::Failed(Failure::Card);
            return Ok(self.step);
        }
        if next < job.total {
            self.step = Step::Downloading {
                done: next,
                total: job.total,
            };
            match self.ask(wire, next)? {
                Asked::Sent => {}
                Asked::Card => {
                    self.cancel(wire);
                    self.step = Step::Failed(Failure::Card);
                }
                Asked::Refused => {
                    self.cancel(wire);
                    self.step = Step::Failed(Failure::Network);
                }
            }
        } else {
            self.step = Step::Ready;
        }
        Ok(self.step)
    }

    /// Asks for the job's file `n`, its folders made first.
    fn ask(&mut self, wire: &mut dyn Wire, n: usize) -> Result<Asked, Error> {
        let storage = wire.storage();
        let (Some(job), Some(file)) = (read_job(storage)?, read_file_line(storage, n)?) else {
            return Ok(Asked::Card);
        };
        let mut dir = staging(&job.plugin)?;
        let parts_end = file.path.rfind('/').unwrap_or(0);
        if let Some(folders) = file.path.get(..parts_end) {
            for part in folders.split('/').filter(|part| !part.is_empty()) {
                dir = concat(&[&dir, "/", part])?;
                let _ = storage.create_dir(&dir);
            }
        }
        let save_to = concat(&[STAGE_DIR, "/", &job.plugin, "/", &file.path])?;
        match wire.get(&file.url, &save_to, FILE_REQUEST, file.size.max(1)) {
            Ok(id) => {
                self.current = Some(id);
                Ok(Asked::Sent)
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(Asked::Refused),
        }
    }

    /// Stops the download going on and forgets what it fetched; the answers of
    /// its requests are passed over.
    pub fn cancel(&mut self, wire: &mut dyn Wire) {
        if self.current.take().is_some() {
            wire.cancel(FILE_REQUEST);
        }
        let storage = wire.storage();
        match read_job(storage) {
            Ok(Some(job)) => {
                if let Ok(staging) = staging(&job.plugin) {
                    let _ = storage.remove_dir_all(&staging);
                }
            }
            // The job cannot be read for want of memory: its plugin is not
            // known, so every staged plugin goes rather than one stays behind.
            Err(_) => {
                let _ = storage.remove_dir_all(STAGE_DIR);
            }
            Ok(None) => {}
        }
        let _ = storage.remove(JOB_FILE);
        let _ = storage.remove(DONE_FILE);
        if !matches!(self.step, Step::Failed(_)) {
            self.step = Step::Idle;
        }
    }

    /// The installation going on, downloading or ready (read from the storage,
    /// as screens ask again when an answer came).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn progress(storage: &dyn Storage) -> Result<Option<Progress>, Error> {
        let Some(job) = read_job(storage)? else {
            return Ok(None);
        };
        Ok(Some(Progress {
            done: counted(storage).min(job.total),
            plugin: job.plugin,
            name: job.name,
            total: job.total,
        }))
    }

    /// The plugin whose files are all here, handed to the install: the job is
    /// forgotten, the files stay in their folder until the install ended.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn take_ready(&mut self, storage: &dyn Storage) -> Result<Option<Staged>, Error> {
        if self.step != Step::Ready {
            return Ok(None);
        }
        let Some(job) = read_job(storage)? else {
            return Ok(None);
        };
        if counted(storage) < job.total {
            return Ok(None);
        }
        let folder = staging(&job.plugin)?;
        let _ = storage.remove(JOB_FILE);
        let _ = storage.remove(DONE_FILE);
        self.step = Step::Installing;
        Ok(Some(Staged {
            plugin: job.plugin,
            name: job.name,
            folder,
        }))
    }
}

/// What asking for a file came to.
enum Asked {
    Sent,
    Card,
    Refused,
}

/// The folder a plugin is downloaded into.
///
/// # Errors
/// `NoMemory`.
pub fn staging(plugin: &str) -> Result<String, Error> {
    concat(&[STAGE_DIR, "/", plugin])
}

/// Forgets a job and its files.
fn forget(storage: &dyn Storage, staging: &str) {
    let _ = storage.remove_dir_all(staging);
    let _ = storage.remove(JOB_FILE);
    let _ = storage.remove(DONE_FILE);
}

/// Whether `files` make a package the install takes: a plugin name that fits,
/// at most 64 files, each a safe path, a digest and an address, none twice, a
/// `plugin.ini` among them.
#[must_use]
pub fn is_package(plugin: &str, files: &[File]) -> bool {
    is_plugin_name(plugin)
        && !files.is_empty()
        && files.len() <= MAX_FILES
        && files.iter().all(is_file)
        && files.iter().any(|file| file.path == "plugin.ini")
        && files
            .iter()
            .enumerate()
            .all(|(i, file)| files.iter().take(i).all(|other| other.path != file.path))
}

fn is_file(file: &File) -> bool {
    is_safe_path(&file.path)
        && file.path.len() <= MAX_PATH
        && !file.url.is_empty()
        && file.url.len() <= MAX_URL
        && !file.url.contains(['\t', '\n', '\r'])
        && digest_is_valid(&file.digest)
}

fn digest_is_valid(digest: &Digest) -> bool {
    let (hex, len) = match digest {
        Digest::Sha1(hex) => (hex, 40),
        Digest::Sha256(hex) => (hex, 64),
    };
    hex.len() == len && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A digest as kept in a job: `sha1:…` or `sha256:…`, lower case.
fn digest_text(digest: &Digest) -> Result<String, Error> {
    let (kind, hex) = match digest {
        Digest::Sha1(hex) => ("sha1", hex),
        Digest::Sha256(hex) => ("sha256", hex),
    };
    let mut out = format(format_args!("{kind}:"))?;
    out.try_reserve(hex.len()).map_err(|_| Error::NoMemory)?;
    out.extend(hex.chars().map(|c| c.to_ascii_lowercase()));
    Ok(out)
}

fn digest_parse(text: &str) -> Result<Option<Digest>, Error> {
    Ok(match text.split_once(':') {
        Some(("sha1", hex)) => Some(Digest::Sha1(copy(hex)?)),
        Some(("sha256", hex)) => Some(Digest::Sha256(copy(hex)?)),
        _ => None,
    })
}

/// Whether the file at `path` has `digest`, read in pieces of 512 bytes.
fn holds(storage: &dyn Storage, path: &str, digest: &Digest) -> bool {
    let mut sha1 = paperpaw_sdk::digest::sha1::Sha1::new();
    let mut sha256 = paperpaw_sdk::digest::sha256::Sha256::new();
    let mut buf = [0u8; 512];
    let mut at = 0u64;
    loop {
        let Ok(n) = storage.read_at(path, at, &mut buf) else {
            return false;
        };
        let Some(piece) = buf.get(..n) else {
            return false;
        };
        if n == 0 {
            break;
        }
        match digest {
            Digest::Sha1(_) => sha1.update(piece),
            Digest::Sha256(_) => sha256.update(piece),
        }
        at += n as u64;
    }
    match digest {
        Digest::Sha1(hex) => sha1.hex().eq_ignore_ascii_case(hex),
        Digest::Sha256(hex) => sha256.hex().eq_ignore_ascii_case(hex),
    }
}

/// The first line of the job.
struct Job {
    plugin: String,
    name: String,
    total: usize,
}

/// A name on one line, without tabs (for the job's first line).
fn one_line(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.extend(text.chars().map(|c| if c.is_control() { ' ' } else { c }));
    Ok(out)
}

fn read_job(storage: &dyn Storage) -> Result<Option<Job>, Error> {
    let Some(line) = nth_line(storage, JOB_FILE, 0)? else {
        return Ok(None);
    };
    let mut parts = line.split('\t');
    let (Some(plugin), Some(name), Some(total)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(None);
    };
    let Ok(total) = total.parse::<usize>() else {
        return Ok(None);
    };
    if !is_plugin_name(plugin) || total == 0 || total > MAX_FILES {
        return Ok(None);
    }
    Ok(Some(Job {
        plugin: copy(plugin)?,
        name: copy(name)?,
        total,
    }))
}

/// The `n`th file of the job (from 0).
fn read_file_line(storage: &dyn Storage, n: usize) -> Result<Option<File>, Error> {
    let Some(line) = nth_line(storage, JOB_FILE, n + 1)? else {
        return Ok(None);
    };
    let mut parts = line.splitn(4, '\t');
    let (Some(path), Some(size), Some(digest), Some(url)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Ok(None);
    };
    let (Ok(size), Some(digest)) = (size.parse::<u64>(), digest_parse(digest)?) else {
        return Ok(None);
    };
    Ok(Some(File {
        path: copy(path)?,
        url: copy(url)?,
        size,
        digest,
    }))
}

/// How many files are here and checked.
fn counted(storage: &dyn Storage) -> usize {
    let mut buf = [0u8; 16];
    let Ok(n) = storage.read_at(DONE_FILE, 0, &mut buf) else {
        return 0;
    };
    buf.get(..n)
        .and_then(|bytes| core::str::from_utf8(bytes).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Line `n` (from 0) of the text file `path`, read in pieces of 256 bytes; a
/// line over [`MAX_LINE`] is none.
fn nth_line(storage: &dyn Storage, path: &str, n: usize) -> Result<Option<String>, Error> {
    let mut buf = [0u8; 256];
    let mut at = 0u64;
    let mut line = 0;
    let mut text = alloc::vec::Vec::new();
    loop {
        let Ok(read) = storage.read_at(path, at, &mut buf) else {
            return Ok(None);
        };
        if read == 0 {
            break;
        }
        at += read as u64;
        for &byte in buf.get(..read).unwrap_or_default() {
            if byte == b'\n' {
                if line == n {
                    return Ok(String::from_utf8(text).ok());
                }
                line += 1;
            } else if line == n {
                if text.len() >= MAX_LINE {
                    return Ok(None);
                }
                text.try_reserve(1).map_err(|_| Error::NoMemory)?;
                text.push(byte);
            }
        }
    }
    if line == n && !text.is_empty() {
        return Ok(String::from_utf8(text).ok());
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
