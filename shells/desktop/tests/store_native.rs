//! The native plugin store, run end to end in the emulator: the real kernel with
//! the linked store, the card in memory and the plugin library answered from
//! recorded answers (the route the emulator sends its requests through instead
//! of the network). Driven through the buttons: the list, the install of a
//! plugin (download, the host's confirmation, the install), a declined install
//! and the stage a power cut left, the update of an installed plugin and its
//! removal (#273: the host jobs the X4 family's firmware wires the same way,
//! `paperpaw_x4::host_jobs`). Not run here (gaps, see the PR's coverage
//! table): what other plugins ask for, *Discover fonts*; they are tested on the
//! pieces (`tests/native_*.rs`).
#![cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use paperpaw_core::sdk::{MemoryStorage, Storage};
use paperpaw_core::{Effects, HttpCall, HttpResponse};
use paperpaw_emulator::device::Device;
use paperpaw_emulator::headless::run_script;
use paperpaw_emulator::http::{Route, set_route};
use paperpaw_emulator::profiles;

/// The library's recorded answers by address without its query; what was asked
/// is kept in order.
#[derive(Clone, Default)]
struct Library {
    answers: Rc<RefCell<HashMap<String, Vec<u8>>>>,
    asked: Rc<RefCell<Vec<String>>>,
}

impl Library {
    fn put(&self, url: &str, body: impl Into<Vec<u8>>) {
        self.answers
            .borrow_mut()
            .insert(url.to_string(), body.into());
    }

    /// The addresses asked for so far, in order, with their queries.
    fn asked(&self) -> Vec<String> {
        self.asked.borrow().clone()
    }
}

struct Recorded(Library);

impl Route for Recorded {
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        self.0.asked.borrow_mut().push(call.url.clone());
        let path = call.url.split('?').next().unwrap_or("");
        let found = self.0.answers.borrow().get(path).cloned();
        Some(HttpResponse {
            id: call.id,
            status: if found.is_some() { 200 } else { 404 },
            body: found.unwrap_or_default(),
            ..HttpResponse::default()
        })
    }

    fn later(&mut self) -> Option<HttpResponse> {
        None
    }
}

const BASE: &str = "https://paperpaw.org/api/v1/";

/// A plugin of the library: its list entry and its page.
struct Offered {
    id: &'static str,
    name: &'static str,
    version: &'static str,
    manifest: String,
    summary: &'static str,
}

impl Offered {
    fn data(id: &'static str, name: &'static str, version: &'static str) -> Self {
        let manifest =
            format!("[plugin]\nid = {id}\nname = {name}\nversion = {version}\nformats = 4\n");
        Self {
            id,
            name,
            version,
            manifest,
            summary: "A pack.",
        }
    }

    /// The same plugin with another summary in the library.
    fn summarised(mut self, summary: &'static str) -> Self {
        self.summary = summary;
        self
    }

    /// A data plugin that requires `requires` (plugin ids apart by commas).
    fn requiring(
        id: &'static str,
        name: &'static str,
        version: &'static str,
        requires: &str,
    ) -> Self {
        let mut plugin = Self::data(id, name, version);
        plugin
            .manifest
            .push_str(&format!("requires = {requires}\n"));
        plugin
    }

    fn entry(&self) -> String {
        format!(
            r#"{{"id": "{}", "name": "{}", "summary": "{}", "group": "reading",
  "author": "PaperPaw", "licence": "OFL-1.1", "kind": "data",
  "added": "2026-01-01", "latest": {{"version": "{}", "sdk": "0.4", "formats": 4,
  "size": {}, "date": "2026-01-02"}}}}"#,
            self.id,
            self.name,
            self.summary,
            self.version,
            self.manifest.len()
        )
    }

    fn page(&self) -> String {
        format!(
            r#"{{"format": 2, "plugin": {{"id": "{id}", "name": "{name}",
  "summary": "{summary}", "group": "reading", "author": "PaperPaw",
  "licence": "OFL-1.1", "kind": "data",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 4, "size": {size},
    "files": [{{"path": "plugin.ini", "url": "{id}/{version}/plugin.ini",
                "size": {size}, "sha1": "{sha}"}}]}}}}}}"#,
            id = self.id,
            name = self.name,
            summary = self.summary,
            version = self.version,
            size = self.manifest.len(),
            sha = paperpaw_digest::sha1::hex(self.manifest.as_bytes()),
        )
    }
}

