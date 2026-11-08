//! The native files model reads and writes what the interface-6 code did:
//! the same Lua values (as the same text back), the same names, the same
//! file of what is known.
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;

use crate::native_model as native;
use alloc::{format, string::String, vec, vec::Vec};
use paperpaw_sdk::annotations::Kind;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

const SIDECAR: &str = r#"-- we can read Lua syntax here!
return {
    ["annotations"] = {
        [1] = {
            ["chapter"] = "Chapter I",
            ["note"] = "Two\
lines",
            ["pos0"] = "/body/DocFragment[3]/body/p[4]/text().0",
            ["text"] = "Down the \"Rabbit\" Hole",
        },
        [2] = { ["datetime"] = "2026-10-02 08:00:00" },
    },
    ["percent_finished"] = 0.25,
    ["summary"] = {
        status = 'reading', -- a comment
        modified = [[2026-10-02]],
        long = [==[a ]] b]==],
        bytes = "\x41\065\z
                 B",
    },
    ["stats"] = { 1, 2, -3.5e-2, true, false, nil, 0x1F },
}
"#;

/// Both parsers' values, written back as text; `None` where either refused.
fn both(text: &str) -> (Option<String>, Option<String>) {
    let old = crate::lua::parse(text).map(|v| crate::lua::write(&v));
    let new = native::parse(text)
        .unwrap()
        .map(|v| native::write(&v).unwrap());
    (old, new)
}

#[test]
fn files_are_read_and_written_as_before() {
    for text in [
        SIDECAR,
        "",
        "return {",
        "return {[1] 2}",
        "return \"open",
        "return nonsense",
        "return nil",
        "return 12",
        "-- only a comment",
        "--[[ long\ncomment ]] return { a = 1 }",
        "return { [\"k\"] = [[x]], }",
        "return { 1, 2, [5] = 3 }",
        "return \"a\\xZZ\"",
        "return \"\\300\"",
        "return \"line\nbreak\"",
        "\u{feff}return {}",
    ] {
        let (old, new) = both(text);
        assert_eq!(new, old, "{text:?}");
    }
    let deep = format!("return {}{}", "{".repeat(100), "}".repeat(100));
    let (old, new) = both(&deep);
    assert_eq!((old, new), (None, None));
    let ok = format!("return {}{}", "{".repeat(60), "}".repeat(60));
    let (old, new) = both(&ok);
    assert!(old.is_some());
    assert_eq!(new, old);
}

#[test]
fn tables_are_used_as_before() {
    let Some(crate::lua::Value::Table(old)) = crate::lua::parse(SIDECAR) else {
        panic!("old")
    };
    let Some(native::Value::Table(new)) = native::parse(SIDECAR).unwrap() else {
        panic!("new")
    };
    let (Some(crate::lua::Value::Table(old_ann)), Some(native::Value::Table(new_ann))) =
        (old.get("annotations"), new.get("annotations"))
    else {
        panic!()
    };
    assert_eq!(old_ann.list().count(), new_ann.list().unwrap().len());
    let (Some(crate::lua::Value::Table(a)), Some(native::Value::Table(b))) = (
        old_ann.list().next(),
        new_ann.list().unwrap().first().copied(),
    ) else {
        panic!()
    };
    for key in ["note", "text", "chapter", "nothing"] {
        assert_eq!(a.str(key), b.str(key), "{key}");
    }
    // Set, remove and build lists the same.
    let mut old = crate::lua::Table::default();
    let mut new = native::Table::default();
    old.set("a", crate::lua::string("x"));
    new.set("a", native::string("x").unwrap()).unwrap();
    old.set("b", crate::lua::Value::Bool(true));
    new.set("b", native::Value::Bool(true)).unwrap();
    old.set("a", crate::lua::Value::Nil);
    new.set("a", native::Value::Nil).unwrap();
    let old_text = crate::lua::write(&crate::lua::Value::Table(old));
    let new_text = native::write(&native::Value::Table(new)).unwrap();
    assert_eq!(new_text, old_text);
    let old = crate::lua::Table::from_list([crate::lua::string("p"), crate::lua::string("q")]);
    let new =
        native::Table::from_list([native::string("p").unwrap(), native::string("q").unwrap()])
            .unwrap();
    assert_eq!(
        native::write(&native::Value::Table(new)).unwrap(),
        crate::lua::write(&crate::lua::Value::Table(old))
    );
    assert_eq!(
        native::write(
            &native::table([("text", native::string("a\"b\\c\nd\u{1}\u{7f}").unwrap())]).unwrap()
        )
        .unwrap(),
        crate::lua::write(&crate::lua::table([(
            "text",
            crate::lua::string("a\"b\\c\nd\u{1}\u{7f}")
        )]))
    );
}

