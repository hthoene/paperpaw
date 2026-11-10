//! The native IMAP and SMTP readers give the same answers as the interface-6
//! ones on what both take: fixed lines, random lines, lines cut in pieces.
//! Types differ, so answers are compared by their `Debug` text (the variants
//! and fields have the same names). The native ones add bounds and fix a
//! FETCH line that ends in white space; those differences are tested too.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_mail::native_model::imap::{self as new_imap, Fault};
use paperpaw_plugin_mail::native_model::smtp as new_smtp;
mod pin;

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

fn new_response(line: &[u8]) -> String {
    format!("{:?}", new_imap::response(line).unwrap())
}

#[test]
fn fixed_lines_are_read_the_same() {
    let mut pin = pin::Pin::new("fixed_lines_are_read_the_same");
    for line in LINES {
        let bytes = line.as_bytes();
        pin.check(format!("{line:?}"), new_response(bytes));
        pin.check(
            format!("{line:?}"),
            format!("{:?}", new_imap::tokens(bytes).unwrap()),
        );
        pin.check(format!("{line:?}"), new_imap::literal_at_end(bytes));
    }
    pin.finish();
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
    let mut pin = pin::Pin::new("random_lines_are_read_the_same");
    let mut rng = Rng(3);
    for _ in 0..6000 {
        let line = random_line(&mut rng);
        pin.fold(format!("{line:?}"), new_response(&line));
        pin.fold(
            format!("{line:?}"),
            format!("{:?}", new_imap::tokens(&line).unwrap()),
        );
        pin.fold("", new_imap::literal_at_end(&line));
    }
    pin.finish();
}

#[test]
fn the_fetch_helpers_read_the_same() {
    let mut pin = pin::Pin::new("the_fetch_helpers_read_the_same");
    let line = b"* 12 FETCH (UID 42 FLAGS (\\Seen \\Answered \\Deleted) RFC822.SIZE 1000 INTERNALDATE \"d\")";
    let Some(new) = new_imap::response(line).unwrap() else {
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
        pin.check("", new_imap::fetch_number(&new, key));
        pin.check("", new_imap::fetch_text(&new, key));
    }
    let flags = new_imap::fetch_flags(&new).unwrap();
    pin.check("", &(flags));
    pin.check("", new_imap::flag_letters(&flags).unwrap());
    assert_eq!(new_imap::fetch_flags(&[]).unwrap(), Vec::<String>::new());
    pin.finish();
}

#[test]
fn quoting_and_mailbox_names_are_the_same() {
    let mut pin = pin::Pin::new("quoting_and_mailbox_names_are_the_same");
    for text in [
        "",
        "plain",
        "with \"quote\" and \\ slash",
        "tab\tand\nnewline\u{7f}",
        "caf\u{e9} \u{4e16}\u{754c}",
    ] {
        pin.fold("", new_imap::quoted(text).unwrap());
        pin.fold("", new_imap::quoted_secret(text).unwrap().as_str());
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
        pin.fold(
            format!("{name:?}"),
            new_imap::mailbox_display(name).unwrap(),
        );
    }
    let mut rng = Rng(8);
    let alphabet: Vec<char> = "&-,AOQPYw/+9z\u{e9}x ".chars().collect();
    for _ in 0..3000 {
        let name: String = (0..rng.below(14))
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        pin.fold(
            format!("{name:?}"),
            new_imap::mailbox_display(&name).unwrap(),
        );
    }
    pin.finish();
}