/// Serves `plugins` as the library of the X4.
fn serve(library: &Library, plugins: &[Offered]) {
    let entries: Vec<String> = plugins.iter().map(Offered::entry).collect();
    library.put(
        &format!("{BASE}plugins.json"),
        format!(r#"{{"format": 2, "plugins": [{}]}}"#, entries.join(",")),
    );
    for plugin in plugins {
        library.put(&format!("{BASE}plugins/{}.json", plugin.id), plugin.page());
        library.put(
            &format!("{BASE}plugins/{}/{}/plugin.ini", plugin.id, plugin.version),
            plugin.manifest.clone(),
        );
    }
}

/// The emulated X4 with the linked store, its card and its library.
struct Shop {
    device: Device,
    card: Rc<MemoryStorage>,
    library: Library,
}

impl Shop {
    fn new(plugins: &[Offered]) -> Self {
        Self::on(MemoryStorage::new(), plugins)
    }

    fn on(card: MemoryStorage, plugins: &[Offered]) -> Self {
        let card = Rc::new(
            card.with(".paperpaw/plugins/setup/done", "")
                .with("paperpaw.ini", "[help]\nshow = false\n"),
        );
        let library = Library::default();
        serve(&library, plugins);
        set_route(Some(Box::new(Recorded(library.clone()))));
        paperpaw_emulator::budget::override_limit(None);
        let profile = profiles::find("xteink-x4").expect("the X4 profile");
        let mut device = Device::in_memory(profile, None, card.clone(), true, None).unwrap();
        device.start();
        let mut shop = Self {
            device,
            card,
            library,
        };
        shop.settle(3000);
        shop
    }

    /// Time passes, answers arrive, the kernel runs when it asks to.
    fn settle(&mut self, millis: u64) {
        for _ in 0..(millis / 100).max(1) {
            self.device.poll_http(Some(Duration::from_millis(20)));
            self.device.advance(100);
            self.device.tick_if_due();
        }
    }

    /// Steps of a script: `run:action`, button names, as the headless runs.
    fn keys(&mut self, script: &str) {
        run_script(&mut self.device, script).unwrap();
        self.settle(500);
    }

    fn run(&mut self, action: &str) {
        self.keys(&format!("run:{action}"));
    }

    /// The labels of what the screen shows, in order.
    fn shown(&mut self) -> String {
        let mut shown = String::new();
        self.device.call(|kernel, _| {
            kernel.render(&mut |_, _| {});
            shown = kernel
                .describe()
                .into_iter()
                .map(|part| {
                    let focus = if part.focused { "*" } else { "" };
                    format!("{focus}{}", part.label.replace('\n', " / "))
                })
                .collect::<Vec<_>>()
                .join(" | ");
            Effects::default()
        });
        shown
    }

    fn screen(&mut self) -> String {
        let mut screen = String::new();
        self.device.call(|kernel, _| {
            screen = kernel
                .active_screen()
                .map(ToString::to_string)
                .unwrap_or_default();
            Effects::default()
        });
        screen
    }

    /// Moves the focus down to the row that begins with `label` and chooses it.
    fn choose(&mut self, label: &str) {
        for _ in 0..80 {
            let shown = self.shown();
            if shown
                .split(" | ")
                .any(|part| part.strip_prefix('*').is_some_and(|p| p.starts_with(label)))
            {
                return self.keys("run:core.confirm");
            }
            self.keys("run:core.down");
        }
        panic!("{label} is not on the screen: {}", self.shown());
    }
}

#[test]
fn the_list_of_the_library_is_fetched_and_shown() {
    let mut shop = Shop::new(&[
        Offered::data("pack-a", "Pack A", "1.0.0"),
        Offered::data("pack-b", "Pack B", "1.0.0"),
    ]);
    shop.run("store.open");
    let shown = shop.shown();
    assert_eq!(shop.screen(), "store.list", "{shown}");
    assert!(
        shop.library
            .asked()
            .iter()
            .any(|url| url.starts_with(&format!("{BASE}plugins.json?device=x4"))),
        "{:?}",
        shop.library.asked()
    );
    assert!(shown.contains("Plugin store"), "{shown}");
}

/// Opens the page of Pack A and chooses *Install*, as the user does; the
/// store downloads the plugin and the host asks the user to confirm it.
fn install_pack_a(shop: &mut Shop) {
    shop.keys("open:store.plugin:id=pack-a");
    shop.settle(3000);
    shop.choose("Install");
    shop.settle(3000);
}

/// The host's confirmation is open; its input guard (`device.md` 5.0) is over.
fn confirm_screen(shop: &mut Shop) -> String {
    shop.settle(1500);
    shop.screen()
}

#[test]
fn a_plugin_is_downloaded_confirmed_by_the_user_and_installed() {
    let mut shop = Shop::new(&[Offered::data("pack-a", "Pack A", "1.0.0")]);
    install_pack_a(&mut shop);
    assert_eq!(confirm_screen(&mut shop), "core.firmware-confirm");
    let asked = shop.shown();
    assert!(asked.contains("Pack A 1.0.0 (pack-a)"), "{asked}");
    shop.keys("run:core.up");
    shop.keys("run:core.confirm");
    shop.settle(3000);
    assert_eq!(shop.screen(), "store.plugin");
    let shown = shop.shown();
    assert!(shown.contains("Installed: version 1.0.0"), "{shown}");
    assert!(shop.card.exists("plugins/pack-a/plugin.ini"));
    assert!(
        !shop
            .card
            .exists(".paperpaw/plugins/store/stage/pack-a/plugin.ini")
    );
    assert!(
        shop.library
            .asked()
            .iter()
            .any(|url| url == &format!("{BASE}plugins/pack-a/1.0.0/plugin.ini")),
        "{:?}",
        shop.library.asked()
    );
}

#[test]
fn a_declined_install_leaves_the_card_as_it_was() {
    let mut shop = Shop::new(&[Offered::data("pack-a", "Pack A", "1.0.0")]);
    install_pack_a(&mut shop);
    assert_eq!(confirm_screen(&mut shop), "core.firmware-confirm");
    // The focus starts on *Cancel*.
    shop.keys("run:core.confirm");
    shop.settle(3000);
    let shown = shop.shown();
    assert_eq!(shop.screen(), "store.plugin", "{shown}");
    assert!(shown.contains("*Install"), "{shown}");
    assert!(!shop.card.exists("plugins/pack-a/plugin.ini"));
    assert!(
        !shop
            .card
            .exists(".paperpaw/plugins/store/stage/pack-a/plugin.ini")
    );
}

#[test]
fn files_a_power_cut_left_staged_are_removed_at_the_start() {
    // The hand-over to the host had removed the download's job file: only the
    // staged folder was left.
    let card = MemoryStorage::new().with(".paperpaw/plugins/store/stage/pack-a/plugin.ini", "x");
    let shop = Shop::on(card, &[Offered::data("pack-a", "Pack A", "1.0.0")]);
    assert!(
        !shop
            .card
            .exists(".paperpaw/plugins/store/stage/pack-a/plugin.ini")
    );
}

/// Chooses `label` in the dialog the store asks: its input guard is over first.
fn answer_dialog(shop: &mut Shop, label: &str) {
    shop.settle(1500);
    shop.choose(label);
    shop.settle(3000);
}

/// Installs plugin `id` from its page and confirms the host's question.
fn install_and_confirm(shop: &mut Shop, id: &str) {
    // Through the list, as the user comes: the plugins' names are known then.
    shop.run("store.open");
    shop.settle(2000);
    shop.keys(&format!("open:store.plugin:id={id}"));
    shop.settle(3000);
    shop.choose("Install");
    shop.settle(3000);
    assert_eq!(confirm_screen(shop), "core.firmware-confirm");
    shop.keys("run:core.up");
    shop.keys("run:core.confirm");
    shop.settle(3000);
}

/// A plugin that requires one the device lacks (Arabic menus need the Arabic
/// letters): once installed the store offers it with its size.
#[test]
fn what_the_installed_plugin_requires_is_offered_and_fetched_on_download() {
    let mut shop = Shop::new(&[
        Offered::requiring("language-ar", "Arabic menus", "1.0.0", "script-arabic"),
        Offered::data("script-arabic", "Arabic letters", "1.0.0"),
    ]);
    install_and_confirm(&mut shop, "language-ar");
    assert!(shop.card.exists("plugins/language-ar/plugin.ini"));
    let shown = shop.shown();
    assert!(shown.contains("The plugin just installed also"), "{shown}");
    assert!(shown.contains("Arabic letters"), "{shown}");
    // Nothing is fetched before the reader says so.
    assert!(!shop.card.exists("plugins/script-arabic/plugin.ini"));
    answer_dialog(&mut shop, "Download");
    // The host asks to confirm the second install too.
    assert_eq!(confirm_screen(&mut shop), "core.firmware-confirm");
    shop.keys("run:core.up");
    shop.keys("run:core.confirm");
    shop.settle(3000);
    // The host lets no plugin close a screen from an event: the fetch screen
    // shows the end and a key closes it.
    assert_eq!(shop.screen(), "store.fetch");
    assert!(shop.shown().contains("100 %"), "{}", shop.shown());
    shop.keys("run:core.confirm");
    assert_ne!(shop.screen(), "store.fetch", "{}", shop.shown());
    assert!(shop.card.exists("plugins/script-arabic/plugin.ini"));
    let shown = shop.shown();
    assert!(!shown.contains("just installed also"), "{shown}");
}

#[test]
fn not_now_leaves_the_plugin_installed_and_fetches_nothing() {
    let mut shop = Shop::new(&[
        Offered::requiring("language-ar", "Arabic menus", "1.0.0", "script-arabic"),
        Offered::data("script-arabic", "Arabic letters", "1.0.0"),
    ]);
    install_and_confirm(&mut shop, "language-ar");
    assert!(shop.shown().contains("just installed also"));
    answer_dialog(&mut shop, "Not now");
    assert!(shop.card.exists("plugins/language-ar/plugin.ini"));
    assert!(!shop.card.exists("plugins/script-arabic/plugin.ini"));
    assert!(!shop.shown().contains("just installed also"));
    assert!(
        !shop
            .library
            .asked()
            .iter()
            .any(|url| url.contains("script-arabic/1.0.0")),
        "{:?}",
        shop.library.asked()
    );
}

#[test]
fn what_is_on_the_device_or_not_in_the_library_is_not_asked_for() {
    let have = MemoryStorage::new().with(
        "plugins/script-arabic/plugin.ini",
        "[plugin]\nid = script-arabic\nname = Arabic letters\nversion = 1.0.0\nformats = 4\n",
    );
    let mut shop = Shop::on(
        have,
        &[Offered::requiring(
            "language-ar",
            "Arabic menus",
            "1.0.0",
            "script-arabic, not-in-the-library",
        )],
    );
    install_and_confirm(&mut shop, "language-ar");
    assert!(shop.card.exists("plugins/language-ar/plugin.ini"));
    let shown = shop.shown();
    assert!(!shown.contains("just installed also"), "{shown}");
}

/// Alpha requires Beta, Beta requires Gamma: a dialog for each step.
#[test]
fn a_chain_of_requirements_is_a_dialog_for_each_step() {
    let mut shop = Shop::new(&[
        Offered::requiring("alpha", "Alpha", "1.0.0", "beta"),
        Offered::requiring("beta", "Beta", "1.0.0", "gamma"),
        Offered::data("gamma", "Gamma", "1.0.0"),
    ]);
    install_and_confirm(&mut shop, "alpha");
    assert!(shop.shown().contains("Beta"), "{}", shop.shown());
    answer_dialog(&mut shop, "Download");
    // The host asks to confirm the second install too.
    assert_eq!(confirm_screen(&mut shop), "core.firmware-confirm");
    shop.keys("run:core.up");
    shop.keys("run:core.confirm");
    shop.settle(3000);
    let shown = shop.shown();
    assert!(shown.contains("just installed also"), "{shown}");
    assert!(shown.contains("Gamma"), "{shown}");
    assert!(shop.card.exists("plugins/beta/plugin.ini"));
}

/// Alpha and Beta require each other: nothing is asked after the second.
#[test]
fn two_plugins_requiring_each_other_end_after_the_second() {
    let mut shop = Shop::new(&[
        Offered::requiring("alpha", "Alpha", "1.0.0", "beta"),
        Offered::requiring("beta", "Beta", "1.0.0", "alpha"),
    ]);
    install_and_confirm(&mut shop, "alpha");
    answer_dialog(&mut shop, "Download");
    // The host asks to confirm the second install too.
    assert_eq!(confirm_screen(&mut shop), "core.firmware-confirm");
    shop.keys("run:core.up");
    shop.keys("run:core.confirm");
    shop.settle(3000);
    assert!(shop.card.exists("plugins/beta/plugin.ini"));
    let shown = shop.shown();
    assert!(!shown.contains("just installed also"), "{shown}");
}

/// A long summary shows whole on the plugin's page (four lines), not cut after
/// two with "…".
#[test]
fn the_page_of_a_plugin_shows_its_long_summary_whole() {
    let mut shop = Shop::new(&[Offered::data("pack-a", "Pack A", "1.0.0").summarised(
        "Reading aids for the words: bold starts of words, wider spacing of letters, \
         words and lines, and a calmer type for those who read with dyslexia.",
    )]);
    shop.run("store.open");
    shop.settle(2000);
    shop.keys("open:store.plugin:id=pack-a");
    shop.settle(3000);
    let shown = shop.shown();
    assert!(shown.contains("with dyslexia."), "{shown}");
    assert!(!shown.contains('\u{2026}'), "{shown}");
}

/// A route that ends every request with status 0 only after a while, as a
/// refused connection does on the real network (`never`: not at all).
#[derive(Default)]
struct Unreachable {
    never: bool,
    waiting: Vec<u32>,
    polls: u32,
}

impl Route for Unreachable {
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        self.waiting.push(call.id);
        None
    }

    fn later(&mut self) -> Option<HttpResponse> {
        self.polls += 1;
        if self.never || self.polls < 20 {
            return None;
        }
        let id = self.waiting.pop()?;
        Some(HttpResponse {
            id,
            status: 0,
            ..HttpResponse::default()
        })
    }
}

