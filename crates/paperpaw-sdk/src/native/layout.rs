//! The layouts [`LAYOUT`](super::LAYOUT) digests, and the hash the
//! fingerprint is made with.

use core::mem::{align_of, offset_of, size_of};

use paperpaw_native::{Api, Entry};

/// FNV-1a with 64 bits, at compile time (as `paperpaw_digest`'s).
#[derive(Clone, Copy)]
pub(super) struct Fnv(pub(super) u64);

impl Fnv {
    pub(super) const fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    const fn byte(self, b: u8) -> Fnv {
        Fnv((self.0 ^ b as u64).wrapping_mul(0x0100_0000_01b3))
    }

    pub(super) const fn number(self, n: u64) -> Fnv {
        let mut hash = self;
        let mut i = 0;
        while i < 8 {
            hash = hash.byte((n >> (8 * i)) as u8);
            i += 1;
        }
        hash
    }

    pub(super) const fn text(self, text: &str) -> Fnv {
        let bytes = text.as_bytes();
        let mut hash = self;
        let mut i = 0;
        while i < bytes.len() {
            hash = hash.byte(bytes[i]);
            i += 1;
        }
        hash.byte(0)
    }
}

/// The chip, as the compiler sees it.
pub(super) const TARGET: &str = if cfg!(target_arch = "riscv32") {
    "riscv32"
} else if cfg!(target_arch = "xtensa") {
    "xtensa"
} else if cfg!(all(target_arch = "aarch64", target_os = "android")) {
    "aarch64-android"
} else if cfg!(target_arch = "aarch64") {
    "aarch64"
} else if cfg!(target_arch = "x86_64") {
    "x86_64"
} else if cfg!(target_arch = "arm") {
    "arm"
} else if cfg!(target_arch = "wasm32") {
    "wasm32"
} else {
    "other"
};

/// The size and alignment of `T`.
const fn live<T>() -> (u64, u64) {
    (size_of::<T>() as u64, align_of::<T>() as u64)
}

