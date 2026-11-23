//! The language changed while the setup guide is open (#505): everything it
//! shows after, its pages and the screens of its steps, is in the language
//! chosen. The guide keeps the steps it opened with, so a step its plugin
//! hides for a while (the store's, where everything it offers is on) is
//! looked at too: an English guide showed it as "Mehr hinzufügen".
//!
//! Walked on every device with buttons alone, from English to each other
//! language and back, as the app starts and with everything the store offers
//! on: forward through every page, each step's screen opened and left (its
//! plugin has looked then), the language changed on the last page, and every
//! page and screen looked at again on the way back. `lint.language-mixed`: a
//! text that only another language has is shown.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use paperpaw_core::sdk::semantics::Described;

use super::catalogs::Language;
use crate::cli::Context;
use crate::report::{Finding, Report};
use crate::session::{Session, Start};
use crate::variant::Variant;

pub const MIXED: &str = "lint.language-mixed";

/// The guide's screen.
const GUIDE: &str = "setup.guide";
/// More pages than a guide has: a walk that gets this far is lost.
const PAGES_AT_MOST: usize = 30;

/// Tells text that is plainly another language's: a message of another
/// language that the language shown has nowhere.
pub struct Foreign {
    shown: HashSet<String>,
    /// Each other language's code and its texts, each with its keys.
    others: Vec<(String, BTreeMap<String, Vec<String>>)>,
}

impl Foreign {
    pub fn new(shown: &Language, all: &[Language]) -> Foreign {
        let others = all
            .iter()
            .filter(|l| l.code != shown.code)
            .map(|l| {
                let mut by_text: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for (key, text) in &l.messages {
                    by_text.entry(text.clone()).or_default().push(key.clone());
                }
                (l.code.clone(), by_text)
            })
            .collect();
        Foreign {
            shown: shown.messages.values().cloned().collect(),
            others,
        }
    }

    /// The language `label` is of and its keys there, where it is plainly
    /// another's (a card's *›* aside).
    pub fn of(&self, label: &str) -> Option<(&str, &[String])> {
        let label = label.trim().trim_end_matches('›').trim_end();
        if !label.chars().any(char::is_alphabetic) || self.shown.contains(label) {
            return None;
        }
        self.others.iter().find_map(|(code, by_text)| {
            by_text
                .get(label)
                .map(|keys| (code.as_str(), keys.as_slice()))
        })
    }
}

/// Walks the guide with the language changed on its last page, on every
/// device with buttons alone, between English and each other language of
/// `languages`.
pub fn guide(context: &Context, languages: &[Language], report: &mut Report) -> Result<(), String> {
    let english = paperpaw_i18n::FALLBACK_LANGUAGE;
    let mut pairs: Vec<(&Language, &Language)> = Vec::new();
    for other in languages.iter().filter(|l| l.code != english) {
        if let Some(en) = languages.iter().find(|l| l.code == english) {
            pairs.extend([(en, other), (other, en)]);
        }
    }
    // The store hides its step where everything it offers is on.
    let offered: Vec<(String, String)> = paperpaw_plugin_store::native_ask::OFFERED
        .iter()
        .map(|id| (format!("plugins.{id}"), String::from("true")))
        .collect();
    for profile in context.profiles.iter().filter(|p| !p.device.input.touch) {
        for &(from, to) in &pairs {
            for settings in [Vec::new(), offered.clone()] {
                let variant = Variant {
                    language: from.code.clone(),
                    ..Variant::of(profile)
                };
                let start = Start {
                    setup: true,
                    settings,
                    ..Start::default()
                };
                let mut session = Session::start(context, &variant, &start)?;
                let foreign = Foreign::new(to, languages);
                let mut found = BTreeSet::new();
                walk(&mut session, from, to, &mut |place, drawn| {
                    for d in drawn {
                        if let Some((code, keys)) = foreign.of(&d.label) {
                            let label = d.label.trim().to_string();
                            found.insert((
                                place.to_string(),
                                label,
                                code.to_string(),
                                keys.to_vec(),
                            ));
                        }
                    }
                })
                .map_err(|e| format!("{}: {e}", variant.name()))?;
                for (place, label, code, keys) in found {
                    report.add(
                        Finding::error(
                            MIXED,
                            place,
                            format!(
                                "\"{label}\" is the `{code}` text of `{}`, shown in `{}` \
                                 after the language changed from `{}` while the guide was \
                                 open: what a screen keeps of its texts is read again when \
                                 `core.language` changes, all of it.",
                                keys.join("`, `"),
                                to.code,
                                from.code
                            ),
                        )
                        .plugin(super::namespace(keys.first().map_or("", String::as_str)))
                        .variant(variant.name()),
                    );
                }
            }
        }
    }
    Ok(())
}