/// What the panel shows once the store was opened and the library answered
/// `never` or refused the connection.
fn panel_of_the_store(never: bool) -> Vec<u8> {
    let mut shop = Shop::new(&[]);
    set_route(Some(Box::new(Unreachable {
        never,
        ..Unreachable::default()
    })));
    // Not through the script: it would wait for the library as long as it takes.
    let open = paperpaw_core::sdk::Id::parse("store.open").unwrap();
    shop.device.run_action(open);
    shop.settle(6000);
    shop.device.panel.rgb_bytes()
}

/// The list's end without an answer is drawn: the screen that was loading says
/// there is no connection (a screen shown by its rows alone would stay as it
/// was drawn).
#[test]
fn a_list_that_cannot_be_fetched_is_redrawn_as_no_connection() {
    assert_ne!(panel_of_the_store(false), panel_of_the_store(true));
}

/// The plugins "Pack 01" to "Pack 40" of the library, in that order.
fn many_packs() -> Vec<Offered> {
    (1..=40)
        .map(|n| {
            let id: &'static str = Box::leak(format!("pack-{n:02}").into_boxed_str());
            let name: &'static str = Box::leak(format!("Pack {n:02}").into_boxed_str());
            Offered::data(id, name, "1.0.0")
        })
        .collect()
}

