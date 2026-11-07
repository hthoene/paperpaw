//! Plugins a shell did not install at the start (the web flasher's, ADR
//! 0024) are named in a notice, with why; one notice shows at a time, so
//! several share it, and safe mode's wins.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{CodeFile, CodeLoader, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::native::{Native, NotInstalled, NotInstalledReason, SafeMode, SafeModeReason};
use paperpaw_sdk::{Id, MemoryStorage, Plugin, PluginProblem, Storage};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// Loads nothing: no plugin here has code.
struct NoCode;

impl CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Unsupported)
    }
}

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// The text shown right after a start with `not_installed`, and in safe
/// mode if `safe_mode`.
fn shown(not_installed: &[(&'static str, NotInstalledReason)], safe_mode: bool) -> String {
    let safe_mode = safe_mode.then(|| SafeMode::new(SafeModeReason::KeyHeld));
    shown_in("en", not_installed, safe_mode)
}

/// [`shown`] in `language`, in `safe_mode` if one.
fn shown_in(
    language: &str,
    not_installed: &[(&'static str, NotInstalledReason)],
    safe_mode: Option<SafeMode>,
) -> String {
    let mut native = Native::new("code.ppn");
    native.not_installed = not_installed
        .iter()
        .map(|&(id, reason)| NotInstalled::new(Id::new_static(id), reason))
        .collect();
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(format!(
            "[help]\nshow = false\n[core]\nlanguage = {language}\n"
        ))
        .code_plugins(native, Box::new(NoCode));
    let builder = match safe_mode {
        Some(safe_mode) => builder.safe_mode(safe_mode),
        None => builder,
    };
    let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
    let _ = kernel.start(Instant(0));
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .into_iter()
        .map(|p| p.label)
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_plugin_by_another_author_is_named() {
    let shown = shown(&[("clock", NotInstalledReason::OtherAuthor)], false);
    assert!(
        shown.contains("clock was not installed: signed by another key than before"),
        "{shown}"
    );
}

#[test]
fn several_share_a_notice() {
    let shown = shown(
        &[
            ("clock", NotInstalledReason::OtherAuthor),
            ("weather", NotInstalledReason::Damaged),
        ],
        false,
    );
    // By the name of the version installed, if there is one.
    assert!(shown.contains("Not installed: clock, Weather."), "{shown}");
}

#[test]
fn safe_mode_says_its_own() {
    let shown = shown(&[("clock", NotInstalledReason::Damaged)], true);
    assert!(shown.contains("The key was held at the start"), "{shown}");
    assert!(!shown.contains("not installed"), "{shown}");
}

#[test]
fn nothing_is_said_when_all_were_installed() {
    let shown = shown(&[], false);
    assert!(!shown.contains("installed"), "{shown}");
}

#[test]
fn the_notices_fit_whole() {
    // A notice has three lines: a long name still leaves the reason.
    for language in ["en", "de"] {
        for not_installed in [
            &[("reading-clock", NotInstalledReason::OtherAuthor)][..],
            &[("reading-clock", NotInstalledReason::Damaged)],
            &[
                ("reading-clock", NotInstalledReason::OtherAuthor),
                ("weather", NotInstalledReason::Damaged),
            ],
        ] {
            let shown = shown_in(language, not_installed, None);
            assert!(fits(&shown), "{language}: {shown}");
        }
        // Safe mode's, naming the plugin.
        let mut crashed = SafeMode::new(SafeModeReason::FailedStarts);
        crashed.crashed = Some(String::from("reading-clock"));
        let mut last = SafeMode::new(SafeModeReason::FailedStarts);
        last.last_loaded = Some(String::from("reading-clock"));
        for safe_mode in [crashed, last] {
            let shown = shown_in(language, &[], Some(safe_mode));
            assert!(fits(&shown), "{language}: {shown}");
        }
    }
}

/// Whether the notice `shown` names the plugin and is not cut off.
fn fits(shown: &str) -> bool {
    shown.replace("- ", "-").contains("reading-clock") && !shown.contains('…')
}
