//! A sweep over every screen the app has (`paperpaw-dev sweep`): each one
//! shown once in English and in German on a device with buttons alone,
//! and looked at for what is plainly wrong. Fast, and strict: every
//! finding is an error and names its screen, and no screen is left out
//! by a list (but for the few that cannot be shown alone, each with its
//! reason: [`CANNOT_BE_SHOWN_ALONE`]).
//!
//! The screens are the registry's (`Kernel::known_screens`), those of
//! the plugins looked at. Each is shown as a reader gets to it, by the
//! way the buttons lead there from the home screen ([`reach::search`],
//! searched once a profile); one no way leads to is opened as a plugin
//! would open it: with the fixtures' parameters where they give its data
//! (`--network fixtures`), without parameters, with a book's path, and
//! with the radio on. Every screen is shown on a device of its own, as at
//! the start: what the way to one changed is not seen on the next. Then:
//!
//! - `sweep.not-shown`: it does not show: another screen does (it closed
//!   at once, or opened something else).
//! - `sweep.failed`: while it opened and was drawn a call of its plugin
//!   failed, or the kernel warned. One opened without what it needs says
//!   so this way: nothing of it was looked at.
//! - `sweep.blank`: it draws nothing.
//! - `sweep.overflow`: a part of it reaches past the screen, or text
//!   lies under a control or other text (the errors of `visual.off-screen`
//!   and `visual.overlap`).
//! - `sweep.text-cut`: text cut at the edge of its area without an
//!   ellipsis (the errors of `visual.text-cut`).
//! - `sweep.text`: a text is missing: a translation key is shown as it
//!   is, or German shows the English text of a key it lacks.
//! - `sweep.unreachable`: no way leads to it with the buttons alone.
//!   Only where every way there is was searched: where the search ended
//!   at its bounds (the whole app is larger than they are), the note on
//!   how a screen was shown (`sweep.shown`) says that no way to it was
//!   found, which is no proof that there is none.
//!
//! No screen is non-visual by its name, and none is left out: all are
//! shown and looked at. What is declared about a screen only says where
//! the buttons need not lead: to one shown while the device sleeps, and
//! to one a setting chooses in another's place (the sleep screen's
//! modes, a choice of `sleep.show`).
//!
//! Beside `visual`, `reach` and `lint`, which look closer at the plugins
//! being made (every size, look and language, the focus, the catalogs),
//! and `perf`, which measures: this one says in a minute whether every
//! screen of the app still comes up whole.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use paperpaw_core::sdk::builtin::params;
use paperpaw_core::sdk::semantics::Described;
use paperpaw_emulator::linked;
use paperpaw_emulator::profiles::Profile;
use paperpaw_native::host_glue as abi;
use paperpaw_platform::{Rect, buttons};

use crate::cases::{self, Shown};
use crate::cli::Context;
use crate::image::Image;
use crate::lint::catalogs::{self, Language};
use crate::reach::{self, Driver, Found, Limits};
use crate::report::{Finding, Report};
use crate::session::{self, Session, Start};
use crate::variant::Variant;
use crate::visual::{Looker, checks};

pub const NOT_SHOWN: &str = "sweep.not-shown";
pub const FAILED: &str = "sweep.failed";
pub const BLANK: &str = "sweep.blank";
pub const OVERFLOW: &str = "sweep.overflow";
pub const TEXT_CUT: &str = "sweep.text-cut";
pub const TEXT: &str = "sweep.text";
pub const UNREACHABLE: &str = "sweep.unreachable";

/// Screens that cannot be shown alone, with why: what they need comes from
/// outside the screen and the tools cannot give it (a question another
/// plugin asked, a radio the emulator lacks). Where one of these shows
/// nothing or a failed call, it is a note with the reason, not an error:
/// what is declared here is the reason, so it is checked that the reason
/// holds (the screen still fails or closes) and a screen that shows by
/// the way the buttons lead or with the fixtures never lands here.
const CANNOT_BE_SHOWN_ALONE: &[(&str, &str)] = &[
    (
        "keyboard.ask",
        "it shows the question another plugin asked (`core.choice`, `core.confirm`) and \
         opened without one its plugin refuses it: the request is not a parameter",
    ),
    (
        "vocabulary.sleep-card",
        "it is a mode of the sleep screen (`sleep.show`) that the sleep plugin binds to \
         the slot `core.sleep` and the device shows while it sleeps, with the card the \
         sleep chose: opened alone it has no card and every draw ends with ERR_STATE. \
         Binding the slot in the emulator (a settings change the sleep plugin answers) did \
         not make it show; the sleep screen's other modes are reached by the power button",
    ),
    (
        "bluetooth.search",
        "its plugin asks for a scan first and the emulator has no radio, which refuses \
         it (ERR_UNSUPPORTED): the screen closes with a notice, as on a device without one",
    ),
];

