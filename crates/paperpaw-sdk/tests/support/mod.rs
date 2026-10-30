//! What the tests of the UI building blocks share: an `Env` made without
//! a kernel (the UI settings as the kernel defines them, the UI and icon
//! fonts of fonts-default), a framebuffer to draw into and a way to find
//! what was drawn outside a rectangle, and pointer samples.

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::cell::RefCell;

use paperpaw_graphics::{Described, Framebuffer};
use paperpaw_i18n::Catalog;
use paperpaw_sdk::builtin::{fonts, settings as core_settings};
use paperpaw_sdk::{
    Actions, Capabilities, Color, DeviceInfo, DisplayInfo, Env, EnvParts, Font, FontSpec, Fonts,
    Formats, Hyphenation, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Menus, Metrics,
    Output, PixelFormat, Plugins, Point, PointerEvent, PointerPhase, PointerTool, Priority, Rect,
    SettingKind, SettingSpec, SettingValue, Settings, StatusItems, SystemStatus, Translator,
};

/// Source Sans 3, the UI font PaperPaw ships.
const UI_FONT: &[u8] = include_bytes!("../../../../plugins/fonts-default/fonts/SourceSans3.ttf");
/// The icons PaperPaw ships.
const ICON_FONT: &[u8] =
    include_bytes!("../../../../plugins/fonts-default/fonts/PaperPawIcons.ttf");

/// The kernel's words that the SDK's own parts show.
const KERNEL_WORDS: &str = "[meta]
language = en
name = English
plural = one-other

[messages]
core.on = On
core.off = Off
core.page-of = {page} / {pages}
core.duration.under-a-minute = < 1 min
core.duration.minutes = {m} min
core.duration.hours = {h} h {m} min
";

/// A device: its size, and whether it has a touch screen.
#[derive(Debug, Clone, Copy)]
pub struct Device {
    pub width: u16,
    pub height: u16,
    pub touch: bool,
    /// The UI and icon fonts registered.
    pub fonts: bool,
}

impl Device {
    /// A Boox-like touch screen, fonts there.
    pub const TOUCH: Device = Device {
        width: 480,
        height: 800,
        touch: true,
        fonts: true,
    };
    /// An X4-like device with buttons alone, fonts there.
    pub const BUTTONS: Device = Device {
        width: 480,
        height: 800,
        touch: false,
        fonts: true,
    };
}

struct NoKeys;

impl paperpaw_sdk::keys::KeyBindings for NoKeys {
    fn bound(&self, _: &[&str], _: &str) -> Option<Id> {
        None
    }
}

/// What an `Env` is made of, owned.
pub struct Harness {
    pub device: DeviceInfo,
    pub settings: Settings,
    pub i18n: Translator,
    pub fonts: Fonts,
    pub formats: Formats,
    pub hyphenation: Hyphenation,
    pub menus: Menus,
    pub actions: Actions,
    pub plugins: Plugins,
    pub status: SystemStatus,
    pub usage: paperpaw_sdk::usage::Usage,
    pub plugin_usage: RefCell<Vec<paperpaw_sdk::usage::PluginUsage>>,
    pub status_items: StatusItems,
    pub text_filters: paperpaw_sdk::filters::TextFilters,
    pub decorators: paperpaw_sdk::decorations::Decorators,
    pub page_layers: paperpaw_sdk::layers::PageLayers,
    pub lookup: paperpaw_sdk::lookup::LookupProviders,
    pub readers: paperpaw_sdk::readers::Readers,
    pub widgets: paperpaw_sdk::widgets::Widgets,
    pub setup: paperpaw_sdk::setup::SetupSteps,
    pub services: paperpaw_sdk::services::Services,
    pub secrets: paperpaw_sdk::secrets::Secrets,
    pub storage: MemoryStorage,
}

impl Harness {
    pub fn new(device: Device) -> Self {
        let info = DeviceInfo::new(
            "test",
            "Test device",
            DisplayInfo::new(
                device.width,
                device.height,
                220,
                PixelFormat::Gray8,
                paperpaw_platform::Rotation::Deg0,
                None,
            ),
            InputCaps::new(true, device.touch, false, &[]),
            MemoryClass::Large,
        );
        let mut h = Harness {
            device: info,
            settings: Settings::new(),
            i18n: Translator::new(),
            fonts: Fonts::new(512 * 1024),
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
            widgets: paperpaw_sdk::widgets::Widgets::new(),
            setup: Default::default(),
            services: Default::default(),
            secrets: Default::default(),
            storage: MemoryStorage::new(),
        };
        // The kernel's words that the SDK's own parts show, as its English
        // catalog has them (crates/paperpaw-core/i18n/en.ini).
        h.i18n
            .add(Catalog::parse(KERNEL_WORDS).expect("the kernel's words"));
        // The look of the UI, as the kernel defines it by default.
        for (id, value) in [
            (core_settings::UI_SIZE.as_str(), 12),
            (core_settings::UI_ROW.as_str(), 160),
            (core_settings::UI_MARGIN.as_str(), 4),
            (core_settings::UI_LINES.as_str(), 2),
        ] {
            h.define_int(id, value, 0, 400);
        }
        if device.fonts {
            let ui = Font::from_data(UI_FONT, &[("wght", 430.0)]).expect("the UI font");
            let ui_id = Id::new_static("test.ui");
            h.fonts.add(FontSpec::new(ui_id.clone(), ui)).unwrap();
            h.fonts.propose(fonts::UI, ui_id, Priority::DEFAULT);
            let icons = Font::from_data(ICON_FONT, &[]).expect("the icon font");
            let icons_id = Id::new_static("test.icons");
            h.fonts.add(FontSpec::new(icons_id.clone(), icons)).unwrap();
            h.fonts.propose(fonts::ICONS, icons_id, Priority::DEFAULT);
        }
        h
    }

