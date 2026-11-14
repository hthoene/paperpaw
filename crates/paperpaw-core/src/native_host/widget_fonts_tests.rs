//! The font setting the host gives every widget (#262), seen on the widget
//! pack: made once the plugins are hosted, described and not kept, with the
//! value the configuration has, and the family chosen kept open.
//!
//! Linux x86_64 only, as the pack's image is (`described_settings_tests`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
extern crate std;
use super::super::shell::tests::FAIL_NTH;
use super::tests::{add_pack, admit_pack, configured, held, kernel};
use crate::font_refresh::widget_fonts::{ALL, BOOK, GLOBAL, MENUS};
use paperpaw_plugin_widgets::{NATIVE_GROUPS, NATIVE_SETTINGS};
use paperpaw_sdk::{FontFace, Id, Instant, MemoryStorage, SettingKind, SettingValue, Settings};
use std::rc::Rc;
use std::string::String;
use std::vec::Vec;

fn pack() -> Vec<u8> {
    let mut pack = paperpaw_text::PackBuilder::new();
    pack.add_strike(
        10.0,
        paperpaw_text::LineMetrics::new(8.0, 2.0, 12.0),
        std::vec![('a', paperpaw_text::Glyph::empty(5.0))],
    );
    pack.build()
}

/// Puts the families `Slab` and `Hand` on the card, as a font plugin
/// registering does.
fn add_families(kernel: &mut crate::Kernel) {
    let mut card = MemoryStorage::new();
    for (id, path, family) in [
        ("t.slab", "t/slab.ppfp", "Slab"),
        ("t.hand", "t/hand.ppfp", "Hand"),
    ] {
        card = card.with(path, pack());
        let face = FontFace::new(Id::new_static(id), path, family, false, false);
        kernel.shared.fonts.add_face(face).unwrap();
    }
    kernel.shared.storage = Rc::new(card);
    kernel.offer_font_choices();
}

/// The ids of the widgets' font settings the host made.
fn font_settings(settings: &Settings) -> Vec<String> {
    let mut ids = Vec::new();
    let pick = paperpaw_sdk::Pick::All;
    settings
        .each(pick, |spec| {
            if spec.id.as_str().ends_with(".font") && spec.id.as_str() != GLOBAL {
                ids.push(String::from(spec.id.as_str()));
            }
        })
        .unwrap();
    ids
}

fn choices(settings: &Settings, id: &str) -> Vec<String> {
    match &settings.try_spec(id).unwrap().expect(id).kind {
        SettingKind::Choice(choices) => choices.iter().map(String::from).collect(),
        other => panic!("{id} is {other:?}"),
    }
}

#[test]
fn every_widget_with_settings_gets_a_font_setting_when_its_plugin_is_admitted() {
    for table in [false, true] {
        let mut kernel = kernel(table);
        // There before the start: the shell reports the warnings about the
        // configuration then.
        let early = font_settings(&kernel.shared.settings);
        assert_eq!(early.len(), NATIVE_GROUPS.len(), "table {table}");
        let before = held(&kernel.shared.settings);
        let kept = kernel.shared.settings.kept_specs().count();
        kernel.native_hosted();
        let settings = &kernel.shared.settings;
        // One for every widget of the pack: each has a group of its id.
        let wanted: Vec<String> = NATIVE_GROUPS
            .iter()
            .map(|group| std::format!("{}.font", group.id.as_str()))
            .collect();
        let mut made = font_settings(settings);
        made.sort();
        let mut sorted = wanted.clone();
        sorted.sort();
        assert_eq!(made, sorted, "table {table}");
        assert!(made.len() > 20, "{}", made.len());
        // No spec of them stays on the heap, nor what the check made.
        assert_eq!(held(settings), before);
        assert_eq!(settings.kept_specs().count(), kept);
        assert!(!settings.any_made());
        assert!(!settings.went_short());
        let id = "widgets.day.font";
        let spec = settings.try_spec(id).unwrap().expect(id);
        // In the widget's own group, after the plugin's settings there.
        assert_eq!(spec.group.as_str(), "widgets.day");
        assert_eq!(spec.title, "core.setting.font");
        assert_eq!(spec.default, SettingValue::Text(ALL.into()));
        assert_eq!(choices(settings, id), [ALL, MENUS, BOOK]);
        assert_eq!(settings.text(id), ALL);
        let mut group: Vec<String> = Vec::new();
        settings
            .each(paperpaw_sdk::Pick::All, |spec| {
                if spec.group.as_str() == "widgets.day" {
                    group.push(String::from(spec.id.as_str()));
                }
            })
            .unwrap();
        assert_eq!(group.last().map(String::as_str), Some(id), "{group:?}");
        assert!(group.len() > 1);
        // A font plugin that comes later is offered by every one of them.
        add_families(&mut kernel);
        let settings = &kernel.shared.settings;
        assert_eq!(choices(settings, id), [ALL, MENUS, BOOK, "Slab", "Hand"]);
        assert_eq!(choices(settings, GLOBAL), [MENUS, BOOK, "Slab", "Hand"]);
        // Hosting again (nothing new) gives none twice.
        kernel.offer_widget_fonts();
        assert_eq!(font_settings(&kernel.shared.settings).len(), wanted.len());
    }
}

