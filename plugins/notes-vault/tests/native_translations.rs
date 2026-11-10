//! Every text the native declarations ask for exists in both languages.
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn the_group_and_the_setting_have_words() {
    let source = include_str!("../declarations.rs");
    let mut asked = 0;
    for part in source.split('"').skip(1).step_by(2) {
        // `notes-vault.group` and `notes-vault.open` are ids (of the group and of
        // the action the Tools entry and its icon name), not texts.
        let id = part == "notes-vault.group" || part == "notes-vault.open";
        if part.starts_with("notes-vault.") && !part.ends_with('.') && !id {
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
    // The setting's and the screens' titles, and the folder help.
    assert!(asked >= 4, "{asked}");
}
