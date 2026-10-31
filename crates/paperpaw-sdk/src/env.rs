use core::fmt;

use paperpaw_i18n::Translator;
use paperpaw_platform::{DeviceInfo, Instant, Storage, SystemStatus};

use crate::action::Actions;
use crate::fonts::Fonts;
use crate::formats::Formats;
use crate::hyphenation::Hyphenation;
use crate::menus::Menus;
use crate::plugins::Plugins;
use crate::settings::Settings;
use crate::status::StatusItems;

/// What the device and its shell can do: plugins offer only what works
/// here, and menu entries that need more are left out
/// ([`MenuNeed`](crate::MenuNeed)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// A touch screen.
    pub touch: bool,
    /// The network can be reached: HTTP requests
    /// ([`Command::Http`](crate::Command::Http)), servers and connections.
    /// Not whether it is connected now: where PaperPaw manages Wi-Fi
    /// ([`Env::wifi`]), work waits for a network a while.
    pub network: bool,
    /// PaperPaw manages Wi-Fi itself ([`Env::wifi`]).
    pub wifi: bool,
    /// A Bluetooth Low Energy radio the kernel uses ([`Env::bluetooth`]).
    pub bluetooth: bool,
    /// PaperPaw is the device's firmware and updates it
    /// ([`Env::firmware`]).
    pub firmware: bool,
    /// Text is spoken aloud ([`Command::Speak`](crate::Command::Speak)):
    /// by the system, or a plugin's voice (see [`voices`](crate::voices)).
    pub speech: bool,
    /// PaperPaw shows a screen while the device sleeps
    /// ([`actions::SLEEP`](crate::builtin::actions::SLEEP)); where the
    /// system sleeps on its own (Android), it shows its own.
    pub sleep: bool,
    /// The screen can be turned
    /// ([`settings::ROTATION`](crate::builtin::settings::ROTATION)).
    pub rotation: bool,
    /// The screen is shown in the colours of
    /// [`settings::TEXT_COLOR`](crate::builtin::settings::TEXT_COLOR) and
    /// [`settings::PAPER_COLOR`](crate::builtin::settings::PAPER_COLOR).
    pub colors: bool,
    /// Buttons lie in a row below the screen, with a row of hints above
    /// them ([`settings::BUTTON_HINTS`](crate::builtin::settings::BUTTON_HINTS)).
    pub button_row: bool,
    /// The system has a clipboard of its own, shared with other apps.
    pub system_clipboard: bool,
    /// A front light PaperPaw sets
    /// ([`settings::FRONTLIGHT`](crate::builtin::settings::FRONTLIGHT)).
    pub frontlight: bool,
    /// The panel shows black and white alone (no grays), e.g. the X4.
    pub black_and_white: bool,
    /// PaperPaw switches the device off on its own
    /// ([`Command::PowerOff`](crate::Command::PowerOff)), e.g. the X4; not
    /// where the system decides (Android) or that never switches off (the
    /// emulator).
    pub power_off: bool,
    /// More of what the device can do, by [`Capability`] (new ones come as
    /// constants, without a new plugin interface).
    pub(crate) more: u64,
}

/// What a device can do beyond [`Capabilities`]' fields
/// ([`Capabilities::has`]): constants of one type, so more can come
/// without a new plugin interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Capability(u64);

impl Capability {
    /// Sound can be played.
    pub const AUDIO: Self = Self(1);
    /// The front light's warmth can be set.
    pub const WARMTH: Self = Self(1 << 1);
    /// Charging can be stopped at a limit.
    pub const CHARGE_LIMIT: Self = Self(1 << 2);
    /// A keyboard of keys (not one on the screen) is there.
    pub const KEYBOARD: Self = Self(1 << 3);
    /// Code plugins can mark hot code that runs from internal RAM
    /// (`paperpaw_sdk::hot!`).
    pub const HOT_CODE: Self = Self(1 << 4);
    /// Keys that turn pages (page forward and back, or left and right).
    pub const PAGE_KEYS: Self = Self(1 << 5);
    /// Buttons enough for every screen without touch: *Back*, *Confirm*
    /// and keys to move the focus (up and down, or left and right). Where
    /// a device has page keys alone, screens offer by touch what they
    /// would by buttons.
    pub const BUTTONS: Self = Self(1 << 6);
    /// A USB link to a computer that developer tools may use
    /// ([`developer`](crate::developer)), once the reader allows them.
    pub const USB_LINK: Self = Self(1 << 7);
    /// The kernel calls render hooks ([`render`](crate::render)). Where it
    /// is missing a hook is accepted and never run; the devices say so
    /// from the release that runs them.
    pub const RENDER_HOOK: Self = Self(1 << 8);
    /// The kernel fills [`BandView::regions`](crate::render::BandView::regions)
    /// where it knows what a band holds; where it is missing they are
    /// empty, which means unknown.
    pub const RENDER_REGIONS: Self = Self(1 << 9);
    /// The SDK asks a look for parts of kinds beyond the named ones
    /// ([`Painters::extras`](crate::ui::look::Painters::extras)); where it
    /// is missing none is asked.
    pub const LOOK_EXTRAS: Self = Self(1 << 10);
}

impl Capabilities {
    /// Whether the device can do `capability`.
    pub const fn has(&self, capability: Capability) -> bool {
        self.more & capability.0 == capability.0
    }

    /// With `capability` too (the kernel and shells say so).
    #[cfg(feature = "kernel")]
    #[must_use]
    pub const fn with(mut self, capability: Capability) -> Self {
        self.more |= capability.0;
        self
    }
}

