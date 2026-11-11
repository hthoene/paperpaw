//! The English catalogs built in, indexed together: one catalog for all,
//! filled as each plugin adds its own, as each would have answered.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_i18n::{Arg, BuiltTable, Catalog, Translator};

const CLOCK: &str = "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n\
    [messages]\nclock.title = Clock\nclock.quoted = \"  spaced  \"\nclock.tab = \"a\\tb\"\n\
    clock.hours.one = {count} hour\nclock.hours.other = {count} hours\n";
const NOTES: &str =
    "[meta]\nlanguage = en\n[messages]\nnotes.title = Notes\nnotes.hello = Hello, {name}!\n";
const CLASH: &str = "[meta]\nlanguage = en\n[messages]\nclock.title = Another clock\n";
const GERMAN: &str = "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n\
    [messages]\nclock.title = Uhr\n";

fn table() -> &'static paperpaw_i18n::BuiltCatalogs {
    let mut table = BuiltTable::new();
    assert!(table.add(CLOCK).unwrap());
    assert!(table.add(NOTES).unwrap());
    // A key another holds, and a language not English: left out.
    assert!(!table.add(CLASH).unwrap());
    assert!(!table.add(GERMAN).unwrap());
    assert!(!table.add(CLOCK).unwrap(), "added once");
    assert_eq!(table.len(), 2);
    assert!(
        table
            .to_rust()
            .starts_with("paperpaw_i18n::BuiltCatalogs::new(&[")
    );
    table.leak()
}

/// The same answers as the catalogs added one by one.
fn each(translator: &mut Translator) {
    for source in [CLOCK, NOTES] {
        translator.add(Catalog::parse(source).unwrap());
    }
}

#[test]
fn the_table_answers_as_the_catalogs_would() {
    let mut built = Translator::new();
    built.built_catalogs(table());
    each(&mut built);
    let mut plain = Translator::new();
    each(&mut plain);
    for key in [
        "clock.title",
        "clock.quoted",
        "clock.tab",
        "notes.title",
        "notes.hello",
        "missing.key",
    ] {
        assert_eq!(built.get(key), plain.get(key), "{key}");
    }
    assert_eq!(built.get("clock.quoted"), Some("  spaced  "));
    assert_eq!(built.get("clock.tab"), Some("a\tb"));
    assert_eq!(built.plural("clock.hours", 2, &[]), "2 hours");
    assert_eq!(
        built.format("notes.hello", &[("name", Arg::Str("Ada"))]),
        "Hello, Ada!"
    );
    // One catalog in place of two, with the keys of both (for tools,
    // such as the pseudo-language's).
    assert_eq!(built.catalogs().len(), 1);
    let keys: Vec<String> = built.catalogs()[0].keys().map(String::from).collect();
    let mut both: Vec<String> = [CLOCK, NOTES]
        .iter()
        .flat_map(|s| {
            let catalog = Catalog::parse(*s).unwrap();
            catalog.keys().map(String::from).collect::<Vec<_>>()
        })
        .collect();
    both.sort();
    assert_eq!(keys, both);
    assert_eq!(built.languages(), [("en", "English")]);
}

#[test]
fn a_plugin_not_added_answers_nothing_and_later_catalogs_win() {
    let mut t = Translator::new();
    t.built_catalogs(table());
    t.add(Catalog::parse(NOTES).unwrap());
    // The clock plugin is not here: its keys are not either, plain or
    // decoded (quoted, escaped), and none is listed.
    assert_eq!(t.get("clock.title"), None);
    assert_eq!(t.get("clock.quoted"), None);
    assert_eq!(t.get("clock.tab"), None);
    assert_eq!(t.get("notes.title"), Some("Notes"));
    let keys: Vec<String> = t.catalogs()[0].keys().map(String::from).collect();
    assert_eq!(keys, ["notes.hello", "notes.title"]);
    // A catalog added after (a data plugin's) takes precedence.
    t.add(Catalog::parse(CLASH).unwrap());
    t.add(Catalog::parse(CLOCK).unwrap());
    assert_eq!(t.get("clock.title"), Some("Another clock"));
    // An English catalog not in the table stays one of its own.
    assert_eq!(t.catalogs().len(), 2);
}

#[test]
fn another_language_goes_first_and_english_stays_behind_it() {
    let mut t = Translator::new();
    t.built_catalogs(table());
    each(&mut t);
    t.add(Catalog::parse(GERMAN).unwrap());
    t.set_language("de");
    assert_eq!(t.get("clock.title"), Some("Uhr"));
    // What German lacks comes from the table still (not dropped as
    // covered).
    assert_eq!(t.get("notes.title"), Some("Notes"));
    t.set_language("en");
    assert_eq!(t.get("clock.title"), Some("Clock"));
}

#[test]
fn an_owned_source_is_never_taken() {
    let mut t = Translator::new();
    t.built_catalogs(table());
    t.add(Catalog::parse(String::from(CLOCK)).unwrap());
    assert_eq!(t.get("clock.title"), Some("Clock"));
    assert_eq!(t.catalogs().len(), 2);
}

const NOTES_DE: &str = "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n\
    [messages]\nnotes.title = Notizen\nnotes.lines = \"eins\\nzwei\"\n";
const RTL: &str = "[meta]\nlanguage = de\nname = Deutsch\ndirection = rtl\n[messages]\nrtl.a = a\n";
const OTHER_NAME: &str = "[meta]\nlanguage = de\nname = German\n[messages]\nname.a = a\n";

fn german() -> &'static paperpaw_i18n::BuiltCatalogs {
    let mut table = BuiltTable::for_language("de");
    assert!(table.add(GERMAN).unwrap());
    assert!(table.add(NOTES_DE).unwrap());
    // English, a direction of its own and another name: left out.
    assert!(!table.add(NOTES).unwrap());
    assert!(!table.add(RTL).unwrap());
    assert!(!table.add(OTHER_NAME).unwrap());
    assert!(
        table
            .to_rust()
            .ends_with(".in_language(\"de\", \"Deutsch\")")
    );
    table.leak()
}

#[test]
fn a_table_for_german_answers_as_its_catalogs_would_and_keeps_decoded_texts() {
    let mut t = Translator::new();
    t.built_catalogs(table());
    t.built_catalogs(german());
    each(&mut t);
    for source in [GERMAN, NOTES_DE] {
        t.add(Catalog::parse(source).unwrap());
    }
    // One catalog for each language, and German is listed while English
    // is chosen.
    assert_eq!(t.catalogs().len(), 2);
    assert_eq!(t.languages(), [("en", "English"), ("de", "Deutsch")]);
    assert_eq!(t.get("notes.title"), Some("Notes"));
    t.set_language("de");
    assert_eq!(t.get("clock.title"), Some("Uhr"));
    assert_eq!(t.get("notes.title"), Some("Notizen"));
    assert_eq!(t.get("notes.lines"), Some("eins\nzwei"));
    // What German lacks comes from English.
    assert_eq!(t.get("clock.quoted"), Some("  spaced  "));
    t.set_language("en");
    assert_eq!(t.get("notes.title"), Some("Notes"));
    assert_eq!(t.get("notes.lines"), None);
}
