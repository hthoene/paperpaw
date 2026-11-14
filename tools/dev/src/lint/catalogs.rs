//! Translations: a plugin's messages in each language compared with
//! English (missing, left over, placeholders that differ, plural forms
//! missing), and the keys of its catalog files (in its namespace, well
//! formed).

use std::collections::{BTreeMap, BTreeSet};

use paperpaw_i18n::check::{self, Issue};
use paperpaw_i18n::{Catalog, FALLBACK_LANGUAGE, PluralCategory, PluralRule};

use crate::report::{Finding, Level};

/// The messages of one language, from all its catalogs.
#[derive(Debug, Clone)]
pub struct Language {
    pub code: String,
    /// Its name in itself, e.g. `Deutsch`.
    pub name: String,
    pub plural: PluralRule,
    pub messages: BTreeMap<String, String>,
}

impl Language {
    /// The keys in `namespace`.
    pub fn keys_of<'a>(&'a self, namespace: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.messages
            .keys()
            .map(String::as_str)
            .filter(move |key| super::namespace(key) == namespace)
    }

    pub fn has(&self, key: &str) -> bool {
        self.messages.contains_key(key)
    }
}

/// Every language of `catalogs`, its messages merged (later catalogs
/// win, as in the translator), English first.
pub fn merge<'a>(catalogs: impl IntoIterator<Item = &'a Catalog>) -> Vec<Language> {
    let mut languages: Vec<Language> = Vec::new();
    for catalog in catalogs {
        let mut catalog = catalog.clone();
        catalog.load();
        let code = catalog.language().to_string();
        let index = match languages.iter().position(|l| l.code == code) {
            Some(index) => index,
            None => {
                languages.push(Language {
                    code: code.clone(),
                    name: catalog.name().to_string(),
                    plural: catalog.plural_rule(),
                    messages: BTreeMap::new(),
                });
                languages.len() - 1
            }
        };
        let language = &mut languages[index];
        for key in catalog.keys() {
            if let Some(value) = catalog.get(key) {
                language.messages.insert(key.to_string(), value.to_string());
            }
        }
    }
    languages.sort_by_key(|l| (l.code != FALLBACK_LANGUAGE, l.code.clone()));
    languages
}

/// A catalog of `language` with only the messages in `namespace`, for
/// [`check::compare`].
fn part(language: &Language, namespace: &str) -> Catalog {
    let mut source = format!(
        "[meta]\nlanguage = {}\nplural = {}\n\n[messages]\n",
        language.code,
        language.plural.id()
    );
    for key in language.keys_of(namespace) {
        let value = &language.messages[key];
        source.push_str(&format!("{key} = {}\n", paperpaw_config::quote(value)));
    }
    // Made of what catalogs parsed already: it parses.
    Catalog::parse(source).unwrap_or_else(|_| {
        Catalog::parse(format!("[meta]\nlanguage = {}\n", language.code))
            .unwrap_or_else(|_| unreachable!("a catalog with only a language parses"))
    })
}

/// The plural categories a rule uses.
fn categories(rule: PluralRule) -> Vec<PluralCategory> {
    let mut used: Vec<PluralCategory> = Vec::new();
    for n in 0..=1000 {
        let category = rule.category(n);
        if !used.contains(&category) {
            used.push(category);
        }
    }
    if !used.contains(&PluralCategory::Other) {
        used.push(PluralCategory::Other);
    }
    used
}

const CATEGORIES: [PluralCategory; 6] = [
    PluralCategory::Zero,
    PluralCategory::One,
    PluralCategory::Two,
    PluralCategory::Few,
    PluralCategory::Many,
    PluralCategory::Other,
];

/// The base of a plural message's key (`books` of `books.other`).
fn plural_base(key: &str) -> Option<&str> {
    let (base, suffix) = key.rsplit_once('.')?;
    CATEGORIES.iter().any(|c| c.id() == suffix).then_some(base)
}

/// Keys translations may have that English does not: the plugin's name
/// and description, which English takes from its manifest.
fn english_elsewhere(plugin: &str, key: &str) -> bool {
    key == format!("{plugin}.plugin.name") || key == format!("{plugin}.plugin.description")
}

/// A list of keys for a message: the first few and how many more.
fn listed(keys: &[String]) -> String {
    const SHOWN: usize = 8;
    let mut text: Vec<String> = keys.iter().take(SHOWN).map(|k| format!("`{k}`")).collect();
    if keys.len() > SHOWN {
        text.push(format!("and {} more", keys.len() - SHOWN));
    }
    text.join(", ")
}

