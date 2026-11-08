//! Every text the native declarations ask for exists in both languages.
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn the_setting_and_the_panel_have_words() {
    let source = include_str!("../declarations.rs");
    let mut asked = 0;
    for part in source.split('"').skip(1).step_by(2) {
        if part.starts_with("translate.") && !part.ends_with('.') && part != "translate.panel" {
            asked += 1;
            for (name, catalog) in [("en", EN), ("de", DE)] {
                assert!(
                    catalog
                        .lines()
                        .any(|l| l.starts_with(&format!("{part} = "))),
                    "{part} missing in {name}"
                );
            }
        }
    }
    // The setting's title and help, its section, the panel and the 32 languages.
    assert!(asked >= 36, "{asked}");
}

const CORE_EN: &str = include_str!("../../../crates/paperpaw-core/i18n/en.ini");
const CORE_DE: &str = include_str!("../../../crates/paperpaw-core/i18n/de.ini");

/// The texts the panel and the plugin ask for in their code: the plugin's own
/// are in its catalogs, the shared ones (the offer to go online, the hint where
/// a password cannot be saved) in the core's.
#[test]
fn the_panel_and_the_plugin_have_every_text_they_ask_for_in_both_languages() {
    let mut own = 0;
    let mut shared = 0;
    for source in [
        include_str!("../src/native_panel.rs"),
        include_str!("../src/native_plugin.rs"),
        include_str!("../src/native_ask.rs"),
    ] {
        for part in source.split('"').skip(1).step_by(2) {
            let (catalogs, count) =
                if part.starts_with("translate.") && !part.ends_with('.') && part != "translate.to"
                {
                    ([("en", EN), ("de", DE)], &mut own)
                } else if part.starts_with("core.offline.") || part == "core.secrets-unsupported" {
                    ([("en", CORE_EN), ("de", CORE_DE)], &mut shared)
                } else {
                    continue;
                };
            *count += 1;
            for (name, catalog) in catalogs {
                assert!(
                    catalog
                        .lines()
                        .any(|l| l.starts_with(&format!("{part} = "))),
                    "{part} missing in {name}"
                );
            }
        }
    }
    // The seven notes, the service's name and the key's label; the offer's text
    // and buttons and the hint.
    assert!(own >= 9, "{own}");
    assert!(shared >= 4, "{shared}");
}
