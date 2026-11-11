//! What the transfer decides without the host: the address a server is opened
//! at, which names a book may be put under, which file is read-only, and what
//! an offer from a computer over the cable says. The same rules as the
//! interface-6 plugin (`link.rs`, `web.rs`, `cable.rs`), with fallible
//! allocation and the text given by the caller instead of read from a
//! storage. Serving, the screens and the installs are the native plugin's
//! (see `PORTING-7.md`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::builtin::CONFIG_FILE;
use paperpaw_sdk::install::{self, is_plugin_name, is_safe_path};
use paperpaw_sdk::native::{CODE_FILES, Error};

/// Books of an offer at most (more go in the offers after): each is kept in
/// [`Book`]'s 56 bytes while it is waited for.
pub const MAX_BOOKS: usize = 64;
/// Lines of an offer at most: its plugin's files and its books.
pub const MAX_FILES: usize = install::MAX_FILES + MAX_BOOKS + 1;
/// The longest line of an offer: a book's name (200 bytes), its size and its
/// SHA-256, with room to spare.
pub const MAX_LINE: usize = 320;
/// The longest offer read.
pub const MAX_OFFER: u64 = (MAX_FILES * MAX_LINE) as u64;
/// Books named on the device at most; the others are counted.
pub const NAMED: usize = 8;

/// The address a server that started at `address` (`192.168.1.20:8080`, the
/// port absent for the usual one) is reached at, with `key`:
/// `(shown, scanned)`. `reach` is the name or address another device uses
/// (`WifiState::reach`), where PaperPaw manages the radio. The text shows the
/// name (easier to type); the QR code keeps the numeric address (a phone's
/// name lookup is not sure everywhere).
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn urls(reach: Option<&str>, address: &str, key: &str) -> Result<(String, String), Error> {
    let scanned = page(address, None, key)?;
    let Some(reach) = reach else {
        return Ok((copy(&scanned)?, scanned));
    };
    let port = address.rsplit_once(':').map(|(_, port)| port);
    Ok((page(reach, port, key)?, scanned))
}

/// `http://<host>[:<port>]/?k=<key>`.
fn page(host: &str, port: Option<&str>, key: &str) -> Result<String, Error> {
    let mut out = String::new();
    let port_len = port.map_or(0, |port| 1 + port.len());
    reserve(&mut out, 7 + host.len() + port_len + 4 + key.len())?;
    out.push_str("http://");
    out.push_str(host);
    if let Some(port) = port {
        out.push(':');
        out.push_str(port);
    }
    out.push_str("/?k=");
    out.push_str(key);
    Ok(out)
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    reserve(&mut out, text.len())?;
    out.push_str(text);
    Ok(out)
}

fn reserve(out: &mut String, extra: usize) -> Result<(), Error> {
    out.try_reserve_exact(extra).map_err(|_| Error::NoMemory)
}

fn push<T>(list: &mut Vec<T>, item: T) -> Result<(), Error> {
    list.try_reserve(1).map_err(|_| Error::NoMemory)?;
    list.push(item);
    Ok(())
}

/// Whether the file at `path` (as found) may be fetched but not changed:
/// PaperPaw's settings (`paperpaw.ini`, in any case, as the card takes it),
/// which name the servers that get passwords and keys.
pub fn read_only(path: &str) -> bool {
    path.eq_ignore_ascii_case(CONFIG_FILE)
}

/// A name a book may be put under in the library folder: no folders, no
/// hidden file.
pub fn is_book_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(char::is_control)
}

/// A file offered: its path, size and SHA-256.
pub type Offered = (String, u64, [u8; 32]);

/// A book offered while it is waited for: its name by part of the name's
/// SHA-256 (so that many take little memory), its size and SHA-256.
pub type Book = ([u8; 16], u64, [u8; 32]);

/// What a book's name is kept as ([`Book`]).
pub fn name_key(name: &str) -> [u8; 16] {
    let digest = paperpaw_sdk::digest::sha256::digest(name.as_bytes());
    let mut key = [0; 16];
    key.copy_from_slice(&digest[..16]);
    key
}

/// What the computer means to send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offer {
    /// A plugin: its id and files.
    pub plugin: Option<(String, Vec<Offered>)>,
    pub books: Vec<Book>,
    /// The first books' names and sizes, as the reader is shown them.
    pub named: Vec<(String, u64)>,
}

