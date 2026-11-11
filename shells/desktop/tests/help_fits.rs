//! Every setting's description of PaperPaw's own plugins shows whole in
//! its row on the narrowest devices, the X4 and the X4 Pro, in English and
//! German: a row gives it two lines, and a longer one would be shortened
//! with "…". Detail that does not fit belongs in the plugin's How it
//! works. paperpaw-dev says the same of other plugins (`lint.help-cut`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;
use std::sync::Mutex;

use paperpaw_core::KernelBuilder;
use paperpaw_core::sdk::ui::{self, UiStyle};
use paperpaw_core::sdk::{
    Canvas, Env, Id, Manifest, MemoryStorage, Plugin, RegisterError, Registrar, Screen, ScreenSpec,
    Version,
};
use paperpaw_emulator::{device::Device, headless, profiles};

/// The descriptions found too long: device, language, setting, text.
static CUT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Looks at every setting's description when its screen is drawn.
struct Probe;

const SCREEN: Id = Id::new_static("help-probe.screen");

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("help-probe"),
            "Help probe",
            Version::new(0, 1, 0),
        )
        .uses(paperpaw_core::sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "Help probe",
            Box::new(|_, _| Box::new(Look)),
        ))
    }
}

struct Look;

impl Screen for Look {
    fn draw(&mut self, _: &mut Canvas<'_>, env: &Env<'_>) {
        // Not the device's heap: the probe is no part of what it runs.
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

/// Every setting's description that does not fit, kept in [`CUT`].
fn look(env: &Env<'_>) {
    {
        let style = UiStyle::new(env);
        let width = env.device().display.width;
        let language = env.i18n().language().to_string();
        for spec in env.settings().specs() {
            let Some(key) = &spec.description else {
                continue;
            };
            if spec.id.as_str().starts_with("help-probe.") {
                continue;
            }
            let text = env.i18n().tr(key);
            if !ui::note_fits(env, &style, width, text) {
                CUT.lock().unwrap().push(format!(
                    "{} {language} {} [{key}]: {text}",
                    env.device().id,
                    spec.id
                ));
            }
        }
    }
}

fn add(builder: KernelBuilder) -> KernelBuilder {
    builder.plugin(Probe)
}

#[test]
fn every_settings_description_fits_its_row() {
    for profile in ["xteink-x4", "xteink-x4-pro"] {
        for language in ["en", "de"] {
            let profile = profiles::find(profile).unwrap();
            let config = format!("[core]\nlanguage = {language}\n[help]\nshow = false\n");
            let files = Rc::new(MemoryStorage::new());
            let mut device =
                Device::in_memory(profile, Some(config), files, true, Some(add)).unwrap();
            headless::run_script(&mut device, "wait:500,open:help-probe.screen,wait:300").unwrap();
            device.draw_again();
        }
    }
    let mut cut = CUT.lock().unwrap().clone();
    cut.sort();
    cut.dedup();
    assert!(
        cut.is_empty(),
        "{} descriptions do not fit the two lines of their row; shorten them (detail goes in \
         the plugin's How it works):\n{}",
        cut.len(),
        cut.join("\n")
    );
}
