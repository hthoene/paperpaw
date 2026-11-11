//! `status-lines.txt` and the rotation: version 1 stays readable, what is
//! saved is read back, a file damaged or missing is no choice without a panic,
//! and a tap goes round the lines.
extern crate std;

use alloc::string::{String, ToString};

use paperpaw_sdk::MemoryStorage;
use proptest::prelude::*;

use super::*;

const LINES: &str =
    "core.status-line.items, reader.status-line.progress , reader.status-line.empty";

#[test]
fn the_choice_of_version_1_is_read() {
    let storage =
        MemoryStorage::new().with(FILE, include_str!("../../../testdata/status-lines.txt.v1"));
    assert_eq!(
        load(&storage).unwrap(),
        [
            String::from("reader.status-line.empty"),
            String::from("reader.status-line.progress")
        ]
    );
}

#[test]
fn the_writer_still_writes_version_1() {
    let storage = MemoryStorage::new();
    let chosen = [
        String::from("reader.status-line.empty"),
        String::from("reader.status-line.progress"),
    ];
    save(&storage, &chosen).unwrap();
    let v1 = include_str!("../../../testdata/status-lines.txt.v1").replace("\r\n", "\n");
    assert_eq!(storage.get(FILE).unwrap(), v1.as_bytes());
}

#[test]
fn no_file_and_a_file_too_large_are_no_choice() {
    let none = [String::new(), String::new()];
    assert_eq!(load(&MemoryStorage::new()).unwrap(), none);
    let large = MemoryStorage::new().with(FILE, "x".repeat(5000));
    assert_eq!(load(&large).unwrap(), none);
}

#[test]
fn the_current_line_is_the_chosen_one_or_the_first() {
    assert_eq!(
        current(LINES, "reader.status-line.progress"),
        "reader.status-line.progress"
    );
    // A line that is no longer among them, or none chosen: the first.
    assert_eq!(current(LINES, "gone.line"), "core.status-line.items");
    assert_eq!(current(LINES, ""), "core.status-line.items");
    // No lines: none.
    assert_eq!(current("", "x"), "");
    assert_eq!(current(" , ,", "x"), "");
}

#[test]
fn a_tap_goes_round_the_lines() {
    let mut at = String::new();
    let mut seen = std::vec::Vec::new();
    for _ in 0..4 {
        at = next(LINES, &at).to_string();
        seen.push(at.clone());
    }
    assert_eq!(
        seen,
        [
            "reader.status-line.progress",
            "reader.status-line.empty",
            "core.status-line.items",
            "reader.status-line.progress"
        ]
    );
    // One line: it stays; none: none.
    assert_eq!(next("only.line", "only.line"), "only.line");
    assert_eq!(next("", ""), "");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    #[test]
    fn a_choice_saved_is_read_back(top in "[a-z.\\-]{0,40}", bottom in "[a-z.\\-]{0,40}") {
        let storage = MemoryStorage::new();
        save(&storage, &[top.clone(), bottom.clone()]).unwrap();
        prop_assert_eq!(load(&storage).unwrap(), [top, bottom]);
    }

    #[test]
    fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(any::<u8>(), 0..1100)) {
        let storage = MemoryStorage::new();
        storage.write(FILE, &data).unwrap();
        let [top, bottom] = load(&storage).unwrap();
        prop_assert!(!top.contains('\n') && !bottom.contains('\n'));
        prop_assert_eq!(top.trim(), top.to_string());
    }
}
