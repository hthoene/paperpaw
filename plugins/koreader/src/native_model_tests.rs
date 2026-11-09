//! The native files model: what it reads, writes and names. The expected
//! texts were checked against the interface-6 code before it was removed.
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;

use crate::native_model as native;
use alloc::{format, string::String, vec, vec::Vec};
use paperpaw_sdk::annotations::Kind;
use paperpaw_sdk::document::Locator;
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

/// A text read and written back; `None` where it is refused.
fn again(text: &str) -> Option<String> {
    native::parse(text)
        .unwrap()
        .map(|v| native::write(&v).unwrap())
}

const HEAD: &str = "-- we can read Lua syntax here!\n";

#[test]
fn a_settings_file_is_read_and_written_in_koreaders_form() {
    let expected = concat!(
        "-- we can read Lua syntax here!\nreturn {\n",
        "    [\"annotations\"] = {\n        [1] = {\n",
        "            [\"chapter\"] = \"Chapter I\",\n",
        "            [\"note\"] = \"Two\\nlines\",\n",
        "            [\"pos0\"] = \"/body/DocFragment[3]/body/p[4]/text().0\",\n",
        "            [\"text\"] = \"Down the \\\"Rabbit\\\" Hole\",\n        },\n",
        "        [2] = {\n            [\"datetime\"] = \"2026-10-02 08:00:00\",\n        },\n    },\n",
        "    [\"percent_finished\"] = 0.25,\n    [\"summary\"] = {\n",
        "        [\"status\"] = \"reading\",\n        [\"modified\"] = \"2026-10-02\",\n",
        "        [\"long\"] = \"a ]] b\",\n        [\"bytes\"] = \"AAB\",\n    },\n",
        "    [\"stats\"] = {\n        [1] = 1,\n        [2] = 2,\n        [3] = -3.5e-2,\n",
        "        [4] = true,\n        [5] = false,\n        [7] = 0x1F,\n    },\n}\n",
    );
    assert_eq!(again(SIDECAR).as_deref(), Some(expected));
}

#[test]
fn small_files_are_read_or_refused() {
    let with = |body: &str| Some(format!("{HEAD}{body}"));
    assert_eq!(again("return nil"), with("return nil\n"));
    assert_eq!(again("return 12"), with("return 12\n"));
    assert_eq!(
        again("--[[ long\ncomment ]] return { a = 1 }"),
        with("return {\n    [\"a\"] = 1,\n}\n")
    );
    assert_eq!(
        again("return { [\"k\"] = [[x]], }"),
        with("return {\n    [\"k\"] = \"x\",\n}\n")
    );
    assert_eq!(
        again("return { 1, 2, [5] = 3 }"),
        with("return {\n    [1] = 1,\n    [2] = 2,\n    [5] = 3,\n}\n")
    );
    for refused in [
        "",
        "return {",
        "return {[1] 2}",
        "return \"open",
        "return nonsense",
        "-- only a comment",
        "return \"a\\xZZ\"",
        "return \"\\300\"",
        "return \"line\nbreak\"",
        "\u{feff}return {}",
    ] {
        assert_eq!(again(refused), None, "{refused:?}");
    }
}

#[test]
fn nesting_stops_at_sixty_four() {
    let ok = format!("return {}{}", "{".repeat(60), "}".repeat(60));
    assert!(again(&ok).is_some());
    let deep = format!("return {}{}", "{".repeat(100), "}".repeat(100));
    assert_eq!(again(&deep), None);
}

#[test]
fn tables_are_used_and_built() {
    let Some(native::Value::Table(file)) = native::parse(SIDECAR).unwrap() else {
        panic!("file")
    };
    let Some(native::Value::Table(list)) = file.get("annotations") else {
        panic!("annotations")
    };
    let first = list.list().unwrap();
    assert_eq!(first.len(), 2);
    let native::Value::Table(a) = first[0] else {
        panic!("first")
    };
    assert_eq!(a.str("note"), Some("Two\nlines"));
    assert_eq!(a.str("chapter"), Some("Chapter I"));
    assert_eq!(a.str("nothing"), None);
    let mut table = native::Table::default();
    table.set("a", native::string("x").unwrap()).unwrap();
    table.set("b", native::Value::Bool(true)).unwrap();
    table.set("a", native::Value::Nil).unwrap();
    assert_eq!(
        native::write(&native::Value::Table(table)).unwrap(),
        format!("{HEAD}return {{\n    [\"b\"] = true,\n}}\n")
    );
    let list =
        native::Table::from_list([native::string("p").unwrap(), native::string("q").unwrap()])
            .unwrap();
    assert_eq!(
        native::write(&native::Value::Table(list)).unwrap(),
        format!("{HEAD}return {{\n    [1] = \"p\",\n    [2] = \"q\",\n}}\n")
    );
    assert_eq!(
        native::write(
            &native::table([("text", native::string("a\"b\\c\nd\u{1}\u{7f}").unwrap())]).unwrap()
        )
        .unwrap(),
        format!("{HEAD}return {{\n    [\"text\"] = \"a\\\"b\\\\c\\nd\\001\\127\",\n}}\n")
    );
}

