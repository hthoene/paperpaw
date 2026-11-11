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
    install_with(builder, fonts, default_store())
}

/// What [`install`] does with the plugins the store offers as files too: they
/// are built in and on where the feature `store-plugins` builds them, left out
/// otherwise.
#[must_use]
pub const fn default_store() -> StorePlugins<'static> {
    #[cfg(feature = "store-plugins")]
    {
        StorePlugins::On
    }
    #[cfg(not(feature = "store-plugins"))]
    {
        StorePlugins::Left
    }
}

/// The first-party plugins that are interface-7 native plugins already,
/// linked into the program (`native_plugin!(…, linked = NATIVE_ENTRY)`), with
/// their ids: a plugin joins the list when its port is finished and its
/// interface-6 form leaves [`install_with`] (under the same feature). A shell
/// registers each through the kernel (`Kernel::register_native`); until a
/// device activates them it only keeps them linked.
///
/// The plugins the store offers as files too ([`STORE_PLUGINS`]) are in the
/// list only with the feature `native-linked-store`; whether one registers is
/// [`native_registers`]. Not in the list yet, because their ports are not
/// finished (each one's `PORTING-7.md` says what is open) or the interface-6
/// reader still reads books through their interface-6 forms: the formats (txt,
/// xtc, docx, images, mobi), sleep, dictionary, store, fonts-browser,
/// picture-home. Home, reader, news and vocabulary are linked since their
/// settings tables are kept in the program (`fixed_tables`): without them
/// linking costs about 39 KiB of heap (Core's measurement on #1140).
#[cfg(feature = "native-linked")]
pub static NATIVE_PLUGINS: &[(&str, &paperpaw_core::sdk::native::Linked)] = &[
    ("about", &paperpaw_plugin_about::NATIVE_ENTRY),
    #[cfg(feature = "native-linked-store")]
    ("aids", &paperpaw_plugin_aids::NATIVE_ENTRY),
    ("bluetooth", &paperpaw_plugin_bluetooth::NATIVE_ENTRY),
    ("file-manager", &paperpaw_plugin_file_manager::NATIVE_ENTRY),
    ("files", &paperpaw_plugin_files::NATIVE_ENTRY),
    ("help", &paperpaw_plugin_help::NATIVE_ENTRY),
    #[cfg(feature = "native-linked-reading")]
    ("home", &paperpaw_plugin_home::NATIVE_ENTRY),
    #[cfg(feature = "native-linked-reading")]
    ("format-txt", &paperpaw_plugin_format_txt::NATIVE_ENTRY),
    #[cfg(feature = "native-linked-reading")]
    ("format-xtc", &paperpaw_plugin_format_xtc::NATIVE_ENTRY),
    ("keyboard", &paperpaw_plugin_keyboard::NATIVE_ENTRY),
    ("lock", &paperpaw_plugin_lock::NATIVE_ENTRY),
    #[cfg(feature = "native-linked-store")]
    ("mail", &paperpaw_plugin_mail::NATIVE_ENTRY),
    ("menu", &paperpaw_plugin_menu::NATIVE_ENTRY),
    #[cfg(all(feature = "native-linked-reading", feature = "native-linked-store"))]
    ("news", &paperpaw_plugin_news::NATIVE_ENTRY),
    #[cfg(feature = "native-linked-store")]
    ("notes-vault", &paperpaw_plugin_notes_vault::NATIVE_ENTRY),
    ("opds", &paperpaw_plugin_opds::NATIVE_ENTRY),
    (
        "plugin-manager",
        &paperpaw_plugin_plugin_manager::NATIVE_ENTRY,
    ),
    ("power", &paperpaw_plugin_power::NATIVE_ENTRY),
    #[cfg(feature = "native-linked-reading")]
    ("reader", &paperpaw_plugin_reader::NATIVE_ENTRY),
    ("settings", &paperpaw_plugin_settings::NATIVE_ENTRY),
    ("setup", &paperpaw_plugin_setup::NATIVE_ENTRY),
    ("transfer", &paperpaw_plugin_transfer::NATIVE_ENTRY),
    ("update", &paperpaw_plugin_update::NATIVE_ENTRY),
    #[cfg(all(feature = "native-linked-reading", feature = "native-linked-store"))]
    ("vocabulary", &paperpaw_plugin_vocabulary::NATIVE_ENTRY),
    #[cfg(feature = "native-linked-store")]
    ("widgets", &paperpaw_plugin_widgets::NATIVE_ENTRY),
    ("wifi", &paperpaw_plugin_wifi::NATIVE_ENTRY),
];

