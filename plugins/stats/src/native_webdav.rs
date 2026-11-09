//! Sharing statistics with KOReader through the cloud folder (WebDAV, e.g.
//! Nextcloud) it syncs its statistics to: the engine, without a host.
//!
//! A sync downloads `statistics.sqlite3` from the folder, merges it (see
//! [`merge`]), uploads the result (only if nobody changed the file in
//! between: `If-Match` with its ETag, or `If-None-Match: *` where there was
//! none, else it starts over, at most [`ATTEMPTS`] times) and keeps what it
//! uploaded, to tell later what was deleted there; for that folder and user
//! only. KOReader does the same, so either may sync first. A downloaded file
//! that cannot be read ends the sync: nothing is merged and nothing uploaded.
//!
//! Nothing counts as deleted there where the download came without an ETag:
//! the upload could then replace a file changed meanwhile. A file gone from a
//! folder this device uploaded to is not replaced at once (a server may say so
//! by mistake): the sync says so, and the next one uploads. Where the file
//! there lacks much and it cannot be told whether that was deleted there (see
//! [`merge`]), nothing is uploaded: the user is asked whether to remove it
//! here too, and the sync starts again with the answer, kept for that file
//! until an upload changes it.
//!
//! The engine is the interface-6 `Sync` as a state machine over a
//! [`Storage`] (the data folder as the data layer sees it): every call says
//! the next [`Step`] for the caller to carry out (the request, the message,
//! the question), and every allocation is fallible.
use crate::merge::{self, Failure, Outcome};
use crate::texts::Words;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::{Storage, StorageError};
use paperpaw_ui_kit::text;

/// The plugin's data folder as the data layer names it.
pub(crate) const DIR: &str = ".paperpaw/plugins/stats";
/// The file's name in the folder, as KOReader names it; also the file this
/// device uploads from, in the data folder.
pub(crate) const FILE: &str = "statistics.sqlite3";
/// Where the download is saved, in the data folder.
pub(crate) const INCOMING: &str = "incoming.sqlite3";
/// Tries when the file changes during a sync.
const ATTEMPTS: u8 = 3;
/// Where KOReader keeps its statistics on a device: on Android's shared
/// storage, and at the root of a memory card.
pub(crate) const KOREADER_FILES: [&str; 2] = [
    "storage/koreader/settings/statistics.sqlite3",
    "koreader/settings/statistics.sqlite3",
];

/// The path of `name` in the data folder, as the data layer names it.
fn path(name: &str) -> Result<String, Error> {
    text::concat(&[DIR, "/", name])
}

/// The folder as a base URL: `https://` unless another scheme is given, no
/// `/` at the end.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn folder(text: &str) -> Result<String, Error> {
    paperpaw_sdk::web::try_base_url(text).map_err(|_| Error::NoMemory)
}

/// What a sync's settings are, as set when it starts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Facts<'a> {
    /// Whether the device has a network at all.
    pub network: bool,
    /// The folder as typed, and the user.
    pub folder: &'a str,
    pub user: &'a str,
}

/// What the server answered to the download.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Downloaded<'a> {
    /// The status; 0 where there was no answer.
    pub status: u16,
    /// The file's ETag, where the answer had one.
    pub etag: Option<&'a str>,
}

/// How a sync ended, in words ([`Say::text`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Say {
    NoNetwork,
    NoFolder,
    Running,
    /// The file is gone from the folder this device uploaded to.
    Missing,
    /// The file in the folder cannot be read (with why).
    Unreadable(String),
    /// The merge failed here (with why).
    Failed(String),
    /// The file there lacks much; the user is asked.
    Asked,
    Done {
        books: usize,
        kept: bool,
    },
    /// The server's status, or 0 where it could not be reached.
    Status(u16),
    NoKoreader,
    KoreaderBusy,
    Imported {
        books: usize,
        stays: usize,
    },
    ImportUnreadable(String),
}