/// The languages looked at unless `--language` names others.
pub const LANGUAGES: [&str; 2] = ["en", "de"];

/// The profiles swept: those asked for with buttons and no touch screen;
/// only the X4 unless `--profile` named others.
fn profiles(context: &Context) -> Vec<&'static Profile> {
    let defaults = context
        .profiles
        .iter()
        .map(|p| p.device.id)
        .eq(crate::variant::DEFAULT_PROFILES.iter().copied());
    context
        .profiles
        .iter()
        .copied()
        .filter(|p| !p.device.input.touch && p.buttons.iter().any(|b| *b != buttons::POWER))
        .filter(|p| !defaults || p.device.id == "xteink-x4")
        .collect()
}

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    let profiles = profiles(context);
    if profiles.is_empty() {
        return Err(String::from(
            "sweep: none of the profiles asked for has buttons and no touch screen",
        ));
    }
    if !context.in_memory {
        // Pictures of an earlier run would mislead.
        let _ = std::fs::remove_dir_all(context.path("sweep"));
    }
    let languages: Vec<String> = context
        .languages
        .clone()
        .unwrap_or_else(|| LANGUAGES.iter().map(ToString::to_string).collect());
    for profile in profiles {
        let english = Variant::of(profile);
        eprintln!(
            "paperpaw-dev: sweep on {}: the ways the buttons lead",
            profile.device.id
        );
        let mut driver = Driver::new(context, &english)?;
        let screens = cases::screens(&driver.session, &context.target);
        let wanted: HashSet<String> = screens.iter().cloned().collect();
        let started = paperpaw_emulator::host::micros();
        let found = reach::search(&mut driver, &wanted, &Limits::default());
        eprintln!(
            "paperpaw-dev: sweep: {} of {} screens found in {:.0} s ({} searched{})",
            found.ways.keys().filter(|id| wanted.contains(*id)).count(),
            screens.len(),
            (paperpaw_emulator::host::micros() - started) as f32 / 1e6,
            found.views,
            if found.complete {
                ", all there were"
            } else {
                ""
            }
        );
        let asleep = std::mem::take(&mut driver.asleep);
        drop(driver);
        for language in &languages {
            let variant = Variant {
                language: language.clone(),
                ..Variant::of(profile)
            };
            eprintln!("paperpaw-dev: sweep on {}", variant.name());
            let mut sweep = Sweep::start(context, &variant, &found, &asleep)?;
            for id in &screens {
                for finding in sweep.screen(id)? {
                    report.add(finding);
                }
            }
            report.add(
                Finding::info(
                    "sweep.done",
                    "screens",
                    format!(
                        "{} screens looked at: {} as the buttons lead to them, {} opened \
                         directly ({} screens and their parts searched for the ways).",
                        screens.len(),
                        sweep.reached,
                        screens.len() - sweep.reached,
                        found.views
                    ),
                )
                .variant(variant.name()),
            );
        }
    }
    Ok(())
}

/// The parameters a screen is opened with.
type Given<'a> = &'a [(&'a str, &'a str)];

/// A screen as the sweep showed it.
struct Showed {
    shown: Shown,
    /// How it was opened, for the findings.
    how: String,
    /// The buttons led to it.
    reached: bool,
    /// What the device showed before.
    before: Image,
    /// What it described itself: without the screens below an overlay
    /// and the row of hints.
    own: Vec<Described>,
    /// The calls of plugins that failed meanwhile, and the kernel's
    /// warnings, each said once.
    failed: Vec<String>,
}

