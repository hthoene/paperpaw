//! What Mail keeps in its data folder, for the native plugin: the interface-6
//! `store` made fallible and bounded. The folders of an account, a folder's
//! list of messages (headers only), its counts, and each message read, as
//! text. A cache: everything in it comes again from the server, so a file
//! that cannot be read is no loss.
//!
//! What differs from interface 6, on purpose:
//!
//! - the paths are relative to the plugin's data folder (storage root 0,
//!   `core.files` is not needed for them): `a1/f<name>-<hash>/index.txt`,
//!   not `.paperpaw/plugins/mail/a1/…` on the card. The data folder *is*
//!   `.paperpaw/plugins/mail/` (`docs/abi/files.md` 1.1), so the files of
//!   an interface-6 Mail are in place: the cache is read or comes again, and
//!   the drafts and outboxes (`aN/draft.txt`, `aN/outbox.eml`, the user's own
//!   writing) are found at the same paths, so nothing is copied or removed;
//! - a write is [`Storage::write`] of the data folder, which makes the
//!   parent folders, and is the old file or the new one whole (a write over
//!   4 KiB is staged beside the file by the storage): there is no `.part`
//!   file of Mail's own;
//! - every file is read with a cap, and a refused allocation is
//!   [`Fail::NoMemory`], never an abort and never taken for a card that is
//!   gone;
//! - a list of folders is sorted by lower-case characters without making a
//!   lower-case copy of each name for every comparison (a word-final sigma
//!   sorts as `σ`, where interface 6's `to_lowercase` made it `ς`).
use super::fail::Fail;
use super::header;
use super::imap;
use super::mem::{copy, push, push_char, push_str, string_with};
use super::render::render;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{Storage, StorageError};
use paperpaw_ui_kit::mem::join;
use paperpaw_ui_kit::text::{self, cmp_lowercase};

/// The most accounts.
pub const MAX_ACCOUNTS: u8 = 3;
/// The most messages in a folder's list.
pub const MAX_MESSAGES: u32 = 50;
/// The most folders listed.
pub const MAX_FOLDERS: usize = 64;
/// The longest name of a folder taken (as the server names it).
pub const MAX_FOLDER_NAME: usize = 255;
/// The most text of a message kept.
pub const MAX_TEXT: usize = 24 * 1024;
/// The most a letter's header takes: four lists of addresses, the ids, the
/// subject, the names of attachments.
const LETTER_HEAD: usize = 24 * 1024;
/// The most of a message fetched from the server (the rest is cut).
pub const MAX_FETCH: usize = 256 * 1024;
/// The most bytes of a folder's index read.
const MAX_INDEX: usize = 64 * 1024;
/// The most bytes of the list of folders read.
const MAX_FOLDERS_FILE: usize = 32 * 1024;
/// The most bytes of a folder's counts read.
const MAX_COUNTS: usize = 256;

/// The files of one account (1 to [`MAX_ACCOUNTS`]).
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn account_dir(account: u8) -> Result<String, Fail> {
    Ok(text::format(format_args!("a{account}"))?)
}

/// A name that is safe as a file name and the same for the same folder:
/// letters and digits of it, and its hash.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn folder_key(raw_name: &str) -> Result<String, Fail> {
    let mut hash: u32 = 0x811c_9dc5;
    for b in raw_name.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let mut key = string_with(24)?;
    push_char(&mut key, 'f')?;
    for c in raw_name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
    {
        push_char(&mut key, c)?;
    }
    text::write_to(&mut key, format_args!("-{hash:08x}"))?;
    Ok(key)
}

/// The folder of one mail folder of an account.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn folder_dir(account: u8, raw_name: &str) -> Result<String, Fail> {
    Ok(text::format(format_args!(
        "{}/{}",
        account_dir(account)?,
        folder_key(raw_name)?
    ))?)
}

/// What an account lists as its folders.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn folders_file(account: u8) -> Result<String, Fail> {
    Ok(text::format(format_args!(
        "{}/folders.txt",
        account_dir(account)?
    ))?)
}