impl Say {
    /// The message, in the catalog's words.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the catalog says.
    pub(crate) fn text(&self, words: &mut impl Words) -> Result<String, Error> {
        match self {
            Self::NoNetwork => words.copy("stats.sync.no-network"),
            Self::NoFolder => words.copy("stats.sync.no-folder"),
            Self::Running => words.copy("stats.sync.running"),
            Self::Missing => words.copy("stats.sync.missing"),
            Self::Asked => words.copy("stats.sync.asked"),
            Self::NoKoreader => words.copy("stats.import.none"),
            Self::KoreaderBusy => words.copy("stats.import.busy"),
            Self::Unreadable(why) => with_reason(words, "stats.sync.unreadable", why),
            Self::Failed(why) => with_reason(words, "stats.sync.failed", why),
            Self::ImportUnreadable(why) => with_reason(words, "stats.import.unreadable", why),
            Self::Done { books, kept } => {
                let key = if *kept {
                    "stats.sync.done-kept"
                } else {
                    "stats.sync.done"
                };
                let count = books_text(words, *books)?;
                words.format(
                    key,
                    &[Arg::Text {
                        name: "books",
                        value: &count,
                    }],
                )
            }
            Self::Status(status) => {
                let key = match status {
                    0 => "stats.sync.unreachable",
                    401 | 403 => "stats.sync.denied",
                    404 | 409 => "stats.sync.no-such-folder",
                    _ => "stats.sync.server-error",
                };
                let mut digits = [0u8; 20];
                let shown = text::decimal(usize::from(*status), &mut digits);
                words.format(
                    key,
                    &[Arg::Text {
                        name: "status",
                        value: shown,
                    }],
                )
            }
            Self::Imported { books, stays } => {
                let count = books_text(words, *books)?;
                words.format(
                    "stats.import.done",
                    &[
                        Arg::Text {
                            name: "books",
                            value: &count,
                        },
                        Arg::Int {
                            name: "stays",
                            value: i64::try_from(*stays).unwrap_or(i64::MAX),
                        },
                    ],
                )
            }
        }
    }
}

/// The text of `key` with `why` as its reason.
fn with_reason(words: &mut impl Words, key: &str, why: &str) -> Result<String, Error> {
    words.format(
        key,
        &[Arg::Text {
            name: "reason",
            value: why,
        }],
    )
}

/// "12 books".
fn books_text(words: &mut impl Words, books: usize) -> Result<String, Error> {
    words.format(
        "stats.books-count",
        &[Arg::Count(i64::try_from(books).unwrap_or(i64::MAX))],
    )
}

/// What the upload says about the file there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Precondition {
    /// There was none: only if there still is none (`If-None-Match: *`).
    NoFile,
    /// It was this version (`If-Match`, the weak mark left off).
    Version(String),
    /// The download came without a version: no condition.
    Unknown,
}

/// What to do next.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The sync ends (or does not begin) with this message.
    Say(Say),
    /// Download the file at [`Engine::url`] into [`INCOMING`].
    Download,
    /// Upload [`FILE`] to [`Engine::url`] on this condition.
    Upload(Precondition),
    /// Nothing is uploaded; the sync ends with [`Say::Asked`] and the user is
    /// asked whether to remove here what was removed there, for the file with
    /// this ETag.
    Ask { etag: String },
}

/// What the user decided where the file there lacks much: for that file (its
/// address and ETag), whether to remove it here too.
#[derive(Debug)]
struct Decision {
    url: String,
    etag: String,
    remove: bool,
}

/// A sync in progress, and what it knows.
#[derive(Debug, Default)]
pub(crate) struct Engine {
    attempt: u8,
    running: bool,
    /// The last upload to the folder and user of this sync.
    last_upload: String,
    /// The file's address and the user of this sync, as set when it started.
    url: String,
    user: String,
    /// The books there are after the merge.
    books: usize,
    /// Whether what the file there lacked was kept ([`Outcome::kept`]).
    kept: bool,
    decided: Option<Decision>,
}

/// Removes `path`; a file that cannot be removed only takes room and is
/// written over by the next sync.
fn drop_file(storage: &dyn Storage, path: &str) {
    storage.remove(path).unwrap_or(());
}

impl Engine {
    /// Whether a sync is going.
    pub(crate) const fn running(&self) -> bool {
        self.running
    }

    /// The address of the file in the folder.
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    /// The user (empty: no authorization).
    pub(crate) fn user(&self) -> &str {
        &self.user
    }

    fn end(&mut self) {
        self.running = false;
        self.attempt = 0;
    }