/// Whether the linked native plugin `id` is linked here and, if it is, on where
/// nothing switched it (the default `Kernel::native_decide` takes): the rule
/// [`install_with`] applies to the plugin's interface-6 form, so that a plugin
/// the store offers keeps the state it ships in.
///
/// `None`: not linked here. A plugin of [`STORE_PLUGINS`] where `store` leaves
/// it out (`Left`: the store installs it, as a file) is neither hosted nor
/// listed, whatever the configuration says: its card form is the plugin, and a
/// second copy of the same id must not run beside it.
///
/// `Some(on)`: hosted and listed; `on` is its shipped state. Shipped off
/// (Beta 3's default, owner's ruling for the flip): Mail always (whatever
/// `store` says is built in), and where `store` is `On` also Notes vault and
/// Picture home (it takes the place of the home screen). Every other plugin
/// is on. The configuration (`[plugins] id = on|off`) is the kernel's to read.
#[cfg(feature = "native-linked")]
pub fn native_default_on(id: &str, store: StorePlugins<'_>) -> Option<bool> {
    if !STORE_PLUGINS.contains(&id) {
        return Some(true);
    }
    match store {
        StorePlugins::Left => None,
        #[cfg(feature = "store-plugins")]
        StorePlugins::On => Some(!matches!(id, "mail" | "notes-vault" | "picture-home")),
        #[cfg(feature = "store-plugins")]
        StorePlugins::Off(on) => Some(id != "mail" && on(id)),
        #[allow(
            unreachable_patterns,
            reason = "`On` and `Off` exist only with the feature `store-plugins`"
        )]
        _ => None,
    }
}

/// The version of the plugin crate `id` (its own `Cargo.toml`, what its plugin
/// file is named by), for every plugin crate this build has.
#[must_use]
pub fn native_version(id: &str) -> Option<&'static str> {
    const VERSIONS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/plugin_versions.rs"));
    VERSIONS
        .iter()
        .find_map(|(plugin, version)| (*plugin == id).then_some(*version))
}

