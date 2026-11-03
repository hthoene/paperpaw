//! A plugin the store installs that requires others (`requires` in its
//! `plugin.ini`, e.g. the Arabic menus need the Arabic letters): what the
//! device lacks is offered in the store's request dialog once it is
//! installed, with its size; *Download* fetches it, *Not now* leaves the
//! plugin installed as it is. What is on the device already, what the
//! library does not have, two plugins that require each other, and a
//! chain are handled without a loop.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Shared, Shopping};
use paperpaw_sdk::Storage;

const SIZE: u64 = 1_258_291;

/// The list for the X4: `(id, name)` of each, all of one group.
fn list(items: &[(&str, &str)]) -> String {
    let entries: Vec<String> = items
        .iter()
        .map(|(id, name)| {
            format!(
                r#"{{"id": "{id}", "name": "{name}", "summary": "A pack.",
    "group": "reading", "author": "PaperPaw", "licence": "OFL-1.1", "kind": "data",
    "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 4, "size": {SIZE}}}}}"#
            )
        })
        .collect();
    format!(r#"{{"format": 2, "plugins": [{}]}}"#, entries.join(","))
}

/// The plugin's `plugin.ini`, requiring `requires` (none where empty).
fn manifest(id: &str, name: &str, requires: &str) -> String {
    let requires = if requires.is_empty() {
        String::new()
    } else {
        format!("requires = {requires}\n")
    };
    format!("[plugin]\nid = {id}\nname = {name}\nversion = 1.0.0\nformats = 4\n{requires}")
}

fn page_url(id: &str) -> String {
    format!("https://paperpaw.org/api/v1/plugins/{id}.json?device=x4&lang=en")
}

fn file_url(id: &str) -> String {
    format!("https://paperpaw.org/api/v1/plugins/{id}/1.0.0/plugin.ini")
}

/// The plugin's page at the library: its `plugin.ini` only.
fn page(id: &str, name: &str, manifest: &str) -> String {
    format!(
        r#"{{"format": 2, "plugin": {{"id": "{id}", "name": "{name}",
  "summary": "A pack.", "group": "reading", "author": "PaperPaw",
  "licence": "OFL-1.1", "kind": "data",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 4, "size": {SIZE},
    "files": [{{"path": "plugin.ini", "url": "{id}/1.0.0/plugin.ini",
                "size": {}, "sha1": "{}"}}]}}}}}}"#,
        manifest.len(),
        paperpaw_digest::sha1::hex(manifest.as_bytes())
    )
}

/// Installs `id` by the reader's own steps: its group, the plugin, its
/// page, *Install*.
fn install(shopping: &mut Shopping, id: &str, name: &str, requires: &str) {
    let manifest = manifest(id, name, requires);
    shopping.choose("Reading");
    shopping.choose(name);
    shopping.serve(&page_url(id), page(id, name, &manifest).as_bytes());
    shopping.run(paperpaw_sdk::builtin::actions::CONFIRM);
    shopping.serve(&file_url(id), manifest.as_bytes());
}

/// *Download* chosen in the dialog, the plugin `id` fetched and
/// installed.
fn download(shopping: &mut Shopping, id: &str, name: &str, requires: &str) {
    let manifest = manifest(id, name, requires);
    shopping.choose("Download");
    shopping.serve(&page_url(id), page(id, name, &manifest).as_bytes());
    shopping.serve(&file_url(id), manifest.as_bytes());
}

fn is_installed(shopping: &Shopping, id: &str) -> bool {
    shopping.kernel.plugins().get(id).is_some()
}

#[test]
fn what_the_installed_plugin_requires_is_offered_with_its_size() {
    let mut shopping = Shopping::new(&list(&[
        ("language-ar", "Arabic menus"),
        ("script-arabic", "Arabic letters"),
    ]));
    install(
        &mut shopping,
        "language-ar",
        "Arabic menus",
        "script-arabic",
    );
    assert!(is_installed(&shopping, "language-ar"));
    let shown = shopping.shown();
    assert!(shown.contains("The plugin just installed also"), "{shown}");
    assert!(shown.contains("needs these to work."), "{shown}");
    assert!(shown.contains("Arabic letters (1.2 MB)"), "{shown}");
    assert!(
        shown.contains("Download") && shown.contains("Not now"),
        "{shown}"
    );
    // Nothing is fetched before the reader says so.
    assert!(shopping.requests().is_empty());
    download(&mut shopping, "script-arabic", "Arabic letters", "");
    assert!(is_installed(&shopping, "script-arabic"));
    // Said once; the dialog is gone and nothing is asked again.
    let shown = shopping.shown();
    assert!(!shown.contains("just installed also"), "{shown}");
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/request.txt")
    );
    assert!(shopping.requests().is_empty());
}

