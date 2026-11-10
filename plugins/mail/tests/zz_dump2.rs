//! TEMPORARY
#![allow(clippy::all, clippy::pedantic, missing_docs, dead_code, unused)]
use paperpaw_plugin_mail::{compose, header, imap, render, session, smtp, store};
use paperpaw_sdk::{MemoryStorage, Storage};
use std::fmt::Write as _;

struct Out(String);
impl Out {
    fn push(&mut self, tag: &str, content: String) {
        assert!(!content.contains('\n'), "{content}");
        writeln!(self.0, "{tag}\t{content}").unwrap();
    }
}
fn write(name: &str, out: Out) {
    std::fs::write(
        format!("{}/testdata/{name}.golden", env!("CARGO_MANIFEST_DIR")),
        out.0,
    )
    .unwrap();
}

const LINES: &[&str] = &[
    "",
    "A1 OK LOGIN completed",
    "a2 no [AUTHENTICATIONFAILED] Invalid credentials",
    "A3 BAD Command unknown",
    "A4 MAYBE what",
    "A5",
    "+ go ahead",
    "+",
    "* CAPABILITY IMAP4rev1 STARTTLS AUTH=PLAIN LITERAL+ idle",
    "* LIST (\\HasNoChildren \\Trash) \"/\" \"Deleted Items\"",
    "* LSUB () \".\" INBOX",
    "* LIST (\\Noselect) NIL INBOX",
    "* LIST \\Bad",
    "* 23 EXISTS",
    "* 5 RECENT",
    "* 7 expunge",
    "* SEARCH 2 84 882 x 7",
    "* SEARCH",
    "* STATUS \"INBOX\" (MESSAGES 231 UNSEEN 7 bad x)",
    "* STATUS INBOX",
    "* BYE Server shutting down  ",
    "* OK [UIDVALIDITY 3857529045] UIDs valid",
    "* ok [capability IMAP4rev1 IDLE] ready",
    "* PREAUTH [CAPABILITY X] hi",
    "* NO [ALERT] careful",
    "* OK no code here",
    "* OK [unclosed",
    "* FLAGS (\\Answered \\Flagged)",
    "* 12 FETCH (UID 42 FLAGS (\\Seen \\Answered) RFC822.SIZE 1000 INTERNALDATE \"08-Nov-2026 10:00:00 +0100\")",
    "* 3 FETCH (BODY[HEADER.FIELDS (FROM TO)] {342}",
    "* 3 fetch (UID 9 BODY[] {1024+}",
    "* 4 FETCH NIL",
    "* 4 FETCH",
    "* 4 FETCH (((((((deep)))))))",
    "* 1 FETCH (UID 1 \"quo\\\"ted\\\\ \" nil NIL {12} {} {x} { 3} ) )",
    "* 1 FETCH (\"open quote",
    "* 1 FETCH (UID \u{e4}\u{4e16} caf\u{e9})",
    "* LIST () \"/\" &AOQ-bc",
    "garbage with spaces",
    "* ",
    "*",
];

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
fn dump_imap() {
    let mut out = Out(String::new());
    for line in LINES {
        let b = line.as_bytes();
        out.push(
            "line",
            format!(
                "{line:?}\t{:?}\t{:?}\t{:?}",
                imap::response(b),
                imap::tokens(b),
                imap::literal_at_end(b)
            ),
        );
    }
    let line = b"* 12 FETCH (UID 42 FLAGS (\\Seen \\Answered \\Deleted) RFC822.SIZE 1000 INTERNALDATE \"d\")";
    let Some(imap::Response::Untagged(imap::Untagged::Fetch { items, .. })) = imap::response(line)
    else {
        panic!()
    };
    for key in [
        "UID",
        "uid",
        "RFC822.SIZE",
        "FLAGS",
        "nothing",
        "INTERNALDATE",
    ] {
        out.push(
            "fetch",
            format!(
                "{key}\t{:?}\t{:?}",
                imap::fetch_number(&items, key),
                imap::fetch_text(&items, key)
            ),
        );
    }
    let flags = imap::fetch_flags(&items);
    out.push(
        "flags",
        format!("{flags:?}\t{:?}", imap::flag_letters(&flags)),
    );
    for text in [
        "",
        "plain",
        "with \"quote\" and \\ slash",
        "tab\tand\nnewline\u{7f}",
        "caf\u{e9} \u{4e16}\u{754c}",
    ] {
        out.push("quoted", format!("{text:?}\t{:?}", imap::quoted(text)));
    }
    for name in [
        "INBOX",
        "&AOQ-",
        "Entw&APw-rfe",
        "&-",
        "a&-b",
        "&AOQA9gD8-",
        "&2D3eAA-",
        "&2D0-",
        "&AO-",
        "&!!-",
        "&unclosed",
        "x&AOQ-y&APY-z",
        "Sent,Items",
        "&AOQ,",
        "",
    ] {
        out.push(
            "mailbox",
            format!("{name:?}\t{:?}", imap::mailbox_display(name)),
        );
    }
    // a fetch line ending in white space
    out.push(
        "trailing",
        format!("{:?}", imap::response(b"* 1 FETCH (UID 5) ")),
    );
    out.push(
        "braces",
        format!("{:?}", imap::tokens(&vec![b'{'; 8 * 1024])),
    );
    let junk: Vec<u8> = (0..4000u32).map(|n| (n % 251) as u8).collect();
    out.push("junk", format!("{}", imap::tokens(&junk).len()));
    out.push("literal12", format!("{:?}", imap::tokens(b"{12+}")));
    out.push(
        "literal18",
        format!("{:?}", imap::tokens(b"{123456789012345678}")),
    );
    // smtp
    let scripts: &[&[&str]] = &[
        &[
            "250-mail.example.org",
            "250-STARTTLS",
            "250-AUTH PLAIN LOGIN XOAUTH2",
            "250-SIZE 35882577",
            "250 8BITMIME",
        ],
        &[
            "354",
            "garbage",
            "451 try later",
            "250",
            "25",
            "abc def",
            "",
        ],
        &["250-a", "251 b", "250 c"],
        &["220-hello", "220-world", "220 ready"],
        &[
            "250 caf\u{e9} \u{4e16}",
            "550-5.1.1 no",
            "550 5.1.1 such user",
        ],
        &[
            "250-AUTH=PLAIN LOGIN",
            "250-auth=plain",
            "250-size 12",
            "250-Size x",
            "250 STARTTLS",
        ],
    ];
    for (n, script) in scripts.iter().enumerate() {
        let mut parser = smtp::ReplyParser::default();
        for line in *script {
            let reply = parser.line(line.as_bytes());
            let more = match &reply {
                Some(r) => format!(
                    "{:?}\t{:?}",
                    smtp::Extensions::of(r),
                    (r.is_ok(), r.is_temporary(), r.text())
                ),
                None => String::new(),
            };
            out.push("reply", format!("{n}\t{line:?}\t{reply:?}\t{more}"));
        }
    }
    let long = "x".repeat(1001);
    let mut parser = smtp::ReplyParser::default();
    out.push("reply_long", format!("{:?}", parser.line(long.as_bytes())));
    let mut parser = smtp::ReplyParser::default();
    let mut ended = None;
    for n in 0..200 {
        ended = parser.line("250-0123456789012345678901234567890123456789".as_bytes());
        if ended.is_some() {
            out.push("reply_many", format!("{n}\t{ended:?}"));
            break;
        }
    }
    let mut parser = smtp::ReplyParser::default();
    let long = format!("250-{}", "x".repeat(1100));
    out.push(
        "reply_long_dash",
        format!("{:?}", parser.line(long.as_bytes()).map(|r| r.code)),
    );
    let mut parser = smtp::ReplyParser::default();
    let mut ended = None;
    for n in 0..200 {
        ended = parser.line(format!("250-{}", "y".repeat(200)).as_bytes());
        if ended.is_some() {
            out.push(
                "reply_many_long",
                format!("{n}\t{:?}", ended.map(|r| r.code)),
            );
            break;
        }
    }
    out.push(
        "auth_plain",
        format!("{:?}", smtp::auth_plain("ada", "hunter2")),
    );
    for address in ["a@b.c", "a@b.c>\r\nDATA", "<x y@z>", "caf\u{e9}@x.org", ""] {
        out.push(
            "command",
            format!(
                "{address:?}\t{:?}\t{:?}\t{:?}",
                smtp::mail_from(address),
                smtp::rcpt_to(address),
                smtp::valid_address(address)
            ),
        );
    }
    for message in [
        "a\n.b\n..c\n",
        "no end",
        "x\r\n.\r\n",
        "",
        "a\rb",
        "a\r\nb\r.c",
        ".",
        "\r",
        "\n\n.\n",
        "line\r\n.dot\r\n..two\r\n",
    ] {
        out.push(
            "data",
            format!(
                "{message:?}\t{:?}",
                String::from_utf8_lossy(&smtp::data(message))
            ),
        );
    }
    write("imap_smtp", out);
}

