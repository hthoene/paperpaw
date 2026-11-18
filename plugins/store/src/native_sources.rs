//! Community plugin sources (#214, host-free): plugin libraries of others,
//! added by their address beside PaperPaw's own, which is always there and is
//! not kept here. A source is a list in the format of PaperPaw's library
//! (`plugins.json`, the pages of its plugins beside it), signed by its
//! publisher: `<list>.sig` holds the publisher's Ed25519 key and the signature
//! over [`DOMAIN`] and the list's SHA-256, both as hexadecimal digits.
//!
//! **Trust on first use.** The key that signed the first list read of a source
//! is kept with its address ([`SOURCES`]: `address key` on a line; an address
//! alone is a source not read yet, as the reader writes the file by hand).
//! From then on a list of that source signed by another key, or by none, is
//! not read ([`judge`]), and a code plugin of the source installs only where
//! its page names that key as its author's ([`gate`]): the host then takes the
//! code only if that key signed it (`device.md` 5.2). PaperPaw's own key
//! ([`FIRST_PARTY_KEY`]) is never a source's, and PaperPaw's library is never
//! one of the sources kept, so neither is replaced by a file or a list.
//!
//! **One source owns a plugin.** A plugin a community source installed is
//! kept by its id with the source's key ([`ORIGINS`]); one the store installed
//! and that is not kept there is PaperPaw's library's. A plugin is installed
//! over or updated only from the source it came from ([`foreign`]).
//!
//! **PaperPaw's list stays.** A community source's index is kept in files of
//! their own ([`SourceFiles`]), so the index of
//! PaperPaw's library, which other plugins' requests are answered from, is
//! never written over. The community source listed is held in memory only: a
//! restart, or leaving the store, lists PaperPaw's library again.
//!
//! **Memory (X4).** One source is read at a time. Its list goes from the
//! network to a file (at most [`MAX_SOURCE_LIST`] bytes), is hashed in pieces
//! of 512 bytes and indexed in pieces of 1 KiB like PaperPaw's; the signature
//! file is at most [`MAX_SIGNATURE_FILE`] bytes. Held on the heap: the sources
//! (at most [`MAX_SOURCES`] addresses of [`MAX_URL`] bytes, while they are
//! read or changed) and the plugins of community sources (at most
//! [`MAX_ORIGINS`], while a screen is open).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::digest::{hex, sha256::Sha256};
use paperpaw_sdk::native::{Error, signature::verify_message};
use paperpaw_sdk::{DirEntry, Storage, StorageError};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

use crate::native_item::FIRST_PARTY_KEY;
use crate::native_model::OFFICIAL_LIST;
use crate::native_queue::read_text;

/// The sources, in the data folder: written here, or by the reader by hand
/// (one address on a line).
pub const SOURCES: &str = "sources.txt";
/// The sources while they are written: taken where a cut left no [`SOURCES`].
const SOURCES_PART: &str = "sources.part";
/// The plugins community sources installed, each with its source's key.
pub const ORIGINS: &str = "origins.txt";
const ORIGINS_PART: &str = "origins.part";
/// Where a source's signature is fetched to.
pub const SIGNATURE_PART: &str = "list-sig.part";
/// The most sources kept.
pub const MAX_SOURCES: usize = 8;
/// The longest address of a source (bytes).
pub const MAX_URL: usize = 200;
/// The longest name a source is shown by (bytes).
pub const MAX_NAME: usize = 40;
/// The largest list read of a community source.
pub const MAX_SOURCE_LIST: u64 = 512 * 1024;
/// The largest signature file read.
pub const MAX_SIGNATURE_TEXT: usize = 512;
/// [`MAX_SIGNATURE_TEXT`] as the most the network saves of it.
pub const MAX_SIGNATURE_FILE: u64 = 512;
/// The most plugins kept as a community source's.
pub const MAX_ORIGINS: usize = 64;
/// What a source's signature covers before the list's SHA-256, so that it is
/// no signature of anything else.
pub const DOMAIN: &[u8] = b"PaperPaw plugin source\0";
const MAX_SOURCES_FILE: usize = 4096;
const MAX_ORIGINS_FILE: usize = 9600;
/// A list is hashed in pieces of this many bytes.
const PIECE: usize = 512;

/// Whether `url` is PaperPaw's own library, whatever its query.
#[must_use]
pub fn is_official(url: &str) -> bool {
    let address = url.trim().split(['?', '#']).next().unwrap_or("");
    address.eq_ignore_ascii_case(OFFICIAL_LIST)
}

