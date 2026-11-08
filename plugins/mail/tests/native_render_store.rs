//! The native renderer and store give the same answers as the interface-6 ones
//! on what both take: fixed mails, random mails of every structure the
//! renderer knows (multipart, alternative, quoted-printable, base64, charsets,
//! HTML, attachments), the index, the counts, the folders and the letter.
//! They differ, on purpose, in the paths (relative to the data folder) and in
//! what interface 6 would have panicked on or grown without end.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_mail::native_model::fail::Fail;
use paperpaw_plugin_mail::native_model::{render as new_render, store as new_store};
use paperpaw_plugin_mail::{header as old_header, render as old_render, store as old_store};
use paperpaw_sdk::{MemoryStorage, StorageError};

/// A small deterministic generator, so that failures repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn pairs_old(list: &[old_header::Address]) -> Vec<(String, String)> {
    list.iter()
        .map(|a| (a.name.clone(), a.address.clone()))
        .collect()
}

fn pairs_new(
    list: &[paperpaw_plugin_mail::native_model::header::Address],
) -> Vec<(String, String)> {
    list.iter()
        .map(|a| (a.name.clone(), a.address.clone()))
        .collect()
}

/// Renders `mail` with both, and says where they differ.
fn same(mail: &[u8], limit: usize) {
    let old_storage = MemoryStorage::new().with("raw.eml", mail);
    let new_storage = MemoryStorage::new().with("raw.eml", mail);
    let old = old_render::render(&old_storage, "raw.eml", "out.txt", limit).unwrap();
    let new = new_render::render(&new_storage, "raw.eml", "out.txt", limit).unwrap();
    let shown = String::from_utf8_lossy(mail);
    assert_eq!(
        old_storage.get("out.txt"),
        new_storage.get("out.txt"),
        "text of {shown:?} (limit {limit})"
    );
    assert_eq!(
        pairs_old(&old.from),
        pairs_new(new.from.as_slice()),
        "{shown:?}"
    );
    assert_eq!(
        pairs_old(&old.to),
        pairs_new(new.to.as_slice()),
        "{shown:?}"
    );
    assert_eq!(
        pairs_old(&old.cc),
        pairs_new(new.cc.as_slice()),
        "{shown:?}"
    );
    assert_eq!(
        pairs_old(&old.reply_to),
        pairs_new(new.reply_to.as_slice()),
        "{shown:?}"
    );
    assert_eq!(old.subject, new.subject, "{shown:?}");
    assert_eq!(old.date, new.date, "{shown:?}");
    assert_eq!(old.message_id, new.message_id, "{shown:?}");
    assert_eq!(old.references, new.references, "{shown:?}");
    assert_eq!(old.attachments, new.attachments, "{shown:?}");
    assert_eq!(
        old.truncated, new.truncated,
        "truncated {shown:?} (limit {limit})"
    );
    assert_eq!(old.text_len, new.text_len, "{shown:?} (limit {limit})");
}

const FIXED: &[&str] = &[
    "",
    "Subject: only headers",
    "Subject: x\n\nplain body\nsecond line\n",
    "From: Ada <ada@example.org>\r\nTo: a@x.org, \"B, C\" <b@x.org>\r\nCc: c@x.org\r\n\
     Reply-To: r@x.org\r\nSubject: =?utf-8?B?w6TDtsO8?= caf\r\nDate: Tue, 3 Nov 2026 10:00:00 +0100\r\n\
     Message-ID: <id@x>\r\nReferences: <a@x> <b@x>\r\n\r\nHello\r\n",
    "Content-Type: text/html\n\n<p>Hello &amp; <b>welcome</b></p><p>Second</p>\n",
    "Content-Type: text/plain; charset=iso-8859-1\nContent-Transfer-Encoding: quoted-printable\n\n\
     caf=E9 au lait=\n et plus\n",
    "Content-Type: text/plain\nContent-Transfer-Encoding: base64\n\naGVsbG8gd29ybGQ=\n",
    "Content-Type: multipart/alternative; boundary=\"b\"\n\n--b\nContent-Type: text/plain\n\nplain\n--b\n\
     Content-Type: text/html\n\n<p>html</p>\n--b--\n",
    "Content-Type: multipart/alternative; boundary=b\n\n--b\nContent-Type: text/html\n\n<p>html</p>\n--b\n\
     Content-Type: text/plain\n\nplain\n--b--\n",
    "Content-Type: multipart/mixed; boundary=m\n\n--m\nContent-Type: text/plain\n\nbody\n--m\n\
     Content-Type: application/pdf; name=\"a.pdf\"\nContent-Disposition: attachment; filename=\"a.pdf\"\n\n\
     JVBERi0=\n--m\nContent-Type: image/png\n\nxx\n--m--\n",
    "Content-Type: multipart/mixed; boundary=outer\n\n--outer\nContent-Type: multipart/alternative; boundary=inner\n\n\
     --inner\nContent-Type: text/plain\n\none\n--inner--\n--outer\nContent-Type: text/plain\n\ntwo\n--outer--\n",
    "Content-Type: multipart/mixed; boundary=m\n\n--m\nContent-Type: text/plain\n\none\n--m\n\
     Content-Type: text/plain\n\ntwo\n--m--\nepilogue\n",
    "Content-Type: multipart/mixed\n\nno boundary\n",
    "Content-Type: multipart/mixed; boundary=m\n\n--m\nContent-Type: text/plain\n\nnever closed\n",
    "Content-Disposition: attachment; filename=x.txt\nContent-Type: text/plain\n\nbody of attachment\n",
    "Content-Type: text/plain; charset=utf-8\n\n\u{e4}\u{f6}\u{fc} \u{4e16}\u{754c} \u{1f600}\n",
    "Content-Type: text/plain; charset=koi8-r\n\n\u{ff}\u{fe}\n",
    "Content-Type: text/html\n\n<style>p{x}</style><script>a<b</script><!-- c --><p>x</p>\n",
    "Subject: folded\n header\n\nbody",
    "no colon here\n\nbody",
    "\n\n\n",
    "--b\n--b--\n",
];

