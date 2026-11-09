//! Calibre's wire format on its own: length-prefixed messages framed and
//! read back in any chunking, hostile lengths and bodies refused without a
//! panic, members of hostile JSON, book paths that would leave the library
//! folder, and Calibre's answers to the broadcast.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_plugin_calibre::protocol::{
    Framing, Message, frame, member, name_of_answer, number, op, parse, port_of_answer, safe_path,
    strings, text,
};
use proptest::prelude::*;

/// Feeds `wire` as the plugin does: never more than the framing wants,
/// and at most `chunk` bytes at a time (a socket may bring less).
fn read_all(wire: &str, chunk: usize) -> Result<Vec<Message>, &'static str> {
    let mut framing = Framing::default();
    let mut messages = Vec::new();
    let mut rest = wire;
    while !rest.is_empty() {
        let mut n = (framing.wanted() as usize).min(chunk).min(rest.len());
        while !rest.is_char_boundary(n) {
            n += 1;
        }
        let (part, tail) = rest.split_at(n);
        rest = tail;
        if let Some(message) = framing.feed(part)? {
            messages.push(message);
        }
    }
    Ok(messages)
}

fn wire(messages: &[(u8, &str)]) -> String {
    let mut wire = Vec::new();
    for (op, args) in messages {
        wire.extend_from_slice(&frame(*op, args));
    }
    String::from_utf8(wire).unwrap()
}