/// `url` as a source's address, or `None` where it is none: `https://` with a
/// host, at most [`MAX_URL`] bytes of visible ASCII, no `.` or `..` in its
/// path (the pages of its plugins are beside it).
#[must_use]
pub fn address(url: &str) -> Option<&str> {
    let url = url.trim();
    let rest = url.strip_prefix("https://")?;
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let host = path.split('/').next().unwrap_or("");
    let plain = url.bytes().all(|b| b.is_ascii_graphic());
    let dots = path.split('/').any(|part| matches!(part, "." | ".."));
    (url.len() <= MAX_URL && plain && !host.is_empty() && !dots).then_some(url)
}

/// What a source is shown by: its address without `https://` and its query, at
/// most [`MAX_NAME`] bytes of it.
#[must_use]
pub fn name(url: &str) -> &str {
    let rest = url.strip_prefix("https://").unwrap_or(url);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    rest.get(..MAX_NAME).unwrap_or(rest)
}

/// The first digits of `key`, as the reader compares it with what the source's
/// publisher says: 16 digits in groups of four.
///
/// # Errors
/// `NoMemory`.
pub fn fingerprint(key: &[u8; 32]) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(19).map_err(|_| Error::NoMemory)?;
    for (n, byte) in key.iter().take(8).enumerate() {
        if n > 0 && n % 2 == 0 {
            out.push(' ');
        }
        hex::push(&mut out, *byte);
    }
    Ok(out)
}

fn key_text(key: &[u8; 32]) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(64).map_err(|_| Error::NoMemory)?;
    for byte in key {
        hex::push(&mut out, *byte);
    }
    Ok(out)
}

/// A community source: its address, and the key its lists are signed with
/// once one was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub key: Option<[u8; 32]>,
}

/// What adding an address came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Added {
    /// It is a source now, not read yet.
    New,
    /// It was one already.
    Known,
    /// It is PaperPaw's own library, which is always there.
    Official,
    /// It is no address of a source.
    Invalid,
    /// There are [`MAX_SOURCES`] already.
    Full,
}

/// What keeping a source's key came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pinned {
    /// The key is the source's from now on.
    Now,
    /// It was the source's key already.
    Already,
    /// The source has another key: it stays.
    Other,
    /// PaperPaw's own key is no source's.
    Official,
    /// No such source.
    Unknown,
}

/// The community sources, in the order they were added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Sources {
    list: Vec<Source>,
}

impl Sources {
    /// The sources `text` names, a line each: an address, then its key where
    /// one is kept. A line that is no address, PaperPaw's own library, an
    /// address named before, a key that is none or PaperPaw's own, and what is
    /// past [`MAX_SOURCES`] are left out; `#` begins a remark.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let mut list: Vec<Source> = Vec::new();
        // An editor's byte-order mark is no part of the first address.
        for line in text.trim_start_matches('\u{feff}').lines() {
            let mut words = line.split_whitespace();
            let Some(url) = words.next().filter(|word| !word.starts_with('#')) else {
                continue;
            };
            let Some(url) = address(url).filter(|url| !is_official(url)) else {
                continue;
            };
            let key = match words.next() {
                None => None,
                Some(word) => match hex::decode_array::<32>(word) {
                    Some(key) if key != FIRST_PARTY_KEY => Some(key),
                    // A key that cannot be read is not taken for none: the
                    // source would be trusted anew without a word.
                    _ => continue,
                },
            };
            if list.len() >= MAX_SOURCES || list.iter().any(|known| known.url == url) {
                continue;
            }
            push(
                &mut list,
                Source {
                    url: copy(url)?,
                    key,
                },
            )?;
        }
        Ok(Self { list })
    }

    /// The sources kept in the data folder; none where there is no file, it is
    /// too long or the card does not answer.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(storage: &dyn Storage) -> Result<Self, Error> {
        match read_kept(storage, SOURCES, SOURCES_PART, MAX_SOURCES_FILE)? {
            Some(text) => Self::parse(&text),
            None => Ok(Self::default()),
        }
    }

    /// Keeps the sources on the card.
    ///
    /// # Errors
    /// `NoMemory`; `Io` where the card refuses.
    pub fn keep(&self, storage: &dyn Storage) -> Result<(), Error> {
        let mut text = String::new();
        for source in &self.list {
            let key = match &source.key {
                Some(key) => key_text(key)?,
                None => String::new(),
            };
            let between = if key.is_empty() { "" } else { " " };
            let line = concat(&[&source.url, between, &key, "\n"])?;
            text.try_reserve(line.len()).map_err(|_| Error::NoMemory)?;
            text.push_str(&line);
        }
        keep_text(storage, SOURCES, SOURCES_PART, &text)
    }

    /// The sources.
    #[must_use]
    pub fn all(&self) -> &[Source] {
        &self.list
    }

    /// The source at `url`.
    #[must_use]
    pub fn get(&self, url: &str) -> Option<&Source> {
        self.list.iter().find(|source| source.url == url)
    }

    /// Makes `url` a source, not read yet.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn add(&mut self, url: &str) -> Result<Added, Error> {
        if is_official(url) {
            return Ok(Added::Official);
        }
        let Some(url) = address(url) else {
            return Ok(Added::Invalid);
        };
        if self.get(url).is_some() {
            return Ok(Added::Known);
        }
        if self.list.len() >= MAX_SOURCES {
            return Ok(Added::Full);
        }
        push(
            &mut self.list,
            Source {
                url: copy(url)?,
                key: None,
            },
        )?;
        Ok(Added::New)
    }

    /// Takes the source at `url` away; whether there was one.
    pub fn remove(&mut self, url: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|source| source.url != url);
        self.list.len() != before
    }

    /// Keeps `key` as the key of the source at `url`, where it has none yet.
    /// A key kept is never replaced, and PaperPaw's own is nobody else's.
    pub fn pin(&mut self, url: &str, key: &[u8; 32]) -> Pinned {
        if *key == FIRST_PARTY_KEY {
            return Pinned::Official;
        }
        let Some(source) = self.list.iter_mut().find(|source| source.url == url) else {
            return Pinned::Unknown;
        };
        match source.key {
            None => {
                source.key = Some(*key);
                Pinned::Now
            }
            Some(kept) if kept == *key => Pinned::Already,
            Some(_) => Pinned::Other,
        }
    }
}

