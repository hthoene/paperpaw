//! Texts: translations missing or left over, a pseudo-language that finds
//! text too long for its place, settings without help texts, ids outside
//! the plugin's namespace, and dashes where the project forbids them.
//!
//! What a plugin registers and its catalogs are read from an English
//! session on the X4 (the kernel's translator has every catalog; a data
//! plugin's files are read as well). Then its screens and widgets are
//! drawn on a device with buttons and one with touch, in English and in
//! the pseudo-language ([`pseudo`]), and what is drawn compared.
//!
//! Checks: `lint.translation-missing`, `lint.translation-unused`,
//! `lint.placeholders`, `lint.plural`, `lint.missing-key`,
//! `lint.setting-help`, `lint.untranslated`, `lint.id`, `lint.catalog`,
//! `lint.unknown-setting`, `lint.dash`, `lint.register`,
//! `lint.pseudo-overflow`, `lint.hardcoded`, `lint.how-it-works` (a plugin
//! with code that says not, or hardly, how it works: `Manifest::how`).

pub(crate) mod catalogs;
mod drawn;
mod files;
mod groups;
pub mod pseudo;
mod texts;

use std::path::{Path, PathBuf};

use paperpaw_core::sdk::builtin::{fonts, settings};
use paperpaw_core::sdk::semantics::Described;
use paperpaw_core::sdk::{FontHandle, PluginState};
use paperpaw_emulator::profiles::Profile;
use paperpaw_graphics::Role;
use paperpaw_platform::Rect;

use crate::cases;
use crate::cli::Context;
use crate::report::{Finding, Report};
use crate::session::Session;
use crate::variant::Variant;

pub use catalogs::{Language, well_formed};
pub use drawn::Measure;

/// The pseudo-language's code (see [`pseudo_plugin`]): a data plugin may
/// add any language whose code is letters, digits and `-`.
pub const PSEUDO: &str = "pseudo";

/// The plugin an id belongs to by its first part: `home` of
/// `home.continue@1`.
pub fn namespace(id: &str) -> &str {
    id.split(['.', '@', ' ', '/']).next().unwrap_or(id)
}

/// This repository, where PaperPaw's own plugins are.
fn repository() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

/// The folder of the sources of one of PaperPaw's own plugins with code.
fn sources(plugin: &str) -> Option<PathBuf> {
    let root = repository();
    let folder = if plugin == "core" {
        root.join("crates/paperpaw-core")
    } else {
        root.join("plugins").join(plugin)
    };
    folder.join("Cargo.toml").is_file().then_some(folder)
}

/// Whether `plugin` is one of PaperPaw's own: its data folder is in this
/// repository, or it is one of the plugins with code here. They keep the
/// project's rules strictly (dashes, complete translations).
pub fn own(plugin: &str, folder: Option<&Path>) -> bool {
    match folder {
        Some(folder) => folder
            .canonicalize()
            .is_ok_and(|f| f.starts_with(repository())),
        None => sources(plugin).is_some(),
    }
}