/// A folder's list of messages.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn index_file(account: u8, raw_name: &str) -> Result<String, Fail> {
    Ok(text::format(format_args!(
        "{}/index.txt",
        folder_dir(account, raw_name)?
    ))?)
}

/// A folder's counts.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn status_file(account: u8, raw_name: &str) -> Result<String, Fail> {
    Ok(text::format(format_args!(
        "{}/status.txt",
        folder_dir(account, raw_name)?
    ))?)
}

/// The message as the server sent it (removed once it is text).
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn raw_file(account: u8, raw_name: &str, uid: u32) -> Result<String, Fail> {
    Ok(text::format(format_args!(
        "{}/m{uid}.eml",
        folder_dir(account, raw_name)?
    ))?)
}

/// The message as text (its headers in `m<uid>.txt`'s first lines, see
/// [`Letter`]).
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn text_file(account: u8, raw_name: &str, uid: u32) -> Result<String, Fail> {
    Ok(text::format(format_args!(
        "{}/m{uid}.txt",
        folder_dir(account, raw_name)?
    ))?)
}

/// The draft of an account.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn draft_file(account: u8) -> Result<String, Fail> {
    Ok(text::format(format_args!(
        "{}/draft.txt",
        account_dir(account)?
    ))?)
}

/// The message waiting to be sent.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn outbox_file(account: u8) -> Result<String, Fail> {
    Ok(text::format(format_args!(
        "{}/outbox.eml",
        account_dir(account)?
    ))?)
}

/// Text on one line of a tab-separated file: tabs and line breaks go.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn field(value: &str) -> Result<String, Fail> {
    let mut out = string_with(value.len())?;
    for c in value.chars() {
        push_char(&mut out, if c == '\t' || c.is_control() { ' ' } else { c })?;
    }
    let trimmed = out.trim();
    if trimmed.len() == out.len() {
        return Ok(out);
    }
    Ok(copy(trimmed)?)
}

/// Appends `value` as a [`field`] to `out`.
fn push_field(out: &mut String, value: &str) -> Result<(), Fail> {
    push_str(out, &field(value)?)?;
    Ok(())
}

/// The whole of a file as text, at most `cap` bytes (a byte order mark is
/// dropped). A file over the cap is [`StorageError::TooLarge`], a refused
/// allocation is [`Fail::NoMemory`].
fn read_text(storage: &dyn Storage, path: &str, cap: usize) -> Result<String, Fail> {
    let size = usize::try_from(storage.size(path)?).unwrap_or(usize::MAX);
    if size > cap {
        return Err(StorageError::TooLarge.into());
    }
    let mut data: Vec<u8> = Vec::new();
    data.try_reserve_exact(size).map_err(|_| Fail::NoMemory)?;
    data.resize(size, 0);
    let mut filled = 0;
    while filled < size {
        let Some(room) = data.get_mut(filled..) else {
            break;
        };
        let n = storage.read_at(path, filled as u64, room)?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    data.truncate(filled);
    if data.starts_with(b"\xEF\xBB\xBF") {
        data.drain(..3);
    }
    String::from_utf8(data).map_err(|_| StorageError::InvalidData.into())
}

/// One message of a folder's list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Row {
    /// The server's id of the message in its folder.
    pub uid: u32,
    /// `S` seen, `F` flagged, `A` answered (see [`imap::flag_letters`]).
    pub flags: String,
    /// Seconds since 1970, 0 where unknown.
    pub date: i64,
    /// Bytes, as the server says.
    pub size: u64,
    /// The sender, as shown (name, else address) …
    pub from: String,
    /// … and the address.
    pub from_address: String,
    /// The subject.
    pub subject: String,
    /// The message's own id.
    pub message_id: String,
}

impl Row {
    /// Whether the message was read.
    #[must_use]
    pub fn seen(&self) -> bool {
        self.flags.contains('S')
    }

    /// Whether the message is flagged.
    #[must_use]
    pub fn flagged(&self) -> bool {
        self.flags.contains('F')
    }