#[test]
fn the_line_buffer_cuts_lines_and_literals_the_same() {
    let mut pin = pin::Pin::new("the_line_buffer_cuts_lines_and_literals_the_same");
    let mut rng = Rng(21);
    for _ in 0..200 {
        let mut input = Vec::new();
        for _ in 0..rng.below(8) {
            let line = random_line(&mut rng);
            input.extend_from_slice(&line);
            input.extend_from_slice(["\r\n", "\n", "\r\n", "\r"][rng.below(4)].as_bytes());
        }
        let mut new = new_imap::LineBuffer::default();
        let mut at = 0;
        while at < input.len() {
            let end = (at + 1 + rng.below(40)).min(input.len());
            new.push(&input[at..end]).unwrap();
            at = end;
            loop {
                let line = new.line().unwrap();
                pin.fold("", &line);
                if line.is_none() {
                    break;
                }
            }
            let n = rng.below(6);
            pin.fold("", new.take(n).unwrap());
            pin.fold("", (new.len(), new.is_empty()));
        }
    }
    pin.finish();
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
fn a_line_of_many_small_pieces_is_refused_before_it_grows() {
    // The worst case of a line of 8 KiB: 4,096 single-letter atoms would make
    // as many Tokens (about 200 KiB), the capability list as many Strings.
    let atoms = "a ".repeat(new_imap::MAX_LINE / 2);
    assert_eq!(atoms.len(), new_imap::MAX_LINE);
    assert_eq!(new_imap::tokens(atoms.as_bytes()), Err(Fault::TooLong));
    let fetch = format!("* 1 FETCH ({}", &atoms[..atoms.len() - 12]);
    assert_eq!(new_imap::response(fetch.as_bytes()), Err(Fault::TooLong));
    let list = format!("* LIST ({}) \"/\" x", &atoms[..atoms.len() - 20]);
    assert_eq!(new_imap::response(list.as_bytes()), Err(Fault::TooLong));
    let capability = format!("* CAPABILITY {}", &atoms[..atoms.len() - 16]);
    assert_eq!(
        new_imap::response(capability.as_bytes()),
        Err(Fault::TooLong)
    );
    let code = format!("* OK [X {}] hi", &atoms[..atoms.len() - 16]);
    assert_eq!(new_imap::response(code.as_bytes()), Err(Fault::TooLong));
    let search = format!("* SEARCH {}", "1 ".repeat(100));
    assert!(new_imap::response(search.as_bytes()).is_ok());
    // The bounds themselves are taken: 256 tokens, 64 words.
    let at_most = "a ".repeat(new_imap::MAX_TOKENS);
    assert_eq!(
        new_imap::tokens(at_most.as_bytes()).unwrap().len(),
        new_imap::MAX_TOKENS
    );
    let one_more = "a ".repeat(new_imap::MAX_TOKENS + 1);
    assert_eq!(new_imap::tokens(one_more.as_bytes()), Err(Fault::TooLong));
    let words = |n: usize| format!("* CAPABILITY {}", "w ".repeat(n));
    assert!(new_imap::response(words(new_imap::MAX_WORDS).as_bytes()).is_ok());
    assert_eq!(
        new_imap::response(words(new_imap::MAX_WORDS + 1).as_bytes()),
        Err(Fault::TooLong)
    );
    let flags = |n: usize| format!("* 1 FETCH (FLAGS ({}))", "\\F ".repeat(n));
    let Some(new_imap::Response::Untagged(new_imap::Untagged::Fetch { items, .. })) =
        new_imap::response(flags(new_imap::MAX_WORDS).as_bytes()).unwrap()
    else {
        panic!("not a fetch");
    };
    assert_eq!(
        new_imap::fetch_flags(&items).unwrap().len(),
        new_imap::MAX_WORDS
    );
    let Some(new_imap::Response::Untagged(new_imap::Untagged::Fetch { items, .. })) =
        new_imap::response(flags(new_imap::MAX_WORDS + 1).as_bytes()).unwrap()
    else {
        panic!("not a fetch");
    };
    assert_eq!(new_imap::fetch_flags(&items), Err(Fault::TooLong));
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
}

#[test]
fn a_password_is_quoted_into_a_buffer_that_hides_it() {
    let mut pin = pin::Pin::new("a_password_is_quoted_into_a_buffer_that_hides_it");
    let secret = new_imap::quoted_secret("pa\"ss\\word").unwrap();
    assert_eq!(secret.as_str(), "\"pa\\\"ss\\\\word\"");
    assert!(!format!("{secret:?}").contains("pa"));
    let line = new_smtp::auth_plain("ada", "hunter2").unwrap();
    pin.check("", line.as_str());
    assert!(!format!("{line:?}").contains("hunter2"));
    pin.finish();
}

fn new_reply(reply: &Option<new_smtp::Reply>) -> String {
    format!("{reply:?}")
}

#[test]
fn replies_are_put_together_the_same() {
    let mut pin = pin::Pin::new("replies_are_put_together_the_same");
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
        let mut new = new_smtp::ReplyParser::default();
        for line in *script {
            let b = new.line(line.as_bytes()).unwrap();
            pin.check(format!("{line:?}"), new_reply(&b));
            if let Some(b) = &b {
                pin.check("", format!("{:?}", new_smtp::Extensions::of(b)));
                pin.check("", (b.is_ok(), b.is_temporary(), b.text()));
            }
        }
    }
    // Too long a line, too long a reply, too many lines.
    let long = "x".repeat(1001);
    let mut new = new_smtp::ReplyParser::default();
    pin.check("", new_reply(&new.line(long.as_bytes()).unwrap()));
    let mut new = new_smtp::ReplyParser::default();
    for _ in 0..200 {
        let line = "250-0123456789012345678901234567890123456789";
        pin.check("", new_reply(&new.line(line.as_bytes()).unwrap()));
    }
    pin.finish();
}

