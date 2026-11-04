//! Finding one's way in a large library on the X4: a group's plugins in
//! the order asked for, a page of a long list gone to, the newest, the
//! plugins installed with their updates first and all updated at once,
//! the room left said where the device can tell, and a plugin's page with
//! its source and screenshots.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Shared, Shopping};
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::{Id, Instant, Params, Storage, Version};

const BASE: &str = "https://paperpaw.org/api/v1/plugins";

/// A plugin of the list: id, name, group, version, first published,
/// newest version published, size.
fn entry(id: &str, group: &str, version: &str, added: &str, date: &str, size: u64) -> String {
    let name = name_of(id);
    format!(
        r#"{{"id": "{id}", "name": "{name}", "summary": "Does {id}.", "group": "{group}",
  "author": "Someone", "licence": "MIT", "kind": "data", "added": "{added}",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": {size}, "date": "{date}"}}}}"#
    )
}

/// `apple` is called Apple.
fn name_of(id: &str) -> String {
    let mut name = id.to_string();
    name[..1].make_ascii_uppercase();
    name
}

fn list(entries: &[String]) -> String {
    format!(r#"{{"format": 2, "plugins": [{}]}}"#, entries.join(", "))
}

fn manifest(id: &str, version: &str) -> String {
    format!(
        "[plugin]\nid = {id}\nname = {}\nversion = {version}\nformats = 1\n",
        name_of(id)
    )
}

/// A plugin's page at the library: its plugin.ini, and `more` fields.
fn page(id: &str, version: &str, more: &str) -> String {
    let ini = manifest(id, version);
    format!(
        r#"{{"format": 2, "plugin": {{"id": "{id}", "name": "{}", "summary": "Does {id}.",
  "group": "reading", "author": "Someone", "licence": "MIT", "kind": "data", {more}
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": {},
    "files": [{{"path": "plugin.ini", "url": "{id}/{version}/plugin.ini", "size": {},
                "sha1": "{}"}}]}}}}}}"#,
        name_of(id),
        ini.len(),
        ini.len(),
        paperpaw_digest::sha1::hex(ini.as_bytes())
    )
}

fn page_url(id: &str) -> String {
    format!("{BASE}/{id}.json?device=x4&lang=en")
}

/// Where `first` shows before `second`.
fn before(shown: &str, first: &str, second: &str) -> bool {
    let at = |text: &str| {
        shown
            .find(text)
            .unwrap_or_else(|| panic!("{text}: {shown}"))
    };
    at(first) < at(second)
}

/// Three plugins of reading, each newest and latest updated in another
/// order than by name.
fn fruit() -> String {
    list(&[
        entry("apple", "reading", "1.0.0", "2025-01-01", "2026-01-01", 300),
        entry("berry", "reading", "1.0.0", "2026-05-01", "2026-02-01", 300),
        entry(
            "cherry",
            "reading",
            "1.0.0",
            "2025-06-01",
            "2026-09-01",
            300,
        ),
    ])
}

#[test]
fn a_group_is_sorted_as_asked() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(before(&shown, "Apple", "Berry") && before(&shown, "Berry", "Cherry"));
    // Newest first.
    shopping.choose("Sort by");
    shopping.choose("Newest");
    let shown = shopping.shown();
    assert!(shown.contains("Sort by, Newest"), "{shown}");
    assert!(before(&shown, "Berry", "Cherry") && before(&shown, "Cherry", "Apple"));
    // Most recently updated first.
    shopping.choose("Sort by");
    shopping.choose("Recently updated");
    let shown = shopping.shown();
    assert!(before(&shown, "Cherry", "Berry") && before(&shown, "Berry", "Apple"));
}

#[test]
fn the_newest_come_first() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("New");
    let shown = shopping.shown();
    assert!(shown.starts_with("New"), "{shown}");
    assert!(before(&shown, "Berry", "Cherry") && before(&shown, "Cherry", "Apple"));
}

