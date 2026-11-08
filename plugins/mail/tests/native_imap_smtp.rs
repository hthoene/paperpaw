//! The native IMAP and SMTP readers give the same answers as the interface-6
//! ones on what both take: fixed lines, random lines, lines cut in pieces.
//! Types differ, so answers are compared by their `Debug` text (the variants
//! and fields have the same names). The native ones add bounds and fix a
//! FETCH line that ends in white space; those differences are tested too.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_mail::native_model::imap::{self as new_imap, Fault};
use paperpaw_plugin_mail::native_model::smtp as new_smtp;
use paperpaw_plugin_mail::{imap, smtp};

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

fn old_response(line: &[u8]) -> String {
    format!("{:?}", imap::response(line))
}

fn new_response(line: &[u8]) -> String {
    format!("{:?}", new_imap::response(line).unwrap())
}

#[test]
fn fixed_lines_are_read_the_same() {
    for line in LINES {
        let bytes = line.as_bytes();
        assert_eq!(new_response(bytes), old_response(bytes), "{line:?}");
        assert_eq!(
            format!("{:?}", new_imap::tokens(bytes).unwrap()),
            format!("{:?}", imap::tokens(bytes)),
            "{line:?}"
        );
        assert_eq!(
            new_imap::literal_at_end(bytes),
            imap::literal_at_end(bytes),
            "{line:?}"
        );
    }
}

fn random_line(rng: &mut Rng) -> Vec<u8> {
    const PIECES: &[&str] = &[
        "* ",
        "* ",
        "A1 ",
        "+ ",
        "OK ",
        "NO ",
        "BAD ",
        "LIST ",
        "LSUB ",
        "STATUS ",
        "SEARCH ",
        "FETCH ",
        "EXISTS ",
        "CAPABILITY ",
        "BYE ",
        "UID ",
        "FLAGS ",
        "NIL ",
        "(",
        ")",
        "(",
        ")",
        "\"",
        "\\",
        "{",
        "}",
        "{12}",
        "{3+}",
        "[",
        "]",
        "[CODE a b]",
        " ",
        " ",
        " ",
        "12",
        "4",
        "999999999999999",
        "\\Seen",
        "INBOX",
        "x",
        "\"/\"",
        "&AOQ-",
        "&",
        "-",
        "caf\u{e9}",
        "\u{4e16}",
        "\t",
        "RFC822.SIZE",
        "MESSAGES",
        "UNSEEN",
        "BODY[HEADER.FIELDS (FROM)]",
    ];
    let mut line = Vec::new();
    for _ in 0..rng.below(24) {
        line.extend_from_slice(PIECES[rng.below(PIECES.len())].as_bytes());
    }
    // A FETCH line ending in white space is read differently (see below).
    while matches!(line.last(), Some(b' ' | b'\t')) {
        line.pop();
    }
    line
}

#[test]
fn random_lines_are_read_the_same() {
    let mut rng = Rng(3);
    for _ in 0..6000 {
        let line = random_line(&mut rng);
        assert_eq!(new_response(&line), old_response(&line), "{line:?}");
        assert_eq!(
            format!("{:?}", new_imap::tokens(&line).unwrap()),
            format!("{:?}", imap::tokens(&line)),
            "{line:?}"
        );
        assert_eq!(new_imap::literal_at_end(&line), imap::literal_at_end(&line));
    }
}

#[test]
fn the_fetch_helpers_read_the_same() {
    let line = b"* 12 FETCH (UID 42 FLAGS (\\Seen \\Answered \\Deleted) RFC822.SIZE 1000 INTERNALDATE \"d\")";
    let (Some(imap::Response::Untagged(imap::Untagged::Fetch { items: old, .. })), Some(new)) =
        (imap::response(line), new_imap::response(line).unwrap())
    else {
        panic!("not a fetch");
    };
    let new_imap::Response::Untagged(new_imap::Untagged::Fetch { items: new, .. }) = new else {
        panic!("not a fetch");
    };
    for key in [
        "UID",
        "uid",
        "RFC822.SIZE",
        "FLAGS",
        "nothing",
        "INTERNALDATE",
    ] {
        assert_eq!(
            new_imap::fetch_number(&new, key),
            imap::fetch_number(&old, key)
        );
        assert_eq!(new_imap::fetch_text(&new, key), imap::fetch_text(&old, key));
    }
    let flags = new_imap::fetch_flags(&new).unwrap();
    assert_eq!(flags, imap::fetch_flags(&old));
    assert_eq!(
        new_imap::flag_letters(&flags).unwrap(),
        imap::flag_letters(&flags)
    );
    assert_eq!(new_imap::fetch_flags(&[]).unwrap(), Vec::<String>::new());
}