/// What the translations of `plugin` lack or have too many of, in each
/// of `languages` (English first; the others compared with it), at
/// places `place` names for a language. PaperPaw's own plugins (`own`)
/// must be complete in every language the app has.
pub fn translations(
    plugin: &str,
    own: bool,
    languages: &[Language],
    place: &dyn Fn(&str) -> String,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(english) = languages.iter().find(|l| l.code == FALLBACK_LANGUAGE) else {
        return findings;
    };
    let ours = |language: &Language| language.keys_of(plugin).count() > 0;
    if !languages.iter().any(ours) {
        return findings;
    }
    let reference = part(english, plugin);
    let missing_level = if own { Level::Error } else { Level::Warning };
    findings.extend(plurals(plugin, english, english, &place(&english.code)));
    for language in languages.iter().filter(|l| l.code != FALLBACK_LANGUAGE) {
        let at = place(&language.code);
        if !ours(language) {
            if reference.is_empty() {
                continue;
            }
            let message = format!(
                "There is no {} translation: English is shown instead.",
                language.name
            );
            findings.push(match own {
                true => Finding::error("lint.translation-missing", at, message),
                false => Finding::info("lint.translation-missing", at, message),
            });
            continue;
        }
        let translation = part(language, plugin);
        let (mut missing, mut unknown) = (Vec::new(), Vec::new());
        for issue in check::compare(&reference, &translation) {
            match issue {
                Issue::Missing(key) => missing.push(key),
                Issue::Unknown(key) if !english_elsewhere(plugin, &key) => unknown.push(key),
                Issue::Unknown(_) => {}
                Issue::Placeholders(key) => findings.push(Finding::error(
                    "lint.placeholders",
                    at.clone(),
                    format!(
                        "`{key}` has other placeholders than in English: \
                         `{}` against `{}`.",
                        translation.get(&key).unwrap_or_default(),
                        english
                            .messages
                            .get(&key)
                            .or_else(|| plural_base(&key)
                                .and_then(|b| english.messages.get(&format!("{b}.other"))))
                            .map_or("", String::as_str)
                    ),
                )),
            }
        }
        if !missing.is_empty() {
            findings.push(Finding::new(
                "lint.translation-missing",
                missing_level,
                at.clone(),
                format!(
                    "{} {} English has {} missing: {}.",
                    missing.len(),
                    if missing.len() == 1 {
                        "message"
                    } else {
                        "messages"
                    },
                    if missing.len() == 1 { "is" } else { "are" },
                    listed(&missing)
                ),
            ));
        }
        if !unknown.is_empty() {
            findings.push(Finding::warning(
                "lint.translation-unused",
                at.clone(),
                format!(
                    "{} {} English does not have, left over or misspelt: {}.",
                    unknown.len(),
                    if unknown.len() == 1 {
                        "message"
                    } else {
                        "messages"
                    },
                    listed(&unknown)
                ),
            ));
        }
        findings.extend(plurals(plugin, language, english, &at));
    }
    findings
        .into_iter()
        .map(|f| f.plugin(plugin.to_string()))
        .collect()
}

/// Plural messages of `plugin` in `language` lacking a form its plural
/// rule needs.
fn plurals(plugin: &str, language: &Language, english: &Language, at: &str) -> Vec<Finding> {
    let mut bases: BTreeSet<&str> = BTreeSet::new();
    for key in language.keys_of(plugin) {
        // A plural message has `other` in English and another form;
        // `x.one` and `x.two` alone are plain messages named so.
        if let Some(base) = plural_base(key)
            && english.has(&format!("{base}.other"))
            && CATEGORIES
                .iter()
                .filter(|c| english.has(&format!("{base}.{}", c.id())))
                .count()
                > 1
        {
            bases.insert(base);
        }
    }
    let needed = categories(language.plural);
    let mut findings = Vec::new();
    for base in bases {
        let lacking: Vec<&str> = needed
            .iter()
            .filter(|c| !language.has(&format!("{base}.{}", c.id())))
            .map(|c| c.id())
            .collect();
        if !lacking.is_empty() {
            findings.push(
                Finding::warning(
                    "lint.plural",
                    at.to_string(),
                    format!(
                        "`{base}` lacks the plural {} `{}` that {} needs (rule `{}`).",
                        if lacking.len() == 1 { "form" } else { "forms" },
                        lacking.join("`, `"),
                        language.name,
                        language.plural.id()
                    ),
                )
                .plugin(plugin.to_string()),
            );
        }
    }
    findings
}