#[test]
fn a_long_list_goes_to_a_page() {
    let many: Vec<String> = (0..40)
        .map(|n| {
            entry(
                &format!("p{n:02}"),
                "reading",
                "1.0.0",
                "2026-01-01",
                "2026-01-01",
                10,
            )
        })
        .collect();
    let mut shopping = Shopping::new(&list(&many));
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(shown.contains("P00"), "{shown}");
    // Said under the order, where a list has pages.
    assert!(shown.contains("Hold Select to go to a page."), "{shown}");
    // A long press of Select asks which page.
    shopping.run(actions::MENU);
    let shown = shopping.shown();
    assert!(shown.contains("Go to page (1 to "), "{shown}");
    shopping.run(actions::BACK);
    // Answered as the keyboard answers.
    shopping.now += 100;
    let typed = Params::new().with(params::TEXT, "3");
    shopping
        .kernel
        .send_event(Instant(shopping.now), Id::new_static("store.jumped"), typed);
    let shown = shopping.shown();
    assert!(!shown.contains("P00"), "{shown}");
    assert!(!shown.contains("Sort by"), "{shown}");
    assert!(shown.contains("P1") || shown.contains("P2"), "{shown}");
}

/// A storage with `apple` and `cherry` installed in 1.0.0.
/// Apple and cherry, installed by the store at 1.0.0.
fn with_two() -> Shared {
    let storage = Shared::default();
    for id in ["apple", "cherry"] {
        let path = format!("plugins/{id}/plugin.ini");
        storage
            .write(&path, manifest(id, "1.0.0").as_bytes())
            .unwrap();
        paperpaw_sdk::install::remember_store_install(&storage, id, Version::new(1, 0, 0));
    }
    storage
}

fn version(shopping: &Shopping, id: &str) -> Option<Version> {
    shopping
        .kernel
        .plugins()
        .get(id)
        .map(|p| p.manifest.version)
}

#[test]
fn updates_come_first_and_all_at_once() {
    let updates = list(&[
        entry("apple", "reading", "1.1.0", "2025-01-01", "2026-01-01", 300),
        entry("berry", "reading", "1.0.0", "2026-05-01", "2026-02-01", 300),
        entry(
            "cherry",
            "reading",
            "2.0.0",
            "2025-06-01",
            "2026-09-01",
            300,
        ),
    ]);
    let mut shopping = Shopping::with(Some("x4"), None, &updates, with_two());
    let shown = shopping.shown();
    assert!(shown.contains("Installed, 2 updates"), "{shown}");
    shopping.choose("Installed");
    shopping.check("installed");
    // *Update all*: one after another, each page fetched in its turn.
    shopping.choose("Update all");
    shopping.serve(&page_url("apple"), page("apple", "1.1.0", "").as_bytes());
    let asked = shopping.requests();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].url, format!("{BASE}/apple/1.1.0/plugin.ini"));
    let ini = manifest("apple", "1.1.0");
    shopping.answer(&asked[0], 200, ini.as_bytes());
    assert_eq!(version(&shopping, "apple"), Some(Version::new(1, 1, 0)));
    shopping.serve(&page_url("cherry"), page("cherry", "2.0.0", "").as_bytes());
    let ini = manifest("cherry", "2.0.0");
    shopping.serve(&format!("{BASE}/cherry/2.0.0/plugin.ini"), ini.as_bytes());
    assert_eq!(version(&shopping, "cherry"), Some(Version::new(2, 0, 0)));
    assert!(shopping.requests().is_empty());
    // Nothing left to update.
    let shown = shopping.shown();
    assert!(!shown.contains("Update all"), "{shown}");
}