#[test]
fn quoting_and_mailbox_names_are_the_same() {
    for text in [
        "",
        "plain",
        "with \"quote\" and \\ slash",
        "tab\tand\nnewline\u{7f}",
        "caf\u{e9} \u{4e16}\u{754c}",
    ] {
        assert_eq!(new_imap::quoted(text).unwrap(), imap::quoted(text));
        assert_eq!(
            new_imap::quoted_secret(text).unwrap().as_str(),
            imap::quoted(text)
        );
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
        assert_eq!(
            new_imap::mailbox_display(name).unwrap(),
            imap::mailbox_display(name),
            "{name:?}"
        );
    }
    let mut rng = Rng(8);
    let alphabet: Vec<char> = "&-,AOQPYw/+9z\u{e9}x ".chars().collect();
    for _ in 0..3000 {
        let name: String = (0..rng.below(14))
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        assert_eq!(
            new_imap::mailbox_display(&name).unwrap(),
            imap::mailbox_display(&name),
            "{name:?}"
        );
    }
}

#[test]
fn the_line_buffer_cuts_lines_and_literals_the_same() {
    let mut rng = Rng(21);
    for _ in 0..200 {
        let mut input = Vec::new();
        for _ in 0..rng.below(8) {
            let line = random_line(&mut rng);
            input.extend_from_slice(&line);
            input.extend_from_slice(["\r\n", "\n", "\r\n", "\r"][rng.below(4)].as_bytes());
        }
        let (mut old, mut new) = (imap::LineBuffer::default(), new_imap::LineBuffer::default());
        let mut at = 0;
        while at < input.len() {
            let end = (at + 1 + rng.below(40)).min(input.len());
            old.push(&input[at..end]);
            new.push(&input[at..end]).unwrap();
            at = end;
            loop {
                let (a, b) = (old.line().unwrap(), new.line().unwrap());
                assert_eq!(a, b);
                if a.is_none() {
                    break;
                }
            }
            let n = rng.below(6);
            assert_eq!(old.take(n), new.take(n).unwrap());
            assert_eq!((old.len(), old.is_empty()), (new.len(), new.is_empty()));
        }
    }
}

#[test]
fn what_is_too_long_is_refused() {
    let long = vec![b'x'; new_imap::MAX_LINE + 1];
    assert_eq!(new_imap::response(&long), Err(Fault::TooLong));
    assert_eq!(new_imap::tokens(&long), Err(Fault::TooLong));
    let mut buffer = new_imap::LineBuffer::default();
    buffer.push(&long).unwrap();
    assert_eq!(buffer.line(), Err(Fault::TooLong));
    // A complete long line is dropped; the next is read.
    buffer.push(b"\r\nA1 OK done\r\n").unwrap();
    assert_eq!(buffer.line(), Err(Fault::TooLong));
    assert_eq!(buffer.line().unwrap().unwrap(), b"A1 OK done");
    // The buffer holds at most MAX_BUFFER.
    let mut buffer = new_imap::LineBuffer::default();
    let chunk = vec![b'y'; 1024];
    for _ in 0..(new_imap::MAX_BUFFER / 1024) {
        buffer.push(&chunk).unwrap();
    }
    assert_eq!(buffer.push(b"z"), Err(Fault::TooLong));
    assert_eq!(buffer.len(), new_imap::MAX_BUFFER);
    // A line of exactly MAX_LINE is taken.
    let mut buffer = new_imap::LineBuffer::default();
    buffer.push(&vec![b'a'; new_imap::MAX_LINE]).unwrap();
    buffer.push(b"\n").unwrap();
    assert_eq!(buffer.line().unwrap().unwrap().len(), new_imap::MAX_LINE);
}

#[test]
fn a_fetch_line_is_read_from_its_own_bytes() {
    // Interface 6 counted from the end of the line, which cut the first token
    // off a line that ended in white space or held bytes that are no UTF-8.
    let trailing = b"* 1 FETCH (UID 5) ";
    let items = |bytes: &[u8]| match new_imap::response(bytes).unwrap() {
        Some(new_imap::Response::Untagged(new_imap::Untagged::Fetch { items, .. })) => items,
        other => panic!("{other:?}"),
    };
    assert_eq!(new_imap::fetch_number(&items(trailing), "UID"), Some(5));
    let broken = b"* 1 FETCH (UID 6 NAME \"\xff\xfe\")";
    assert_eq!(new_imap::fetch_number(&items(broken), "UID"), Some(6));
    let Some(imap::Response::Untagged(imap::Untagged::Fetch { items: old, .. })) =
        imap::response(trailing)
    else {
        panic!("not a fetch");
    };
    assert!(old.is_empty(), "interface 6 was expected to lose the items");
}

#[test]
fn a_password_is_quoted_into_a_buffer_that_hides_it() {
    let secret = new_imap::quoted_secret("pa\"ss\\word").unwrap();
    assert_eq!(secret.as_str(), "\"pa\\\"ss\\\\word\"");
    assert!(!format!("{secret:?}").contains("pa"));
    let line = new_smtp::auth_plain("ada", "hunter2").unwrap();
    assert_eq!(line.as_str(), smtp::auth_plain("ada", "hunter2"));
    assert!(!format!("{line:?}").contains("hunter2"));
}

fn old_reply(reply: &Option<smtp::Reply>) -> String {
    format!("{reply:?}")
}