#[test]
fn fixed_mails_render_the_same() {
    for mail in FIXED {
        for limit in [24 * 1024, 40, 7, 0] {
            same(mail.as_bytes(), limit);
        }
    }
}

const TOP: &[&str] = &[
    "From: Ada <ada@example.org>",
    "From: =?utf-8?Q?J=C3=BCrgen?= <j@x.org>",
    "To: a@x.org, b@x.org",
    "Cc: \"C, D\" <c@x.org>",
    "Reply-To: r@x.org",
    "Subject: Re: hello world",
    "Subject: =?iso-8859-1?Q?caf=E9?=",
    "Date: Tue, 3 Nov 2026 10:00:00 +0100",
    "Date: garbage",
    "Message-ID: <abc@x>",
    "References: <a@x> <b@x> <c@x>",
    "X-Other: value",
    "MIME-Version: 1.0",
];

const TYPES: &[&str] = &[
    "text/plain",
    "text/plain; charset=utf-8",
    "text/plain; charset=iso-8859-1",
    "text/plain; charset=windows-1252",
    "text/html",
    "text/html; charset=utf-8",
    "application/pdf; name=\"f.pdf\"",
    "image/png",
    "multipart/alternative; boundary=\"b2\"",
    "multipart/mixed; boundary=b2",
    "application/octet-stream",
    "text/calendar",
];

const ENCODINGS: &[&str] = &[
    "",
    "Content-Transfer-Encoding: 7bit",
    "Content-Transfer-Encoding: quoted-printable",
    "Content-Transfer-Encoding: base64",
    "Content-Transfer-Encoding: Base64",
];

const DISPOSITIONS: &[&str] = &[
    "",
    "",
    "Content-Disposition: inline",
    "Content-Disposition: attachment; filename=\"x.bin\"",
    "Content-Disposition: ATTACHMENT; filename*=utf-8''a%20b.txt",
    "Content-Disposition: att",
];

const BODIES: &[&str] = &[
    "Hello world",
    "caf=E9 au lait=",
    "=C3=A4=C3=B6",
    "<p>para</p>",
    "<b>bold</b> &amp; &lt;x&gt;",
    "<style>a{}</style>text",
    "aGVsbG8gd29ybGQ=",
    "w6TDtsO8",
    "AAEC",
    "",
    " ",
    "--b",
    "--b2",
    "--b2--",
    "--b--",
    "a line with trailing space ",
    "\u{e9}",
    "\u{4e16}\u{754c}",
];