#[test]
fn messages_back_to_back_are_read_in_any_chunking() {
    let sent = [
        (op::GET_INITIALIZATION_INFO, r#"{"passwordChallenge": ""}"#),
        (op::NOOP, "{}"),
        (op::SET_LIBRARY_INFO, r#"{"libraryName": "Bücher ✓"}"#),
    ];
    let wire = wire(&sent);
    for chunk in [1, 2, 3, 7, 64, usize::MAX] {
        let got = read_all(&wire, chunk).unwrap();
        assert_eq!(got.len(), 3, "chunks of {chunk}");
        for ((op, args), message) in sent.iter().zip(&got) {
            assert_eq!(message.op, *op);
            assert_eq!(message.args, *args);
        }
    }
}

#[test]
fn the_framing_never_asks_past_its_message() {
    let wire = wire(&[(op::NOOP, r#"{"count": 3}"#)]);
    let mut framing = Framing::default();
    let mut at = 0;
    loop {
        let wanted = framing.wanted() as usize;
        assert!(at + wanted <= wire.len(), "asks past the end at {at}");
        let got = framing.feed(&wire[at..at + wanted]).unwrap();
        at += wanted;
        if got.is_some() {
            break;
        }
    }
    assert_eq!(at, wire.len());
    // Ready for the next one: a digit at a time.
    assert_eq!(framing.wanted(), 1);
}

#[test]
fn a_truncated_message_waits_for_the_rest() {
    let wire = wire(&[(op::NOOP, r#"{"a": "long enough"}"#)]);
    let mut framing = Framing::default();
    let cut = wire.len() - 5;
    let mut at = 0;
    while at < cut {
        let n = (framing.wanted() as usize).min(cut - at);
        assert_eq!(framing.feed(&wire[at..at + n]), Ok(None));
        at += n;
    }
    assert_eq!(framing.wanted(), 5);
    assert!(framing.feed(&wire[cut..]).unwrap().is_some());
}

#[test]
fn hostile_lengths_are_refused() {
    let cases = [
        // More digits than any message has.
        "123456789[",
        // Larger than a megabyte.
        "2000000[",
        "99999999[",
        // No length, or zero.
        "[",
        "0[",
        "00000000[",
        // Not Calibre's at all.
        "GET / HTTP/1.1\r\n",
        "-5[",
        " 5[",
        "\u{feff}5[",
        "5{",
    ];
    for case in cases {
        let mut framing = Framing::default();
        let mut result = Ok(None);
        for c in case.chars() {
            let mut buf = [0; 4];
            result = framing.feed(c.encode_utf8(&mut buf));
            if result.is_err() {
                break;
            }
        }
        assert!(result.is_err(), "{case:?}");
    }
}

#[test]
fn bodies_that_are_no_message_are_refused() {
    for body in [
        "[a]",
        "[1]",
        "[300, {}]",
        "[-1, {}]",
        "{1, {}}",
        "[1, {}",
        "[x, {}]",
    ] {
        let wire = format!("{}{body}", body.len());
        assert!(read_all(&wire, usize::MAX).is_err(), "{body}");
    }
    // Unknown opcodes are messages all the same: the plugin decides.
    let got = read_all(&wire(&[(99, "{}")]), 1).unwrap();
    assert_eq!(got[0].op, 99);
    assert_eq!(parse("[ 255 ,{} ]").unwrap().op, 255);
}

#[test]
fn members_of_hostile_json_are_none_or_whole() {
    let deep = format!(
        r#"{{"a": {}{}, "length": 5}}"#,
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    assert_eq!(number(&deep, "length"), Some(5));
    // Unterminated: nothing found, nothing panics.
    assert_eq!(member(r#"{"lpath": "abc"#, "lpath"), None);
    assert_eq!(member(r#"{"a": [1, 2"#, "b"), None);
    assert_eq!(text("not json", "lpath"), None);
    assert_eq!(text("", "lpath"), None);
    // Numbers of every kind.
    assert_eq!(number(r#"{"length": 1e30}"#, "length"), Some(u64::MAX));
    assert_eq!(number(r#"{"length": -5}"#, "length"), Some(0));
    assert_eq!(number(r#"{"length": "12"}"#, "length"), None);
    assert_eq!(number(r#"{"length": null}"#, "length"), None);
    assert_eq!(number(r#"{"length": 12.9}"#, "length"), Some(12));
    // Escapes decoded, a lone surrogate not trusted.
    assert_eq!(
        text(r#"{"title": "Caf\u00e9 \"Z\" \\ /"}"#, "title").as_deref(),
        Some("Café \"Z\" \\ /")
    );
    let _ = text(r#"{"title": "\ud800"}"#, "title");
    // An array of other things than strings.
    let _ = strings(r#"{"lpaths": [1, null, {"a": "b"}, "x"]}"#, "lpaths");
    assert!(strings(r#"{"lpaths": "x"}"#, "lpaths").is_empty());
}

#[test]
fn book_paths_never_leave_the_library_folder() {
    let refused = [
        "..",
        "../x.epub",
        "a/../../x.epub",
        "a/b/../../../x.epub",
        "..\\x.epub",
        "a\\..\\..\\x.epub",
        "C:\\Windows\\x.epub",
        "C:x.epub",
        "a/b:c.epub",
        "",
        "/",
        "./.",
        "\\\\server\\share\\..\\x",
        // Hidden names: PaperPaw's own data, and what plugins keep.
        ".paperpaw/secrets.txt",
        "a/.hidden.epub",
        "..a/b.epub",
    ];
    for path in refused {
        assert_eq!(safe_path(path), None, "{path}");
    }
    let kept = [
        ("/etc/x.epub", "etc/x.epub"),
        ("//a///b.epub", "a/b.epub"),
        ("a\\b\\c.epub", "a/b/c.epub"),
        ("./a/./b.epub", "a/b.epub"),
        ("a..b/c...epub", "a..b/c...epub"),
        ("Zoë/Ünïcödé ✓.epub", "Zoë/Ünïcödé ✓.epub"),
    ];
    for (path, safe) in kept {
        assert_eq!(safe_path(path).as_deref(), Some(safe), "{path}");
    }
}

#[test]
fn broadcast_answers_of_others_are_ignored() {
    assert_eq!(port_of_answer(""), None);
    assert_eq!(port_of_answer("calibre wireless device client"), None);
    assert_eq!(
        port_of_answer("calibre wireless device client (on pc);8080,notaport"),
        None
    );
    assert_eq!(
        port_of_answer("calibre wireless device client (on pc);8080,99999"),
        None
    );
    assert_eq!(port_of_answer("someone else (on pc);8080,9090"), None);
    assert_eq!(
        port_of_answer("calibre wireless device client (on a;b);9090"),
        Some(9090)
    );
    assert_eq!(name_of_answer("calibre wireless device client (on "), None);
    assert_eq!(name_of_answer("(on )"), Some(""));
    assert_eq!(
        name_of_answer("calibre wireless device client (on Zoës PC);1,2"),
        Some("Zoës PC")
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// What is framed is read back the same, however the bytes come.
    #[test]
    fn frames_round_trip(
        messages in proptest::collection::vec((any::<u8>(), r"\PC{0,60}"), 1..5),
        chunk in 1usize..40,
    ) {
        let args: Vec<(u8, String)> = messages
            .iter()
            .map(|(op, s)| (*op, format!("{{\"k\": {}}}", paperpaw_sdk::json::string(s))))
            .collect();
        let borrowed: Vec<(u8, &str)> = args.iter().map(|(o, a)| (*o, a.as_str())).collect();
        let got = read_all(&wire(&borrowed), chunk).unwrap();
        prop_assert_eq!(got.len(), args.len());
        for (((op, a), message), (_, original)) in args.iter().zip(&got).zip(&messages) {
            prop_assert_eq!(message.op, *op);
            prop_assert_eq!(&message.args, a);
            let decoded = text(&message.args, "k");
            prop_assert_eq!(decoded.as_deref(), Some(original.as_str()));
        }
    }

    /// Any text fed in any chunks ends in messages or an error, never a
    /// panic, and the framing never asks for nothing.
    #[test]
    fn arbitrary_text_never_panics(text in r"[0-9\[\],{}a-z\x00-\x7f]{0,200}", chunk in 1usize..16) {
        let mut framing = Framing::default();
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let wanted = framing.wanted() as usize;
            prop_assert!(wanted >= 1);
            let n = wanted.min(chunk).min(rest.len());
            let (part, tail) = rest.split_at(n);
            rest = tail;
            if framing.feed(part).is_err() {
                break;
            }
        }
    }

    /// A path kept is relative, plain and has no step out.
    #[test]
    fn safe_paths_are_plain(path in r"[a-zA-Z0-9./\\:_ -]{0,40}") {
        if let Some(safe) = safe_path(&path) {
            prop_assert!(!safe.is_empty());
            prop_assert!(!safe.starts_with('/'));
            prop_assert!(!safe.contains('\\') && !safe.contains(':'));
            for part in safe.split('/') {
                prop_assert!(!part.is_empty() && part != "." && part != "..", "{}", safe);
            }
        }
    }
}
