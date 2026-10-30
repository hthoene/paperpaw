//! Every notice the store gives fits the notice whole, in each language,
//! on the X4 and the X4 Pro: a notice has three lines, and a reason cut
//! off is a reason the reader does not get.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_i18n::Arg;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_store::Store;
use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, Id, Manifest, MemoryStorage, Plugin, RegisterError,
    Registrar, SDK_VERSION,
};

/// The notices, by their translation keys, each taking `{name}`.
const NOTICES: [&str; 11] = [
    "store.installed",
    "store.updated",
    "store.failed",
    "store.installed-code",
    "store.updated-code",
    "store.refused.key-changed",
    "store.refused.damaged",
    "store.refused.other-build",
    "store.refused.unsigned",
    "store.refused.not-the-author",
    "store.refused.unsupported",
];

/// A plugin's name as long as many are.
const NAME: &str = "Reading clock";

/// The X4's and the X4 Pro's screens and input, as their firmware says.
fn devices() -> [DeviceInfo; 2] {
    let device = |id, dpi, touch| {
        DeviceInfo::new(
            id,
            id,
            DisplayInfo::new(480, 800, dpi, PixelFormat::Mono1, Rotation::Deg0, None),
            InputCaps::new(true, touch, false, &[]),
            MemoryClass::Small,
        )
    };
    [
        device("xteink-x4", 220, false),
        device("xteink-x4-pro", 217, true),
    ]
}

/// Shows a notice per action: `probe.<n>` the store's notice `n`.
struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", SDK_VERSION)
            .description("Shows the store's notices.")
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (n, key) in NOTICES.into_iter().enumerate() {
            let id = Id::parse(&format!("probe.n{n}")).unwrap();
            r.add_action(ActionSpec::new(id, "probe").handler(
                move |env: &Env<'_>, _, out: &mut Commands| {
                    let text = env.i18n().format(key, &[("name", Arg::Str(NAME))]);
                    out.push(Command::Notify(text));
                },
            ))?;
        }
        Ok(())
    }
}

/// What the notices say on `device` in `language` at the UI size
/// `ui_size` (`None`: the default), each with its key and its text.
fn shown(device: DeviceInfo, language: &str, ui_size: Option<i64>) -> Vec<(String, String)> {
    let builder = Kernel::builder(device)
        .storage(MemoryStorage::new())
        .user_config(format!("[core]\nlanguage = {language}\n"))
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_menu::Menu)
        .plugin(Store)
        .plugin(Probe);
    let builder = match ui_size {
        Some(size) => builder.setting_default(&settings::UI_SIZE, size),
        None => builder,
    };
    let mut kernel = builder.build().unwrap();
    let _ = kernel.start(Instant(0));
    let mut now = 0;
    NOTICES
        .iter()
        .enumerate()
        .map(|(n, key)| {
            now += 100;
            let id = Id::parse(&format!("probe.n{n}")).unwrap();
            let _ = kernel.run_action(Instant(now), id);
            kernel.render(&mut |_, _| {});
            let lines: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
            (String::from(*key), lines.join(" "))
        })
        .collect()
}

#[test]
fn every_notice_fits_whole() {
    let mut cut = Vec::new();
    for device in devices() {
        for language in ["en", "de"] {
            for ui_size in [None, Some(9)] {
                for (key, shown) in shown(device, language, ui_size) {
                    if shown.contains('…') || !shown.contains(NAME) {
                        cut.push(format!(
                            "{} {language} {ui_size:?} {key}: {shown}",
                            device.id
                        ));
                    }
                }
            }
        }
    }
    assert!(cut.is_empty(), "cut off:\n{}", cut.join("\n"));
}