#[test]
fn the_room_left_is_said_where_known() {
    let entries = list(&[
        entry("apple", "reading", "1.0.0", "2025-01-01", "2026-01-01", 300),
        entry("big", "reading", "1.0.0", "2025-01-01", "2026-01-01", 9000),
    ]);
    // Not known: not said.
    let mut shopping = Shopping::new(&entries);
    assert!(!shopping.shown().contains("free"));
    shopping.choose("Reading");
    shopping.choose("Apple");
    shopping.serve(&page_url("apple"), page("apple", "1.0.0", "").as_bytes());
    assert!(!shopping.shown().contains("free"));

    // 5 KB free: beside the title, on a plugin's page, and a plugin too
    // large offers no *Install*.
    let storage = Shared::default();
    storage.1.set(Some(5000));
    let mut shopping = Shopping::with(Some("x4"), None, &entries, storage);
    let shown = shopping.shown();
    assert!(shown.contains("4.9 KB free"), "{shown}");
    shopping.choose("Reading");
    shopping.choose("Apple");
    shopping.serve(&page_url("apple"), page("apple", "1.0.0", "").as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Needs 61 B, 4.9 KB free"), "{shown}");
    assert!(shown.contains("Install, 61 B"), "{shown}");
    shopping.run(actions::BACK);
    shopping.choose("Big");
    // Its files take 9000 bytes, as the list said.
    let big = page("big", "1.0.0", "").replacen(r#""size": 57,"#, r#""size": 9000,"#, 1);
    shopping.serve(&page_url("big"), big.as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Not enough room"), "{shown}");
    assert!(!shown.contains("Install"), "{shown}");
}

#[test]
fn a_plugins_page_says_where_its_source_is() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("Reading");
    shopping.choose("Apple");
    let more = r#""source": "https://codeberg.org/someone/apple",
  "author_since": "2024-03-02", "author_plugins": 3,
  "screenshots": [
    {"url": "shots/apple-1.png", "width": 360, "height": 600, "size": 20000, "alt": "The list"},
    {"url": "shots/apple-2.webp", "width": 360, "height": 600, "size": 20000},
    {"url": "shots/apple-3.png", "width": 3000, "height": 600, "size": 20000}],"#;
    let body = page("apple", "1.0.0", more).replace(
        r#""latest": {"#,
        r#""latest": {"source": {"url": "https://codeberg.org/someone/apple/src/tag/v1.0.0"}, "#,
    );
    shopping.serve(&page_url("apple"), body.as_bytes());
    // Data only: no build to check; the author since when; the
    // screenshots this device can decode.
    shopping.check("page-full");
    // The source of this version, as a code to scan.
    shopping.choose("Source code");
    let shown = shopping.shown();
    assert!(
        shown.contains("codeberg.org/someone/apple/src/tag/v1.0.0"),
        "{shown}"
    );
    shopping.run(actions::BACK);
    // The screenshot fetched when shown.
    shopping.choose("Screenshots");
    shopping.request(&format!("{BASE}/shots/apple-1.png"));
}

#[test]
fn a_plugins_page_says_how_it_works() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("Reading");
    shopping.choose("Apple");
    let more = r#""description": "First this.\n\nThen \"that\".","#;
    shopping.serve(&page_url("apple"), page("apple", "1.0.0", more).as_bytes());
    // Shown as a text to read paged.
    shopping.choose("How it works");
    let text = paperpaw_sdk::read_file(
        shopping.kernel.storage(),
        ".paperpaw/plugins/store/how.txt",
        1 << 16,
    )
    .map(|data| String::from_utf8(data).unwrap())
    .unwrap();
    assert_eq!(text, "Apple\n\nFirst this.\n\nThen \"that\".\n");
}

#[test]
fn a_plugin_that_says_nothing_more_has_no_how_it_works() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("Reading");
    shopping.choose("Apple");
    shopping.serve(&page_url("apple"), page("apple", "1.0.0", "").as_bytes());
    let shown = shopping.shown();
    assert!(!shown.contains("How it works"), "{shown}");
}

