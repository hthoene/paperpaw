//! Host-owned interface-7 prompts; screens are supplied by the pending host families.
#![allow(clippy::unwrap_used, reason = "catalog regression assertions")]
use paperpaw_i18n::Catalog;
use std::collections::BTreeSet;

const KEYS: &[&str] = &[
    "core.host-confirm.no",
    "core.host-confirm.yes",
    "core.host-install.plugin",
    "core.host-install.font",
    "core.host-install.data",
    "core.host-install.requester",
    "core.host-install.size",
    "core.host-install.signer",
    "core.host-install.key",
    "core.host-install.sideload",
    "core.host-install.older",
    "core.host-install.same",
    "core.host-firmware.question",
    "core.host-firmware.foreign",
    "core.host-firmware.no-undo",
    "core.host-firmware.kept",
    "core.host-firmware.unsigned",
    "core.host-firmware.unknown-key",
    "core.host-firmware.downgrade",
    "core.host-firmware.early-crash",
    "core.firmware.title",
    "core.firmware.title.restart",
    "core.firmware.dual.title",
    "core.firmware.dual.what",
    "core.firmware.dual.yes",
    "core.firmware.asked-by",
    "core.firmware.image",
    "core.firmware.signed",
    "core.firmware.other-key",
    "core.firmware.unsigned",
    "core.firmware.older",
    "core.firmware.foreign.1",
    "core.firmware.foreign.2",
    "core.firmware.foreign.3",
    "core.firmware.foreign.4",
    "core.firmware.yes",
    "core.firmware.yes.restart",
    "core.firmware.no",
    "core.host-remove.question",
    "core.host-remove.keep",
    "core.host-remove.delete",
    "core.host-remove.warning",
    "core.host-wifi.join",
    "core.host-wifi.hotspot",
    "core.host-ui.phone-locked",
    "core.host-ui.typing-stopped",
    "core.host-ui.text-full",
    "core.host-ui.clipboard-full",
    "core.host-ui.clipboard-card",
    "core.host-ui.card-full",
    "core.host-ui.card-error",
    "core.host-permission.library",
    "core.host-library.import-failed",
    "core.host-library.name-taken",
    "core.host-library.replace-denied",
    "core.host-safe-mode.power-escape",
    "core.notice-box",
    "core.screen.open-failed",
    "core.plugin.title",
    "core.plugin.title.remove",
    "core.plugin.what",
    "core.plugin.sideload",
    "core.plugin.uses",
    "core.plugin.yes",
    "core.plugin.yes.remove",
    "core.plugin.remove-data",
    "core.plugin.yes.remove-data",
    "core.plugin.unforgotten",
];

fn placeholders(text: &str) -> Vec<&str> {
    let mut names: Vec<_> = text
        .split('{')
        .skip(1)
        .map(|part| part.split_once('}').unwrap().0)
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn host_prompts_have_both_languages_and_matching_arguments() {
    let en = Catalog::parse(include_str!("../../paperpaw-core/i18n/host/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../../paperpaw-core/i18n/host/de.ini")).unwrap();
    let expected: BTreeSet<_> = KEYS.iter().copied().collect();
    assert_eq!(expected.len(), KEYS.len(), "duplicate expected key");
    assert_eq!(
        en.keys().collect::<BTreeSet<_>>(),
        expected,
        "English key set"
    );
    assert_eq!(
        de.keys().collect::<BTreeSet<_>>(),
        expected,
        "German key set"
    );
    for key in KEYS {
        let english = en
            .get(key)
            .unwrap_or_else(|| panic!("missing English {key}"));
        let german = de
            .get(key)
            .unwrap_or_else(|| panic!("missing German {key}"));
        assert!(!english.trim().is_empty(), "{key}");
        assert!(!german.trim().is_empty(), "{key}");
        assert_eq!(placeholders(english), placeholders(german), "{key}");
    }
    assert_eq!(en.get("core.host-confirm.no"), Some("No"));
    assert_eq!(de.get("core.host-confirm.no"), Some("Nein"));
}

#[test]
fn placeholder_comparison_keeps_occurrence_counts() {
    assert_ne!(placeholders("{name}"), placeholders("{name} {name}"));
    assert_eq!(placeholders("{name} {size}"), placeholders("{size} {name}"));
}

#[test]
fn core_catalogs_fit_flash_indexes() {
    for source in [
        include_str!("../../paperpaw-core/i18n/en.ini"),
        include_str!("../../paperpaw-core/i18n/de.ini"),
        include_str!("../../paperpaw-core/i18n/host/en.ini"),
        include_str!("../../paperpaw-core/i18n/host/de.ini"),
        include_str!("../../paperpaw-core/i18n/settings/en.ini"),
        include_str!("../../paperpaw-core/i18n/settings/de.ini"),
    ] {
        assert!(Catalog::parse(source).unwrap().narrow_index().is_some());
    }
}
