//! The catalogs built in without their keys: one catalog a language that
//! answers as their sources would, with none of them added.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_i18n::{Arg, BuiltMessages, BuiltMessagesTable, Catalog, Translator};

const CLOCK: &str = "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n\
    # A comment the program does not carry.\n\
    [messages]\nclock.title = Clock\nclock.quoted = \"  spaced  \"\nclock.tab = \"a\\tb\"\n\
    clock.empty = \"\"\nclock.only-en = English only\n\
    clock.hours.one = {count} hour\nclock.hours.other = {count} hours\n";
const NOTES: &str = "[meta]\nlanguage = en\nname = English\n[messages]\n\
    notes.title = Notes\nnotes.hello = Hello, {name}!\n";
const CLOCK_DE: &str = "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n\
    [messages]\nclock.title = Uhr\nclock.quoted = \"  weit  \"\nclock.tab = \"a\\tb\"\n\
    clock.empty = \"\"\nclock.hours.one = {count} Stunde\nclock.hours.other = {count} Stunden\n";
const NOTES_DE: &str = "[meta]\nlanguage = de\nname = Deutsch\n[messages]\n\
    notes.title = Notizen\nnotes.hello = Hallo, {name}!\nnotes.only-de = \"eins\\nzwei\"\n";
const SOURCES: [&str; 4] = [CLOCK, NOTES, CLOCK_DE, NOTES_DE];
const KEYS: [&str; 12] = [
    "clock.title",
    "clock.quoted",
    "clock.tab",
    "clock.empty",
    "clock.only-en",
    "clock.hours.one",
    "clock.hours.other",
    "notes.title",
    "notes.hello",
    "notes.only-de",
    "missing.key",
    "",
];

fn table() -> &'static [BuiltMessages] {
    let mut table = BuiltMessagesTable::new(&["en", "de"]);
    for source in SOURCES {
        assert!(table.add(source).unwrap());
    }
    assert_eq!(table.len(), 4);
    assert!(!table.is_empty());
    table.leak()
}

fn built() -> Translator {
    let mut translator = Translator::new();
    for language in table() {
        translator.built_messages(language);
    }
    translator
}

fn plain() -> Translator {
    let mut translator = Translator::new();
    for source in SOURCES {
        translator.add(Catalog::parse(source).unwrap());
    }
    translator
}

#[test]
fn the_tables_answer_as_the_catalogs_would_in_each_language() {
    let (mut built, mut plain) = (built(), plain());
    // One catalog a language, none of the sources added.
    assert_eq!(built.catalogs().len(), 2);
    assert_eq!(built.languages(), [("en", "English"), ("de", "Deutsch")]);
    assert_eq!(built.languages(), plain.languages());
    for language in ["en", "de", "de-AT", "fr"] {
        built.set_language(language);
        plain.set_language(language);
        for key in KEYS {
            assert_eq!(built.get(key), plain.get(key), "{language} {key}");
        }
        for count in [0, 1, 2] {
            let args = [("count", Arg::Int(count))];
            assert_eq!(
                built.plural("clock.hours", count, &args),
                plain.plural("clock.hours", count, &args),
                "{language} {count}"
            );
        }
    }
    // What German lacks comes from English, and the other way not.
    built.set_language("de");
    assert_eq!(built.get("clock.title"), Some("Uhr"));
    assert_eq!(built.get("clock.only-en"), Some("English only"));
    assert_eq!(built.get("notes.only-de"), Some("eins\nzwei"));
    built.set_language("en");
    assert_eq!(built.get("notes.only-de"), None);
    assert_eq!(built.get("clock.empty"), Some(""));
}

#[test]
fn a_catalog_added_later_takes_precedence_over_the_tables() {
    let mut translator = built();
    let user = "[meta]\nlanguage = en\n[messages]\nclock.title = My clock\n";
    translator.add(Catalog::parse(user).unwrap());
    assert_eq!(translator.get("clock.title"), Some("My clock"));
    assert_eq!(translator.get("notes.title"), Some("Notes"));
    // Once the language was changed and is English again, too.
    translator.set_language("de");
    assert_eq!(translator.get("clock.title"), Some("Uhr"));
    translator.set_language("en");
    assert_eq!(translator.get("clock.title"), Some("My clock"));
    translator.shrink_to_fit();
    assert_eq!(translator.get("notes.title"), Some("Notes"));
}