fn many(count: usize) -> String {
    let entries: Vec<String> = (0..count)
        .map(|n| {
            entry(
                &format!("p{n:02}"),
                "reading",
                "1.0.0",
                "2026-01-01",
                "2026-01-01",
                10,
            )
        })
        .collect();
    list(&entries)
}

#[test]
fn on_touch_screens_the_page_number_goes_to_a_page() {
    let mut shopping = Shopping::touch(&many(40));
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(
        shown.contains("Tap the page number to go to a page."),
        "{shown}"
    );
    // A long press on the arrows beside it: a large place for a finger.
    shopping.touch_at(470, 30, true);
    let shown = shopping.shown();
    assert!(shown.contains("Go to page (1 to "), "{shown}");
    shopping.run(actions::BACK);
    // A tap on the number itself, wherever it is between the arrows.
    let mut asked = false;
    for x in (240..480).step_by(4) {
        shopping.touch_at(x, 30, false);
        let shown = shopping.shown();
        if shown.contains("Go to page (1 to ") {
            asked = true;
            break;
        }
        // An arrow turned the page: back to the first.
        shopping.run(actions::PREV_PAGE);
    }
    assert!(asked, "{}", shopping.shown());
}

/// A grey picture of 4 by 4 pixels.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 4, 8, 0, 0,
    0, 0, 140, 154, 193, 162, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 108, 96, 96, 96, 96, 98,
    64, 33, 0, 9, 228, 0, 136, 82, 22, 78, 95, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[test]
fn a_screenshot_turned_past_is_not_shown_for_another() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("Reading");
    shopping.choose("Apple");
    let more = r#""screenshots": [
    {"url": "shots/a.png", "width": 4, "height": 4, "size": 73},
    {"url": "shots/b.png", "width": 4, "height": 4, "size": 73}],"#;
    shopping.serve(&page_url("apple"), page("apple", "1.0.0", more).as_bytes());
    shopping.choose("Screenshots");
    let first = shopping.request(&format!("{BASE}/shots/a.png"));
    // On to the second before the first came.
    shopping.run(actions::NEXT_PAGE);
    let second = shopping.request(&format!("{BASE}/shots/b.png"));
    let gray = ".paperpaw/plugins/store/shot.gray";
    shopping.answer(&first, 200, PNG);
    assert!(!shopping.storage.exists(gray), "the first is not shown");
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/shot-0.part")
    );
    let shown = shopping.shown();
    assert!(shown.contains("Loading"), "{shown}");
    shopping.answer(&second, 200, PNG);
    assert!(shopping.storage.exists(gray));
    let shown = shopping.shown();
    assert!(!shown.contains("Loading"), "{shown}");
}

#[test]
fn the_queue_goes_on_after_a_stop_and_a_restart() {
    let updates = list(&[
        entry("apple", "reading", "1.1.0", "2025-01-01", "2026-01-01", 300),
        entry(
            "cherry",
            "reading",
            "2.0.0",
            "2025-06-01",
            "2026-09-01",
            300,
        ),
    ]);
    let mut shopping = Shopping::with(Some("x4"), None, &updates, with_two());
    shopping.choose("Installed");
    shopping.choose("Update all");
    shopping.serve(&page_url("apple"), page("apple", "1.1.0", "").as_bytes());
    let apple = shopping.requests();
    assert_eq!(apple.len(), 1, "{apple:?}");
    // Cherry waits its turn, said in its row.
    let shown = shopping.shown();
    assert!(shown.contains("Cherry, Waiting"), "{shown}");
    // Apple stopped on its page: cherry's turn.
    shopping.choose("Apple");
    shopping.serve(&page_url("apple"), page("apple", "1.1.0", "").as_bytes());
    shopping.choose("Stop downloading");
    shopping.serve(&page_url("cherry"), page("cherry", "2.0.0", "").as_bytes());
    let cherry = shopping.requests();
    assert_eq!(cherry.len(), 1, "{cherry:?}");
    assert_eq!(version(&shopping, "apple"), Some(Version::new(1, 0, 0)));
    // A restart cuts cherry's download off: nothing is fetched at the
    // start, and it goes on when the store is opened again.
    shopping.now += 100;
    shopping.kernel.send_event(
        Instant(shopping.now),
        Id::new_static("core.started"),
        Params::new(),
    );
    assert!(shopping.requests().is_empty());
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(paperpaw_plugin_store::OPEN);
    let asked: Vec<String> = shopping.requests().into_iter().map(|c| c.url).collect();
    assert!(asked.contains(&page_url("cherry")), "{asked:?}");
}