/// The name of a status of the plugin interface.
fn status_name(status: i32) -> String {
    let name = match status {
        abi::ERR_UNSUPPORTED => "ERR_UNSUPPORTED",
        abi::ERR_INVALID => "ERR_INVALID",
        abi::ERR_NO_SPACE => "ERR_NO_SPACE",
        abi::ERR_NO_MEMORY => "ERR_NO_MEMORY",
        abi::ERR_UTF8 => "ERR_UTF8",
        abi::ERR_STATE => "ERR_STATE",
        abi::ERR_CANCELLED => "ERR_CANCELLED",
        abi::ERR_IO => "ERR_IO",
        abi::ERR_BUSY => "ERR_BUSY",
        abi::ERR_NOT_FOUND => "ERR_NOT_FOUND",
        abi::ERR_FULL => "ERR_FULL",
        abi::ERR_DENIED => "ERR_DENIED",
        abi::ERR_TIMEOUT => "ERR_TIMEOUT",
        _ => return status.to_string(),
    };
    format!("{name} ({status})")
}

/// Forgets what failed before a screen is opened.
fn forget(session: &mut Session) {
    drop(linked::take_reports());
    session.device.warnings.clear();
}

/// What `session` shows now, asked for as `id`, with what failed since
/// [`forget`].
fn take(session: &mut Session, id: &str, how: String, reached: bool, before: Image) -> Showed {
    let shown = Shown {
        id: id.to_string(),
        shown: session.screen(),
        image: session.image(),
        parts: session.parts(),
        described: session.described(),
    };
    let own = session.device.kernel.described_active();
    let mut failed: Vec<String> = Vec::new();
    let said = linked::take_reports()
        .into_iter()
        .map(|r| {
            format!(
                "call {} of the plugin {} ended with {}",
                r.tag,
                r.plugin,
                status_name(r.status)
            )
        })
        .chain(
            session
                .device
                .warnings
                .drain(..)
                .map(|w| format!("the kernel warned: {w}")),
        );
    for line in said {
        if !failed.contains(&line) {
            failed.push(line);
        }
    }
    Showed {
        shown,
        how,
        reached,
        before,
        own,
        failed,
    }
}

/// Whether `area` of `image` is of one colour.
fn plain(image: &Image, area: Rect) -> bool {
    let crop = image.crop(area);
    let first = crop.pixel(0, 0);
    crop.rgb.as_chunks::<3>().0.iter().all(|p| *p == first)
}

/// The words of a label that may be translation keys: without the
/// punctuation around them, and not one shortened to fit ("…").
fn words(label: &str) -> impl Iterator<Item = &str> {
    label
        .split_whitespace()
        .filter(|word| !word.contains('…'))
        .map(|word| word.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
}

/// The texts of the app, to tell what a screen shows for want of one.
struct Texts {
    /// The language looked at.
    language: Language,
    /// English, where another language is looked at: text to the keys
    /// that have it.
    english: Option<BTreeMap<String, Vec<String>>>,
    /// The plugins of the app, whose keys are looked for.
    plugins: Vec<String>,
    /// Ids the app has, shown as they are at times.
    ids: Vec<String>,
}

impl Texts {
    fn of(session: &Session, code: &str) -> Result<Texts, String> {
        let kernel = &session.device.kernel;
        let languages = crate::lint::languages(kernel);
        let find = |code: &str| languages.iter().find(|l| l.code == code).cloned();
        let language =
            find(code).ok_or_else(|| format!("sweep: the app has no language `{code}`"))?;
        let english = (code != paperpaw_i18n::FALLBACK_LANGUAGE)
            .then(|| find(paperpaw_i18n::FALLBACK_LANGUAGE))
            .flatten()
            .map(|english| {
                let mut by_text: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for (key, text) in english.messages {
                    by_text.entry(text).or_default().push(key);
                }
                by_text
            });
        Ok(Texts {
            language,
            english,
            plugins: kernel
                .plugins()
                .iter()
                .map(|p| p.manifest.id.to_string())
                .collect(),
            ids: crate::lint::known_ids(session),
        })
    }

    /// What `drawn` shows for want of a text, each said once.
    fn missing(&self, drawn: &[Described]) -> Vec<String> {
        let mut missing: BTreeSet<String> = BTreeSet::new();
        for d in drawn {
            let label = d.label.trim();
            for word in words(label) {
                if catalogs::well_formed(word)
                    && self
                        .plugins
                        .iter()
                        .any(|p| p == crate::lint::namespace(word))
                    && !self.ids.iter().any(|id| id == word)
                    && self.is_key(word)
                {
                    missing.insert(format!(
                        "the key `{word}` is shown as it is: give it a text in the plugin's \
                         `i18n/en.ini` (and the other languages)"
                    ));
                }
            }
            if let Some(keys) = self.english.as_ref().and_then(|english| english.get(label))
                && label.chars().any(char::is_alphabetic)
                && keys.iter().all(|key| !self.language.has(key))
                && !self.language.messages.values().any(|text| text == label)
            {
                missing.insert(format!(
                    "\"{label}\" is shown in English: `i18n/{}.ini` has no text for `{}`",
                    self.language.code,
                    keys.join("`, `")
                ));
            }
        }
        missing.into_iter().collect()
    }

    /// Whether `word`, of the form of a key and in a plugin's namespace,
    /// is one: a key the catalogs have (shown without being looked up),
    /// or one of three words or more beside keys they have (`a.b.c` where
    /// there is an `a.b.d`). An address such as `news.example.org` or a
    /// file such as `news.txt` is none.
    fn is_key(&self, word: &str) -> bool {
        if self.language.has(word) {
            return true;
        }
        let Some((group, _)) = word.rsplit_once('.') else {
            return false;
        };
        group.contains('.')
            && self
                .language
                .messages
                .keys()
                .any(|key| key.rsplit_once('.').is_some_and(|(g, _)| g == group))
    }
}

/// One variant being swept.
struct Sweep<'a> {
    context: &'a Context,
    variant: Variant,
    found: &'a Found,
    asleep: &'a HashSet<String>,
    /// The device of the screen being looked at.
    driver: Driver<'a>,
    /// The home screen.
    home: String,
    looker: Looker,
    texts: Texts,
    /// Screens shown as the buttons lead to them.
    reached: usize,
}

