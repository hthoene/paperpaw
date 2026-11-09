//! Every text the native declarations ask for exists in both languages.
const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn the_screen_and_the_provider_have_words() {
    let source = include_str!("../declarations.rs");
    let mut asked = 0;
    for part in source.split('"').skip(1).step_by(2) {
        if part.starts_with("wikipedia.") && !part.ends_with('.') && part != "wikipedia.lookup" {
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

#[test]
fn the_about_line_fits_the_stage_reply_in_both_languages() {
    for (name, catalog) in [("en", EN), ("de", DE)] {
        let line = catalog
            .lines()
            .find_map(|l| l.strip_prefix("wikipedia.about = "))
            .unwrap_or_else(|| panic!("wikipedia.about missing in {name}"));
        assert!(line.len() <= 512, "{name}: {}", line.len());
        assert!(line.contains("CC BY-SA"), "{name}");
    }
}
