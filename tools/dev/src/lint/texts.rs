//! The texts a plugin registers: titles of settings, groups, widgets and
//! actions, help texts, the names of choices, units, presets. Each is a
//! translation key; one English lacks is shown as the key itself.

use paperpaw_core::Kernel;
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::{SettingKind, SettingSpec};

use super::catalogs::Language;
use crate::report::Finding;

/// A translation key a plugin's registration uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Used {
    pub plugin: String,
    /// What it is the text of, e.g. the setting's id.
    pub place: String,
    /// Which text, e.g. `title`, `help`.
    pub what: &'static str,
    pub key: String,
    /// Plural forms are looked up (`<key>.other`).
    pub plural: bool,
}

/// Whether `text` looks like a translation key rather than plain text:
/// one word with dots, starting with a letter.
pub fn looks_like_key(text: &str) -> bool {
    text.contains('.')
        && text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'@'))
}

/// The keys a setting's texts use, and a finding if it has no help text.
pub fn setting(plugin: &str, spec: &SettingSpec) -> (Vec<Used>, bool) {
    let id = spec.id.as_str();
    let used = |what: &'static str, key: &str, plural: bool| Used {
        plugin: plugin.to_string(),
        place: id.to_string(),
        what,
        key: key.to_string(),
        plural,
    };
    let mut keys = vec![used("title", &spec.title, false)];
    if let Some(key) = &spec.description {
        keys.push(used("help", key, false));
    }
    if let SettingKind::Choice(choices) = &spec.kind {
        for value in choices.iter().filter(|v| looks_like_key(v)) {
            keys.push(used("choice", value, false));
        }
    }
    let extra = spec.extra();
    if let Some(unit) = &extra.unit {
        keys.push(used("unit", unit, true));
    }
    if let Some(section) = &extra.section {
        keys.push(used("section", section, false));
    }
    keys.retain(|u| !u.key.is_empty());
    (keys, spec.description.is_none())
}