    /// Starts a sync, or says why it cannot.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn start(&mut self, storage: &dyn Storage, facts: Facts<'_>) -> Result<Step, Error> {
        if !facts.network {
            return Ok(Step::Say(Say::NoNetwork));
        }
        if facts.folder.trim().is_empty() {
            return Ok(Step::Say(Say::NoFolder));
        }
        let base = folder(facts.folder)?;
        let url = text::concat(&[&base, "/", FILE])?;
        let user = text::copy(facts.user)?;
        let last = last_upload(&base, facts.user)?;
        let incoming = path(INCOMING)?;
        self.url = url;
        self.user = user;
        self.last_upload = last;
        self.running = true;
        self.attempt = self.attempt.saturating_add(1);
        // The download is written over what an earlier sync left.
        drop_file(storage, &incoming);
        Ok(Step::Download)
    }

    /// The other side's file is here (or not): merge, then upload.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn downloaded(
        &mut self,
        storage: &dyn Storage,
        reply: Downloaded<'_>,
    ) -> Result<Option<Step>, Error> {
        if !self.running {
            return Ok(None);
        }
        let incoming = path(INCOMING)?;
        let uploaded_before = storage.size(&self.last_upload).is_ok();
        let income = match reply.status {
            200 => Some(incoming.as_str()),
            // Gone from where this device uploaded to (or a server's mistake):
            // not replaced with this device's alone. Without the last upload,
            // the next sync uploads.
            404 if uploaded_before => {
                drop_file(storage, &incoming);
                drop_file(storage, &self.last_upload);
                self.end();
                return Ok(Some(Step::Say(Say::Missing)));
            }
            // None yet: this is the first.
            404 => None,
            status => {
                drop_file(storage, &incoming);
                self.end();
                return Ok(Some(Step::Say(Say::Status(status))));
            }
        };
        // Without an ETag the upload replaces the file whatever came
        // meanwhile: what it lacks then is not taken for deleted there.
        let cached = (uploaded_before && reply.etag.is_some()).then_some(self.last_upload.as_str());
        let upload = path(FILE)?;
        let decided = self.decided_for(reply.etag);
        let result = merge::merge_as(storage, income, cached, Some(&upload), DIR, decided);
        drop_file(storage, &incoming);
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(failure) => {
                self.end();
                let why = text::format(format_args!("{failure}"))?;
                return Ok(Some(Step::Say(match failure {
                    Failure::Income(_) => Say::Unreadable(why),
                    Failure::Here(_) => Say::Failed(why),
                })));
            }
        };
        if outcome.unclear {
            let etag = text::copy(reply.etag.unwrap_or_default())?;
            drop_file(storage, &upload);
            self.end();
            return Ok(Some(Step::Ask { etag }));
        }
        self.books = outcome.books;
        self.kept = outcome.kept;
        Ok(Some(Step::Upload(match (income, reply.etag) {
            (None, _) => Precondition::NoFile,
            // A weak validator does not match strongly (as KOReader does).
            (Some(_), Some(etag)) => Precondition::Version(text::copy(
                etag.trim_start_matches("W/").trim_start_matches("w/"),
            )?),
            (Some(_), None) => Precondition::Unknown,
        })))
    }

    /// What the user decided for the file there with `etag`, if anything.
    fn decided_for(&self, etag: Option<&str>) -> Option<bool> {
        let decision = self.decided.as_ref()?;
        (decision.url == self.url && Some(decision.etag.as_str()) == etag)
            .then_some(decision.remove)
    }

    /// The user decided: the sync starts again, with the decision for the file
    /// asked about.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn decide(
        &mut self,
        storage: &dyn Storage,
        facts: Facts<'_>,
        (etag, remove): (&str, bool),
    ) -> Result<Option<Step>, Error> {
        if self.running {
            return Ok(None);
        }
        let decision = Decision {
            url: text::copy(&self.url)?,
            etag: text::copy(etag)?,
            remove,
        };
        self.decided = Some(decision);
        self.start(storage, facts).map(Some)
    }

    /// The merged file is up (or someone was faster).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn uploaded(
        &mut self,
        storage: &dyn Storage,
        facts: Facts<'_>,
        status: u16,
    ) -> Result<Option<Step>, Error> {
        if !self.running {
            return Ok(None);
        }
        let upload = path(FILE)?;
        match status {
            200..=299 => {
                // What was uploaded is what the next sync compares with. Where
                // it cannot be kept, the one before stays: it can only tell
                // less, never more, as deleted there.
                if paperpaw_sdk::files::move_to(storage, &upload, &self.last_upload).is_err() {
                    drop_file(storage, &upload);
                }
                forget_other_uploads(storage, &self.last_upload);
                // The file there changed: a decision for it holds no more.
                self.decided = None;
                self.end();
                Ok(Some(Step::Say(Say::Done {
                    books: self.books,
                    kept: self.kept,
                })))
            }
            // Changed in between: again, from the download.
            412 if self.attempt < ATTEMPTS => {
                drop_file(storage, &upload);
                self.running = false;
                self.start(storage, facts).map(Some)
            }
            status => {
                drop_file(storage, &upload);
                self.end();
                Ok(Some(Step::Say(Say::Status(status))))
            }
        }
    }

    /// A request that ended without an answer (cancelled, or the network went
    /// away) ends the sync as an unreachable server does.
    pub(crate) fn lost(&mut self, storage: &dyn Storage) -> Option<Step> {
        if !self.running {
            return None;
        }
        if let Ok(incoming) = path(INCOMING) {
            drop_file(storage, &incoming);
        }
        if let Ok(upload) = path(FILE) {
            drop_file(storage, &upload);
        }
        self.end();
        Some(Step::Say(Say::Status(0)))
    }
}

