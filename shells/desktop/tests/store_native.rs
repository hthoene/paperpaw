//! The native plugin store, run end to end in the emulator: the real kernel with
//! the linked store, the card in memory and the plugin library answered from
//! recorded answers (the route the emulator sends its requests through instead
//! of the network). What the interface-6 store tests drove on a hosted kernel
//! is driven here through the buttons: the list, a plugin's page, the install,
//! the update, the removal, what other plugins ask for, *Discover fonts*.
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
        }
    }

    fn entry(&self) -> String {
        format!(
            r#"{{"id": "{}", "name": "{}", "summary": "A pack.", "group": "reading",
  "author": "PaperPaw", "licence": "OFL-1.1", "kind": "data",
  "added": "2026-01-01", "latest": {{"version": "{}", "sdk": "0.4", "formats": 4,
  "size": {}, "date": "2026-01-02"}}}}"#,
            self.id,
            self.name,
            self.version,
            self.manifest.len()
        )
    }

    fn page(&self) -> String {
        format!(
            r#"{{"format": 2, "plugin": {{"id": "{id}", "name": "{name}",
  "summary": "A pack.", "group": "reading", "author": "PaperPaw",
  "licence": "OFL-1.1", "kind": "data",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 4, "size": {size},
    "files": [{{"path": "plugin.ini", "url": "{id}/{version}/plugin.ini",
                "size": {size}, "sha1": "{sha}"}}]}}}}}}"#,
            id = self.id,
            name = self.name,
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

/// Wakes the kernel and waits out the dialog's input guard (`device.md` 5.0).
fn confirm_screen(shop: &mut Shop) -> String {
    shop.device.call(|kernel, now| kernel.tick(now));
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