    /// The row as the index file keeps it.
    ///
    /// # Errors
    /// [`Fail::NoMemory`].
    pub fn line(&self) -> Result<String, Fail> {
        let mut out = string_with(64 + self.subject.len() + self.from.len())?;
        text::write_to(&mut out, format_args!("{}\t", self.uid))?;
        push_field(&mut out, &self.flags)?;
        text::write_to(&mut out, format_args!("\t{}\t{}\t", self.date, self.size))?;
        push_field(&mut out, &self.from)?;
        push_char(&mut out, '\t')?;
        push_field(&mut out, &self.from_address)?;
        push_char(&mut out, '\t')?;
        push_field(&mut out, &self.subject)?;
        push_char(&mut out, '\t')?;
        push_field(&mut out, &self.message_id)?;
        push_char(&mut out, '\n')?;
        Ok(out)
    }

    /// A row from a line of the index (`None` where it cannot be read).
    ///
    /// # Errors
    /// [`Fail::NoMemory`].
    pub fn parse(line: &str) -> Result<Option<Self>, Fail> {
        let mut f = line.split('\t');
        let Some(uid) = f.next().and_then(|v| v.parse().ok()) else {
            return Ok(None);
        };
        let Some(flags) = f.next() else {
            return Ok(None);
        };
        let Some(date) = f.next().and_then(|v| v.parse().ok()) else {
            return Ok(None);
        };
        let Some(size) = f.next().and_then(|v| v.parse().ok()) else {
            return Ok(None);
        };
        let (Some(from), Some(from_address), Some(subject)) = (f.next(), f.next(), f.next()) else {
            return Ok(None);
        };
        Ok(Some(Self {
            uid,
            flags: copy(flags)?,
            date,
            size,
            from: copy(from)?,
            from_address: copy(from_address)?,
            subject: copy(subject)?,
            message_id: copy(f.next().unwrap_or(""))?,
        }))
    }

    /// A row from the headers a FETCH gave.
    ///
    /// # Errors
    /// [`Fail::NoMemory`].
    pub fn from_headers(
        uid: u32,
        flags: &[String],
        date: i64,
        size: u64,
        raw: &[u8],
    ) -> Result<Self, Fail> {
        let headers = header::Headers::parse(raw)?;
        let from = header::addresses(headers.get("from").unwrap_or(""))?
            .into_iter()
            .next();
        let date = headers
            .get("date")
            .and_then(header::date_seconds)
            .unwrap_or(date);
        let (from_name, from_address) = match from {
            Some(address) => {
                let shown = copy(address.shown())?;
                (shown, address.address)
            }
            None => (String::new(), String::new()),
        };
        Ok(Self {
            uid,
            flags: imap::flag_letters(flags)?,
            date,
            size,
            from: from_name,
            from_address,
            subject: header::capped(
                headers.get("subject").unwrap_or("").trim(),
                header::MAX_SUBJECT,
            )?,
            message_id: header::capped(
                headers.get("message-id").unwrap_or("").trim(),
                header::MAX_ID,
            )?,
        })
    }
}

/// Writes `data` to `file` so that a power cut leaves the old file or the
/// new, never half of it (the storage of the data folder makes the folders
/// and does it).
///
/// # Errors
/// [`Fail::Storage`].
pub fn write_atomic(storage: &dyn Storage, file: &str, data: &[u8]) -> Result<(), Fail> {
    Ok(storage.write(file, data)?)
}

/// The list of a folder, newest first, as the file keeps it; rows that
/// cannot be read are left out, and a file that cannot be read is an empty
/// list.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn read_index(storage: &dyn Storage, file: &str) -> Result<Vec<Row>, Fail> {
    let text = match read_text(storage, file, MAX_INDEX) {
        Ok(text) => text,
        Err(Fail::NoMemory) => return Err(Fail::NoMemory),
        Err(_) => return Ok(Vec::new()),
    };
    let mut rows = Vec::new();
    for line in text.lines() {
        // A list is at most what a write keeps: a damaged file of many
        // short lines must not make many rows.
        if rows.len() >= MAX_MESSAGES as usize {
            break;
        }
        if let Some(row) = Row::parse(line)? {
            push(&mut rows, row)?;
        }
    }
    Ok(rows)
}