#[test]
fn random_replies_are_put_together_the_same() {
    let mut pin = pin::Pin::new("random_replies_are_put_together_the_same");
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
        let mut new = new_smtp::ReplyParser::default();
        for _ in 0..rng.below(12) {
            let mut line = String::new();
            for _ in 0..rng.below(6) {
                line.push_str(PIECES[rng.below(PIECES.len())]);
            }
            let b = new.line(line.as_bytes()).unwrap();
            pin.fold(format!("{line:?}"), new_reply(&b));
            if let Some(b) = &b {
                pin.fold("", format!("{:?}", new_smtp::Extensions::of(b)));
            }
        }
    }
    pin.finish();
}

#[test]
fn smtp_commands_and_data_are_the_same() {
    let mut pin = pin::Pin::new("smtp_commands_and_data_are_the_same");
    for address in ["a@b.c", "a@b.c>\r\nDATA", "<x y@z>", "caf\u{e9}@x.org", ""] {
        pin.fold("", new_smtp::mail_from(address).unwrap());
        pin.fold("", new_smtp::rcpt_to(address).unwrap());
        pin.fold("", new_smtp::valid_address(address));
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
        pin.fold(format!("{message:?}"), new_smtp::data(message).unwrap());
    }
    let mut rng = Rng(5);
    let alphabet: Vec<char> = ".\r\nab \u{e9}".chars().collect();
    for _ in 0..2000 {
        let message: String = (0..rng.below(20))
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        pin.fold(format!("{message:?}"), new_smtp::data(&message).unwrap());
    }
    pin.finish();
}

/// What hostile bytes may come to: an answer or a line past a bound, never a
/// refused allocation (nothing is refused here) and never a panic.
fn assert_ok_or_too_long<T>(result: Result<T, Fault>) {
    assert!(matches!(result, Ok(_) | Err(Fault::TooLong)));
}

#[test]
fn hostile_bytes_are_read_without_a_panic() {
    let mut rng = Rng(99);
    for len in [0usize, 1, 2, 5, 30, 300, 3000, 8000] {
        for _ in 0..40 {
            let line: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            assert_ok_or_too_long(new_imap::response(&line));
            assert_ok_or_too_long(new_imap::tokens(&line));
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
        assert_ok_or_too_long(new_imap::response(text.as_bytes()));
        assert_ok_or_too_long(new_imap::tokens(text.as_bytes()));
    }
}
