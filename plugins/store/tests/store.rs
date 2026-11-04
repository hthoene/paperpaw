//! The store with the real kernel on an X4: the list is fetched for the
//! device and shown by group, a plugin's page fetched, the plugin
//! installed file by file, checked and loaded at once, updated to a newer
//! version and removed; a damaged download is not installed, answers to a
//! download stopped are passed over, and a search finds by name and
//! summary.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{LIST, Shared, Shopping};
use paperpaw_core::Kernel;
use paperpaw_plugin_store::{DISCOVER_FONTS, OPEN, Store};
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::{Id, Instant, Params, Storage, Version};

const PAGE: &str = "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en";

fn manifest(version: &str) -> String {
    format!(
        "[plugin]\nid = airy\nname = Airy\ndescription = Roomy.\nversion = {version}\nformats = 1\n"
    )
}

const CATALOG_DE: &str =
    "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\nairy.hello = Hallo\n";

/// The list for the X4: Airy in `version`, then a plugin of another
/// group, and one with code the device passes over.
fn list(version: &str) -> String {
    format!(
        r#"{{"format": 2, "plugins": [
  {{"id": "airy", "name": "Airy", "summary": "Light headers and wide margins.",
    "group": "reading", "author": "Someone", "licence": "MIT", "kind": "data",
    "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": 300}}}},
  {{"id": "night-keys", "name": "Night keys", "summary": "Keys for reading in the dark.",
    "group": "system", "author": "Someone", "licence": "MIT", "kind": "data",
    "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 100}}}},
  {{"id": "reader", "name": "Reader", "summary": "Reads books.", "group": "reading",
    "kind": "code", "latest": {{"version": "0.4.0"}}}}
]}}"#
    )
}

/// Airy's page at the library, its files relative to the page.
fn page(version: &str, manifest: &str, catalog: &str) -> String {
    let file = |path: &str, data: &str| {
        format!(
            r#"{{"path": "{path}", "url": "airy/{version}/{path}", "size": {}, "sha1": "{}"}}"#,
            data.len(),
            paperpaw_digest::sha1::hex(data.as_bytes())
        )
    };
    format!(
        r#"{{"format": 2, "plugin": {{"id": "airy", "name": "Airy",
  "summary": "Light headers and wide margins.", "group": "reading",
  "author": "Someone", "licence": "MIT", "kind": "data",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": 300,
    "files": [{}, {}]}}}}}}"#,
        file("plugin.ini", manifest),
        file("i18n/de.ini", catalog),
    )
}

/// What the tests of Airy do with the store.
trait Airy {
    fn open_airy(&mut self, page: &str);
    fn install(&mut self, version: &str, manifest: &str, catalog: &str);
    fn installed(&self) -> Option<Version>;
}

impl Airy for Shopping {
    /// Airy's page opened from its group's list, its page at the library
    /// answered with `page`.
    fn open_airy(&mut self, page: &str) {
        self.choose("Reading");
        self.choose("Airy");
        let call = self.request(PAGE);
        self.answer(&call, 200, page.as_bytes());
    }

    /// The first row of Airy's page chosen (*Install* or *Update*), its
    /// files answered with `manifest` and `catalog`.
    fn install(&mut self, version: &str, manifest: &str, catalog: &str) {
        self.run(actions::CONFIRM);
        let base = format!("https://paperpaw.org/api/v1/plugins/airy/{version}");
        let call = self.request(&format!("{base}/plugin.ini"));
        assert_eq!(
            call.save_to.as_deref(),
            Some("plugins/.airy.part/plugin.ini")
        );
        self.answer(&call, 200, manifest.as_bytes());
        let shown = self.shown();
        assert!(shown.contains("Stop downloading"), "{shown}");
        let call = self.request(&format!("{base}/i18n/de.ini"));
        self.answer(&call, 200, catalog.as_bytes());
    }

    fn installed(&self) -> Option<Version> {
        self.kernel
            .plugins()
            .get("airy")
            .map(|p| p.manifest.version)
    }
}