/// A random mail: headers, then parts in the few shapes the renderer knows.
fn random_mail(rng: &mut Rng) -> Vec<u8> {
    let newline = if rng.below(2) == 0 { "\n" } else { "\r\n" };
    let mut mail = String::new();
    for _ in 0..rng.below(7) {
        mail.push_str(rng.pick(TOP));
        mail.push_str(newline);
    }
    let kind = *rng.pick(TYPES);
    mail.push_str("Content-Type: ");
    mail.push_str(if rng.below(3) == 0 {
        "multipart/mixed; boundary=b"
    } else {
        kind
    });
    mail.push_str(newline);
    mail.push_str(rng.pick(ENCODINGS));
    mail.push_str(newline);
    mail.push_str(newline);
    let parts = 1 + rng.below(4);
    for _ in 0..parts {
        if rng.below(4) != 0 {
            mail.push_str(if rng.below(2) == 0 { "--b" } else { "--b2" });
            mail.push_str(newline);
            mail.push_str("Content-Type: ");
            mail.push_str(rng.pick(TYPES));
            mail.push_str(newline);
            mail.push_str(rng.pick(ENCODINGS));
            mail.push_str(newline);
            mail.push_str(rng.pick(DISPOSITIONS));
            mail.push_str(newline);
            mail.push_str(newline);
        }
        for _ in 0..rng.below(8) {
            mail.push_str(rng.pick(BODIES));
            mail.push_str(newline);
        }
    }
    if rng.below(2) == 0 {
        mail.push_str(if rng.below(2) == 0 { "--b--" } else { "--b2--" });
        mail.push_str(newline);
    }
    mail.into_bytes()
}

#[test]
fn random_mails_render_the_same() {
    let mut rng = Rng(0x6d61_696c);
    for _ in 0..3_000 {
        let mail = random_mail(&mut rng);
        let limit = *rng.pick(&[24 * 1024, 200, 31, 5]);
        same(&mail, limit);
    }
}

#[test]
fn a_long_html_mail_is_cut_and_said() {
    let mut mail = String::from("Content-Type: text/html\n\n");
    for _ in 0..4_000 {
        mail.push_str("<p>paragraph of the mail</p>\n");
    }
    same(mail.as_bytes(), 24 * 1024);
    let storage = MemoryStorage::new().with("raw.eml", mail.as_bytes());
    let info = new_render::render(&storage, "raw.eml", "out.txt", 24 * 1024).unwrap();
    assert!(info.truncated);
    assert!(info.text_len <= 24 * 1024);
}

#[test]
fn hostile_bytes_are_rendered_without_a_panic() {
    let mut rng = Rng(0x0bad_cafe);
    for round in 0..600 {
        let mut mail = random_mail(&mut rng);
        for _ in 0..(round % 8) {
            let at = rng.below(mail.len().max(1));
            if let Some(byte) = mail.get_mut(at) {
                *byte = rng.next() as u8;
            }
        }
        if round % 5 == 0 {
            mail.truncate(rng.below(mail.len() + 1));
        }
        let storage = MemoryStorage::new().with("raw.eml", mail);
        new_render::render(&storage, "raw.eml", "out.txt", 24 * 1024).unwrap();
    }
    // Bytes only.
    for _ in 0..300 {
        let bytes: Vec<u8> = (0..rng.below(2_000)).map(|_| rng.next() as u8).collect();
        let storage = MemoryStorage::new().with("raw.eml", bytes);
        new_render::render(&storage, "raw.eml", "out.txt", 1024).unwrap();
    }
}

#[test]
fn a_mail_of_endless_parts_and_lines_stays_bounded() {
    let mut mail = String::from("Content-Type: multipart/mixed; boundary=b\n\n");
    for _ in 0..600 {
        mail.push_str(
            "--b\nContent-Type: application/x-a; name=n\n\n--b\nContent-Type: text/plain\n\nx\n",
        );
    }
    let storage = MemoryStorage::new().with("raw.eml", mail.as_bytes());
    let info = new_render::render(&storage, "raw.eml", "out.txt", 24 * 1024).unwrap();
    assert!(info.attachments.len() <= 32);
    // A line of a megabyte comes as lines of at most 8 KiB.
    let long = format!("Content-Type: text/plain\n\n{}\n", "x".repeat(1 << 20));
    let storage = MemoryStorage::new().with("raw.eml", long.as_bytes());
    let info = new_render::render(&storage, "raw.eml", "out.txt", 24 * 1024).unwrap();
    assert!(info.truncated);
    assert_eq!(info.text_len, 24 * 1024);
}

// ---- the store ----

