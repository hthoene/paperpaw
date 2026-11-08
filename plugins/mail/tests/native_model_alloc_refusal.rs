//! Every allocation of the native readers and writers of mail may be refused:
//! each ends in `NoMemory` or succeeds, never aborts, and a password in
//! the credentials is wiped by its owner on every path.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_mail::native_model::fail::Fail;
use paperpaw_plugin_mail::native_model::imap::{self, Fault};
use paperpaw_plugin_mail::native_model::{codec, header, html, mem, render, smtp, store};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::{DirEntry, Storage, StorageError};
use std::cell::{Cell, RefCell};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

/// A sweep that must find allocations to refuse.
fn swept<T>(name: &str, run: impl FnMut() -> Result<T, Error>) {
    let total = fail_alloc::sweep(run);
    assert!(
        total > 0,
        "{name} allocates nothing: the sweep proves nothing"
    );
}

#[test]
fn the_encodings_survive_refused_allocations() {
    swept("base64", || {
        let data = codec::decode_base64(b"aGVsbG8gd29ybGQ=\r\nAAEC")?;
        let text = codec::encode_base64(&data)?;
        codec::base64_line(&data)?;
        let mut out = Vec::new();
        let mut pieces = codec::Base64::default();
        pieces.push(text.as_bytes(), &mut out)?;
        pieces.finish(&mut out)
    });
    swept("qp and words", || {
        codec::decode_qp(b"caf=E9=\r\n au_lait", true)?;
        codec::encode_qp("caf\u{e9} au lait \u{4e16}\u{754c}")?;
        codec::encode_words("caf\u{e9} \u{4e16}\u{754c}")?;
        codec::decode_words(b"=?utf-8?B?w6TDtsO8?= x =?iso-8859-1?Q?caf=E9?= tail")?;
        codec::decode_words(&[b'c', 0xE9, b' ', 0xFF])?;
        Ok(())
    });
    swept("charsets", || {
        for charset in [
            codec::Charset::Utf8,
            codec::Charset::Latin1,
            codec::Charset::Windows1252,
            codec::Charset::Koi8R,
        ] {
            codec::text_of(charset, &[b'a', 0xC3, 0xA4, 0xFF, 0xC3])?;
            let mut decoder = codec::Decoder::new(charset);
            let mut out = String::new();
            decoder.push(&[b'a', 0xC3], &mut out)?;
            decoder.push(&[0xA4, b'z'], &mut out)?;
            decoder.finish(&mut out)?;
        }
        codec::utf8_lossy(&[b'a', 0xFF, b'b'])?;
        Ok(())
    });
}

#[test]
fn the_headers_survive_refused_allocations() {
    let raw = b"Subject: =?utf-8?B?w6TDtsO8?=\r\nFrom: Ada <ada@example.org>\r\n\
To: \"Doe, John\" <j@x>, b@y\r\nContent-Type: multipart/mixed;\r\n boundary=\"b\"; name*=utf-8''%C3%A4\r\n\
References: <1@x> <2@x>\r\n\r\nbody";
    swept("headers", || {
        let headers = header::Headers::parse(raw)?;
        header::ContentType::parse(headers.get("content-type"))?;
        header::ContentType::parse(None)?;
        header::addresses(headers.get("to").unwrap())?;
        header::reply_subject("Re: Hello")?;
        header::capped("caf\u{e9} caf\u{e9}", 5)?;
        header::last_ids("<1@x> <2@x> <3@x> <4@x>", 12)?;
        Ok(())
    });
}

#[test]
fn credentials_and_secrets_survive_refused_allocations() {
    swept("credentials", || {
        let creds = codec::plain_credentials("ada", "hunter2")?;
        assert_eq!(creds.as_slice(), b"\0ada\0hunter2");
        let mut secret = mem::Secret::with_capacity(7)?;
        secret.push_str("hunter2")?;
        mem::wipe_text(secret.into_string());
        Ok(())
    });
}

#[test]
fn the_html_reader_survives_refused_allocations() {
    swept("html", || {
        let text = html::to_text(
            "<p>Hello &amp; welcome &unknown; &#x1F600;</p><ul><li>one</li><li>two</li></ul>\
             <style>p { color: red }</style><!-- note --><br>caf\u{e9} <b>bold</b> &am",
        )?;
        assert!(text.starts_with("Hello & welcome"));
        Ok(())
    });
    swept("html in pieces", || {
        let mut parser = html::HtmlText::with_limit(64);
        let mut out = String::new();
        for piece in [
            "<p>He",
            "llo &a",
            "mp; <scr",
            "ipt>x</script>",
            "<li>a",
            "b",
            "</li>",
        ] {
            parser.push(piece, &mut out)?;
        }
        parser.finish(&mut out)
    });
}