#[test]
fn the_first_list_shows_loading_before_the_network_answers() {
    let mut kernel = Kernel::builder(common::DEVICE)
        .network(true)
        .catalog_device("x4")
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Store)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(100), OPEN);
    kernel.render(&mut |_, _| {});
    assert!(
        kernel
            .describe()
            .iter()
            .any(|p| p.label.contains("Loading plugins"))
    );
    assert!(
        kernel
            .take_io()
            .iter()
            .any(|io| matches!(io, paperpaw_core::Io::Http(_)))
    );
}

#[test]
fn refreshing_the_saved_list_shows_loading_until_success_or_failure() {
    let mut shopping = Shopping::new(&list("1.0.0"));
    for status in [200, 0, 503] {
        shopping.run(OPEN);
        let call = shopping.request(LIST);
        let shown = shopping.shown();
        assert!(shown.contains("Loading plugins"), "{shown}");
        assert!(shown.contains("Reading"), "{shown}");
        shopping.answer(&call, status, list("1.1.0").as_bytes());
        let shown = shopping.shown();
        assert!(!shown.contains("Loading plugins"), "{shown}");
        assert!(shown.contains("Reading"), "{shown}");
    }
}

#[test]
fn loading_changes_the_visible_header_even_beside_touch_paging() {
    let groups = [
        "reading",
        "formats",
        "library",
        "sync",
        "home",
        "look",
        "notes",
        "learning",
        "news",
        "tools",
        "games",
        "languages",
        "system",
    ];
    let entries: Vec<_> = groups
        .iter()
        .enumerate()
        .map(|(i, group)| {
            format!(
                r#"{{"id":"item-{i}","name":"Item {i}","group":"{group}","kind":"data","latest":{{"version":"1.0.0","sdk":"0.4","formats":1}}}}"#
            )
        })
        .collect();
    let list = format!(r#"{{"format":2,"plugins":[{}]}}"#, entries.join(","));
    let mut shopping = Shopping::touch(&list);
    let before = header_pixels(&mut shopping.kernel);
    shopping.run(OPEN);
    let call = shopping.requests().pop().unwrap();
    let loading = header_pixels(&mut shopping.kernel);
    assert_ne!(
        before, loading,
        "paging must not hide the loading indicator"
    );
    shopping.answer(&call, 200, list.as_bytes());
    assert_eq!(before, header_pixels(&mut shopping.kernel));
}

fn header_pixels(kernel: &mut Kernel) -> Vec<u8> {
    let mut pixels = Vec::new();
    kernel.render(&mut |top, frame| {
        if top < 67 {
            let rows = frame.height.min(67 - top);
            pixels.extend_from_slice(&frame.data[..usize::from(rows) * frame.stride]);
        }
    });
    pixels
}

#[test]
fn the_list_shows_what_fits_by_group() {
    let mut shopping = Shopping::new(&list("1.0.0"));
    // Search, the newest, then the groups in the list's order with how
    // many plugins each has; the looks for the fonts.
    shopping.check("start");
    // A group: how its plugins are ordered, then each with what it does
    // and its size.
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(
        shown.contains("Sort by, A to Z | Airy, 300 B, Light headers and wide margins."),
        "{shown}"
    );
    // With code: not for the store.
    assert!(!shown.contains("Reads books."), "{shown}");
    shopping.run(actions::BACK);
    shopping.choose("System");
    let shown = shopping.shown();
    assert!(shown.contains("Night keys"), "{shown}");
    // Kept for the next visit, the list itself gone.
    let kept = ".paperpaw/plugins/store";
    assert!(shopping.storage.exists(&format!("{kept}/index.txt")));
    assert!(!shopping.storage.exists(&format!("{kept}/list.part")));
}

#[test]
fn a_plugin_is_installed_updated_and_removed() {
    let old = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0"));
    shopping.open_airy(&page("1.0.0", &old, CATALOG_DE));
    let shown = shopping.shown();
    assert!(shown.contains("By Someone"), "{shown}");
    assert!(shown.contains("Licence: MIT"), "{shown}");
    shopping.install("1.0.0", &old, CATALOG_DE);
    // Loaded at once, as a plugin of its own.
    assert_eq!(shopping.installed(), Some(Version::new(1, 0, 0)));
    assert!(shopping.storage.exists("plugins/airy/i18n/de.ini"));
    assert!(!shopping.storage.exists("plugins/.airy.part/plugin.ini"));
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/install.txt")
    );
    let shown = shopping.shown();
    assert!(shown.contains("Installed: version 1.0.0"), "{shown}");

    // A newer version: offered, and it takes the old one's place, in the
    // same folder.
    let new = manifest("1.1.0");
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    let call = shopping.request(LIST);
    shopping.answer(&call, 200, list("1.1.0").as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Installed, 1 update"), "{shown}");
    shopping.open_airy(&page("1.1.0", &new, CATALOG_DE));
    shopping.install("1.1.0", &new, CATALOG_DE);
    assert_eq!(shopping.installed(), Some(Version::new(1, 1, 0)));
    assert!(shopping.storage.exists("plugins/airy/plugin.ini"));
    assert!(!shopping.storage.exists("plugins/airy-2/plugin.ini"));

    // *Remove*: asked first.
    let shown = shopping.shown();
    assert!(!shown.contains("Update"), "{shown}");
    shopping.run(actions::CONFIRM);
    let shown = shopping.shown();
    assert!(shown.contains("Remove Airy"), "{shown}");
    shopping.run(actions::CONFIRM);
    assert_eq!(shopping.installed(), None);
    assert!(!shopping.storage.exists("plugins/airy/plugin.ini"));
    let shown = shopping.shown();
    assert!(shown.contains("Install"), "{shown}");
}

/// A plugin of one's own by the store's id (copied over the Wi-Fi
/// transfer, a memory card, a build of one's own) is not the store's: an
/// older version of it is no update, and nothing replaces it by itself.
/// The store's version takes its place only when the reader says so, and
/// is updated from then on.
#[test]
fn a_plugin_of_ones_own_is_no_update_until_the_store_version_is_chosen() {
    let storage = Shared::default();
    storage
        .write("plugins/airy/plugin.ini", manifest("0.9.0").as_bytes())
        .unwrap();
    // Origins are kept on this device (none of the store's yet).
    storage.write(".paperpaw/plugin-origin.txt", b"").unwrap();
    let mut shopping = Shopping::with(Some("x4"), None, &list("1.0.0"), storage);
    let shown = shopping.shown();
    assert!(shown.contains("Installed"), "{shown}");
    assert!(!shown.contains("update"), "{shown}");
    let m = manifest("1.0.0");
    shopping.open_airy(&page("1.0.0", &m, CATALOG_DE));
    let shown = shopping.shown();
    assert!(shown.contains("Use the store version"), "{shown}");
    assert!(!shown.contains("Update"), "{shown}");
    shopping.install("1.0.0", &m, CATALOG_DE);
    assert_eq!(shopping.installed(), Some(Version::new(1, 0, 0)));
    // The store's now: its next version is an update.
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    let call = shopping.request(LIST);
    shopping.answer(&call, 200, list("1.1.0").as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Installed, 1 update"), "{shown}");
}

/// On a device that kept no origins yet (its store installed plugins
/// before they were kept), the plugins there are taken for the store's
/// once: they keep its updates.
#[test]
fn plugins_installed_before_origins_were_kept_keep_the_stores_updates() {
    let storage = Shared::default();
    storage
        .write("plugins/airy/plugin.ini", manifest("0.9.0").as_bytes())
        .unwrap();
    let mut shopping = Shopping::with(Some("x4"), None, &list("1.0.0"), storage);
    let shown = shopping.shown();
    assert!(shown.contains("Installed, 1 update"), "{shown}");
}

#[test]
fn a_damaged_download_is_not_installed() {
    let manifest = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0"));
    shopping.open_airy(&page("1.0.0", &manifest, CATALOG_DE));
    shopping.install("1.0.0", &manifest, "[meta]\nlanguage = de\n");
    assert_eq!(shopping.installed(), None);
    assert!(!shopping.storage.exists("plugins/.airy.part/plugin.ini"));
    assert!(!shopping.storage.exists("plugins/airy/plugin.ini"));
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/install.txt")
    );
    // Offered again.
    let shown = shopping.shown();
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn answers_to_a_download_stopped_are_passed_over() {
    let manifest = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0"));
    shopping.open_airy(&page("1.0.0", &manifest, CATALOG_DE));
    let first = "https://paperpaw.org/api/v1/plugins/airy/1.0.0/plugin.ini";
    // *Install*, *Stop downloading*, *Install* again.
    shopping.run(actions::CONFIRM);
    let stopped = shopping.request(first);
    shopping.run(actions::CONFIRM);
    shopping.run(actions::CONFIRM);
    let again = shopping.request(first);
    // The answer for the job stopped comes late: the new job still waits
    // for its own.
    shopping.answer(&stopped, 200, manifest.as_bytes());
    assert!(shopping.requests().is_empty());
    shopping.answer(&again, 200, manifest.as_bytes());
    shopping.request("https://paperpaw.org/api/v1/plugins/airy/1.0.0/i18n/de.ini");
}

#[test]
fn a_search_finds_names_and_summaries() {
    let mut shopping = Shopping::new(&list("1.0.0"));
    // *Search*, answered as the keyboard answers.
    shopping.run(actions::CONFIRM);
    shopping.now += 100;
    let typed = Params::new().with(params::TEXT, "Dark");
    shopping.kernel.send_event(
        Instant(shopping.now),
        Id::new_static("store.searched"),
        typed,
    );
    let shown = shopping.shown();
    assert!(shown.contains("Search: Dark"), "{shown}");
    assert!(shown.contains("Night keys"), "{shown}");
    assert!(!shown.contains("Airy"), "{shown}");
}

#[test]
fn a_device_no_library_knows_is_told_so() {
    let mut shopping = Shopping::on(None, None, "");
    assert!(shopping.requests().is_empty());
    let shown = shopping.shown();
    assert!(shown.contains("No plugins for this device"), "{shown}");
}

#[test]
fn fonts_open_the_fonts_browser() {
    let mut shopping = Shopping::new(&list("1.0.0"));
    // With the looks, before their plugins.
    shopping.choose("Look");
    shopping.choose("Fonts");
    shopping.request("https://paperpaw.org/api/v1/fonts.json");
}

/// The page of the text says where fonts with larger sizes are found: the
/// font size goes as far as the packs built in (48 pixels at 220 dpi: 16
/// points), and *Discover fonts* opens the fonts browser, or its page
/// where it is not installed.
#[test]
fn larger_sizes_are_discovered_from_the_reading_settings() {
    let mut shopping = Shopping::new(&list("1.0.0"));
    let kernel = &shopping.kernel;
    let size = kernel
        .settings()
        .spec("core.font-size")
        .map(|s| s.kind.clone());
    assert_eq!(
        size,
        Some(paperpaw_sdk::SettingKind::Int { min: 6, max: 16 })
    );
    let text = kernel.menus().ids("core.group.reading-text");
    assert!(text.contains(&&DISCOVER_FONTS), "{text:?}");
    shopping.run(DISCOVER_FONTS);
    shopping.request("https://paperpaw.org/api/v1/fonts.json");

    let mut kernel = Kernel::builder(common::DEVICE)
        .storage(Shared::default())
        .network(true)
        .catalog_device("x4")
        .plugin(Store)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(100), DISCOVER_FONTS);
    let top = kernel.screens().last().map(Id::as_str);
    assert_eq!(top, Some("store.plugin"));
}

#[test]
fn a_plugin_built_in_is_not_offered_over() {
    // The library offers a plugin with the id of the fonts browser, which
    // the firmware has built in.
    let list = r#"{"format": 2, "plugins": [
  {"id": "fonts-browser", "name": "Other fonts", "summary": "Takes the fonts browser's place.",
    "group": "reading", "author": "Someone", "licence": "MIT", "kind": "data",
    "latest": {"version": "9.0.0", "sdk": "0.4", "formats": 1, "size": 100}}]}"#;
    let mut shopping = Shopping::new(list);
    let shown = shopping.shown();
    assert!(!shown.contains("Other fonts"), "{shown}");
    // Its page, opened by its id, offers nothing to do with it.
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.plugin"),
        Params::new().with(params::ID, "fonts-browser"),
    );
    let call = shopping
        .request("https://paperpaw.org/api/v1/plugins/fonts-browser.json?device=x4&lang=en");
    shopping.answer(
        &call,
        200,
        list.replace("\"plugins\": [", "\"plugin\": ")
            .replace("}]}", "}}")
            .as_bytes(),
    );
    let shown = shopping.shown();
    assert!(!shown.contains("Install"), "{shown}");
    assert!(!shown.contains("Remove"), "{shown}");
}

