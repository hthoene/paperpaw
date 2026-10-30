//! What the integration tests of the SDK share: an [`Env`] made of empty
//! registries, a storage in memory and a clock, as the kernel would make
//! one.

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use core::cell::RefCell;

use paperpaw_sdk::decorations::Decorators;
use paperpaw_sdk::filters::TextFilters;
use paperpaw_sdk::secrets::Secrets;
use paperpaw_sdk::services::Services;
use paperpaw_sdk::setup::SetupSteps;
use paperpaw_sdk::usage::{PluginUsage, Usage};
use paperpaw_sdk::widgets::Widgets;
use paperpaw_sdk::{
    Actions, Capabilities, DeviceInfo, DisplayInfo, Env, EnvParts, Fonts, Formats, Hyphenation, Id,
    InputCaps, Instant, MemoryClass, MemoryStorage, Menus, Metrics, PixelFormat, Plugins, Settings,
    StatusItems, SystemStatus, Translator, WallClock,
};

pub const DEVICE: DeviceInfo = DeviceInfo::new(
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

/// The parts of an [`Env`]; tests change them before asking for one.
pub struct Harness {
    pub settings: Settings,
    pub i18n: Translator,
    pub fonts: Fonts,
    pub formats: Formats,
    pub hyphenation: Hyphenation,
    pub menus: Menus,
    pub actions: Actions,
    pub plugins: Plugins,
    pub status: SystemStatus,
    pub usage: Usage,
    pub plugin_usage: RefCell<Vec<PluginUsage>>,
    pub status_items: StatusItems,
    pub text_filters: TextFilters,
    pub decorators: Decorators,
    pub page_layers: paperpaw_sdk::layers::PageLayers,
    pub lookup: paperpaw_sdk::lookup::LookupProviders,
    pub readers: paperpaw_sdk::readers::Readers,
    pub widgets: Widgets,
    pub setup: SetupSteps,
    pub services: Services,
    pub secrets: Secrets,
    pub storage: MemoryStorage,
    pub now: Instant,
}

impl Harness {
    /// Nothing registered, no clock.
    pub fn new() -> Self {
        Harness {
            settings: Settings::new(),
            i18n: Translator::new(),
            fonts: Fonts::new(16 * 1024),
            formats: Formats::new(),
            hyphenation: Hyphenation::new(),
            menus: Menus::new(),
            actions: Actions::new(),
            plugins: Plugins::new(),
            status: SystemStatus::default(),
            usage: Usage::default(),
            plugin_usage: RefCell::default(),
            status_items: StatusItems::new(),
            text_filters: TextFilters::default(),
            decorators: Decorators::default(),
            page_layers: Default::default(),
            lookup: Default::default(),
            readers: Default::default(),
            widgets: Widgets::new(),
            setup: SetupSteps::default(),
            services: Services::default(),
            secrets: Secrets::default(),
            storage: MemoryStorage::new(),
            now: Instant(0),
        }
    }

    /// The device knows the time: `unix_millis` at the harness's `now`,
    /// the system's zone `utc_offset_minutes` east of UTC.
    pub fn with_clock(mut self, unix_millis: i64, utc_offset_minutes: Option<i32>) -> Self {
        self.status.clock = Some(WallClock::new(self.now, unix_millis, utc_offset_minutes));
        self
    }

    pub fn env(&self) -> Env<'_> {
        self.env_on(&self.storage)
    }

    /// [`Harness::env`] over another storage (one that fails, say).
    pub fn env_on<'a>(&'a self, storage: &'a dyn paperpaw_sdk::Storage) -> Env<'a> {
        Env::from(EnvParts {
            device: &DEVICE,
            now: self.now,
            random: paperpaw_sdk::random::Random::NONE,
            memory: paperpaw_sdk::memory::Memory::NONE,
            clock: paperpaw_sdk::time::Clock::NONE,
            started: paperpaw_sdk::power::Start::Unknown,
            slept_for: None,
            panel: paperpaw_sdk::realtime::PanelInfo::default(),
            keys_held: &[],
            room: paperpaw_sdk::realtime::Room::default(),
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
            caps: Capabilities::default(),
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
            storage,
        })
    }
}