/// Pieces of Lua settings files, put together at random.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::from("return ")),
        Just(String::from("{")),
        Just(String::from("}")),
        Just(String::from(",")),
        Just(String::from(";")),
        Just(String::from(" = ")),
        Just(String::from("[1]")),
        Just(String::from("[\"k\"]")),
        Just(String::from("name")),
        Just(String::from("\"text\\n\\\"q\\\"\"")),
        Just(String::from("'single'")),
        Just(String::from("[[long]]")),
        Just(String::from("[=[a]]b]=]")),
        Just(String::from("-- comment\n")),
        Just(String::from("--[[ block ]]")),
        Just(String::from("12")),
        Just(String::from("-0.5e-3")),
        Just(String::from("0x1F")),
        Just(String::from("true")),
        Just(String::from("nil")),
        Just(String::from("\\x41\\065\\z \n")),
        "[a-z0-9 \\-\\[\\]=\"'{}]{0,12}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(400)))]

    #[test]
    fn pieces_of_files_are_read_as_before(parts in proptest::collection::vec(fragment(), 0..30)) {
        let text = parts.concat();
        let (old, new) = both(&text);
        prop_assert_eq!(new, old, "{}", text);
    }

    #[test]
    fn any_bytes_are_read_as_before_and_without_panic(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let text = String::from_utf8_lossy(&bytes);
        let (old, new) = both(&text);
        prop_assert_eq!(new, old);
    }

    #[test]
    fn any_text_in_a_string_is_written_as_before(text in "\\PC{0,40}|[\\x00-\\x7f]{0,40}") {
        prop_assert_eq!(
            native::write(&native::string(&text).unwrap()).unwrap(),
            crate::lua::write(&crate::lua::string(&text))
        );
    }

    #[test]
    fn names_are_made_as_before(book in "[a-zA-Z0-9 ./_-]{0,30}") {
        prop_assert_eq!(native::sidecar(&book).unwrap(), crate::sidecar(&book));
        prop_assert_eq!(native::metadata_file(&book).unwrap(), crate::metadata_file(&book));
        prop_assert_eq!(native::exchange_file(&book).unwrap(), crate::exchange_file(&book));
        prop_assert_eq!(
            native::known_file(&paperpaw_sdk::Env::plugin_dir("koreader"), &book).unwrap(),
            crate::known_file(&book)
        );
    }

    #[test]
    fn notes_are_fingerprinted_as_before(note in "\\PC{0,60}") {
        prop_assert_eq!(native::fingerprint(&note), crate::fingerprint(&note));
    }
}

fn locator() -> impl Strategy<Value = Locator> {
    (0u32..50, 0u32..5000).prop_map(|(s, o)| Locator::new(s, o))
}

fn known() -> impl Strategy<Value = (Kind, Locator, Locator, Option<u64>)> {
    (
        prop_oneof![Just(Kind::Bookmark), Just(Kind::Highlight)],
        locator(),
        locator(),
        proptest::option::of(any::<u64>()),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// What is known is written and read as before.
    #[test]
    fn what_is_known_is_kept_as_before(
        items in proptest::collection::vec(known(), 0..12),
        seen in 0u32..400,
        emptied in any::<bool>(),
    ) {
        let book = "Books/a.epub";
        let old_known: Vec<crate::Known> = items
            .iter()
            .map(|&(kind, s, e, note)| crate::Known { key: (kind, s, e), note })
            .collect();
        let new_known: Vec<native::Known> = items
            .iter()
            .map(|&(kind, s, e, note)| native::Known { key: (kind, s, e), note })
            .collect();
        let storage = MemoryStorage::new();
        crate::save_known(&storage, book, &old_known, seen);
        let mut written = std::string::String::from_utf8(
            paperpaw_sdk::read_file(&storage, &crate::known_file(book), 1 << 20).unwrap(),
        )
        .unwrap();
        prop_assert_eq!(native::write_known(&new_known, seen).unwrap(), written.clone());
        if emptied {
            written.push_str("emptied\n");
        }
        storage.write(&crate::known_file(book), written.as_bytes()).unwrap();
        let old = crate::load_known(&storage, book).unwrap();
        let new = native::parse_known(&written).unwrap();
        prop_assert_eq!(new.emptied, old.emptied);
        prop_assert_eq!(new.seen, old.seen);
        prop_assert_eq!(new.known.len(), old.known.len());
        for (n, o) in new.known.iter().zip(&old.known) {
            prop_assert_eq!((n.key, n.note), (o.key, o.note));
        }
    }

    #[test]
    fn any_text_of_what_is_known_is_read_as_before(text in "[bh0-9a-f ets\\nm]{0,80}") {
        let storage = MemoryStorage::new();
        storage.write(&crate::known_file("a.epub"), text.as_bytes()).unwrap();
        let old = crate::load_known(&storage, "a.epub").unwrap();
        let new = native::parse_known(&text).unwrap();
        prop_assert_eq!((new.emptied, new.seen), (old.emptied, old.seen));
        prop_assert_eq!(new.known.len(), old.known.len());
        for (n, o) in new.known.iter().zip(&old.known) {
            prop_assert_eq!((n.key, n.note), (o.key, o.note));
        }
    }
}

#[test]
fn a_list_is_in_key_order_with_equal_keys_in_file_order() {
    let text = r#"return { [3] = "c", [1] = "a1", [1] = "a2", [2] = "b", ["x"] = "no" }"#;
    let Some(crate::lua::Value::Table(old)) = crate::lua::parse(text) else {
        panic!("old")
    };
    let Some(native::Value::Table(new)) = native::parse(text).unwrap() else {
        panic!("new")
    };
    let old: Vec<String> = old
        .list()
        .map(|v| match v {
            crate::lua::Value::Str(s) => s.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    let new: Vec<String> = new
        .list()
        .unwrap()
        .into_iter()
        .map(|v| match v {
            native::Value::Str(s) => s.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(new, old);
    assert_eq!(new, ["a1", "a2", "b", "c"]);
}