/// A fault of the IMAP reader as the sweep sees it: only a refusal is allowed.
fn refusal(fault: Fault) -> Error {
    match fault {
        Fault::NoMemory => Error::NoMemory,
        _ => Error::Invalid,
    }
}

#[test]
fn the_imap_reader_survives_refused_allocations() {
    const LINES: &[&str] = &[
        "A1 OK LOGIN completed",
        "+ go ahead",
        "* CAPABILITY IMAP4rev1 STARTTLS AUTH=PLAIN",
        "* LIST (\\HasNoChildren \\Trash) \"/\" \"Deleted Items\"",
        "* SEARCH 2 84 882",
        "* STATUS \"INBOX\" (MESSAGES 231 UNSEEN 7)",
        "* BYE gone",
        "* OK [UIDVALIDITY 3857529045] UIDs valid",
        "* 12 FETCH (UID 42 FLAGS (\\Seen \\Answered) RFC822.SIZE 1000 INTERNALDATE \"d\")",
        "* 3 FETCH (BODY[HEADER.FIELDS (FROM TO)] {342}",
    ];
    for line in LINES {
        swept("response", || {
            imap::response(line.as_bytes()).map_err(refusal)?;
            imap::tokens(line.as_bytes()).map_err(refusal)
        });
    }
    swept("fetch parts", || {
        let line = b"* 12 FETCH (UID 42 FLAGS (\\Seen \\Deleted) RFC822.SIZE 9)";
        let Some(imap::Response::Untagged(imap::Untagged::Fetch { items, .. })) =
            imap::response(line).map_err(refusal)?
        else {
            return Err(Error::Invalid);
        };
        let flags = imap::fetch_flags(&items).map_err(refusal)?;
        assert_eq!(imap::flag_letters(&flags)?, "SD");
        Ok(())
    });
    swept("names and quoting", || {
        assert_eq!(imap::mailbox_display("Entw&APw-rfe")?, "Entw\u{fc}rfe");
        imap::quoted("a \"b\" c")?;
        let secret = imap::quoted_secret("pa\"ss")?;
        assert_eq!(secret.as_str(), "\"pa\\\"ss\"");
        Ok(())
    });
    swept("line buffer", || {
        let mut buffer = imap::LineBuffer::default();
        buffer
            .push(b"A1 OK one\r\nA2 OK two\r\n{5}abcde")
            .map_err(refusal)?;
        assert!(buffer.line().map_err(refusal)?.is_some());
        assert!(buffer.line().map_err(refusal)?.is_some());
        buffer.take(3)?;
        Ok(())
    });
}

#[test]
fn the_smtp_reader_survives_refused_allocations() {
    swept("replies", || {
        let mut parser = smtp::ReplyParser::default();
        for line in ["250-mail.example.org", "250-STARTTLS", "250 AUTH PLAIN"] {
            parser.line(line.as_bytes())?;
        }
        parser.line(b"garbage")?;
        parser.line(&[b'x'; 1001])?;
        Ok(())
    });
    swept("commands", || {
        assert_eq!(smtp::mail_from("a@b.c")?, "MAIL FROM:<a@b.c>\r\n");
        smtp::rcpt_to("a@b.c>\r\nDATA")?;
        let line = smtp::auth_plain("ada", "hunter2")?;
        assert_eq!(line.as_str(), "AUTH PLAIN AGFkYQBodW50ZXIy\r\n");
        smtp::data("a\n.b\n..c\n")?;
        Ok(())
    });
}

/// A card of fixed files that never allocates (each file has its room), so that
/// a refused allocation is the code's own and never the test double's.
struct Slots(Vec<Slot>);

struct Slot {
    name: &'static str,
    data: RefCell<Vec<u8>>,
    exists: Cell<bool>,
}

