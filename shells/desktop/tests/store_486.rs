//! The plugin store's queue and setup offer end to end (#486), in the emulator
//! as `store_native.rs` runs it: the real kernel with the linked store, the
//! card in memory, the library answered from recorded answers. Covers what the
//! store's own tests cannot reach without a host: the offer left with Back,
//! the queue that goes on when the store is opened, and what is said of a
//! plugin whose page did not come.
#![cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use paperpaw_core::sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use paperpaw_core::{Effects, HttpCall, HttpResponse};
use paperpaw_emulator::device::Device;
use paperpaw_emulator::headless::run_script;
use paperpaw_emulator::http::{Route, set_route};
use paperpaw_emulator::profiles;

const BASE: &str = "https://paperpaw.org/api/v1/";
/// The store's data folder on the card.
const STORE: &str = ".paperpaw/plugins/store/";
/// The settings of the card: a notice stays up for the test to read it.
const SETTINGS: &str = "[core]\nnotice-seconds = 60\n[help]\nshow = false\n";

/// The library's recorded answers by address without its query; what was asked
/// is kept in order.
#[derive(Clone, Default)]
struct Library {
    answers: Rc<RefCell<HashMap<String, Vec<u8>>>>,
    asked: Rc<RefCell<Vec<String>>>,
    /// The answer held back ([`Shop::holding`]) may come.
    released: Rc<Cell<bool>>,
}

impl Library {
    fn put(&self, url: &str, body: impl Into<Vec<u8>>) {
        self.answers
            .borrow_mut()
            .insert(url.to_string(), body.into());
    }

    /// The library no longer answers for `url`.
    fn drop_answer(&self, url: &str) {
        self.answers.borrow_mut().remove(url);
    }

    /// The answer held back comes with the next poll of the network.
    fn release(&self) {
        self.released.set(true);
    }

    /// Whether `url` (without its query) was asked for.
    fn was_asked(&self, url: &str) -> bool {
        self.asked
            .borrow()
            .iter()
            .any(|asked| asked.split('?').next() == Some(url))
    }
}

/// The library's answers; the one for `held` (an address) only once the test
/// released it ([`Library::release`]).
struct Recorded {
    library: Library,
    held: Option<String>,
    waiting: Option<HttpResponse>,
}

impl Route for Recorded {
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        self.library.asked.borrow_mut().push(call.url.clone());
        let path = call.url.split('?').next().unwrap_or("");
        let found = self.library.answers.borrow().get(path).cloned();
        let answer = HttpResponse {
            id: call.id,
            status: if found.is_some() { 200 } else { 404 },
            body: found.unwrap_or_default(),
            ..HttpResponse::default()
        };
        if self.held.as_deref() == Some(path) {
            self.waiting = Some(answer);
            return None;
        }
        Some(answer)
    }

    fn later(&mut self) -> Option<HttpResponse> {
        if !self.library.released.get() {
            return None;
        }
        self.waiting.take()
    }
}