/// The sizes and alignments the old plugin API's types had when interface 7
/// was made, in the order [`CROSSING`] names them (#244, slice D7a): frozen,
/// so that the types can leave the SDK while [`DIGEST`], and with it the
/// fingerprint, stays what it was. Read from the SDK as built for each
/// target, with and without its features (none of them changes one): one
/// table for 64-bit targets (x86_64, aarch64), one for 32-bit x86 (`u64`
/// aligned to 4), one for the other 32-bit targets (riscv32, xtensa, arm,
/// wasm32). They go when the fingerprint is next made anew.
#[cfg(target_pointer_width = "64")]
const OLD: [(u64, u64); 91] = [
    (744, 8),
    (712, 8),
    (32, 8),
    (2, 2),
    (40, 8),
    (88, 8),
    (80, 8),
    (32, 8),
    (64, 8),
    (40, 8),
    (4, 4),
    (400, 8),
    (152, 8),
    (24, 8),
    (40, 8),
    (192, 8),
    (144, 8),
    (40, 8),
    (80, 8),
    (40, 8),
    (24, 8),
    (72, 8),
    (88, 8),
    (64, 8),
    (320, 8),
    (96, 8),
    (32, 8),
    (208, 8),
    (192, 8),
    (48, 8),
    (1, 1),
    (24, 8),
    (16, 8),
    (136, 8),
    (72, 8),
    (48, 8),
    (120, 8),
    (80, 8),
    (72, 8),
    (24, 8),
    (88, 8),
    (96, 8),
    (56, 8),
    (24, 8),
    (48, 8),
    (104, 8),
    (56, 8),
    (56, 8),
    (48, 8),
    (64, 8),
    (40, 8),
    (32, 8),
    (72, 8),
    (96, 8),
    (16, 8),
    (56, 8),
    (56, 8),
    (88, 8),
    (72, 8),
    (112, 8),
    (56, 8),
    (56, 8),
    (32, 8),
    (112, 8),
    (88, 8),
    (16, 8),
    (16, 8),
    (64, 8),
    (2, 2),
    (40, 8),
    (104, 8),
    (56, 8),
    (56, 8),
    (64, 8),
    (80, 8),
    (112, 8),
    (104, 8),
    (88, 8),
    (104, 8),
    (144, 8),
    (72, 8),
    (40, 8),
    (104, 8),
    (112, 8),
    (56, 8),
    (1, 1),
    (1, 1),
    (1, 1),
    (1, 1),
    (136, 8),
    (56, 8),
];
#[cfg(all(target_pointer_width = "32", target_arch = "x86"))]
const OLD: [(u64, u64); 91] = [
    (456, 4),
    (440, 4),
    (28, 4),
    (2, 2),
    (20, 4),
    (44, 4),
    (48, 4),
    (20, 4),
    (44, 4),
    (24, 4),
    (4, 4),
    (200, 4),
    (76, 4),
    (12, 4),
    (20, 4),
    (112, 4),
    (88, 4),
    (20, 4),
    (48, 4),
    (32, 4),
    (12, 4),
    (40, 4),
    (44, 4),
    (32, 4),
    (172, 4),
    (48, 4),
    (16, 4),
    (104, 4),
    (104, 4),
    (32, 4),
    (1, 1),
    (12, 4),
    (8, 4),
    (80, 4),
    (44, 4),
    (44, 4),
    (64, 4),
    (40, 4),
    (36, 4),
    (12, 4),
    (44, 4),
    (48, 4),
    (32, 4),
    (16, 4),
    (24, 4),
    (52, 4),
    (28, 4),
    (28, 4),
    (24, 4),
    (36, 4),
    (20, 4),
    (16, 4),
    (40, 4),
    (48, 4),
    (8, 4),
    (28, 4),
    (28, 4),
    (44, 4),
    (36, 4),
    (56, 4),
    (28, 4),
    (28, 4),
    (16, 4),
    (56, 4),
    (44, 4),
    (8, 4),
    (12, 4),
    (32, 4),
    (2, 2),
    (20, 4),
    (52, 4),
    (28, 4),
    (28, 4),
    (32, 4),
    (40, 4),
    (56, 4),
    (52, 4),
    (48, 4),
    (52, 4),
    (72, 4),
    (36, 4),
    (20, 4),
    (52, 4),
    (60, 4),
    (28, 4),
    (1, 1),
    (1, 1),
    (1, 1),
    (1, 1),
    (96, 4),
    (40, 4),
];
#[cfg(all(target_pointer_width = "32", not(target_arch = "x86")))]
const OLD: [(u64, u64); 91] = [
    (472, 8),
    (456, 8),
    (32, 8),
    (2, 2),
    (20, 4),
    (48, 8),
    (48, 8),
    (20, 4),
    (48, 8),
    (24, 4),
    (4, 4),
    (200, 4),
    (88, 8),
    (16, 8),
    (24, 8),
    (112, 4),
    (88, 4),
    (20, 4),
    (48, 4),
    (32, 4),
    (12, 4),
    (40, 4),
    (44, 4),
    (32, 4),
    (176, 8),
    (48, 4),
    (16, 4),
    (104, 4),
    (112, 8),
    (32, 4),
    (1, 1),
    (12, 4),
    (8, 4),
    (88, 8),
    (44, 4),
    (48, 8),
    (72, 8),
    (40, 4),
    (36, 4),
    (12, 4),
    (44, 4),
    (48, 4),
    (32, 4),
    (16, 4),
    (24, 4),
    (52, 4),
    (28, 4),
    (28, 4),
    (24, 4),
    (40, 8),
    (20, 4),
    (16, 4),
    (48, 8),
    (48, 4),
    (8, 4),
    (28, 4),
    (28, 4),
    (44, 4),
    (36, 4),
    (56, 4),
    (28, 4),
    (28, 4),
    (16, 4),
    (56, 4),
    (44, 4),
    (8, 4),
    (16, 8),
    (32, 4),
    (2, 2),
    (20, 4),
    (52, 4),
    (28, 4),
    (28, 4),
    (32, 4),
    (40, 4),
    (56, 4),
    (52, 4),
    (48, 4),
    (52, 4),
    (72, 4),
    (36, 4),
    (20, 4),
    (52, 4),
    (60, 4),
    (28, 4),
    (1, 1),
    (1, 1),
    (1, 1),
    (1, 1),
    (96, 4),
    (40, 4),
];