/// Read-only view of the environment a plugin runs in.
///
/// Only the kernel makes one (fields are added in minor versions).
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct Env<'a> {
    pub(crate) device: &'a DeviceInfo,
    /// Time of the event being handled.
    pub(crate) now: Instant,
    pub(crate) metrics: Metrics,
    pub(crate) settings: &'a Settings,
    pub(crate) i18n: &'a Translator,
    pub(crate) fonts: &'a Fonts,
    pub(crate) formats: &'a Formats,
    /// Hyphenation patterns by language.
    pub(crate) hyphenation: &'a Hyphenation,
    pub(crate) menus: &'a Menus,
    /// Every action, e.g. to offer them in a menu editor.
    pub(crate) actions: &'a Actions,
    /// Every plugin with its state, for plugin managers.
    pub(crate) plugins: &'a Plugins,
    /// The time of day and the battery, as far as the device knows them
    /// (see [`time`](crate::time) for the local time).
    pub(crate) status: &'a SystemStatus,
    /// What the device did since PaperPaw started: refreshes, input (see
    /// [`usage`](crate::usage)); what each plugin took is
    /// [`Env::plugin_usage`].
    pub(crate) usage: &'a crate::usage::Usage,
    /// What each plugin took, kept by the kernel as it goes.
    plugin_usage: &'a core::cell::RefCell<alloc::vec::Vec<crate::usage::PluginUsage>>,
    /// Items for status lines, from every plugin.
    pub(crate) status_items: &'a StatusItems,
    /// Filters of book text before it is laid out, from every plugin.
    pub(crate) text_filters: &'a crate::filters::TextFilters,
    /// Marks on the words of book pages, from every plugin (see
    /// [`decorations`](crate::decorations)).
    pub(crate) decorators: &'a crate::decorations::Decorators,
    /// Layers on book pages and frames of their text, from every plugin
    /// (see [`layers`](crate::layers)).
    pub(crate) page_layers: &'a crate::layers::PageLayers,
    /// Where words are looked up, from every plugin (see
    /// [`lookup`](crate::lookup)).
    pub(crate) lookup: &'a crate::lookup::LookupProviders,
    /// The reading apps, from every plugin (see
    /// [`readers`](crate::readers)).
    pub(crate) readers: &'a crate::readers::Readers,
    /// Plugins' stylesheets for books, in the order of the plugins (see
    /// [`Registrar::add_stylesheet`](crate::Registrar::add_stylesheet)).
    pub(crate) stylesheets: &'a [alloc::borrow::Cow<'static, str>],
    /// Folders of what plugins keep for each book
    /// ([`Registrar::add_book_data`](crate::Registrar::add_book_data)).
    pub(crate) book_data: &'a [crate::library::BookData],
    /// Fields plugins keep with books
    /// ([`Registrar::add_book_field`](crate::Registrar::add_book_field)).
    pub(crate) book_fields: &'a [crate::library::BookField],
    /// Badges plugins put on books' rows
    /// ([`Registrar::add_book_badge`](crate::Registrar::add_book_badge)).
    pub(crate) book_badges: &'a [crate::library::BookBadge],
    /// Widgets for screens that show them, from every plugin.
    pub(crate) widgets: &'a crate::widgets::Widgets,
    /// The steps of the first-run setup, from every plugin (see
    /// [`setup`](crate::setup)).
    pub(crate) setup: &'a crate::setup::SetupSteps,
    /// What plugins offer each other (see [`services`](crate::services)).
    pub(crate) services: &'a crate::services::Services,
    /// What buttons, taps and swipes do (see [`keys`](crate::keys)).
    pub(crate) keys: &'a dyn crate::keys::KeyBindings,
    /// The look a plugin gave the parts of screens, set with
    /// [`Registrar::set_ui_look`](crate::Registrar::set_ui_look) (see
    /// [`ui::look`](crate::ui::look)).
    pub(crate) look: crate::ui::look::Look,
    /// What the device and its shell can do (network, speech, the sleep
    /// screen …).
    pub(crate) caps: Capabilities,
    /// Wi-Fi, where PaperPaw manages it itself (see
    /// [`wifi`](crate::wifi)); `None` where the system does.
    pub(crate) wifi: Option<&'a crate::wifi::Wifi>,
    /// Bluetooth, where the kernel uses a radio (see
    /// [`bluetooth`](crate::bluetooth)).
    pub(crate) bluetooth: Option<crate::bluetooth::BluetoothView<'a>>,
    /// Firmware updates, where PaperPaw is the device's firmware (see
    /// [`firmware`](crate::firmware)); `None` where it is an app.
    pub(crate) firmware: Option<&'a crate::firmware::Firmware>,
    /// How native code plugins are loaded here, e.g. which builds fit
    /// (see [`native`](crate::native)); `None` where PaperPaw loads none.
    pub(crate) native: Option<&'a crate::native::Native>,
    /// The power level and mode in use (see [`power`](crate::power)).
    pub(crate) power: &'a crate::power::Power,
    /// Where declared work stands, and plugins' notes (see
    /// [`work`](crate::work)).
    pub(crate) work: &'a crate::work::WorkStates,
    /// What is handled now is the reader's own doing (input, a screen or
    /// an action they opened or ran, the answer to their request): act at
    /// once. Otherwise it is background work (an event, a rule's action,
    /// declared work), which may wait for a good moment (see
    /// [`work`](crate::work)) and says nothing over reading.
    pub(crate) reader: bool,
    /// Whether there is text on the clipboard, copied here
    /// ([`Command::SetClipboard`](crate::Command::SetClipboard)) or, where
    /// the system has a clipboard, in another app; read it with
    /// [`Command::ReadClipboard`](crate::Command::ReadClipboard).
    pub(crate) clipboard: bool,
    /// What the reader used last: buttons, a finger or a pen, e.g. to
    /// show a focus only after buttons were used, also on touch devices.
    pub(crate) last_input: crate::keys::InputMode,
    /// The inputs plugins added ([`keys::InputSpec`](crate::keys::InputSpec)),
    /// to bind like any other.
    pub(crate) inputs: &'a [crate::keys::InputSpec],
    /// Makes the screens plugins added, for [`Env::make_screen`]: the
    /// kernel's ([`Env::with_screen_maker`]). Never behind a feature: a
    /// plugin built without `kernel` reads the `Env` the kernel made.
    screen_maker: Option<&'a dyn crate::screen::ScreenMaker>,
    /// The part of the screen what is handled now is made for and drawn
    /// into, with its place on the screen: all of it but bars and the row
    /// of hints for a screen, a bar's strip for a bar. A screen draws and
    /// gets input relative to its corner, and
    /// [`device.display`](paperpaw_platform::DeviceInfo::display) has its
    /// size.
    pub(crate) content: paperpaw_platform::Rect,
    /// The open screens, the one shown last (see [`StackEntry`](crate::StackEntry)):
    /// what is shown, e.g. for a rule or a refresh filter.
    pub(crate) stack: &'a [crate::StackEntry],
    /// The slots some screen is bound to (see [`Env::has_slot`]).
    pub(crate) slots: &'a [crate::Id],
    /// Every screen: its id and the translation key of its name, e.g. for
    /// settings that differ per screen (what gestures do there).
    pub(crate) screens: crate::screen::ScreenList<'a>,
    /// The folders reading fonts are found in at start-up, the first one
    /// PaperPaw's own (where new fonts go); empty where the device has
    /// only its built-in fonts.
    pub(crate) font_dirs: &'a [alloc::string::String],
    /// What plugin libraries call this kind of device, e.g. `x4`, `x4-pro`
    /// or `android` for paperpaw.org's (ADR 0023); `None` where the shell
    /// names none, and a store has nothing made for it.
    pub(crate) catalog_device: Option<&'a str>,
    /// Passwords and keys, kept for every plugin in one place (see
    /// [`secrets`](crate::secrets)).
    pub(crate) secrets: crate::secrets::Visible<'a>,
    /// Random bytes from the platform's source, for keys and tokens (see
    /// [`random`](crate::random)).
    pub(crate) random: crate::random::Random,
    /// How much memory is free now, where the shell can tell (see
    /// [`memory`](crate::memory)).
    pub(crate) memory: crate::memory::Memory,
    /// A monotonic clock in microseconds, for measuring how long work
    /// takes (see [`time::Clock`](crate::time::Clock)).
    pub(crate) clock: crate::time::Clock,
    /// How PaperPaw started this time, where the shell can tell (see
    /// [`power::Start`](crate::power::Start)).
    pub(crate) started: crate::power::Start,
    /// How long the device slept last (see [`Env::slept_for`]).
    pub(crate) slept_for: Option<core::time::Duration>,
    /// What the panel can do and how the last frame went (see
    /// [`realtime`](crate::realtime)).
    pub(crate) panel: crate::realtime::PanelInfo,
    /// The buttons held down now, in the order they went down.
    pub(crate) keys_held: &'a [&'static str],
    /// The room the screen on top asked for and got (see
    /// [`realtime::Room`](crate::realtime::Room)).
    pub(crate) room: crate::realtime::Room,
    /// The device's storage. Plugins keep their own files below
    /// [`Env::plugin_dir`].
    pub(crate) storage: &'a dyn Storage,
}