/// A data plugin of the library: its list entry, its page and its manifest.
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
    "files": [{{"path": "plugin.ini", "url": "{id}/{version}/plugin.ini", "size": {size},
      "sha1": "{sha1}"}}]}}}}}}"#,
            id = self.id,
            name = self.name,
            version = self.version,
            size = self.manifest.len(),
            sha1 = paperpaw_digest::sha1::hex(self.manifest.as_bytes())
        )
    }

    fn page_url(&self) -> String {
        format!("{BASE}plugins/{}.json", self.id)
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
        library.put(&plugin.page_url(), plugin.page());
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
    /// The X4 started on `card` (with [`SETTINGS`] and the setup done), the
    /// library serving `plugins`.
    fn on(card: MemoryStorage, plugins: &[Offered]) -> Self {
        Self::holding(card, plugins, None)
    }

    /// As [`on`](Self::on), the answer for the address `held` coming late.
    fn holding(card: MemoryStorage, plugins: &[Offered], held: Option<String>) -> Self {
        let card = set_up(card);
        let library = answering(plugins, held);
        let device = Device::in_memory(x4(), None, card.clone(), true, None).unwrap();
        Self::started(device, card, library)
    }

    /// As [`on`](Self::on), on a card that refuses every change once the test
    /// says it is full ([`FullCard::full`]).
    fn on_full_card(card: MemoryStorage, plugins: &[Offered]) -> (Self, Rc<FullCard>) {
        let files = set_up(card);
        let full = Rc::new(FullCard {
            files: files.clone(),
            full: Cell::new(false),
            refused: RefCell::default(),
        });
        let library = answering(plugins, None);
        let device = Device::on_storage(x4(), full.clone(), true).unwrap();
        (Self::started(device, files, library), full)
    }

    fn started(mut device: Device, card: Rc<MemoryStorage>, library: Library) -> Self {
        // On USB in a network joined before, as a reader opens the store: the
        // emulator brings no network up for a request by itself (as the X4).
        device.set_wifi_online(true);
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

    /// Steps of a script: `run:action`, `open:screen`, as the headless runs.
    fn keys(&mut self, script: &str) {
        run_script(&mut self.device, script).unwrap();
        self.settle(500);
    }

    /// The labels of what the screen shows (a notice over it included), in
    /// order; the focused one starts with `*`.
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

/// `card` with [`SETTINGS`] and the setup done.
fn set_up(card: MemoryStorage) -> Rc<MemoryStorage> {
    Rc::new(
        card.with(".paperpaw/plugins/setup/done", "")
            .with("paperpaw.ini", SETTINGS),
    )
}

/// The library serving `plugins`, the answer for the address `held` coming
/// late.
fn answering(plugins: &[Offered], held: Option<String>) -> Library {
    let library = Library::default();
    serve(&library, plugins);
    set_route(Some(Box::new(Recorded {
        library: library.clone(),
        held,
        waiting: None,
    })));
    paperpaw_emulator::budget::override_limit(None);
    library
}

fn x4() -> &'static profiles::Profile {
    profiles::find("xteink-x4").expect("the X4 profile")
}

/// A card that refuses every change while it is `full`; the paths refused, in
/// order.
struct FullCard {
    files: Rc<MemoryStorage>,
    full: Cell<bool>,
    refused: RefCell<Vec<String>>,
}

impl FullCard {
    fn change(&self, path: &str) -> Result<(), StorageError> {
        if self.full.get() {
            self.refused.borrow_mut().push(path.to_string());
            Err(StorageError::Full)
        } else {
            Ok(())
        }
    }
}

impl Storage for FullCard {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.change(path)?;
        self.files.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.change(path)?;
        self.files.append(path, data)
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        self.change(path)?;
        self.files.write_at(path, offset, data)
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        self.change(path)?;
        self.files.truncate(path, len)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.change(path)?;
        self.files.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.change(from)?;
        self.files.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.change(path)?;
        self.files.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.change(path)?;
        self.files.remove_dir_all(path)
    }
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        self.files.entry_identity(path)
    }
    fn free_space(&self) -> Option<u64> {
        if self.full.get() {
            Some(0)
        } else {
            self.files.free_space()
        }
    }
}

/// The card of a device that has `pack-a` installed at `version`.
fn card_with_pack_a(version: &str) -> MemoryStorage {
    let manifest = Offered::data("pack-a", "Pack A", "0").manifest;
    let manifest = manifest.replace("version = 0", &format!("version = {version}"));
    MemoryStorage::new().with("plugins/pack-a/plugin.ini", manifest)
}

/// Fix 1: the plugins chosen on the setup's offer are added when the page is
/// left with Back, as *Done* adds them (the step of leaving was never run).
#[test]
fn the_plugins_chosen_on_the_offer_are_added_when_it_is_left_with_back() {
    let mut shop = Shop::on(MemoryStorage::new(), &[]);
    shop.keys("open:store.offer");
    shop.settle(1000);
    assert_eq!(shop.screen(), "store.offer", "{}", shop.shown());
    // The focus starts on the first plugin offered: it is chosen.
    shop.keys("run:core.confirm");
    let shown = shop.shown();
    assert!(shown.contains("Chosen"), "{shown}");
    shop.keys("run:core.back");
    shop.settle(1000);
    assert_ne!(shop.screen(), "store.offer", "{}", shop.shown());
    let chosen = format!("{STORE}offer-chosen.txt");
    assert!(shop.card.exists(&chosen), "nothing was added");
}

