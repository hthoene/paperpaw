//! The configuration dialect at its edges: comments, duplicate sections
//! and keys, keys without values, whitespace, CRLF, very long lines,
//! escapes, identifiers, typed values; `set` and `remove` keeping
//! everything else. The reference file `testdata/dialect.v1.ini` holds
//! every part of the format. Properties: any text parses without
//! panicking, and any sequence of `set` and `remove` reads back as the
//! same settings a map holds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeMap;

use paperpaw_config::{Entry, ErrorKind, ParseError, parse, quote, remove, set};
use proptest::prelude::*;

/// `(section, key, value)` of every entry; panics on errors.
fn entries(source: &str) -> Vec<(String, String, String)> {
    parse(source)
        .map(|entry| {
            let entry = entry.unwrap_or_else(|e| panic!("{e} in {source:?}"));
            (
                entry.section.to_string(),
                entry.key.to_string(),
                entry.value.as_str().into_owned(),
            )
        })
        .collect()
}

/// The settings as a reader sees them: the last entry of a key wins.
fn settings(source: &str) -> BTreeMap<(String, String), String> {
    entries(source)
        .into_iter()
        .map(|(section, key, value)| ((section, key), value))
        .collect()
}

fn first_error(source: &str) -> ParseError {
    parse(source).find_map(Result::err).expect(source)
}

fn e(section: &str, key: &str, value: &str) -> (String, String, String) {
    (section.into(), key.into(), value.into())
}

#[test]
fn the_reference_file_reads_as_written() {
    let source = include_str!("../testdata/dialect.v1.ini");
    assert_eq!(
        entries(source),
        [
            e("", "top-level", "before any section"),
            e("", "dotted.key-with_dash@2", "1"),
            e("display", "full-refresh-interval", "6"),
            e("display", "night-mode", "off"),
            e("display", "tabbed", "value between tabs"),
            e("display", "empty", ""),
            e("messages", "greeting", "Hello, {name}!"),
            e(
                "messages",
                "literal",
                "# not a comment; \\n stays as written"
            ),
            e("messages", "equals", "a = b = c"),
            e("messages", "padded", "  quoted values keep spaces  "),
            e("messages", "escapes", "tab\tnewline\nquote\"backslash\\"),
            e("messages", "unicode", "\u{1f4d6} \u{e4}A"),
            e("messages", "quoted-empty", ""),
            e("messages", "non-ascii", "Größe, Maß, Ελληνικά, 日本語"),
            e("display", "night-mode", "on"),
            e("spaced.section", "key", "value"),
        ]
    );
    let read = settings(source);
    assert_eq!(read[&("display".into(), "night-mode".into())], "on");
    // Line numbers count every line, comments and blank ones too.
    let greeting = parse(source)
        .filter_map(Result::ok)
        .find(|entry| entry.key == "greeting")
        .unwrap();
    assert_eq!(greeting.line, 15);
}

#[test]
fn the_reference_file_survives_set_and_remove() {
    let source = include_str!("../testdata/dialect.v1.ini");
    let before = settings(source);
    let changed = set(source, "messages", "greeting", "Hallo, {name}!");
    let changed = set(&changed, "display", "night-mode", "auto");
    let changed = set(&changed, "new", "key", " padded ");
    let changed = remove(&changed, "messages", "equals");
    let mut expected = before;
    expected.insert(
        ("messages".into(), "greeting".into()),
        "Hallo, {name}!".into(),
    );
    expected.insert(("display".into(), "night-mode".into()), "auto".into());
    expected.insert(("new".into(), "key".into()), " padded ".into());
    expected.remove(&("messages".into(), "equals".into()));
    assert_eq!(settings(&changed), expected);
    // Comments stay where they were.
    assert!(changed.starts_with("# Every part of the configuration dialect"));
    assert!(changed.contains("   # an indented comment\n"));
}

#[test]
fn comments_only_at_the_start_of_a_line() {
    assert_eq!(
        entries("# a\n; b\n  \t# c\n\t; d\nk = v # e ; f\n"),
        [e("", "k", "v # e ; f")]
    );
    assert_eq!(
        first_error("[section] # comment").kind,
        ErrorKind::InvalidSection
    );
    assert_eq!(
        first_error("k = \"v\" # comment").kind,
        ErrorKind::InvalidLine
    );
}

