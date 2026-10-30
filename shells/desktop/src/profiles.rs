//! Built-in device profiles.
//!
//! A profile describes everything the emulator needs to behave like a
//! device: screen, pixel format, input hardware, buttons and the heap the
//! kernel may use. The Xteink profiles build the kernel as the firmware
//! does (`shells/xteink/src/main.rs`), the Boox Go 7 as the Android shell
//! does (`shells/android/src/app.rs`); a change there belongs here too.
//! The emulator keeps the kernel's own clipboard on every profile.

use crate::light::Light;
use paperpaw_platform::{
    ColorPanel, DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation, buttons,
};

/// The X4's front buttons, left to right, as `paperpaw-x4` names them.
const X4_ROW: [&str; 4] = [
    buttons::BACK,
    buttons::CONFIRM,
    buttons::LEFT,
    buttons::RIGHT,
];

/// Every button of the X4 and the X4 Classic.
const X4_BUTTONS: [&str; 7] = [
    buttons::PAGE_BACK,
    buttons::PAGE_FORWARD,
    buttons::LEFT,
    buttons::RIGHT,
    buttons::CONFIRM,
    buttons::BACK,
    buttons::POWER,
];

/// The device's body as emulators draw it around the screen, with its
/// physical keys where they sit (the web emulator's frame, the plugin
/// workshop; docs/web-emulator.md describes it as they get it).
#[derive(Debug, Clone, Copy)]
pub struct Body {
    /// The bezel around the screen, top, right, bottom and left, in
    /// hundredths of the screen's width.
    pub bezel: [u8; 4],
    /// The keys on the body.
    pub keys: &'static [Key],
}

/// A physical key on a device's body.
#[derive(Debug, Clone, Copy)]
pub struct Key {
    /// The button it is (one of the profile's `buttons`).
    pub button: &'static str,
    /// Where on the body it sits.
    pub place: Place,
    /// Its middle along that place, in hundredths of the body's length
    /// there: from the left on the top, the bottom and the face below the
    /// screen; from the top on the sides and the face beside the screen.
    pub at: u8,
    /// Its length along that place, in hundredths of the same.
    pub length: u8,
    /// The computer's keys that press it, as browsers name them
    /// (`KeyboardEvent.key`).
    pub keys: &'static [&'static str],
}

/// Where on a device's body a key sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// On an edge of the body, out of its side.
    Top,
    Right,
    Bottom,
    Left,
    /// On the face, in the bezel below the screen.
    Below,
    /// On the face, in the bezel right of the screen.
    Beside,
}

impl Place {
    /// Its name in the data emulators get.
    pub fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Right => "right",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Below => "below",
            Self::Beside => "beside",
        }
    }
}

impl Body {
    /// A body of even bezels with no key drawn on it.
    pub const PLAIN: Self = Self {
        bezel: [6, 6, 6, 6],
        keys: &[],
    };

    /// As JSON, as emulators get it (docs/web-emulator.md): `{"bezel":
    /// [top, right, bottom, left], "keys": [{"button", "place", "at",
    /// "length", "keys"}]}`.
    pub fn json(&self) -> String {
        use paperpaw_core::sdk::json::string;
        let keys: Vec<String> = self
            .keys
            .iter()
            .map(|key| {
                let pressed: Vec<String> = key.keys.iter().map(|k| string(k)).collect();
                format!(
                    "{{\"button\":{},\"place\":{},\"at\":{},\"length\":{},\"keys\":[{}]}}",
                    string(key.button),
                    string(key.place.name()),
                    key.at,
                    key.length,
                    pressed.join(","),
                )
            })
            .collect();
        let [top, right, bottom, left] = self.bezel;
        format!(
            "{{\"bezel\":[{top},{right},{bottom},{left}],\"keys\":[{}]}}",
            keys.join(",")
        )
    }
}

/// The X4 and the X4 Classic: the four front keys in two rockers below
/// the screen (back and select, left and right), a rocker on the right
/// edge (page back above page forward) and the power key above it, near
/// the top (as reviews and the device show them).
const X4_BODY: Body = Body {
    bezel: [7, 7, 30, 7],
    keys: &[
        Key {
            button: buttons::BACK,
            place: Place::Below,
            at: 19,
            length: 17,
            keys: &["Escape", "Backspace"],
        },
        Key {
            button: buttons::CONFIRM,
            place: Place::Below,
            at: 37,
            length: 17,
            keys: &["Enter"],
        },
        Key {
            button: buttons::LEFT,
            place: Place::Below,
            at: 63,
            length: 17,
            keys: &["ArrowLeft"],
        },
        Key {
            button: buttons::RIGHT,
            place: Place::Below,
            at: 81,
            length: 17,
            keys: &["ArrowRight"],
        },
        Key {
            button: buttons::POWER,
            place: Place::Right,
            at: 14,
            length: 8,
            keys: &["p"],
        },
        Key {
            button: buttons::PAGE_BACK,
            place: Place::Right,
            at: 36,
            length: 13,
            keys: &["PageUp", "ArrowUp"],
        },
        Key {
            button: buttons::PAGE_FORWARD,
            place: Place::Right,
            at: 50,
            length: 13,
            keys: &["PageDown", "ArrowDown", " "],
        },
    ],
};

