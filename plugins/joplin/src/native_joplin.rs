//! What the Joplin plugin decides without the host: the kinds of sync targets
//! and the addresses of their files, Joplin's ids and items (the properties
//! at the end of a note, the text of a notebook and of a note's properties),
//! and the title and author at the start of the notes a reader exports. The
//! same rules as the interface-6 plugin (`service.rs`, `item.rs`,
//! `markdown.rs`), with fallible allocation and the texts given by the
//! caller instead of read from a storage. Sending, the files, the locks and
//! the set-up screen are the native plugin's (see `PORTING-7.md`).
use alloc::string::String;
use paperpaw_sdk::digest::md5::Md5;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::time::UnixTime;

/// Joplin Cloud's address.
pub const CLOUD: &str = "https://api.joplincloud.com";
/// The version of the sync target's format these items are written for.
pub const SYNC_VERSION: u32 = 3;
/// `type_` of a note and of a notebook.
pub const NOTE: u8 = 1;
pub const NOTEBOOK: u8 = 2;

/// The kinds of Joplin sync targets reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Cloud,
    Server,
    WebDav,
}

impl Service {
    /// All of them, in the order offered.
    pub const ALL: [Service; 3] = [Service::Cloud, Service::Server, Service::WebDav];
    /// The setting's values (and their translation keys).
    pub const KEYS: [&'static str; 3] = [
        "joplin.service.cloud",
        "joplin.service.server",
        "joplin.service.webdav",
    ];

    /// The setting's value for it.
    pub fn key(self) -> &'static str {
        match self {
            Service::Cloud => Self::KEYS[0],
            Service::Server => Self::KEYS[1],
            Service::WebDav => Self::KEYS[2],
        }
    }

    /// The service the setting's value `key` names; Joplin Cloud for any
    /// other.
    pub fn of_key(key: &str) -> Service {
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or(Service::Cloud)
    }

    /// The one after it, for choosing by going through them.
    pub fn next(self) -> Service {
        match self {
            Service::Cloud => Service::Server,
            Service::Server => Service::WebDav,
            Service::WebDav => Service::Cloud,
        }
    }

    /// Whether it is reached at an address the user gives.
    pub fn has_address(self) -> bool {
        self != Service::Cloud
    }

    /// Whether it signs in first, for a session.
    pub fn signs_in(self) -> bool {
        self != Service::WebDav
    }

    /// The address of the file `name` (e.g. `info.json`) at `base`.
    ///
    /// # Errors
    /// `NoMemory` when the address cannot be kept.
    pub fn url(self, base: &str, name: &str) -> Result<String, Error> {
        match self {
            Service::WebDav => join(&[base, "/", name]),
            _ => join(&[base, "/api/items/root:/", name, ":/content"]),
        }
    }

    /// The address of the sync locks at `base`.
    ///
    /// # Errors
    /// `NoMemory` when the address cannot be kept.
    pub fn locks_url(self, base: &str) -> Result<String, Error> {
        match self {
            Service::WebDav => join(&[base, "/locks/"]),
            _ => join(&[base, "/api/locks"]),
        }
    }
}

/// The parts one after the other, in memory that may be refused.
fn join(parts: &[&str]) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len()).sum())
        .map_err(|_| Error::NoMemory)?;
    for part in parts {
        out.push_str(part);
    }
    Ok(out)
}

/// A new Joplin id (32 hex digits) from `seed`, which makes it unique (what
/// it is for and the time), and the id of the item it `replaces`, from which
/// it differs even if the clock gave the same time again.
///
/// # Errors
/// `NoMemory` when the id cannot be kept.
pub fn new_id(seed: &str, replaces: Option<&str>) -> Result<String, Error> {
    let mut md5 = Md5::new();
    md5.update(seed.as_bytes());
    if let Some(old) = replaces {
        md5.update(b" ");
        md5.update(old.as_bytes());
    }
    let mut digits = [0u8; 32];
    join(&[md5.hex_into(&mut digits)])
}

/// Whether `text` is a Joplin id.
pub fn is_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The file of item `id` in a sync target.
///
/// # Errors
/// `NoMemory` when the name cannot be kept.
pub fn file_name(id: &str) -> Result<String, Error> {
    join(&[id, ".md"])
}