impl Offer {
    /// The offer in `text`, a line at a time: `Ok(None)` unless every line is
    /// understood, names only what may be written, and something is offered.
    ///
    /// # Errors
    /// `NoMemory` when the offer cannot be kept.
    pub fn parse(text: &str) -> Result<Option<Offer>, Error> {
        if text.len() as u64 > MAX_OFFER {
            return Ok(None);
        }
        let mut offer = Offer::default();
        let mut lines = 0;
        for line in text.split('\n') {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            lines += 1;
            if lines > MAX_FILES || line.len() > MAX_LINE || offer.take(line)?.is_none() {
                return Ok(None);
            }
        }
        Ok(offer.finish())
    }

    /// The offer once every line was taken: `None` where nothing is offered.
    #[must_use]
    pub fn finish(self) -> Option<Offer> {
        let empty = self.plugin.as_ref().is_none_or(|(_, f)| f.is_empty());
        (!(empty && self.books.is_empty())).then_some(self)
    }

    /// Takes a line of the offer (already trimmed, at most [`MAX_LINE`] bytes,
    /// and not more than [`MAX_FILES`] of them: the caller counts):
    /// `Ok(None)` where it is not understood.
    ///
    /// # Errors
    /// `NoMemory` when the line cannot be kept.
    pub fn take(&mut self, line: &str) -> Result<Option<()>, Error> {
        let Some((word, rest)) = line.split_once(' ') else {
            return Ok(None);
        };
        match word {
            "plugin" if self.plugin.is_none() => {
                let id = rest.trim();
                if !is_plugin_name(id) {
                    return Ok(None);
                }
                self.plugin = Some((copy(id)?, Vec::new()));
            }
            "book" => {
                let Some((name, size, sha256)) = sized(rest) else {
                    return Ok(None);
                };
                if !is_book_name(name) || self.books.len() >= MAX_BOOKS {
                    return Ok(None);
                }
                push(&mut self.books, (name_key(name), size, sha256))?;
                if self.named.len() < NAMED {
                    push(&mut self.named, (copy(name)?, size))?;
                }
            }
            path => {
                let Some((_, plugin_files)) = self.plugin.as_mut() else {
                    return Ok(None);
                };
                let Some((name, size, sha256)) = sized(line) else {
                    return Ok(None);
                };
                if name != path || !is_safe_path(path) || plugin_files.len() >= install::MAX_FILES {
                    return Ok(None);
                }
                push(plugin_files, (copy(name)?, size, sha256))?;
            }
        }
        Ok(Some(()))
    }

    /// The bytes of all of it; `None` past what can be counted.
    pub fn size(&self) -> Option<u64> {
        let plugin = self.plugin.iter().flat_map(|(_, files)| files.iter());
        let books = self.books.iter().map(|(_, size, _)| *size);
        plugin
            .map(|(_, size, _)| *size)
            .chain(books)
            .try_fold(0u64, u64::checked_add)
    }

    /// Whether the plugin offered has code.
    pub fn code(&self) -> bool {
        self.plugin.as_ref().is_some_and(|(_, files)| {
            files
                .iter()
                .any(|(path, ..)| CODE_FILES.contains(&path.as_str()))
        })
    }

    /// The size and SHA-256 offered of file `path` of plugin `id`.
    pub fn plugin_file(&self, id: &str, path: &str) -> Option<(u64, [u8; 32])> {
        let (offered, files) = self.plugin.as_ref()?;
        (offered == id).then_some(())?;
        files
            .iter()
            .find(|(p, ..)| p == path)
            .map(|(_, size, sha)| (*size, *sha))
    }

    /// The size and SHA-256 offered of book `name`.
    pub fn book(&self, name: &str) -> Option<(u64, [u8; 32])> {
        let key = name_key(name);
        let found = self.books.iter().find(|(k, ..)| *k == key);
        found.map(|(_, size, sha)| (*size, *sha))
    }
}

/// `<name> <size> <sha256>`: a book's name may hold spaces.
fn sized(rest: &str) -> Option<(&str, u64, [u8; 32])> {
    let (rest, sha256) = rest.rsplit_once(' ')?;
    let (name, size) = rest.rsplit_once(' ')?;
    Some((name.trim(), size.parse().ok()?, digest(sha256)?))
}

/// The 32 bytes of 64 hex digits (either case), without allocating.
fn digest(hex: &str) -> Option<[u8; 32]> {
    let text = hex.as_bytes();
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(text.as_chunks::<2>().0) {
        let digit = |c: u8| char::from(c).to_digit(16);
        *byte = (digit(pair[0])? << 4 | digit(pair[1])?) as u8;
    }
    Some(out)
}