/// The settings and setting groups plugin `id` keeps in the program, as Beta 3
/// did, instead of one copy on the heap for each it declares
/// (`Kernel::register_linked`): the tables its build script makes of its own
/// declarations. Empty for a plugin that has none; one that is not listed here
/// only costs memory.
#[cfg(feature = "native-linked")]
pub fn fixed_tables(id: &str) -> paperpaw_core::native_host::FixedTables {
    use paperpaw_core::native_host::FixedTables;
    macro_rules! tables {
        ($plugin:ident) => {
            FixedTables::new($plugin::NATIVE_SETTINGS, $plugin::NATIVE_GROUPS)
                .with_ids($plugin::NATIVE_IDS)
        };
    }
    match id {
        "about" => tables!(paperpaw_plugin_about),
        "files" => tables!(paperpaw_plugin_files),
        "plugin-manager" => tables!(paperpaw_plugin_plugin_manager),
        "setup" => tables!(paperpaw_plugin_setup),
        "transfer" => tables!(paperpaw_plugin_transfer),
        "update" => tables!(paperpaw_plugin_update),
        "wifi" => tables!(paperpaw_plugin_wifi),
        #[cfg(feature = "native-linked-store")]
        "mail" => tables!(paperpaw_plugin_mail),
        #[cfg(feature = "native-linked-store")]
        "notes-vault" => tables!(paperpaw_plugin_notes_vault),
        "bluetooth" => tables!(paperpaw_plugin_bluetooth),
        "file-manager" => tables!(paperpaw_plugin_file_manager),
        "help" => tables!(paperpaw_plugin_help),
        #[cfg(feature = "native-linked-reading")]
        "home" => tables!(paperpaw_plugin_home),
        "keyboard" => tables!(paperpaw_plugin_keyboard),
        "lock" => tables!(paperpaw_plugin_lock),
        "menu" => tables!(paperpaw_plugin_menu),
        "opds" => tables!(paperpaw_plugin_opds),
        "power" => tables!(paperpaw_plugin_power),
        #[cfg(feature = "native-linked-reading")]
        "reader" => tables!(paperpaw_plugin_reader),
        "settings" => tables!(paperpaw_plugin_settings),
        #[cfg(feature = "native-linked-store")]
        "aids" => tables!(paperpaw_plugin_aids),
        #[cfg(all(feature = "native-linked-reading", feature = "native-linked-store"))]
        "news" => tables!(paperpaw_plugin_news),
        #[cfg(all(feature = "native-linked-reading", feature = "native-linked-store"))]
        "vocabulary" => tables!(paperpaw_plugin_vocabulary),
        #[cfg(feature = "native-linked-store")]
        "widgets" => tables!(paperpaw_plugin_widgets),
        _ => FixedTables::NONE,
    }
}

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
/// that fails later. `image` is the program's read-only data as a mapping that
/// stays for the whole run (null and zero for none): the plugins' declarations
/// are kept as offsets into it, not copied (`Kernel::register_linked`).
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
    default_on: impl Fn(&str) -> Option<bool>,
    image: (*const u8, usize),
) -> alloc::vec::Vec<(
    &'static str,
    Result<(), paperpaw_core::native_host::shell::Error>,
)> {
    use paperpaw_core::native_host::shell::Error;
    use paperpaw_core::sdk::PluginState;
    let config = kernel.native_config();
    let mut outcome = alloc::vec::Vec::new();
    if outcome.try_reserve_exact(NATIVE_PLUGINS.len()).is_err() {
        return outcome;
    }
    add_native_catalogs(kernel);
    for (id, linked) in NATIVE_PLUGINS {
        // Not linked here (a store plugin the store installs as a file): not
        // hosted, not listed, whatever the configuration says.
        let Some(default) = default_on(id) else {
            continue;
        };
        // Once for each plugin: in safe mode a decision saves the file.
        let decision = kernel.native_decide(&config, id, default, false);
        if !decision.hosts() {
            list(
                kernel,
                id,
                (PluginState::Off, decision.enabled(), decision.problem()),
            );
            continue;
        }
        let Some(share) = resources.try_clone() else {
            outcome.push((*id, Err(Error::Allocation)));
            list(kernel, id, (PluginState::Failed, true, None));
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
                image,
                fixed_tables(id),
            )
        };
        let state = if status.is_ok() {
            PluginState::Running
        } else {
            PluginState::Failed
        };
        list(kernel, id, (state, true, None));
        outcome.push((*id, status));
    }
    outcome
}

/// Lists the linked native plugin `id` for the plugin manager, with its state,
/// whether it is switched on and why it is held back (safe mode): its version
/// from its crate, its name the id (the registry shows the plugin's own
/// `<id>.plugin.name` and `.description` from its catalog, in the reader's
/// language, wherever it has them). A listing that fails (the id is listed
/// already) is left alone: the plugin runs or not without it.
#[cfg(feature = "native-linked")]
fn list(
    kernel: &mut paperpaw_core::Kernel,
    id: &'static str,
    (state, enabled, problem): (
        paperpaw_core::sdk::PluginState,
        bool,
        Option<paperpaw_core::sdk::PluginProblem>,
    ),
) {
    use paperpaw_core::sdk::{Id, Manifest, PluginInfo, PluginKind, RequiredFor, Version};
    let Some(version) = native_version(id).and_then(Version::parse) else {
        return;
    };
    let manifest = Manifest::new(Id::new_static(id), id, version);
    let info = PluginInfo::new(
        manifest,
        state,
        enabled,
        RequiredFor::default(),
        PluginKind::BuiltIn,
        problem,
        None,
    );
    let _ = kernel.list_native(info);
}

/// The translation catalogs plugin `id` brings built in, as the texts of its
/// `i18n/<language>.ini` files (a linked native plugin has no code file to
/// carry them: `screens-input.md` [D]). Empty for a plugin with none.
#[cfg(feature = "native-linked")]
#[must_use]
pub fn native_catalogs(id: &str) -> &'static [&'static str] {
    const CATALOGS: &[(&str, &[&str])] = include!(concat!(env!("OUT_DIR"), "/native_catalogs.rs"));
    CATALOGS
        .iter()
        .find_map(|(plugin, catalogs)| (*plugin == id).then_some(*catalogs))
        .unwrap_or(&[])
}