/// The types that cross between a plugin and PaperPaw: what the SDK
/// gives a plugin (by reference or by value), what a plugin gives
/// back, and the boxes and trait objects they come in.
const CROSSING: &[(u64, u64)] = &[
    OLD[0],  // crate::Env<'static>
    OLD[1],  // crate::env::EnvParts<'static>
    OLD[2],  // crate::Capabilities
    OLD[3],  // crate::Metrics
    OLD[4],  // Collected
    OLD[5],  // crate::contribution::item::Contribution
    OLD[6],  // crate::Command
    OLD[7],  // crate::Cx<'static, 'static>
    OLD[8],  // crate::screen::Output
    OLD[9],  // crate::StackEntry
    OLD[10], // crate::ScreenKey
    live::<crate::Canvas<'static>>(),
    OLD[11], // crate::Settings
    OLD[12], // crate::SettingSpec
    OLD[13], // crate::SettingValue
    OLD[14], // crate::SettingKind
    live::<crate::Translator>(),
    // What a `Translator` holds: a plugin's own `tr` walks its catalogs.
    live::<paperpaw_i18n::Catalog>(),
    OLD[15], // crate::Fonts
    OLD[16], // crate::FontHandle<'static>
    OLD[17], // crate::Params
    live::<crate::Id>(),
    live::<crate::Version>(),
    OLD[18], // crate::Manifest
    OLD[19], // crate::PluginInfo
    OLD[20], // crate::Plugins
    OLD[21], // crate::ScreenSpec
    OLD[22], // crate::ActionSpec
    OLD[23], // crate::MenuEntry
    OLD[24], // crate::Menus
    OLD[25], // crate::Actions
    OLD[26], // crate::RegisterError
    OLD[27], // crate::HttpRequest
    OLD[28], // crate::HttpReply
    OLD[29], // crate::Gesture
    OLD[30], // crate::Handled
    OLD[31], // crate::Hint
    live::<crate::Rect>(),
    live::<crate::Point>(),
    live::<crate::Color>(),
    live::<crate::InputEvent>(),
    live::<crate::DeviceInfo>(),
    live::<crate::Instant>(),
    live::<super::Native>(),
    live::<super::Fingerprint>(),
    // An old plugin in its box, while the SDK had the trait (#244): two
    // words, as any boxed trait object is.
    live::<alloc::boxed::Box<dyn core::any::Any>>(),
    OLD[32], // alloc::boxed::Box<dyn crate::Screen>
    live::<&'static dyn crate::Storage>(),
    // Not `Api`: it grows at its end without a raise; `TABLE` keeps
    // where its fields of interface 5 lie.
    live::<Entry>(),
    // What `Env` reaches.
    OLD[33], // crate::firmware::Firmware
    OLD[34], // crate::formats::Formats
    OLD[35], // crate::usage::Usage
    // `Env::hyphenation` and what its `get` gives: card-backed since
    // draft 2.
    OLD[36], // crate::hyphenation::Hyphenation
    OLD[37], // crate::hyphenation::Hyphenator<'static>
    OLD[38], // crate::status::StatusItems
    OLD[39], // crate::readers::Readers
    OLD[40], // crate::widgets::Widgets
    OLD[41], // crate::services::Services
    OLD[42], // crate::services::Service
    OLD[43], // crate::power::Power
    OLD[44], // crate::work::WorkStates
    OLD[45], // crate::wifi::Wifi
    OLD[46], // crate::library::BookData
    OLD[47], // crate::library::service::BookBadge
    OLD[48], // crate::library::service::BookField
    OLD[49], // crate::keys::InputSpec
    // What a `Contribution` holds: the enum's size hides a change of
    // a smaller one.
    OLD[50], // crate::action::ActionOverride
    OLD[51], // crate::action::ObserverSpec
    OLD[52], // crate::chrome::ChromeSpec
    OLD[53], // crate::decorations::DecoratorSpec
    OLD[54], // crate::event::EventHandler
    OLD[55], // crate::filters::InputFilterSpec
    OLD[56], // crate::filters::RefreshFilterSpec
    OLD[57], // crate::filters::TextFilterSpec
    OLD[58], // crate::fonts::FontFace
    live::<crate::fonts::FontSpec>(),
    OLD[59], // crate::formats::FormatSpec
    OLD[60], // crate::hyphenation::HyphenationSpec
    OLD[61], // crate::keys::GestureSpec
    OLD[62], // crate::layers::PageFrameSpec
    OLD[63], // crate::layers::PageLayerSpec
    OLD[64], // crate::lookup::LookupProviderSpec
    OLD[65], // crate::menus::ItemStateFn
    OLD[66], // crate::menus::MenuNeed
    OLD[67], // crate::menus::MenuSection
    OLD[68], // crate::plugin::Priority
    OLD[69], // crate::plugin::SlotSpec
    OLD[70], // crate::presets::Preset
    OLD[71], // crate::readers::ReaderSpec
    OLD[72], // crate::services::EventSpec
    OLD[73], // crate::services::PointSpec
    OLD[74], // crate::settings::SettingGroupSpec
    OLD[75], // crate::setup::SetupStepSpec
    OLD[76], // crate::status::Place
    OLD[77], // crate::status::StatusItemSpec
    OLD[78], // crate::status::StatusLineSpec
    OLD[79], // crate::ui::look::Painters
    OLD[80], // crate::voices::VoiceSpec
    OLD[81], // crate::widgets::WidgetCategory
    OLD[82], // crate::widgets::WidgetPlace
    OLD[83], // crate::widgets::WidgetSpec
    OLD[84], // crate::work::WorkSpec
    // What trait methods take and give.
    OLD[85], // crate::screen::HintVisibility
    OLD[86], // crate::screen::Outside
    OLD[87], // crate::screen::ShowsThrough
    OLD[88], // crate::screen::CloseCause
    live::<paperpaw_platform::PointerEvent>(),
    live::<paperpaw_document::Block>(),
    // What `Env`, `Canvas` and `Storage` reach whose fields a plugin's own
    // copy of the code reads: the frame it draws in (its drawing code is
    // the plugin's), the device and status it is given, the storage's
    // replies.
    live::<paperpaw_graphics::Framebuffer>(),
    // What page layers and the reading code see of a page.
    OLD[89], // crate::layers::PageView<'static>
    OLD[90], // crate::reading::WordBox
    live::<paperpaw_layout::Page>(),
    live::<paperpaw_layout::PageLine>(),
    live::<paperpaw_layout::LineRun>(),
    live::<paperpaw_layout::LineBidi>(),
    live::<crate::PixelFormat>(),
    live::<crate::DisplayInfo>(),
    live::<crate::InputCaps>(),
    live::<crate::SystemStatus>(),
    live::<crate::Battery>(),
    live::<crate::WallClock>(),
    live::<crate::StorageError>(),
    live::<crate::DirEntry<'static>>(),
    // What the shared text functions take and give by value, between the
    // firmware's code and the plugin's: the plugin
    // lives on across firmware releases, so a moved field of one of these
    // is an interface change. (The font crates they hold are pinned with
    // `=` in paperpaw-text; `tools/interface.py` digests those pins.)
    live::<paperpaw_text::Font>(),
    live::<paperpaw_text::GlyphCache>(),
    live::<paperpaw_text::Line>(),
    live::<paperpaw_text::TextStyle<'static>>(),
    live::<paperpaw_text::GlyphRef<'static>>(),
    // Taken and given by the shared text entries and the methods of the
    // handles (`StoredPack`, `GlyphCache`) the firmware makes.
    live::<paperpaw_text::Around>(),
    live::<paperpaw_text::ToneCurve>(),
    live::<paperpaw_text::LineMetrics>(),
    live::<paperpaw_text::FontError>(),
    live::<paperpaw_text::Glyph>(),
    live::<paperpaw_text::StoredPack>(),
    live::<paperpaw_text::PackLimits>(),
];