/// The X4 Pro: a page key on each edge (back on the left, forward on the
/// right, as the X3's), the power key on the top edge and home touched
/// below the screen.
const X4_PRO_BODY: Body = Body {
    bezel: [7, 7, 16, 7],
    keys: &[
        Key {
            button: buttons::PAGE_BACK,
            place: Place::Left,
            at: 45,
            length: 14,
            keys: &["PageUp", "ArrowLeft", "ArrowUp"],
        },
        Key {
            button: buttons::PAGE_FORWARD,
            place: Place::Right,
            at: 45,
            length: 14,
            keys: &["PageDown", "ArrowRight", "ArrowDown", " "],
        },
        Key {
            button: buttons::POWER,
            place: Place::Top,
            at: 78,
            length: 14,
            keys: &["p"],
        },
        Key {
            button: buttons::HOME,
            place: Place::Below,
            at: 50,
            length: 22,
            keys: &["h", "Escape"],
        },
    ],
};

/// The Boox Go 7: its page keys on the wide bezel beside the screen;
/// back is Android's (a gesture) and so is its power key, not drawn.
const BOOX_GO_7_BODY: Body = Body {
    bezel: [5, 16, 5, 5],
    keys: &[
        Key {
            button: buttons::PAGE_BACK,
            place: Place::Beside,
            at: 43,
            length: 12,
            keys: &["PageUp", "ArrowLeft", "ArrowUp"],
        },
        Key {
            button: buttons::PAGE_FORWARD,
            place: Place::Beside,
            at: 57,
            length: 12,
            keys: &["PageDown", "ArrowRight", "ArrowDown", " "],
        },
    ],
};

/// The firmware's setting defaults (the X4, X4 Pro and X4 Classic), with
/// the keyboard's size of the board.
macro_rules! x4_settings {
    ($keyboard:literal) => {
        [
            // A 4.3" screen: smaller text, so menus and pages hold enough.
            ("core.ui-size", "9"),
            ("core.font-size", "9"),
            // The keyboard's size: large keys to tap on the X4 Pro, small ones
            // chosen with the buttons on the others (the firmware's
            // `KEYBOARD_SIZE`).
            ("keyboard.size", $keyboard),
            // A full refresh takes 3.5 s on its panel: screens change fast.
            ("core.flash-screens", "false"),
            // On USB the radio stays on in a network joined before.
            ("wifi.while-charging", "true"),
            // A small screen: calm status lines, the time and the battery at the
            // edges; while reading the time at the left, the page at the right
            // (and the mark of reading aloud beside the time while it reads).
            (
                "reader.status.bottom-left",
                "core.status.clock, reader.status.aloud, lock.touch-status",
            ),
            ("reader.status.bottom-center", ""),
            ("reader.status.bottom-right", "reader.status.page"),
            ("home.status.left", "core.status.clock"),
            ("home.status.center", ""),
            (
                "home.status.right",
                "lock.touch-status, core.status.battery",
            ),
        ]
    };
}

const X4_SETTINGS: [(&str, &str); 11] = x4_settings!("keyboard.size.small");
const X4_PRO_SETTINGS: [(&str, &str); 11] = x4_settings!("keyboard.size.large");

/// The firmware's memory for glyphs of font packs on the card.
const X4_GLYPH_CACHE: usize = 12 * KIB;

/// The heap of the ESP32-S3 devices: 8 MB of PSRAM and 256 KiB of the
/// chip's own, with about a fifth more here (pointers of 8 bytes, not 4).
const S3_HEAP: usize = 10 * 1024 * KIB;

