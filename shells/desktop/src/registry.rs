//! What PaperPaw's standard app offers data plugins, as JSON
//! (`paperpaw-emu --registry <file>`): every setting (its id, kind, range
//! or choices, default and texts), every widget a home screen layout can
//! place, every action a key map can bind, and the emulator's device
//! profiles. Tools that write or check plugins for a release (the
//! handbook, the workshop) read it from that release, so they name only
//! what it has and need no change of their own when it changes.
//!
//! [`VERSION`] says which fields there are; a field is only added, and
//! one whose meaning changes gets a new name, the old one kept.

use std::fmt::Write as _;

use paperpaw_core::sdk::json::string;
use paperpaw_core::sdk::{SettingKind, SettingValue};

use crate::device::{Device, library};
use crate::profiles::{PROFILES, Profile};

/// The registry's version. 2: `version`, `paperpaw`, and for profiles
/// `colour_screen` (a colour panel) and `tinted` (black and white shown
/// in the colours set, as `colors` said, which is kept).
pub const VERSION: u32 = 2;

/// The registry of the standard app on the first profile (the X4: what
/// every device has; profiles add defaults, not settings), its card in a
/// folder of its own for the time it takes.
pub fn registry() -> Result<String, String> {
    let dir = crate::private::dir("paperpaw-registry")
        .map_err(|e| format!("a folder for the card: {e}"))?;
    let made = library(&dir)
        .map_err(|e| format!("the emulated card: {e}"))
        .and_then(|storage| {
            Device::new(&PROFILES[0], None, storage, true).map_err(|e| format!("{e:?}"))
        })
        .map(|device| describe(&device));
    let _ = std::fs::remove_dir_all(&dir);
    made
}

/// The registry of `device`'s app.
fn describe(device: &Device) -> String {
    let kernel = &device.kernel;
    let tr = |key: &str| string(kernel.translator().tr(key));
    let mut out = format!(
        "{{\n\"version\": {VERSION},\n\"paperpaw\": {},\n\"settings\": [",
        string(env!("CARGO_PKG_VERSION"))
    );
    for (i, spec) in kernel.settings().specs().enumerate() {
        let kind = match &spec.kind {
            SettingKind::Bool => String::from("\"kind\": \"switch\""),
            SettingKind::Int { min, max } => {
                format!("\"kind\": \"number\", \"min\": {min}, \"max\": {max}")
            }
            SettingKind::Choice(choices) => {
                let choices: Vec<String> = choices
                    .iter()
                    .map(|c| format!("{{\"value\": {}, \"label\": {}}}", string(c), tr(c)))
                    .collect();
                format!(
                    "\"kind\": \"choice\", \"choices\": [{}]",
                    choices.join(", ")
                )
            }
            SettingKind::Text => String::from("\"kind\": \"text\""),
            SettingKind::Language => String::from("\"kind\": \"language\""),
            SettingKind::List => String::from("\"kind\": \"list\""),
            _ => String::from("\"kind\": \"other\""),
        };
        let default = match &spec.default {
            SettingValue::Bool(b) => b.to_string(),
            SettingValue::Int(n) => n.to_string(),
            SettingValue::Text(t) => string(t),
            _ => String::from("null"),
        };
        let help = spec
            .description
            .as_ref()
            .map(|help| format!(", \"help\": {}", tr(help)))
            .unwrap_or_default();
        let _ = write!(
            out,
            "{}\n{{{kind}, \"id\": {}, \"title\": {}{help}, \"default\": {default}, \"group\": {}, \"advanced\": {}, \"scoped\": {}}}",
            if i > 0 { "," } else { "" },
            string(spec.id.as_str()),
            tr(&spec.title),
            string(spec.group.as_str()),
            format!("{:?}", spec.level) == "Advanced",
            spec.scoped,
        );
    }
    out.push_str("\n],\n\"widgets\": [");
    // The widgets plugins registered and those native plugins declared.
    for (i, (id, title, _kind)) in kernel.widgets().everywhere().iter().enumerate() {
        let comma = if i > 0 { "," } else { "" };
        let _ = write!(
            out,
            "{comma}\n{{\"id\": {}, \"title\": {}}}",
            string(id.as_str()),
            tr(title)
        );
    }
    out.push_str("\n],\n\"actions\": [");
    for (i, action) in kernel.actions().iter().enumerate() {
        let comma = if i > 0 { "," } else { "" };
        let _ = write!(
            out,
            "{comma}\n{{\"id\": {}, \"title\": {}}}",
            string(action.id().as_str()),
            tr(action.title())
        );
    }
    out.push_str("\n],\n\"profiles\": [");
    for (i, profile) in PROFILES.iter().enumerate() {
        let comma = if i > 0 { "," } else { "" };
        let _ = write!(out, "{comma}\n{}", profile_json(profile));
    }
    out.push_str("\n]\n}\n");
    out
}

