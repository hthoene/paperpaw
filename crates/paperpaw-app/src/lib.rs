//! The standard PaperPaw app: the first-party plugins every shell installs.
//!
//! The kernel itself has no product features; a shell decides which plugins
//! it ships. This crate keeps that list in one place, so the emulator, the
//! Android app and the firmware ship the same app. Shells can still add,
//! leave out or replace any plugin.
//!
//! Two kinds of first-party plugins:
//!
//! - **Built into every PaperPaw**: reading, the formats, the library and
//!   home, menus and settings, and what sets up Wi-Fi, updates, installs
//!   more and recovers (the store, the plugin manager, the keyboard), as
//!   a beginner needs them on a fresh device.
//! - **Offered by the store as files too** (feature `store-plugins`, ADR
//!   0024): statistics, widgets, sync and more. The X4 and the X4 Pro
//!   leave them out of their firmware, which stays lean, and install them
//!   from the store; the emulator and the Android app build them in and
//!   switch them off until the reader wants them ([`StorePlugins`]).
//!
//! The diagnostics screen (feature `diagnostics`) is for developer
//! builds.

#![no_std]

extern crate alloc;

pub mod flasher;
mod pebble;
pub mod startup;

pub use pebble::Pebble;

use paperpaw_core::KernelBuilder;

/// How the default fonts are shipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fonts {
    /// Scalable fonts rendered at runtime (needs the `outline` feature).
    #[cfg(feature = "outline")]
    Outline,
    /// Bitmap font packs read in place, for small devices.
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    Packs,
}

/// What a shell does with the plugins the store also offers as files.
#[derive(Clone, Copy)]
pub enum StorePlugins<'a> {
    /// Left out: the store installs them (the X4, the X4 Pro).
    Left,
    /// Built in and switched on as they always were (tests).
    #[cfg(feature = "store-plugins")]
    On,
    /// Built in, switched off until the reader switches one on (in the
    /// plugin manager, or the store), unless the function says it is on:
    /// e.g. one used before, whose data is on the card (Android, the
    /// emulator).
    #[cfg(feature = "store-plugins")]
    Off(&'a dyn Fn(&str) -> bool),
    #[doc(hidden)]
    #[cfg(not(feature = "store-plugins"))]
    _Never(core::marker::PhantomData<&'a ()>, core::convert::Infallible),
}

impl core::fmt::Debug for StorePlugins<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            StorePlugins::Left => "Left",
            #[cfg(feature = "store-plugins")]
            StorePlugins::On => "On",
            #[cfg(feature = "store-plugins")]
            StorePlugins::Off(_) => "Off",
            #[cfg(not(feature = "store-plugins"))]
            StorePlugins::_Never(_, never) => match *never {},
        })
    }
}

/// The ids of the plugins the store offers as files too: built in only
/// with the feature `store-plugins`.
pub const STORE_PLUGINS: [&str; 34] = [
    "aids",
    "battery",
    "browser",
    "calibre",
    "end-of-book",
    "fonts-browser",
    "hyphenation-da",
    "hyphenation-de",
    "hyphenation-es",
    "hyphenation-fr",
    "hyphenation-it",
    "hyphenation-nb",
    "hyphenation-nl",
    "hyphenation-pl",
    "hyphenation-pt",
    "hyphenation-ru",
    "hyphenation-sv",
    "hyphenation-uk",
    "joplin",
    "keys",
    "koreader",
    "kosync",
    "menu-editor",
    "news",
    "profiles",
    "reading-goal",
    "readwise",
    "rules",
    "stats",
    "translate",
    "vocabulary",
    "wallabag",
    "weather",
    "widgets",
];

/// Whether PaperPaw ran on `storage` before (it keeps its folder or its
/// configuration there, also under its old name): a shell that builds in
/// the plugins the store offers but switches them off for a new reader
/// keeps them as they were for one who had them (on as they shipped, off
/// where the reader switched one off), so nobody loses one by an update.
pub fn ran_before(storage: &dyn paperpaw_core::sdk::Storage) -> bool {
    paperpaw_core::sdk::data::take_over_old_name(storage);
    storage.exists(paperpaw_core::sdk::builtin::CONFIG_FILE)
        || storage
            .list(paperpaw_core::sdk::data::DIR, &mut |_| {})
            .is_ok()
}