#[test]
fn not_now_leaves_the_plugin_installed_and_fetches_nothing() {
    let mut shopping = Shopping::new(&list(&[
        ("language-ar", "Arabic menus"),
        ("script-arabic", "Arabic letters"),
    ]));
    install(
        &mut shopping,
        "language-ar",
        "Arabic menus",
        "script-arabic",
    );
    shopping.choose("Not now");
    assert!(is_installed(&shopping, "language-ar"));
    assert!(!is_installed(&shopping, "script-arabic"));
    assert!(shopping.requests().is_empty());
    assert!(!shopping.shown().contains("just installed also"));
}

#[test]
fn what_is_on_the_device_is_not_asked_for() {
    let storage = Shared::default();
    storage
        .write(
            "plugins/script-arabic/plugin.ini",
            manifest("script-arabic", "Arabic letters", "").as_bytes(),
        )
        .unwrap();
    let mut shopping = Shopping::with(
        Some("x4"),
        None,
        &list(&[
            ("language-ar", "Arabic menus"),
            ("script-arabic", "Arabic letters"),
        ]),
        storage,
    );
    install(
        &mut shopping,
        "language-ar",
        "Arabic menus",
        "script-arabic",
    );
    assert!(is_installed(&shopping, "language-ar"));
    let shown = shopping.shown();
    assert!(!shown.contains("just installed also"), "{shown}");
    assert!(shopping.requests().is_empty());
}

#[test]
fn a_plugin_the_library_does_not_have_is_passed_over() {
    let mut shopping = Shopping::new(&list(&[("language-ar", "Arabic menus")]));
    install(
        &mut shopping,
        "language-ar",
        "Arabic menus",
        "script-arabic",
    );
    assert!(is_installed(&shopping, "language-ar"));
    let shown = shopping.shown();
    assert!(!shown.contains("just installed also"), "{shown}");
    assert!(shopping.requests().is_empty());
}

#[test]
fn a_plugin_without_requirements_asks_for_nothing() {
    let mut shopping = Shopping::new(&list(&[("script-arabic", "Arabic letters")]));
    install(&mut shopping, "script-arabic", "Arabic letters", "");
    assert!(is_installed(&shopping, "script-arabic"));
    assert!(!shopping.shown().contains("just installed also"));
}

#[test]
fn two_plugins_requiring_each_other_end_after_the_second() {
    let mut shopping = Shopping::new(&list(&[("alpha", "Alpha"), ("beta", "Beta")]));
    install(&mut shopping, "alpha", "Alpha", "beta");
    let shown = shopping.shown();
    assert!(shown.contains("Beta (1.2 MB)"), "{shown}");
    download(&mut shopping, "beta", "Beta", "alpha");
    assert!(is_installed(&shopping, "beta"));
    // Alpha is on the device: nothing more is asked.
    let shown = shopping.shown();
    assert!(!shown.contains("just installed also"), "{shown}");
    assert!(shopping.requests().is_empty());
}

#[test]
fn a_chain_is_a_dialog_for_each_step() {
    let mut shopping = Shopping::new(&list(&[
        ("alpha", "Alpha"),
        ("beta", "Beta"),
        ("gamma", "Gamma"),
    ]));
    install(&mut shopping, "alpha", "Alpha", "beta");
    assert!(shopping.shown().contains("Beta (1.2 MB)"));
    download(&mut shopping, "beta", "Beta", "gamma");
    // Beta requires Gamma, which is not on the device: asked in turn.
    let shown = shopping.shown();
    assert!(shown.contains("Gamma (1.2 MB)"), "{shown}");
    download(&mut shopping, "gamma", "Gamma", "");
    assert!(is_installed(&shopping, "gamma"));
    let shown = shopping.shown();
    assert!(!shown.contains("just installed also"), "{shown}");
}
