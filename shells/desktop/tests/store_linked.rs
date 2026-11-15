//! The store plugins the emulator links (#277, option A): `stats` and the others
//! of `paperpaw_app::LINKED_STORE_PLUGINS` are built into the emulator, shipped off,
//! and "install" is *Switch on* in the store, "remove" is switching off in the plugin manager. The emulator
//! cannot load a plugin file the X4 family installs (it runs ELF libraries, and the
//! install check of the core takes only `code.ppn`), so the store plugins ride along
//! as linked ones, as aids and widgets do. Driven through the buttons as
//! `store_native.rs` does: the real kernel, the card in memory, the library answered
//! from recorded answers.
#![cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
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

/// The library's recorded answers by address without its query.
#[derive(Clone, Default)]
struct Library {
    answers: Rc<RefCell<HashMap<String, Vec<u8>>>>,
}

impl Library {
    fn put(&self, url: &str, body: impl Into<Vec<u8>>) {
        self.answers
            .borrow_mut()
            .insert(url.to_string(), body.into());
    }
}

struct Recorded(Library);

impl Route for Recorded {
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
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
    kind: &'static str,
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
            kind: "data",
        }
    }

    fn entry(&self) -> String {
        format!(
            r#"{{"id": "{}", "name": "{}", "summary": "{}", "group": "reading",
  "author": "PaperPaw", "licence": "OFL-1.1", "kind": "{}",
  "added": "2026-01-01", "latest": {{"version": "{}", "sdk": "0.4", "formats": 4,
  "size": {}, "date": "2026-01-02"}}}}"#,
            self.id,
            self.name,
            self.summary,
            self.kind,
            self.version,
            self.manifest.len()
        )
    }

    fn page(&self) -> String {
        format!(
            r#"{{"format": 2, "plugin": {{"id": "{id}", "name": "{name}",
  "summary": "{summary}", "group": "reading", "author": "PaperPaw",
  "licence": "OFL-1.1", "kind": "{kind}",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 4, "size": {size},
    "files": [{{"path": "plugin.ini", "url": "{id}/{version}/plugin.ini",
                "size": {size}, "sha1": "{sha}"}}]}}}}}}"#,
            id = self.id,
            name = self.name,
            summary = self.summary,
            kind = self.kind,
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
        let mut shop = Self { device };
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

    /// Whether the kernel has the action `id` (the plugin is hosted).
    fn has_action(&mut self, id: &str) -> bool {
        let mut found = false;
        self.device.call(|kernel, _| {
            found = kernel.actions().contains(id);
            Effects::default()
        });
        found
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

/// The library offers `stats` as a code plugin of the X4 (a plugin file).
fn offered_stats() -> Offered {
    let mut stats = Offered::data("stats", "Reading statistics", "1.0.9");
    stats.kind = "code";
    stats
}

#[test]
fn the_page_of_a_linked_store_plugin_offers_switch_on() {
    let mut shop = Shop::new(&[offered_stats()]);
    shop.run("store.open");
    shop.settle(2000);
    shop.keys("open:store.plugin:id=stats");
    shop.settle(3000);
    let shown = shop.shown();
    assert!(
        !shown.contains("no longer offers"),
        "the page says the library does not offer the plugin: {shown}"
    );
    assert!(shown.contains("Switch on"), "{shown}");
}

#[test]
fn a_linked_store_plugin_is_switched_on_used_kept_over_a_restart_and_switched_off() {
    let mut shop = Shop::new(&[offered_stats()]);
    // Shipped off: its action is not there.
    assert!(!shop.has_action("stats.open"));
    shop.run("store.open");
    shop.settle(2000);
    shop.keys("open:store.plugin:id=stats");
    shop.settle(3000);
    shop.choose("Switch on");
    shop.settle(3000);
    // Switched on for the next start, as in Beta 3: not hosted until then.
    assert!(!shop.has_action("stats.open"), "hosted before the restart");
    shop.device.restart();
    shop.settle(3000);
    // On: it is used.
    assert!(shop.has_action("stats.open"), "switched on, not hosted");
    shop.run("stats.open");
    assert_eq!(shop.screen(), "stats.overview", "{}", shop.shown());
    // Kept over a restart.
    shop.device.restart();
    shop.settle(3000);
    assert!(shop.has_action("stats.open"), "off again after a restart");
    // On, the library no longer offers it (the store lists a built-in plugin
    // while it is off): it is switched off where it is listed, the plugin
    // manager, and again after a restart it is gone.
    shop.keys("open:plugin-manager.list");
    shop.settle(2000);
    shop.choose("Reading statistics");
    shop.settle(2000);
    shop.device.restart();
    shop.settle(3000);
    assert!(!shop.has_action("stats.open"), "on again after a restart");
}
