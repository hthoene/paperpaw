//! What is kept on the card (`.paperpaw/plugins/mail/…`): the folders of
//! an account, a folder's list of messages (headers only), its counts, and
//! each message read, as text. A cache: everything in it comes again from
//! the server, so a file that cannot be read is no loss.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{Env, Storage, StorageError};

use crate::header;
use crate::imap;

/// The plugin's folder on the card.
pub fn root() -> String {
    Env::plugin_dir("mail")
}

/// The files of one account (1 to [`MAX_ACCOUNTS`]).
pub fn account_dir(account: u8) -> String {
    format!("{}/a{account}", root())
}

/// The most accounts.
pub const MAX_ACCOUNTS: u8 = 3;
/// The most messages in a folder's list.
pub const MAX_MESSAGES: u32 = 50;
/// The most folders listed.
pub const MAX_FOLDERS: usize = 64;
/// The most text of a message kept.
/// The longest name of a folder taken (as the server names it).
pub const MAX_FOLDER_NAME: usize = 255;
pub const MAX_TEXT: usize = 24 * 1024;
/// The most the header of a letter takes: four lists of addresses, the
/// ids, the subject, the names of attachments.
const LETTER_HEAD: usize = 24 * 1024;
/// The most of a message fetched from the server (the rest is cut).
pub const MAX_FETCH: usize = 256 * 1024;

/// A name that is safe as a file name and the same for the same folder:
/// letters and digits of it, and its hash.
pub fn folder_key(raw_name: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for b in raw_name.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let mut key = String::from("f");
    key.extend(
        raw_name
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(12),
    );
    key.push('-');
    key.push_str(&format!("{hash:08x}"));
    key
}

pub fn folder_dir(account: u8, raw_name: &str) -> String {
    format!("{}/{}", account_dir(account), folder_key(raw_name))
}

pub fn folders_file(account: u8) -> String {
    format!("{}/folders.txt", account_dir(account))
}

pub fn index_file(account: u8, raw_name: &str) -> String {
    format!("{}/index.txt", folder_dir(account, raw_name))
}

pub fn status_file(account: u8, raw_name: &str) -> String {
    format!("{}/status.txt", folder_dir(account, raw_name))
}

/// The message as the server sent it (removed once it is text).
pub fn raw_file(account: u8, raw_name: &str, uid: u32) -> String {
    format!("{}/m{uid}.eml", folder_dir(account, raw_name))
}

/// The message as text (its headers in `m<uid>.txt`'s first lines, see
/// [`Letter`]).
pub fn text_file(account: u8, raw_name: &str, uid: u32) -> String {
    format!("{}/m{uid}.txt", folder_dir(account, raw_name))
}

pub fn draft_file(account: u8) -> String {
    format!("{}/draft.txt", account_dir(account))
}

pub fn outbox_file(account: u8) -> String {
    format!("{}/outbox.eml", account_dir(account))
}

/// Text on one line of a tab-separated file: tabs and line breaks go.
pub fn field(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\t' || c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .into()
}

/// One message of a folder's list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    pub uid: u32,
    /// `S` seen, `F` flagged, `A` answered (see [`imap::flag_letters`]).
    pub flags: String,
    /// Seconds since 1970, 0 where unknown.
    pub date: i64,
    pub size: u64,
    /// The sender, as shown (name, else address) and the address.
    pub from: String,
    pub from_address: String,
    pub subject: String,
    pub message_id: String,
}

impl Row {
    pub fn seen(&self) -> bool {
        self.flags.contains('S')
    }

    pub fn flagged(&self) -> bool {
        self.flags.contains('F')
    }

    pub fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            self.uid,
            field(&self.flags),
            self.date,
            self.size,
            field(&self.from),
            field(&self.from_address),
            field(&self.subject),
            field(&self.message_id),
        )
    }

    pub fn parse(line: &str) -> Option<Self> {
        let mut f = line.split('\t');
        let uid = f.next()?.parse().ok()?;
        Some(Self {
            uid,
            flags: String::from(f.next()?),
            date: f.next()?.parse().ok()?,
            size: f.next()?.parse().ok()?,
            from: String::from(f.next()?),
            from_address: String::from(f.next()?),
            subject: String::from(f.next()?),
            message_id: String::from(f.next().unwrap_or("")),
        })
    }

    /// A row from the headers a FETCH gave.
    pub fn from_headers(uid: u32, flags: &[String], date: i64, size: u64, raw: &[u8]) -> Self {
        let headers = header::Headers::parse(raw);
        let from = header::addresses(headers.get("from").unwrap_or(""))
            .into_iter()
            .next();
        let date = headers
            .get("date")
            .and_then(header::date_seconds)
            .unwrap_or(date);
        Self {
            uid,
            flags: imap::flag_letters(flags),
            date,
            size,
            from: from
                .as_ref()
                .map(|a| String::from(a.shown()))
                .unwrap_or_default(),
            from_address: from.map(|a| a.address).unwrap_or_default(),
            subject: header::capped(
                headers.get("subject").unwrap_or("").trim(),
                header::MAX_SUBJECT,
            ),
            message_id: header::capped(
                headers.get("message-id").unwrap_or("").trim(),
                header::MAX_ID,
            ),
        }
    }
}

