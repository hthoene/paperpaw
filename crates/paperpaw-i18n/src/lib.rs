//! Lean message catalogs for PaperPaw.
//!
//! Translations are plain files in the PaperPaw config format (see
//! `paperpaw-config`), one per language and plugin, e.g. `i18n/de.ini`:
//!
//! ```ini
//! [meta]
//! language = de
//! name = Deutsch
//! plural = one-other
//!
//! [messages]
//! home.continue = Weiterlesen
//! library.books.one = {count} Buch
//! library.books.other = {count} Bücher
//! ```
//!
//! Adding a language means adding files, no code changes, unless the
//! language needs a plural rule that [`PluralRule`] does not know yet.
//! Every plugin ships its own catalogs with keys in its namespace; language
//! packs may add catalogs for many plugins at once.
//!
//! A [`Translator`] looks messages up through a fallback chain (for example
//! `de-AT` → `de` → `en`) and finally returns the key itself, so a missing
//! translation never breaks the UI.

#![no_std]

extern crate alloc;

mod catalog;
pub mod check;
mod format;
mod plural;

pub use catalog::{Catalog, CatalogError, FixedIndexes, fingerprint};
pub use catalog::{calendars, digit_systems};
pub use format::Arg;
pub use paperpaw_lang::{Direction, Lang};
pub use plural::{PluralCategory, PluralRule, cldr_rules};

use alloc::string::String;
use alloc::vec::Vec;

use catalog::Parked;

/// The language every message must exist in; the last fallback.
pub const FALLBACK_LANGUAGE: &str = "en";

/// Looks up messages in all loaded catalogs, following the fallback chain of
/// the selected language.
///
/// Only the languages of the chain are kept as catalogs; the others are
/// parked, their source and little more, until their language is chosen.
/// Where the language is not English, an English catalog whose every
/// message the others of the chain have drops its index
/// ([`Translator::shrink_to_fit`]).
#[derive(Debug, Default)]
pub struct Translator {
    catalogs: Vec<Catalog>,
    parked: Vec<Parked>,
    language: String,
    /// Catalog indices in lookup order.
    order: Vec<u16>,
    /// Catalogs added so far.
    added: u32,
    /// Indexes of the catalogs built in, kept in the program.
    fixed: FixedIndexes,
    /// The interface's direction in the language: 1 right to left, 0
    /// left to right (see [`Translator::direction`]).
    direction: u8,
}

impl Translator {
    pub fn new() -> Self {
        let mut translator = Self::default();
        translator.set_language(FALLBACK_LANGUAGE);
        translator
    }

    /// The indexes of the catalogs built in, kept in the program: such
    /// catalogs keep no index on the heap.
    pub fn fixed_indexes(&mut self, fixed: FixedIndexes) {
        self.fixed = fixed;
        for catalog in &mut self.catalogs {
            catalog.use_fixed(fixed);
        }
    }

    /// Adds a catalog. For the same language, catalogs added later take
    /// precedence, so user overrides can be loaded last.
    pub fn add(&mut self, mut catalog: Catalog) {
        catalog.added = self.added;
        self.added = self.added.wrapping_add(1);
        if self.chain().contains(&catalog.language()) {
            catalog.load();
            catalog.use_fixed(self.fixed);
            self.catalogs.push(catalog);
            self.rebuild_order();
            return;
        }
        match catalog.park() {
            Ok(parked) => self.parked.push(parked),
            Err(mut catalog) => {
                catalog.unload();
                self.catalogs.push(catalog);
            }
        }
    }

    /// Releases unused capacity once all catalogs are added: English
    /// catalogs the language chosen covers drop their index.
    pub fn shrink_to_fit(&mut self) {
        self.drop_covered();
        self.catalogs.shrink_to_fit();
        self.parked.shrink_to_fit();
    }

