//! The device's defaults reach the settings a native plugin declares: the
//! kernel is built before the plugin is hosted, so a default for a setting
//! nobody has declared yet is kept and set as the plugin is published (the
//! `keyboard.size` of the X4 and X4 Pro profiles).
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::{
    native_screen_install::{ARMS, install},
    *,
};
use paperpaw_sdk::SettingValue;

/// A kernel built with the device's `defaults`, and `factory.flag` (a switch
/// that is on unless said otherwise) declared by a native plugin after it.
fn kernel_with(defaults: &[(&str, &str)]) -> crate::Kernel {
    reset(NORMAL);
    let mut builder = crate::Kernel::builder(crate::tests::DEVICE);
    for (id, value) in defaults {
        builder = builder.setting_default_text(id, value);
    }
    let mut kernel = builder.build().unwrap();
    // Not declared yet: the default waits and nothing is warned about.
    assert!(kernel.shared.settings.spec("factory.flag").is_none());
    assert!(
        !kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains("factory.flag")),
    );
    ARMS.with(|arms| arms.set(1));
    let result = install(&mut kernel, 0);
    ARMS.with(|arms| arms.set(0));
    result.unwrap();
    kernel
}

#[test]
fn a_device_default_reaches_a_setting_the_native_plugin_declares_later() {
    let kernel = kernel_with(&[("factory.flag", "off")]);
    assert_eq!(
        kernel.shared.settings.get("factory.flag"),
        Some(&SettingValue::Bool(false)),
        "the device's default, not the plugin's own"
    );
}

#[test]
fn without_a_device_default_the_plugins_own_stands() {
    let kernel = kernel_with(&[]);
    assert_eq!(
        kernel.shared.settings.get("factory.flag"),
        Some(&SettingValue::Bool(true))
    );
}

#[test]
fn what_the_user_set_still_wins_and_a_bad_value_is_warned_about() {
    // A value the user sets (or a stored one, applied once the setting
    // exists) is above any default, whatever put the default there.
    let mut kernel = kernel_with(&[("factory.flag", "off")]);
    kernel
        .shared
        .settings
        .set("factory.flag", SettingValue::Bool(true))
        .unwrap();
    assert_eq!(
        kernel.shared.settings.get("factory.flag"),
        Some(&SettingValue::Bool(true))
    );
    // A value the setting does not take is warned about once declared.
    let mut kernel = kernel_with(&[("factory.flag", "banana")]);
    let warnings = kernel.take_warnings();
    assert!(
        warnings.iter().any(|w| w.message.contains("factory.flag")),
        "{warnings:?}"
    );
    // Used up: a later publish does not warn again.
    kernel.apply_late_defaults();
    assert!(kernel.take_warnings().is_empty());
}
