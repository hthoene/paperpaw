//! What the tests of the UI building blocks share: an `Env` made without
//! a kernel (the UI settings as the kernel defines them, the UI and icon
//! fonts of fonts-default), a framebuffer to draw into and a way to find
//! what was drawn outside a rectangle, and pointer samples.

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

extern crate std;
#[allow(unused_imports)]
use std::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use std::cell::RefCell;

use crate::Place;
use crate::kit::action::Actions;
use crate::kit::menus::Menus;
use crate::kit::status::StatusItems;
use paperpaw_graphics::{Described, Framebuffer};
use paperpaw_i18n::Catalog;
use paperpaw_sdk::builtin::{fonts, settings as core_settings};
use paperpaw_sdk::{
    Capabilities, Color, DeviceInfo, DisplayInfo, Font, FontSpec, Fonts, Formats, Hyphenation, Id,
    InputCaps, Instant, MemoryClass, MemoryStorage, Metrics, Output, PixelFormat, Plugins, Point,
    PointerEvent, PointerPhase, PointerTool, Priority, Rect, SettingKind, SettingSpec,
    SettingValue, Settings, SystemStatus, Translator,
};

/// Source Sans 3, the UI font PaperPaw ships.
const UI_FONT: &[u8] = include_bytes!("../../../../../plugins/fonts-default/fonts/SourceSans3.ttf");
/// The icons PaperPaw ships.
const ICON_FONT: &[u8] =
    include_bytes!("../../../../../plugins/fonts-default/fonts/PaperPawIcons.ttf");

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

/// A kernel with nothing registered but the kernel's words and the look
/// of the UI (and the fonts where the device has them): tests change its
/// state (through `Deref` to the kernel's shared state) before asking for
/// a [`Place`].
pub(crate) struct Harness {
    kernel: crate::Kernel,
    /// The kernel's storage, as the test sees it.
    pub storage: super::common::Card,
}

impl core::ops::Deref for Harness {
    type Target = crate::Shared;
    fn deref(&self) -> &crate::Shared {
        &self.kernel.shared
    }
}

impl core::ops::DerefMut for Harness {
    fn deref_mut(&mut self) -> &mut crate::Shared {
        &mut self.kernel.shared
    }
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
        let mut kernel = crate::Kernel::builder(info)
            .glyph_cache(512 * 1024)
            .build_bare()
            .unwrap();
        let storage = super::common::Card::default();
        kernel.shared.storage = alloc::rc::Rc::new(storage.clone());
        kernel.shared.now = Instant(1000);
        let mut h = Harness { kernel, storage };
        // The kernel's words that the kit shows, as its English catalog has
        // them (crates/paperpaw-core/i18n/en.ini).
        h.translator
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
        // The kernel makes the interface's ink of this setting.
        if id == core_settings::UI_TEXT_CURVE.as_str() {
            self.fonts.set_ui_ink(crate::kit::ui::text_ink(value, "en"));
        }
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

    /// The kernel's state as a handler sees it now.
    pub fn env(&self) -> Place<'_> {
        self.kernel.shared.place(self.kernel.shared.now)
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
