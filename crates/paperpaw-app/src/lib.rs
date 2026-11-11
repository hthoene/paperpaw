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
//! builds; it is a native interface-7 plugin and not registered here yet.

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
pub const STORE_PLUGINS: [&str; 11] = [
    "aids",
    "fonts-browser",
    "format-docx",
    "format-images",
    "format-mobi",
    "mail",
    "news",
    "notes-vault",
    "picture-home",
    "vocabulary",
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

/// The first-party plugins that are interface-7 native plugins already,
/// linked into the program (`native_plugin!(…, linked = NATIVE_ENTRY)`), with
/// their ids: the list is what Beta 3 built in, so a plugin joins it when its
/// port lands and its interface-6 form leaves [`install_with`]. A shell
/// registers each through the kernel (`Kernel::register_native`); until a
/// device activates them it only keeps them linked.
#[cfg(feature = "native-linked")]
pub static NATIVE_PLUGINS: [(&str, &paperpaw_core::sdk::native::Linked); 15] = [
    ("about", &paperpaw_plugin_about::NATIVE_ENTRY),
    ("bluetooth", &paperpaw_plugin_bluetooth::NATIVE_ENTRY),
    ("file-manager", &paperpaw_plugin_file_manager::NATIVE_ENTRY),
    ("files", &paperpaw_plugin_files::NATIVE_ENTRY),
    ("help", &paperpaw_plugin_help::NATIVE_ENTRY),
    ("keyboard", &paperpaw_plugin_keyboard::NATIVE_ENTRY),
    ("lock", &paperpaw_plugin_lock::NATIVE_ENTRY),
    ("menu", &paperpaw_plugin_menu::NATIVE_ENTRY),
    ("opds", &paperpaw_plugin_opds::NATIVE_ENTRY),
    (
        "plugin-manager",
        &paperpaw_plugin_plugin_manager::NATIVE_ENTRY,
    ),
    ("power", &paperpaw_plugin_power::NATIVE_ENTRY),
    ("settings", &paperpaw_plugin_settings::NATIVE_ENTRY),
    ("setup", &paperpaw_plugin_setup::NATIVE_ENTRY),
    ("update", &paperpaw_plugin_update::NATIVE_ENTRY),
    ("wifi", &paperpaw_plugin_wifi::NATIVE_ENTRY),
];

/// Registers every plugin of [`NATIVE_PLUGINS`] with `kernel` through
/// `Kernel::register_linked`, before the kernel starts (the one path the X4,
/// the emulator and the tests share). `host` is what the shell offers
/// (its profile, the services it wires, and the consent features it backs: a
/// plugin whose metadata needs a `uses.…` that is not in the host's list is
/// refused at admission); `resources` is
/// the shell's table of handles (each plugin gets a share) with its services
/// wired; `depth` the shell's one dispatch budget; `services` makes one
/// [`Services`](paperpaw_native::host_glue::Services) (heap and clock) per
/// plugin. A plugin the host refuses (a service it does not wire, no memory)
/// is returned with the reason and does not stop the others.
///
/// # Safety
/// Every entry of [`NATIVE_PLUGINS`] comes from `native_plugin!(..., linked =
/// NAME)` of a crate linked into this program, so its factory obeys the SDK's
/// native factory contract and its code and static data stay for the whole
/// run; the caller must not call this with a `host` that offers more than it
/// backs: a service or a consent feature offered and not wired is a plugin
/// that fails later.
#[cfg(feature = "native-linked")]
#[allow(
    clippy::too_many_arguments,
    reason = "the shell supplies what Kernel::register_linked takes, once for every plugin"
)]
pub unsafe fn register_native_plugins<S: paperpaw_native::host_glue::Services + 'static>(
    kernel: &mut paperpaw_core::Kernel,
    host: &paperpaw_native::metadata::Host<'static>,
    resources: &paperpaw_core::native_host::HostResources,
    depth: &paperpaw_core::native_host::flat::FlatDepth,
    mut services: impl FnMut() -> S,
    teardown: paperpaw_core::native_host::shell::Report<S>,
    report: fn(&S, u32, i32),
) -> alloc::vec::Vec<(
    &'static str,
    Result<(), paperpaw_core::native_host::shell::Error>,
)> {
    use paperpaw_core::native_host::shell::Error;
    let mut outcome = alloc::vec::Vec::new();
    if outcome.try_reserve_exact(NATIVE_PLUGINS.len()).is_err() {
        return outcome;
    }
    for (id, linked) in &NATIVE_PLUGINS {
        let Some(share) = resources.try_clone() else {
            outcome.push((*id, Err(Error::Allocation)));
            continue;
        };
        // SAFETY: as this function's contract says.
        let status = unsafe {
            kernel.register_linked(
                linked.entry(),
                linked.metadata(),
                host,
                id,
                services(),
                share,
                depth,
                teardown,
                report,
            )
        };
        outcome.push((*id, status));
    }
    outcome
}

