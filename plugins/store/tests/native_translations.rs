//! Every text the native declarations ask for exists in both languages.
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn the_settings_have_words() {
    let source = include_str!("../declarations.rs");
    let mut asked = 0;
    for part in source.split('"').skip(1).step_by(2) {
        // `store.open` is the id of the action (its title is `store.title`),
        // `store.offer` the id of the page the setup step opens.
        if part.starts_with("store.")
            && !part.ends_with('.')
            && !["store.open", "store.offer"].contains(&part)
        {
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
    assert!(asked >= 4, "{asked}");
}