/// The plugin crates this build has (`paperpaw-plugin-…`), sorted: what
/// the X4's firmware lists in its image (ADR 0022).
pub const CRATES: &str = env!("PAPERPAW_APP_CRATES");

/// Adds the first-party plugins to `builder`: every one this build has,
/// switched on as each ships (the plugins the store offers as files too
/// on, where the feature `store-plugins` builds them).
pub fn install(builder: KernelBuilder, fonts: Fonts) -> KernelBuilder {
    #[cfg(feature = "store-plugins")]
    let store = StorePlugins::On;
    #[cfg(not(feature = "store-plugins"))]
    let store = StorePlugins::Left;
    install_with(builder, fonts, store)
}

/// The network settings of every networked first-party plugin, kept in
/// the program rather than made on the heap at every start (see
/// [`KernelBuilder::network_settings`]); one missing here only costs
/// memory (the tests say so).
pub static NETWORK_SETTINGS: &[paperpaw_core::NetworkSettings] = paperpaw_core::network_settings!(
    "browser",
    "calibre",
    "fonts-browser",
    "joplin",
    "kosync",
    "news",
    "opds",
    "readwise",
    "stats",
    "store",
    "transfer",
    "translate",
    "update",
    "vocabulary",
    "wallabag",
    "weather",
    "wifi",
    "wikipedia",
);

/// The indexes of the first-party plugins' translation catalogs, made
/// when the program was built (see `build.rs`): their catalogs keep none
/// on the heap.
pub static CATALOG_INDEXES: &[(u64, &[u16])] =
    include!(concat!(env!("OUT_DIR"), "/catalog_indexes.rs"));

/// Adds the first-party plugins to `builder`, those the store offers as
/// files too as `store` says.
pub fn install_with(
    builder: KernelBuilder,
    fonts: Fonts,
    store: StorePlugins<'_>,
) -> KernelBuilder {
    let builder = builder
        .network_settings(NETWORK_SETTINGS)
        .catalog_indexes(CATALOG_INDEXES);
    let builder = match fonts {
        #[cfg(feature = "outline")]
        Fonts::Outline => builder.plugin(paperpaw_plugin_fonts_default::DefaultFonts),
        #[cfg(any(feature = "packs", not(feature = "outline")))]
        Fonts::Packs => builder.plugin(paperpaw_plugin_fonts_default::DefaultFontPacks),
    };
    #[cfg(not(feature = "store-plugins"))]
    let _ = store;
    let mut b = builder
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(paperpaw_plugin_format_epub::EpubFormat)
        .plugin(paperpaw_plugin_format_epub::HtmlFormat)
        .plugin(paperpaw_plugin_format_epub::Fb2Format)
        .plugin(paperpaw_plugin_format_epub::MarkdownFormat)
        .plugin(paperpaw_plugin_format_epub::CbzFormat)
        .plugin(paperpaw_plugin_format_xtc::XtcFormat)
        .plugin(paperpaw_plugin_hyphenation::Hyphenation)
        .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
        .plugin(paperpaw_plugin_reader::Reader)
        .plugin(paperpaw_plugin_files::Files)
        .plugin(paperpaw_plugin_home::Home)
        .plugin(paperpaw_plugin_menu::Menu);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_menu_editor::MenuEditor);
        b = offered(b, store, paperpaw_plugin_kosync::Kosync);
        b = offered(b, store, paperpaw_plugin_koreader::KoReader);
        b = offered(b, store, paperpaw_plugin_calibre::Calibre);
        b = offered(b, store, paperpaw_plugin_stats::Stats);
        b = offered(b, store, paperpaw_plugin_reading_goal::ReadingGoal);
        b = offered(b, store, paperpaw_plugin_end_of_book::EndOfBook);
        b = offered(b, store, paperpaw_plugin_battery::Battery);
        b = offer_hyphenation(b, store);
    }
    b = b
        .plugin(paperpaw_plugin_wifi::Wifi)
        .plugin(paperpaw_plugin_bluetooth::Bluetooth)
        .plugin(paperpaw_plugin_power::Power)
        .plugin(paperpaw_plugin_file_manager::FileManager)
        .plugin(paperpaw_plugin_transfer::Transfer)
        .plugin(paperpaw_plugin_opds::Opds);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_fonts_browser::FontsBrowser);
    }
    b = b.plugin(paperpaw_plugin_store::Store);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_rules::Rules);
    }
    b = b.plugin(paperpaw_plugin_lock::Lock);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_profiles::Profiles);
        b = offered(b, store, paperpaw_plugin_keys::Keys);
    }
    b = b.plugin(paperpaw_plugin_wikipedia::Wikipedia);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_aids::Aids);
        b = offered(b, store, paperpaw_plugin_vocabulary::Vocabulary);
    }
    b = b.plugin(paperpaw_plugin_sleep::Sleep);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_translate::Translate);
        b = offered(b, store, paperpaw_plugin_news::News);
        b = offered(b, store, paperpaw_plugin_weather::Weather);
        b = offered(b, store, paperpaw_plugin_widgets::Widgets);
        // Off as they ship, wherever they are built in.
        let shipped_off = match store {
            StorePlugins::Left => StorePlugins::Left,
            _ => StorePlugins::Off(&|_| false),
        };
        b = offered(b, shipped_off, paperpaw_plugin_browser::Browser);
        b = offered(b, shipped_off, paperpaw_plugin_readwise::Readwise);
        b = offered(b, shipped_off, paperpaw_plugin_joplin::Joplin);
        b = offered(b, shipped_off, paperpaw_plugin_wallabag::Wallabag);
    }
    // A theme of the app's own, off as it ships, on every device.
    let builder = b
        .plugin_off(Pebble)
        .plugin(paperpaw_plugin_keyboard::Keyboard)
        .plugin(paperpaw_plugin_settings::SettingsPlugin)
        .plugin(paperpaw_plugin_plugin_manager::PluginManager);
    #[cfg(feature = "diagnostics")]
    let builder = builder.plugin(paperpaw_plugin_diagnostics::Diagnostics);
    builder
        .plugin(paperpaw_plugin_update::Update)
        .plugin(paperpaw_plugin_about::About)
        .plugin(paperpaw_plugin_setup::Setup)
        .plugin(paperpaw_plugin_help::Help)
}

