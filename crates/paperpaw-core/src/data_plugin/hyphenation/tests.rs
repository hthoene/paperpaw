use super::*;

fn read(source: &str, formats: u32) -> (Vec<Table>, Vec<String>) {
    let mut problems = Vec::new();
    let found = tables(source, "plugins/hyphenation-nb", formats, &mut problems);
    (found, problems)
}

fn table(tag: &str, path: &str) -> Table {
    Table {
        tag: tag.to_string(),
        path: path.to_string(),
    }
}

#[test]
fn names_languages_and_shared_files() {
    let source = "[plugin]\nid = hyphenation-nb\n[hyphenation]\nNB = hyphenation/nb.pphp\n\
                  no = hyphenation/nb.pphp\n";
    let (found, problems) = read(source, FORMATS_5);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        found,
        [
            table("nb", "hyphenation/nb.pphp"),
            table("no", "hyphenation/nb.pphp")
        ]
    );
}

#[test]
fn before_formats_5_the_section_is_left_out_once() {
    let source = "[hyphenation]\nde = hyphenation/de.pphp\nde-ch = hyphenation/de.pphp\n";
    let (found, problems) = read(source, 4);
    assert!(found.is_empty());
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("formats 4"), "{}", problems[0]);
}

#[test]
fn bad_lines_are_left_out_and_told() {
    let source = "[hyphenation]\n\
                  de = hyphenation/de.pphp\n\
                  DE = hyphenation/other.pphp\n\
                  d_e = hyphenation/de.pphp\n\
                  de-ch = ../de.pphp\n\
                  fr = /hyphenation/fr.pphp\n\
                  it = hyphenation/.it.pphp\n\
                  pt = a/b/c/d.pphp\n\
                  es = hyphenation/es pphp\n\
                  sv = hyphenation//sv.pphp\n\
                  da =\n";
    let (found, problems) = read(source, FORMATS_5);
    assert_eq!(found, [table("de", "hyphenation/de.pphp")]);
    assert_eq!(problems.len(), 9, "{problems:?}");
    assert!(problems[0].contains(":3: [hyphenation] DE left out, the language is named twice"));
    assert!(
        problems
            .iter()
            .all(|p| p.starts_with("plugins/hyphenation-nb/plugin.ini:"))
    );
}

#[test]
fn at_most_eight_languages() {
    let mut source = String::from("[hyphenation]\n");
    for i in 0..10 {
        source.push_str(&format!("l{i} = hyphenation/t.pphp\n"));
    }
    let (found, problems) = read(&source, FORMATS_5);
    assert_eq!(found.len(), MAX_TABLES);
    assert_eq!(problems.len(), 2);
    assert!(problems.iter().all(|p| p.contains("more languages")));
}

#[test]
fn tag_and_path_limits() {
    let long_tag = "a".repeat(35);
    let longer_tag = "a".repeat(36);
    assert!(valid_tag(&long_tag));
    assert!(!valid_tag(&longer_tag));
    assert!(!valid_tag(""));
    assert!(valid_tag("sr-Latn-RS"));
    let long_path = format!("h/{}", "p".repeat(98));
    assert_eq!(long_path.len(), 100);
    assert!(valid_path(&long_path));
    assert!(!valid_path(&format!("{long_path}x")));
    assert!(valid_path("a/b/c.pphp"));
    assert!(!valid_path("a/b/c/d"));
    assert!(!valid_path("a/"));
    assert!(!valid_path("."));
    assert!(!valid_path(""));
    assert!(!valid_path("a/ü.pphp"));
}

fn tables_of(entries: &[(&str, &str)]) -> Vec<Table> {
    entries.iter().map(|(t, p)| table(t, p)).collect()
}

#[test]
fn the_greatest_id_has_a_language_in_any_order() {
    let base = tables_of(&[("en", "hyphenation/en.pphp")]);
    let gb = tables_of(&[("en", "hyphenation/en-gb.pphp")]);
    let before = tables_of(&[("en", "en.pphp")]);
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0], [0, 2, 1]] {
        let mut t = Tables::default();
        let mut left_out = Vec::new();
        for i in order {
            let (plugin, tables) = match i {
                0 => ("hyphenation", &base),
                1 => ("hyphenation-en-gb", &gb),
                _ => ("en-patterns", &before),
            };
            t.add(plugin, &format!("plugins/{plugin}"), tables).unwrap();
            left_out.extend(t.collisions(plugin).map(|(lost, _)| lost.name()));
        }
        let found = t.find("en").unwrap();
        assert_eq!(found.plugin, "hyphenation-en-gb", "order {order:?}");
        assert_eq!(
            found.path,
            "plugins/hyphenation-en-gb/hyphenation/en-gb.pphp"
        );
        left_out.sort();
        left_out.dedup();
        assert!(
            left_out.contains(&"en-patterns:en".to_string()),
            "{left_out:?}"
        );
        assert!(
            left_out.contains(&"hyphenation:en".to_string()),
            "{left_out:?}"
        );
        assert!(
            !left_out.contains(&"hyphenation-en-gb:en".to_string()),
            "{left_out:?}"
        );
    }
}

#[test]
fn a_plugin_taken_out_gives_its_language_back() {
    let mut t = Tables::default();
    t.add(
        "hyphenation",
        "plugins/hyphenation",
        &tables_of(&[("en", "en.pphp")]),
    )
    .unwrap();
    t.add(
        "hyphenation-en-gb",
        "plugins/gb",
        &tables_of(&[("en", "gb.pphp")]),
    )
    .unwrap();
    assert_eq!(t.find("en").unwrap().plugin, "hyphenation-en-gb");
    t.remove("hyphenation-en-gb");
    assert_eq!(t.find("en-GB").unwrap().path, "plugins/hyphenation/en.pphp");
}

#[test]
fn ids_compare_in_lowercase() {
    assert!(wins("hyphenation-b", "Hyphenation-A"));
    assert!(!wins("Hyphenation-A", "hyphenation-b"));
    assert!(wins("hyphenation-", "hyphenation"));
    assert!(!wins("hyphenation", "hyphenation"));
}

#[test]
fn a_book_language_falls_back_to_shorter_tags_only() {
    let mut t = Tables::default();
    let tables = tables_of(&[("de", "de.pphp"), ("de-ch", "ch.pphp"), ("nb", "nb.pphp")]);
    t.add("hyphenation-de", "plugins/de", &tables).unwrap();
    assert_eq!(t.find("de-CH-1996").unwrap().path, "plugins/de/ch.pphp");
    assert_eq!(t.find("de_AT").unwrap().path, "plugins/de/de.pphp");
    assert_eq!(t.find("DE").unwrap().path, "plugins/de/de.pphp");
    assert!(t.find("no").is_none());
    assert!(t.find("d").is_none());
    assert!(t.find("").is_none());
    assert!(t.find("-").is_none());
    assert!(t.find("ü-de").is_none());
}

#[test]
fn removing_a_plugin_takes_only_its_tables() {
    let mut t = Tables::default();
    let nb = tables_of(&[("nb", "nb.pphp"), ("no", "nb.pphp")]);
    let de = tables_of(&[("de", "de.pphp")]);
    t.add("hyphenation-nb", "plugins/nb", &nb).unwrap();
    t.add("hyphenation-de", "plugins/de", &de).unwrap();
    t.remove("hyphenation-nb");
    assert!(t.find("nb").is_none() && t.find("no").is_none());
    assert_eq!(t.find("de").unwrap().plugin, "hyphenation-de");
}
