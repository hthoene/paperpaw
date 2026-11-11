//! A value the configuration file (or the shell) gives for a setting a native
//! plugin declares is set as the plugin is published: the kernel reads the
//! file when it is built, before the plugin is hosted, and the setting is not
//! known then (`reader.open-last` of the reader).
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::{
    native_screen_install::{ARMS, install},
    *,
};
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use paperpaw_sdk::{
    Id, MemoryStorage, SettingExtra, SettingKind, SettingSpec, SettingValue, builtin::CONFIG_FILE,
};

/// A kernel built with `config` as the user's settings, and `factory.flag` (a
/// switch that is on unless said otherwise) declared by a native plugin after
/// it. The warnings of the whole start.
fn kernel_with(config: &str) -> (crate::Kernel, Vec<String>) {
    kernel_with_files("", config)
}

/// [`kernel_with`], `stored` being the device's configuration file
/// (`paperpaw.ini`) and `config` the shell's overrides.
fn kernel_with_files(stored: &str, config: &str) -> (crate::Kernel, Vec<String>) {
    reset(NORMAL);
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(MemoryStorage::new().with(CONFIG_FILE, stored))
        .user_config(config)
        .build()
        .unwrap();
    assert!(kernel.shared.settings.spec("factory.flag").is_none());
    ARMS.with(|arms| arms.set(1));
    let result = install(&mut kernel, 0);
    ARMS.with(|arms| arms.set(0));
    result.unwrap();
    let warnings = kernel
        .take_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    (kernel, warnings)
}

#[test]
fn a_value_in_the_file_reaches_a_setting_declared_later() {
    let (kernel, warnings) = kernel_with("[factory]\nflag = off\n");
    assert_eq!(
        kernel.shared.settings.get("factory.flag"),
        Some(&SettingValue::Bool(false)),
        "the file's value, not the plugin's default"
    );
    assert!(
        !warnings.iter().any(|w| w.contains("factory.flag")),
        "no \"unknown setting\" once it is declared: {warnings:?}"
    );
}

#[test]
fn a_setting_nobody_declares_is_still_warned_about() {
    let (_, warnings) = kernel_with("[nobody]\nflag = off\n");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("`nobody.flag`: unknown setting")),
        "{warnings:?}"
    );
}

#[test]
fn a_value_that_does_not_fit_is_warned_about_once_declared() {
    let (kernel, warnings) = kernel_with("[factory]\nflag = banana\n");
    assert_eq!(
        kernel.shared.settings.get("factory.flag"),
        Some(&SettingValue::Bool(true)),
        "the plugin's default stands"
    );
    let about: Vec<_> = warnings
        .iter()
        .filter(|w| w.contains("factory.flag"))
        .collect();
    assert_eq!(about.len(), 1, "{warnings:?}");
    assert!(about[0].contains("invalid value"), "{about:?}");
}

#[test]
fn the_shells_value_wins_over_the_file_and_the_last_of_a_key_wins() {
    // The device's file first, the shell's after it: the later one is set last.
    let (kernel, _) = kernel_with_files("[factory]\nflag = on\n", "[factory]\nflag = off\n");
    assert_eq!(
        kernel.shared.settings.get("factory.flag"),
        Some(&SettingValue::Bool(false))
    );
    let (kernel, _) = kernel_with_files("[factory]\nflag = off\n", "[factory]\nflag = on\n");
    assert_eq!(
        kernel.shared.settings.get("factory.flag"),
        Some(&SettingValue::Bool(true))
    );
    // Twice in one file: as for a setting known at the build, the last.
    let (kernel, _) = kernel_with("[factory]\nflag = on\nflag = off\n");
    assert_eq!(
        kernel.shared.settings.get("factory.flag"),
        Some(&SettingValue::Bool(false))
    );
}

#[test]
fn a_secret_goes_to_the_secrets_and_not_to_the_settings() {
    static SECRET: SettingExtra = SettingExtra::new().secret();
    reset(NORMAL);
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(MemoryStorage::new().with(CONFIG_FILE, "[factory]\ntoken = abc\n"))
        .build()
        .unwrap();
    // Declared after the build, as a native plugin does.
    kernel
        .shared
        .settings
        .define(
            SettingSpec::new(
                Id::new_static("factory.token"),
                "token",
                SettingKind::Text,
                SettingValue::Text("".into()),
                paperpaw_sdk::builtin::groups::OTHER,
            )
            .with_extra(&SECRET),
        )
        .unwrap();
    kernel.apply_late_config();
    assert_eq!(
        kernel.shared.settings.get("factory.token"),
        Some(&SettingValue::Text("".into())),
        "not a plain setting value"
    );
    assert_eq!(kernel.shared.secrets.get("factory.token"), Some("abc"));
    let warnings = kernel.take_warnings();
    assert!(
        !warnings
            .iter()
            .any(|w| w.to_string().contains("factory.token")
                && !w.to_string().contains("unknown setting")),
        "{warnings:?}"
    );
}

#[test]
fn only_so_many_values_are_kept() {
    let mut config = String::from("[nobody]\n");
    for n in 0..100 {
        config.push_str(&format!("key{n} = 1\n"));
    }
    let (kernel, warnings) = kernel_with(&config);
    assert_eq!(
        kernel.late_config.len(),
        64,
        "the most kept; the start drops them"
    );
    // Every one is warned about: kept or not.
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.contains("unknown setting"))
            .count(),
        100
    );
}