/// Apple, installed by the store as `version`.
fn with_apple(version: &str) -> Shared {
    let storage = Shared::default();
    let path = "plugins/apple/plugin.ini";
    storage
        .write(path, manifest("apple", version).as_bytes())
        .unwrap();
    paperpaw_sdk::install::remember_store_install(&storage, "apple", version);
    storage
}

/// Betas of one release are told apart by their whole version: a newer
/// beta is an update where the device runs a beta (or takes betas), the
/// release is one over any of its betas, an older beta never.
#[test]
fn betas_are_updates_where_the_device_takes_them() {
    let offered = |version: &str| {
        list(&[entry(
            "apple",
            "reading",
            version,
            "2025-01-01",
            "2026-01-01",
            300,
        )])
    };
    let updates = |shopping: &mut Shopping| shopping.shown().contains("1 update");
    // A release firmware: no beta, but the release.
    let mut shopping = Shopping::with(
        Some("x4"),
        None,
        &offered("1.0.0-beta.2"),
        with_apple("1.0.0-beta.1"),
    );
    assert!(!updates(&mut shopping), "{}", shopping.shown());
    let mut shopping = Shopping::with(
        Some("x4"),
        None,
        &offered("1.0.0"),
        with_apple("1.0.0-beta.1"),
    );
    assert!(updates(&mut shopping), "{}", shopping.shown());
    // A beta firmware: the newer beta, never an older one.
    let mut shopping = Shopping::on_firmware(
        "0.5.0-beta.1",
        &offered("1.0.0-beta.2"),
        with_apple("1.0.0-beta.1"),
    );
    assert!(updates(&mut shopping), "{}", shopping.shown());
    let mut shopping = Shopping::on_firmware(
        "0.5.0-beta.1",
        &offered("1.0.0-beta.1"),
        with_apple("1.0.0-beta.2"),
    );
    assert!(!updates(&mut shopping), "{}", shopping.shown());
}

/// A plugin's page with a long summary and facts on several lines: what
/// it says, its facts and its rows each below the other, nothing drawn
/// over another part (each part's place, as told, apart).
#[test]
fn a_plugins_page_draws_its_parts_apart() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("Reading");
    shopping.choose("Apple");
    let more = r#""source": "https://codeberg.org/someone/apple", "author_since": "2024-03-02", "author_plugins": 3,"#;
    let long = "Reading aids for the words: bold starts of words, wider spacing of letters, words and lines, and a calmer type for those who read with dyslexia.";
    let body = page("apple", "1.0.0", more).replace("Does apple.", long);
    shopping.serve(&page_url("apple"), body.as_bytes());
    shopping.kernel.render(&mut |_, _| {});
    let parts = shopping.kernel.describe();
    let heading = parts
        .iter()
        .position(|p| format!("{:?}", p.role) == "Heading");
    let shown: Vec<_> = parts
        .iter()
        .enumerate()
        .filter(|(i, p)| Some(*i) != heading && p.area.width > 0 && p.area.height > 0)
        .map(|(_, p)| p)
        .collect();
    for (i, a) in shown.iter().enumerate() {
        for b in &shown[i + 1..] {
            // A part inside another (a row's own text) is no overlap.
            let inside = |x: &paperpaw_sdk::Rect, y: &paperpaw_sdk::Rect| {
                x.x >= y.x && x.y >= y.y && x.right() <= y.right() && x.bottom() <= y.bottom()
            };
            if inside(&a.area, &b.area) || inside(&b.area, &a.area) {
                continue;
            }
            assert!(
                a.area.intersection(&b.area).is_none(),
                "{:?} {:?} over {:?} {:?}",
                a.label,
                a.area,
                b.label,
                b.area
            );
        }
    }
}