impl<'a> Sweep<'a> {
    fn start(
        context: &'a Context,
        variant: &Variant,
        found: &'a Found,
        asleep: &'a HashSet<String>,
    ) -> Result<Self, String> {
        let mut driver = Driver::new(context, variant)?;
        let home = driver.session.screen().unwrap_or_default();
        let looker = Looker::new(&mut driver.session);
        let texts = Texts::of(&driver.session, &variant.language)?;
        Ok(Sweep {
            context,
            variant: variant.clone(),
            found,
            asleep,
            driver,
            home,
            looker,
            texts,
            reached: 0,
        })
    }

    /// A device as at the start, its radio on if asked: what was done to
    /// show the screen before (rows pressed on the way, settings changed
    /// so) is left behind.
    fn fresh(&mut self, radio: bool) -> Result<(), String> {
        let start = Start {
            // Notices gone before a screen is looked at, as `visual` has it.
            settings: vec![("core.notice-seconds".into(), "1".into())],
            wifi_online: radio,
            ..Start::default()
        };
        let session = Session::start(self.context, &self.variant, &start)?;
        self.driver = Driver::with(self.context, session);
        Ok(())
    }

    /// Shows `id` on a fresh device: by the way the buttons lead to it,
    /// else opened directly, the first way it shows without a failed
    /// call; else the first it shows at all; else what showed instead.
    fn show(&mut self, id: &str) -> Result<Showed, String> {
        if let Some((way, _)) = self.found.ways.get(id) {
            self.fresh(false)?;
            let before = self.driver.session.image();
            forget(&mut self.driver.session);
            if self
                .driver
                .go(way, None)
                .is_some_and(|seen| seen.top() == id)
            {
                let how = if way.is_empty() {
                    String::from("shown at the start")
                } else {
                    format!("reached with {}", way.join(", "))
                };
                return Ok(take(&mut self.driver.session, id, how, true, before));
            }
        }
        let book = [(params::PATH, session::BOOK)];
        let fixtures = cases::params(&self.driver.session, id);
        let mut tries: Vec<(&str, Given<'_>, bool)> = Vec::new();
        if !fixtures.is_empty() {
            tries.push(("with the fixtures' parameters", fixtures, false));
        }
        tries.push(("without parameters", &[], false));
        tries.push(("with a book's path", &book, false));
        tries.push(("without parameters, the radio on", &[], true));
        let mut second: Option<Showed> = None;
        for (how, given, radio) in tries {
            self.fresh(radio)?;
            let session = &mut self.driver.session;
            let before = session.image();
            forget(session);
            session.open(id, given)?;
            let showed = take(session, id, format!("opened {how}"), false, before);
            if showed.shown.opened() && showed.failed.is_empty() {
                return Ok(showed);
            }
            let better = match &second {
                None => true,
                Some(kept) => !kept.shown.opened() && showed.shown.opened(),
            };
            if better {
                second = Some(showed);
            }
        }
        second.ok_or_else(|| format!("sweep: {id} was not tried"))
    }

    /// Shows `id` and says what is wrong with it; first, as a note, how
    /// it was shown.
    fn screen(&mut self, id: &str) -> Result<Vec<Finding>, String> {
        let showed = self.show(id)?;
        let variant = self.variant.name();
        let plugin = self.context.target.owner(id).map_or_else(
            || crate::lint::namespace(id).to_string(),
            ToString::to_string,
        );
        let picture = self.picture(id, &showed.shown.image);
        let not_found = if showed.reached || self.found.complete {
            String::new()
        } else {
            format!(
                " The search for the ways the buttons lead ended at its bounds ({} screens \
                 and their parts searched) without one to it: whether one leads there is \
                 not known.",
                self.found.views
            )
        };
        let mut findings = vec![Finding::info(
            "sweep.shown",
            id,
            format!("It was {}.{not_found}", showed.how),
        )];
        if showed.reached {
            self.reached += 1;
        }
        let declared = CANNOT_BE_SHOWN_ALONE
            .iter()
            .find(|(screen, _)| *screen == id)
            .map(|(_, why)| *why);
        for (check, message, area) in self.wrong(id, &showed) {
            if let Some(why) = declared
                && matches!(check, NOT_SHOWN | FAILED | BLANK)
            {
                let note = format!("It cannot be shown alone: {why}.");
                if !findings.iter().any(|f| f.message == note) {
                    findings.push(Finding::info("sweep.declared", id, note));
                }
                continue;
            }
            let mut finding = Finding::error(check, id, message);
            if let Some(picture) = &picture {
                finding = finding.picture(picture.clone());
                if let Some(area) = area {
                    finding = finding.area(area);
                }
            }
            findings.push(finding);
        }
        Ok(findings
            .into_iter()
            .map(|f| f.plugin(plugin.clone()).variant(variant.clone()))
            .collect())
    }

    /// What is wrong with `id` as it was shown: the check, what to say
    /// and where in the picture.
    fn wrong(&self, id: &str, showed: &Showed) -> Vec<(&'static str, String, Option<Rect>)> {
        let mut wrong = Vec::new();
        if !showed.reached
            && self.found.complete
            && let Some(why) = self.unreachable(id)
        {
            wrong.push((UNREACHABLE, why, None));
        }
        if !showed.shown.opened() {
            wrong.push((
                NOT_SHOWN,
                format!(
                    "It does not show: opened without parameters, with a book's path and with \
                     the radio on, {} showed instead (it closed at once, or opened something \
                     else). Nothing of it was looked at.",
                    showed.shown.shown.as_deref().unwrap_or("nothing")
                ),
                None,
            ));
            return wrong;
        }
        let area = self.looker.screen;
        let image = &showed.shown.image;
        let blank = plain(image, area)
            || (id != self.home && showed.before.crop(area).differing(&image.crop(area), 0) == 0);
        if !showed.failed.is_empty() {
            let unseen = if blank {
                " It shows nothing so: nothing of it was looked at. If it needs parameters, \
                 give it a way in with the buttons in the sample library, or parameters with \
                 the fixtures (SCREENS in shells/desktop/src/offline.rs)."
            } else {
                ""
            };
            wrong.push((
                FAILED,
                format!(
                    "While it was {} and drawn, {}.{unseen}",
                    showed.how,
                    showed.failed.join("; ")
                ),
                None,
            ));
        } else if blank {
            wrong.push((BLANK, format!("It draws nothing ({}).", showed.how), None));
        }
        if blank {
            return wrong;
        }
        for issue in self.looker.look(&showed.shown, showed.own.clone()) {
            if issue.level != crate::report::Level::Error {
                continue;
            }
            let check = match issue.check {
                checks::TEXT_CUT => TEXT_CUT,
                checks::OFF_SCREEN | checks::OVERLAP => OVERFLOW,
                _ => continue,
            };
            wrong.push((check, issue.message, Some(issue.area)));
        }
        for missing in self.texts.missing(&showed.own) {
            wrong.push((TEXT, missing, None));
        }
        wrong
    }

    /// Why it is an error that no way leads to `id` with the buttons
    /// (every way there is was searched); `None` where the buttons need
    /// not lead to it: the sleeping device shows it, or a setting chooses
    /// it in another screen's place.
    fn unreachable(&self, id: &str) -> Option<String> {
        if self.asleep.contains(id) || reach::chosen_by(&self.driver.session, id).is_some() {
            return None;
        }
        Some(format!(
            "No way leads to it from the home screen with the buttons alone (all {} screens \
             there are with their parts searched): give it a way in, such as a menu entry \
             or a row of another screen.",
            self.found.views
        ))
    }

    /// Saves the picture of `id`; its path in the output folder. `None`
    /// where nothing is written ([`Context::in_memory`]).
    fn picture(&self, id: &str, image: &Image) -> Option<PathBuf> {
        if self.context.in_memory {
            return None;
        }
        let picture = PathBuf::from("sweep")
            .join(self.variant.slug())
            .join(format!("{id}.png"));
        image.save_png(&self.context.out.join(&picture)).ok()?;
        Some(picture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_core::sdk::semantics::Role;

    fn language(code: &str, messages: &[(&str, &str)]) -> Language {
        Language {
            code: code.to_string(),
            name: code.to_string(),
            plural: paperpaw_i18n::PluralRule::OneOther,
            messages: messages
                .iter()
                .map(|(key, text)| (key.to_string(), text.to_string()))
                .collect(),
        }
    }

    fn texts(language: Language, english: Option<&Language>) -> Texts {
        Texts {
            language,
            english: english.map(|english| {
                let mut by_text: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for (key, text) in &english.messages {
                    by_text.entry(text.clone()).or_default().push(key.clone());
                }
                by_text
            }),
            plugins: vec!["news".into(), "stats".into()],
            ids: vec!["news.list".into()],
        }
    }

    fn drawn(labels: &[&str]) -> Vec<Described> {
        labels
            .iter()
            .map(|label| Described::new(Role::Text, label.to_string(), Rect::new(0, 0, 10, 10)))
            .collect()
    }

    const ENGLISH: &[(&str, &str)] = &[
        ("news.empty.title", "No feeds yet"),
        ("news.add", "Add a feed"),
        ("stats.widget.today", "Read today"),
        ("news.ok", "OK"),
    ];

    #[test]
    fn keys_shown_are_told_from_ids_addresses_and_files() {
        let texts = texts(language("en", ENGLISH), None);
        let missing = texts.missing(&drawn(&[
            // A key the catalogs have, shown without being looked up.
            "stats.widget.today",
            // A key no catalog has, beside one they have.
            "(news.empty.note)",
            // An id, an address, a file, a version, a shortened word.
            "news.list",
            "news.example.org",
            "news.txt",
            "1.2.0",
            "news.empty.ti…",
            "No feeds yet",
        ]));
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing[0].contains("`news.empty.note`"));
        assert!(missing[1].contains("`stats.widget.today`"));
    }

    #[test]
    fn english_shown_for_a_key_german_lacks_is_missing() {
        let english = language("en", ENGLISH);
        let german = language(
            "de",
            &[("news.empty.title", "Noch keine Feeds"), ("news.ok", "OK")],
        );
        let texts = texts(german, Some(&english));
        let missing = texts.missing(&drawn(&[
            "Noch keine Feeds",
            // German lacks `news.add`: its English text shows.
            "Add a feed",
            // The same in both languages.
            "OK",
            // No text of the catalogs: a book's title, say.
            "Alice in Wonderland",
        ]));
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("\"Add a feed\"") && missing[0].contains("`news.add`"));
    }

    #[test]
    fn an_area_of_one_colour_is_plain() {
        let mut image = Image::filled(20, 20, [255, 255, 255]);
        assert!(plain(&image, Rect::new(0, 0, 20, 20)));
        image.set_pixel(15, 15, [0, 0, 0]);
        assert!(!plain(&image, Rect::new(0, 0, 20, 20)));
        // The row of hints below the screen's area is not looked at.
        assert!(plain(&image, Rect::new(0, 0, 20, 10)));
    }

    #[test]
    fn statuses_are_named() {
        assert_eq!(status_name(-2), "ERR_INVALID (-2)");
        assert_eq!(status_name(-99), "-99");
    }
}