#[test]
fn rows_are_written_and_read_the_same() {
    let flags = vec![
        "\\Seen".to_string(),
        "\\Flagged".to_string(),
        "\\X".to_string(),
    ];
    let raw = b"From: \"Ada, L\" <ada@x.org>\r\nSubject: Tab\there \xe4\r\nDate: Tue, 3 Nov 2026 10:00:00 +0100\r\nMessage-ID: <a@x>\r\n\r\n";
    let old = old_store::Row::from_headers(7, &flags, 55, 1234, raw);
    let new = new_store::Row::from_headers(7, &flags, 55, 1234, raw).unwrap();
    assert_eq!(old.line(), new.line().unwrap());
    assert_eq!(
        (old.seen(), old.flagged(), old.date, old.size),
        (new.seen(), new.flagged(), new.date, new.size)
    );
    let none = old_store::Row::from_headers(1, &[], 9, 10, b"no headers");
    let none_new = new_store::Row::from_headers(1, &[], 9, 10, b"no headers").unwrap();
    assert_eq!(none.line(), none_new.line().unwrap());
    for line in [
        "7\tSF\t55\t1234\tAda\tada@x.org\tSubject\t<a@x>",
        "7\tS\t55\t1234\tAda\tada@x.org\tSubject",
        "x\tS\t55\t1234\tAda\tada@x.org\tSubject",
        "7\tS\t55\tbig\tAda\tada@x.org\tSubject",
        "7\tS\t55",
        "",
    ] {
        let old = old_store::Row::parse(line);
        let new = new_store::Row::parse(line).unwrap();
        assert_eq!(
            old.map(|r| r.line()),
            new.map(|r| r.line().unwrap()),
            "{line:?}"
        );
    }
}

#[test]
fn the_index_is_written_and_read_the_same() {
    let old_storage = MemoryStorage::new();
    let new_storage = MemoryStorage::new();
    let rows: Vec<(u32, String)> = (0..70)
        .map(|n| (n, format!("subject {n}\twith tab")))
        .collect();
    let old_rows: Vec<old_store::Row> = rows
        .iter()
        .map(|(uid, subject)| old_store::Row {
            uid: *uid,
            flags: "S".into(),
            date: 5,
            size: 6,
            from: "A".into(),
            from_address: "a@x".into(),
            subject: subject.clone(),
            message_id: "<m>".into(),
        })
        .collect();
    let new_rows: Vec<new_store::Row> = old_rows
        .iter()
        .map(|r| new_store::Row::parse(&r.line()).unwrap().unwrap())
        .collect();
    old_store::write_index(&old_storage, "i.txt", &old_rows).unwrap();
    new_store::write_index(&new_storage, "i.txt", &new_rows).unwrap();
    assert_eq!(old_storage.get("i.txt"), new_storage.get("i.txt"));
    old_store::write_index(&old_storage, "i.txt", &old_rows[..3]).unwrap();
    new_store::write_index(&new_storage, "i.txt", &new_rows[..3]).unwrap();
    assert_eq!(old_storage.get("i.txt"), new_storage.get("i.txt"));
    assert_eq!(
        old_store::index_serial(&old_storage, "i.txt"),
        new_store::index_serial(&new_storage, "i.txt")
    );
    assert_eq!(new_store::index_serial(&new_storage, "i.txt"), 2);
    let read_old: Vec<String> = old_store::read_index(&old_storage, "i.txt")
        .iter()
        .map(old_store::Row::line)
        .collect();
    let read_new: Vec<String> = new_store::read_index(&new_storage, "i.txt")
        .unwrap()
        .iter()
        .map(|r| r.line().unwrap())
        .collect();
    assert_eq!(read_old, read_new);
    assert!(
        new_store::read_index(&new_storage, "none.txt")
            .unwrap()
            .is_empty()
    );
    // A file over the cap is an empty list, as in interface 6.
    let big = MemoryStorage::new().with("big.txt", vec![b'x'; 70 * 1024]);
    assert!(new_store::read_index(&big, "big.txt").unwrap().is_empty());
}

#[test]
fn counts_and_folders_are_kept_the_same() {
    let old = old_store::Counts {
        messages: 12,
        unseen: 3,
        uid_validity: 987_654_321_012,
        checked: 1_700_000_000,
    };
    let storage = MemoryStorage::new().with("c.txt", old.line());
    let new = new_store::Counts::read(&storage, "c.txt").unwrap();
    assert_eq!(new.line().unwrap(), old.line());
    assert_eq!(new_store::Counts::read(&storage, "none.txt"), None);
    let broken = MemoryStorage::new().with("c.txt", "1\t2\n");
    assert_eq!(new_store::Counts::read(&broken, "c.txt"), None);

    let names = [
        ("Sent", "\\sent"),
        ("INBOX", ""),
        ("Zeta", ""),
        ("alpha", ""),
        ("Trash", "\\trash \\noselect"),
        ("Archiv/2026", ""),
        ("&AOQA9gD8-", ""),
        ("", ""),
    ];
    let mut text = String::new();
    for (raw, flags) in names {
        text.push_str(&format!("{raw}\t{flags}\n"));
    }
    let old_storage = MemoryStorage::new().with("f.txt", text.clone());
    let new_storage = MemoryStorage::new().with("f.txt", text);
    let mut old_folders = old_store::read_folders(&old_storage, "f.txt");
    let mut new_folders = new_store::read_folders(&new_storage, "f.txt").unwrap();
    assert_eq!(old_folders.len(), new_folders.len());
    old_store::sort_folders(&mut old_folders);
    new_store::sort_folders(&mut new_folders);
    let old_names: Vec<_> = old_folders
        .iter()
        .map(|f| (f.raw.clone(), f.name.clone(), f.selectable()))
        .collect();
    let new_names: Vec<_> = new_folders
        .iter()
        .map(|f| (f.raw.clone(), f.name.clone(), f.selectable()))
        .collect();
    assert_eq!(old_names, new_names);
    old_store::write_folders(&old_storage, "g.txt", &old_folders).unwrap();
    new_store::write_folders(&new_storage, "g.txt", &new_folders).unwrap();
    assert_eq!(old_storage.get("g.txt"), new_storage.get("g.txt"));
}

