//! German is a language pack (#251): one compiled catalog of every German
//! `i18n/**/de.ini` of the kernel, the app, the shells and the plugins. The
//! interface-7 plugins' texts come through it: every message of every source
//! is in the pack, the same.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::{Path, PathBuf};
use std::rc::Rc;

use paperpaw_i18n::{Catalog, CatalogFile};

struct Bytes(Vec<u8>);

impl CatalogFile for Bytes {
    fn read_exact_at(&self, offset: u32, buf: &mut [u8]) -> bool {
        let start = offset as usize;
        match self.0.get(start..start + buf.len()) {
            Some(bytes) => {
                buf.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }

    fn size(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap()
    }
}

/// Every German catalog of the repository, as `tools/language-pack.py` finds
/// them.
fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                walk(&path, found);
            } else if path.file_name().is_some_and(|n| n == "de.ini") {
                found.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    for group in ["crates", "plugins", "shells"] {
        let Ok(members) = std::fs::read_dir(root.join(group)) else {
            continue;
        };
        for member in members.flatten() {
            walk(&member.path().join("i18n"), &mut found);
        }
    }
    found.sort();
    found
}

#[test]
fn every_german_message_of_every_plugin_comes_through_the_pack() {
    let files = sources();
    assert!(files.len() > 40, "{} German catalogs", files.len());
    let texts: Vec<String> = files
        .iter()
        .map(|file| std::fs::read_to_string(file).unwrap())
        .collect();
    let merged = Catalog::merge_sources(texts.iter().map(String::as_str));
    let mut pack = Catalog::open(Rc::new(Bytes(Catalog::compile(&merged).unwrap()))).unwrap();
    assert_eq!(pack.language(), "de");
    assert_eq!(pack.name(), "Deutsch");
    let mut seen = std::collections::HashSet::new();
    let mut count = 0;
    for (file, text) in files.iter().zip(&texts) {
        let source = Catalog::parse(text.clone()).unwrap();
        for key in source.keys() {
            assert!(seen.insert(key.to_string()), "{key} is in two catalogs");
            assert_eq!(
                pack.get(key),
                source.get(key),
                "{key} of {} differs in the pack",
                file.display()
            );
            count += 1;
            // What is read from a catalog on the card is kept until the
            // kernel lets it go, and no more than `MAX_READ` is read.
            if count % 20 == 0 {
                pack.forget_read();
            }
        }
    }
    assert!(count > 3000, "{count} messages");
}

/// Without the feature the program has no German of its own: the language
/// chosen reads English until the pack is on the card.
#[cfg(not(feature = "german"))]
#[test]
fn without_the_feature_the_program_has_no_german_built_in() {
    assert!(
        paperpaw_app::CORE_MESSAGES
            .iter()
            .all(|table| table.language != "de")
    );
    assert!(
        paperpaw_app::BUILT_CATALOGS
            .iter()
            .all(|table| table.language != "de")
    );
}