/// Writes `data` to `file` so that a power cut leaves the old file or the
/// new, never half of it: to `<file>.part`, then moved over it.
pub fn write_atomic(storage: &dyn Storage, file: &str, data: &[u8]) -> Result<(), StorageError> {
    ensure_parent(storage, file)?;
    let part = format!("{file}.part");
    storage.write(&part, data)?;
    match storage.rename(&part, file) {
        Ok(()) => Ok(()),
        // A storage that cannot move files writes in place.
        Err(StorageError::Unsupported) => {
            let _ = storage.remove(&part);
            storage.write(file, data)
        }
        Err(e) => {
            let _ = storage.remove(&part);
            Err(e)
        }
    }
}

/// The list of a folder, newest first, as the file keeps it; rows that
/// cannot be read are left out.
pub fn read_index(storage: &dyn Storage, file: &str) -> Vec<Row> {
    let Ok(text) = paperpaw_sdk::files::read_text(storage, file, 64 * 1024) else {
        return Vec::new();
    };
    text.lines().filter_map(Row::parse).collect()
}

/// Which write of the index file it is, counted by every write, so that
/// those who keep what they read from it know when it changed (a file's
/// time and size may stay as they were where the device has no clock).
pub fn index_serial(storage: &dyn Storage, file: &str) -> u64 {
    let mut head = [0u8; 40];
    let Ok(n) = storage.read_at(file, 0, &mut head) else {
        return 0;
    };
    core::str::from_utf8(&head[..n])
        .ok()
        .and_then(|t| t.lines().next())
        .and_then(|l| l.strip_prefix(SERIAL_MARK))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

const SERIAL_MARK: &str = "#serial ";

pub fn write_index(storage: &dyn Storage, file: &str, rows: &[Row]) -> Result<(), StorageError> {
    let mut text = format!("{SERIAL_MARK}{}\n", index_serial(storage, file) + 1);
    for row in rows.iter().take(MAX_MESSAGES as usize) {
        text.push_str(&row.line());
    }
    write_atomic(storage, file, text.as_bytes())
}

/// What a folder holds, as of its last sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub messages: u32,
    pub unseen: u32,
    pub uid_validity: u64,
    /// Seconds since 1970 of the sync, 0 where the clock was not known.
    pub checked: i64,
}

impl Counts {
    pub fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\n",
            self.messages, self.unseen, self.uid_validity, self.checked
        )
    }

    pub fn read(storage: &dyn Storage, file: &str) -> Option<Self> {
        let text = paperpaw_sdk::files::read_text(storage, file, 256).ok()?;
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
pub struct Folder {
    /// As the server names it (what its commands take).
    pub raw: String,
    /// As the reader sees it.
    pub name: String,
    /// `\Sent`, `\Drafts`, `\Trash`, `\Junk`, `\Noselect` … in lower case.
    pub flags: Vec<String>,
}

impl Folder {
    pub fn selectable(&self) -> bool {
        !self
            .flags
            .iter()
            .any(|f| f == "\\noselect" || f == "\\nonexistent")
    }

    /// Whether this is the inbox (the name is `INBOX` in any case).
    pub fn is_inbox(&self) -> bool {
        self.raw.eq_ignore_ascii_case("INBOX")
    }
}

pub fn read_folders(storage: &dyn Storage, file: &str) -> Vec<Folder> {
    let Ok(text) = paperpaw_sdk::files::read_text(storage, file, 32 * 1024) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let raw = f.next()?;
            let flags = f.next().unwrap_or("");
            (!raw.is_empty()).then(|| Folder {
                raw: String::from(raw),
                name: imap::mailbox_display(raw),
                flags: flags
                    .split(' ')
                    .filter(|f| !f.is_empty())
                    .map(String::from)
                    .collect(),
            })
        })
        .collect()
}

