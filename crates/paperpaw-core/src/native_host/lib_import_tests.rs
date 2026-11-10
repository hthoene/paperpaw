#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The decision of an import on a card in memory: the name, the free name and
//! the replace rule, all before anything is changed.
extern crate std;
use super::*;
use crate::native_host::lib_reads::importer_set;
use paperpaw_sdk::MemoryStorage;
use std::vec;

const FROM: &str = ".paperpaw/plugins/news/dl/issue.epub";

fn request<'a>(name: &'a str, replace: bool, free_name: bool) -> ImportRequest<'a> {
    ImportRequest {
        from: "dl/issue.epub",
        folder: "feed",
        name,
        replace,
        free_name,
    }
}

fn card() -> MemoryStorage {
    MemoryStorage::new().with(FROM, vec![1_u8; 3000])
}

#[test]
fn a_new_name_is_taken_below_the_library_folder_and_the_source_is_the_callers_own() {
    let storage = card();
    let plan = plan(
        &storage,
        "news",
        "Books",
        &request("issue.epub", false, false),
    )
    .unwrap();
    assert_eq!(
        plan,
        Plan {
            from: FROM.into(),
            to: "Books/feed/issue.epub".into(),
            replace: false
        }
    );
    // The library at the card's top, and no folder.
    let at_top = ImportRequest {
        folder: "",
        ..request("issue.epub", false, false)
    };
    assert_eq!(plan_to(&storage, "", &at_top), "issue.epub");
    // Another plugin has no such file.
    assert_eq!(
        super::plan(
            &storage,
            "wallabag",
            "Books",
            &request("issue.epub", false, false)
        ),
        Err(ERR_NOT_FOUND)
    );
    // A name of the library that is only in the card's root: nothing there.
    let missing = ImportRequest {
        from: "dl/none.epub",
        ..request("x.epub", false, false)
    };
    assert_eq!(
        super::plan(&storage, "news", "Books", &missing),
        Err(ERR_NOT_FOUND)
    );
}

fn plan_to(storage: &MemoryStorage, library: &str, request: &ImportRequest<'_>) -> String {
    super::plan(storage, "news", library, request).unwrap().to
}

#[test]
fn an_existing_name_is_state_unless_replace_or_a_free_name_says_otherwise() {
    let storage = card();
    storage
        .write("Books/feed/issue.epub", &[9_u8; 100])
        .unwrap();
    assert_eq!(
        super::plan(
            &storage,
            "news",
            "Books",
            &request("issue.epub", false, false)
        ),
        Err(ERR_STATE)
    );
    // A free name: `issue (2).epub`, then `(3)` while those are taken.
    let free = request("issue.epub", false, true);
    assert_eq!(
        plan_to(&storage, "Books", &free),
        "Books/feed/issue (2).epub"
    );
    storage.write("Books/feed/issue (2).epub", &[9_u8]).unwrap();
    assert_eq!(
        plan_to(&storage, "Books", &free),
        "Books/feed/issue (3).epub"
    );
    // A name without an extension.
    storage.write("Books/feed/readme", &[9_u8]).unwrap();
    let bare = request("readme", false, true);
    assert_eq!(plan_to(&storage, "Books", &bare), "Books/feed/readme (2)");
}

#[test]
fn replace_is_for_the_plugins_own_unchanged_import_only() {
    let storage = card();
    storage
        .write("Books/feed/issue.epub", &[1_u8; 3000])
        .unwrap();
    let replace = request("issue.epub", true, false);
    // The user's file, or another plugin's: denied, whatever the bytes.
    assert_eq!(
        super::plan(&storage, "news", "Books", &replace),
        Err(ERR_DENIED)
    );
    importer_set(&storage, "Books/feed/issue.epub", "wallabag").unwrap();
    assert_eq!(
        super::plan(&storage, "news", "Books", &replace),
        Err(ERR_DENIED)
    );
    // The plugin's own import is replaced, keeping the name.
    importer_set(&storage, "Books/feed/issue.epub", "news").unwrap();
    let plan = super::plan(&storage, "news", "Books", &replace).unwrap();
    assert_eq!(
        (plan.to.as_str(), plan.replace),
        ("Books/feed/issue.epub", true)
    );
    // The user put another file there since: denied again.
    storage
        .write("Books/feed/issue.epub", &[7_u8; 5000])
        .unwrap();
    assert_eq!(
        super::plan(&storage, "news", "Books", &replace),
        Err(ERR_DENIED)
    );
    // `replace` on a name that does not exist is a plain import.
    let other = request("new.epub", true, false);
    let plan = super::plan(&storage, "news", "Books", &other).unwrap();
    assert!(!plan.replace);
}

#[test]
fn a_path_that_would_be_too_long_is_refused_before_anything_is_changed() {
    let storage = card();
    let long = "n".repeat(200);
    let folder = "f".repeat(200);
    let at = ImportRequest {
        folder: &folder,
        ..request(&long, false, false)
    };
    let library = "l".repeat(130);
    assert_eq!(
        super::plan(&storage, "news", &library, &at),
        Err(ERR_NO_SPACE)
    );
}

#[test]
fn a_free_name_is_looked_for_up_to_99_only() {
    let storage = card();
    storage.write("Books/feed/issue.epub", &[9_u8]).unwrap();
    for n in 2..=98 {
        storage
            .write(&alloc::format!("Books/feed/issue ({n}).epub"), &[9_u8])
            .unwrap();
    }
    let free = request("issue.epub", false, true);
    assert_eq!(
        plan_to(&storage, "Books", &free),
        "Books/feed/issue (99).epub"
    );
    storage
        .write("Books/feed/issue (99).epub", &[9_u8])
        .unwrap();
    assert_eq!(
        super::plan(&storage, "news", "Books", &free),
        Err(ERR_STATE)
    );
}