/// Fix 5: an update left waiting in the queue (a restart, no connection) goes
/// on when the store is opened, as in Beta 3, not only at the next install.
#[test]
fn an_update_waiting_in_the_queue_goes_on_when_the_store_is_opened() {
    let card = card_with_pack_a("1.0.0").with(&format!("{STORE}queue.txt"), "pack-a\n");
    let newer = Offered::data("pack-a", "Pack A", "1.1.0");
    let page = newer.page_url();
    let mut shop = Shop::on(card, &[newer]);
    // An update is no quiet plugin: nothing goes on at the start.
    assert!(!shop.library.was_asked(&page));
    shop.keys("run:store.open");
    shop.settle(3000);
    assert!(shop.library.was_asked(&page), "the queue did not go on");
    // Its download is here: the host asks to confirm the install.
    shop.settle(1500);
    assert_eq!(shop.screen(), "core.firmware-confirm", "{}", shop.shown());
}

/// Fix 4: a plugin the setup put in quietly whose page did not come is named in
/// the summary at the queue's end (it said "Could not be added: , .").
#[test]
fn a_quiet_plugin_whose_page_did_not_come_is_named_in_the_summary() {
    // A quiet queue goes on at the start; the library has no page of the
    // plugin, and says so once the store is open.
    let page = format!("{BASE}plugins/kosync.json");
    let card = MemoryStorage::new().with(&format!("{STORE}queue.txt"), "~kosync\n");
    let mut shop = Shop::holding(card, &[], Some(page.clone()));
    assert!(shop.library.was_asked(&page));
    assert!(shop.card.exists(&format!("{STORE}queue.txt")));
    // With the store open (not through the script: it would wait for the
    // answer held back).
    let open = paperpaw_core::sdk::Id::parse("store.open").unwrap();
    shop.device.run_action(open);
    shop.settle(1000);
    shop.library.release();
    shop.settle(2000);
    let shown = shop.shown();
    assert!(
        shown.contains("Could not be added: Progress sync."),
        "{shown}"
    );
    assert!(!shop.card.exists(&format!("{STORE}queue.txt")));
}

/// Fix 4: *Update all* says nothing of a plugin whose page did not come, as in
/// Beta 3 (it said " could not be downloaded", without a name); it stays
/// listed with its update.
#[test]
fn update_all_says_nothing_of_a_plugin_whose_page_did_not_come() {
    let newer = Offered::data("pack-a", "Pack A", "1.1.0");
    let page = newer.page_url();
    let mut shop = Shop::on(card_with_pack_a("1.0.0"), &[newer]);
    shop.library.drop_answer(&page);
    shop.keys("run:store.open");
    shop.settle(2000);
    shop.keys("open:store.view:view=installed");
    shop.settle(2000);
    shop.choose("Update all");
    shop.settle(3000);
    assert!(shop.library.was_asked(&page), "no page was asked for");
    let shown = shop.shown();
    assert!(!shown.contains("could not be downloaded"), "{shown}");
    assert!(!shop.card.exists(&format!("{STORE}queue.txt")));
    assert!(shop.card.exists("plugins/pack-a/plugin.ini"));
}

/// #495: a card that refuses the queue (full) gives no plugin its turn. The
/// turn it did not keep was given again by the queue read next: the store
/// asked for the same page for ever, inside one call where the request failed
/// at once (the device hung), else at each answer (here: each poll, the page
/// refused by the card each time).
#[test]
fn a_full_card_gives_no_turn_and_the_store_still_opens() {
    let (done, ended) = std::sync::mpsc::channel();
    // The store's own thread: a loop that never ends is told by the clock.
    std::thread::spawn(move || {
        let newer = Offered::data("pack-a", "Pack A", "1.1.0");
        let (mut shop, card) = Shop::on_full_card(card_with_pack_a("1.0.0"), &[newer]);
        // The library was listed once, while the card had room.
        shop.keys("run:store.open");
        shop.settle(2000);
        for _ in 0..3 {
            shop.keys("run:core.back");
        }
        assert!(!shop.screen().starts_with("store."), "{}", shop.shown());
        shop.card
            .write(&format!("{STORE}queue.txt"), b"pack-a\n")
            .unwrap();
        card.full.set(true);
        shop.keys("run:store.open");
        shop.settle(3000);
        let pages = card
            .refused
            .borrow()
            .iter()
            .filter(|path| path.contains("queue-pack-a"))
            .count();
        let _ = done.send((pages, shop.screen()));
    });
    let (pages, screen) = match ended.recv_timeout(Duration::from_secs(120)) {
        Ok(seen) => seen,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("the store hung"),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("the run failed"),
    };
    assert_eq!(
        pages, 0,
        "the page was asked for though the queue is not kept"
    );
    assert_eq!(screen, "store.list");
}
