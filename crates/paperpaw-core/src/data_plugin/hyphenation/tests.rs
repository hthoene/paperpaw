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