/// The environment's parts, read through methods: what PaperPaw adds
/// later reaches plugins without a change to how they read it.
impl<'a> Env<'a> {
    /// Its `device`.
    pub fn device(&self) -> &'a DeviceInfo {
        self.device
    }

    /// Time of the event being handled.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// Its `metrics`.
    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    /// Its `settings`.
    pub fn settings(&self) -> &'a Settings {
        self.settings
    }

    /// Its `i18n`.
    pub fn i18n(&self) -> &'a Translator {
        self.i18n
    }

    /// The direction of the interface: right to left where the catalog
    /// of its language says so ([`Translator::direction`]).
    pub fn direction(&self) -> paperpaw_lang::Direction {
        self.i18n.direction()
    }

    /// The language of the interface, e.g. `de` or `ar`.
    pub fn lang(&self) -> paperpaw_lang::Lang {
        paperpaw_lang::Lang::parse(self.i18n.language())
    }

    /// Its `fonts`.
    pub fn fonts(&self) -> &'a Fonts {
        self.fonts
    }

    /// Its `formats`.
    pub fn formats(&self) -> &'a Formats {
        self.formats
    }

    /// Hyphenation patterns by language.
    pub fn hyphenation(&self) -> &'a Hyphenation {
        self.hyphenation
    }

    /// Its menus, labelled by its actions and settings.
    pub fn menus(&self) -> crate::menus::MenuView<'a> {
        crate::menus::MenuView::new(self.menus, self.actions, self.settings)
    }

    /// Every action, e.g. to offer them in a menu editor.
    pub fn actions(&self) -> &'a Actions {
        self.actions
    }

    /// Every plugin with its state, for plugin managers.
    pub fn plugins(&self) -> &'a Plugins {
        self.plugins
    }

    /// The time of day and the battery, as far as the device knows them
    /// (see [`time`](crate::time) for the local time).
    pub fn status(&self) -> &'a SystemStatus {
        self.status
    }

    /// What the device did since PaperPaw started: refreshes, input (see
    /// [`usage`](crate::usage)); what each plugin took is
    /// [`Env::plugin_usage`].
    pub fn usage(&self) -> &'a crate::usage::Usage {
        self.usage
    }

    /// Items for status lines, from every plugin.
    pub fn status_items(&self) -> &'a StatusItems {
        self.status_items
    }

    /// Filters of book text before it is laid out, from every plugin.
    pub fn text_filters(&self) -> &'a crate::filters::TextFilters {
        self.text_filters
    }

    /// Marks on the words of book pages, from every plugin (see
    /// [`decorations`](crate::decorations)).
    pub fn decorators(&self) -> &'a crate::decorations::Decorators {
        self.decorators
    }

    /// Layers on book pages and frames of their text, from every plugin
    /// (see [`layers`](crate::layers)).
    pub fn page_layers(&self) -> &'a crate::layers::PageLayers {
        self.page_layers
    }

    /// Where words are looked up, from every plugin (see
    /// [`lookup`](crate::lookup)).
    pub fn lookup(&self) -> &'a crate::lookup::LookupProviders {
        self.lookup
    }

    /// The reading apps, from every plugin (see
    /// [`readers`](crate::readers)).
    pub fn readers(&self) -> &'a crate::readers::Readers {
        self.readers
    }

    /// Plugins' stylesheets for books, in the order of the plugins (see
    /// [`Registrar::add_stylesheet`](crate::Registrar::add_stylesheet)).
    pub fn stylesheets(&self) -> &'a [alloc::borrow::Cow<'static, str>] {
        self.stylesheets
    }

    /// Folders of what plugins keep for each book
    /// ([`Registrar::add_book_data`](crate::Registrar::add_book_data)).
    pub fn book_data(&self) -> &'a [crate::library::BookData] {
        self.book_data
    }

    /// Fields plugins keep with books
    /// ([`Registrar::add_book_field`](crate::Registrar::add_book_field)).
    pub fn book_fields(&self) -> &'a [crate::library::BookField] {
        self.book_fields
    }

    /// Badges plugins put on books' rows
    /// ([`Registrar::add_book_badge`](crate::Registrar::add_book_badge)).
    pub fn book_badges(&self) -> &'a [crate::library::BookBadge] {
        self.book_badges
    }

    /// Widgets for screens that show them, from every plugin.
    pub fn widgets(&self) -> &'a crate::widgets::Widgets {
        self.widgets
    }

    /// The steps of the first-run setup, from every plugin (see
    /// [`setup`](crate::setup)).
    pub fn setup(&self) -> &'a crate::setup::SetupSteps {
        self.setup
    }

    /// What plugins offer each other (see [`services`](crate::services)).
    pub fn services(&self) -> &'a crate::services::Services {
        self.services
    }

    /// What buttons, taps and swipes do (see [`keys`](crate::keys)).
    pub fn keys(&self) -> &'a dyn crate::keys::KeyBindings {
        self.keys
    }

    /// The look a plugin gave the parts of screens, set with
    /// [`Registrar::set_ui_look`](crate::Registrar::set_ui_look) (see
    /// [`ui::look`](crate::ui::look)).
    pub fn look(&self) -> crate::ui::look::Look {
        self.look
    }

    /// What the device and its shell can do (network, speech, the sleep
    /// screen …).
    pub fn caps(&self) -> Capabilities {
        self.caps
    }

    /// Wi-Fi, where PaperPaw manages it itself (see
    /// [`wifi`](crate::wifi)); `None` where the system does.
    pub fn wifi(&self) -> Option<&'a crate::wifi::Wifi> {
        self.wifi
    }

    /// Bluetooth, where the kernel uses a radio (see
    /// [`bluetooth`](crate::bluetooth)); `None` elsewhere.
    pub fn bluetooth(&self) -> Option<crate::bluetooth::BluetoothView<'a>> {
        self.bluetooth
    }

    /// Firmware updates, where PaperPaw is the device's firmware (see
    /// [`firmware`](crate::firmware)); `None` where it is an app.
    pub fn firmware(&self) -> Option<&'a crate::firmware::Firmware> {
        self.firmware
    }

    /// How native code plugins are loaded here, e.g. which builds fit
    /// (see [`native`](crate::native)); `None` where PaperPaw loads none.
    pub fn native(&self) -> Option<&'a crate::native::Native> {
        self.native
    }

    /// The power level and mode in use (see [`power`](crate::power)).
    pub fn power(&self) -> &'a crate::power::Power {
        self.power
    }

    /// Where declared work stands, and plugins' notes (see
    /// [`work`](crate::work)).
    pub fn work(&self) -> &'a crate::work::WorkStates {
        self.work
    }

    /// What is handled now is the reader's own doing (input, a screen or
    /// an action they opened or ran, the answer to their request): act at
    /// once. Otherwise it is background work (an event, a rule's action,
    /// declared work), which may wait for a good moment (see
    /// [`work`](crate::work)) and says nothing over reading.
    pub fn reader(&self) -> bool {
        self.reader
    }

    /// Whether there is text on the clipboard, copied here
    /// ([`Command::SetClipboard`](crate::Command::SetClipboard)) or, where
    /// the system has a clipboard, in another app; read it with
    /// [`Command::ReadClipboard`](crate::Command::ReadClipboard).
    pub fn clipboard(&self) -> bool {
        self.clipboard
    }

    /// What the reader used last: buttons, a finger or a pen, e.g. to
    /// show a focus only after buttons were used, also on touch devices.
    pub fn last_input(&self) -> crate::keys::InputMode {
        self.last_input
    }

    /// The inputs plugins added ([`keys::InputSpec`](crate::keys::InputSpec)),
    /// to bind like any other.
    pub fn inputs(&self) -> &'a [crate::keys::InputSpec] {
        self.inputs
    }

    /// The part of the screen what is handled now is made for and drawn
    /// into, with its place on the screen: all of it but bars and the row
    /// of hints for a screen, a bar's strip for a bar. A screen draws and
    /// gets input relative to its corner, and
    /// [`device.display`](paperpaw_platform::DeviceInfo::display) has its
    /// size.
    pub fn content(&self) -> paperpaw_platform::Rect {
        self.content
    }

    /// The open screens, the one shown last (see [`StackEntry`](crate::StackEntry)):
    /// what is shown, e.g. for a rule or a refresh filter.
    pub fn stack(&self) -> &'a [crate::StackEntry] {
        self.stack
    }

    /// The slots some screen is bound to (see [`Env::has_slot`]).
    pub fn slots(&self) -> &'a [crate::Id] {
        self.slots
    }

    /// Every screen: its id and the translation key of its name, e.g. for
    /// settings that differ per screen (what gestures do there).
    pub fn screens(&self) -> crate::screen::ScreenList<'a> {
        self.screens
    }

    /// The folders reading fonts are found in at start-up, the first one
    /// PaperPaw's own (where new fonts go); empty where the device has
    /// only its built-in fonts.
    pub fn font_dirs(&self) -> &'a [alloc::string::String] {
        self.font_dirs
    }

    /// What plugin libraries call this kind of device, e.g. `x4`, `x4-pro`
    /// or `android` for paperpaw.org's (ADR 0023); `None` where the shell
    /// names none, and a store has nothing made for it.
    pub fn catalog_device(&self) -> Option<&'a str> {
        self.catalog_device
    }

    /// Passwords and keys, kept for every plugin in one place (see
    /// [`secrets`](crate::secrets)).
    pub fn secrets(&self) -> crate::secrets::Visible<'a> {
        self.secrets
    }

    /// Random bytes from the platform's source, for keys and tokens (see
    /// [`random`](crate::random)).
    pub fn random(&self) -> crate::random::Random {
        self.random
    }

    /// How much memory is free now, where the shell can tell (see
    /// [`memory`](crate::memory)).
    pub fn memory(&self) -> crate::memory::Memory {
        self.memory
    }

    /// A monotonic clock in microseconds, for measuring how long work
    /// takes (see [`time::Clock`](crate::time::Clock)).
    pub fn clock(&self) -> crate::time::Clock {
        self.clock
    }

    /// How PaperPaw started this time, where the shell can tell (see
    /// [`power::Start`](crate::power::Start)).
    pub fn started(&self) -> crate::power::Start {
        self.started
    }

    /// How long the device slept before it woke last, where the shell can
    /// tell: after [`events::RESUME`](crate::builtin::events::RESUME), and
    /// after a start that ends a sleep in which the device was switched
    /// off, if its clock ran on. `None` before the first sleep and where
    /// it is not known: a device without a clock that keeps time switched
    /// off (the X4) cannot tell; a plugin that needs it keeps a stamp of
    /// its own. Where the clock is the chip's slow RTC clock (the X4's light
    /// sleep), expect an error of a few percent (1-5 %): right for "slept
    /// for two hours", not for a stopwatch.
    pub fn slept_for(&self) -> Option<core::time::Duration> {
        self.slept_for
    }

    /// What the panel can do, and how the last frame went (see
    /// [`realtime`](crate::realtime)): for screens that pace themselves.
    pub fn panel(&self) -> crate::realtime::PanelInfo {
        self.panel
    }

    /// The buttons held down now: for screens that take raw keys
    /// ([`ScreenMode::raw_keys`](crate::realtime::ScreenMode::raw_keys)),
    /// also for the others.
    pub fn keys_held(&self) -> crate::realtime::KeysHeld<'a> {
        crate::realtime::KeysHeld::of(self.keys_held)
    }

    /// The memory the screen on top asked for
    /// ([`ScreenMode::memory`](crate::realtime::ScreenMode::memory)) and
    /// what it got.
    pub fn room(&self) -> crate::realtime::Room {
        self.room
    }

    /// The device's storage. Plugins keep their own files below
    /// [`Env::plugin_dir`].
    pub fn storage(&self) -> &'a dyn Storage {
        self.storage
    }
}