/// An emulated device.
#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub device: DeviceInfo,
    /// Physical buttons the device has; other keys are ignored.
    pub buttons: &'static [&'static str],
    /// Heap available to the kernel and plugins, if limited.
    pub heap_budget: Option<usize>,
    /// Memory for runtime-rasterized glyphs.
    pub glyph_cache: usize,
    /// A mark shows at once while slow work goes on (see
    /// `KernelBuilder::busy_mark`), where the panel refreshes on its own.
    pub busy_mark: bool,
    /// Black and white are shown in the colours of the settings (see
    /// `KernelBuilder::colors`).
    pub colors: bool,
    /// Use bitmap font packs read in place, like small devices without
    /// runtime font rendering; otherwise scalable fonts.
    pub font_packs: bool,
    /// Draw frames in bands of this many rows (see
    /// `KernelBuilder::frame_rows`); `None` keeps the whole frame in RAM.
    pub frame_rows: Option<u16>,
    /// Rows passed from the frame start and end at multiples of this (see
    /// `KernelBuilder::row_align`): 8 where the panel controller takes 8
    /// of them in a byte.
    pub row_align: u16,
    /// Device-specific setting defaults, as `(id, value)`.
    pub settings: &'static [(&'static str, &'static str)],
    /// The device speaks text aloud (emulated: the text is logged and
    /// takes as long as it would to speak).
    pub speech: bool,
    /// PaperPaw manages Wi-Fi itself, as on the X4 (emulated: made-up
    /// networks, see `wifi`); elsewhere the system does.
    pub wifi: bool,
    /// The most Bluetooth links the device keeps (emulated: made-up
    /// devices, see `bluetooth`); 0 for no Bluetooth.
    pub bluetooth: u8,
    /// What plugin libraries call the device (see
    /// `KernelBuilder::catalog_device`): `None` where PaperPaw has no
    /// build for it yet, and its plugin store offers nothing.
    pub catalog_device: Option<&'static str>,
    /// PaperPaw shows its sleep screen and the device sleeps (the power
    /// button); Android shows its own.
    pub sleep: bool,
    /// The buttons in a row below the screen, left to right (see
    /// `KernelBuilder::button_row`).
    pub button_row: &'static [&'static str],
    /// The device's front light, if it has one.
    pub light: Option<Light>,
    /// Its body and keys, as emulators draw it.
    pub body: Body,
    /// One-line description for `--list-profiles`.
    pub summary: &'static str,
}

const KIB: usize = 1024;

