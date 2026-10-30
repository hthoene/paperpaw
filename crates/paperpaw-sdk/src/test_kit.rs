//! What unit tests make an [`Env`] of ([`Kit`]): a small device and empty
//! registries, nothing loaded.

use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_platform::Rotation;

use crate::decorations::Decorators;
use crate::filters::TextFilters;
use crate::id::Id;
use crate::secrets::Secrets;
use crate::services::Services;
use crate::setup::SetupSteps;
use crate::status::StatusItems;
use crate::usage::{PluginUsage, Usage};
use crate::widgets::Widgets;
use crate::{
    Actions, DeviceInfo, DisplayInfo, Env, EnvParts, Fonts, Formats, Hyphenation, InputCaps,
    Instant, MemoryClass, MemoryStorage, Menus, Metrics, PixelFormat, Plugins, Settings,
    SystemStatus, Translator,
};

struct NoKeys;

impl crate::keys::KeyBindings for NoKeys {
    fn bound(&self, _: &[&str], _: &str) -> Option<Id> {
        None
    }
}

/// What an [`Env`] is made of.
pub(crate) struct Kit {
    pub(crate) device: DeviceInfo,
    pub(crate) settings: Settings,
    pub(crate) i18n: Translator,
    pub(crate) fonts: Fonts,
    pub(crate) formats: Formats,
    pub(crate) hyphenation: Hyphenation,
    pub(crate) menus: Menus,
    pub(crate) actions: Actions,
    pub(crate) plugins: Plugins,
    pub(crate) status: SystemStatus,
    pub(crate) usage: Usage,
    pub(crate) plugin_usage: RefCell<Vec<PluginUsage>>,
    pub(crate) status_items: StatusItems,
    pub(crate) text_filters: TextFilters,
    pub(crate) decorators: Decorators,
    pub(crate) page_layers: crate::layers::PageLayers,
    pub(crate) lookup: crate::lookup::LookupProviders,
    pub(crate) readers: crate::readers::Readers,
    pub(crate) widgets: Widgets,
    pub(crate) setup: SetupSteps,
    pub(crate) services: Services,
    pub(crate) secrets: Secrets,
    pub(crate) storage: MemoryStorage,
}

impl Kit {
    /// A device with buttons alone (as the X4), or with touch too.
    pub(crate) fn new(touch: bool) -> Self {
        Self {
            device: DeviceInfo::new(
                "test",
                "Test",
                DisplayInfo::new(400, 400, 254, PixelFormat::Gray4, Rotation::Deg0, None),
                InputCaps::new(true, touch, false, &[]),
                MemoryClass::Small,
            ),
            settings: Settings::new(),
            i18n: Translator::new(),
            fonts: Fonts::new(0),
            formats: Formats::new(),
            hyphenation: Hyphenation::new(),
            menus: Menus::new(),
            actions: Actions::new(),
            plugins: Plugins::new(),
            status: SystemStatus::default(),
            usage: Usage::default(),
            plugin_usage: RefCell::new(Vec::new()),
            status_items: StatusItems::new(),
            text_filters: TextFilters::new(),
            decorators: Decorators::new(),
            page_layers: Default::default(),
            lookup: Default::default(),
            readers: Default::default(),
            widgets: Widgets::new(),
            setup: SetupSteps::new(),
            services: Services::new(),
            secrets: Secrets::default(),
            storage: MemoryStorage::new(),
        }
    }

    pub(crate) fn env(&self) -> Env<'_> {
        Env::from(EnvParts {
            device: &self.device,
            now: Instant::default(),
            metrics: Metrics::new(self.device.display.dpi),
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
            look: crate::ui::look::Look::none(),
            caps: crate::Capabilities::default(),
            wifi: None,
            bluetooth: None,
            firmware: None,
            native: None,
            power: crate::power::Power::normal(),
            work: crate::work::WorkStates::none(),
            reader: true,
            clipboard: false,
            last_input: crate::keys::InputMode::at_start(&self.device.input),
            inputs: &[],
            content: paperpaw_platform::Rect::default(),
            stack: &[],
            slots: &[],
            screens: crate::ScreenList::default(),
            font_dirs: &[],
            catalog_device: None,
            secrets: &self.secrets,
            running: &crate::secrets::Anyone,
            random: crate::random::Random::default(),
            memory: crate::memory::Memory::NONE,
            clock: crate::time::Clock::NONE,
            started: crate::power::Start::Unknown,
            slept_for: None,
            panel: crate::realtime::PanelInfo::default(),
            keys_held: &[],
            room: crate::realtime::Room::default(),
            storage: &self.storage,
        })
    }
}