/// The fields of an [`Env`], for the kernel to make one (plugins get it
/// made). Named outside the SDK with the `kernel` feature only.
#[allow(
    missing_debug_implementations,
    reason = "the parts an environment is made of, for the kernel alone; their references print nothing useful"
)]
pub struct EnvParts<'a> {
    pub device: &'a DeviceInfo,
    /// Time of the event being handled.
    pub now: Instant,
    pub metrics: Metrics,
    pub settings: &'a Settings,
    pub i18n: &'a Translator,
    pub fonts: &'a Fonts,
    pub formats: &'a Formats,
    /// Hyphenation patterns by language.
    pub hyphenation: &'a Hyphenation,
    pub menus: &'a Menus,
    /// Every action, e.g. to offer them in a menu editor.
    pub actions: &'a Actions,
    /// Every plugin with its state, for plugin managers.
    pub plugins: &'a Plugins,
    /// The time of day and the battery, as far as the device knows them
    /// (see [`time`](crate::time) for the local time).
    pub status: &'a SystemStatus,
    /// What the device did since PaperPaw started: refreshes, input (see
    /// [`usage`](crate::usage)).
    pub usage: &'a crate::usage::Usage,
    /// What each plugin took, kept by the kernel as it goes (see
    /// [`Env::plugin_usage`]).
    pub plugin_usage: &'a core::cell::RefCell<alloc::vec::Vec<crate::usage::PluginUsage>>,
    /// Items for status lines, from every plugin.
    pub status_items: &'a StatusItems,
    /// Filters of book text before it is laid out, from every plugin.
    pub text_filters: &'a crate::filters::TextFilters,
    /// Marks on the words of book pages, from every plugin (see
    /// [`decorations`](crate::decorations)).
    pub decorators: &'a crate::decorations::Decorators,
    /// Layers on book pages and frames of their text, from every plugin
    /// (see [`layers`](crate::layers)).
    pub page_layers: &'a crate::layers::PageLayers,
    /// Where words are looked up, from every plugin (see
    /// [`lookup`](crate::lookup)).
    pub lookup: &'a crate::lookup::LookupProviders,
    /// The reading apps, from every plugin (see
    /// [`readers`](crate::readers)).
    pub readers: &'a crate::readers::Readers,
    /// Plugins' stylesheets for books, in the order of the plugins (see
    /// [`Registrar::add_stylesheet`](crate::Registrar::add_stylesheet)).
    pub stylesheets: &'a [alloc::borrow::Cow<'static, str>],
    /// Folders of what plugins keep for each book
    /// ([`Registrar::add_book_data`](crate::Registrar::add_book_data)).
    pub book_data: &'a [crate::library::BookData],
    /// Fields plugins keep with books
    /// ([`Registrar::add_book_field`](crate::Registrar::add_book_field)).
    pub book_fields: &'a [crate::library::BookField],
    /// Badges plugins put on books' rows
    /// ([`Registrar::add_book_badge`](crate::Registrar::add_book_badge)).
    pub book_badges: &'a [crate::library::BookBadge],
    /// Widgets for screens that show them, from every plugin.
    pub widgets: &'a crate::widgets::Widgets,
    /// The steps of the first-run setup, from every plugin (see
    /// [`setup`](crate::setup)).
    pub setup: &'a crate::setup::SetupSteps,
    /// What plugins offer each other (see [`services`](crate::services)).
    pub services: &'a crate::services::Services,
    /// What buttons, taps and swipes do (see [`keys`](crate::keys)).
    pub keys: &'a dyn crate::keys::KeyBindings,
    /// The look a plugin gave the parts of screens, set with
    /// [`Registrar::set_ui_look`](crate::Registrar::set_ui_look) (see
    /// [`ui::look`](crate::ui::look)).
    pub look: crate::ui::look::Look,
    /// What the device and its shell can do (network, speech, the sleep
    /// screen …).
    pub caps: Capabilities,
    /// Wi-Fi, where PaperPaw manages it itself (see
    /// [`wifi`](crate::wifi)); `None` where the system does.
    pub wifi: Option<&'a crate::wifi::Wifi>,
    /// Bluetooth, where the kernel uses a radio (see
    /// [`bluetooth`](crate::bluetooth)).
    pub bluetooth: Option<&'a crate::bluetooth::Bluetooth>,
    /// Firmware updates, where PaperPaw is the device's firmware (see
    /// [`firmware`](crate::firmware)); `None` where it is an app.
    pub firmware: Option<&'a crate::firmware::Firmware>,
    /// How native code plugins are loaded here, e.g. which builds fit
    /// (see [`native`](crate::native)); `None` where PaperPaw loads none.
    pub native: Option<&'a crate::native::Native>,
    /// The power level and mode in use (see [`power`](crate::power)).
    pub power: &'a crate::power::Power,
    /// Where declared work stands, and plugins' notes (see
    /// [`work`](crate::work)).
    pub work: &'a crate::work::WorkStates,
    /// What is handled now is the reader's own doing (input, a screen or
    /// an action they opened or ran, the answer to their request): act at
    /// once. Otherwise it is background work (an event, a rule's action,
    /// declared work), which may wait for a good moment (see
    /// [`work`](crate::work)) and says nothing over reading.
    pub reader: bool,
    /// Whether there is text on the clipboard, copied here
    /// ([`Command::SetClipboard`](crate::Command::SetClipboard)) or, where
    /// the system has a clipboard, in another app; read it with
    /// [`Command::ReadClipboard`](crate::Command::ReadClipboard).
    pub clipboard: bool,
    /// What the reader used last: buttons, a finger or a pen, e.g. to
    /// show a focus only after buttons were used, also on touch devices.
    pub last_input: crate::keys::InputMode,
    /// The inputs plugins added ([`keys::InputSpec`](crate::keys::InputSpec)),
    /// to bind like any other.
    pub inputs: &'a [crate::keys::InputSpec],
    /// The part of the screen what is handled now is made for and drawn
    /// into, with its place on the screen: all of it but bars and the row
    /// of hints for a screen, a bar's strip for a bar. A screen draws and
    /// gets input relative to its corner, and
    /// [`device.display`](paperpaw_platform::DeviceInfo::display) has its
    /// size.
    pub content: paperpaw_platform::Rect,
    /// The open screens, the one shown last (see [`StackEntry`](crate::StackEntry)):
    /// what is shown, e.g. for a rule or a refresh filter.
    pub stack: &'a [crate::StackEntry],
    /// The slots some screen is bound to (see [`Env::has_slot`]).
    pub slots: &'a [crate::Id],
    /// Every screen: its id and the translation key of its name, e.g. for
    /// settings that differ per screen (what gestures do there).
    pub screens: crate::screen::ScreenList<'a>,
    /// The folders reading fonts are found in at start-up, the first one
    /// PaperPaw's own (where new fonts go); empty where the device has
    /// only its built-in fonts.
    pub font_dirs: &'a [alloc::string::String],
    /// What plugin libraries call this kind of device, e.g. `x4`, `x4-pro`
    /// or `android` for paperpaw.org's (ADR 0023); `None` where the shell
    /// names none, and a store has nothing made for it.
    pub catalog_device: Option<&'a str>,
    /// Passwords and keys, kept for every plugin in one place (see
    /// [`secrets`](crate::secrets)).
    pub secrets: &'a crate::secrets::Secrets,
    /// Which plugin's code runs, for [`Env::secrets`] (see
    /// [`secrets::Running`](crate::secrets::Running)).
    pub running: &'a dyn crate::secrets::Running,
    /// Random bytes from the platform's source, for keys and tokens (see
    /// [`random`](crate::random)).
    pub random: crate::random::Random,
    /// How much memory is free now, where the shell can tell (see
    /// [`memory`](crate::memory)).
    pub memory: crate::memory::Memory,
    /// A monotonic clock in microseconds, for measuring how long work
    /// takes (see [`time::Clock`](crate::time::Clock)).
    pub clock: crate::time::Clock,
    /// How PaperPaw started this time, where the shell can tell (see
    /// [`power::Start`](crate::power::Start)).
    pub started: crate::power::Start,
    /// How long the device slept last, where the shell can tell.
    pub slept_for: Option<core::time::Duration>,
    /// What the panel can do and how the last frame went (see
    /// [`realtime`](crate::realtime)).
    pub panel: crate::realtime::PanelInfo,
    /// The buttons held down now, in the order they went down.
    pub keys_held: &'a [&'static str],
    /// The room the screen on top asked for and got (see
    /// [`realtime::Room`](crate::realtime::Room)).
    pub room: crate::realtime::Room,
    /// The device's storage. Plugins keep their own files below
    /// [`Env::plugin_dir`].
    pub storage: &'a dyn Storage,
}