    /// Defines a number setting `id` set to `value`.
    pub fn define_int(&mut self, id: &str, value: i64, min: i64, max: i64) {
        self.settings
            .define(SettingSpec::new(
                Id::parse(id).unwrap(),
                "x",
                SettingKind::Int { min, max },
                SettingValue::Int(value),
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
    }

    /// Defines a text setting `id` set to `value`.
    pub fn define_text(&mut self, id: &str, value: &str) {
        self.settings
            .define(SettingSpec::new(
                Id::parse(id).unwrap(),
                "x",
                SettingKind::Text,
                SettingValue::Text(value.to_string().into()),
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
    }

    /// Defines a switch `id` set to `value`.
    pub fn define_bool(&mut self, id: &str, value: bool) {
        self.settings
            .define(SettingSpec::new(
                Id::parse(id).unwrap(),
                "x",
                SettingKind::Bool,
                SettingValue::Bool(value),
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
    }

    pub fn env(&self) -> Env<'_> {
        Env::from(EnvParts {
            device: &self.device,
            now: Instant(1000),
            random: paperpaw_sdk::random::Random::NONE,
            memory: paperpaw_sdk::memory::Memory::NONE,
            clock: paperpaw_sdk::time::Clock::NONE,
            started: paperpaw_sdk::power::Start::Unknown,
            panel: paperpaw_sdk::realtime::PanelInfo::default(),
            keys_held: &[],
            room: paperpaw_sdk::realtime::Room::default(),
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
            last_input: paperpaw_sdk::keys::InputMode::at_start(&self.device.input),
            inputs: &[],
            content: paperpaw_platform::Rect::default(),
            stack: &[],
            slots: &[],
            screens: paperpaw_sdk::ScreenList::default(),
            font_dirs: &[],
            catalog_device: None,
            secrets: &self.secrets,
            running: &paperpaw_sdk::secrets::Anyone,
            storage: &self.storage,
        })
    }
}

/// An odd gray no drawing uses: where a pixel is not this, something was
/// drawn.
pub const PAPER: Color = Color::gray(77);

/// A framebuffer `width` × `height` cleared to [`PAPER`].
pub fn frame(width: u16, height: u16) -> Framebuffer {
    let mut frame = Framebuffer::new(width, height, PixelFormat::Gray8).unwrap();
    frame.canvas().clear(PAPER);
    frame.take_dirty();
    frame
}

/// The first pixel drawn on (changed from [`PAPER`]) outside `bound`,
/// looked for where the drawing touched the frame since it was made.
pub fn drawn_outside(frame: &mut Framebuffer, bound: Rect) -> Option<Point> {
    let dirty = frame.take_dirty()?;
    let view = frame.view();
    for y in dirty.y..(dirty.bottom() as u16) {
        for x in dirty.x..(dirty.right() as u16) {
            let point = Point::new(i32::from(x), i32::from(y));
            if !bound.contains(point) && view.pixel(x, y) != PAPER {
                return Some(point);
            }
        }
    }
    None
}

/// Whether anything was drawn inside `area`.
pub fn drawn_in(frame: &Framebuffer, area: Rect) -> bool {
    let view = frame.view();
    (area.y..area.bottom() as u16)
        .any(|y| (area.x..area.right() as u16).any(|x| view.pixel(x, y) != PAPER))
}

/// Draws with `draw` on a frame of the device's size and returns what the
/// drawing described and the frame.
pub fn describe(
    width: u16,
    height: u16,
    draw: impl FnOnce(&mut paperpaw_sdk::Canvas<'_>),
) -> (Vec<Described>, Framebuffer) {
    let mut frame = frame(width, height);
    let mut described = Vec::new();
    {
        let mut canvas = frame.canvas().describing(&mut described);
        draw(&mut canvas);
    }
    (described, frame)
}

/// A pointer sample of finger 0.
pub fn pointer(phase: PointerPhase, x: i32, y: i32) -> PointerEvent {
    PointerEvent::new(0, phase, Point::new(x, y), PointerTool::Finger, 1000)
}

/// A tap at `point`.
pub fn tap(x: i32, y: i32) -> paperpaw_sdk::Gesture {
    let point = Point::new(x, y);
    paperpaw_sdk::Gesture::new(
        paperpaw_sdk::GestureKind::Tap,
        point,
        point,
        PointerTool::Finger,
    )
}

/// The middle of `rect`.
pub fn middle(rect: Rect) -> Point {
    Point::new(
        i32::from(rect.x) + i32::from(rect.width) / 2,
        i32::from(rect.y) + i32::from(rect.height) / 2,
    )
}

/// A fresh output for a `Cx`.
pub fn output() -> Output {
    Output::default()
}
