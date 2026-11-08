//! Joplin's items as its sync targets keep them, the same for Joplin
//! Cloud, Joplin Server and WebDAV: a file `<id>.md` per note or
//! notebook holding its title, a blank line, its body (Markdown, as it
//! is), a blank line and its properties as `key: value` lines, the last
//! one `type_` (1 a note, 2 a notebook). Joplin reads the properties
//! from the end up to the first blank line, so a body may hold any text.
//! Written as Joplin 3 writes them.

use alloc::format;
use alloc::string::{String, ToString};

use paperpaw_sdk::digest::md5::Md5;
use paperpaw_sdk::time::UnixTime;
use paperpaw_sdk::{Storage, StorageError};

use crate::markdown;

/// The version of the sync target's format these items are written for
/// (`version` in its `info.json`, Joplin's `syncVersion`, 3 since Joplin
/// 2.x): Joplin syncs only with targets of its own version, upgrading
/// older ones itself.
pub const SYNC_VERSION: u32 = 3;
/// `type_` of a note and of a notebook.
pub const NOTE: u8 = 1;
pub const NOTEBOOK: u8 = 2;
/// The end of an item read for its properties: they take about 700
/// bytes.
const TAIL: usize = 2048;

/// A new Joplin id (32 hex digits) from `seed`, which makes it unique
/// (what it is for and the time), and the id of the item it `replaces`,
/// from which it differs even if the clock gave the same time again.
pub fn new_id(seed: &str, replaces: Option<&str>) -> String {
    let mut md5 = Md5::new();
    md5.update(seed.as_bytes());
    if let Some(old) = replaces {
        md5.update(b" ");
        md5.update(old.as_bytes());
    }
    md5.hex()
}

/// Whether `text` is a Joplin id.
pub fn is_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The file of item `id` in a sync target.
pub fn file_name(id: &str) -> String {
    format!("{id}.md")
}

/// A property's value on one line, escaped as Joplin escapes it: a line
/// break as `\n`, a `\n` written out as `\\n` (the same for `\r`).
pub(crate) fn escape(value: &str) -> String {
    value
        .replace("\\n", "\\\\n")
        .replace("\\r", "\\\\r")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// A title on one line, with no blank around it.
pub(crate) fn one_line(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<alloc::vec::Vec<_>>()
        .join(" ")
}

/// The properties of an item that matter here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Props {
    pub kind: u8,
    pub parent: String,
    pub created: String,
    /// When it changed last, in ms: Joplin takes a note written over
    /// only if it changed later.
    pub updated: i64,
    /// When it was made as the user sees it.
    pub user_created: String,
    /// Its place in a notebook sorted by hand.
    pub order: i64,
    /// In Joplin's trash.
    pub deleted: bool,
    /// Encrypted (Joplin's end-to-end encryption).
    pub encrypted: bool,
    /// Where its text (title and body) ends in its file: at the blank
    /// line before the properties ([`read_props`] only).
    pub text_end: u64,
}

/// The properties at the end of an item's text.
pub fn parse_props(text: &str) -> Props {
    let block = text.trim_end().rsplit("\n\n").next().unwrap_or_default();
    let mut props = Props::default();
    for line in block.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "type_" => props.kind = value.parse().unwrap_or(0),
            "parent_id" => props.parent = value.to_string(),
            "created_time" => props.created = value.to_string(),
            "updated_time" => {
                props.updated = UnixTime::parse_iso8601(value).map_or(0, UnixTime::millis)
            }
            "user_created_time" => props.user_created = value.to_string(),
            "order" => props.order = value.parse().unwrap_or(0),
            "deleted_time" => props.deleted = !matches!(value, "" | "0"),
            "encryption_applied" => props.encrypted = value == "1",
            _ => {}
        }
    }
    props
}

/// The properties of the item in the file at `path`, and where its text
/// ends: only the file's end is read, however long its body.
pub fn read_props(storage: &dyn Storage, path: &str) -> Result<Props, StorageError> {
    let size = storage.size(path)?;
    let from = size.saturating_sub(TAIL as u64);
    let mut tail = [0u8; TAIL];
    let n = storage.read_at(path, from, &mut tail)?;
    let tail = tail[..n].trim_ascii_end();
    let blank = tail.windows(2).rposition(|w| w == b"\n\n").unwrap_or(0);
    Ok(Props {
        text_end: from + blank as u64,
        ..parse_props(&String::from_utf8_lossy(tail))
    })
}