#[test]
fn duplicate_sections_and_keys_are_all_reported_in_order() {
    let source = "[a]\nk = 1\n[b]\nk = 2\n[a]\nk = 3\nk = 4\n";
    assert_eq!(
        entries(source),
        [
            e("a", "k", "1"),
            e("b", "k", "2"),
            e("a", "k", "3"),
            e("a", "k", "4")
        ]
    );
    assert_eq!(settings(source)[&("a".into(), "k".into())], "4");
}

#[test]
fn keys_without_values_and_values_without_keys() {
    assert_eq!(
        entries("k =\nl=\nm = \t \n"),
        [e("", "k", ""), e("", "l", ""), e("", "m", "")]
    );
    assert_eq!(first_error("lonely").kind, ErrorKind::InvalidLine);
    assert_eq!(first_error("= value").kind, ErrorKind::InvalidKey);
    assert_eq!(first_error("  = value").kind, ErrorKind::InvalidKey);
}

#[test]
fn identifiers_are_ascii_letters_digits_and_four_marks() {
    assert_eq!(
        entries("[A.b-c_d@1]\nZ9.x-y_z@2 = v\n"),
        [e("A.b-c_d@1", "Z9.x-y_z@2", "v")]
    );
    for bad in [
        "a b = 1",
        "ä = 1",
        "a:b = 1",
        "a/b = 1",
        "a\\b = 1",
        "\"a\" = 1",
        "a\u{a0}b = 1",
    ] {
        assert_eq!(first_error(bad).kind, ErrorKind::InvalidKey, "{bad}");
    }
    for bad in [
        "[]", "[ ]", "[a  b]", "[a b ]x", "[ä]", "[a]]", "[[a]", "[a", "a]",
    ] {
        let kind = first_error(bad).kind;
        assert!(
            matches!(kind, ErrorKind::InvalidSection | ErrorKind::InvalidLine),
            "{bad}: {kind:?}"
        );
    }
    assert_eq!(entries("[ spaced ]\nk = v\n"), [e("spaced", "k", "v")]);
    // A section name may be words, one space between each: a menu of
    // another plugin in a data plugin's menus.ini.
    assert_eq!(
        entries("[menu core.reader-menu]\nadd = a.b\n"),
        [e("menu core.reader-menu", "add", "a.b")]
    );
}

#[test]
fn whitespace_around_keys_and_values_is_trimmed() {
    assert_eq!(
        entries("\t key \t=\t value \t\n\u{a0}k2 = v2\u{3000}\n"),
        [e("", "key", "value"), e("", "k2", "v2")]
    );
    // Inside a value it stays.
    assert_eq!(entries("k = a \t b\n"), [e("", "k", "a \t b")]);
}

#[test]
fn crlf_and_missing_final_newline() {
    let source = "# c\r\n[s]\r\nk = v\r\nq = \"x\"\r\nlast = end";
    assert_eq!(
        entries(source),
        [e("s", "k", "v"), e("s", "q", "x"), e("s", "last", "end")]
    );
    let lines: Vec<u32> = parse(source).map(|e| e.unwrap().line).collect();
    assert_eq!(lines, [3, 4, 5]);
    assert_eq!(first_error("a = 1\r\nb = \"x\r\n").line, 2);
}

#[test]
fn very_long_lines_are_read_whole_and_borrowed() {
    let value = "x".repeat(1 << 20);
    let source = format!("[s]\nk = {value}\nafter = 1\n");
    let parsed: Vec<Entry<'_>> = parse(&source).map(Result::unwrap).collect();
    assert_eq!(parsed[0].value.raw(), value);
    assert!(matches!(
        parsed[0].value.as_str(),
        std::borrow::Cow::Borrowed(_)
    ));
    assert_eq!(parsed[1].line, 3);
    let quoted = format!("k = \"{}\\n\"\n", "y".repeat(1 << 20));
    assert_eq!(entries(&quoted)[0].2.len(), (1 << 20) + 1);
}