/// The text of `name`, or of `part` where a cut while it was written left
/// only that.
fn read_kept(
    storage: &dyn Storage,
    name: &str,
    part: &str,
    cap: usize,
) -> Result<Option<String>, Error> {
    match read_text(storage, name, cap)? {
        Some(text) => Ok(Some(text)),
        None if storage.size(name).is_err() => read_text(storage, part, cap),
        None => Ok(None),
    }
}

/// Writes `text` as `name` through `part`, so that a cut leaves the file as it
/// was or as it is to be; no text is no file.
fn keep_text(storage: &dyn Storage, name: &str, part: &str, text: &str) -> Result<(), Error> {
    let gone = |path: &str| match storage.remove(path) {
        Ok(()) | Err(StorageError::NotFound) => Ok(()),
        Err(_) => Err(Error::Io),
    };
    if text.is_empty() {
        gone(part)?;
        return gone(name);
    }
    storage
        .write(part, text.as_bytes())
        .map_err(|_| Error::Io)?;
    gone(name)?;
    storage.rename(part, name).map_err(|_| Error::Io)
}

/// Whose word a list is read on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Trust {
    /// PaperPaw's own library: read as before, its plugins by their authors'
    /// keys.
    #[default]
    Official,
    /// A community source no list was read of yet: the key that signed the
    /// first one is kept.
    FirstUse,
    /// A community source with its key.
    Pinned([u8; 32]),
}

impl Trust {
    /// The key kept of a community source.
    #[must_use]
    pub const fn key(&self) -> Option<&[u8; 32]> {
        match self {
            Trust::Pinned(key) => Some(key),
            _ => None,
        }
    }
}

/// What a signature file says: who signed, and the signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signed {
    pub key: [u8; 32],
    pub signature: [u8; 64],
}

impl Signed {
    /// Reads a signature file: the key (64 digits), then the signature (128),
    /// apart by white space; `None` for anything else.
    #[must_use]
    pub fn read(text: &str) -> Option<Self> {
        let mut words = text.split_whitespace();
        let key = hex::decode_array::<32>(words.next()?)?;
        let signature = hex::decode_array::<64>(words.next()?)?;
        words.next().is_none().then_some(Self { key, signature })
    }
}

/// Where the signature of the list at `list` is: `.sig` after its path, with
/// the list's query.
///
/// # Errors
/// `NoMemory`.
pub fn signature_url(list: &str) -> Result<String, Error> {
    let list = list.split('#').next().unwrap_or(list);
    match list.split_once('?') {
        Some((path, query)) => concat(&[path, ".sig?", query]),
        None => concat(&[list, ".sig"]),
    }
}