impl<'a> From<EnvParts<'a>> for Env<'a> {
    fn from(p: EnvParts<'a>) -> Self {
        Self {
            device: p.device,
            now: p.now,
            metrics: p.metrics,
            settings: p.settings,
            i18n: p.i18n,
            fonts: p.fonts,
            formats: p.formats,
            hyphenation: p.hyphenation,
            menus: p.menus,
            actions: p.actions,
            plugins: p.plugins,
            status: p.status,
            usage: p.usage,
            plugin_usage: p.plugin_usage,
            status_items: p.status_items,
            text_filters: p.text_filters,
            decorators: p.decorators,
            page_layers: p.page_layers,
            lookup: p.lookup,
            readers: p.readers,
            stylesheets: p.stylesheets,
            book_data: p.book_data,
            book_fields: p.book_fields,
            book_badges: p.book_badges,
            widgets: p.widgets,
            setup: p.setup,
            services: p.services,
            keys: p.keys,
            look: p.look,
            caps: p.caps,
            wifi: p.wifi,
            bluetooth: p
                .bluetooth
                .map(|all| crate::bluetooth::BluetoothView::new(all, p.running)),
            firmware: p.firmware,
            native: p.native,
            power: p.power,
            work: p.work,
            reader: p.reader,
            clipboard: p.clipboard,
            last_input: p.last_input,
            inputs: p.inputs,
            screen_maker: None,
            content: p.content,
            stack: p.stack,
            slots: p.slots,
            screens: p.screens,
            font_dirs: p.font_dirs,
            catalog_device: p.catalog_device,
            secrets: crate::secrets::Visible::new(p.secrets, p.running),
            random: p.random,
            memory: p.memory,
            clock: p.clock,
            started: p.started,
            slept_for: p.slept_for,
            panel: p.panel,
            keys_held: p.keys_held,
            room: p.room,
            storage: p.storage,
        }
    }
}

