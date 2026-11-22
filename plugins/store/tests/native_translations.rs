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

/// The setup's guide shows a screen step's help as `<id>.help` (the registry
/// has no help of a step): each step's help is that key (#472: the offer's
/// step `store.setup` had its help as `store.offer.help`, and showed none).
#[test]
fn the_setup_steps_help_is_what_the_guide_reads() {
    let source = include_str!("../declarations.rs");
    let mut steps = 0;
    for declared in source.split("SetupStepDecl::new(").skip(1) {
        let name = declared.split('"').nth(1).unwrap();
        let help = declared
            .split("step.help = \"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        assert_eq!(help, format!("store.{name}.help"));
        steps += 1;
    }
    assert_eq!(steps, 2);
}
