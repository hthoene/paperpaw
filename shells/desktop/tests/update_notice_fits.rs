//! The update plugin's refusal notices show whole on its page on the narrowest
//! devices, the X4 and the X4 Pro, in English and German. The page wraps the
//! text under its title to at most six lines in the small size, in the width
//! of the screen less four margins (`paperpaw_ui_kit` `draw_empty`), and
//! shortens a longer text with "…". One of them is what a CrossPoint user is
//! shown when the update does not fit the device's storage layout (Decision
//! 179): it must be read whole, with the way out in it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;
use std::sync::Mutex;

use paperpaw_core::KernelBuilder;
use paperpaw_core::sdk::ui::UiStyle;
use paperpaw_core::sdk::{
    Canvas, Env, Id, Manifest, MemoryStorage, Plugin, RegisterError, Registrar, Screen, ScreenSpec,
    Version,
};
use paperpaw_emulator::{device::Device, headless, profiles};

/// The most lines the page gives its text (`draw_empty`).
const PAGE_LINES: usize = 6;
/// The texts the page shows for a refusal: device, language, key, lines.
static SEEN: Mutex<Vec<(String, String, String, usize)>> = Mutex::new(Vec::new());

const SCREEN: Id = Id::new_static("notice-probe.screen");

/// The `key = value` lines of one of the plugin's catalogues.
fn catalogue(language: &str) -> Vec<(String, String)> {
    let path = format!(
        "{}/../../plugins/update/i18n/{language}.ini",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|line| line.split_once(" = "))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("notice-probe"),
            "Notice probe",
            Version::new(0, 1, 0),
        )
        .uses(paperpaw_core::sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "Notice probe",
            Box::new(|_, _| Box::new(Look)),
        ))
    }
}

struct Look;

impl Screen for Look {
    fn draw(&mut self, _: &mut Canvas<'_>, env: &Env<'_>) {
        paperpaw_emulator::budget::untracked(|| look(env));
    }

    fn on_action(
        &mut self,
        _: &Id,
        _: &mut paperpaw_core::sdk::Cx<'_, '_>,
    ) -> paperpaw_core::sdk::Handled {
        paperpaw_core::sdk::Handled::No
    }
}

/// How many lines each refusal reason of the update plugin takes on the page.
fn look(env: &Env<'_>) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let width = f32::from(env.device().display.width) - 4.0 * f32::from(style.margin());
    let language = env.i18n().language().to_string();
    for (key, text) in catalogue(&language) {
        if key.starts_with("update.reason.") {
            let lines = font.wrap(style.small(), &text, width).len();
            SEEN.lock()
                .unwrap()
                .push((env.device().id.to_string(), language.clone(), key, lines));
        }
    }
}

fn add(builder: KernelBuilder) -> KernelBuilder {
    builder.plugin(Probe)
}

#[test]
fn the_refusal_notices_show_whole_on_the_updates_page() {
    for profile in ["xteink-x4", "xteink-x4-pro"] {
        for language in ["en", "de"] {
            let profile = profiles::find(profile).unwrap();
            let config = format!("[core]\nlanguage = {language}\n[help]\nshow = false\n");
            let files = Rc::new(MemoryStorage::new());
            let mut device =
                Device::in_memory(profile, Some(config), files, true, Some(add)).unwrap();
            headless::run_script(&mut device, "wait:500,open:notice-probe.screen,wait:300")
                .unwrap();
            device.draw_again();
        }
    }
    let mut seen = SEEN.lock().unwrap().clone();
    seen.sort();
    seen.dedup();
    // The notice of Decision 179 was looked at on every device in both languages.
    let notice = |device: &str, language: &str| {
        seen.iter()
            .find(|(d, l, key, _)| d == device && l == language && key == "update.reason.too-large")
            .map(|(.., lines)| *lines)
    };
    for device in ["xteink-x4", "xteink-x4-pro"] {
        for language in ["en", "de"] {
            assert!(
                notice(device, language).is_some(),
                "the too-large notice was not looked at: {device} {language}"
            );
        }
    }
    for (device, language, key, lines) in &seen {
        if key == "update.reason.too-large" {
            println!("{device} {language} {key}: {lines} of {PAGE_LINES} lines");
        }
    }
    let cut: Vec<String> = seen
        .iter()
        .filter(|(.., lines)| *lines > PAGE_LINES)
        .map(|(device, language, key, lines)| format!("{device} {language} {key}: {lines} lines"))
        .collect();
    assert!(
        cut.is_empty(),
        "these notices need more than the {PAGE_LINES} lines of the page and would be cut:\n{}",
        cut.join("\n")
    );
}
