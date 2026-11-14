//! *Discover fonts* (the reading settings' entry below the font size): what it
//! does where the fonts browser runs, is missing, failed, is off or waits for a
//! restart, with the interface-6 store's words in both languages.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_i18n::{Arg as Word, Catalog, Translator};
use paperpaw_plugin_store::native_ask::Say;
use paperpaw_plugin_store::native_fonts::{self as fonts, Step, Then};
use paperpaw_plugin_store::native_installs::Row;
use paperpaw_plugin_store::native_page::{Kind, Trouble};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;

const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

struct Words(Translator);

fn say_in(language: &str, manager: bool) -> Words {
    let mut text = String::from(if language == "de" { DE } else { EN });
    if manager {
        // The plugin manager's word, which the host merges into the same catalog.
        text.push_str("\nplugin-manager.code.other-build = Built for another PaperPaw.\n");
    }
    let mut translator = Translator::new();
    translator.add(Catalog::parse(text).unwrap());
    translator.set_language(language);
    Words(translator)
}

impl Say for Words {
    fn lookup(&mut self, key: &str) -> Result<Option<String>, Error> {
        Ok(self.0.get(key).map(ToString::to_string))
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let named: Vec<(&str, Word<'_>)> = args
            .iter()
            .filter_map(|arg| match arg {
                Arg::Text { name, value } => Some((*name, Word::Str(value))),
                _ => None,
            })
            .collect();
        Ok(self.0.format(key, &named))
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        Ok(self.0.localize_digits(text).into_owned())
    }
}

fn row(enabled: bool, restart: bool, trouble: Option<Trouble>) -> Row {
    Row::new(
        "fonts-browser",
        Kind::Code,
        enabled,
        restart,
        "1.0.0",
        trouble,
    )
    .unwrap()
}

fn asked(step: Step) -> fonts::Words {
    match step {
        Step::Ask(words) => words,
        other => panic!("not a question: {other:?}"),
    }
}

#[test]
fn the_fonts_browser_runs_its_own_list() {
    // Whatever else the device says of it.
    assert_eq!(fonts::decide(true, None), Step::Run);
    assert_eq!(
        fonts::decide(true, Some(&row(false, true, Some(Trouble::Other)))),
        Step::Run
    );
}

#[test]
fn a_fonts_browser_that_is_not_installed_is_asked_about() {
    let words = asked(fonts::decide(false, None));
    assert_eq!(words.title, "store.fonts.missing");
    assert_eq!(words.then, Then::Page);
}

#[test]
fn one_switched_off_is_switched_on_with_a_restart() {
    let words = asked(fonts::decide(false, Some(&row(false, false, None))));
    assert_eq!(words.title, "store.fonts.off");
    assert_eq!(words.then, Then::On);
}

#[test]
fn one_that_waits_for_a_restart_asks_for_it() {
    let words = asked(fonts::decide(false, Some(&row(true, true, None))));
    assert_eq!(words.title, "store.fonts.restart");
    assert_eq!(words.then, Then::Restart);
    assert_eq!(words.yes, "store.restart");
}

#[test]
fn one_that_failed_says_why_and_offers_its_page() {
    let words = asked(fonts::decide(
        false,
        Some(&row(true, false, Some(Trouble::OtherBuild))),
    ));
    assert_eq!(words.title, "store.fonts.failed");
    assert_eq!(words.then, Then::Page);
    assert_eq!(words.failed, Some(Trouble::OtherBuild));
    // A plugin that failed is never taken for one that was only switched off.
    let off = asked(fonts::decide(
        false,
        Some(&row(false, false, Some(Trouble::Other))),
    ));
    assert_eq!(off.title, "store.fonts.failed");
}

#[test]
fn one_switched_on_and_not_running_opens_its_page() {
    assert_eq!(
        fonts::decide(false, Some(&row(true, false, None))),
        Step::Page
    );
}

#[test]
fn every_question_is_said_in_both_languages_with_two_words() {
    let rows = [
        None,
        Some(row(false, false, None)),
        Some(row(true, true, None)),
        Some(row(true, false, Some(Trouble::OtherBuild))),
        Some(row(true, false, Some(Trouble::Other))),
    ];
    for language in ["en", "de"] {
        for manager in [false, true] {
            for row in &rows {
                let words = asked(fonts::decide(false, row.as_ref()));
                let mut say = say_in(language, manager);
                let texts = fonts::texts(&mut say, &words).unwrap();
                texts.question().unwrap();
                assert!(!texts.title().is_empty(), "{language} {words:?}");
                assert!(!texts.text().is_empty(), "{language} {words:?}");
                let (yes, no) = texts.labels().split_once('\n').unwrap();
                assert!(!yes.is_empty() && !no.is_empty(), "{language} {words:?}");
            }
        }
    }
}

#[test]
fn the_reason_is_the_plugin_managers_word_or_the_stores_own() {
    let without = fonts::reason(&mut say_in("en", false), Trouble::OtherBuild).unwrap();
    assert!(without.starts_with("It could not be loaded."), "{without}");
    assert!(without.contains("Manage plugins"), "{without}");
    let with = fonts::reason(&mut say_in("en", true), Trouble::OtherBuild).unwrap();
    assert!(with.starts_with("Built for another PaperPaw."), "{with}");
    // Any other trouble has no word of the manager here.
    let other = fonts::reason(&mut say_in("en", true), Trouble::Other).unwrap();
    assert!(other.starts_with("It could not be loaded."), "{other}");
}