/// A property's value on one line, escaped as Joplin escapes it: a line
/// break as `\n`, a `\n` written out as `\\n` (the same for `\r`).
fn escape_into(out: &mut String, value: &str) -> Result<(), Error> {
    let mut rest = value;
    while let Some(c) = rest.chars().next() {
        let piece = if let Some(after) = rest.strip_prefix("\\n") {
            rest = after;
            "\\\\n"
        } else if let Some(after) = rest.strip_prefix("\\r") {
            rest = after;
            "\\\\r"
        } else if c == '\n' {
            rest = &rest[1..];
            "\\n"
        } else if c == '\r' {
            rest = &rest[1..];
            "\\r"
        } else {
            let len = c.len_utf8();
            let (one, after) = rest.split_at(len);
            rest = after;
            one
        };
        out.try_reserve(piece.len()).map_err(|_| Error::NoMemory)?;
        out.push_str(piece);
    }
    Ok(())
}

/// A title on one line, with no blank around it, appended to `out`.
fn one_line_into(out: &mut String, title: &str) -> Result<(), Error> {
    for (i, word) in title.split_whitespace().enumerate() {
        let gap = usize::from(i > 0);
        out.try_reserve(gap + word.len())
            .map_err(|_| Error::NoMemory)?;
        if i > 0 {
            out.push(' ');
        }
        out.push_str(word);
    }
    Ok(())
}

/// The properties of an item that matter here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Props {
    pub kind: u8,
    pub parent: String,
    pub created: String,
    /// When it changed last, in ms.
    pub updated: i64,
    /// When it was made as the user sees it.
    pub user_created: String,
    /// Its place in a notebook sorted by hand.
    pub order: i64,
    /// In Joplin's trash.
    pub deleted: bool,
    /// Encrypted (Joplin's end-to-end encryption).
    pub encrypted: bool,
    /// Where its text (title and body) ends in its file: at the blank line
    /// before the properties ([`item_props`] only).
    pub text_end: u64,
}

/// How much of the end of an item's file is read for its properties, as in
/// interface 6: only the end, however long the body. Properties further from
/// the end are not seen.
pub const TAIL: usize = 2048;
/// How much of the start of a notes file is read for its title and author, as
/// in interface 6.
pub const HEAD: usize = 1024;

/// `bytes` as text, an invalid sequence replaced by U+FFFD as
/// `String::from_utf8_lossy` does, in memory that may be refused.
fn lossy(bytes: &[u8]) -> Result<String, Error> {
    let size = bytes
        .utf8_chunks()
        .map(|c| c.valid().len() + 3 * usize::from(!c.invalid().is_empty()))
        .sum();
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            out.push('\u{fffd}');
        }
    }
    Ok(out)
}

/// The properties of the item whose file is `size` bytes long and ends in
/// `tail`, and where its text ends. Only the last [`TAIL`] bytes count: a
/// longer `tail` is cut from the front, so the caller may give what it read
/// of the end, but need not read more.
///
/// # Errors
/// `NoMemory` when the values cannot be kept.
pub fn item_props(size: u64, tail: &[u8]) -> Result<Props, Error> {
    let from = size.saturating_sub(TAIL as u64);
    // What the file has of its end, and no more than it has.
    let have = usize::try_from(size - from).unwrap_or(TAIL).min(tail.len());
    let tail = tail[tail.len() - have..].trim_ascii_end();
    let blank = tail.windows(2).rposition(|w| w == b"\n\n").unwrap_or(0);
    Ok(Props {
        text_end: from + blank as u64,
        ..parse_props(&lossy(tail)?)?
    })
}

/// The title and author at the start of a notes file whose first bytes are
/// `start`. Only the first [`HEAD`] bytes count.
///
/// # Errors
/// `NoMemory` when the texts cannot be kept.
pub fn head_of(start: &[u8]) -> Result<Head, Error> {
    parse_head(&lossy(&start[..start.len().min(HEAD)])?)
}

fn copy(text: &str) -> Result<String, Error> {
    join(&[text])
}

/// The properties at the end of an item's text.
///
/// # Errors
/// `NoMemory` when the values cannot be kept.
pub fn parse_props(text: &str) -> Result<Props, Error> {
    let block = text.trim_end().rsplit("\n\n").next().unwrap_or_default();
    let mut props = Props::default();
    for line in block.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "type_" => props.kind = value.parse().unwrap_or(0),
            "parent_id" => props.parent = copy(value)?,
            "created_time" => props.created = copy(value)?,
            "updated_time" => {
                props.updated = UnixTime::parse_iso8601(value).map_or(0, UnixTime::millis);
            }
            "user_created_time" => props.user_created = copy(value)?,
            "order" => props.order = value.parse().unwrap_or(0),
            "deleted_time" => props.deleted = !matches!(value, "" | "0"),
            "encryption_applied" => props.encrypted = value == "1",
            _ => {}
        }
    }
    Ok(props)
}

