//! Hostile and unusual JSON for both readers: escapes (surrogate pairs,
//! lone surrogates, malformed `\u`), numbers at their limits, deep
//! nesting, truncated and invalid input. Properties: no input panics;
//! written strings read back; generated documents give the members,
//! raw values and array items they were written with; the streaming
//! parser reports every value with its path, the same in any chunking.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_json::stream::{Event, MAX_DEPTH, MAX_STRING, Parser, read_file, try_read_file};
use paperpaw_json::{Value, number, object, raw_member, string, string_array, text, unquote};
use paperpaw_platform::{DirEntry, Storage, StorageError};
use proptest::prelude::*;

/// An event that outlives the parser's buffers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ev {
    Open(Vec<String>),
    Close(Vec<String>),
    Value(Vec<String>, String),
}

fn events_of(data: &[u8], chunk: usize) -> Vec<Ev> {
    let mut parser = Parser::default();
    let mut out = Vec::new();
    let mut emit = |event: Event<'_>| {
        out.push(match event {
            Event::Open(path) => Ev::Open(path.to_vec()),
            Event::Close(path) => Ev::Close(path.to_vec()),
            Event::Value(path, text) => Ev::Value(path.to_vec(), text.to_string()),
        });
    };
    for piece in data.chunks(chunk.max(1)) {
        parser.feed(piece, &mut emit);
    }
    parser.finish(&mut emit);
    out
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

/// A JSON value as the tests generate it.
#[derive(Debug, Clone)]
enum J {
    Obj(Vec<(String, J)>),
    Arr(Vec<J>),
    Str(String),
    /// A number or literal, as written.
    Plain(String),
}

/// Writes `s` as a string literal with every character outside ASCII as
/// a `\u` escape (pairs above the basic plane).
fn ascii_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04X}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn write(j: &J, out: &mut String, pretty: bool, ascii: bool) {
    let literal = |s: &str| if ascii { ascii_string(s) } else { string(s) };
    let space = if pretty { "\n  " } else { "" };
    match j {
        J::Obj(members) => {
            out.push('{');
            for (i, (key, value)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(space);
                out.push_str(&literal(key));
                out.push_str(if pretty { " : " } else { ":" });
                write(value, out, pretty, ascii);
            }
            out.push_str(space);
            out.push('}');
        }
        J::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(space);
                write(item, out, pretty, ascii);
            }
            out.push_str(space);
            out.push(']');
        }
        J::Str(s) => out.push_str(&literal(s)),
        J::Plain(p) => out.push_str(p),
    }
}

fn json(j: &J, pretty: bool, ascii: bool) -> String {
    let mut out = String::new();
    write(j, &mut out, pretty, ascii);
    out
}

/// The events a streaming parser must report for `j` at `at`.
fn expected(j: &J, at: &mut Vec<String>, out: &mut Vec<Ev>) {
    match j {
        J::Obj(members) => {
            out.push(Ev::Open(at.clone()));
            for (key, value) in members {
                at.push(key.clone());
                expected(value, at, out);
                at.pop();
            }
            out.push(Ev::Close(at.clone()));
        }
        J::Arr(items) => {
            out.push(Ev::Open(at.clone()));
            for item in items {
                at.push("[]".to_string());
                expected(item, at, out);
                at.pop();
            }
            out.push(Ev::Close(at.clone()));
        }
        J::Str(s) => out.push(Ev::Value(at.clone(), s.clone())),
        J::Plain(p) => out.push(Ev::Value(at.clone(), p.clone())),
    }
}

fn plain() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("true".to_string()),
        Just("false".to_string()),
        Just("null".to_string()),
        any::<i64>().prop_map(|n| n.to_string()),
        "-?(0|[1-9][0-9]{0,15})(\\.[0-9]{1,8})?([eE][+-]?[0-9]{1,3})?",
    ]
}

fn value() -> impl Strategy<Value = J> {
    let leaf = prop_oneof![
        "\\PC{0,30}".prop_map(J::Str),
        // Control characters, quotes and backslashes too.
        "[\\x00-\\x1f\"\\\\a-z]{0,10}".prop_map(J::Str),
        plain().prop_map(J::Plain),
    ];
    leaf.prop_recursive(5, 64, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..6).prop_map(J::Arr),
            proptest::collection::vec(("\\PC{0,12}", inner), 0..6).prop_map(J::Obj),
        ]
    })
}

/// Objects or arrays at the top, as the streaming parser reads them.
fn document() -> impl Strategy<Value = J> {
    value().prop_map(|j| match j {
        J::Obj(_) | J::Arr(_) => j,
        leaf => J::Arr(vec![leaf]),
    })
}