/// The MD5 of an item's text, the first `end` bytes of the file `path`
/// (see [`Props::text_end`]), read in pieces: whether it was edited in
/// Joplin since PaperPaw wrote it, whatever Joplin did with its times.
pub fn text_hash(storage: &dyn Storage, path: &str, end: u64) -> Result<String, StorageError> {
    let mut md5 = Md5::new();
    let mut piece = [0u8; 1024];
    let mut at = 0u64;
    while at < end {
        let want = (end - at).min(piece.len() as u64) as usize;
        let n = storage.read_at(path, at, &mut piece[..want])?;
        if n == 0 {
            break;
        }
        md5.update(&piece[..n]);
        at += n as u64;
    }
    Ok(md5.hex())
}

/// A notebook named `title`, made at `now` (a [`time`]), in no other.
pub fn notebook(id: &str, title: &str, now: &str) -> String {
    format!(
        "{}\n\nid: {id}\ncreated_time: {now}\nupdated_time: {now}\n\
         user_created_time: {now}\nuser_updated_time: {now}\n\
         encryption_cipher_text: \nencryption_applied: 0\nparent_id: \n\
         is_shared: 0\nshare_id: \nmaster_key_id: \nicon: \nuser_data: \n\
         deleted_time: 0\ntype_: {NOTEBOOK}",
        one_line(title)
    )
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
    /// ([`time`]s); its place in a notebook sorted by hand (Joplin takes
    /// the time made, in ms).
    pub created: &'a str,
    pub user_created: &'a str,
    pub updated: &'a str,
    pub order: i64,
}