impl Slots {
    fn new(names: &[&'static str]) -> Self {
        Self(
            names
                .iter()
                .map(|name| Slot {
                    name,
                    data: RefCell::new(Vec::with_capacity(96 * 1024)),
                    exists: Cell::new(false),
                })
                .collect(),
        )
    }

    fn slot(&self, path: &str) -> Result<&Slot, StorageError> {
        self.0
            .iter()
            .find(|s| s.name == path)
            .ok_or(StorageError::InvalidPath)
    }

    /// Puts `data` in `path` (within its room: no allocation).
    fn put(&self, path: &str, data: &[u8]) {
        let slot = self.slot(path).unwrap();
        let mut file = slot.data.borrow_mut();
        file.clear();
        file.extend_from_slice(data);
        slot.exists.set(true);
    }

    fn get(&self, path: &str) -> Vec<u8> {
        self.slot(path).unwrap().data.borrow().clone()
    }
}

impl Storage for Slots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let slot = self.slot(path)?;
        if !slot.exists.get() {
            return Err(StorageError::NotFound);
        }
        let data = slot.data.borrow();
        let from = usize::try_from(offset).unwrap().min(data.len());
        let n = buf.len().min(data.len() - from);
        buf[..n].copy_from_slice(&data[from..from + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let slot = self.slot(path)?;
        if !slot.exists.get() {
            return Err(StorageError::NotFound);
        }
        Ok(slot.data.borrow().len() as u64)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.slot(path)?;
        self.put(path, data);
        Ok(())
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let slot = self.slot(path)?;
        slot.data.borrow_mut().extend_from_slice(data);
        slot.exists.set(true);
        Ok(())
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let slot = self.slot(path)?;
        slot.data.borrow_mut().clear();
        slot.exists.set(false);
        Ok(())
    }
}

/// A `Fail` as the sweep wants it: a refusal passes, anything else is a bug.
fn refused(fail: Fail) -> Error {
    match fail {
        Fail::NoMemory => Error::NoMemory,
        other => panic!("not a refusal: {other:?}"),
    }
}

const MAIL: &[u8] = b"From: =?utf-8?Q?J=C3=BCrgen?= <j@x.org>\r\nTo: a@x.org, \"B, C\" <b@x.org>\r\n\
Subject: =?iso-8859-1?Q?caf=E9?= hello\r\nDate: Tue, 3 Nov 2026 10:00:00 +0100\r\nMessage-ID: <id@x>\r\n\
References: <a@x> <b@x>\r\nContent-Type: multipart/mixed; boundary=m\r\n\r\n--m\r\n\
Content-Type: multipart/alternative; boundary=a\r\n\r\n--a\r\nContent-Type: text/plain; charset=iso-8859-1\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\r\ncaf=E9 au lait=\r\n et plus\r\n--a\r\nContent-Type: text/html\r\n\r\n\
<p>html</p>\r\n--a--\r\n--m\r\nContent-Type: text/html\r\nContent-Transfer-Encoding: base64\r\n\r\nPHA+aGk8L3A+\r\n--m\r\n\
Content-Type: application/pdf; name=\"a.pdf\"\r\nContent-Disposition: attachment; filename=\"a.pdf\"\r\n\r\nJVBERi0=\r\n--m--\r\n";

#[test]
fn rendering_and_the_store_survive_refused_allocations() {
    let card = Slots::new(&[
        "raw.eml",
        "out.txt",
        "m1.txt",
        "m1.txt.tmp",
        "i.txt",
        "f.txt",
        "s.txt",
    ]);
    swept("render", || {
        card.put("raw.eml", MAIL);
        let info = render::render(&card, "raw.eml", "out.txt", 24 * 1024).map_err(refused)?;
        assert_eq!(info.attachments.len(), 1);
        // And cut short.
        render::render(&card, "raw.eml", "out.txt", 9).map_err(refused)?;
        Ok(())
    });
    swept("letter", || {
        card.put("raw.eml", MAIL);
        let letter = store::make_letter(&card, "raw.eml", "m1.txt").map_err(refused)?;
        assert!(letter.text.contains("caf\u{e9}"));
        let read = store::read_letter(&card, "m1.txt").map_err(refused)?;
        assert!(read.is_some());
        Ok(())
    });
    // Made before the sweeps: the test's own allocations are not the code's.
    let seen = vec!["\\Seen".to_string()];
    swept("index and counts", || {
        let row =
            store::Row::from_headers(3, &seen, 9, 100, b"From: A <a@x>\r\nSubject: s\r\n\r\n")
                .map_err(refused)?;
        store::write_index(&card, "i.txt", &[row]).map_err(refused)?;
        let rows = store::read_index(&card, "i.txt").map_err(refused)?;
        assert_eq!(rows.len(), 1);
        let counts = store::Counts::default();
        card.put("s.txt", counts.line().map_err(refused)?.as_bytes());
        // A cache that cannot be read for want of memory is no counts.
        store::Counts::read(&card, "s.txt");
        Ok(())
    });
    swept("folders and names", || {
        let mut folders = Vec::new();
        for (raw, flag) in [
            ("INBOX", ""),
            ("Sent", "\\sent"),
            ("Zeta", ""),
            ("alpha", ""),
        ] {
            let mut flags = Vec::new();
            if !flag.is_empty() {
                mem::push(&mut flags, mem::copy(flag)?)?;
            }
            mem::push(
                &mut folders,
                store::Folder::new(raw, flags).map_err(refused)?,
            )?;
        }
        store::sort_folders(&mut folders);
        store::write_folders(&card, "f.txt", &folders).map_err(refused)?;
        assert_eq!(
            store::read_folders(&card, "f.txt").map_err(refused)?.len(),
            4
        );
        store::index_file(1, "INBOX").map_err(refused)?;
        store::text_file(1, "Archiv/2026", 7).map_err(refused)?;
        store::field(" a\tb\n ").map_err(refused)?;
        Ok(())
    });
    assert_eq!(
        card.get("raw.eml").len(),
        0,
        "the letter took the raw message away"
    );
}
