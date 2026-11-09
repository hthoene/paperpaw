//! Every text the native declarations ask for exists in both languages.
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn the_group_and_the_setting_have_words() {
    let source = include_str!("../declarations.rs");
    let mut asked = 0;
    for part in source.split('"').skip(1).step_by(2) {
        // `notes-vault.group` is the group's id, not a text.
        if part.starts_with("notes-vault.") && !part.ends_with('.') && part != "notes-vault.group" {
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
    assert!(asked >= 2, "{asked}");
}