    /// Selects the UI language, e.g. `de` or `pt-BR`.
    pub fn set_language(&mut self, language: &str) {
        self.language.clear();
        self.language.push_str(language);
        self.repark();
        self.drop_covered();
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// The direction of the interface in the language: right to left
    /// where the first of its catalogs that says (`[meta] direction`)
    /// says so, else left to right.
    pub fn direction(&self) -> Direction {
        match self.direction {
            1 => Direction::Rtl,
            _ => Direction::Ltr,
        }
    }

    /// Available languages as `(tag, name)`, each listed once: English,
    /// PaperPaw's own language, first, then the others by tag.
    pub fn languages(&self) -> Vec<(&str, &str)> {
        let mut languages: Vec<(&str, &str)> = Vec::new();
        let live = self
            .catalogs
            .iter()
            .map(|c| (c.added, c.language(), c.name()));
        let parked = self
            .parked
            .iter()
            .map(|p| (p.added, p.language(), p.name()));
        let mut all: Vec<(u32, &str, &str)> = live.chain(parked).collect();
        all.sort_unstable_by_key(|(added, ..)| *added);
        for (_, tag, name) in all {
            if !languages.iter().any(|(t, _)| *t == tag) {
                languages.push((tag, name));
            }
        }
        let english = |tag: &str| tag == "en" || tag.starts_with("en-");
        languages.sort_unstable_by_key(|&(tag, _)| (!english(tag), tag));
        languages
    }

    /// The bytes the catalogs' indexes take on the heap (see
    /// [`Translator::fixed_indexes`]).
    pub fn indexes_on_heap(&self) -> usize {
        self.catalogs.iter().map(Catalog::index_on_heap).sum()
    }

    /// Every catalog added, in the order added, for tools that check
    /// translations: copies, those of languages not in use without their
    /// messages (such a tool [`Catalog::load`]s it).
    pub fn catalogs(&self) -> Vec<Catalog> {
        let mut all: Vec<Catalog> = self.catalogs.clone();
        all.extend(self.parked.iter().cloned().map(Parked::unpark));
        all.sort_unstable_by_key(|c| c.added);
        all
    }

    /// The message for `key`, if a catalog has it.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.ordered().find_map(|catalog| catalog.get(key))
    }

    /// The message for `key`, or `key` itself if no catalog has it.
    pub fn tr<'a>(&'a self, key: &'a str) -> &'a str {
        self.ordered()
            .find_map(|catalog| catalog.get(key))
            .unwrap_or(key)
    }