/// The files of a data plugin that adds the pseudo-language, as `(path,
/// bytes)` in the library: put in by sessions whose language is
/// [`PSEUDO`]. Its catalogs hold every English text of the app, plugins'
/// names and descriptions too, made longer and accented (see
/// [`pseudo::pseudo`]).
pub fn pseudo_plugin(context: &Context) -> Result<Vec<(String, Vec<u8>)>, String> {
    pseudo::plugin(context)
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    // Pictures of an earlier run would mislead.
    if !context.in_memory {
        let _ = std::fs::remove_dir_all(context.path("lint"));
    }
    let session = Session::new(context, &Variant::x4())?;
    let kernel = &session.device.kernel;
    let all: Vec<String> = kernel
        .plugins()
        .iter()
        .map(|p| p.manifest.id.to_string())
        .filter(|id| !id.starts_with(crate::probe::PLUGIN))
        .collect();
    let looked: Vec<String> = if context.target.plugins.is_empty() {
        all.clone()
    } else {
        context.target.plugins.clone()
    };

    for info in kernel.plugins().iter() {
        let id = info.manifest.id.as_str();
        if !looked.iter().any(|p| p == id) {
            continue;
        }
        if info.state == PluginState::Failed {
            report.add(
                Finding::error(
                    "lint.register",
                    id,
                    "The plugin could not be registered, so nothing of it was looked at; \
                     the kernel's warnings (printed above) say why.",
                )
                .plugin(id),
            );
        }
        // A data plugin's manifest is a file, looked at with the others.
        if context.target.data.iter().all(|d| d.id != id) {
            if let Some(finding) = how_it_works(id, info.how(kernel.translator())) {
                report.add(finding);
            }
            let is_own = own(id, None);
            for (place, text) in [
                ("name", info.manifest.name.as_ref()),
                ("description", info.manifest.description.as_ref()),
            ] {
                for finding in files::dashes(id, &format!("manifest {place}"), text, is_own) {
                    report.add(finding);
                }
            }
        }
    }
    if let Some(missing) = looked.iter().find(|p| !all.contains(p)) {
        return Err(format!(
            "the app has no plugin `{missing}` (it has: {})",
            all.join(", ")
        ));
    }

    // Translations, compared with English.
    let wanted: Vec<String> = context
        .languages()
        .into_iter()
        .filter(|l| l != PSEUDO)
        .collect();
    let mut languages = catalogs::languages(kernel.translator());
    languages.retain(|l| l.code == paperpaw_i18n::FALLBACK_LANGUAGE || wanted.contains(&l.code));
    let english = languages
        .iter()
        .find(|l| l.code == paperpaw_i18n::FALLBACK_LANGUAGE)
        .cloned();
    let repository = repository();
    // A key of another plugin of the app English has: a translation of it.
    let translates = |key: &str| {
        all.iter().any(|p| p == namespace(key)) && english.as_ref().is_some_and(|e| e.has(key))
    };
    for plugin in &looked {
        let data = context.target.data.iter().find(|d| &d.id == plugin);
        let is_own = own(plugin, data.and_then(|d| d.folder.as_deref()));
        let source = sources(plugin);
        let place = |code: &str| match (data, &source) {
            (Some(data), _) if data.file(&format!("i18n/{code}.ini")).is_some() => {
                format!("i18n/{code}.ini")
            }
            (None, Some(folder)) if folder.join(format!("i18n/{code}.ini")).is_file() => {
                files::relative(&folder.join(format!("i18n/{code}.ini")), &repository)
            }
            _ => format!("i18n ({code})"),
        };
        for finding in catalogs::translations(plugin, is_own, &languages, &place) {
            report.add(finding);
        }
        // Their files: dashes, keys; a data plugin's settings.
        let found = match (data, &source) {
            (Some(data), _) => {
                let ini = files::data_ini_files(data);
                let mut found = files::catalog_files(plugin, ini, is_own, &translates);
                let known = |id: &str| kernel.settings().spec(id).is_some();
                found.extend(files::settings_set(data, &known));
                found
            }
            (None, Some(folder)) => files::catalog_files(
                plugin,
                files::folder_ini_files(&folder.join("i18n"), &repository),
                is_own,
                &translates,
            ),
            (None, None) => {
                let mut found = Vec::new();
                for language in &languages {
                    for key in language.keys_of(plugin) {
                        let at = format!("i18n ({}) {key}", language.code);
                        found.extend(files::dashes(plugin, &at, &language.messages[key], is_own));
                    }
                }
                found
            }
        };
        for finding in found {
            report.add(finding);
        }
    }

    // What the plugins registered: keys English lacks, help texts.
    let ours = |id: &str| {
        let space = namespace(id);
        looked.iter().find(|p| p.as_str() == space).cloned()
    };
    let (used, findings) = texts::registered(kernel, &ours);
    for finding in findings {
        report.add(finding);
    }
    for finding in groups::groups(kernel, &ours) {
        report.add(finding);
    }
    // Descriptions their rows cut, measured with the X4's letters.
    let mut session = session;
    crate::cases::show_screen(&mut session, crate::probe::HELP.as_str(), &[])?;
    for (id, text) in crate::probe::cut_help() {
        let Some(plugin) = ours(&id) else {
            continue;
        };
        report.add(
            Finding::warning(
                "lint.help-cut",
                id,
                format!(
                    "The description is longer than the two lines of its row on the X4, so it \
                     is cut short: make it shorter (detail goes in How it works): \"{text}\""
                ),
            )
            .plugin(&plugin),
        );
    }
    if let Some(english) = &english {
        for finding in texts::missing(&used, english) {
            report.add(finding);
        }
    }
    drop(session);

    drawn(context, &looked, &all, report)
}

