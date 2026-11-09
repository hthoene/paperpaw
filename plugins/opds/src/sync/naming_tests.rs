//! File names of downloaded books, for any title, authors and address a
//! catalog sends: always one safe name inside the target folder.

extern crate std;

use alloc::string::String;
use alloc::vec;

use paperpaw_platform::storage::is_valid_path;
use proptest::prelude::*;

use super::book_name;
use crate::feed::Entry;

const EXTENSIONS: [&str; 6] = ["epub", "txt", "pdf", "fb2", "fb2.zip", "cbz"];

/// Text with the characters file systems and paths care about.
fn hostile() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just(String::from("..")),
            Just(String::from("/")),
            Just(String::from("\\")),
            Just(String::from(":")),
            Just(String::from(".")),
            Just(String::from(" ")),
            Just(String::from("\u{0}")),
            Just(String::from("\n")),
            Just(String::from("%2F")),
            Just(String::from("%2e%2e")),
            Just(String::from(" & ")),
            Just(String::from("CON")),
            any::<char>().prop_map(String::from),
            "[a-zA-Z0-9äöüß]{1,8}",
        ],
        0..20,
    )
    .prop_map(|parts| parts.concat())
}

fn checked(name: &str, ext: &str) {
    let path = alloc::format!("Books/{name}");
    assert!(is_valid_path(&path), "{name:?}");
    assert!(name.ends_with(&alloc::format!(".{ext}")), "{name:?}");
    assert!(!name.starts_with('.'), "{name:?}");
    assert!(!name.starts_with(' '), "{name:?}");
    assert!(!name.chars().any(char::is_control), "{name:?}");
    assert!(
        !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']),
        "{name:?}"
    );
    let stem = &name[..name.len() - ext.len() - 1];
    assert!(!stem.trim().is_empty(), "{name:?}");
    assert!(name.chars().count() <= 200 + ext.len() + 1, "{name:?}");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn any_title_gives_a_safe_name_in_the_folder(
        title in hostile(),
        authors in hostile(),
        path in hostile(),
        ext in prop::sample::select(&EXTENSIONS[..]),
    ) {
        let entry = Entry {
            title,
            authors,
            ..Entry::default()
        };
        let url = alloc::format!("https://catalog.example/{path}");
        checked(&book_name(&entry, &url, ext), ext);
    }

    #[test]
    fn any_address_gives_a_safe_name_for_an_untitled_book(
        url in ".{0,80}",
        ext in prop::sample::select(&EXTENSIONS[..]),
    ) {
        checked(&book_name(&Entry::default(), &url, ext), ext);
    }
}

#[test]
fn titles_of_dots_and_slashes_fall_back_to_the_address() {
    let entry = Entry {
        title: String::from("../.."),
        ..Entry::default()
    };
    assert_eq!(
        book_name(&entry, "https://a.org/get/%2e%2e%2Fsecret.epub", "epub"),
        "secret.epub"
    );
    assert_eq!(book_name(&entry, "https://a.org/", "epub"), "download.epub");
    assert_eq!(
        book_name(&entry, "javascript:alert(1)", "epub"),
        "download.epub"
    );
}