#[test]
fn a_page_fetched_for_another_plugin_is_not_taken() {
    let manifest = manifest("1.0.0");
    let mut shopping = Shopping::new(&list("1.0.0"));
    // Airy's page asked for, into a file of its own; another plugin's
    // page comes.
    shopping.choose("Reading");
    shopping.choose("Airy");
    let call = shopping.request(PAGE);
    assert_eq!(
        call.save_to.as_deref(),
        Some(".paperpaw/plugins/store/plugin-airy.part")
    );
    let other =
        page("1.0.0", &manifest, CATALOG_DE).replace(r#""id": "airy""#, r#""id": "night-keys""#);
    shopping.answer(&call, 200, other.as_bytes());
    // *Install* downloads nothing of the other plugin.
    shopping.run(actions::CONFIRM);
    assert!(shopping.requests().is_empty());
    assert_eq!(shopping.installed(), None);
}

#[test]
fn the_pages_of_a_library_with_a_key_keep_it() {
    let mut shopping = Shopping::new(&list("1.0.0"));
    shopping.run(actions::BACK);
    shopping.now += 100;
    shopping
        .kernel
        .set_setting(
            Instant(shopping.now),
            "store.catalog",
            "https://example.org/p/plugins.json?key=abc",
        )
        .unwrap();
    shopping.run(OPEN);
    let call =
        shopping.request("https://example.org/p/plugins.json?key=abc&device=x4&lang=en&order=fair");
    shopping.answer(&call, 200, list("1.0.0").as_bytes());
    shopping.choose("Reading");
    shopping.choose("Airy");
    shopping.request("https://example.org/p/plugins/airy.json?key=abc&device=x4&lang=en");
}

#[test]
fn a_built_in_plugin_switched_off_is_switched_on_and_never_downloaded() {
    // Android builds the statistics in, switched off; the library lists them.
    let list = r#"{"format": 2, "plugins": [
  {"id": "stats", "name": "Reading statistics", "summary": "Your reading in numbers.",
    "group": "tools", "author": "PaperPaw", "licence": "Apache-2.0", "kind": "data",
    "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 100}}]}"#;
    let mut shopping = Shopping::with_stats_off(list);
    shopping.choose("Tools");
    let shown = shopping.shown();
    assert!(shown.contains("Reading statistics"), "{shown}");
    assert!(shown.contains("Off"), "{shown}");
    shopping.choose("Reading statistics");
    let call = shopping.request("https://paperpaw.org/api/v1/plugins/stats.json?device=x4&lang=en");
    shopping.answer(
        &call,
        200,
        list.replace("\"plugins\": [", "\"plugin\": ")
            .replace("}]}", "}}")
            .as_bytes(),
    );
    let shown = shopping.shown();
    assert!(shown.contains("Switch on"), "{shown}");
    assert!(!shown.contains("Install"), "{shown}");
    assert!(!shown.contains("MB") && !shown.contains("KB"), "{shown}");
    shopping.choose("Switch on");
    let stats = shopping.kernel.plugins().get("stats").unwrap();
    assert!(stats.enabled, "{stats:?}");
    // Nothing was downloaded: no request after the list and the page.
    assert!(
        shopping
            .kernel
            .take_io()
            .iter()
            .filter_map(|io| match io {
                paperpaw_core::Io::Http(call) => Some(call.url.clone()),
                _ => None,
            })
            .all(|url| !url.contains(".ppn") && !url.contains("/b/"))
    );
}