pub const PROFILES: &[Profile] = &[
    Profile {
        device: DeviceInfo::new(
            "xteink-x4",
            "Xteink X4",
            DisplayInfo::new(
                480,
                800,
                220,
                // As the firmware draws: black and white, what the panel
                // shows until its four grays (the controller's LUTs) come.
                PixelFormat::Mono1,
                Rotation::Deg90,
                None,
            ),
            InputCaps::new(true, false, false, &[]),
            MemoryClass::Small,
        ),
        buttons: &X4_BUTTONS,
        // The firmware's heap: 192 KiB, and 64 KiB in the memory the boot
        // loader used (two regions: no block larger than 192 KiB). The
        // same things take about a fifth more here (pointers of 8 bytes,
        // not 4), so the emulator runs out a little before the X4 would.
        heap_budget: Some(256 * KIB),
        glyph_cache: X4_GLYPH_CACHE,
        busy_mark: true,
        colors: false,
        font_packs: true,
        // 192 of 800 rows: 11.3 KiB instead of 47 KiB, five bands a frame.
        // The controller takes 8 rows (turned) in a byte.
        frame_rows: Some(192),
        row_align: 8,
        settings: &X4_SETTINGS,
        speech: false,
        wifi: true,
        bluetooth: 0,
        catalog_device: Some("x4"),
        sleep: true,
        button_row: &X4_ROW,
        light: None,
        body: X4_BODY,
        summary: "ESP32-C3, 4.3\" 800x480, buttons only, 256 KiB heap budget",
    },
    Profile {
        device: DeviceInfo::new(
            "xteink-x4-pro",
            "Xteink X4 Pro",
            DisplayInfo::new(480, 800, 217, PixelFormat::Mono1, Rotation::Deg90, None),
            InputCaps::new(true, true, false, &[]),
            MemoryClass::Large,
        ),
        // Previous/next, a touch home key and power (reset is hardware).
        buttons: &[
            buttons::PAGE_BACK,
            buttons::PAGE_FORWARD,
            buttons::HOME,
            buttons::POWER,
        ],
        heap_budget: Some(S3_HEAP),
        glyph_cache: X4_GLYPH_CACHE,
        busy_mark: true,
        colors: false,
        font_packs: true,
        // The controller takes 8 rows (turned) in a byte.
        frame_rows: Some(192),
        row_align: 8,
        settings: &X4_PRO_SETTINGS,
        speech: false,
        wifi: true,
        bluetooth: 2,
        catalog_device: Some("x4-pro"),
        sleep: true,
        button_row: &[],
        // As the X4 Pro's shell has it: brightness in single percents,
        // cool and warm LEDs mixed in fives.
        light: Some(Light {
            max: 100,
            step: 1,
            percent: true,
            warmth: Some(5),
        }),
        body: X4_PRO_BODY,
        summary: "ESP32-S3 with 8 MB PSRAM, 4.3\" 800x480, touch + buttons, 10 MiB heap budget",
    },
    Profile {
        device: DeviceInfo::new(
            "xteink-x4-classic",
            "Xteink X4 Classic",
            DisplayInfo::new(480, 800, 217, PixelFormat::Mono1, Rotation::Deg90, None),
            InputCaps::new(true, false, false, &[]),
            MemoryClass::Large,
        ),
        // The X4's buttons, the X4 Pro's screen and memory.
        buttons: &X4_BUTTONS,
        heap_budget: Some(S3_HEAP),
        glyph_cache: X4_GLYPH_CACHE,
        busy_mark: true,
        colors: false,
        font_packs: true,
        frame_rows: Some(192),
        row_align: 8,
        settings: &X4_SETTINGS,
        speech: false,
        wifi: true,
        bluetooth: 0,
        catalog_device: Some("x4-classic"),
        sleep: true,
        button_row: &X4_ROW,
        light: None,
        body: X4_BODY,
        summary: "experimental: ESP32-S3 with 8 MB PSRAM, 4.3\" 800x480, buttons only, 10 MiB heap budget",
    },
    // The X3 (experimental, not yet tried on one): the X4's chip, buttons
    // and memory, a 3.68" panel of 792 x 528 (528 x 792 upright). Its
    // firmware takes the plugin library's X4 builds (the same chip and
    // fingerprint).
    Profile {
        device: DeviceInfo::new(
            "xteink-x3",
            "Xteink X3",
            DisplayInfo::new(528, 792, 260, PixelFormat::Mono1, Rotation::Deg90, None),
            InputCaps::new(true, false, false, &[]),
            MemoryClass::Small,
        ),
        buttons: &X4_BUTTONS,
        heap_budget: Some(256 * KIB),
        glyph_cache: X4_GLYPH_CACHE,
        busy_mark: true,
        colors: false,
        font_packs: true,
        frame_rows: Some(192),
        row_align: 8,
        settings: &X4_SETTINGS,
        speech: false,
        wifi: true,
        bluetooth: 0,
        catalog_device: Some("x4"),
        sleep: true,
        button_row: &X4_ROW,
        light: None,
        body: X4_BODY,
        summary: "experimental: ESP32-C3, 3.7\" 792x528, buttons only, 256 KiB heap budget",
    },
    Profile {
        device: DeviceInfo::new(
            "boox-go-7",
            "Boox Go 7",
            DisplayInfo::new(
                1264,
                1680,
                // As the Android shell draws on gray e-ink.
                300,
                PixelFormat::Gray8,
                Rotation::Deg0,
                None,
            ),
            InputCaps::new(true, true, false, &[]),
            MemoryClass::Large,
        ),
        // The page buttons and Android's back; the system keeps power.
        buttons: &[buttons::PAGE_BACK, buttons::PAGE_FORWARD, buttons::BACK],
        heap_budget: None,
        glyph_cache: 256 * KIB,
        busy_mark: false,
        colors: true,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        settings: &[],
        speech: true,
        wifi: false,
        bluetooth: 0,
        catalog_device: Some("android"),
        sleep: false,
        button_row: &[],
        // Unverified: Onyx's levels from 0 to 255, warm and cool LEDs;
        // the Android shell takes the device's range, in steps of 1.
        light: Some(Light {
            max: 255,
            step: 1,
            percent: false,
            warmth: Some(1),
        }),
        body: BOOX_GO_7_BODY,
        summary: "Android, 7\" Carta 1300 1264x1680, touch + page buttons",
    },
    Profile {
        device: DeviceInfo::new(
            "kobo-libra-colour",
            "Kobo Libra Colour",
            DisplayInfo::new(
                1264,
                1680,
                300,
                PixelFormat::Rgb888,
                Rotation::Deg0,
                Some(ColorPanel::new(2, 55)),
            ),
            InputCaps::new(true, true, true, &[]),
            MemoryClass::Large,
        ),
        buttons: &[buttons::PAGE_BACK, buttons::PAGE_FORWARD, buttons::POWER],
        heap_budget: None,
        glyph_cache: 256 * KIB,
        busy_mark: false,
        colors: false,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        settings: &[],
        speech: false,
        wifi: false,
        bluetooth: 0,
        catalog_device: None,
        sleep: true,
        button_row: &[],
        light: None,
        body: Body::PLAIN,
        summary: "Linux, 7\" Kaleido 3 colour 1264x1680, touch + page buttons",
    },
    Profile {
        device: DeviceInfo::new(
            "kindle-paperwhite-5",
            "Kindle Paperwhite (11th gen)",
            DisplayInfo::new(1236, 1648, 300, PixelFormat::Gray4, Rotation::Deg0, None),
            InputCaps::new(false, true, false, &[]),
            MemoryClass::Large,
        ),
        buttons: &[buttons::POWER],
        heap_budget: None,
        glyph_cache: 256 * KIB,
        busy_mark: false,
        colors: false,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        settings: &[],
        speech: false,
        wifi: false,
        bluetooth: 0,
        catalog_device: None,
        sleep: true,
        button_row: &[],
        light: None,
        body: Body::PLAIN,
        summary: "Linux, 6.8\" 1236x1648, touch only",
    },
    Profile {
        device: DeviceInfo::new(
            "kobo-clara-bw",
            "Kobo Clara BW",
            DisplayInfo::new(1072, 1448, 300, PixelFormat::Gray4, Rotation::Deg0, None),
            InputCaps::new(false, true, false, &[]),
            MemoryClass::Large,
        ),
        buttons: &[buttons::POWER],
        heap_budget: None,
        glyph_cache: 256 * KIB,
        busy_mark: false,
        colors: false,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        settings: &[],
        speech: false,
        wifi: false,
        bluetooth: 0,
        catalog_device: None,
        sleep: true,
        button_row: &[],
        light: None,
        body: Body::PLAIN,
        summary: "Linux, 6\" 1072x1448, touch only",
    },
    Profile {
        device: DeviceInfo::new(
            "two-buttons",
            "Two-button e-reader",
            DisplayInfo::new(600, 800, 167, PixelFormat::Gray4, Rotation::Deg0, None),
            InputCaps::new(true, false, false, &[]),
            MemoryClass::Large,
        ),
        buttons: &[buttons::PAGE_BACK, buttons::PAGE_FORWARD, buttons::POWER],
        heap_budget: None,
        glyph_cache: 64 * KIB,
        busy_mark: false,
        colors: false,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        // Only two buttons: everything works with forward and back.
        settings: &[("core.keymap-preset", "core.two-buttons")],
        speech: false,
        wifi: false,
        bluetooth: 0,
        catalog_device: None,
        sleep: true,
        button_row: &[],
        light: None,
        body: Body::PLAIN,
        summary: "Generic 6\" 600x800, only page buttons (also: Bluetooth page turners)",
    },
];