/// A profile as the registry has it.
fn profile_json(p: &Profile) -> String {
    let d = &p.device;
    let buttons: Vec<String> = p.buttons.iter().map(|b| string(b)).collect();
    let store = p
        .catalog_device
        .map_or_else(|| String::from("null"), string);
    format!(
        "{{\"id\": {}, \"name\": {}, \"width\": {}, \"height\": {}, \"dpi\": {}, \"touch\": {}, \"buttons\": [{}], \"colour_screen\": {}, \"tinted\": {}, \"colors\": {}, \"store\": {store}, \"body\": {}, \"summary\": {}}}",
        string(d.id),
        string(d.name),
        d.display.width,
        d.display.height,
        d.display.dpi,
        d.input.touch,
        buttons.join(", "),
        colour_screen(p),
        p.colors,
        p.colors,
        p.body.json(),
        string(p.summary),
    )
}

/// Whether the profile's screen shows colour (a colour panel such as
/// Kaleido or Gallery), not black and white shown in colours set.
pub fn colour_screen(p: &Profile) -> bool {
    p.device.display.color.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str) -> &'static Profile {
        PROFILES.iter().find(|p| p.device.id == id).unwrap()
    }

    /// A colour screen is one with a colour panel: the Kobo Libra Colour,
    /// not the Boox Go 7 (gray, its black and white shown tinted) nor
    /// the X4s.
    #[test]
    fn colour_screens_are_colour_panels() {
        assert!(colour_screen(profile("kobo-libra-colour")));
        for id in ["boox-go-7", "xteink-x4", "xteink-x4-pro"] {
            assert!(!colour_screen(profile(id)), "{id}");
        }
        let boox = profile_json(profile("boox-go-7"));
        assert!(boox.contains("\"colour_screen\": false"), "{boox}");
        assert!(boox.contains("\"tinted\": true") && boox.contains("\"colors\": true"));
        let kobo = profile_json(profile("kobo-libra-colour"));
        assert!(kobo.contains("\"colour_screen\": true"), "{kobo}");
    }

    /// The registry is JSON with what tools read: its version, settings,
    /// widgets, actions and every profile.
    #[test]
    fn the_registry_names_what_the_app_offers() {
        let _one = crate::one_device_at_a_time();
        let text = registry().unwrap();
        let members = paperpaw_core::sdk::json::object(&text);
        let keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            [
                "version", "paperpaw", "settings", "widgets", "actions", "profiles"
            ],
            "{text}"
        );
        assert_eq!(
            paperpaw_core::sdk::json::number(&members, "version"),
            Some(f64::from(VERSION))
        );
        assert!(text.contains("\"id\": \"core.device-name\""));
        // What native plugins declare is listed as well as what the kernel
        // registered itself (#372).
        assert!(text.contains("{\"id\": \"core.space\""), "{text}");
        assert!(text.contains("{\"id\": \"widgets.word-clock\""), "{text}");
        assert!(text.contains("{\"id\": \"widgets.todo.open\""), "{text}");
        assert!(
            text.contains("\"id\": \"widgets.word-clock.font\""),
            "{text}"
        );
        assert_eq!(text.matches("\"colour_screen\":").count(), PROFILES.len());
        assert_eq!(
            text.matches("\"body\": {\"bezel\":").count(),
            PROFILES.len()
        );
    }
}