#[test]
fn a_family_the_configuration_has_for_a_widget_is_taken_and_open_at_the_start() {
    let config =
        "[widgets]\nday.font = Slab\nmoon.font = Gone\nnote.font = core.widgets.font.book\n";
    let mut kernel = configured(false, config);
    // The settings are there as the widgets' plugin is admitted, and a value
    // that fits is set: nothing is "unknown" when the shell reports the
    // warnings, which it does before the start.
    assert_eq!(kernel.shared.settings.text("widgets.note.font"), BOOK);
    let early = kernel.take_warnings();
    assert!(
        early.iter().all(|w| !w.message.contains(".font")),
        "{early:?}"
    );
    // The fonts come with a plugin that registers after the widgets: the
    // value naming one of them waited.
    assert_eq!(kernel.shared.settings.text("widgets.day.font"), ALL);
    add_families(&mut kernel);
    assert!(kernel.shared.fonts.get("t.slab").is_none());
    kernel.native_hosted();
    let settings = &kernel.shared.settings;
    assert_eq!(settings.text("widgets.day.font"), "Slab");
    assert_eq!(settings.text("widgets.note.font"), BOOK);
    assert!(kernel.shared.fonts.get("t.slab").is_some());
    assert!(kernel.shared.fonts.get("t.hand").is_none());
    assert!(!kernel.shared.settings.any_made());
    // A family that is there no more: the widget follows the one for all,
    // and the only warning left is about that value.
    assert_eq!(kernel.shared.settings.text("widgets.moon.font"), ALL);
    let warnings = kernel.take_warnings();
    let about: Vec<&str> = warnings
        .iter()
        .map(|warning| warning.message.as_str())
        .filter(|message| message.contains(".font"))
        .collect();
    assert_eq!(about.len(), 1, "{about:?}");
    assert!(about[0].contains("widgets.moon.font"), "{about:?}");
}

#[test]
fn a_family_chosen_for_a_widget_while_running_is_opened_and_closed_with_the_choice() {
    let mut kernel = kernel(false);
    add_families(&mut kernel);
    kernel.native_hosted();
    let id = "widgets.day.font";
    kernel.set_setting(Instant(0), id, "Slab").unwrap();
    assert_eq!(kernel.shared.settings.text(id), "Slab");
    assert!(kernel.shared.fonts.get("t.slab").is_some());
    assert!(kernel.shared.fonts.get("t.hand").is_none());
    // One placement of the widget in another family.
    kernel
        .set_setting(Instant(0), "widgets.day@2.font", "Hand")
        .unwrap();
    assert!(kernel.shared.fonts.get("t.hand").is_some());
    assert!(kernel.shared.fonts.get("t.slab").is_some());
    kernel.set_setting(Instant(0), id, ALL).unwrap();
    assert!(kernel.shared.fonts.get("t.slab").is_none());
    assert!(kernel.shared.fonts.get("t.hand").is_some());
    assert!(
        kernel
            .set_setting(Instant(0), id, "No such family")
            .is_err()
    );
}

#[test]
fn a_plugin_installed_while_running_gets_its_widgets_font_settings_at_once() {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    add_families(&mut kernel);
    kernel.native_hosted();
    assert!(font_settings(&kernel.shared.settings).is_empty());
    add_pack(&mut kernel, false);
    let settings = &kernel.shared.settings;
    assert_eq!(font_settings(settings).len(), NATIVE_GROUPS.len());
    assert_eq!(
        choices(settings, "widgets.day.font"),
        [ALL, MENUS, BOOK, "Slab", "Hand"]
    );
    // And they go with the plugin.
    kernel.shared.settings.remove_plugin("widgets");
    assert!(font_settings(&kernel.shared.settings).is_empty());
}

#[test]
fn a_font_setting_the_plugin_declared_itself_is_left_to_it() {
    // None of the pack's own settings is a widget's `font`: the host's
    // would hide it.
    assert!(
        NATIVE_SETTINGS
            .iter()
            .all(|spec| !spec.id.as_str().ends_with(".font"))
    );
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    admit_pack(&mut kernel);
    // As if the pack had declared the day's itself.
    let own = paperpaw_sdk::SettingSpec::switch(
        "widgets.day.font",
        "t.own",
        false,
        Id::new_static("widgets.day"),
    );
    kernel.shared.settings.define(own).unwrap();
    kernel.native_hosted();
    let settings = &kernel.shared.settings;
    assert_eq!(font_settings(settings).len(), NATIVE_GROUPS.len());
    assert_eq!(
        settings.get("widgets.day.font"),
        Some(&SettingValue::Bool(false))
    );
    assert_eq!(settings.text("widgets.moon.font"), ALL);
}

#[test]
fn without_room_the_widgets_left_out_follow_the_setting_for_all_and_a_warning_says_so() {
    let mut tried = 0;
    for nth in 1..256 {
        // The pack admitted, its widgets' font settings not given yet.
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        admit_pack(&mut kernel);
        assert!(font_settings(&kernel.shared.settings).is_empty());
        let _ = kernel.take_warnings();
        FAIL_NTH.with(|fail| fail.set(nth));
        kernel.offer_widget_fonts();
        let fired = FAIL_NTH.with(|fail| fail.replace(0)) == 0;
        let made = font_settings(&kernel.shared.settings).len();
        let warned = kernel
            .take_warnings()
            .iter()
            .any(|warning| warning.message.contains("font setting of every widget"));
        if !fired {
            assert_eq!(made, NATIVE_GROUPS.len());
            assert!(!warned);
            break;
        }
        tried += 1;
        // As many as there was room for, and said where any is left out.
        assert!(warned || made == NATIVE_GROUPS.len(), "allocation {nth}");
        // The next time finds room and gives the rest.
        kernel.offer_widget_fonts();
        assert_eq!(
            font_settings(&kernel.shared.settings).len(),
            NATIVE_GROUPS.len(),
            "allocation {nth}"
        );
    }
    assert!(tried > 0, "giving the settings took no allocation");
}
