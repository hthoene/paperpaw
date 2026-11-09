//! Every text the native library asks for exists in both languages.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]

const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

#[test]
fn every_key_the_native_plugin_asks_for_is_in_both_languages() {
    let sources = [
        include_str!("../src/native_plugin.rs"),
        include_str!("../src/native_plugin/books.rs"),
        include_str!("../src/native_plugin/names.rs"),
        include_str!("../src/native_plugin/open.rs"),
        include_str!("../src/native_plugin/sheet.rs"),
        include_str!("../src/native_cells.rs"),
        include_str!("../src/native_sheet.rs"),
        include_str!("../src/native_states.rs"),
        include_str!("../src/native_model.rs"),
        include_str!("../src/native_shelves.rs"),
        include_str!("../src/native_info.rs"),
        include_str!("../declarations.rs"),
    ];
    let mut asked = 0;
    for source in sources {
        for part in source.split('"').skip(1).step_by(2) {
            if part.starts_with("files.")
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'.')
                && !part.ends_with('.')
                && !matches!(
                    part,
                    "files.browser"
                        | "files.names"
                        | "files.shelf"
                        | "files.shelves"
                        | "files.favourites"
                        | "files.sort"
                        | "files.archive"
                        | "files.view"
                        | "files.covers"
                        | "files.sort.recent"
                        | "files.sort.name"
                        | "files.view.list"
                        | "files.view.covers"
                        | "files.covers.mono"
                        | "files.covers.gray"
                        | "files.books"
                        | "files.archive"
                        | "files.menu"
                )
            {
                asked += 1;
                for (name, catalog) in [("en", EN), ("de", DE)] {
                    // A text with a count has a form for one and for the others.
                    assert!(
                        catalog.lines().any(|l| l.starts_with(&format!("{part} = "))
                            || l.starts_with(&format!("{part}.one = "))),
                        "{part} missing in {name}"
                    );
                }
            }
        }
    }
    assert!(asked >= 30, "{asked}");
}

#[test]
fn the_book_count_has_both_plural_forms_in_both_languages() {
    for catalog in [EN, DE] {
        for key in ["files.books.one", "files.books.other"] {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{key} = "))),
                "{key}"
            );
        }
    }
}

#[test]
fn the_values_of_the_settings_have_words() {
    for key in [
        "files.sort.recent",
        "files.sort.name",
        "files.view.list",
        "files.view.covers",
        "files.covers.mono",
        "files.covers.gray",
        "files.shelves",
        "files.favourites",
    ] {
        for catalog in [EN, DE] {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{key} = "))),
                "{key}"
            );
        }
    }
}