/// Adds the hyphenation plugins of the languages the store offers, one
/// each, as `store` says.
#[cfg(feature = "store-plugins")]
fn offer_hyphenation(mut b: KernelBuilder, store: StorePlugins<'_>) -> KernelBuilder {
    b = offered(b, store, paperpaw_plugin_hyphenation_da::Danish);
    b = offered(b, store, paperpaw_plugin_hyphenation_de::German);
    b = offered(b, store, paperpaw_plugin_hyphenation_es::Spanish);
    b = offered(b, store, paperpaw_plugin_hyphenation_fr::French);
    b = offered(b, store, paperpaw_plugin_hyphenation_it::Italian);
    b = offered(b, store, paperpaw_plugin_hyphenation_nb::Norwegian);
    b = offered(b, store, paperpaw_plugin_hyphenation_nl::Dutch);
    b = offered(b, store, paperpaw_plugin_hyphenation_pl::Polish);
    b = offered(b, store, paperpaw_plugin_hyphenation_pt::Portuguese);
    b = offered(b, store, paperpaw_plugin_hyphenation_ru::Russian);
    b = offered(b, store, paperpaw_plugin_hyphenation_sv::Swedish);
    b = offered(b, store, paperpaw_plugin_hyphenation_uk::Ukrainian);
    b
}

/// Adds `plugin`, one the store offers as a file too, as `store` says.
#[cfg(feature = "store-plugins")]
fn offered(
    builder: KernelBuilder,
    store: StorePlugins<'_>,
    plugin: impl paperpaw_core::sdk::Plugin + 'static,
) -> KernelBuilder {
    match store {
        StorePlugins::Left => builder,
        StorePlugins::On => builder.plugin(plugin),
        StorePlugins::Off(on) if on(plugin.manifest().id.as_str()) => builder.plugin(plugin),
        StorePlugins::Off(_) => builder.plugin_off(plugin),
    }
}