/// A notebook named `title`, made at `now` (a time as Joplin writes them),
/// in no other.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn notebook(id: &str, title: &str, now: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(300 + id.len() + title.len() + 4 * now.len())
        .map_err(|_| Error::NoMemory)?;
    one_line_into(&mut out, title)?;
    for part in [
        "\n\nid: ",
        id,
        "\ncreated_time: ",
        now,
        "\nupdated_time: ",
        now,
        "\nuser_created_time: ",
        now,
        "\nuser_updated_time: ",
        now,
        "\nencryption_cipher_text: \nencryption_applied: 0\nparent_id: \n\
         is_shared: 0\nshare_id: \nmaster_key_id: \nicon: \nuser_data: \n\
         deleted_time: 0\ntype_: 2",
    ] {
        append(&mut out, part)?;
    }
    Ok(out)
}

fn append(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

/// A note besides its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note<'a> {
    pub id: &'a str,
    /// Its notebook.
    pub parent: &'a str,
    pub title: &'a str,
    pub author: &'a str,
    /// When it was made, as Joplin and as the user see it, and changed
    /// (times as Joplin writes them); its place in a notebook sorted by hand.
    pub created: &'a str,
    pub user_created: &'a str,
    pub updated: &'a str,
    pub order: i64,
}

impl Note<'_> {
    /// The properties that follow the note's text.
    ///
    /// # Errors
    /// `NoMemory` when the text cannot be kept.
    pub fn props(&self) -> Result<String, Error> {
        let (created, user_created, updated) = (self.created, self.user_created, self.updated);
        let mut out = String::new();
        out.try_reserve(700 + 2 * self.id.len() + self.parent.len() + self.author.len() * 2)
            .map_err(|_| Error::NoMemory)?;
        for part in [
            "id: ",
            self.id,
            "\nparent_id: ",
            self.parent,
            "\ncreated_time: ",
            created,
            "\nupdated_time: ",
            updated,
            "\nis_conflict: 0\nlatitude: 0.00000000\nlongitude: 0.00000000\naltitude: 0.0000\n\
             author: ",
        ] {
            append(&mut out, part)?;
        }
        escape_into(&mut out, self.author)?;
        append(
            &mut out,
            "\nsource_url: \nis_todo: 0\ntodo_due: 0\ntodo_completed: 0\n\
             source: paperpaw\nsource_application: org.paperpaw\napplication_data: \norder: ",
        )?;
        number(&mut out, self.order)?;
        for part in [
            "\nuser_created_time: ",
            user_created,
            "\nuser_updated_time: ",
            updated,
            "\nencryption_cipher_text: \nencryption_applied: 0\nmarkup_language: 1\n\
             is_shared: 0\nshare_id: \nconflict_original_id: \nmaster_key_id: \n\
             user_data: \ndeleted_time: 0\nis_locked: 0\nextracted_resource_ids: \n\
             type_: 1",
        ] {
            append(&mut out, part)?;
        }
        Ok(out)
    }
}

/// `value` in decimal, appended to `out`.
fn number(out: &mut String, value: i64) -> Result<(), Error> {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    let mut rest = value.unsigned_abs();
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 {
        append(out, "-")?;
    }
    append(out, core::str::from_utf8(&digits[at..]).unwrap_or("0"))
}

/// A book's title and author, as its notes start.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Head {
    pub title: String,
    pub author: String,
}

/// The title (`# Title`) and the author, the next line that is not blank,
/// both before the first `##` heading.
///
/// # Errors
/// `NoMemory` when the texts cannot be kept.
pub fn parse_head(text: &str) -> Result<Head, Error> {
    let mut head = Head::default();
    for line in text.lines().map(str::trim) {
        if line.starts_with("## ") {
            break;
        }
        if let Some(title) = line.strip_prefix("# ") {
            head.title = copy(title.trim())?;
        } else if !line.is_empty() && !head.title.is_empty() {
            head.author = copy(line)?;
            break;
        }
    }
    Ok(head)
}