/// The file this device last uploaded to the folder `base` and `user`: the
/// next sync there compares with it. One is kept for one folder and user only:
/// another one's tells nothing about what was deleted here.
fn last_upload(base: &str, user: &str) -> Result<String, Error> {
    let target = text::concat(&[base, "\n", user])?;
    let key = paperpaw_sdk::digest::fnv1a64(&target);
    let name = text::format(format_args!("{FILE}.{key:016x}.sync"))?;
    path(&name)
}

/// Removes the last uploads kept but `keep`: those of other folders or users,
/// and the one of earlier versions (`statistics.sqlite3.sync`, of whichever
/// folder was set then).
fn forget_other_uploads(storage: &dyn Storage, keep: &str) {
    let mut others: Vec<String> = Vec::new();
    let mut stopped = false;
    let listed = storage.list(DIR, &mut |entry| {
        if stopped || !entry.name.starts_with(FILE) || !entry.name.ends_with(".sync") {
            return;
        }
        let Ok(full) = path(entry.name) else {
            stopped = true;
            return;
        };
        if full != keep {
            if others.try_reserve(1).is_err() {
                stopped = true;
            } else {
                others.push(full);
            }
        }
    });
    // A folder that cannot be listed has no old uploads to forget.
    if listed.is_err() {
        return;
    }
    for full in others {
        drop_file(storage, &full);
    }
}

/// KOReader's statistics on this device, if any, as the card (root 1) names
/// them.
pub(crate) fn koreader_file(card: &dyn Storage) -> Option<&'static str> {
    KOREADER_FILES
        .into_iter()
        .find(|file| card.size(file).is_ok())
}

/// Takes over KOReader's statistics on this device: `storage` names the data
/// folder's files by [`DIR`] and the card's by their own paths.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn import(storage: &dyn Storage, card: &dyn Storage) -> Result<Say, Error> {
    let Some(file) = koreader_file(card) else {
        return Ok(Say::NoKoreader);
    };
    // Written by KOReader while it runs: its newest pages may still be in the
    // log beside the file.
    let wal = text::concat(&[file, "-wal"])?;
    if card.size(&wal).is_ok_and(|size| size > 0) {
        return Ok(Say::KoreaderBusy);
    }
    match merge::merge(storage, Some(file), None, None, DIR) {
        Ok(Outcome { books, stays, .. }) => Ok(Say::Imported { books, stays }),
        Err(failure) => {
            let why = text::format(format_args!("{failure}"))?;
            Ok(match failure {
                Failure::Income(_) => Say::ImportUnreadable(why),
                Failure::Here(_) => Say::Failed(why),
            })
        }
    }
}

/// The password as a `Basic` authorization value for `user`, in a fresh
/// allocation: the caller wipes it ([`paperpaw_ui_kit::mem::wipe_text`]) when
/// the request is made.
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn basic_auth(user: &str, password: &str) -> Result<String, Error> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut pair: Vec<u8> = Vec::new();
    pair.try_reserve_exact(user.len() + 1 + password.len())
        .map_err(|_| Error::NoMemory)?;
    pair.extend_from_slice(user.as_bytes());
    pair.push(b':');
    pair.extend_from_slice(password.as_bytes());
    let mut out = String::new();
    let made = out.try_reserve_exact(6 + pair.len().div_ceil(3) * 4);
    if made.is_err() {
        paperpaw_ui_kit::mem::wipe(&mut pair);
        return Err(Error::NoMemory);
    }
    out.push_str("Basic ");
    for chunk in pair.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                let at = ((n >> (18 - 6 * i)) & 63) as usize;
                out.push(char::from(ALPHABET.get(at).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    paperpaw_ui_kit::mem::wipe(&mut pair);
    Ok(out)
}

/// A storage that names the data folder's files by [`DIR`] and sends every
/// other path to the card (root 1), which it only reads.
pub(crate) struct Both<'a> {
    pub data: &'a dyn Storage,
    pub card: &'a dyn Storage,
}

impl Both<'_> {
    fn is_data(path: &str) -> bool {
        path.strip_prefix(DIR)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }
}

impl Storage for Both<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if Self::is_data(path) {
            self.data.read_at(path, offset, buf)
        } else {
            self.card.read_at(path, offset, buf)
        }
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        if Self::is_data(path) {
            self.data.size(path)
        } else {
            self.card.size(path)
        }
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if Self::is_data(path) {
            self.data.write(path, data)
        } else {
            Err(StorageError::Unsupported)
        }
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if Self::is_data(path) {
            self.data.append(path, data)
        } else {
            Err(StorageError::Unsupported)
        }
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        if Self::is_data(dir) {
            self.data.list(dir, visit)
        } else {
            Err(StorageError::Unsupported)
        }
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        if Self::is_data(path) {
            self.data.remove(path)
        } else {
            Err(StorageError::Unsupported)
        }
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if Self::is_data(from) && Self::is_data(to) {
            self.data.rename(from, to)
        } else {
            Err(StorageError::Unsupported)
        }
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.data.sync()
    }
}
