//! Every text the native declarations ask for exists in both languages (the
//! ids of the plugin's own actions are not texts).
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn the_settings_have_words() {
    let source = include_str!("../declarations.rs");
    let mut asked = 0;
    for part in source.split('"').skip(1).step_by(2) {
        let is_id = matches!(part, "opds.open" | "opds.sync");
        if part.starts_with("opds.") && !part.ends_with('.') && !is_id {
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
    assert!(asked >= 8, "{asked}");
}