const SERIAL_MARK: &str = "#serial ";

/// Which write of the index file it is, counted by every write, so that
/// those who keep what they read from it know when it changed (a file's
/// time and size may stay as they were where the device has no clock).
pub fn index_serial(storage: &dyn Storage, file: &str) -> u64 {
    let mut head = [0u8; 40];
    let Ok(n) = storage.read_at(file, 0, &mut head) else {
        return 0;
    };
    head.get(..n)
        .and_then(|head| core::str::from_utf8(head).ok())
        .and_then(|t| t.lines().next())
        .and_then(|l| l.strip_prefix(SERIAL_MARK))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

/// Writes the list of a folder (at most [`MAX_MESSAGES`] rows).
///
/// # Errors
/// [`Fail::Storage`], [`Fail::NoMemory`].
pub fn write_index(storage: &dyn Storage, file: &str, rows: &[Row]) -> Result<(), Fail> {
    let mut out = string_with(256)?;
    text::write_to(
        &mut out,
        format_args!(
            "{SERIAL_MARK}{}\n",
            index_serial(storage, file).saturating_add(1)
        ),
    )?;
    for row in rows.iter().take(MAX_MESSAGES as usize) {
        push_str(&mut out, &row.line()?)?;
    }
    write_atomic(storage, file, out.as_bytes())
}

/// What a folder holds, as of its last sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Counts {
    /// Messages in the folder.
    pub messages: u32,
    /// Messages not seen.
    pub unseen: u32,
    /// The server's `UIDVALIDITY`.
    pub uid_validity: u64,
    /// Seconds since 1970 of the sync, 0 where the clock was not known.
    pub checked: i64,
}

impl Counts {
    /// The counts as the status file keeps them.
    ///
    /// # Errors
    /// [`Fail::NoMemory`].
    pub fn line(&self) -> Result<String, Fail> {
        Ok(text::format(format_args!(
            "{}\t{}\t{}\t{}\n",
            self.messages, self.unseen, self.uid_validity, self.checked
        ))?)
    }

    /// The counts of a folder (`None` where there are none or they cannot
    /// be read).
    pub fn read(storage: &dyn Storage, file: &str) -> Option<Self> {
        let text = read_text(storage, file, MAX_COUNTS).ok()?;
        let mut f = text.lines().next()?.split('\t');
        Some(Self {
            messages: f.next()?.parse().ok()?,
            unseen: f.next()?.parse().ok()?,
            uid_validity: f.next()?.parse().ok()?,
            checked: f.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        })
    }
}

/// A folder of an account.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Folder {
    /// As the server names it (what its commands take).
    pub raw: String,
    /// As the reader sees it.
    pub name: String,
    /// `\Sent`, `\Drafts`, `\Trash`, `\Junk`, `\Noselect` … in lower case.
    pub flags: Vec<String>,
}

impl Folder {
    /// A folder named `raw` by the server with `flags`.
    ///
    /// # Errors
    /// [`Fail::NoMemory`].
    pub fn new(raw: &str, flags: Vec<String>) -> Result<Self, Fail> {
        Ok(Self {
            raw: copy(raw)?,
            name: imap::mailbox_display(raw)?,
            flags,
        })
    }

    /// Whether commands can select it.
    #[must_use]
    pub fn selectable(&self) -> bool {
        !self
            .flags
            .iter()
            .any(|f| f == "\\noselect" || f == "\\nonexistent")
    }

    /// Whether this is the inbox (the name is `INBOX` in any case).
    #[must_use]
    pub fn is_inbox(&self) -> bool {
        self.raw.eq_ignore_ascii_case("INBOX")
    }
}

