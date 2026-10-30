//! The shared function table's list and the sources agree (prototype of
//! option B2, #44): `tools/shared-table.py --check` says the same, with
//! the function each name stands at.

use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn the_list_and_the_sources_agree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut declared: BTreeMap<String, u32> = BTreeMap::new();
    for krate in ["paperpaw-text", "paperpaw-sdk"] {
        let mut stack = vec![root.join(krate).join("src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for line in text.lines() {
                        if let Some(rest) = line.split("} @ ").nth(1) {
                            *declared.entry(rest.trim().to_string()).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    let list = std::fs::read_to_string(root.join("paperpaw-native/shared.list")).unwrap();
    let listed: Vec<&str> = list
        .lines()
        .map(|l| l.split('#').next().unwrap().trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(listed.len() as u32, paperpaw_sdk::shared::COUNT);
    for name in &listed {
        assert_eq!(declared.get(*name), Some(&1), "{name} is declared once");
    }
    assert_eq!(declared.len(), listed.len(), "{declared:?}");
}