/// A list of more plugins than a page holds: a hold of Select asks which page to
/// go to ("Go to page (1 to N)"), and the number typed puts the focus on the
/// first plugin of that page; a number past the last page is the last one.
#[test]
fn a_long_list_jumps_to_the_page_typed() {
    let mut shop = Shop::new(&many_packs());
    shop.run("store.open");
    shop.settle(2000);
    shop.keys("open:store.view:view=new");
    shop.settle(2000);
    let first = shop.shown();
    let per_page = first.matches("Pack ").count();
    assert!(per_page > 1 && per_page < 40, "{first}");
    assert!(first.contains("*Pack 01"), "{first}");
    // The hold of Select asks for the page.
    shop.keys("run:core.menu");
    shop.settle(1500);
    let asked = shop.shown();
    assert!(asked.contains("Go to page (1 to"), "{asked}");
    shop.keys("type-enter:2");
    shop.settle(1000);
    let shown = shop.shown();
    let second = format!("*Pack {:02}", per_page + 1);
    assert!(shown.contains(&second), "wanted {second} in {shown}");
}

/// The list of the store drawn in part: what the panel shows after each step
/// of the focus (the rows drawn put over the last frame) is what drawing the
/// whole screen anew shows. Covers what `tests/partial_redraw.rs` did for the
/// interface-6 store, which cannot run there (that harness wires the network
/// offline).
#[test]
fn the_list_of_the_store_is_drawn_in_part_as_it_is_drawn_whole() {
    let mut shop = Shop::new(&many_packs());
    shop.run("store.open");
    shop.settle(2000);
    shop.keys("open:store.view:view=new");
    shop.settle(2000);
    let mut last = shop.device.panel.rgb_bytes();
    let mut changed = 0;
    for step in 0..12 {
        let key = if step < 8 {
            "run:core.down"
        } else {
            "run:core.up"
        };
        shop.keys(key);
        let in_part = shop.device.panel.rgb_bytes();
        changed += usize::from(in_part != last);
        last = in_part.clone();
        shop.device.draw_again();
        shop.settle(300);
        assert!(
            shop.device.panel.rgb_bytes() == in_part,
            "step {step}: drawn in part, the panel is not what the whole screen shows"
        );
    }
    // Each step moved the focus and drew something.
    assert!(changed >= 10, "{changed} of 12 steps changed the panel");
}

