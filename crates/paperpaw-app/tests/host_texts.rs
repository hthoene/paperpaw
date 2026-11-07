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
];

fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split('{')
        .skip(1)
        .map(|part| part.split_once('}').unwrap().0)
        .collect()
}

#[test]
fn host_prompts_have_both_languages_and_matching_arguments() {
    let en = Catalog::parse(include_str!("../../paperpaw-core/i18n/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../../paperpaw-core/i18n/de.ini")).unwrap();
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