    /// The message for `key` with `{placeholders}` replaced by `args`.
    pub fn format(&self, key: &str, args: &[(&str, Arg<'_>)]) -> String {
        format::interpolate(self.tr(key), args)
    }

    /// Selects the plural form of `key` for `count`, then formats it.
    ///
    /// Looks up `key.<category>` (e.g. `library.books.few`) using each
    /// catalog's own plural rule, falling back to `key.other`. `{count}` is
    /// always available as an argument.
    pub fn plural(&self, key: &str, count: i64, args: &[(&str, Arg<'_>)]) -> String {
        let mut candidate = String::with_capacity(key.len() + 6);
        for catalog in self.ordered() {
            for category in [catalog.plural_rule().category(count), PluralCategory::Other] {
                candidate.clear();
                candidate.push_str(key);
                candidate.push('.');
                candidate.push_str(category.id());
                if let Some(message) = catalog.get(&candidate) {
                    return format::interpolate_with_count(message, count, args);
                }
            }
        }
        String::from(key)
    }

    fn ordered(&self) -> impl Iterator<Item = &Catalog> {
        self.order.iter().map(|&i| &self.catalogs[i as usize])
    }

    /// The languages looked in, in turn: the one chosen, its base
    /// language, English.
    fn chain(&self) -> Vec<&str> {
        let base = self.language.split('-').next().unwrap_or_default();
        let mut chain: Vec<&str> = Vec::with_capacity(3);
        for language in [self.language.as_str(), base, FALLBACK_LANGUAGE] {
            if !language.is_empty() && !chain.contains(&language) {
                chain.push(language);
            }
        }
        chain
    }

    /// Catalogs of the chain's languages made and loaded, the others
    /// parked, in the order added.
    fn repark(&mut self) {
        let chain: Vec<String> = self.chain().into_iter().map(String::from).collect();
        let in_chain = |language: &str| chain.iter().any(|c| c == language);
        let mut at = 0;
        while at < self.parked.len() {
            if in_chain(self.parked[at].language()) {
                let catalog = self.parked.swap_remove(at).unpark();
                self.catalogs.push(catalog);
            } else {
                at += 1;
            }
        }
        let mut kept = Vec::with_capacity(self.catalogs.len());
        for mut catalog in core::mem::take(&mut self.catalogs) {
            if in_chain(catalog.language()) {
                catalog.load();
                catalog.use_fixed(self.fixed);
                kept.push(catalog);
                continue;
            }
            match catalog.park() {
                Ok(parked) => self.parked.push(parked),
                Err(mut catalog) => {
                    catalog.unload();
                    kept.push(catalog);
                }
            }
        }
        kept.sort_unstable_by_key(|c| c.added);
        self.catalogs = kept;
        self.rebuild_order();
    }

    /// Where the language is not English: English catalogs whose every
    /// message the chain's other languages have drop their index, never
    /// looked in.
    fn drop_covered(&mut self) {
        let english = |language: &str| language == FALLBACK_LANGUAGE;
        if english(self.language.split('-').next().unwrap_or_default()) {
            return;
        }
        let covered: Vec<bool> = self
            .catalogs
            .iter()
            .map(|catalog| {
                english(catalog.language())
                    && catalog.is_loaded()
                    && catalog.keys().all(|key| {
                        self.ordered()
                            .filter(|other| !english(other.language()))
                            .any(|other| other.get(key).is_some())
                    })
            })
            .collect();
        for (catalog, covered) in self.catalogs.iter_mut().zip(covered) {
            if covered {
                catalog.unload();
            }
        }
        self.rebuild_order();
    }

    fn rebuild_order(&mut self) {
        let chain = self.chain();
        let mut order = Vec::new();
        for language in chain {
            let matching = self
                .catalogs
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, catalog)| catalog.language() == language && catalog.is_loaded())
                .map(|(i, _)| i as u16);
            order.extend(matching);
        }
        order.shrink_to_fit();
        self.direction = order
            .iter()
            .filter_map(|&i| self.catalogs.get(usize::from(i)))
            .map(Catalog::direction)
            .find(|d| *d != Direction::Auto)
            .map_or(0, |d| u8::from(d == Direction::Rtl));
        self.order = order;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n\
        [messages]\nhello = Hello, {name}!\nonly-en = English only\n\
        books.one = {count} book\nbooks.other = {count} books\n";
    const DE: &str = "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n\
        [messages]\nhello = Hallo, {name}!\nbooks.one = {count} Buch\nbooks.other = {count} Bücher\n";
    const DE_AT: &str = "[meta]\nlanguage = de-AT\nname = Deutsch (Österreich)\n\n\
        [messages]\nhello = Servus, {name}!\n";
    const DE_OVERRIDE: &str = "[meta]\nlanguage = de\n[messages]\nhello = Moin, {name}!\n";

    #[test]
    fn the_direction_and_more_come_from_the_catalog() {
        let ar = Catalog::parse(
            "[meta]\nlanguage = ar\nplural = arabic\ndirection = rtl\nscript = Arab\n\
             digits = arab\ncalendar = islamic\n[messages]\nhello = !\n",
        )
        .unwrap();
        assert_eq!(ar.direction(), Direction::Rtl);
        assert_eq!(ar.script(), Some("Arab"));
        assert_eq!(
            ar.digits(),
            ['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩']
        );
        assert_eq!(ar.calendar(), "islamic");
        assert_eq!(ar.plural_rule(), PluralRule::Cldr(0));
        let mut t = Translator::new();
        t.add(Catalog::parse(EN).unwrap());
        t.add(ar);
        assert_eq!(t.direction(), Direction::Ltr);
        t.set_language("ar");
        assert_eq!(t.direction(), Direction::Rtl);
        t.set_language("en");
        assert_eq!(t.direction(), Direction::Ltr);
        // What this release does not know is left as not said.
        let newer = Catalog::parse(
            "[meta]\nlanguage = xx\ndirection = ttb\nscript = Zzzz9\ndigits = new\ncalendar = new\n",
        )
        .unwrap();
        assert_eq!(newer.direction(), Direction::Auto);
        assert_eq!(
            (newer.script(), newer.digits(), newer.calendar()),
            (
                None,
                ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'],
                "gregorian"
            )
        );
    }

    #[test]
    fn digits_are_named_or_given() {
        let digits = |value: &str| {
            Catalog::parse(alloc::format!("[meta]\nlanguage = xx\ndigits = {value}\n"))
                .unwrap()
                .digits()
                .iter()
                .collect::<String>()
        };
        assert_eq!(digits("arabext"), "۰۱۲۳۴۵۶۷۸۹");
        assert_eq!(digits("hanidec"), "〇一二三四五六七八九");
        assert_eq!(digits("adlm"), "𞥐𞥑𞥒𞥓𞥔𞥕𞥖𞥗𞥘𞥙");
        // Any system by its ten digits, escaped or not.
        assert_eq!(digits("០១២៣៤៥៦៧៨៩"), "០១២៣៤៥៦៧៨៩");
        assert_eq!(digits("\"\\u{17e0}១២៣៤៥៦៧៨៩\""), "០១២៣៤៥៦៧៨៩");
        // Not ten, or a name this release does not know: Latin.
        assert_eq!(digits("០១២"), "0123456789");
        assert_eq!(digits("klingon"), "0123456789");
        // The digits of every system named are in a row, 0 to 9.
        for name in digit_systems() {
            let found: Vec<u32> = digits(name).chars().map(u32::from).collect();
            assert_eq!(found.len(), 10, "{name}");
            if name != "hanidec" {
                assert!(found.windows(2).all(|w| w[1] == w[0] + 1), "{name}");
            }
        }
        // A parked catalog (a language not in use) keeps them.
        let mut t = Translator::new();
        t.add(Catalog::parse(EN).unwrap());
        t.add(Catalog::parse("[meta]\nlanguage = km\ndigits = ០១២៣៤៥៦៧៨៩\n").unwrap());
        t.set_language("en");
        assert_eq!(t.catalogs()[1].digits()[1], '១');
    }

    fn translator(language: &str) -> Translator {
        let mut t = Translator::new();
        for source in [EN, DE, DE_AT] {
            t.add(Catalog::parse(source).unwrap());
        }
        t.set_language(language);
        t
    }

    #[test]
    fn falls_back_through_the_chain() {
        let t = translator("de-AT");
        assert_eq!(t.language(), "de-AT");
        assert_eq!(
            t.format("hello", &[("name", Arg::Str("Ada"))]),
            "Servus, Ada!"
        );
        assert_eq!(t.plural("books", 2, &[]), "2 Bücher");
        assert_eq!(t.tr("only-en"), "English only");
        assert_eq!(t.tr("missing.key"), "missing.key");
    }

    #[test]
    fn switches_language_at_runtime() {
        let mut t = translator("de");
        assert_eq!(t.plural("books", 1, &[]), "1 Buch");
        t.set_language("en");
        assert_eq!(t.plural("books", 1, &[]), "1 book");
        t.set_language("fr");
        assert_eq!(t.plural("books", 3, &[]), "3 books");
    }

    #[test]
    fn later_catalogs_override_earlier_ones() {
        let mut t = translator("de");
        t.add(Catalog::parse(DE_OVERRIDE).unwrap());
        assert_eq!(
            t.format("hello", &[("name", Arg::Str("Ada"))]),
            "Moin, Ada!"
        );
    }

    #[test]
    fn catalogs_of_other_languages_load_when_cloned() {
        let t = translator("de");
        let all = t.catalogs();
        let en = all.iter().find(|c| c.language() == "en").unwrap();
        assert!(en.keys().any(|k| k == "hello"));
        let at = all.iter().find(|c| c.language() == "de-AT").unwrap();
        assert!(at.is_empty());
        let mut at = at.clone();
        at.load();
        assert_eq!(at.get("hello"), Some("Servus, {name}!"));
    }

    /// Languages not in use are parked and come back when chosen; an
    /// English catalog the language chosen covers drops its index, one
    /// it does not cover keeps it.
    #[test]
    fn only_the_languages_in_use_are_kept() {
        const EN_ALL: &str = "[meta]\nlanguage = en\n[messages]\nall = All\n";
        const DE_ALL: &str = "[meta]\nlanguage = de\n[messages]\nall = Alle\n";
        let mut t = translator("de");
        t.add(Catalog::parse(EN_ALL).unwrap());
        t.add(Catalog::parse(DE_ALL).unwrap());
        t.shrink_to_fit();
        assert_eq!(t.parked.len(), 1, "de-AT");
        let loaded: Vec<(&str, bool)> = t
            .catalogs
            .iter()
            .map(|c| (c.language(), c.is_loaded()))
            .collect();
        assert_eq!(
            loaded,
            [("en", true), ("de", true), ("en", false), ("de", true)]
        );
        assert_eq!(t.tr("only-en"), "English only");
        assert_eq!(t.tr("all"), "Alle");
        t.set_language("de-AT");
        assert_eq!(t.tr("hello"), "Servus, {name}!");
        t.set_language("en");
        assert_eq!(t.parked.len(), 3);
        assert_eq!(t.tr("all"), "All");
        assert_eq!(t.tr("hello"), "Hello, {name}!");
    }

    /// A catalog whose index the program holds (made when it was built)
    /// keeps none on the heap; one whose source differs makes its own.
    #[test]
    fn indexes_kept_in_the_program_are_used() {
        use alloc::boxed::Box;
        let made = Catalog::parse(EN).unwrap();
        let index: &'static [u16] = Box::leak(made.narrow_index().unwrap().into());
        let fixed: &'static [(u64, &'static [u16])] =
            Box::leak(Box::new([(fingerprint(EN), index)]));
        let mut t = Translator::new();
        t.fixed_indexes(fixed);
        t.add(Catalog::parse(EN).unwrap());
        assert_eq!(t.indexes_on_heap(), 0);
        assert_eq!(t.tr("only-en"), "English only");
        t.add(Catalog::parse(DE_OVERRIDE).unwrap());
        t.set_language("de");
        assert!(t.indexes_on_heap() > 0, "the German one is its own");
        assert_eq!(
            t.format("hello", &[("name", Arg::Str("Ada"))]),
            "Moin, Ada!"
        );
    }

    #[test]
    fn lists_languages_once() {
        let mut t = translator("de");
        t.add(Catalog::parse(DE_OVERRIDE).unwrap());
        let tags: Vec<&str> = t.languages().into_iter().map(|(tag, _)| tag).collect();
        assert_eq!(tags, ["en", "de", "de-AT"]);
    }

    /// A catalog finds none of the keys before its first or after its
    /// last, but its first and last; later catalogs still win.
    #[test]
    fn keys_outside_a_catalog_are_found_elsewhere() {
        const HOME: &str = "[meta]\nlanguage = en\n[messages]\n\
            home.a = Home A\nhome.z = \"Home Z\"\nhome.books.one = {count} book\n";
        const CORE: &str = "[meta]\nlanguage = en\n[messages]\n\
            font-folder.x = Fonts\ncore.on = On\nhome.m = Core M\n";
        const LATER: &str = "[meta]\nlanguage = en\n[messages]\n\
            home.m = Later M\ncore.on = Yes\nhome.books.other = {count} books\n";
        let home = Catalog::parse(HOME).unwrap();
        assert_eq!(home.get("home.a"), Some("Home A"));
        assert_eq!(home.get("home.z"), Some("Home Z"));
        for outside in ["", "a", "home", "home.", "home.zz", "home.z.x", "z"] {
            assert_eq!(home.get(outside), None, "{outside}");
        }
        let mut t = Translator::new();
        for source in [CORE, HOME, LATER] {
            t.add(Catalog::parse(source).unwrap());
        }
        assert_eq!(t.tr("core.on"), "Yes");
        assert_eq!(t.tr("home.m"), "Later M");
        assert_eq!(t.tr("font-folder.x"), "Fonts");
        assert_eq!(t.tr("home.a"), "Home A");
        assert_eq!(t.tr("home.z"), "Home Z");
        assert_eq!(t.tr("home.b"), "home.b");
        assert_eq!(t.plural("home.books", 1, &[]), "1 books");
        assert_eq!(t.plural("home.books", 2, &[]), "2 books");
    }

    #[test]
    fn missing_plural_returns_key() {
        assert_eq!(translator("de").plural("nothing", 3, &[]), "nothing");
    }
}