#[test]
fn the_names_beside_a_book() {
    let dir = paperpaw_sdk::Env::plugin_dir("koreader");
    let name = |book: &str| {
        (
            native::sidecar(book).unwrap(),
            native::metadata_file(book).unwrap(),
            native::exchange_file(book).unwrap(),
        )
    };
    assert_eq!(
        name("Books/a.epub"),
        (
            "Books/a.sdr".into(),
            "Books/a.sdr/metadata.epub.lua".into(),
            "Books/a.sdr/a.epub.annotations.lua".into()
        )
    );
    assert_eq!(
        name("a"),
        (
            "a.sdr".into(),
            "a.sdr/metadata._.lua".into(),
            "a.sdr/a.annotations.lua".into()
        )
    );
    assert_eq!(
        name("x.y/b.c.epub"),
        (
            "x.y/b.c.sdr".into(),
            "x.y/b.c.sdr/metadata.epub.lua".into(),
            "x.y/b.c.sdr/b.c.epub.annotations.lua".into()
        )
    );
    assert_eq!(
        native::known_file(&dir, "Books/a.epub").unwrap(),
        ".paperpaw/plugins/koreader/7a3c21d7188a879f.txt"
    );
    assert_eq!(
        native::known_file(&dir, "").unwrap(),
        ".paperpaw/plugins/koreader/cbf29ce484222325.txt"
    );
}

#[test]
fn notes_are_fingerprinted_with_fnv_1a() {
    assert_eq!(native::fingerprint(""), 14_695_981_039_346_656_037);
    assert_eq!(native::fingerprint("a"), 12_638_187_200_555_641_996);
    assert_eq!(native::fingerprint("Two\nlines"), 7_669_852_316_015_712_144);
    assert_eq!(
        native::fingerprint("héllo wörld"),
        1_261_653_000_850_579_490
    );
}

#[test]
fn a_list_is_in_key_order_with_equal_keys_in_file_order() {
    let text = r#"return { [3] = "c", [1] = "a1", [1] = "a2", [2] = "b", ["x"] = "no" }"#;
    let Some(native::Value::Table(table)) = native::parse(text).unwrap() else {
        panic!("table")
    };
    let list: Vec<String> = table
        .list()
        .unwrap()
        .into_iter()
        .map(|v| match v {
            native::Value::Str(s) => s.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(list, ["a1", "a2", "b", "c"]);
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

    /// What reads is written in a form that reads back as the same value.
    #[test]
    fn pieces_of_files_read_back_as_the_same(parts in proptest::collection::vec(fragment(), 0..30)) {
        let text = parts.concat();
        if let Some(value) = native::parse(&text).unwrap() {
            let written = native::write(&value).unwrap();
            prop_assert_eq!(native::parse(&written).unwrap(), Some(value), "{}", text);
        }
    }

    #[test]
    fn any_bytes_are_read_without_panic(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let text = String::from_utf8_lossy(&bytes);
        std::hint::black_box(native::parse(&text).is_ok());
    }

    #[test]
    fn any_text_in_a_string_reads_back(text in "\\PC{0,40}|[\\x00-\\x7f]{0,40}") {
        let written = native::write(&native::string(&text).unwrap()).unwrap();
        prop_assert_eq!(native::parse(&written).unwrap(), Some(native::string(&text).unwrap()));
    }

    #[test]
    fn names_are_made_without_panic(book in "[a-zA-Z0-9 ./_-]{0,30}") {
        prop_assert!(native::sidecar(&book).unwrap().ends_with(".sdr"));
        prop_assert!(native::metadata_file(&book).unwrap().ends_with(".lua"));
        prop_assert!(native::exchange_file(&book).unwrap().ends_with(".annotations.lua"));
    }
}

fn locator() -> impl Strategy<Value = Locator> {
    (0u32..50, 0u32..5000).prop_map(|(s, o)| Locator::new(s, o))
}

fn known() -> impl Strategy<Value = native::Known> {
    (
        prop_oneof![Just(Kind::Bookmark), Just(Kind::Highlight)],
        locator(),
        locator(),
        proptest::option::of(any::<u64>()),
    )
        .prop_map(|(kind, s, e, note)| native::Known {
            key: (kind, s, e),
            note,
        })
}

#[test]
fn what_is_known_is_written_in_its_form() {
    let items = [
        native::Known {
            key: (Kind::Highlight, Locator::new(3, 4), Locator::new(3, 9)),
            note: Some(77),
        },
        native::Known {
            key: (Kind::Bookmark, Locator::new(1, 0), Locator::new(1, 0)),
            note: None,
        },
    ];
    assert_eq!(
        native::write_known(&items, 12).unwrap(),
        "theirs 12\nh 3 4 3 9 000000000000004d\nb 1 0 1 0\n"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// What is known is written and read back the same.
    #[test]
    fn what_is_known_is_kept(items in proptest::collection::vec(known(), 0..12), seen in 0u32..400) {
        let written = native::write_known(&items, seen).unwrap();
        let back = native::parse_known(&written).unwrap();
        prop_assert_eq!(back.seen, seen);
        prop_assert_eq!(back.known.len(), items.len());
        for (b, i) in back.known.iter().zip(&items) {
            prop_assert_eq!((b.key, b.note), (i.key, i.note));
        }
    }

    #[test]
    fn any_text_of_what_is_known_is_read_without_panic(text in "[bh0-9a-f ets\\nm]{0,80}") {
        std::hint::black_box(native::parse_known(&text).is_ok());
    }
}