/// One finding for the settings of a group (`place`) without help texts.
pub fn without_help(plugin: &str, place: &str, settings: &[String]) -> Finding {
    let listed: Vec<String> = settings.iter().take(8).map(|s| format!("`{s}`")).collect();
    let more = settings.len().saturating_sub(8);
    Finding::warning(
        "lint.setting-help",
        place,
        format!(
            "{} {} no help text: {}{}; give each a sentence on what it changes with \
             `.description(key)`.",
            settings.len(),
            if settings.len() == 1 {
                "setting has"
            } else {
                "settings have"
            },
            listed.join(", "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        ),
    )
    .plugin(plugin.to_string())
}

/// The keys a preset uses, and findings for names given as plain text.
pub fn preset(plugin: &str, preset: &Preset) -> (Vec<Used>, Vec<Finding>) {
    let mut keys = Vec::new();
    let mut findings = Vec::new();
    for (what, text) in [
        ("name", preset.title.as_ref()),
        ("description", preset.description.as_ref()),
    ] {
        if text.is_empty() {
            continue;
        }
        if looks_like_key(text) {
            keys.push(Used {
                plugin: plugin.to_string(),
                place: preset.id.to_string(),
                what,
                key: text.to_string(),
                plural: false,
            });
        } else {
            findings.push(
                Finding::warning(
                    "lint.untranslated",
                    preset.id.to_string(),
                    format!(
                        "The preset's {what} is plain text, `{text}`: it is shown so in every \
                         language; give a translation key instead."
                    ),
                )
                .plugin(plugin.to_string()),
            );
        }
    }
    (keys, findings)
}

/// The keys in `used` English lacks.
pub fn missing(used: &[Used], english: &Language) -> Vec<Finding> {
    used.iter()
        .filter(|u| {
            let key = if u.plural {
                format!("{}.other", u.key)
            } else {
                u.key.clone()
            };
            !english.has(&key) && !(u.plural && english.has(&u.key))
        })
        .map(|u| {
            Finding::error(
                "lint.missing-key",
                u.place.clone(),
                format!(
                    "Its {} `{}` has no English message, so the key itself is shown; \
                     add it to the plugin's `i18n/en.ini`.",
                    u.what, u.key
                ),
            )
            .plugin(u.plugin.clone())
        })
        .collect()
}

/// Everything the plugins `ours` names registered, as keys used, and the
/// findings about their texts besides missing keys.
pub fn registered(
    kernel: &Kernel,
    ours: &dyn Fn(&str) -> Option<String>,
) -> (Vec<Used>, Vec<Finding>) {
    let mut used = Vec::new();
    let mut findings = Vec::new();
    let settings = kernel.settings();
    // Settings without help, by plugin and group.
    let mut helpless: Vec<(String, String, Vec<String>)> = Vec::new();
    for spec in settings.specs() {
        if let Some(plugin) = ours(spec.id.as_str()) {
            let (keys, lacks_help) = setting(&plugin, spec);
            used.extend(keys);
            if lacks_help {
                let group = settings.group_of(spec).to_string();
                match helpless
                    .iter_mut()
                    .find(|(p, g, _)| *p == plugin && *g == group)
                {
                    Some((_, _, ids)) => ids.push(spec.id.to_string()),
                    None => helpless.push((plugin, group, vec![spec.id.to_string()])),
                }
            }
        }
    }
    for (plugin, group, ids) in helpless {
        findings.push(without_help(&plugin, &group, &ids));
    }
    for group in settings.groups() {
        if let Some(plugin) = ours(group.id.as_str()) {
            used.push(Used {
                plugin,
                place: group.id.to_string(),
                what: "title",
                key: group.title.to_string(),
                plural: false,
            });
        }
    }
    for preset_ in settings.presets().iter() {
        if let Some(plugin) = ours(preset_.id.as_str()) {
            let (keys, more) = preset(&plugin, preset_);
            used.extend(keys);
            findings.extend(more);
        }
    }
    let widgets = kernel.widgets();
    for (id, title, _) in widgets.everywhere() {
        let Some(plugin) = ours(id.as_str()) else {
            continue;
        };
        if id.as_str().starts_with(crate::probe::PLUGIN) {
            continue;
        }
        used.push(Used {
            plugin: plugin.clone(),
            place: id.to_string(),
            what: "title",
            key: title.to_string(),
            plural: false,
        });
        if let Some(category) = widgets.category(id.as_str()).filter(|c| looks_like_key(c)) {
            used.push(Used {
                plugin,
                place: id.to_string(),
                what: "category",
                key: category.to_string(),
                plural: false,
            });
        }
    }
    for action in kernel.actions().iter() {
        if let Some(plugin) = ours(action.id().as_str())
            && !action.title().is_empty()
        {
            used.push(Used {
                plugin,
                place: action.id().to_string(),
                what: "title",
                key: action.title().to_string(),
                plural: false,
            });
        }
    }
    used.retain(|u| !u.key.is_empty());
    (used, findings)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::collections::BTreeMap;

    use paperpaw_i18n::PluralRule;
    use paperpaw_sdk::{Id, SettingValue};

    use super::*;

    fn english() -> Language {
        let mut messages = BTreeMap::new();
        for key in [
            "demo.face",
            "demo.face.help",
            "demo.face.plain",
            "demo.unit.pt.other",
        ] {
            messages.insert(key.to_string(), String::from("Text"));
        }
        Language {
            code: "en".into(),
            name: "English".into(),
            plural: PluralRule::OneOther,
            messages,
        }
    }

    #[test]
    fn settings_need_titles_help_and_names() {
        static POINTS: paperpaw_sdk::SettingExtra =
            paperpaw_sdk::SettingExtra::new().unit("demo.unit.pt");
        let choices: &'static [&'static str] = &["demo.face.plain", "demo.face.ticks"];
        let spec = SettingSpec::new(
            Id::new_static("demo.face"),
            "demo.face",
            SettingKind::Choice(choices.into()),
            SettingValue::Text(Cow::Borrowed("demo.face.plain")),
            paperpaw_sdk::builtin::groups::OTHER,
        )
        .description("demo.face.help")
        .with_extra(&POINTS);
        let (used, lacks_help) = setting("demo", &spec);
        assert!(!lacks_help);
        let lacking = missing(&used, &english());
        assert_eq!(lacking.len(), 1, "{lacking:#?}");
        assert!(lacking[0].message.contains("choice `demo.face.ticks`"));
        assert_eq!(lacking[0].place, "demo.face");

        let bare = SettingSpec::new(
            Id::new_static("demo.size"),
            "demo.size",
            SettingKind::Int { min: 1, max: 9 },
            SettingValue::Int(3),
            paperpaw_sdk::builtin::groups::OTHER,
        );
        let (used, lacks_help) = setting("demo", &bare);
        assert!(lacks_help);
        assert_eq!(missing(&used, &english())[0].check, "lint.missing-key");
        let help = without_help("demo", "demo.group", &["demo.size".into()]);
        assert_eq!(help.check, "lint.setting-help");
        assert!(
            help.message
                .starts_with("1 setting has no help text: `demo.size`")
        );
    }

    #[test]
    fn presets_name_keys_not_plain_text() {
        let keyed = Preset::new(
            Id::new_static("demo.nook"),
            Id::new_static("home.layout"),
            "demo.face",
            "demo.nook.help",
            String::new(),
        );
        let (used, findings) = preset("demo", &keyed);
        assert!(findings.is_empty());
        assert_eq!(missing(&used, &english()).len(), 1);
        let plain = Preset::new(
            Id::new_static("demo.plain"),
            Id::new_static("home.layout"),
            "Reading nook",
            "",
            String::new(),
        );
        let (used, findings) = preset("demo", &plain);
        assert!(used.is_empty());
        assert_eq!(findings[0].check, "lint.untranslated");
    }

    #[test]
    fn keys_and_plain_text() {
        assert!(looks_like_key("core.corners.square"));
        assert!(!looks_like_key("Reading nook"));
        assert!(!looks_like_key("1.5"));
        assert!(!looks_like_key("plain"));
    }
}