/// The shortest *How it works* that says enough, in characters.
const LEAST_HOW: usize = 120;

/// What to say of plugin `id`'s *How it works* (`how`), if anything: a
/// plugin with code must say how it works, at some length.
fn how_it_works(id: &str, how: Option<&str>) -> Option<Finding> {
    if id == paperpaw_core::sdk::builtin::CORE_PLUGIN.as_str() {
        return None;
    }
    let message = match how {
        None => String::from(
            "It says nothing of how it works: give its manifest `how` (`how/en.txt` and a \
             file per other language, see Manifest::how), a few sentences (paragraphs apart \
             by an empty line) on what it does, where it is found and how it is used. The \
             plugin manager and the store show it as How it works.",
        ),
        Some(text) if text.chars().count() < LEAST_HOW => format!(
            "How it works says little ({} characters): a few sentences on what it does, \
             where it is found and how it is used, at least {LEAST_HOW} characters.",
            text.chars().count()
        ),
        Some(_) => return None,
    };
    Some(Finding::warning("lint.how-it-works", id, message).plugin(id))
}

/// The profiles the pseudo-language is drawn on, among those asked for:
/// the X4 (buttons alone) and the Boox (touch), else the first of each.
fn profiles(context: &Context) -> Vec<&'static Profile> {
    let mut chosen: Vec<&'static Profile> = Vec::new();
    for (preferred, touch) in [("xteink-x4", false), ("boox-go-7", true)] {
        let asked = &context.profiles;
        let profile = asked
            .iter()
            .find(|p| p.device.id == preferred)
            .or_else(|| asked.iter().find(|p| p.device.input.touch == touch));
        if let Some(p) = profile
            && !chosen.iter().any(|c| c.device.id == p.device.id)
        {
            chosen.push(p);
        }
    }
    chosen
}

/// The UI font at the size of notes, the smallest the UI draws text in.
struct Ui<'a> {
    font: FontHandle<'a>,
    small: f32,
}

impl<'a> Ui<'a> {
    fn of(kernel: &'a paperpaw_core::Kernel) -> Option<Ui<'a>> {
        let font = kernel.fonts().role(fonts::UI.as_str())?;
        let dpi = f32::from(kernel.device().display.dpi);
        let base = kernel.settings().read(&settings::UI_SIZE).clamp(6, 40) as f32;
        // As `UiStyle::small`.
        let small = base * 0.75 * dpi / 72.0;
        Some(Ui { font, small })
    }
}

impl Measure for Ui<'_> {
    fn width(&self, text: &str) -> f32 {
        self.font.measure(self.small, text)
    }

    fn line_height(&self) -> f32 {
        self.font.line_metrics(self.small).line_height
    }

    fn lines(&self, text: &str, width: f32) -> usize {
        self.font.wrap(self.small, text, width).len().max(1)
    }
}

/// Saves `image` for a finding at `name` in the output folder; the path
/// relative to it.
fn picture(context: &Context, image: &crate::image::Image, name: &str) -> Option<PathBuf> {
    if context.in_memory {
        return None;
    }
    let relative = PathBuf::from("lint").join(format!("{name}.png"));
    let path = context.path(&relative.to_string_lossy());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    image.save_png(&path).ok()?;
    Some(relative)
}