impl fmt::Debug for Env<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env")
            .field("device", &self.device.id)
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

impl<'a> Env<'a> {
    /// The device's name as others see it, the one the reader set
    /// ([`device_name::SETTING`](crate::device_name::SETTING)) or its
    /// model's: what a plugin tells a server or a computer, never one it
    /// makes itself (see [`device_name`](crate::device_name)).
    pub fn device_name(&self) -> alloc::string::String {
        let set = self.settings.read(&crate::device_name::SETTING);
        crate::device_name::chosen(set, self.device)
    }

    /// The device as a screen made for `area` sees it: the display as
    /// large as `area` (see [`Env::within`]).
    pub fn device_within(&self, area: paperpaw_platform::Rect) -> paperpaw_platform::DeviceInfo {
        let mut device = *self.device;
        device.display.width = area.width;
        device.display.height = area.height;
        device
    }

    /// This environment for a screen made for `area` of the screen (a
    /// tab of a screen that hosts others, a bar): `device` is
    /// [`Env::device_within`]`(area)`, and [`Env::content`] is `area`.
    pub fn within<'b>(
        &self,
        device: &'b paperpaw_platform::DeviceInfo,
        area: paperpaw_platform::Rect,
    ) -> Env<'b>
    where
        'a: 'b,
    {
        let mut env: Env<'b> = *self;
        env.device = device;
        env.content = area;
        env
    }

    /// The screen slot `slot` shows now: the user's choice, else the
    /// plugins' of the highest priority.
    pub fn slot_screen(&self, slot: &str) -> Option<&'a crate::Id> {
        self.screen_maker?.slot_screen(slot)
    }

    /// Every screen offered for slot `slot` (to choose another, e.g. a
    /// launcher of several): the highest priority first, those of one
    /// priority in the order offered. Where the user chose none, those
    /// before [`Env::slot_screen`] bind the slot as high as it, those
    /// after it lower (a fallback).
    pub fn slot_screens(&self, slot: &str) -> alloc::vec::Vec<&'a crate::Id> {
        self.screen_maker
            .map_or_else(alloc::vec::Vec::new, |maker| maker.slot_screens(slot))
    }

    /// The screen the user chose for slot `slot` (`[slots]`, see
    /// [`Command::BindSlot`](crate::Command::BindSlot)), where it is
    /// there; `None` for automatic, the plugins' choice.
    pub fn slot_choice(&self, slot: &str) -> Option<&'a crate::Id> {
        self.screen_maker?.slot_choice(slot)
    }

    /// The translation key of slot `slot`'s name (e.g. "Home screen").
    pub fn slot_title(&self, slot: &str) -> Option<&'a str> {
        self.screen_maker?.slot_title(slot)
    }

    /// The translation key of screen `screen`'s name.
    pub fn screen_title(&self, screen: &crate::Id) -> Option<&'a str> {
        self.screen_maker?.screen_title(screen)
    }

    /// This environment with `maker` making the screens
    /// [`Env::make_screen`] asks for: the kernel's.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn with_screen_maker(mut self, maker: &'a dyn crate::screen::ScreenMaker) -> Self {
        self.screen_maker = Some(maker);
        self
    }
}

