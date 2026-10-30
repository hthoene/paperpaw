//! Tests of the plugin: its parts, and syncing against a sync server
//! played by the tests, through the plugin's own handlers.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_i18n::{Catalog, Translator, check};
use paperpaw_sdk::builtin::events::{self, Reading, SettingChanged};
use paperpaw_sdk::secrets::{SecretChange, Secrets};
use paperpaw_sdk::{
    ActionHandler, Actions, Capabilities, Command, Contribution, DeviceInfo, DisplayInfo, Env,
    EnvParts, Event, EventData, EventHandler, Fonts, Formats, HttpReply, HttpRequest, Hyphenation,
    Id, InputCaps, Instant, MemoryClass, MemoryStorage, Menus, Metrics, Params, PixelFormat,
    Plugin, Plugins, Registrar, SettingValue, Settings, StatusItems, SystemStatus,
};

use super::*;

#[test]
fn translations_are_complete() {
    let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
    let issues = check::compare(&en, &de);
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn positions_come_from_reading_events() {
    let found = position(&{
        let mut moved = reading(42.5);
        moved.section = 3;
        moved.offset = 120;
        moved
    });
    assert_eq!(found.path, BOOK);
    assert_eq!(found.at, Locator::new(3, 120));
    assert!((found.progress - 0.425).abs() < 1e-6);
}

#[test]
fn server_addresses_are_completed() {
    assert_eq!(normalize_server(""), DEFAULT_SERVER);
    assert_eq!(
        normalize_server(" sync.example.org/ "),
        "https://sync.example.org"
    );
    assert_eq!(
        normalize_server("http://192.168.1.5:8081"),
        "http://192.168.1.5:8081"
    );
    assert_eq!(
        normalize_server("example.org/kosync"),
        "https://example.org/kosync"
    );
    assert_eq!(
        server_label("https://example.org/kosync"),
        "example.org/kosync"
    );
    assert_eq!(server_label("http://nas:8081"), "http://nas:8081");
}

#[test]
fn failures_have_messages() {
    assert_eq!(failure("0"), "kosync.unreachable");
    assert_eq!(failure("401"), "kosync.wrong");
    assert_eq!(failure("500"), "kosync.server-error");
}

/// What version 1 wrote stays readable (see `docs/data-files.md`).
#[test]
fn files_of_version_1_are_read() {
    let storage = MemoryStorage::new()
        .with("device.ini", include_str!("../testdata/device.ini.v1"))
        .with("account.ini", include_str!("../testdata/account.ini.v1"));
    assert_eq!(
        super::read_value(&storage, "device.ini", "id").as_deref(),
        Some("0123456789ABCDEF0123456789ABCDEF")
    );
    // Kept by earlier versions; moved to the passwords and keys.
    assert_eq!(
        super::read_value(&storage, "account.ini", "key").as_deref(),
        Some("5f4dcc3b5aa765d61d8327deb882cf99")
    );
}

#[test]
fn answers_are_read_as_progress_or_not_at_all() {
    let read = |text: &str| remote_progress(text, &json::object(text));
    assert_eq!(read(r#"{"percentage":0.5,"device":"KOReader"}"#), Some(0.5));
    assert_eq!(read(r#"{"percentage":"0.25"}"#), Some(0.25));
    // A book the server has no progress of.
    assert_eq!(read("{}"), Some(0.0));
    assert_eq!(read(" { } "), Some(0.0));
    assert_eq!(read(r#"{"document":"abc"}"#), Some(0.0));
    // No progress at all: nothing known about the server's.
    assert_eq!(read(""), None);
    assert_eq!(read("<html>Down for maintenance</html>"), None);
    assert_eq!(read(r#"{"percentage":80}"#), None);
    assert_eq!(read(r#"{"percentage":"soon"}"#), None);
}

// Syncing, with the tests as the sync server.

const BOOK: &str = "books/tales.epub";
/// This device's id, kept in the plugin's folder.
const DEVICE_ID: &str = "0123456789ABCDEF0123456789ABCDEF";
const KEY: &str = "5f4dcc3b5aa765d61d8327deb882cf99";

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(
        480,
        800,
        220,
        PixelFormat::Gray4,
        paperpaw_platform::Rotation::Deg0,
        None,
    ),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

struct NoKeys;

impl paperpaw_sdk::keys::KeyBindings for NoKeys {
    fn bound(&self, _: &[&str], _: &str) -> Option<Id> {
        None
    }
}

/// The plugin registered, with what its handlers see: signed in, on a
/// device with network, a book on the storage.
struct Harness {
    settings: Settings,
    i18n: Translator,
    fonts: Fonts,
    formats: Formats,
    hyphenation: Hyphenation,
    menus: Menus,
    actions: Actions,
    plugins: Plugins,
    status: SystemStatus,
    usage: paperpaw_sdk::usage::Usage,
    plugin_usage: RefCell<Vec<paperpaw_sdk::usage::PluginUsage>>,
    status_items: StatusItems,
    text_filters: paperpaw_sdk::filters::TextFilters,
    decorators: paperpaw_sdk::decorations::Decorators,
    page_layers: paperpaw_sdk::layers::PageLayers,
    lookup: paperpaw_sdk::lookup::LookupProviders,
    readers: paperpaw_sdk::readers::Readers,
    widgets: paperpaw_sdk::widgets::Widgets,
    setup: paperpaw_sdk::setup::SetupSteps,
    services: paperpaw_sdk::services::Services,
    secrets: Secrets,
    storage: MemoryStorage,
    now: u64,
    handlers: Vec<(Id, EventHandler)>,
    sync_now: Option<ActionHandler>,
}

impl Harness {
    /// Signed in, syncing by itself if `auto`; started.
    fn new(auto: bool) -> Self {
        let device = format!("{}/device.ini", dir());
        let mut h = Harness {
            settings: Settings::new(),
            i18n: Translator::new(),
            fonts: Fonts::new(16 * 1024),
            formats: Formats::new(),
            hyphenation: Hyphenation::new(),
            menus: Menus::new(),
            actions: Actions::new(),
            plugins: Plugins::new(),
            status: SystemStatus::default(),
            usage: Default::default(),
            plugin_usage: Default::default(),
            status_items: StatusItems::new(),
            text_filters: Default::default(),
            decorators: Default::default(),
            page_layers: Default::default(),
            lookup: Default::default(),
            readers: Default::default(),
            widgets: Default::default(),
            setup: Default::default(),
            services: Default::default(),
            secrets: Secrets::default(),
            storage: MemoryStorage::new()
                .with(BOOK, "the text of a book")
                .with(&device, format!("[device]\nid = {DEVICE_ID}\n")),
            now: 1_000,
            handlers: Vec::new(),
            sync_now: None,
        };
        let mut r = Registrar::new(PLUGIN);
        Kosync.register(&mut r).unwrap();
        for contribution in r.into_contributions() {
            match contribution {
                Contribution::FixedSettings(list) => {
                    for at in 0..list.len() {
                        h.settings.define_fixed(list, at).unwrap();
                    }
                }
                Contribution::SettingGroup(group) => h.settings.add_group(group).unwrap(),
                Contribution::Translations(text) => h.i18n.add(Catalog::parse(text).unwrap()),
                Contribution::Subscription { event, handler, .. } => {
                    h.handlers.push((event, handler))
                }
                Contribution::Action(action) if action.id == SYNC_NOW => {
                    h.sync_now = action.handler;
                }
                _ => {}
            }
        }
        h.i18n.set_language("en");
        h.set(&USER, "ann");
        h.set_bool(&AUTO, auto);
        h.secrets
            .apply(SecretChange::keep(SECRET, "kosync.secret", KEY));
        h.emit(&events::STARTED, ());
        h
    }

    fn set(&mut self, setting: &Setting<Text>, value: &str) {
        let value = SettingValue::Text(Cow::Owned(value.to_string()));
        self.settings.set(setting.id().as_str(), value).unwrap();
    }

    fn set_bool(&mut self, setting: &Setting<bool>, value: bool) {
        let id = setting.id().as_str();
        self.settings.set(id, SettingValue::Bool(value)).unwrap();
    }

    fn env(&self) -> Env<'_> {
        let mut caps = Capabilities::default();
        caps.network = true;
        Env::from(EnvParts {
            device: &DEVICE,
            now: Instant(self.now),
            metrics: Metrics::new(DEVICE.display.dpi),
            settings: &self.settings,
            i18n: &self.i18n,
            fonts: &self.fonts,
            formats: &self.formats,
            hyphenation: &self.hyphenation,
            menus: &self.menus,
            actions: &self.actions,
            plugins: &self.plugins,
            status: &self.status,
            usage: &self.usage,
            plugin_usage: &self.plugin_usage,
            status_items: &self.status_items,
            text_filters: &self.text_filters,
            decorators: &self.decorators,
            page_layers: &self.page_layers,
            lookup: &self.lookup,
            readers: &self.readers,
            stylesheets: &[],
            book_data: &[],
            book_fields: &[],
            book_badges: &[],
            widgets: &self.widgets,
            setup: &self.setup,
            services: &self.services,
            keys: &NoKeys,
            look: paperpaw_sdk::ui::look::Look::none(),
            caps,
            wifi: None,
            bluetooth: None,
            firmware: None,
            native: None,
            power: paperpaw_sdk::power::Power::normal(),
            work: paperpaw_sdk::work::WorkStates::none(),
            reader: true,
            clipboard: false,
            last_input: paperpaw_sdk::keys::InputMode::at_start(&DEVICE.input),
            inputs: &[],
            content: paperpaw_platform::Rect::default(),
            stack: &[],
            slots: &[],
            screens: paperpaw_sdk::ScreenList::default(),
            font_dirs: &[],
            catalog_device: None,
            secrets: &self.secrets,
            running: &paperpaw_sdk::secrets::Anyone,
            random: paperpaw_sdk::random::Random::NONE,
            memory: paperpaw_sdk::memory::Memory::NONE,
            clock: paperpaw_sdk::time::Clock::NONE,
            started: paperpaw_sdk::power::Start::Unknown,
            slept_for: None,
            panel: paperpaw_sdk::realtime::PanelInfo::default(),
            keys_held: &[],
            room: paperpaw_sdk::realtime::Room::default(),
            storage: &self.storage,
        })
    }

    /// Hands event `id` to the plugin's handlers; what they asked for.
    fn dispatch(&self, id: &Id, params: &Params) -> Vec<Command> {
        let env = self.env();
        let mut out = paperpaw_sdk::Commands::new();
        for (event, handler) in &self.handlers {
            if event == id {
                handler(&env, params, &mut out);
            }
        }
        out.into_iter().collect()
    }

    fn emit<P: EventData>(&self, event: &Event<P>, data: P) -> Vec<Command> {
        self.dispatch(event.id(), &data.to_params())
    }

    /// Chooses *Sync progress*.
    fn sync_now(&self) -> Vec<Command> {
        let mut out = paperpaw_sdk::Commands::new();
        (self.sync_now.as_ref().unwrap())(&self.env(), &Params::new(), &mut out);
        out.into_iter().collect()
    }

    /// The server answers `request` with `status` and `body`.
    fn answer(&self, request: &HttpRequest, status: u16, body: &str) -> Vec<Command> {
        let mut reply = HttpReply::new(status, body);
        reply.content_type = Some("application/json".to_string());
        reply.context = request.params().clone();
        self.dispatch(request.reply(), &reply.to_params())
    }

    /// Does what the kernel does with the settings and secrets in `out`.
    fn apply(&mut self, out: Vec<Command>) {
        for command in out {
            match command {
                Command::SetSetting(id, value) => self.settings.set(id.as_str(), value).unwrap(),
                Command::SetSecret(change) => self.secrets.apply(*change),
                _ => {}
            }
        }
    }
}

fn reading(percent: f32) -> Reading {
    Reading::new(
        BOOK.to_string(),
        String::new(),
        percent,
        0,
        (percent * 10.0) as u32,
        String::new(),
    )
}

fn requests(out: &[Command]) -> Vec<&HttpRequest> {
    out.iter()
        .filter_map(|c| match c {
            Command::Http(request) => Some(&**request),
            _ => None,
        })
        .collect()
}

/// The one fetch of the progress in `out`.
fn fetch(out: &[Command]) -> &HttpRequest {
    let found = requests(out);
    assert_eq!(found.len(), 1, "{out:?}");
    assert_eq!(found[0].method(), "GET");
    found[0]
}

/// The progress sent in `out`, if any.
fn sent(out: &[Command]) -> Option<f64> {
    let puts: Vec<_> = requests(out)
        .into_iter()
        .filter(|r| r.method() == "PUT")
        .collect();
    assert!(puts.len() <= 1, "{out:?}");
    let body = json::object(puts.first()?.body_text().unwrap_or_default());
    json::number(&body, "percentage")
}

/// Whether `out` offers to go to another device's position.
fn offers(out: &[Command]) -> bool {
    out.iter().any(|c| {
        matches!(c, Command::OpenScreen(screen, params)
            if *screen == CONFIRM_SCREEN && params.get(confirm::PERCENTAGE).is_some())
    })
}

/// The message `out` shows, if any.
fn shown(out: &[Command]) -> Option<&str> {
    out.iter().find_map(|c| match c {
        Command::OpenScreen(screen, params) if *screen == CONFIRM_SCREEN => {
            params.get(paperpaw_sdk::builtin::params::TEXT)
        }
        _ => None,
    })
}

/// KOReader on a phone, further in the book.
const FURTHER_THERE: &str = r#"{"document":"x","progress":"/body/DocFragment[20]/body/p[3]/text().0","percentage":0.8,"device":"Phone","device_id":"PHONE","timestamp":1}"#;

fn stay(path: &str) -> confirm::Answered {
    confirm::Answered {
        path: path.to_string(),
        percentage: "0.8".to_string(),
        went: false,
        stay: true,
    }
}

#[test]
fn sync_progress_fetches_before_it_sends() {
    let h = Harness::new(false);
    assert!(requests(&h.emit(&events::BOOK_OPENED, reading(20.0))).is_empty());
    let out = h.sync_now();
    let request = fetch(&out);
    // Sending first would overwrite the further position there.
    assert_eq!(sent(&out), None);
    let out = h.answer(request, 200, FURTHER_THERE);
    assert!(offers(&out));
    assert_eq!(sent(&out), None);
    // Staying here: this position is the one to keep.
    let out = h.emit(&confirm::ANSWERED, stay(BOOK));
    assert_eq!(sent(&out), Some(0.2));
}

#[test]
fn sync_progress_sends_when_the_server_is_not_further() {
    let h = Harness::new(false);
    h.emit(&events::BOOK_OPENED, reading(50.0));
    let out = h.sync_now();
    let behind = r#"{"percentage":0.3,"device":"Phone","device_id":"PHONE"}"#;
    let out = h.answer(fetch(&out), 200, behind);
    assert!(!offers(&out));
    assert_eq!(shown(&out), Some(h.i18n.tr("kosync.up-to-date")));
    assert_eq!(sent(&out), Some(0.5));
}

#[test]
fn nothing_is_sent_before_the_fetch_on_opening_is_answered() {
    let mut h = Harness::new(true);
    let request = fetch(&h.emit(&events::BOOK_OPENED, reading(20.0))).clone();
    h.now += 1_000;
    assert!(requests(&h.emit(&events::PAGE_TURNED, reading(21.0))).is_empty());
    assert!(requests(&h.emit(&events::SUSPEND, ())).is_empty());
    let out = h.answer(&request, 200, FURTHER_THERE);
    assert!(offers(&out));
    assert_eq!(sent(&out), None);
    // Nor while the offer is open.
    assert!(requests(&h.emit(&events::BOOK_CLOSED, reading(21.0))).is_empty());
    // Gone there: the reader's next position is sent.
    let gone = confirm::Answered {
        went: true,
        stay: false,
        ..stay(BOOK)
    };
    assert_eq!(sent(&h.emit(&confirm::ANSWERED, gone)), None);
    assert_eq!(
        sent(&h.emit(&events::PAGE_TURNED, reading(80.0))),
        Some(0.8)
    );
}

#[test]
fn what_was_held_back_is_sent_when_the_server_is_not_further() {
    let mut h = Harness::new(true);
    let request = fetch(&h.emit(&events::BOOK_OPENED, reading(50.0))).clone();
    h.now += 1_000;
    assert!(requests(&h.emit(&events::PAGE_TURNED, reading(51.0))).is_empty());
    let behind = r#"{"percentage":0.3,"device":"Phone","device_id":"PHONE"}"#;
    let out = h.answer(&request, 200, behind);
    assert!(!offers(&out));
    // Not asked for: no message.
    assert_eq!(shown(&out), None);
    assert_eq!(sent(&out), Some(0.51));
}

#[test]
fn this_devices_own_position_is_not_offered() {
    let h = Harness::new(false);
    h.emit(&events::BOOK_OPENED, reading(20.0));
    let out = h.sync_now();
    let own = format!(r#"{{"percentage":0.9,"device":"PaperPaw","device_id":"{DEVICE_ID}"}}"#);
    let out = h.answer(fetch(&out), 200, &own);
    assert!(!offers(&out));
    assert_eq!(shown(&out), Some(h.i18n.tr("kosync.up-to-date")));
    assert_eq!(sent(&out), Some(0.2));
}

#[test]
fn an_unreadable_answer_sends_nothing() {
    let h = Harness::new(false);
    h.emit(&events::BOOK_OPENED, reading(20.0));
    let out = h.sync_now();
    let out = h.answer(fetch(&out), 200, "<html>Down for maintenance</html>");
    assert!(!offers(&out));
    assert_eq!(shown(&out), Some(h.i18n.tr("kosync.unreadable")));
    assert_eq!(sent(&out), None);
}

#[test]
fn a_failed_fetch_sends_nothing_until_a_fetch_succeeds() {
    let mut h = Harness::new(true);
    h.emit(&events::BOOK_OPENED, reading(20.0));
    let out = h.sync_now();
    let out = h.answer(fetch(&out), 500, "Internal Server Error");
    let error = h
        .i18n
        .format("kosync.server-error", &[("status", Arg::Str("500"))]);
    assert_eq!(shown(&out), Some(error.as_str()));
    assert_eq!(sent(&out), None);
    // Reading on sends nothing over the progress not read.
    h.now += 1_000;
    assert!(requests(&h.emit(&events::PAGE_TURNED, reading(22.0))).is_empty());
    // A while later it is fetched again first, then sent.
    h.now += PULL_EVERY_MS;
    let out = h.emit(&events::PAGE_TURNED, reading(23.0));
    assert_eq!(sent(&out), None);
    let behind = r#"{"percentage":0.1,"device":"Phone","device_id":"PHONE"}"#;
    assert_eq!(sent(&h.answer(fetch(&out), 200, behind)), Some(0.23));
}

#[test]
fn a_position_stayed_away_from_is_not_offered_again() {
    let mut h = Harness::new(true);
    let request = fetch(&h.emit(&events::BOOK_OPENED, reading(20.0))).clone();
    assert!(offers(&h.answer(&request, 200, FURTHER_THERE)));
    h.emit(&confirm::ANSWERED, stay(BOOK));
    h.now += PULL_EVERY_MS;
    let out = h.emit(&events::RESUME, ());
    assert!(!offers(&h.answer(fetch(&out), 200, FURTHER_THERE)));
    // Asked for, it is.
    let out = h.sync_now();
    assert!(offers(&h.answer(fetch(&out), 200, FURTHER_THERE)));
}

/// Back on the offer means not now for this book: the same position is
/// not offered again (not after every wake either), and nothing is sent
/// over it while it is undecided. Moved on the server, it is offered.
#[test]
fn an_offer_left_with_back_comes_again_only_when_the_server_moves() {
    let mut h = Harness::new(true);
    let request = fetch(&h.emit(&events::BOOK_OPENED, reading(20.0))).clone();
    assert!(offers(&h.answer(&request, 200, FURTHER_THERE)));
    let back = confirm::Answered {
        stay: false,
        ..stay(BOOK)
    };
    assert_eq!(sent(&h.emit(&confirm::ANSWERED, back)), None);
    // Woken soon after: not even fetched again.
    h.now += 1_000;
    assert!(requests(&h.emit(&events::RESUME, ())).is_empty());
    // Woken later: fetched, but the same position is not offered.
    h.now += PULL_EVERY_MS;
    let out = h.emit(&events::RESUME, ());
    let out = h.answer(fetch(&out), 200, FURTHER_THERE);
    assert!(!offers(&out));
    assert_eq!(sent(&out), None);
    // Nor when the book is opened again.
    h.now += PULL_EVERY_MS;
    let out = h.emit(&events::BOOK_OPENED, reading(21.0));
    assert!(!offers(&h.answer(fetch(&out), 200, FURTHER_THERE)));
    // Nothing goes over it meanwhile.
    h.now += PUSH_EVERY_MS;
    assert_eq!(sent(&h.emit(&events::PAGE_TURNED, reading(22.0))), None);
    assert_eq!(sent(&h.emit(&events::SUSPEND, ())), None);
    // Further still on the server: offered again.
    h.now += PULL_EVERY_MS;
    let out = h.emit(&events::RESUME, ());
    let further = FURTHER_THERE.replace("0.8", "0.9");
    let out = h.answer(fetch(&out), 200, &further);
    assert!(offers(&out));
    assert_eq!(sent(&out), None);
}

#[test]
fn another_server_signs_out() {
    let mut h = Harness::new(false);
    h.emit(&events::BOOK_OPENED, reading(20.0));
    h.set(&SERVER, "sync.example.org");
    let changed = SettingChanged::new(SERVER.id().clone());
    let out = h.emit(&events::SETTING_CHANGED, changed);
    assert!(out.contains(&save_key(None)), "{out:?}");
    h.apply(out);
    assert_eq!(h.secrets.get(SECRET), None);
    assert!(h.settings.read(&USER).is_empty());
    // The key went nowhere: signing in comes first.
    let out = h.sync_now();
    assert!(requests(&out).is_empty());
    assert!(matches!(&out[..], [Command::OpenScreen(screen, _)] if *screen == ACCOUNT_SCREEN));
}

#[test]
fn the_same_server_keeps_the_account() {
    let mut h = Harness::new(false);
    h.set(&SERVER, "sync.koreader.rocks/");
    let changed = SettingChanged::new(SERVER.id().clone());
    assert!(h.emit(&events::SETTING_CHANGED, changed).is_empty());
}
