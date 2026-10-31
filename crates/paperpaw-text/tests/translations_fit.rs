//! Every character of every translation is one the font packs carry
//! (`charset::named("latin")`): on devices that draw from packs (the X4) any other
//! shows as a question mark.

use std::fs;
use std::path::Path;

use paperpaw_text::charset;

#[test]
fn translations_use_only_packed_characters() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut missing = Vec::new();
    for parent in ["crates", "plugins"] {
        let Ok(entries) = fs::read_dir(root.join(parent)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(files) = fs::read_dir(entry.path().join("i18n")) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|e| e != "ini") {
                    continue;
                }
                let text = fs::read_to_string(&path).unwrap();
                for (n, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with('#') {
                        continue;
                    }
                    for c in line.chars() {
                        if !charset::named("latin")
                            .unwrap()
                            .iter()
                            .any(|range| range.contains(&c))
                        {
                            missing.push(format!("{}:{}: {c:?}", path.display(), n + 1));
                        }
                    }
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "not in the font packs:\n{}",
        missing.join("\n")
    );
}