/// Whether `key` is well formed: lowercase words of letters, digits and
/// `-`, at least two, separated by dots.
pub fn well_formed(key: &str) -> bool {
    let segments: Vec<&str> = key.split('.').collect();
    segments.len() >= 2
        && segments.iter().all(|s| {
            !s.is_empty()
                && !s.starts_with('-')
                && !s.ends_with('-')
                && !s.contains("--")
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// The keys of one of `plugin`'s catalog files (`place`): well formed,
/// and in its namespace, or translating a message another plugin of the
/// app has in English (`translates`).
pub fn file_keys(
    plugin: &str,
    place: &str,
    catalog: &Catalog,
    translates: &dyn Fn(&str) -> bool,
) -> Vec<Finding> {
    let mut catalog = catalog.clone();
    catalog.load();
    let mut malformed = Vec::new();
    let mut outside = Vec::new();
    for key in catalog.keys() {
        if !well_formed(key) {
            malformed.push(key.to_string());
        }
        // A plugin's name and description may be translated where it is
        // not installed (by a language pack, by the kernel for one it
        // makes itself).
        let name = key.ends_with(".plugin.name") || key.ends_with(".plugin.description");
        if super::namespace(key) != plugin && !translates(key) && !name {
            outside.push(key.to_string());
        }
    }
    let mut findings = Vec::new();
    if !malformed.is_empty() {
        findings.push(Finding::warning(
            "lint.id",
            place,
            format!(
                "Keys not well formed (lowercase words of letters, digits and `-`, \
                 separated by dots): {}.",
                listed(&malformed)
            ),
        ));
    }
    if !outside.is_empty() {
        findings.push(Finding::warning(
            "lint.id",
            place,
            format!(
                "Keys outside the namespace `{plugin}` that no plugin has in English: {}; \
                 a plugin's keys start with its id.",
                listed(&outside)
            ),
        ));
    }
    findings
        .into_iter()
        .map(|f| f.plugin(plugin.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Catalog {
        let path = format!("{}/tests/fixtures/lint/{name}", env!("CARGO_MANIFEST_DIR"));
        Catalog::parse(std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    fn languages() -> Vec<Language> {
        let catalogs = [
            fixture("i18n/en.ini"),
            fixture("i18n/de.ini"),
            fixture("i18n/ru.ini"),
        ];
        merge(catalogs.iter())
    }

    fn checks(findings: &[Finding]) -> Vec<(&str, &str)> {
        findings
            .iter()
            .map(|f| (f.check, f.place.as_str()))
            .collect()
    }

    #[test]
    fn translations_are_compared_with_english() {
        let languages = languages();
        assert_eq!(languages[0].code, "en");
        let place = |code: &str| format!("i18n/{code}.ini");
        let findings = translations("demo", false, &languages, &place);
        let found = checks(&findings);
        assert!(
            found.contains(&("lint.translation-missing", "i18n/de.ini")),
            "{findings:#?}"
        );
        assert!(found.contains(&("lint.translation-unused", "i18n/de.ini")));
        assert!(found.contains(&("lint.placeholders", "i18n/de.ini")));
        // Russian needs `few` and `many`.
        assert!(
            found.contains(&("lint.plural", "i18n/ru.ini")),
            "{findings:#?}"
        );
        // Messages named `one` and `two` without `other` are no plurals.
        assert!(
            !findings
                .iter()
                .any(|f| f.check == "lint.plural" && f.message.contains("demo.ring")),
            "{findings:#?}"
        );
        let missing = findings
            .iter()
            .find(|f| f.check == "lint.translation-missing")
            .unwrap();
        assert!(
            missing.message.contains("`demo.only-english`"),
            "{}",
            missing.message
        );
        assert_eq!(missing.level, Level::Warning);
        // Its name comes from the manifest in English.
        let unused = findings
            .iter()
            .find(|f| f.check == "lint.translation-unused")
            .unwrap();
        assert!(unused.message.contains("`demo.left-over`"));
        assert!(!unused.message.contains("plugin.name"));
        assert!(findings.iter().all(|f| f.plugin.as_deref() == Some("demo")));
        // A plugin without texts has nothing to translate.
        assert!(translations("nothing", true, &languages, &place).is_empty());
    }

    #[test]
    fn own_plugins_must_be_complete() {
        let mut languages = languages();
        languages.push(Language {
            code: "fr".into(),
            name: "Français".into(),
            plural: PluralRule::ZeroOneOther,
            messages: BTreeMap::new(),
        });
        let place = |code: &str| format!("i18n/{code}.ini");
        let findings = translations("demo", true, &languages, &place);
        let french = findings.iter().find(|f| f.place == "i18n/fr.ini").unwrap();
        assert_eq!(french.level, Level::Error);
        assert!(
            findings
                .iter()
                .filter(|f| f.check == "lint.translation-missing")
                .all(|f| f.level == Level::Error)
        );
    }

    #[test]
    fn keys_of_files() {
        assert!(well_formed("demo.setting.face-2"));
        for bad in [
            "demo",
            "Demo.title",
            "demo..x",
            "demo.x_y",
            "demo.-x",
            "demo.a--b",
        ] {
            assert!(!well_formed(bad), "{bad}");
        }
        let languages = languages();
        let english = languages.iter().find(|l| l.code == "en").unwrap();
        // `other` is a plugin of the app, `stranger` none.
        let translates = |key: &str| key.starts_with("other.") && english.has(key);
        let findings = file_keys("demo", "i18n/de.ini", &fixture("i18n/de.ini"), &translates);
        assert_eq!(findings.len(), 2, "{findings:#?}");
        assert!(findings[0].message.contains("`demo.Bad_key`"));
        assert!(findings[1].message.contains("`stranger.title`"));
        // A message of another plugin English has is a translation of it.
        assert!(!findings[1].message.contains("other.title"));
        // A plugin's name may be translated anywhere.
        assert!(!findings[1].message.contains("stranger.plugin.name"));
    }
}