impl Env<'_> {
    /// Screen `id`, made with `params` for this environment, to host it in
    /// a screen of one's own (tabs, a split view; see
    /// [`host::ScreenHost`](crate::host::ScreenHost)): made for
    /// [`Env::content`] as it is here, so make it with the environment
    /// [`Env::within`] the part it gets. `None` where no plugin added it.
    pub fn make_screen(
        &self,
        id: &crate::Id,
        params: &crate::Params,
    ) -> Option<alloc::boxed::Box<dyn crate::Screen>> {
        self.screen_maker?.make(id, self, params)
    }

    /// The screen shown on top, if any.
    pub fn top(&self) -> Option<&crate::StackEntry> {
        self.stack.last()
    }

    /// Whether a screen is bound to `slot`, e.g. whether a browser is
    /// there to open links to the web
    /// ([`slots::BROWSER`](crate::builtin::slots::BROWSER)).
    pub fn has_slot(&self, slot: &crate::Id) -> bool {
        self.slots.contains(slot)
    }

    /// What each plugin took since PaperPaw started (`core`: the kernel
    /// itself), up to now: a copy made when asked, as the kernel keeps
    /// one table only (memory is short on small devices).
    pub fn plugin_usage(&self) -> alloc::vec::Vec<crate::usage::PluginUsage> {
        self.plugin_usage.borrow().clone()
    }

    /// The directory where plugin `plugin` keeps its data, e.g.
    /// `.paperpaw/plugins/reader` (in PaperPaw's own folder,
    /// [`data::DIR`](crate::data::DIR)).
    pub fn plugin_dir(plugin: &str) -> alloc::string::String {
        alloc::format!("{}/plugins/{plugin}", crate::data::DIR)
    }
}

/// Converts physical lengths to pixels for the current screen, so layouts
/// look the same on low- and high-density displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metrics {
    pub dpi: u16,
}

impl Metrics {
    pub const fn new(dpi: u16) -> Self {
        Self { dpi }
    }

    /// Pixels for `mm` millimetres (rounded, at least 1 for non-zero input).
    pub const fn mm(&self, mm: u32) -> u32 {
        self.tenth_mm(mm * 10)
    }

    /// Pixels for a font size in typographic points (1/72 inch).
    pub fn pt(&self, points: f32) -> f32 {
        points * f32::from(self.dpi) / 72.0
    }

    /// Pixels for `tenths` tenths of a millimetre.
    pub const fn tenth_mm(&self, tenths: u32) -> u32 {
        let px = (tenths * self.dpi as u32 + 127) / 254;
        if px == 0 && tenths > 0 { 1 } else { px }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_millimetres() {
        let metrics = Metrics::new(254);
        assert_eq!(metrics.mm(1), 10);
        assert_eq!(metrics.tenth_mm(5), 5);
        assert_eq!(Metrics::new(100).tenth_mm(1), 1);
        assert_eq!(Metrics::new(100).mm(0), 0);
    }
}
