//! The interface's letters are inked as `core.ui-text-curve` says: the
//! kernel makes it of the setting and the language, and the UI font's
//! handles carry it, so no plugin parses the setting.

use super::*;
use paperpaw_text::{TextInk, ToneCurve};

fn ui_ink(kernel: &Kernel) -> TextInk {
    kernel
        .shared
        .fonts
        .role(paperpaw_sdk::builtin::fonts::UI.as_str())
        .unwrap()
        .text_ink()
}

fn set(kernel: &mut Kernel, id: &Id, value: &str) {
    kernel.apply(Ask::SetSetting(
        id.clone(),
        SettingValue::Text(value.to_string().into()),
    ));
    // (There is no card to save it to here.)
    let _ = kernel.take_warnings();
}

#[test]
fn the_ui_font_carries_the_ink_of_the_setting_and_the_language() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            crate::tests::font_plugin_manifest(),
            crate::tests::font_plugin(),
        )
        .build()
        .unwrap();
    assert_eq!(ui_ink(&kernel), TextInk::DEFAULT);
    let curve = settings::UI_TEXT_CURVE.id().clone();
    set(&mut kernel, &curve, "contrast:2");
    assert_eq!(ui_ink(&kernel).tone(), ToneCurve::contrast(2));
    // Another font role is not inked by it.
    let reading = kernel
        .shared
        .fonts
        .role(paperpaw_sdk::builtin::fonts::READING.as_str())
        .unwrap();
    assert_eq!(reading.text_ink(), TextInk::DEFAULT);
    // An entry for a script applies where the language is written in it,
    // and goes when the language changes.
    set(&mut kernel, &curve, "smooth; arab=sharp:100");
    assert_eq!(ui_ink(&kernel).tone(), ToneCurve::SMOOTH);
    let language = settings::LANGUAGE.id().clone();
    set(&mut kernel, &language, "ar");
    assert_eq!(ui_ink(&kernel).tone(), ToneCurve::SMOOTH.sharp(100));
    set(&mut kernel, &language, "en");
    assert_eq!(ui_ink(&kernel).tone(), ToneCurve::SMOOTH);
    set(&mut kernel, &curve, "");
    assert_eq!(ui_ink(&kernel), TextInk::DEFAULT);
}

/// Where the interface's language runs right to left (its catalog says
/// so), the UI font's handles say its lines do, so a label of both
/// directions starts at the right; the tone stays as set, and the flag
/// goes with the language.
#[test]
fn the_ui_font_says_its_lines_run_right_to_left_in_arabic() {
    let arabic = "[meta]\nlanguage = ar\nname = العربية\ndirection = rtl\n\n[messages]\nx.y = س\n";
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("plugins/arabic/plugin.ini", "[plugin]\nname = Arabic\n")
        .with("plugins/arabic/i18n/ar.ini", arabic);
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            crate::tests::font_plugin_manifest(),
            crate::tests::font_plugin(),
        )
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(!ui_ink(&kernel).is_right_to_left());
    let language = settings::LANGUAGE.id().clone();
    set(&mut kernel, &language, "ar");
    assert!(ui_ink(&kernel).is_right_to_left());
    let curve = settings::UI_TEXT_CURVE.id().clone();
    set(&mut kernel, &curve, "contrast:2");
    assert!(ui_ink(&kernel).is_right_to_left());
    assert_eq!(ui_ink(&kernel).tone(), ToneCurve::contrast(2));
    // Another font role is not the interface's: its first letters say.
    let reading = kernel
        .shared
        .fonts
        .role(paperpaw_sdk::builtin::fonts::READING.as_str())
        .unwrap();
    assert!(!reading.text_ink().is_right_to_left());
    set(&mut kernel, &language, "en");
    assert!(!ui_ink(&kernel).is_right_to_left());
    assert_eq!(ui_ink(&kernel).tone(), ToneCurve::contrast(2));
}