/// What a source's key signs of a list with the SHA-256 `digest`.
#[must_use]
pub fn message(digest: &[u8; 32]) -> [u8; DOMAIN.len() + 32] {
    let mut out = [0; DOMAIN.len() + 32];
    let (domain, rest) = out.split_at_mut(DOMAIN.len());
    domain.copy_from_slice(DOMAIN);
    rest.copy_from_slice(digest);
    out
}

/// The SHA-256 of the file `path`, read in pieces of 512 bytes from the stack;
/// `None` where the card does not answer.
#[inline(never)]
#[must_use]
pub fn digest(storage: &dyn Storage, path: &str) -> Option<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut piece = [0u8; PIECE];
    let mut at = 0u64;
    loop {
        let n = storage.read_at(path, at, &mut piece).ok()?;
        if n == 0 {
            break;
        }
        hash.update(piece.get(..n)?);
        at = at.saturating_add(n as u64);
    }
    Some(hash.finish())
}

/// Whether a list is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// Signed by the source's key; `first`: the key is the source's from now
    /// on (trust on first use).
    Trusted { key: [u8; 32], first: bool },
    /// No signature, or one that does not hold for this list.
    Unsigned,
    /// Signed by another key than the one kept of the source.
    KeyChanged,
    /// It names PaperPaw's own key, which is no community source's.
    Official,
}

/// Judges a community source's list with the SHA-256 `digest` by what its
/// signature file says (`signed`) and the key kept of the source (`trust`).
#[must_use]
pub fn judge(trust: Trust, signed: Option<&Signed>, digest: &[u8; 32]) -> Verdict {
    let Some(signed) = signed else {
        return Verdict::Unsigned;
    };
    if signed.key == FIRST_PARTY_KEY {
        return Verdict::Official;
    }
    if trust.key().is_some_and(|kept| *kept != signed.key) {
        return Verdict::KeyChanged;
    }
    if !verify_message(&signed.key, &message(digest), &signed.signature) {
        return Verdict::Unsigned;
    }
    Verdict::Trusted {
        key: signed.key,
        first: trust == Trust::FirstUse,
    }
}

/// The plugins community sources installed, each with its source's key.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Origins {
    list: Vec<(String, [u8; 32])>,
}

impl Origins {
    /// The plugins `text` names: an id, a tab, the key; what is no such line
    /// and what is past [`MAX_ORIGINS`] is left out.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let mut list = Vec::new();
        for line in text.lines().take(MAX_ORIGINS) {
            let Some((id, key)) = line.split_once('\t') else {
                continue;
            };
            let Some(key) = hex::decode_array::<32>(key.trim()) else {
                continue;
            };
            if paperpaw_sdk::install::is_plugin_name(id) {
                push(&mut list, (copy(id)?, key))?;
            }
        }
        Ok(Self { list })
    }

    /// The plugins kept in the data folder.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(storage: &dyn Storage) -> Result<Self, Error> {
        match read_kept(storage, ORIGINS, ORIGINS_PART, MAX_ORIGINS_FILE)? {
            Some(text) => Self::parse(&text),
            None => Ok(Self::default()),
        }
    }

    /// Keeps them on the card.
    ///
    /// # Errors
    /// `NoMemory`; `Io` where the card refuses.
    pub fn keep(&self, storage: &dyn Storage) -> Result<(), Error> {
        let mut text = String::new();
        for (id, key) in &self.list {
            let line = concat(&[id, "\t", &key_text(key)?, "\n"])?;
            text.try_reserve(line.len()).map_err(|_| Error::NoMemory)?;
            text.push_str(&line);
        }
        keep_text(storage, ORIGINS, ORIGINS_PART, &text)
    }

    /// The key of the community source plugin `id` came from, `None` where it
    /// came from none.
    #[must_use]
    pub fn of(&self, id: &str) -> Option<&[u8; 32]> {
        self.list
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, key)| key)
    }

    /// Plugin `id` is the source's with `key` from now on, or PaperPaw's
    /// library's (`None`); whether that changed what is kept.
    ///
    /// # Errors
    /// `NoMemory`; `NoSpace` where [`MAX_ORIGINS`] plugins are kept already.
    pub fn set(&mut self, id: &str, key: Option<&[u8; 32]>) -> Result<bool, Error> {
        if self.of(id) == key {
            return Ok(false);
        }
        self.list.retain(|(known, _)| known != id);
        if let Some(key) = key {
            if self.list.len() >= MAX_ORIGINS {
                return Err(Error::NoSpace);
            }
            push(&mut self.list, (copy(id)?, *key))?;
        }
        Ok(true)
    }
}