/// One file in memory, as the storage of `read_file`.
struct OneFile(Vec<u8>);

impl Storage for OneFile {
    fn read_at(&self, _: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let start = (offset as usize).min(self.0.len());
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(n)
    }
    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Ok(self.0.len() as u64)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

// The flat reader (`object`, `raw_member`, `string_array`, `unquote`).

#[test]
fn surrogate_pairs_and_lone_low_surrogates() {
    assert_eq!(
        unquote(r#""\ud83d\udcd6 \uD83D\uDCD6""#).as_deref(),
        Some("📖 📖")
    );
    assert_eq!(unquote(r#""\udc00x""#).as_deref(), Some("\u{fffd}x"));
    assert_eq!(unquote(r#""\u00e4\u20AC""#).as_deref(), Some("ä€"));
    // A high surrogate without its low half stands for U+FFFD.
    assert_eq!(unquote(r#""\ud83dx""#).as_deref(), Some("\u{fffd}x"));
}

#[test]
fn a_high_surrogate_before_another_escape_does_not_panic() {
    for literal in [
        r#""\ud800\u0041""#,
        r#""\ud800\ud800""#,
        r#""\udbff\u0000""#,
    ] {
        let read = unquote(literal);
        assert!(
            read.as_deref().is_none_or(|s| s.starts_with('\u{fffd}')),
            "{literal}: {read:?}"
        );
    }
}

#[test]
fn malformed_escapes_spoil_only_their_string() {
    for bad in [
        r#""\u12""#,
        r#""\uZZZZ""#,
        r#""\u""#,
        r#""abc\"#,
        r#""abc"#,
        "\"",
        "",
    ] {
        assert_eq!(unquote(bad), None, "{bad}");
    }
    // Unknown escapes stand for the character.
    assert_eq!(unquote(r#""\q\/\'""#).as_deref(), Some("q/'"));
    assert_eq!(unquote(r#""\b\f""#).as_deref(), Some("\u{8}\u{c}"));
}

#[test]
fn numbers_at_their_limits() {
    let o = object(
        r#"{"max": 9223372036854775807, "min": -9223372036854775808,
            "big": 1.7976931348623157e308, "tiny": 5e-324, "zero": -0,
            "exp": 1E+2, "frac": 0.1, "inexact": 9007199254740993}"#,
    );
    assert_eq!(number(&o, "max"), Some(9.223_372_036_854_776e18));
    assert_eq!(number(&o, "min"), Some(-9.223_372_036_854_776e18));
    assert_eq!(number(&o, "big"), Some(f64::MAX));
    assert_eq!(number(&o, "tiny"), Some(5e-324));
    assert!(number(&o, "zero").unwrap().is_sign_negative());
    assert_eq!(number(&o, "exp"), Some(100.0));
    assert_eq!(number(&o, "frac"), Some(0.1));
    assert_eq!(number(&o, "inexact"), Some(9_007_199_254_740_992.0));
    // Past the largest finite number, and below the smallest.
    let o = object(r#"{"over": 1e400, "under": -1e400, "gone": 1e-400}"#);
    assert_eq!(number(&o, "over"), Some(f64::INFINITY));
    assert_eq!(number(&o, "under"), Some(f64::NEG_INFINITY));
    assert_eq!(number(&o, "gone"), Some(0.0));
}

#[test]
fn malformed_numbers_spoil_the_whole_object() {
    for bad in [
        r#"{"a": --1}"#,
        r#"{"a": 1.2.3}"#,
        r#"{"a": 1e}"#,
        r#"{"a": -}"#,
        r#"{"a": +1}"#,
        r#"{"a": .5}"#,
    ] {
        assert!(object(bad).is_empty(), "{bad}");
    }
}

#[test]
fn deep_nesting_is_skipped_without_recursion() {
    let depth = 200_000;
    let nested = format!("{}{}", "[{\"k\":".repeat(depth), "1}]".repeat(depth));
    let doc = format!(r#"{{"deep": {nested}, "after": "x"}}"#);
    let o = object(&doc);
    assert_eq!(o.len(), 2);
    assert_eq!(o[0], ("deep".to_string(), Value::Other));
    assert_eq!(text(&o, "after"), Some("x"));
    assert_eq!(raw_member(&doc, "deep"), Some(nested.as_str()));
    assert_eq!(raw_member(&doc, "after"), Some("\"x\""));
    let array = format!("[\"a\", {nested}, \"b\"]");
    assert_eq!(string_array(&array), ["a", "b"]);
}

#[test]
fn brackets_and_quotes_inside_strings_do_not_end_values() {
    let doc = r#"{"a": {"s": "}]\"{["}, "b": ["]", "\\", "\"]"], "c": "\\\"}"}"#;
    assert_eq!(raw_member(doc, "a"), Some(r#"{"s": "}]\"{["}"#));
    assert_eq!(
        string_array(raw_member(doc, "b").unwrap()),
        ["]", "\\", "\"]"]
    );
    assert_eq!(text(&object(doc), "c"), Some("\\\"}"));
}

#[test]
fn not_objects_and_broken_objects_are_empty() {
    for bad in [
        "",
        "[]",
        "\"s\"",
        "{",
        "{\"a\"",
        "{\"a\":",
        "{\"a\":1",
        "{\"a\":1,",
        "{\"a\" 1}",
        "{a:1}",
        "{,}",
        "{\"a\":1}}",
        "{\"a\":1} trailing",
    ] {
        let o = object(bad);
        // Trailing text after a complete object is not read.
        if bad.starts_with("{\"a\":1}") {
            assert_eq!(number(&o, "a"), Some(1.0), "{bad}");
        } else {
            assert!(o.is_empty(), "{bad}");
        }
    }
    assert_eq!(raw_member("{\"a\" 1}", "a"), None);
    assert_eq!(raw_member("{\"a\":", "a"), None);
    assert!(string_array("[\"a\"").first().is_some_and(|a| a == "a"));
}

#[test]
fn every_truncation_of_an_object_reads_nothing() {
    let doc = r#"{"title": "Größe \"x\" \ud83d\udcd6", "n": -12.5e3, "nested": {"a": [1, "}"]}, "ok": true}"#;
    assert_eq!(object(doc).len(), 4);
    for (cut, _) in doc.char_indices() {
        assert!(object(&doc[..cut]).is_empty(), "cut at {cut}");
        let _ = raw_member(&doc[..cut], "nested");
        let _ = string_array(&doc[..cut]);
    }
}

// The streaming parser.

#[test]
fn values_are_reported_with_their_paths() {
    let doc = br#"{"a": {"b": [1, "two", {"c": null}], "d": true}, "e": "x"}"#;
    assert_eq!(
        events_of(doc, usize::MAX),
        [
            Ev::Open(path(&[])),
            Ev::Open(path(&["a"])),
            Ev::Open(path(&["a", "b"])),
            Ev::Value(path(&["a", "b", "[]"]), "1".into()),
            Ev::Value(path(&["a", "b", "[]"]), "two".into()),
            Ev::Open(path(&["a", "b", "[]"])),
            Ev::Value(path(&["a", "b", "[]", "c"]), "null".into()),
            Ev::Close(path(&["a", "b", "[]"])),
            Ev::Close(path(&["a", "b"])),
            Ev::Value(path(&["a", "d"]), "true".into()),
            Ev::Close(path(&["a"])),
            Ev::Value(path(&["e"]), "x".into()),
            Ev::Close(path(&[])),
        ]
    );
}

#[test]
fn stream_escapes_and_surrogate_pairs() {
    let events = events_of(
        br#"["\ud83d\udcd6", "a\nb\t\"\\\/", "\u00e4", "\udc00", "\uZZZZ"]"#,
        3,
    );
    let values: Vec<String> = events
        .into_iter()
        .filter_map(|e| match e {
            Ev::Value(_, v) => Some(v),
            _ => None,
        })
        .collect();
    // A broken `\u` escape stands for U+FFFD; the bytes after it are text.
    assert_eq!(
        values,
        ["📖", "a\nb\t\"\\/", "ä", "\u{fffd}", "\u{fffd}ZZZZ"]
    );
}

#[test]
fn stream_strings_are_cut_at_the_limit() {
    let long = "x".repeat(3 * MAX_STRING);
    let escaped = "\\u00e4".repeat(MAX_STRING);
    let doc = format!("[\"{long}\", \"{escaped}\", \"after\"]");
    let values: Vec<String> = events_of(doc.as_bytes(), 1000)
        .into_iter()
        .filter_map(|e| match e {
            Ev::Value(_, v) => Some(v),
            _ => None,
        })
        .collect();
    assert_eq!(values.len(), 3);
    assert_eq!(values[0], "x".repeat(MAX_STRING));
    assert!(values[1].len() <= MAX_STRING + 4 && values[1].chars().all(|c| c == 'ä'));
    assert_eq!(values[2], "after");
}

#[test]
fn stream_invalid_utf8_is_replaced() {
    let events = events_of(b"{\"k\xff\": \"v\xfe\xfd\"}", 1);
    assert_eq!(
        events[1],
        Ev::Value(path(&["k\u{fffd}"]), "v\u{fffd}\u{fffd}".into())
    );
}

#[test]
fn stream_stray_closers_and_garbage_do_not_panic() {
    for doc in [
        &b"]]]}}}"[..],
        b"{\"a\":}}]]",
        b"<html><body>Not Found</body></html>",
        b"\"unterminated",
        b"{\"a\":\"\\",
        b"\xef\xbb\xbf{\"a\":1}",
        b"{\"a\" \"b\" \"c\": 1}",
        b",,,:::",
    ] {
        for chunk in [1, 2, 7, 100] {
            let _ = events_of(doc, chunk);
        }
    }
}

#[test]
fn stream_nesting_opens_and_closes_every_level_up_to_the_limit() {
    let depth = MAX_DEPTH;
    let doc = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    let events = events_of(doc.as_bytes(), 7);
    let opens = events.iter().filter(|e| matches!(e, Ev::Open(_))).count();
    let closes = events.iter().filter(|e| matches!(e, Ev::Close(_))).count();
    assert_eq!((opens, closes), (depth, depth));
    assert!(events.contains(&Ev::Value(vec!["[]".to_string(); depth], "1".into())));
    assert_eq!(events.last(), Some(&Ev::Close(Vec::new())));
}

#[test]
fn stream_nesting_past_the_limit_is_skipped_whole() {
    let depth = 2000;
    let doc = format!(
        "{{\"deep\": {}1{}, \"after\": 2}}",
        "[".repeat(depth),
        "]".repeat(depth)
    );
    let events = events_of(doc.as_bytes(), 512);
    let opens = events.iter().filter(|e| matches!(e, Ev::Open(_))).count();
    let closes = events.iter().filter(|e| matches!(e, Ev::Close(_))).count();
    // The object and the arrays up to the limit open and close; deeper
    // arrays and the value inside them give no events.
    assert_eq!((opens, closes), (MAX_DEPTH, MAX_DEPTH));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Ev::Value(_, v) if v == "1"))
    );
    assert!(events.contains(&Ev::Value(path(&["after"]), "2".into())));
    assert_eq!(events.last(), Some(&Ev::Close(Vec::new())));
}

/// Only the end of the input ends a plain value there: `finish` reports it.
#[test]
fn a_plain_value_at_the_end_of_the_input_is_reported() {
    assert_eq!(
        events_of(b"42", usize::MAX),
        [Ev::Value(Vec::new(), "42".into())]
    );
}

#[test]
fn a_lone_high_surrogate_stays_in_its_own_string() {
    let values: Vec<Ev> = events_of(br#"["a\ud800", "\udc00b"]"#, usize::MAX);
    assert_eq!(
        values[1..3],
        [
            Ev::Value(path(&["[]"]), "a\u{fffd}".into()),
            Ev::Value(path(&["[]"]), "\u{fffd}b".into()),
        ]
    );
}

#[test]
fn a_broken_unicode_escape_spoils_only_its_string() {
    let doc = "[\"\\uabcä\", \"next\", 7, \"last\"]".as_bytes();
    let values: Vec<String> = events_of(doc, usize::MAX)
        .into_iter()
        .filter_map(|e| match e {
            Ev::Value(_, v) => Some(v),
            _ => None,
        })
        .collect();
    assert_eq!(values[1..], ["next", "7", "last"]);
}

#[test]
fn plain_values_are_cut_at_the_limit_like_strings() {
    let doc = format!("[{}]", "9".repeat(1 << 20));
    let values: Vec<String> = events_of(doc.as_bytes(), 4096)
        .into_iter()
        .filter_map(|e| match e {
            Ev::Value(_, v) => Some(v),
            _ => None,
        })
        .collect();
    assert_eq!(values.len(), 1);
    assert!(values[0].len() <= MAX_STRING, "{} bytes", values[0].len());
}

#[test]
fn read_file_reads_a_file_larger_than_its_chunks() {
    let items: Vec<String> = (0..2000)
        .map(|i| format!("{{\"id\": {i}, \"t\": \"ü{i}\"}}"))
        .collect();
    let doc = format!("{{\"items\": [{}]}}", items.join(", "));
    let storage = OneFile(doc.clone().into_bytes());
    let mut events = Vec::new();
    read_file(&storage, "f.json", &mut |event| {
        if let Event::Value(path, text) = event {
            events.push(Ev::Value(path.to_vec(), text.to_string()));
        }
    });
    let whole: Vec<Ev> = events_of(doc.as_bytes(), usize::MAX)
        .into_iter()
        .filter(|e| matches!(e, Ev::Value(..)))
        .collect();
    assert_eq!(events.len(), 4000);
    assert_eq!(events, whole);
    assert_eq!(
        events[3999],
        Ev::Value(path(&["items", "[]", "t"]), "ü1999".into())
    );
}

#[test]
fn the_fallible_file_reader_reports_the_same() {
    let doc = r#"{"a": [1, "zwei", {"b": null}], "c": "\u00fc"}"#;
    let storage = OneFile(doc.as_bytes().to_vec());
    let (mut infallible, mut fallible) = (Vec::new(), Vec::new());
    read_file(&storage, "f.json", &mut |event| {
        infallible.push(format!("{event:?}"))
    });
    try_read_file(&storage, "f.json", &mut |event| {
        fallible.push(format!("{event:?}"))
    })
    .unwrap();
    assert_eq!(fallible, infallible);
    assert_eq!(fallible.len(), 10);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(160)))]

    #[test]
    fn written_strings_read_back(s in "\\PC*|[\\x00-\\x7f]*") {
        prop_assert_eq!(unquote(&string(&s)), Some(s.clone()));
        prop_assert_eq!(unquote(&ascii_string(&s)), Some(s.clone()));
        let doc = format!("{{\"k\": {}}}", string(&s));
        let members = object(&doc);
        prop_assert_eq!(text(&members, "k"), Some(s.as_str()));
        prop_assert_eq!(raw_member(&doc, "k").and_then(unquote), Some(s.clone()));
        let streamed = events_of(doc.as_bytes(), 7);
        prop_assert_eq!(&streamed[1], &Ev::Value(path(&["k"]), s.clone()));
    }

    #[test]
    fn flat_objects_give_their_members(
        members in proptest::collection::vec(
            ("\\PC{0,10}", prop_oneof![
                "\\PC{0,20}".prop_map(J::Str),
                plain().prop_map(J::Plain),
                value(),
            ]),
            0..10,
        ),
        pretty in any::<bool>(),
    ) {
        let doc = json(&J::Obj(members.clone()), pretty, false);
        let read = object(&doc);
        prop_assert_eq!(read.len(), members.len());
        for ((key, value), (read_key, read_value)) in members.iter().zip(&read) {
            prop_assert_eq!(key, read_key);
            match value {
                J::Str(s) => prop_assert_eq!(read_value, &Value::Str(s.clone())),
                J::Plain(p) if p.parse::<f64>().is_ok() => {
                    prop_assert_eq!(read_value, &Value::Num(p.parse().unwrap()));
                }
                _ => prop_assert_eq!(read_value, &Value::Other),
            }
        }
    }

    #[test]
    fn raw_members_are_the_values_as_written(
        values in proptest::collection::vec(value(), 1..8),
        pretty in any::<bool>(),
    ) {
        let members: Vec<(String, J)> = values
            .into_iter()
            .enumerate()
            .map(|(i, v)| (format!("key{i}"), v))
            .collect();
        let doc = json(&J::Obj(members.clone()), pretty, false);
        for (key, value) in &members {
            let written = json(value, pretty, false);
            prop_assert_eq!(raw_member(&doc, key), Some(written.as_str()));
        }
        prop_assert_eq!(raw_member(&doc, "missing"), None);
    }

    #[test]
    fn string_arrays_give_their_strings(items in proptest::collection::vec(value(), 0..10)) {
        let doc = json(&J::Arr(items.clone()), false, false);
        let strings: Vec<String> = items
            .iter()
            .filter_map(|j| match j {
                J::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        prop_assert_eq!(string_array(&doc), strings);
    }

    #[test]
    fn the_stream_reports_every_value_with_its_path(
        doc in document(),
        pretty in any::<bool>(),
        ascii in any::<bool>(),
        chunk in 1usize..64,
    ) {
        let text = json(&doc, pretty, ascii);
        let mut want = Vec::new();
        expected(&doc, &mut Vec::new(), &mut want);
        prop_assert_eq!(&events_of(text.as_bytes(), usize::MAX), &want);
        prop_assert_eq!(&events_of(text.as_bytes(), chunk), &want);
    }

    #[test]
    fn arbitrary_text_never_panics(s in "\\PC{0,300}|[\\[\\]{}\",:\\\\u0-9a-f ]{0,300}") {
        let _ = object(&s);
        let _ = raw_member(&s, "a");
        let _ = string_array(&s);
        let _ = unquote(&s);
    }

    #[test]
    fn arbitrary_bytes_never_panic_the_stream(
        data in proptest::collection::vec(any::<u8>(), 0..2000),
        chunk in 1usize..100,
    ) {
        let whole = events_of(&data, usize::MAX);
        prop_assert_eq!(events_of(&data, chunk), whole);
    }
}