/// The folders of an account, as the file keeps them (an unreadable file
/// is no folders).
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn read_folders(storage: &dyn Storage, file: &str) -> Result<Vec<Folder>, Fail> {
    let text = match read_text(storage, file, MAX_FOLDERS_FILE) {
        Ok(text) => text,
        Err(Fail::NoMemory) => return Err(Fail::NoMemory),
        Err(_) => return Ok(Vec::new()),
    };
    let mut folders = Vec::new();
    for line in text.lines() {
        if folders.len() >= MAX_FOLDERS {
            break;
        }
        let mut f = line.split('\t');
        let raw = f.next().unwrap_or("");
        if raw.is_empty() {
            continue;
        }
        let mut flags = Vec::new();
        for flag in f.next().unwrap_or("").split(' ').filter(|f| !f.is_empty()) {
            push(&mut flags, copy(flag)?)?;
        }
        push(&mut folders, Folder::new(raw, flags)?)?;
    }
    Ok(folders)
}

/// Writes the list of folders (at most [`MAX_FOLDERS`]).
///
/// # Errors
/// [`Fail::Storage`], [`Fail::NoMemory`].
pub fn write_folders(storage: &dyn Storage, file: &str, folders: &[Folder]) -> Result<(), Fail> {
    let mut out = String::new();
    for folder in folders.iter().take(MAX_FOLDERS) {
        push_field(&mut out, &folder.raw)?;
        push_char(&mut out, '\t')?;
        push_field(&mut out, &join(&folder.flags, " ")?)?;
        push_char(&mut out, '\n')?;
    }
    write_atomic(storage, file, out.as_bytes())
}

/// The folders sorted for the reader: the inbox first, then the special ones
/// (drafts, sent, …) as the server names them, then the rest by name.
pub fn sort_folders(folders: &mut [Folder]) {
    let rank = |f: &Folder| -> u8 {
        if f.is_inbox() {
            0
        } else if f.flags.iter().any(|x| {
            matches!(
                x.as_str(),
                "\\sent" | "\\drafts" | "\\trash" | "\\junk" | "\\archive" | "\\flagged"
            )
        }) {
            1
        } else {
            2
        }
    };
    paperpaw_sdk::sort::sort_by(folders, |a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| cmp_lowercase(&a.name, &b.name))
    });
}

/// A message read, as `m<uid>.txt` keeps it: its headers on top, a blank
/// line, the text. What the reader's screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Letter {
    /// The senders, written out.
    pub from: String,
    /// The recipients.
    pub to: String,
    /// The copies.
    pub cc: String,
    /// The subject.
    pub subject: String,
    /// Seconds since 1970, 0 where unknown.
    pub date: i64,
    /// The message's id.
    pub message_id: String,
    /// The ids it refers to.
    pub references: String,
    /// Where a reply goes.
    pub reply_to: String,
    /// `name (type)` of each attachment.
    pub attachments: Vec<String>,
    /// Whether the text was cut.
    pub truncated: bool,
    /// The text.
    pub text: String,
}

impl Letter {
    /// The headers as the file keeps them, with the blank line after them.
    ///
    /// # Errors
    /// [`Fail::NoMemory`].
    pub fn header_text(&self) -> Result<String, Fail> {
        let mut out = string_with(256)?;
        let mut put = |name: &str, value: &str| -> Result<(), Fail> {
            push_str(&mut out, name)?;
            push_str(&mut out, ": ")?;
            push_field(&mut out, value)?;
            Ok(push_char(&mut out, '\n')?)
        };
        put("From", &self.from)?;
        put("To", &self.to)?;
        put("Cc", &self.cc)?;
        put("Subject", &self.subject)?;
        put("Date", &text::format(format_args!("{}", self.date))?)?;
        put("Message-ID", &self.message_id)?;
        put("References", &self.references)?;
        put("Reply-To", &self.reply_to)?;
        let mut names = String::new();
        for (at, name) in self.attachments.iter().enumerate() {
            if at > 0 {
                push_char(&mut names, '\t')?;
            }
            push_field(&mut names, name)?;
        }
        put("Attachments", &names)?;
        put("Truncated", if self.truncated { "1" } else { "0" })?;
        push_char(&mut out, '\n')?;
        Ok(out)
    }