/// A file name for a place.
fn slug(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// The sessions a profile is drawn in: English, the pseudo-language and,
/// if the app has one, another language (to tell a language's content,
/// such as quotes, from text not translated).
struct Sessions {
    english: Session,
    pseudo: Session,
    other: Option<Session>,
}

impl Sessions {
    fn start(context: &Context, profile: &'static Profile) -> Result<Sessions, String> {
        let english = Session::new(context, &Variant::of(profile))?;
        let pseudo = Session::new(
            context,
            &Variant {
                language: PSEUDO.to_string(),
                ..Variant::of(profile)
            },
        )?;
        let translator = pseudo.device.kernel.translator();
        if translator.language() != PSEUDO
            || !translator
                .languages()
                .iter()
                .any(|(code, _)| *code == PSEUDO)
        {
            return Err(format!(
                "{}: the pseudo-language was not taken (plugins/{} not loaded)",
                pseudo.variant.name(),
                pseudo::PLUGIN
            ));
        }
        let other = context
            .app_languages()
            .iter()
            .find(|code| *code != paperpaw_i18n::FALLBACK_LANGUAGE && *code != PSEUDO)
            .map(|code| {
                Session::new(
                    context,
                    &Variant {
                        language: code.clone(),
                        ..Variant::of(profile)
                    },
                )
            })
            .transpose()?;
        Ok(Sessions {
            english,
            pseudo,
            other,
        })
    }
}

/// What a case drew in each session.
struct Drawn {
    english: Vec<Described>,
    pseudo: Vec<Described>,
    other: Option<Vec<String>>,
}

/// The names in the library of `session` (files and folders, two levels
/// deep), lowercase, as words: data, not texts of the app.
fn library_words(session: &Session) -> Vec<String> {
    let storage = session.storage();
    let mut names: Vec<String> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    let _ = storage.list("", &mut |entry| {
        names.push(entry.name.to_string());
        if entry.is_dir {
            dirs.push(entry.name.to_string());
        }
    });
    for dir in dirs {
        let _ = storage.list(&dir, &mut |entry| names.push(entry.name.to_string()));
    }
    let mut words: Vec<String> = Vec::new();
    for name in names {
        for word in name.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
            if !word.is_empty() && !words.iter().any(|w| w == word) {
                words.push(word.to_string());
            }
        }
    }
    words
}

/// Every id the app registered that may be shown as it is.
fn known_ids(session: &Session) -> Vec<String> {
    let kernel = &session.device.kernel;
    let mut ids: Vec<String> = kernel.known_screens().map(ToString::to_string).collect();
    ids.extend(kernel.settings().specs().map(|s| s.id.to_string()));
    ids.extend(kernel.settings().groups().map(|g| g.id.to_string()));
    ids.extend(
        kernel
            .widgets()
            .everywhere()
            .into_iter()
            .map(|(id, _, _)| id.to_string()),
    );
    ids.extend(kernel.actions().iter().map(|a| a.id().to_string()));
    ids.extend(kernel.plugins().iter().map(|p| p.manifest.id.to_string()));
    ids.sort();
    ids
}

