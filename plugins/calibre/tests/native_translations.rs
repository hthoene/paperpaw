//! Every text the native plugin asks for exists in both languages: the keys in
//! the declarations and in the plugin's code.
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

/// Ids that are not texts (the action's, the tools entry's and the icon's).
const NOT_TEXTS: [&str; 3] = ["calibre.open", "calibre.screen", "calibre.address"];

/// The keys quoted in `source`: `calibre.` names (and the shared `core.` text).
fn keys(source: &str) -> Vec<String> {
    source
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|part| {
            (part.starts_with("calibre.") || *part == "core.secrets-unsupported")
                && !part.ends_with('.')
                && !part.contains(' ')
                && !NOT_TEXTS.contains(part)
        })
        .map(String::from)
        .collect()
}

#[test]
fn every_text_asked_for_has_words_in_both_languages() {
    let mut asked = keys(include_str!("../declarations.rs"));
    asked.extend(keys(include_str!("../src/native_plugin.rs")));
    asked.sort();
    asked.dedup();
    assert!(asked.len() >= 25, "{}", asked.len());
    for key in &asked {
        // The shared text is the kernel's catalogue's.
        if key.starts_with("core.") {
            continue;
        }
        // A plural is asked for by its stem.
        let stem = [key.clone(), format!("{key}.one")];
        for (name, catalog) in [("en", EN), ("de", DE)] {
            assert!(
                stem.iter()
                    .any(|k| catalog.lines().any(|l| l.starts_with(&format!("{k} = ")))),
                "{key} missing in {name}"
            );
        }
    }
}

#[test]
fn no_text_is_left_without_a_use() {
    let used = [
        keys(include_str!("../declarations.rs")),
        keys(include_str!("../src/native_plugin.rs")),
    ]
    .concat();
    for line in EN.lines() {
        let Some((key, _)) = line.split_once(" = ") else {
            continue;
        };
        if !key.starts_with("calibre.") || key.starts_with("calibre.plugin.") {
            continue;
        }
        let stem = key
            .strip_suffix(".one")
            .or(key.strip_suffix(".other"))
            .unwrap_or(key);
        assert!(used.iter().any(|u| u == stem), "{key} is not used");
    }
}