/// A group opened from the list kept, while the fresh list is still being
/// fetched, shows its plugins before and after the fresh list arrives
/// (it replaced the index the open list picked from: the rows were left
/// empty, only their dividers drawn) (#169).
#[test]
fn a_group_opened_while_the_list_loads_shows_its_plugins_after_it() {
    let first = Shopping::new(&fruit());
    let (mut shopping, call) = Shopping::reopened(first.storage.clone());
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(before(&shown, "Apple", "Berry"), "{shown}");
    // The fresh list arrives with another plugin and another order.
    let fresh = list(&[
        entry("apple", "reading", "1.0.0", "2025-01-01", "2026-01-01", 300),
        entry(
            "avocado",
            "reading",
            "1.0.0",
            "2025-01-01",
            "2026-01-01",
            300,
        ),
        entry("berry", "reading", "1.0.0", "2026-05-01", "2026-02-01", 300),
        entry(
            "cherry",
            "reading",
            "1.0.0",
            "2025-06-01",
            "2026-09-01",
            300,
        ),
    ]);
    shopping.answer(&call, 200, fresh.as_bytes());
    let shown = shopping.shown();
    for name in ["Apple", "Avocado", "Berry", "Cherry"] {
        assert!(shown.contains(name), "{name} missing: {shown}");
    }
    assert!(before(&shown, "Apple", "Avocado") && before(&shown, "Avocado", "Berry"));
    // Opening one still works.
    shopping.choose("Avocado");
    assert!(shopping.shown().contains("Avocado"), "{}", shopping.shown());
}

/// The same on a page past the first, and the focus stays on it.
#[test]
fn a_page_of_a_long_group_is_not_left_empty_when_the_list_arrives() {
    let first = Shopping::new(&many(40));
    let (mut shopping, call) = Shopping::reopened(first.storage.clone());
    shopping.choose("Reading");
    shopping.now += 100;
    let typed = Params::new().with(params::TEXT, "3");
    shopping
        .kernel
        .send_event(Instant(shopping.now), Id::new_static("store.jumped"), typed);
    let shown = shopping.shown();
    assert!(shown.contains("Does p"), "{shown}");
    shopping.answer(&call, 200, many(41).as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Does p"), "empty rows: {shown}");
    assert!(shown.matches("Does p").count() >= 3, "{shown}");
    // Paging on after it: the next page has its plugins too (the hits were
    // of the list before, and gone).
    shopping.run(actions::NEXT_PAGE);
    let shown = shopping.shown();
    assert!(shown.matches("Does p").count() >= 3, "empty page: {shown}");
    shopping.run(actions::PREV_PAGE);
    shopping.run(actions::PREV_PAGE);
    let shown = shopping.shown();
    assert!(shown.matches("Does p").count() >= 3, "empty page: {shown}");
}

/// A search or the newest, open while the fresh list arrives, are picked
/// again from it too.
#[test]
fn the_newest_are_not_left_empty_when_the_list_arrives() {
    let first = Shopping::new(&fruit());
    let (mut shopping, call) = Shopping::reopened(first.storage.clone());
    shopping.choose("New");
    assert!(shopping.shown().contains("Berry"), "{}", shopping.shown());
    shopping.answer(&call, 200, fruit().as_bytes());
    let shown = shopping.shown();
    assert!(
        shown.contains("Berry") && shown.contains("Cherry"),
        "{shown}"
    );
}
