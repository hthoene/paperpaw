//! Every text the declarations and the code ask for exists in both languages:
//! the plugin's own (`weather.*`, plural ones by their `.one` and `.other`)
//! and the shared ones of the core (`core.offline.*` for the offer to go online).
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");
const CORE_EN: &str = include_str!("../../../crates/paperpaw-core/i18n/en.ini");
const CORE_DE: &str = include_str!("../../../crates/paperpaw-core/i18n/de.ini");

fn has(catalog: &str, key: &str) -> bool {
    [
        format!("{key} = "),
        format!("{key}.one = "),
        format!("{key}.other = "),
    ]
    .iter()
    .any(|line| catalog.lines().any(|l| l.starts_with(line.as_str())))
}

#[test]
fn the_declarations_and_the_code_have_every_text_they_ask_for_in_both_languages() {
    let (mut own, mut shared) = (0, 0);
    for source in [
        include_str!("../declarations.rs"),
        include_str!("../src/native_plugin.rs"),
        include_str!("../src/native_fetch.rs"),
        include_str!("../src/native_shown.rs"),
        include_str!("../src/native_now.rs"),
        include_str!("../src/native_forecast.rs"),
        include_str!("../src/native_draw.rs"),
        include_str!("../src/native_place.rs"),
    ] {
        for part in source.split('"').skip(1).step_by(2) {
            // Setting ids (`weather.place`), prefixes and file names are no texts.
            let text = part.starts_with("weather.")
                && !part.ends_with('.')
                && part.matches('.').count() >= 2
                && !part.contains(' ');
            if text && !is_id(part) {
                own += 1;
                for (name, catalog) in [("en", EN), ("de", DE)] {
                    assert!(has(catalog, part), "{part} missing in {name}");
                }
            } else if part.starts_with("core.offline.") {
                shared += 1;
                for (name, catalog) in [("en", CORE_EN), ("de", CORE_DE)] {
                    assert!(has(catalog, part), "{part} missing in {name}");
                }
            }
        }
    }
    assert!(own >= 70, "{own}");
    let _ = shared;
}

/// The ids the plugin uses that are no texts: settings, actions, screens,
/// the work and the like.
fn is_id(part: &str) -> bool {
    matches!(
        part,
        "weather.coordinates"
            | "weather.place"
            | "weather.every"
            | "weather.refresh"
            | "weather.choose-place"
            | "weather.place-screen"
            | "weather.now"
            | "weather.forecast"
            | "weather.group"
            | "weather.temperature"
            | "weather.wind"
    )
}