/// Adds the catalogs of every linked plugin of [`NATIVE_PLUGINS`] to the
/// kernel's translator, before the kernel starts (one call for a shell that
/// registers the plugins itself, `register_native_plugins` makes it for its
/// own; a second call adds nothing): without them the texts of a linked plugin (a title, a key's label) are
/// not found. The room for them is asked for once.
#[cfg(feature = "native-linked")]
pub fn add_native_catalogs(kernel: &mut paperpaw_core::Kernel) {
    // Once: a shell that calls it after `register_native_plugins` (which does)
    // adds nothing twice.
    if !kernel.claim_native_catalogs() {
        return;
    }
    let plugins = NATIVE_PLUGINS
        .iter()
        .filter(|(id, _)| !native_catalogs(id).is_empty())
        .count();
    kernel.reserve_catalogs(plugins);
    for (id, _) in NATIVE_PLUGINS {
        for source in native_catalogs(id) {
            kernel.add_catalog(source);
        }
    }
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
    let mut b = builder;
    // The plugins that are native plugins, linked (`NATIVE_PLUGINS`), where the
    // feature says, are not installed here: one plugin, one form.
    #[cfg(not(feature = "native-linked-reading"))]
    {
        b = b
            .plugin(paperpaw_plugin_format_txt::TxtFormat)
            .plugin(paperpaw_plugin_format_xtc::XtcFormat);
    }
    b = b
        .plugin(paperpaw_epub_family::EpubFormat)
        .plugin(paperpaw_epub_family::HtmlFormat)
        .plugin(paperpaw_epub_family::Fb2Format)
        .plugin(paperpaw_epub_family::MarkdownFormat)
        .plugin(paperpaw_epub_family::CbzFormat)
        .plugin(paperpaw_plugin_dictionary::DictionaryPlugin);
    // Reader and Home keep their order; they are native plugins, linked
    // (`NATIVE_PLUGINS`), where the feature says (`native-linked-reading`);
    // Menu where `native-linked` says.
    #[cfg(not(feature = "native-linked-reading"))]
    {
        b = b
            .plugin(paperpaw_plugin_reader::Reader)
            .plugin(paperpaw_plugin_home::Home);
    }
    #[cfg(not(feature = "native-linked"))]
    {
        b = b.plugin(paperpaw_plugin_menu::Menu);
    }
    #[cfg(all(feature = "store-plugins", not(feature = "native-linked-store")))]
    {
        b = offered(b, store, paperpaw_plugin_notes_vault::NotesVault);
    }
    #[cfg(feature = "store-plugins")]
    {
        b = offered_replacing_home(b, store, paperpaw_plugin_picture_home::PictureHome);
    }
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_format_mobi::MobiFormat);
        b = offered(b, store, paperpaw_plugin_format_docx::DocxFormat);
        b = offered(b, store, paperpaw_plugin_format_images::ImagesFormat);
    }
    // Wi-Fi and Transfer are native plugins, linked (`NATIVE_PLUGINS`), where
    // the feature says.
    #[cfg(not(feature = "native-linked"))]
    {
        b = b.plugin(paperpaw_plugin_wifi::Wifi);
        b = b.plugin(paperpaw_plugin_transfer::Transfer);
    }
    #[cfg(feature = "store-plugins")]
    {
        b = offered(b, store, paperpaw_plugin_fonts_browser::FontsBrowser);
    }
    b = b.plugin(paperpaw_plugin_store::Store);
    #[cfg(all(feature = "store-plugins", not(feature = "native-linked-store")))]
    {
        b = offered(b, store, paperpaw_plugin_aids::Aids);
    }
    #[cfg(all(
        feature = "store-plugins",
        not(all(feature = "native-linked-store", feature = "native-linked-reading"))
    ))]
    {
        b = offered(b, store, paperpaw_plugin_vocabulary::Vocabulary);
    }
    b = b.plugin(paperpaw_plugin_sleep::Sleep);
    #[cfg(all(
        feature = "store-plugins",
        not(all(feature = "native-linked-store", feature = "native-linked-reading"))
    ))]
    {
        b = offered(b, store, paperpaw_plugin_news::News);
    }
    #[cfg(all(feature = "store-plugins", not(feature = "native-linked-store")))]
    {
        b = offered(b, store, paperpaw_plugin_widgets::Widgets);
    }
    #[cfg(all(feature = "store-plugins", not(feature = "native-linked-store")))]
    {
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
#[cfg(all(feature = "store-plugins", not(feature = "native-linked-store")))]
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
