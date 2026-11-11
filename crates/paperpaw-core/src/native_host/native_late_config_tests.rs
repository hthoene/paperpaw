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
    string::{String, ToString},
    vec::Vec,
};
use paperpaw_sdk::SettingValue;

/// A kernel built with `config` as the user's settings, and `factory.flag` (a
/// switch that is on unless said otherwise) declared by a native plugin after
/// it. The warnings of the whole start.
fn kernel_with(config: &str) -> (crate::Kernel, Vec<String>) {
    reset(NORMAL);
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
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
