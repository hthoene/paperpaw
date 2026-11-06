//! The packages as built from the repository's sources, and the checks'
//! refusals on folders made wrong on purpose.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::*;
use crate::check::{path_problem, section};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// All thirteen built once, into a folder of the target directory.
fn built() -> &'static Path {
    static OUT: OnceLock<PathBuf> = OnceLock::new();
    OUT.get_or_init(|| {
        let out = std::env::temp_dir().join(format!("pp-hyphenation-packs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        build_all(&root(), &out).unwrap();
        out
    })
}

/// A copy of one built package to damage.
fn copy_of(id: &str, name: &str) -> PathBuf {
    let to = built().join(format!("damaged-{name}")).join(id);
    let _ = fs::remove_dir_all(&to);
    copy_dir(&built().join(id), &to);
    to
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn problems(dir: &Path) -> Vec<String> {
    check(dir).into_iter().map(|p| p.0).collect()
}

fn mentions(problems: &[String], what: &str) -> bool {
    problems.iter().any(|p| p.contains(what))
}

#[test]
fn thirteen_packages_each_in_its_own_folder_and_all_clean() {
    assert_eq!(PACKAGES.len(), 13);
    for package in &PACKAGES {
        let found = problems(&built().join(package.id));
        assert!(found.is_empty(), "{}: {found:?}", package.id);
    }
    let mut ids: Vec<&str> = PACKAGES.iter().map(|p| p.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 13);
}

#[test]
fn ids_are_the_interface_6_plugins_and_the_source_is_there() {
    for package in &PACKAGES {
        let source = root().join("plugins").join(package.id);
        let cargo = fs::read_to_string(source.join("Cargo.toml")).unwrap();
        assert!(cargo.contains(&format!("name = \"paperpaw-plugin-{}\"", package.id)));
        assert!(source.join("patterns").join(package.patterns).exists());
    }
}

#[test]
fn a_package_is_newer_than_the_code_plugin_it_replaces() {
    let new: Vec<u32> = VERSION.split('.').map(|p| p.parse().unwrap()).collect();
    for package in &PACKAGES {
        let cargo =
            fs::read_to_string(root().join("plugins").join(package.id).join("Cargo.toml")).unwrap();
        let old = cargo
            .lines()
            .find_map(|l| l.strip_prefix("version = "))
            .unwrap()
            .trim_matches('"');
        let old: Vec<u32> = old.split('.').map(|p| p.parse().unwrap()).collect();
        assert!(new > old, "{}: {VERSION} is not newer", package.id);
    }
}

/// What the build script of the interface-6 plugin compiles, so that the
/// package's table is the same table.
#[test]
fn inputs_follow_the_interface_6_build_scripts() {
    for package in &PACKAGES {
        let script =
            fs::read_to_string(root().join("plugins").join(package.id).join("build.rs")).unwrap();
        let (left, right) = package.minimums;
        let own = format!("patterns/{}", package.patterns);
        assert!(script.contains(&own), "{}: patterns", package.id);
        if package.id == "hyphenation" {
            assert!(script.contains(&format!("{left},")) && script.contains(&format!("{right},")));
        } else {
            assert!(
                script.contains(&format!("const LEFT: u8 = {left};")),
                "{}",
                package.id
            );
            assert!(
                script.contains(&format!("const RIGHT: u8 = {right};")),
                "{}",
                package.id
            );
        }
        match package.exceptions {
            Some(name) => assert!(
                script.contains(&format!("patterns/{name}")),
                "{}",
                package.id
            ),
            None => assert!(!script.contains("hyp.txt"), "{}: exceptions", package.id),
        }
    }
}

#[test]
fn languages_are_those_the_code_plugin_registers() {
    for package in &PACKAGES {
        let lib =
            fs::read_to_string(root().join("plugins").join(package.id).join("src/lib.rs")).unwrap();
        for &(tag, _) in package.languages {
            assert!(lib.contains(&format!("\"{tag}\"")), "{}: {tag}", package.id);
        }
    }
}

#[test]
fn every_table_is_pphp_v1_within_the_cap_with_the_minimums_it_was_made_for() {
    for package in &PACKAGES {
        for stem in package.tables() {
            let file = built()
                .join(package.id)
                .join(format!("hyphenation/{stem}.pphp"));
            let bytes = fs::read(&file).unwrap();
            assert!(bytes.starts_with(b"PPHP\x01"));
            assert!(
                bytes.len() as u64 <= MAX_TABLE,
                "{}: {} bytes",
                package.id,
                bytes.len()
            );
            let patterns = paperpaw_hyphen::Patterns::new(&bytes).unwrap();
            let (left, right) = package.minimums;
            assert_eq!(
                patterns.minimums(),
                (left as usize, right as usize),
                "{}",
                package.id
            );
        }
    }
}

#[test]
fn the_german_table_breaks_a_long_word() {
    let bytes = fs::read(built().join("hyphenation-de/hyphenation/de.pphp")).unwrap();
    let patterns = paperpaw_hyphen::Patterns::new(&bytes).unwrap();
    let mut breaks = Vec::new();
    patterns.hyphenate("Donaudampfschifffahrtsgesellschaft", &mut |at| {
        breaks.push(at)
    });
    assert!(breaks.len() >= 4, "{breaks:?}");
}

#[test]
fn norwegian_names_one_table_for_both_tags() {
    let dir = built().join("hyphenation-nb");
    let ini = fs::read_to_string(dir.join("plugin.ini")).unwrap();
    assert_eq!(
        section(&ini, "hyphenation"),
        [("nb", "hyphenation/nb.pphp"), ("no", "hyphenation/nb.pphp")]
    );
    let tables: Vec<_> = fs::read_dir(dir.join("hyphenation"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(
        tables
            .iter()
            .filter(|e| e.path().extension().is_some_and(|x| x == "pphp"))
            .count(),
        1
    );
}

#[test]
fn catalogs_and_texts_are_the_interface_6_plugins_unchanged() {
    for package in &PACKAGES {
        for file in ["i18n/en.ini", "i18n/de.ini", "how/en.txt", "how/de.txt"] {
            let old = fs::read(root().join("plugins").join(package.id).join(file)).unwrap();
            let new = fs::read(built().join(package.id).join(file)).unwrap();
            assert_eq!(old, new, "{}: {file}", package.id);
        }
    }
}

#[test]
fn the_paths_rule() {
    assert_eq!(path_problem("hyphenation/de.pphp"), None);
    assert_eq!(path_problem("a/b/c.pphp"), None);
    assert!(path_problem("a/b/c/d.pphp").is_some());
    assert!(path_problem("../x.pphp").is_some());
    assert!(path_problem("/x.pphp").is_some());
    assert!(path_problem("x/.hidden").is_some());
    assert!(path_problem("x y.pphp").is_some());
    assert!(path_problem("x//y").is_some());
    assert!(path_problem(&"a".repeat(101)).is_some());
    assert_eq!(path_problem(&"a".repeat(100)), None);
}

fn edit(dir: &Path, file: &str, change: impl Fn(String) -> String) {
    let path = dir.join(file);
    fs::write(&path, change(fs::read_to_string(&path).unwrap())).unwrap();
}

#[test]
fn a_table_that_is_not_pphp_v1_is_refused() {
    let dir = copy_of("hyphenation-da", "magic");
    let path = dir.join("hyphenation/da.pphp");
    let mut bytes = fs::read(&path).unwrap();
    bytes[..5].copy_from_slice(b"QIHP\x01");
    fs::write(&path, bytes).unwrap();
    assert!(mentions(&problems(&dir), "not a PPHP version 1 table"));
}

#[test]
fn a_table_whose_header_does_not_fit_is_refused() {
    let dir = copy_of("hyphenation-da", "truncated");
    let path = dir.join("hyphenation/da.pphp");
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, &bytes[..40]).unwrap();
    assert!(mentions(&problems(&dir), "header does not fit"));
}

#[test]
fn a_table_over_the_cap_is_refused() {
    let dir = copy_of("hyphenation-da", "oversize");
    let path = dir.join("hyphenation/da.pphp");
    let mut bytes = fs::read(&path).unwrap();
    bytes.resize(MAX_TABLE as usize + 1, 0);
    fs::write(&path, bytes).unwrap();
    assert!(mentions(&problems(&dir), "at most"));
    // Exactly the cap passes the size rule (the header still fits).
    let mut bytes = fs::read(&path).unwrap();
    bytes.truncate(MAX_TABLE as usize);
    fs::write(&path, bytes).unwrap();
    assert!(!mentions(&problems(&dir), "at most"));
}

#[test]
fn the_section_is_checked() {
    let dir = copy_of("hyphenation-da", "section");
    edit(&dir, "plugin.ini", |s| {
        s.replace("da = hyphenation/da.pphp", "da = hyphenation/missing.pphp")
    });
    assert!(mentions(&problems(&dir), "does not exist"));
    edit(&dir, "plugin.ini", |s| {
        s.replace("hyphenation/missing.pphp", "../da.pphp")
    });
    assert!(mentions(&problems(&dir), "dot-leading"));
    edit(&dir, "plugin.ini", |s| {
        s.replace(
            "../da.pphp",
            "hyphenation/da.pphp\nDA = hyphenation/da.pphp",
        )
    });
    assert!(mentions(&problems(&dir), "the language twice"));
    edit(&dir, "plugin.ini", |s| {
        s.replace("\nDA = hyphenation/da.pphp", "\nd_a = hyphenation/da.pphp")
    });
    assert!(mentions(&problems(&dir), "tag not [A-Za-z0-9-]"));
}

#[test]
fn more_than_eight_languages_are_refused() {
    let dir = copy_of("hyphenation-da", "many");
    let lines: String = (0..9)
        .map(|i| format!("l{i} = hyphenation/da.pphp\n"))
        .collect();
    edit(&dir, "plugin.ini", |s| {
        s.replace("da = hyphenation/da.pphp\n", &lines)
    });
    assert!(mentions(&problems(&dir), "at most 8"));
}

#[test]
fn the_manifest_is_checked() {
    let dir = copy_of("hyphenation-sv", "manifest");
    edit(&dir, "plugin.ini", |s| {
        s.replace("formats = 5", "formats = 4")
    });
    assert!(mentions(&problems(&dir), "formats must be 5"));
    edit(&dir, "plugin.ini", |s| {
        s.replace("id = hyphenation-sv", "id = hyphenation-xx")
    });
    assert!(mentions(&problems(&dir), "id is not hyphenation-sv"));
    edit(&dir, "plugin.ini", |s| {
        s.replace("version = 2.0.0", "version = 2.0")
    });
    assert!(mentions(&problems(&dir), "is not x.y.z"));
    edit(&dir, "plugin.ini", |s| {
        s.replace("description = ", "description = a \u{2014} b ")
    });
    assert!(mentions(&problems(&dir), "em or en dash"));
}

#[test]
fn a_missing_german_catalog_or_text_is_refused() {
    let dir = copy_of("hyphenation-sv", "catalog");
    fs::remove_file(dir.join("i18n/de.ini")).unwrap();
    fs::remove_file(dir.join("how/en.txt")).unwrap();
    let found = problems(&dir);
    assert!(mentions(&found, "i18n/de.ini is missing"));
    assert!(mentions(&found, "how/en.txt is missing"));
}

#[test]
fn a_name_that_differs_from_the_catalog_is_refused() {
    let dir = copy_of("hyphenation-sv", "name");
    edit(&dir, "plugin.ini", |s| {
        s.replace("name = Hyphenation (Swedish)", "name = Swedish")
    });
    assert!(mentions(
        &problems(&dir),
        "differs from hyphenation-sv.plugin.name"
    ));
}

#[test]
fn files_outside_the_layout_and_unused_tables_are_refused() {
    let dir = copy_of("hyphenation-sv", "extra");
    fs::write(dir.join("code.ppn"), b"x").unwrap();
    fs::write(dir.join("hyphenation/other.pphp"), b"x").unwrap();
    fs::write(dir.join(".hidden"), b"x").unwrap();
    let found = problems(&dir);
    assert!(mentions(
        &found,
        "code.ppn: a file that PaperPaw does not read here"
    ));
    assert!(mentions(&found, "other.pphp: no language names this table"));
    assert!(mentions(&found, ".hidden: not a plain file name"));
}

#[test]
fn the_cap_constants_are_the_contract_ones() {
    assert_eq!(
        (MAX_LANGUAGES, MAX_TABLE, MAX_PATH, FORMATS),
        (8, 2 * 1024 * 1024, 100, 5)
    );
}