pub fn write_folders(
    storage: &dyn Storage,
    file: &str,
    folders: &[Folder],
) -> Result<(), StorageError> {
    let mut text = String::new();
    for folder in folders.iter().take(MAX_FOLDERS) {
        text.push_str(&field(&folder.raw));
        text.push('\t');
        text.push_str(&field(&folder.flags.join(" ")));
        text.push('\n');
    }
    write_atomic(storage, file, text.as_bytes())
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
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// The parent folder of `file` exists (a storage that makes none makes
/// the file's own).
pub fn ensure_parent(storage: &dyn Storage, file: &str) -> Result<(), StorageError> {
    if let Some((parent, _)) = file.rsplit_once('/') {
        let mut path = String::new();
        for part in parent.split('/') {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            if !storage.exists(&path) {
                storage.create_dir(&path)?;
            }
        }
    }
    Ok(())
}

/// A message read, as `m<uid>.txt` keeps it: its headers on top, a blank
/// line, the text. What the reader's screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Letter {
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date: i64,
    pub message_id: String,
    pub references: String,
    pub reply_to: String,
    pub attachments: Vec<String>,
    pub truncated: bool,
    pub text: String,
}

impl Letter {
    pub fn header_text(&self) -> String {
        let mut out = String::new();
        let mut put = |name: &str, value: &str| {
            out.push_str(name);
            out.push_str(": ");
            out.push_str(&field(value));
            out.push('\n');
        };
        put("From", &self.from);
        put("To", &self.to);
        put("Cc", &self.cc);
        put("Subject", &self.subject);
        put("Date", &alloc::format!("{}", self.date));
        put("Message-ID", &self.message_id);
        put("References", &self.references);
        put("Reply-To", &self.reply_to);
        put(
            "Attachments",
            &self
                .attachments
                .iter()
                .map(|a| field(a))
                .collect::<Vec<_>>()
                .join("\t"),
        );
        put("Truncated", if self.truncated { "1" } else { "0" });
        out.push('\n');
        out
    }

    pub fn parse(text: &str) -> Self {
        let (head, body) = text.split_once("\n\n").unwrap_or((text, ""));
        let mut letter = Letter {
            text: String::from(body),
            ..Self::default()
        };
        for line in head.lines() {
            let Some((name, value)) = line.split_once(": ") else {
                continue;
            };
            let value = String::from(value);
            match name {
                "From" => letter.from = value,
                "To" => letter.to = value,
                "Cc" => letter.cc = value,
                "Subject" => letter.subject = value,
                "Date" => letter.date = value.parse().unwrap_or(0),
                "Message-ID" => letter.message_id = value,
                "References" => letter.references = value,
                "Reply-To" => letter.reply_to = value,
                "Attachments" => {
                    letter.attachments = value
                        .split('\t')
                        .filter(|a| !a.is_empty())
                        .map(String::from)
                        .collect()
                }
                "Truncated" => letter.truncated = value == "1",
                _ => {}
            }
        }
        letter
    }
}

/// The message in `raw` as a [`Letter`] in `out` (and the raw message
/// removed): what the reader's screen shows.
pub fn make_letter(storage: &dyn Storage, raw: &str, out: &str) -> Result<Letter, StorageError> {
    let scratch = format!("{out}.part");
    let info = crate::render::render(storage, raw, &scratch, MAX_TEXT)?;
    let list = |list: &[header::Address]| {
        list.iter()
            .map(|a| {
                if a.name.is_empty() {
                    a.address.clone()
                } else {
                    format!("{} <{}>", a.name, a.address)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let list = |l: &[header::Address]| header::capped(&list(l), header::MAX_LIST);
    let mut letter = Letter {
        from: list(&info.from),
        to: list(&info.to),
        cc: list(&info.cc),
        reply_to: list(&info.reply_to),
        subject: info.subject.clone(),
        date: info.date.unwrap_or(0),
        message_id: info.message_id.clone(),
        references: info.references.clone(),
        attachments: info.attachments.clone(),
        truncated: info.truncated,
        text: String::new(),
    };
    let body = paperpaw_sdk::files::read_text(storage, &scratch, MAX_TEXT + 16)?;
    letter.text = body;
    let mut file = letter.header_text();
    file.push_str(&letter.text);
    write_atomic(storage, out, file.as_bytes())?;
    let _ = storage.remove(&scratch);
    let _ = storage.remove(raw);
    Ok(letter)
}

pub fn read_letter(storage: &dyn Storage, file: &str) -> Option<Letter> {
    let text = paperpaw_sdk::files::read_text(storage, file, MAX_TEXT + LETTER_HEAD).ok()?;
    Some(Letter::parse(&text))
}