/// Whether a plugin the device has is another source's than the one listed
/// now (`trust`): `recorded` is the key kept of it ([`Origins::of`]),
/// `by_store` whether a store installed the version here (`None`: it is not
/// installed). A plugin a store installed and that is kept for no source is
/// PaperPaw's library's; one put in place another way is nobody's, and the
/// reader may put a source's version in its place.
#[must_use]
pub fn foreign(recorded: Option<&[u8; 32]>, by_store: Option<bool>, trust: Trust) -> bool {
    by_store == Some(true)
        && match trust {
            Trust::Official => recorded.is_some(),
            Trust::Pinned(key) => recorded != Some(&key),
            Trust::FirstUse => true,
        }
}

/// Whether a plugin of the source listed may be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Gate {
    /// It may.
    Open,
    /// The device has it from another source: its updates come from there.
    Foreign,
    /// The source has no key yet: no list of it was read.
    NoKey,
    /// Its code is not said to be signed by the source's key.
    OtherKey,
}

/// Whether one of the files at `paths` is a plugin's code (`code.ppn` and the
/// like, as the card compares names). A community source's page that lists
/// one without being a code plugin's is treated as code no key is named for:
/// the host would ask the reader about whatever key signed it.
#[must_use]
pub fn names_code<'a>(paths: impl IntoIterator<Item = &'a str>) -> bool {
    paths.into_iter().any(|path| {
        paperpaw_sdk::native::CODE_FILES
            .iter()
            .any(|name| path.eq_ignore_ascii_case(name))
    })
}

/// Judges the install of a plugin from the source listed: `code` and `author`
/// are its page's (a code plugin, the key named as its author's), `foreign`
/// what [`foreign`] says of it.
#[must_use]
pub fn gate(trust: Trust, code: bool, author: Option<&[u8; 32]>, foreign: bool) -> Gate {
    if foreign {
        return Gate::Foreign;
    }
    match trust {
        Trust::Official => Gate::Open,
        Trust::FirstUse => Gate::NoKey,
        Trust::Pinned(key) if code && author != Some(&key) => Gate::OtherKey,
        Trust::Pinned(_) => Gate::Open,
    }
}

/// What a community source's files begin with.
const PREFIX: &str = "source-";
/// The longest name made for one.
const MAX_FILE_NAME: usize = 48;

/// Whether `path` is a file kept of a list: its index, a search's hits, a
/// sort's runs. The list fetched and its signature are not: the host saves
/// them by the names it is given, one fetch at a time, and they go once read.
fn of_list(path: &str) -> bool {
    ["index.", "sections.", "search.", "sort."]
        .iter()
        .any(|start| path.starts_with(start))
}

/// The data folder as the source listed sees it: for a community source the
/// files of a list are others than those of PaperPaw's library (their names
/// begin with `source-`), every other file is the same. The names are made on
/// the stack.
#[derive(Debug, Clone, Copy)]
pub struct SourceFiles<S> {
    inner: S,
    community: bool,
}

impl<S: Storage> SourceFiles<S> {
    /// `inner`, with the list files of a community source where `community`.
    pub const fn new(inner: S, community: bool) -> Self {
        Self { inner, community }
    }

    /// The data folder itself, every file by its own name.
    pub const fn inner(&self) -> &S {
        &self.inner
    }

    /// Runs `f` with the name `path` has on the card.
    fn at<R>(&self, path: &str, f: impl FnOnce(&str) -> R) -> R {
        let mut name = [0u8; MAX_FILE_NAME];
        let len = PREFIX.len() + path.len();
        if !self.community || !of_list(path) || len > name.len() {
            return f(path);
        }
        let (prefix, rest) = name.split_at_mut(PREFIX.len());
        prefix.copy_from_slice(PREFIX.as_bytes());
        if let Some(rest) = rest.get_mut(..path.len()) {
            rest.copy_from_slice(path.as_bytes());
        }
        match name.get(..len).map(core::str::from_utf8) {
            Some(Ok(name)) => f(name),
            _ => f(path),
        }
    }
}

impl<S: Storage> Storage for SourceFiles<S> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.at(path, |path| self.inner.read_at(path, offset, buf))
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.at(path, |path| self.inner.size(path))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.at(path, |path| self.inner.write(path, data))
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.at(path, |path| self.inner.append(path, data))
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.inner.sync()
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.at(path, |path| self.inner.remove(path))
    }

    fn exists(&self, path: &str) -> bool {
        self.at(path, |path| self.inner.exists(path))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.at(from, |from| self.at(to, |to| self.inner.rename(from, to)))
    }
}