#[test]
fn escapes_at_their_edges() {
    assert_eq!(
        entries(r#"k = "\u{0}\u{10FFFF}\u{000041}ä\u{e4}ü""#)[0].2,
        "\u{0}\u{10ffff}Aääü"
    );
    for (bad, kind) in [
        (r#"k = "\u{110000}""#, ErrorKind::InvalidEscape),
        (r#"k = "\u{D800}""#, ErrorKind::InvalidEscape),
        (r#"k = "\u{DFFF}""#, ErrorKind::InvalidEscape),
        (r#"k = "\u{}""#, ErrorKind::InvalidEscape),
        (r#"k = "\u{1234567}""#, ErrorKind::InvalidEscape),
        (r#"k = "\u{41""#, ErrorKind::InvalidEscape),
        (r#"k = "\u41""#, ErrorKind::InvalidEscape),
        (r#"k = "\r""#, ErrorKind::InvalidEscape),
        (r#"k = "\'""#, ErrorKind::InvalidEscape),
        (r#"k = "abc\""#, ErrorKind::UnterminatedString),
        (r#"k = "abc\"#, ErrorKind::UnterminatedString),
        (r#"k = ""#, ErrorKind::UnterminatedString),
        (r#"k = "a" "b""#, ErrorKind::InvalidLine),
    ] {
        assert_eq!(first_error(bad).kind, kind, "{bad}");
    }
    // A quote inside an unquoted value is just a character.
    assert_eq!(entries(r#"k = say "hi""#)[0].2, r#"say "hi""#);
}

#[test]
fn errors_are_per_line_and_reading_goes_on() {
    let results: Vec<Result<Entry<'_>, ParseError>> =
        parse("a = 1\nbroken\n[s]\nb = \"open\nc = 3\n").collect();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0].as_ref().unwrap().key, "a");
    assert_eq!(
        results[1].unwrap_err(),
        ParseError {
            line: 2,
            kind: ErrorKind::InvalidLine
        }
    );
    assert_eq!(results[2].unwrap_err().kind, ErrorKind::UnterminatedString);
    let c = results[3].as_ref().unwrap();
    assert_eq!((c.section, c.key, c.line), ("s", "c", 5));
    assert_eq!(
        first_error("\n\n\nbad").to_string(),
        "line 4: expected `key = value`, `[section]` or a comment"
    );
}

#[test]
fn typed_values() {
    fn value(source: &str) -> paperpaw_config::Value<'_> {
        parse(source).next().unwrap().unwrap().value
    }
    for (text, expected) in [
        ("true", Some(true)),
        ("yes", Some(true)),
        ("on", Some(true)),
        ("1", Some(true)),
        ("false", Some(false)),
        ("no", Some(false)),
        ("off", Some(false)),
        ("0", Some(false)),
        ("True", None),
        ("ON", None),
        ("2", None),
        ("", None),
    ] {
        assert_eq!(value(&format!("k = {text}")).as_bool(), expected, "{text}");
    }
    assert_eq!(value("k = 9223372036854775807").as_i64(), Some(i64::MAX));
    assert_eq!(value("k = -9223372036854775808").as_i64(), Some(i64::MIN));
    assert_eq!(value("k = 9223372036854775808").as_i64(), None);
    assert_eq!(value("k = 4294967295").as_u32(), Some(u32::MAX));
    assert_eq!(value("k = 4294967296").as_u32(), None);
    assert_eq!(value("k = -1").as_u32(), None);
    assert_eq!(value("k = 1.5").as_i64(), None);
    assert_eq!(value("k = 12abc").as_i64(), None);
    // Quoted numbers are numbers too.
    assert_eq!(value("k = \"42\"").as_i64(), Some(42));
}

#[test]
fn set_ignores_commented_out_entries() {
    let source = "[s]\n# k = old\n; k = older\nother = 1\n";
    let changed = set(source, "s", "k", "new");
    assert_eq!(changed, "[s]\n# k = old\n; k = older\nother = 1\nk = new\n");
}

#[test]
fn set_into_a_section_that_is_there_twice_is_read_back() {
    let source = "[a]\nx = 1\n[b]\ny = 2\n[a]\nz = 3\n";
    let changed = set(source, "a", "new", "v");
    assert_eq!(changed, "[a]\nx = 1\n[b]\ny = 2\n[a]\nz = 3\nnew = v\n");
    let changed = set(source, "a", "x", "9");
    assert_eq!(settings(&changed)[&("a".into(), "x".into())], "9");
}

#[test]
fn set_into_an_empty_section_goes_after_its_header() {
    let source = "[a]\n[b]\nk = 1\n";
    assert_eq!(set(source, "a", "x", "1"), "[a]\nx = 1\n[b]\nk = 1\n");
}

#[test]
fn set_on_crlf_files_reads_back() {
    let source = "[s]\r\nk = 1\r\nother = 2\r\n";
    let changed = set(source, "s", "k", "3");
    assert_eq!(entries(&changed), [e("s", "k", "3"), e("s", "other", "2")]);
}

#[test]
fn written_values_drop_carriage_returns() {
    // There is no escape for a carriage return; quote leaves it out.
    assert_eq!(quote("a\r\nb"), "\"a\\nb\"");
    let changed = set("", "s", "k", "line one\r\nline two\r");
    assert_eq!(entries(&changed), [e("s", "k", "line one\nline two")]);
}

#[test]
fn values_that_need_quotes_get_them() {
    for (value, plain) in [
        ("simple", true),
        ("with # and ; and = inside", true),
        ("trailing\\", true),
        ("ends with quote\"", true),
        ("", false),
        (" leading", false),
        ("trailing ", false),
        ("\"starts with quote", false),
        ("two\nlines", false),
        ("\ttab", false),
        ("\u{a0}", false),
    ] {
        assert_eq!(quote(value) == value, plain, "{value:?}");
        let line = format!("k = {}", quote(value));
        assert_eq!(entries(&line), [e("", "k", value)], "{line}");
    }
}

#[test]
fn set_writes_top_level_keys_readably() {
    let changed = set("", "", "k", "v");
    assert_eq!(entries(&changed), [e("", "k", "v")]);
    let changed = set("[a]\nx = 1\n", "", "top", "v");
    assert_eq!(settings(&changed)[&(String::new(), "top".into())], "v");
}

/// A change made to a file in the properties.
#[derive(Debug, Clone)]
enum Op {
    Set(String, String, String),
    Remove(String, String),
}

fn op() -> impl Strategy<Value = Op> {
    let section = prop::sample::select(vec!["a", "b", "core", "x.y", "keymap.reader", "c@2"]);
    let key = prop::sample::select(vec!["k", "key", "night-mode", "a.b", "z_9", "w@1"]);
    let value = prop_oneof![
        "\\PC{0,30}",
        "[a-z \\t\\n\"\\\\#;=\\[\\]{}@]{0,20}",
        Just(String::new()),
        Just(" ".to_string()),
    ];
    prop_oneof![
        3 => (section.clone(), key.clone(), value)
            .prop_map(|(s, k, v)| Op::Set(s.into(), k.into(), v)),
        1 => (section, key).prop_map(|(s, k)| Op::Remove(s.into(), k.into())),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(200)))]

    #[test]
    fn any_text_parses_without_panicking(source in "\\PC{0,200}|[\\[\\]=\"\\\\u{}#; \\na-z0-9\\r\\t]{0,200}") {
        for entry in parse(&source).flatten() {
            let _ = entry.value.as_str();
            prop_assert_eq!(entry.value.try_as_str().ok(), Some(entry.value.as_str()));
            let _ = entry.value.as_bool();
            let _ = entry.value.as_i64();
        }
        let _ = set(&source, "s", "k", "v");
        let _ = remove(&source, "s", "k");
    }

    #[test]
    fn set_and_remove_read_back_like_a_map(
        start in prop::sample::select(vec![
            "",
            "# settings\n",
            "[a]\nk = 1\n\n# b follows\n[b]\nkey = \"x\"\n",
            "[core]\r\nnight-mode = on\r\n",
            "[a]\n[a]\nk = old\n",
        ]),
        ops in proptest::collection::vec(op(), 0..20),
    ) {
        let mut source = start.to_string();
        let mut model = settings(start);
        for op in ops {
            match op {
                Op::Set(section, key, value) => {
                    source = set(&source, &section, &key, &value);
                    model.insert((section, key), value);
                }
                Op::Remove(section, key) => {
                    source = remove(&source, &section, &key);
                    model.remove(&(section, key));
                }
            }
            prop_assert!(parse(&source).all(|e| e.is_ok()), "{source:?}");
            prop_assert_eq!(&settings(&source), &model, "{:?}", source);
        }
    }

    #[test]
    fn quoted_values_read_back(value in "[^\\r]{0,40}") {
        let line = format!("k = {}", quote(&value));
        let parsed: Vec<_> = parse(&line).collect();
        prop_assert_eq!(parsed.len(), 1);
        let entry = parsed[0].unwrap();
        prop_assert_eq!(entry.value.as_str(), value.as_str());
        let tried = entry.value.try_as_str();
        prop_assert_eq!(tried.as_deref().ok(), Some(value.as_str()));
    }
}