/// The network settings of every networked first-party plugin, kept in
/// the program rather than made on the heap at every start (see
/// [`KernelBuilder::network_settings`]); one missing here only costs
/// memory (the tests say so).
pub static NETWORK_SETTINGS: &[paperpaw_core::NetworkSettings] = paperpaw_core::network_settings!(
    "fonts-browser",
    "mail",
    "news",
    "store",
    "transfer",
    "vocabulary",
    "wifi",
);

/// The indexes of the first-party plugins' translation catalogs, made
/// when the program was built (see `build.rs`): their catalogs keep none
/// on the heap.
pub static CATALOG_INDEXES: &[(u64, &[u16])] =
    include!(concat!(env!("OUT_DIR"), "/catalog_indexes.rs"));

/// The first-party plugins' English catalogs indexed together when the
/// program was built (see `build.rs`): the translator keeps one catalog
/// for all of them.
pub static BUILT_CATALOGS: paperpaw_i18n::BuiltCatalogs =
    include!(concat!(env!("OUT_DIR"), "/built_catalogs.rs"));

/// Adds the first-party plugins to `builder`, those the store offers as
/// files too as `store` says.
pub fn install_with(
    builder: KernelBuilder,
    fonts: Fonts,
    store: StorePlugins<'_>,
) -> KernelBuilder {
    let builder = builder
        .network_settings(NETWORK_SETTINGS)
        .catalog_indexes(CATALOG_INDEXES)
        .built_catalogs(&BUILT_CATALOGS);
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
        .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
        .plugin(paperpaw_plugin_reader::Reader);
    // Home and Menu keep their order; Files and Menu are native plugins,
    // linked (`NATIVE_PLUGINS`), where the feature says.
    b = b.plugin(paperpaw_plugin_home::Home);
    #[cfg(not(feature = "native-linked"))]
    {
        b = b.plugin(paperpaw_plugin_menu::Menu);
    }
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_notes_vault::NotesVault);
        b = offered_replacing_home(b, store, paperpaw_plugin_picture_home::PictureHome);
        b = offered(b, store, paperpaw_plugin_format_mobi::MobiFormat);
        b = offered(b, store, paperpaw_plugin_format_docx::DocxFormat);
        b = offered(b, store, paperpaw_plugin_format_images::ImagesFormat);
    }
    // Wi-Fi is a native plugin, linked (`NATIVE_PLUGINS`), where the feature says.
    #[cfg(not(feature = "native-linked"))]
    {
        b = b.plugin(paperpaw_plugin_wifi::Wifi);
    }
    b = b.plugin(paperpaw_plugin_transfer::Transfer);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_fonts_browser::FontsBrowser);
    }
    b = b.plugin(paperpaw_plugin_store::Store);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_aids::Aids);
        b = offered(b, store, paperpaw_plugin_vocabulary::Vocabulary);
    }
    b = b.plugin(paperpaw_plugin_sleep::Sleep);
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_news::News);
        b = offered(b, store, paperpaw_plugin_widgets::Widgets);
        b = offer_shipped_off(b, store);
    }
    // A theme of the app's own, off as it ships, on every device.
    let builder = b.plugin_off(Pebble);
    #[cfg(not(feature = "native-linked"))]
    let builder = builder
        .plugin(paperpaw_plugin_keyboard::Keyboard)
        .plugin(paperpaw_plugin_settings::SettingsPlugin);
    builder
}

/// Adds the plugins the store offers that ship off, wherever they are
/// built in, as `store` says.
#[cfg(feature = "store-plugins")]
fn offer_shipped_off(mut b: KernelBuilder, store: StorePlugins<'_>) -> KernelBuilder {
    let shipped_off = match store {
        StorePlugins::Left => StorePlugins::Left,
        _ => StorePlugins::Off(&|_| false),
    };
    b = offered(b, shipped_off, paperpaw_plugin_mail::Mail);
    b
}

/// Adds `plugin`, one the store offers as a file too that takes the place of
/// a built-in screen (the home screen): as [`offered`], but switched off
/// where `store` says on for all, so that the tests keep the screen they
/// test unless one switches the plugin on.
#[cfg(feature = "store-plugins")]
fn offered_replacing_home(
    builder: KernelBuilder,
    store: StorePlugins<'_>,
    plugin: impl paperpaw_core::sdk::Plugin + 'static,
) -> KernelBuilder {
    match store {
        StorePlugins::On => builder.plugin_off(plugin),
        other => offered(builder, other, plugin),
    }
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