#[test]
fn a_source_the_tables_cannot_hold_is_left_out_whole() {
    let mut table = BuiltMessagesTable::new(&["en", "de"]);
    assert!(table.add(CLOCK).unwrap());
    assert!(table.add(CLOCK_DE).unwrap());
    let size = table.size();
    let refused = [
        // A language not the table's, a key another source of the
        // language holds, another name for the language or none.
        "[meta]\nlanguage = fr\n[messages]\nfr.a = a\n",
        "[meta]\nlanguage = en\nname = English\n[messages]\nnew.a = a\nclock.title = Clock\n",
        "[meta]\nlanguage = de\nname = German\n[messages]\nname.a = a\n",
        "[meta]\nlanguage = en\n[messages]\nname.b = b\n",
        // A direction of its own, another plural rule, a NUL in a message.
        "[meta]\nlanguage = de\nname = Deutsch\ndirection = rtl\n[messages]\nrtl.a = a\n",
        "[meta]\nlanguage = en\nname = English\nplural = arabic\n[messages]\nplural.a = a\n",
        "[meta]\nlanguage = en\nname = English\n[messages]\nnul.a = \"a\\u{0}b\"\n",
        // Two keys whose hashes share their low half: in one source, and
        // the second in a source of its own.
        "[meta]\nlanguage = en\nname = English\n[messages]\nclash.key34869 = a\nclash.key239602 = b\n",
    ];
    for source in refused {
        assert!(!table.add(source).unwrap(), "{source}");
        assert_eq!((table.len(), table.size()), (2, size), "{source}");
    }
    assert!(
        table
            .add("[meta]\nlanguage = en\nname = English\n[messages]\nclash.key34869 = a\n")
            .unwrap()
    );
    assert!(
        !table
            .add("[meta]\nlanguage = de\nname = Deutsch\n[messages]\nclash.key239602 = b\n")
            .unwrap()
    );
    assert!(table.add("not a catalog = \n[").is_err());
    let rust = table.to_rust();
    assert!(rust.starts_with("{ static KEYS: [u32; 8] = [0x"), "{rust}");
    assert!(rust.contains("paperpaw_i18n::BuiltMessages::new(\"en\", \"English\", &KEYS, &[0x"));
    assert!(rust.contains("paperpaw_i18n::BuiltMessages::new(\"de\", \"Deutsch\", &KEYS, &[0x"));
    assert!(rust.contains("\"Clock\\u{0}"), "{rust}");
    assert!(!rust.contains("clock.title"), "no key is written: {rust}");
    // The key left out is not taken for the one that shares half its
    // hash, and no key of a source refused answers.
    let mut translator = Translator::new();
    for language in table.leak() {
        translator.built_messages(language);
    }
    assert_eq!(translator.get("clash.key34869"), Some("a"));
    for key in [
        "clash.key239602",
        "new.a",
        "fr.a",
        "nul.a",
        "plural.a",
        "name.b",
    ] {
        assert_eq!(translator.get(key), None, "{key}");
    }
    assert_eq!(translator.get("clock.title"), Some("Clock"));
}

#[test]
fn a_language_no_source_was_added_for_makes_no_table() {
    let mut table = BuiltMessagesTable::new(&["en", "de"]);
    assert!(table.is_empty());
    assert!(table.add(NOTES).unwrap());
    let made = table.leak();
    assert_eq!(made.len(), 1);
    assert_eq!((made[0].language, made[0].name), ("en", "English"));
    // A table with no key adds no catalog.
    let none = BuiltMessagesTable::new(&["en"]).leak();
    assert!(none.is_empty());
    let mut translator = Translator::new();
    static EMPTY: BuiltMessages = BuiltMessages::new("en", "English", &[], &[], "");
    translator.built_messages(&EMPTY);
    assert!(translator.catalogs().is_empty());
}