#[test]
fn keys_and_fields_are_the_same() {
    for name in [
        "INBOX",
        "Sent Items",
        "Archiv/2026/März",
        "&AOQA9gD8-",
        "",
        "a b c d e f g h i j k l m n",
    ] {
        let key = old_store::folder_key(name);
        assert_eq!(new_store::folder_key(name).unwrap(), key, "{name:?}");
        assert!(
            new_store::index_file(2, name)
                .unwrap()
                .ends_with("/index.txt")
        );
        assert!(new_store::folder_dir(2, name).unwrap().contains(&key));
    }
    assert_eq!(new_store::account_dir(2).unwrap(), "a2");
    assert_eq!(
        new_store::text_file(1, "INBOX", 42).unwrap(),
        format!("a1/{}/m42.txt", old_store::folder_key("INBOX"))
    );
    assert_eq!(
        new_store::raw_file(1, "INBOX", 42).unwrap(),
        format!("a1/{}/m42.eml", old_store::folder_key("INBOX"))
    );
    assert_eq!(new_store::draft_file(3).unwrap(), "a3/draft.txt");
    assert_eq!(new_store::outbox_file(3).unwrap(), "a3/outbox.eml");
    for text in [
        "plain",
        "  padded  ",
        "tab\there",
        "line\nbreak\r\n",
        "\u{1}ctl\u{7f}",
        "caf\u{e9}\u{3000}",
        "",
    ] {
        assert_eq!(
            new_store::field(text).unwrap(),
            old_store::field(text),
            "{text:?}"
        );
    }
}

#[test]
fn a_letter_is_made_kept_and_read_the_same() {
    for mail in FIXED
        .iter()
        .chain(&["Content-Type: text/html\n\n<p>x</p>\n"])
    {
        let old_storage = MemoryStorage::new().with("raw.eml", mail.as_bytes());
        let new_storage = MemoryStorage::new().with("raw.eml", mail.as_bytes());
        let old = old_store::make_letter(&old_storage, "raw.eml", "m1.txt").unwrap();
        let new = new_store::make_letter(&new_storage, "raw.eml", "m1.txt").unwrap();
        assert_eq!(
            old_storage.get("m1.txt"),
            new_storage.get("m1.txt"),
            "{mail:?}"
        );
        assert_eq!(old.header_text(), new.header_text().unwrap(), "{mail:?}");
        assert_eq!(old.text, new.text);
        assert_eq!(old.truncated, new.truncated);
        // Both remove the raw message and their scratch file.
        assert!(new_storage.get("raw.eml").is_none());
        assert!(new_storage.get("m1.txt.tmp").is_none());
        let read_old = old_store::read_letter(&old_storage, "m1.txt").unwrap();
        let read_new = new_store::read_letter(&new_storage, "m1.txt")
            .unwrap()
            .unwrap();
        assert_eq!(read_old.header_text(), read_new.header_text().unwrap());
        assert_eq!(read_old.text, read_new.text);
        assert_eq!(read_old.attachments, read_new.attachments);
    }
    let storage = MemoryStorage::new();
    assert!(
        new_store::read_letter(&storage, "none.txt")
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_card_that_says_no_is_told_apart_from_no_memory() {
    let storage = MemoryStorage::new();
    let error = new_render::render(&storage, "missing.eml", "out.txt", 1024).unwrap_err();
    assert_eq!(error, Fail::Storage(StorageError::NotFound));
    let error = new_store::make_letter(&storage, "missing.eml", "m.txt").unwrap_err();
    assert_eq!(error, Fail::Storage(StorageError::NotFound));
}