/// The scalable font, with the `skrifa` location it holds by value: in
/// the fingerprint of a plugin built with the `outline` feature only.
#[cfg(feature = "outline")]
const CROSSING_OUTLINE: &[(u64, u64)] = &[live::<paperpaw_text::OutlineFont>()];

/// Where the firmware's table keeps each field of interface 5: a
/// plugin finds them there. A frozen list: fields appended after
/// `build_len` are not in it, so they leave the fingerprint as it is
/// (a plugin that uses one says so in its `needs_api`, and the size of
/// the firmware's table tells whether it has it).
const TABLE: [u64; 12] = [
    offset_of!(Api, size) as u64,
    offset_of!(Api, alloc) as u64,
    offset_of!(Api, dealloc) as u64,
    offset_of!(Api, realloc) as u64,
    offset_of!(Api, panic) as u64,
    offset_of!(Api, micros) as u64,
    offset_of!(Api, icon) as u64,
    offset_of!(Api, alloc_in) as u64,
    offset_of!(Api, family_revs) as u64,
    offset_of!(Api, family_min) as u64,
    offset_of!(Api, build) as u64,
    offset_of!(Api, build_len) as u64,
];

/// Where a pointer event's fields lie: its size hides fields added in its
/// padding (the pen's tilt), and a plugin's own code reads them.
const POINTER: [u64; 7] = {
    use paperpaw_platform::PointerEvent;
    [
        offset_of!(PointerEvent, id) as u64,
        offset_of!(PointerEvent, phase) as u64,
        offset_of!(PointerEvent, point) as u64,
        offset_of!(PointerEvent, tool) as u64,
        offset_of!(PointerEvent, pressure) as u64,
        offset_of!(PointerEvent, tilt_x) as u64,
        offset_of!(PointerEvent, tilt_y) as u64,
    ]
};

pub(super) const DIGEST: u64 = {
    let mut hash = Fnv::new();
    let mut i = 0;
    while i < CROSSING.len() {
        hash = hash.number(CROSSING[i].0).number(CROSSING[i].1);
        i += 1;
    }
    #[cfg(feature = "outline")]
    {
        let mut i = 0;
        while i < CROSSING_OUTLINE.len() {
            hash = hash
                .number(CROSSING_OUTLINE[i].0)
                .number(CROSSING_OUTLINE[i].1);
            i += 1;
        }
    }
    let mut i = 0;
    while i < TABLE.len() {
        hash = hash.number(TABLE[i]);
        i += 1;
    }
    let mut i = 0;
    while i < POINTER.len() {
        hash = hash.number(POINTER[i]);
        i += 1;
    }
    hash.0
};