/// Draws the screens and widgets of the plugins looked at in English and
/// in the pseudo-language and compares what is drawn.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn drawn(
    context: &Context,
    looked: &[String],
    all: &[String],
    report: &mut Report,
) -> Result<(), String> {
    let mine = |id: &str| looked.iter().any(|p| p == namespace(id));
    for profile in profiles(context) {
        let mut sessions = Sessions::start(context, profile)?;
        let variant = sessions.pseudo.variant.clone();
        let data = library_words(&sessions.pseudo);
        let ids = known_ids(&sessions.pseudo);
        let id = |word: &str| ids.binary_search_by(|i| i.as_str().cmp(word)).is_ok();
        let mut found: Vec<(Finding, Option<PathBuf>, Option<Rect>)> = Vec::new();
        let (mut shown_screens, mut pseudo_texts, mut all_texts) = (0, 0, 0);
        let mut check = |plugin: &str,
                         place: &str,
                         drawn: &Drawn,
                         image: &crate::image::Image,
                         ui: Option<&Ui<'_>>| {
            for d in drawn.pseudo.iter().filter(|d| d.role == Role::Text) {
                all_texts += 1;
                if drawn::is_pseudo(&d.label) {
                    pseudo_texts += 1;
                }
            }
            let known = drawn::Known {
                plugins: all,
                id: &id,
                data: &data,
                elsewhere: drawn.other.as_deref(),
            };
            let mut here: Vec<(Finding, Option<Rect>)> = Vec::new();
            if let Some(ui) = ui
                && let Some((finding, area)) =
                    drawn::overflow(plugin, place, &drawn.english, &drawn.pseudo, ui)
            {
                here.push((finding, Some(area)));
            }
            for finding in drawn::untranslated(plugin, place, &drawn.pseudo, &known) {
                here.push((finding, None));
            }
            let picture = (!here.is_empty())
                .then(|| {
                    picture(
                        context,
                        image,
                        &format!("{}-{}", variant.slug(), slug(place)),
                    )
                })
                .flatten();
            for (finding, area) in here {
                found.push((finding, picture.clone(), area));
            }
        };

        let screens: Vec<String> = cases::screens(&sessions.pseudo, &context.target)
            .into_iter()
            .filter(|s| mine(s))
            .collect();
        for screen in &screens {
            cases::home(&mut sessions.pseudo);
            let shown = cases::show_screen(&mut sessions.pseudo, screen, &[])?;
            if !shown.opened() {
                continue;
            }
            shown_screens += 1;
            cases::home(&mut sessions.english);
            let english = cases::show_screen(&mut sessions.english, screen, &[])?;
            let other = match sessions.other.as_mut() {
                Some(other) => {
                    cases::home(other);
                    let shown = cases::show_screen(other, screen, &[])?;
                    Some(shown.described.into_iter().map(|d| d.label).collect())
                }
                None => None,
            };
            let drawn = Drawn {
                english: english.described,
                pseudo: shown.described,
                other,
            };
            let ui = Ui::of(&sessions.pseudo.device.kernel);
            check(namespace(screen), screen, &drawn, &shown.image, ui.as_ref());
        }

        let grid = cases::grid(&sessions.pseudo);
        let size = (grid.0.min(2), grid.1.min(2));
        let widgets: Vec<String> = cases::widgets(&sessions.pseudo, &context.target)
            .into_iter()
            .map(|(id, _)| id)
            .filter(|w| mine(w))
            .collect();
        for widget in &widgets {
            let shown = cases::show_widget(&mut sessions.pseudo, widget, size)?;
            let Some(cell) = shown.cell else {
                continue;
            };
            // Only what the widget drew, in its cell.
            let inside = |described: Vec<Described>| -> Vec<Described> {
                described
                    .into_iter()
                    .filter(|d| d.area.intersection(&cell).is_some())
                    .collect()
            };
            let english = cases::show_widget(&mut sessions.english, widget, size)?;
            let other = match sessions.other.as_mut() {
                Some(other) => {
                    let shown = cases::show_widget(other, widget, size)?;
                    Some(
                        inside(shown.described)
                            .into_iter()
                            .map(|d| d.label)
                            .collect(),
                    )
                }
                None => None,
            };
            let drawn = Drawn {
                english: inside(english.described),
                pseudo: inside(shown.described),
                other,
            };
            let place = format!("{widget} {}x{}", size.0, size.1);
            let ui = Ui::of(&sessions.pseudo.device.kernel);
            check(namespace(widget), &place, &drawn, &shown.image, ui.as_ref());
        }

        report.add(
            Finding::info(
                "lint.pseudo",
                profile.device.id,
                format!(
                    "Drew {shown_screens} of {} screens (the others need parameters) and {} \
                     widgets in the pseudo-language; {pseudo_texts} of {all_texts} texts were \
                     in it.",
                    screens.len(),
                    widgets.len()
                ),
            )
            .variant(variant.name()),
        );
        for (finding, image, area) in found {
            let mut finding = finding.variant(variant.name());
            if let Some(image) = image {
                finding = finding.picture(image);
            }
            if let Some(area) = area {
                finding = finding.area(area);
            }
            report.add(finding);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_plugin_says_how_it_works_at_some_length() {
        let missing = how_it_works("apple", None).unwrap();
        assert_eq!(missing.check, "lint.how-it-works");
        assert!(missing.message.contains("Manifest::how"));
        let short = how_it_works("apple", Some("Shows apples.")).unwrap();
        assert!(short.message.contains("13 characters"), "{}", short.message);
        assert!(how_it_works("apple", Some(&"Shows apples. ".repeat(10))).is_none());
        assert!(how_it_works(paperpaw_core::sdk::builtin::CORE_PLUGIN.as_str(), None).is_none());
    }
}
