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

/// Sizes and alignments of `$t`, in order.
macro_rules! layouts {
    ($($t:ty),* $(,)?) => {
        [$((size_of::<$t>() as u64, align_of::<$t>() as u64)),*]
    };
}

/// The types that cross between a plugin and PaperPaw: what the SDK
/// gives a plugin (by reference or by value), what a plugin gives
/// back, and the boxes and trait objects they come in.
const CROSSING: &[(u64, u64)] = &layouts![
    crate::Env<'static>,
    crate::env::EnvParts<'static>,
    crate::Capabilities,
    crate::Metrics,
    crate::Registrar,
    crate::contribution::item::Contribution,
    crate::Command,
    crate::Cx<'static, 'static>,
    crate::screen::Output,
    crate::StackEntry,
    crate::ScreenKey,
    crate::Canvas<'static>,
    crate::Settings,
    crate::SettingSpec,
    crate::SettingValue,
    crate::SettingKind,
    crate::Translator,
    crate::Fonts,
    crate::FontHandle<'static>,
    crate::Params,
    crate::Id,
    crate::Version,
    crate::Manifest,
    crate::PluginInfo,
    crate::Plugins,
    crate::ScreenSpec,
    crate::ActionSpec,
    crate::MenuEntry,
    crate::Menus,
    crate::Actions,
    crate::RegisterError,
    crate::HttpRequest,
    crate::HttpReply,
    crate::Gesture,
    crate::Handled,
    crate::Hint,
    crate::Rect,
    crate::Point,
    crate::Color,
    crate::InputEvent,
    crate::DeviceInfo,
    crate::Instant,
    super::Native,
    super::Fingerprint,
    alloc::boxed::Box<dyn crate::Plugin>,
    alloc::boxed::Box<dyn crate::Screen>,
    &'static dyn crate::Storage,
    // Not `Api`: it grows at its end without a raise; `TABLE` keeps
    // where its fields of interface 5 lie.
    Entry,
    // What `Env` reaches.
    crate::firmware::Firmware,
    crate::formats::Formats,
    crate::usage::Usage,
    crate::status::StatusItems,
    crate::readers::Readers,
    crate::widgets::Widgets,
    crate::services::Services,
    crate::services::Service,
    crate::power::Power,
    crate::work::WorkStates,
    crate::wifi::Wifi,
    crate::library::BookData,
    crate::library::service::BookBadge,
    crate::library::service::BookField,
    crate::keys::InputSpec,
    // What a `Contribution` holds: the enum's size hides a change of
    // a smaller one.
    crate::action::ActionOverride,
    crate::action::ObserverSpec,
    crate::chrome::ChromeSpec,
    crate::decorations::DecoratorSpec,
    crate::event::EventHandler,
    crate::filters::InputFilterSpec,
    crate::filters::RefreshFilterSpec,
    crate::filters::TextFilterSpec,
    crate::fonts::FontFace,
    crate::fonts::FontSpec,
    crate::formats::FormatSpec,
    crate::hyphenation::HyphenationSpec,
    crate::keys::GestureSpec,
    crate::layers::PageFrameSpec,
    crate::layers::PageLayerSpec,
    crate::lookup::LookupProviderSpec,
    crate::menus::ItemStateFn,
    crate::menus::MenuNeed,
    crate::menus::MenuSection,
    crate::plugin::Priority,
    crate::plugin::SlotSpec,
    crate::presets::Preset,
    crate::readers::ReaderSpec,
    crate::services::EventSpec,
    crate::services::PointSpec,
    crate::settings::SettingGroupSpec,
    crate::setup::SetupStepSpec,
    crate::status::Place,
    crate::status::StatusItemSpec,
    crate::status::StatusLineSpec,
    crate::ui::look::Painters,
    crate::voices::VoiceSpec,
    crate::widgets::WidgetCategory,
    crate::widgets::WidgetPlace,
    crate::widgets::WidgetSpec,
    crate::work::WorkSpec,
    // What trait methods take and give.
    crate::screen::HintVisibility,
    crate::screen::Outside,
    crate::screen::ShowsThrough,
    crate::screen::CloseCause,
    paperpaw_platform::PointerEvent,
    paperpaw_document::Block,
    // What the shared text functions take and give by value, between the
    // firmware's code and the plugin's (`paperpaw_table`): the plugin
    // lives on across firmware releases, so a moved field of one of these
    // is an interface change. (The font crates they hold are pinned with
    // `=` in paperpaw-text; `tools/interface.py` digests those pins.)
    paperpaw_text::Font,
    paperpaw_text::GlyphCache,
    paperpaw_text::Line,
    paperpaw_text::TextStyle<'static>,
    paperpaw_text::GlyphRef<'static>,
];

/// The scalable font, with the `skrifa` location it holds by value: in
/// the fingerprint of a plugin built with the `outline` feature only.
#[cfg(feature = "outline")]
const CROSSING_OUTLINE: &[(u64, u64)] = &layouts![paperpaw_text::OutlineFont];

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
    hash.0
};