    /// A letter from the text of its file.
    ///
    /// # Errors
    /// [`Fail::NoMemory`].
    pub fn parse(text: &str) -> Result<Self, Fail> {
        let (head, body) = text.split_once("\n\n").unwrap_or((text, ""));
        let mut letter = Letter {
            text: copy(body)?,
            ..Self::default()
        };
        for line in head.lines() {
            let Some((name, value)) = line.split_once(": ") else {
                continue;
            };
            match name {
                "From" => letter.from = copy(value)?,
                "To" => letter.to = copy(value)?,
                "Cc" => letter.cc = copy(value)?,
                "Subject" => letter.subject = copy(value)?,
                "Date" => letter.date = value.parse().unwrap_or(0),
                "Message-ID" => letter.message_id = copy(value)?,
                "References" => letter.references = copy(value)?,
                "Reply-To" => letter.reply_to = copy(value)?,
                "Attachments" => {
                    let mut names = Vec::new();
                    for name in value.split('\t').filter(|a| !a.is_empty()) {
                        push(&mut names, copy(name)?)?;
                    }
                    letter.attachments = names;
                }
                "Truncated" => letter.truncated = value == "1",
                _ => {}
            }
        }
        Ok(letter)
    }
}

/// A list of addresses written out, cut to [`header::MAX_LIST`].
fn written(list: &[header::Address]) -> Result<String, Fail> {
    let mut out = String::new();
    for (at, a) in list.iter().enumerate() {
        if at > 0 {
            push_str(&mut out, ", ")?;
        }
        if a.name.is_empty() {
            push_str(&mut out, &a.address)?;
        } else {
            push_str(&mut out, &a.name)?;
            push_str(&mut out, " <")?;
            push_str(&mut out, &a.address)?;
            push_char(&mut out, '>')?;
        }
    }
    Ok(header::capped(&out, header::MAX_LIST)?)
}

/// The message in `raw` as a [`Letter`] in `out` (and the raw message
/// removed): what the reader's screen shows.
///
/// # Errors
/// [`Fail::Storage`], [`Fail::NoMemory`].
pub fn make_letter(storage: &dyn Storage, raw: &str, out: &str) -> Result<Letter, Fail> {
    let scratch = text::format(format_args!("{out}.tmp"))?;
    let info = render(storage, raw, &scratch, MAX_TEXT)?;
    let mut letter = Letter {
        from: written(&info.from)?,
        to: written(&info.to)?,
        cc: written(&info.cc)?,
        reply_to: written(&info.reply_to)?,
        subject: info.subject,
        date: info.date.unwrap_or(0),
        message_id: info.message_id,
        references: info.references,
        attachments: info.attachments,
        truncated: info.truncated,
        text: String::new(),
    };
    letter.text = read_text(storage, &scratch, MAX_TEXT + 16)?;
    let mut file = letter.header_text()?;
    push_str(&mut file, &letter.text)?;
    write_atomic(storage, out, file.as_bytes())?;
    storage.remove(&scratch).ok();
    storage.remove(raw).ok();
    Ok(letter)
}

/// The text of a file of at most `cap` bytes (`None` where there is none or it
/// cannot be read): the draft kept for an account.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn read_kept(storage: &dyn Storage, file: &str, cap: usize) -> Result<Option<String>, Fail> {
    match read_text(storage, file, cap) {
        Ok(text) => Ok(Some(text)),
        Err(Fail::NoMemory) => Err(Fail::NoMemory),
        Err(_) => Ok(None),
    }
}

/// A letter read from its file (`None` where there is none or it cannot be
/// read).
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn read_letter(storage: &dyn Storage, file: &str) -> Result<Option<Letter>, Fail> {
    match read_text(storage, file, MAX_TEXT + LETTER_HEAD) {
        Ok(text) => Ok(Some(Letter::parse(&text)?)),
        Err(Fail::NoMemory) => Err(Fail::NoMemory),
        Err(_) => Ok(None),
    }
}