pub fn find(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.device.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each key drawn is a button the device has, lies on its body, and
    /// no computer key presses two of them.
    #[test]
    fn the_keys_on_a_body_are_the_device_s() {
        for profile in PROFILES {
            let id = profile.device.id;
            let mut taken = Vec::new();
            for key in profile.body.keys {
                assert!(
                    profile.buttons.contains(&key.button),
                    "{id}: {}",
                    key.button
                );
                let half = key.length / 2;
                assert!(
                    key.at >= half && key.at + half <= 100,
                    "{id}: {}",
                    key.button
                );
                for pressed in key.keys {
                    assert!(!taken.contains(pressed), "{id}: {pressed} twice");
                    taken.push(*pressed);
                }
            }
        }
        let x4 = find("xteink-x4").unwrap().body;
        assert_eq!(x4.keys.len(), 7, "every key of the X4");
        let pro = find("xteink-x4-pro").unwrap().body;
        let side = |button| pro.keys.iter().find(|k| k.button == button).unwrap().place;
        assert_eq!(side(buttons::PAGE_BACK), Place::Left);
        assert_eq!(side(buttons::PAGE_FORWARD), Place::Right);
    }

    #[test]
    fn a_body_is_given_as_json() {
        let json = find("xteink-x4").unwrap().body.json();
        assert!(
            json.starts_with("{\"bezel\":[7,7,30,7],\"keys\":["),
            "{json}"
        );
        assert!(
            json.contains("{\"button\":\"page-back\",\"place\":\"right\",\"at\":36,\"length\":13,\"keys\":[\"PageUp\",\"ArrowUp\"]}"),
            "{json}"
        );
        assert_eq!(Body::PLAIN.json(), "{\"bezel\":[6,6,6,6],\"keys\":[]}");
    }
}