fn new_reply(reply: &Option<new_smtp::Reply>) -> String {
    format!("{reply:?}")
}

#[test]
fn replies_are_put_together_the_same() {
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
    for script in scripts {
        let (mut old, mut new) = (
            smtp::ReplyParser::default(),
            new_smtp::ReplyParser::default(),
        );
        for line in *script {
            let (a, b) = (
                old.line(line.as_bytes()),
                new.line(line.as_bytes()).unwrap(),
            );
            assert_eq!(new_reply(&b), old_reply(&a), "{line:?}");
            if let (Some(a), Some(b)) = (&a, &b) {
                assert_eq!(
                    format!("{:?}", new_smtp::Extensions::of(b)),
                    format!("{:?}", smtp::Extensions::of(a))
                );
                assert_eq!(
                    (b.is_ok(), b.is_temporary(), b.text()),
                    (a.is_ok(), a.is_temporary(), a.text())
                );
            }
        }
    }
    // Too long a line, too long a reply, too many lines.
    let long = "x".repeat(1001);
    let (mut old, mut new) = (
        smtp::ReplyParser::default(),
        new_smtp::ReplyParser::default(),
    );
    assert_eq!(
        new_reply(&new.line(long.as_bytes()).unwrap()),
        old_reply(&old.line(long.as_bytes()))
    );
    let (mut old, mut new) = (
        smtp::ReplyParser::default(),
        new_smtp::ReplyParser::default(),
    );
    for _ in 0..200 {
        let line = "250-0123456789012345678901234567890123456789";
        assert_eq!(
            new_reply(&new.line(line.as_bytes()).unwrap()),
            old_reply(&old.line(line.as_bytes()))
        );
    }
}

#[test]
fn random_replies_are_put_together_the_same() {
    let mut rng = Rng(77);
    const PIECES: &[&str] = &[
        "250",
        "250-",
        "250 ",
        "354",
        "451 ",
        "550-",
        "55",
        "x",
        "STARTTLS",
        "AUTH PLAIN",
        "SIZE 100",
        "AUTH=LOGIN",
        " ",
        "caf\u{e9}",
        "\u{4e16}",
        "-",
        "2",
        "000",
        "999",
    ];
    for _ in 0..400 {
        let (mut old, mut new) = (
            smtp::ReplyParser::default(),
            new_smtp::ReplyParser::default(),
        );
        for _ in 0..rng.below(12) {
            let mut line = String::new();
            for _ in 0..rng.below(6) {
                line.push_str(PIECES[rng.below(PIECES.len())]);
            }
            let (a, b) = (
                old.line(line.as_bytes()),
                new.line(line.as_bytes()).unwrap(),
            );
            assert_eq!(new_reply(&b), old_reply(&a), "{line:?}");
            if let (Some(a), Some(b)) = (&a, &b) {
                assert_eq!(
                    format!("{:?}", new_smtp::Extensions::of(b)),
                    format!("{:?}", smtp::Extensions::of(a))
                );
            }
        }
    }
}

#[test]
fn smtp_commands_and_data_are_the_same() {
    for address in ["a@b.c", "a@b.c>\r\nDATA", "<x y@z>", "caf\u{e9}@x.org", ""] {
        assert_eq!(
            new_smtp::mail_from(address).unwrap(),
            smtp::mail_from(address)
        );
        assert_eq!(new_smtp::rcpt_to(address).unwrap(), smtp::rcpt_to(address));
        assert_eq!(
            new_smtp::valid_address(address),
            smtp::valid_address(address)
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
    ] {
        assert_eq!(
            new_smtp::data(message).unwrap(),
            smtp::data(message),
            "{message:?}"
        );
    }
    let mut rng = Rng(5);
    let alphabet: Vec<char> = ".\r\nab \u{e9}".chars().collect();
    for _ in 0..2000 {
        let message: String = (0..rng.below(20))
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        assert_eq!(
            new_smtp::data(&message).unwrap(),
            smtp::data(&message),
            "{message:?}"
        );
    }
}

#[test]
fn hostile_bytes_are_read_without_a_panic() {
    let mut rng = Rng(99);
    for len in [0usize, 1, 2, 5, 30, 300, 3000, 8000] {
        for _ in 0..40 {
            let line: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            let _ = new_imap::response(&line).unwrap();
            let _ = new_imap::tokens(&line).unwrap();
            let _ = new_imap::literal_at_end(&line);
            let mut parser = new_smtp::ReplyParser::default();
            let _ = parser.line(&line).unwrap();
        }
    }
    // Lines made only of what parts tokens, and of unclosed things.
    for text in [
        "(".repeat(8000),
        ")".repeat(8000),
        "{".repeat(8000),
        "\"".repeat(8000),
        "[".repeat(8000),
        "* 1 FETCH ".to_owned() + &"(".repeat(7000),
    ] {
        let _ = new_imap::response(text.as_bytes()).unwrap();
        let _ = new_imap::tokens(text.as_bytes()).unwrap();
    }
}