/// The card of a device that has `pack-a` installed at `version` (its manifest
/// as the library serves it).
fn card_with_pack_a(version: &str) -> MemoryStorage {
    let manifest = Offered::data("pack-a", "Pack A", "0").manifest;
    let manifest = manifest.replace("version = 0", &format!("version = {version}"));
    MemoryStorage::new().with("plugins/pack-a/plugin.ini", manifest)
}

/// An installed plugin whose library has a newer version is updated from its
/// page: downloaded, confirmed by the user, and replaced whole (the plugin
/// folder is the new version's, the staged files are gone).
#[test]
fn an_installed_plugin_is_updated_to_the_newer_version_of_the_library() {
    let mut shop = Shop::on(
        card_with_pack_a("1.0.0"),
        &[Offered::data("pack-a", "Pack A", "1.1.0")],
    );
    shop.run("store.open");
    shop.settle(2000);
    shop.keys("open:store.plugin:id=pack-a");
    shop.settle(3000);
    shop.choose("Update");
    shop.settle(3000);
    assert_eq!(confirm_screen(&mut shop), "core.firmware-confirm");
    shop.keys("run:core.up");
    shop.keys("run:core.confirm");
    shop.settle(3000);
    let shown = shop.shown();
    assert!(shown.contains("Installed: version 1.1.0"), "{shown}");
    let mut ini = [0_u8; 512];
    let n = shop
        .card
        .read_at("plugins/pack-a/plugin.ini", 0, &mut ini)
        .unwrap();
    assert!(String::from_utf8_lossy(&ini[..n]).contains("version = 1.1.0"));
    assert!(
        !shop
            .card
            .exists(".paperpaw/plugins/store/stage/pack-a/plugin.ini")
    );
}

/// Removing asks first (*Remove* or *Keep*), then the host asks again before
/// it touches the card; a *Keep* at the first question changes nothing, a
/// confirmed removal takes the plugin's folder away.
#[test]
fn an_installed_plugin_is_removed_after_the_reader_confirmed_twice() {
    let mut shop = Shop::on(
        card_with_pack_a("1.0.0"),
        &[Offered::data("pack-a", "Pack A", "1.0.0")],
    );
    shop.run("store.open");
    shop.settle(2000);
    shop.keys("open:store.plugin:id=pack-a");
    shop.settle(3000);
    shop.choose("Remove");
    answer_dialog(&mut shop, "Keep");
    assert!(shop.card.exists("plugins/pack-a/plugin.ini"));
    shop.choose("Remove");
    answer_dialog(&mut shop, "Remove");
    assert_eq!(confirm_screen(&mut shop), "core.firmware-confirm");
    shop.keys("run:core.up");
    shop.keys("run:core.confirm");
    shop.settle(3000);
    assert!(!shop.card.exists("plugins/pack-a/plugin.ini"));
}