/// The guide forward in `from`, each step's screen opened and left, the
/// language changed to `to` on its last page, and back to its first page:
/// what each page and screen shows then, by its place, to `look`.
fn walk(
    session: &mut Session,
    from: &Language,
    to: &Language,
    look: &mut dyn FnMut(&str, &[Described]),
) -> Result<(), String> {
    if session.screen().as_deref() != Some(GUIDE) {
        return Err(String::from("the setup guide did not show at the start"));
    }
    let done = from
        .messages
        .get("setup.done")
        .ok_or("the catalogs have no `setup.done`")?;
    let mut pages = 0;
    while !session.described().iter().any(|d| d.label.trim() == done) {
        if pages > 0 {
            enter(session)?;
        }
        session.script(if pages == 0 {
            "confirm,wait:1000"
        } else {
            "right,wait:300,confirm,wait:1500"
        })?;
        pages += 1;
        if session.screen().as_deref() != Some(GUIDE) || pages > PAGES_AT_MOST {
            return Err(format!(
                "the walk through the guide was lost on page {pages}"
            ));
        }
    }
    session.set("core.language", &to.code)?;
    let place = |page: usize, drawn: &[Described]| {
        let title = drawn.first().map_or("", |d| d.label.trim());
        format!("{GUIDE} page {page} \"{title}\"")
    };
    for page in (0..=pages).rev() {
        if page < pages {
            session.script("back,wait:1500")?;
        }
        let drawn = session.described();
        let at = place(page, &drawn);
        look(&at, &drawn);
        if page > 0 && page < pages {
            let (screen, drawn) = enter(session)?;
            if let Some(screen) = screen {
                look(&format!("{screen} (from {at})"), &drawn);
            }
        }
    }
    Ok(())
}

/// Opens what the page's card opens and leaves it with Back, one key, one
/// step (a screen, a list, a number being changed in place): the screen and
/// what it showed (none where the card changed in place).
fn enter(session: &mut Session) -> Result<(Option<String>, Vec<Described>), String> {
    session.script("left,wait:300,confirm,wait:1500")?;
    let screen = session.screen().filter(|s| s != GUIDE);
    let drawn = if screen.is_some() {
        session.described()
    } else {
        Vec::new()
    };
    for _ in 0..3 {
        session.script("back,wait:1500")?;
        if session.screen().as_deref() == Some(GUIDE) {
            return Ok((screen, drawn));
        }
    }
    Err(format!(
        "back from {} did not lead to the guide",
        screen.unwrap_or_default()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language(code: &str, messages: &[(&str, &str)]) -> Language {
        Language {
            code: code.into(),
            name: code.into(),
            plural: paperpaw_i18n::PluralRule::OneOther,
            messages: messages
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        }
    }

    #[test]
    fn another_languages_text_is_told_and_a_shared_one_is_not() {
        let en = language(
            "en",
            &[("store.offer.title", "Add more"), ("wifi.title", "Wi-Fi")],
        );
        let de = language(
            "de",
            &[
                ("store.offer.title", "Mehr hinzufügen"),
                ("wifi.title", "WLAN"),
                ("x.ok", "Wi-Fi"),
            ],
        );
        let all = [en.clone(), de.clone()];
        let in_english = Foreign::new(&en, &all);
        assert_eq!(
            in_english.of("Mehr hinzufügen"),
            Some(("de", &["store.offer.title".to_string()][..]))
        );
        assert_eq!(
            in_english.of(" WLAN ›"),
            Some(("de", &["wifi.title".to_string()][..]))
        );
        assert_eq!(in_english.of("Add more"), None);
        // German has it too: not plainly English.
        assert_eq!(Foreign::new(&de, &all).of("Wi-Fi"), None);
        assert_eq!(
            Foreign::new(&de, &all).of("Add more").map(|f| f.0),
            Some("en")
        );
        assert_eq!(in_english.of("12"), None);
    }
}