impl Note<'_> {
    pub(crate) fn props(&self) -> String {
        let (created, user_created, updated) = (self.created, self.user_created, self.updated);
        format!(
            "id: {}\nparent_id: {}\ncreated_time: {created}\nupdated_time: {updated}\n\
             is_conflict: 0\nlatitude: 0.00000000\nlongitude: 0.00000000\naltitude: 0.0000\n\
             author: {}\nsource_url: \nis_todo: 0\ntodo_due: 0\ntodo_completed: 0\n\
             source: paperpaw\nsource_application: org.paperpaw\napplication_data: \n\
             order: {}\nuser_created_time: {user_created}\nuser_updated_time: {updated}\n\
             encryption_cipher_text: \nencryption_applied: 0\nmarkup_language: 1\n\
             is_shared: 0\nshare_id: \nconflict_original_id: \nmaster_key_id: \n\
             user_data: \ndeleted_time: 0\nis_locked: 0\nextracted_resource_ids: \n\
             type_: {NOTE}",
            self.id,
            self.parent,
            escape(self.author),
            self.order
        )
    }

    /// Writes the note to `to`, its body the reader's notes in the file
    /// `from` (see [`markdown::copy_body`]), copied piece by piece.
    /// Returns the MD5 of its text (see [`text_hash`]).
    pub fn write(
        &self,
        storage: &dyn Storage,
        from: &str,
        to: &str,
    ) -> Result<String, StorageError> {
        let title = one_line(self.title);
        let mut md5 = Md5::new();
        md5.update(title.as_bytes());
        storage.write(to, format!("{title}\n\n").as_bytes())?;
        let mut first = true;
        let body = markdown::copy_body(storage, from, &mut |piece| {
            if core::mem::take(&mut first) {
                md5.update(b"\n\n");
            }
            md5.update(piece);
            storage.append(to, piece)
        })?;
        if body {
            storage.append(to, b"\n\n")?;
        }
        storage.append(to, self.props().as_bytes())?;
        Ok(md5.hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn times_as_joplin_writes_them() {
        let iso8601 = |ms| UnixTime::from_millis(ms).iso8601();
        let parse_iso8601 = |text: &str| UnixTime::parse_iso8601(text).map(UnixTime::millis);
        assert_eq!(iso8601(1_792_448_650_090), "2026-10-19T22:24:10.090Z");
        assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
        for ms in [0, 951_782_400_123, 1_792_448_650_090, 4_107_542_399_999] {
            assert_eq!(parse_iso8601(&iso8601(ms)), Some(ms));
        }
        assert_eq!(parse_iso8601(""), None);
    }

    #[test]
    fn ids() {
        let id = new_id("a book 1", None);
        assert!(is_id(&id));
        assert_ne!(id, new_id("a book 2", None));
        // Made again at the same time, in place of the first.
        assert_ne!(id, new_id("a book 1", Some(&id)));
        assert!(!is_id("info") && !is_id(&"g".repeat(32)));
        assert_eq!(file_name(&id), format!("{id}.md"));
    }

    #[test]
    fn values_stay_on_one_line() {
        assert_eq!(escape("a\nb\\nc"), "a\\nb\\\\nc");
        assert_eq!(one_line(" Alice's\n Adventures "), "Alice's Adventures");
    }

    /// As Joplin 3.7 wrote it to a sync target.
    const JOPLIN_NOTE: &str = "Hello\n\nLine one\n\n> quote\n\n\
        id: 1ae79351dd744c2794eebb334b1629a7\n\
        parent_id: 7db793874c624138b3b4520ae5cdf20c\n\
        created_time: 2026-10-19T22:24:10.090Z\n\
        updated_time: 2026-10-19T22:24:11.708Z\n\
        is_conflict: 0\nauthor: \nsource_url: \n\
        user_created_time: 2026-10-19T22:24:10.090Z\n\
        user_updated_time: 2026-10-19T22:24:10.090Z\n\
        order: 1792448650089\n\
        encryption_cipher_text: \nencryption_applied: 0\n\
        deleted_time: 0\nis_locked: 0\ntype_: 1";

    #[test]
    fn reads_the_properties_at_the_end() {
        let props = parse_props(JOPLIN_NOTE);
        assert_eq!(
            props,
            Props {
                kind: NOTE,
                parent: String::from("7db793874c624138b3b4520ae5cdf20c"),
                created: String::from("2026-10-19T22:24:10.090Z"),
                updated: 1_792_448_651_708,
                user_created: String::from("2026-10-19T22:24:10.090Z"),
                order: 1_792_448_650_089,
                deleted: false,
                encrypted: false,
                text_end: 0,
            }
        );
        let trashed = JOPLIN_NOTE.replace("deleted_time: 0", "deleted_time: 1792448650090");
        assert!(parse_props(&trashed).deleted);
        // Only the end of a long note is read; its text ends at the blank
        // line before the properties.
        let body = "x".repeat(10_000);
        let props_text = &JOPLIN_NOTE[JOPLIN_NOTE.find("id: ").unwrap()..];
        let long = format!("Long\n\n{body}\n\n{props_text}\n");
        let storage = MemoryStorage::new().with("n.md", long);
        let read = read_props(&storage, "n.md").unwrap();
        let text_end = 6 + body.len() as u64;
        assert_eq!(read, Props { text_end, ..props });
        assert_eq!(
            text_hash(&storage, "n.md", text_end).unwrap(),
            paperpaw_sdk::digest::md5::of(&format!("Long\n\n{body}"))
        );
        assert_eq!(read_props(&storage, "none.md"), Err(StorageError::NotFound));
    }

    #[test]
    fn writes_a_note_joplin_reads() {
        let storage = MemoryStorage::new().with(
            "Notes/a.md",
            "# Alice\n\nLewis Carroll\n\n## Highlights\n\n> Curiouser!\n>\n> 12 %\n\n",
        );
        let note = Note {
            id: "1ae79351dd744c2794eebb334b1629a7",
            parent: "7db793874c624138b3b4520ae5cdf20c",
            title: "Alice",
            author: "Lewis Carroll",
            created: "2026-10-19T22:24:10.090Z",
            user_created: "2026-10-19T22:24:09.000Z",
            updated: "2026-10-19T22:24:11.708Z",
            order: 1_792_448_650_090,
        };
        let hash = note.write(&storage, "Notes/a.md", "item.md").unwrap();
        let text = String::from_utf8(storage.get("item.md").unwrap()).unwrap();
        assert!(text.starts_with(
            "Alice\n\nLewis Carroll\n\n## Highlights\n\n> Curiouser!\n>\n> 12 %\n\nid: 1ae7"
        ));
        assert!(text.contains("\nauthor: Lewis Carroll\n"));
        assert!(text.contains("\nuser_updated_time: 2026-10-19T22:24:11.708Z\n"));
        assert!(text.ends_with("\ntype_: 1"));
        let props = read_props(&storage, "item.md").unwrap();
        assert_eq!((props.kind, props.parent.as_str()), (NOTE, note.parent));
        assert_eq!(props.user_created, note.user_created);
        // Its text, as found again.
        assert_eq!(
            text_hash(&storage, "item.md", props.text_end).unwrap(),
            hash
        );
        // Without a body: the title, a blank line, the properties.
        let storage = MemoryStorage::new().with("Notes/b.md", "# Bob\n\n");
        let hash = note.write(&storage, "Notes/b.md", "item.md").unwrap();
        let text = String::from_utf8(storage.get("item.md").unwrap()).unwrap();
        assert!(text.starts_with("Alice\n\nid: "));
        let props = read_props(&storage, "item.md").unwrap();
        assert_eq!(
            text_hash(&storage, "item.md", props.text_end).unwrap(),
            hash
        );
        assert_eq!(hash, paperpaw_sdk::digest::md5::of("Alice"));
    }

    #[test]
    fn notebooks() {
        let text = notebook(
            "7db793874c624138b3b4520ae5cdf20c",
            "Books",
            "2026-10-19T22:24:08.464Z",
        );
        assert!(text.starts_with("Books\n\nid: 7db793874c624138b3b4520ae5cdf20c\n"));
        assert_eq!(parse_props(&text).kind, NOTEBOOK);
    }
}
