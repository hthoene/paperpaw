//! Every text the native declarations ask for exists in both languages.
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn the_settings_have_words() {
    let source = include_str!("../declarations.rs");
    let mut asked = 0;
    for part in source.split('"').skip(1).step_by(2) {
        // `store.open` is the id of the action (its title is `store.title`),
        // `store.offer` and `store.templates` the ids of the pages the setup
        // steps open.
        if part.starts_with("store.")
            && !part.ends_with('.')
            && !["store.open", "store.offer", "store.templates"].contains(&part)
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

#[test]
fn the_templates_and_their_notices_have_words() {
    use paperpaw_plugin_store::native_template::TEMPLATES;
    let mut keys = vec![
        "store.template.title",
        "store.template.help",
        "store.template.hint",
        "store.template.chosen",
        "store.template.keep",
        "store.template.adding",
        "store.template.switched-on",
        "store.template.applied",
        "store.template.later",
    ];
    for template in &TEMPLATES {
        keys.push(template.name);
        keys.push(template.help);
    }
    for key in keys {
        for (name, catalog) in [("en", EN), ("de", DE)] {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{key} = "))),
                "{key} missing in {name}"
            );
        }
    }
}
