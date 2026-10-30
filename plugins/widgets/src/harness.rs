//! What the widget pack's tests draw with: an `Env` made of the widget
//! pack and the default fonts registered, on a storage in memory.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use alloc::vec::Vec;

use paperpaw_graphics::Framebuffer;
use paperpaw_i18n::{Catalog, Translator};
use paperpaw_sdk::builtin::settings as core_settings;
use paperpaw_sdk::widgets::{Widget, Widgets};
use paperpaw_sdk::{
    Actions, Battery, Capabilities, Color, Contribution, DeviceInfo, DisplayInfo, Env, EnvParts,
    Fonts, Formats, Hyphenation, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Menus,
    Metrics, PixelFormat, Plugin, Plugins, Rect, Registrar, SettingKind, SettingSpec, SettingValue,
    Settings, StatusItems, SystemStatus, WallClock,
};

pub(crate) const DEVICE: DeviceInfo = DeviceInfo::new(
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

pub(crate) struct NoKeys;

impl paperpaw_sdk::keys::KeyBindings for NoKeys {
    fn bound(&self, _: &[&str], _: &str) -> Option<Id> {
        None
    }
}

/// What an `Env` is made of: the widget pack and the default fonts
/// registered, a library on a storage in memory.
pub(crate) struct Harness {
    pub(crate) settings: Settings,
    pub(crate) i18n: Translator,
    pub(crate) fonts: Fonts,
    pub(crate) formats: Formats,
    pub(crate) hyphenation: Hyphenation,
    pub(crate) menus: Menus,
    pub(crate) actions: Actions,
    pub(crate) plugins: Plugins,
    pub(crate) status: SystemStatus,
    pub(crate) usage: paperpaw_sdk::usage::Usage,
    pub(crate) plugin_usage: core::cell::RefCell<Vec<paperpaw_sdk::usage::PluginUsage>>,
    pub(crate) status_items: StatusItems,
    pub(crate) text_filters: paperpaw_sdk::filters::TextFilters,
    pub(crate) decorators: paperpaw_sdk::decorations::Decorators,
    pub(crate) page_layers: paperpaw_sdk::layers::PageLayers,
    pub(crate) lookup: paperpaw_sdk::lookup::LookupProviders,
    pub(crate) readers: paperpaw_sdk::readers::Readers,
    pub(crate) widgets: Widgets,
    pub(crate) setup: paperpaw_sdk::setup::SetupSteps,
    pub(crate) services: paperpaw_sdk::services::Services,
    pub(crate) secrets: paperpaw_sdk::secrets::Secrets,
    pub(crate) storage: MemoryStorage,
}

impl Harness {
    pub(crate) fn new(storage: MemoryStorage) -> Self {
        let mut h = Harness {
            settings: Settings::new(),
            i18n: Translator::new(),
            fonts: Fonts::new(64 * 1024),
            formats: Formats::new(),
            hyphenation: Hyphenation::new(),
            menus: Menus::new(),
            actions: Actions::new(),
            plugins: Plugins::new(),
            status: SystemStatus::new(
                Some(WallClock::new(
                    Instant(0),
                    // Wednesday, 14 October 2026, 10:41 UTC.
                    1_791_974_460_000,
                    Some(0),
                )),
                Some(Battery::new(64, true, None)),
            ),
            usage: Default::default(),
            plugin_usage: Default::default(),
            status_items: StatusItems::new(),
            text_filters: Default::default(),
            decorators: Default::default(),
            page_layers: Default::default(),
            lookup: Default::default(),
            readers: Default::default(),
            widgets: Widgets::new(),
            setup: Default::default(),
            services: Default::default(),
            secrets: Default::default(),
            storage,
        };
        // The look of the UI, as the kernel defines it.
        for (id, value) in [
            (core_settings::UI_SIZE.as_str(), 12),
            (core_settings::UI_ROW.as_str(), 160),
            (core_settings::UI_MARGIN.as_str(), 4),
            (core_settings::UI_LINES.as_str(), 2),
            (core_settings::BATTERY_LOW.as_str(), 15),
        ] {
            h.settings
                .define(SettingSpec::new(
                    Id::parse(id).unwrap(),
                    "x",
                    SettingKind::Int { min: 0, max: 400 },
                    SettingValue::Int(value),
                    paperpaw_sdk::builtin::groups::OTHER,
                ))
                .unwrap();
        }
        let mut r = Registrar::new(Id::new_static("fonts-default"));
        paperpaw_plugin_fonts_default::DefaultFonts
            .register(&mut r)
            .unwrap();
        h.apply(r);
        let mut r = Registrar::new(crate::PLUGIN);
        crate::Widgets.register(&mut r).unwrap();
        h.apply(r);
        h.i18n.set_language("en");
        h
    }

    pub(crate) fn apply(&mut self, r: Registrar) {
        for contribution in r.into_contributions() {
            match contribution {
                Contribution::Setting(spec) => self.settings.define(*spec).unwrap(),
                Contribution::FixedSettings(list) => {
                    for at in 0..list.len() {
                        self.settings.define_fixed(list, at).unwrap();
                    }
                }
                Contribution::SettingGroup(group) => self.settings.add_group(group).unwrap(),
                Contribution::Widget(widget) => self.widgets.add(*widget),
                Contribution::Translations(text) => {
                    self.i18n.add(Catalog::parse(text).unwrap());
                }
                Contribution::Font(font) => self.fonts.add(*font).unwrap(),
                Contribution::FontBinding {
                    role,
                    font,
                    priority,
                    ..
                } => self.fonts.propose(role, font, priority),
                _ => {}
            }
        }
    }

    pub(crate) fn set(&mut self, id: &str, value: SettingValue) {
        self.settings.set(id, value).unwrap();
    }

    pub(crate) fn env(&self) -> Env<'_> {
        Env::from(EnvParts {
            device: &DEVICE,
            now: Instant(1000),
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
}

/// The pixels of `widget` drawn into `area` of a page with `env`.
pub(crate) fn pixels(env: &Env<'_>, widget: &dyn Widget, area: Rect) -> Vec<u8> {
    let mut fb = Framebuffer::new(
        DEVICE.display.width,
        DEVICE.display.height,
        PixelFormat::Gray4,
    )
    .unwrap();
    {
        let mut canvas = fb.canvas();
        canvas.clear(Color::WHITE);
        widget.draw(&mut canvas, env, area);
    }
    let view = fb.view();
    let mut pixels = Vec::new();
    for y in area.y..area.y + area.height {
        pixels.extend((area.x..area.x + area.width).map(|x| view.pixel(x, y).luma()));
    }
    pixels
}