fn pairs(list: &[header::Address]) -> Vec<(String, String)> {
    list.iter()
        .map(|a| (a.name.clone(), a.address.clone()))
        .collect()
}

#[test]
fn dump_render() {
    let mut out = Out(String::new());
    for mail in FIXED {
        for limit in [24 * 1024, 40, 7, 0] {
            let storage = MemoryStorage::new().with("raw.eml", mail.as_bytes());
            let info = render::render(&storage, "raw.eml", "out.txt", limit).unwrap();
            let text = storage
                .get("out.txt")
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            out.push(
                "render",
                format!(
                    "{mail:?}\t{limit}\t{text:?}\t{:?}",
                    (
                        pairs(&info.from),
                        pairs(&info.to),
                        pairs(&info.cc),
                        pairs(&info.reply_to),
                        &info.subject,
                        info.date,
                        &info.message_id,
                        &info.references,
                        &info.attachments,
                        info.truncated,
                        info.text_len
                    )
                ),
            );
        }
    }
    for mail in FIXED
        .iter()
        .chain(&["Content-Type: text/html\n\n<p>x</p>\n"])
    {
        let storage = MemoryStorage::new().with("raw.eml", mail.as_bytes());
        let letter = store::make_letter(&storage, "raw.eml", "m1.txt").unwrap();
        let kept = storage
            .get("m1.txt")
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let read = store::read_letter(&storage, "m1.txt").unwrap();
        out.push(
            "letter",
            format!(
                "{mail:?}\t{kept:?}\t{:?}\t{:?}\t{}\t{:?}\t{:?}\t{:?}",
                letter.header_text(),
                letter.text,
                letter.truncated,
                read.header_text(),
                read.text,
                read.attachments
            ),
        );
    }
    // rows
    let flags = vec![
        "\\Seen".to_string(),
        "\\Flagged".to_string(),
        "\\X".to_string(),
    ];
    let raw = b"From: \"Ada, L\" <ada@x.org>\r\nSubject: Tab\there \xe4\r\nDate: Tue, 3 Nov 2026 10:00:00 +0100\r\nMessage-ID: <a@x>\r\n\r\n";
    let row = store::Row::from_headers(7, &flags, 55, 1234, raw);
    out.push(
        "row",
        format!(
            "{:?}\t{:?}",
            row.line(),
            (row.seen(), row.flagged(), row.date, row.size)
        ),
    );
    let none = store::Row::from_headers(1, &[], 9, 10, b"no headers");
    out.push("row", format!("{:?}", none.line()));
    for line in [
        "7\tSF\t55\t1234\tAda\tada@x.org\tSubject\t<a@x>",
        "7\tS\t55\t1234\tAda\tada@x.org\tSubject",
        "x\tS\t55\t1234\tAda\tada@x.org\tSubject",
        "7\tS\t55\tbig\tAda\tada@x.org\tSubject",
        "7\tS\t55",
        "",
    ] {
        out.push(
            "row_parse",
            format!("{line:?}\t{:?}", store::Row::parse(line).map(|r| r.line())),
        );
    }
    // index
    let storage = MemoryStorage::new();
    let rows: Vec<store::Row> = (0..70u32)
        .map(|n| store::Row {
            uid: n,
            flags: "S".into(),
            date: 5,
            size: 6,
            from: "A".into(),
            from_address: "a@x".into(),
            subject: format!("subject {n}\twith tab"),
            message_id: "<m>".into(),
        })
        .collect();
    store::write_index(&storage, "i.txt", &rows).unwrap();
    let text = String::from_utf8(storage.get("i.txt").unwrap()).unwrap();
    for l in text.lines() {
        out.push("index70", format!("{l:?}"));
    }
    store::write_index(&storage, "i.txt", &rows[..3]).unwrap();
    out.push(
        "index3",
        format!(
            "{:?}",
            String::from_utf8(storage.get("i.txt").unwrap()).unwrap()
        ),
    );
    out.push(
        "index_serial",
        format!("{}", store::index_serial(&storage, "i.txt")),
    );
    let read: Vec<String> = store::read_index(&storage, "i.txt")
        .iter()
        .map(store::Row::line)
        .collect();
    out.push("index_read", format!("{read:?}"));
    // counts
    let counts = store::Counts {
        messages: 12,
        unseen: 3,
        uid_validity: 987_654_321_012,
        checked: 1_700_000_000,
    };
    out.push("counts", format!("{:?}", counts.line()));
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
    let storage = MemoryStorage::new().with("f.txt", text);
    let mut folders = store::read_folders(&storage, "f.txt");
    out.push("folders_len", format!("{}", folders.len()));
    store::sort_folders(&mut folders);
    for f in &folders {
        out.push(
            "folders",
            format!("{:?}", (&f.raw, &f.name, f.selectable())),
        );
    }
    store::write_folders(&storage, "g.txt", &folders).unwrap();
    out.push(
        "folders_file",
        format!(
            "{:?}",
            String::from_utf8(storage.get("g.txt").unwrap()).unwrap()
        ),
    );
    for name in [
        "INBOX",
        "Sent Items",
        "Archiv/2026/M\u{e4}rz",
        "&AOQA9gD8-",
        "",
        "a b c d e f g h i j k l m n",
    ] {
        out.push(
            "folder_key",
            format!("{name:?}\t{:?}", store::folder_key(name)),
        );
    }
    out.push(
        "paths",
        format!(
            "{:?}",
            (
                store::root(),
                store::draft_file(1),
                store::outbox_file(2),
                store::account_dir(2),
                store::text_file(1, "INBOX", 42),
                store::raw_file(1, "INBOX", 42)
            )
        ),
    );
    for text in [
        "plain",
        "  padded  ",
        "tab\there",
        "line\nbreak\r\n",
        "\u{1}ctl\u{7f}",
        "caf\u{e9}\u{3000}",
        "",
    ] {
        out.push("field", format!("{text:?}\t{:?}", store::field(text)));
    }
    write("render_store", out);
}
