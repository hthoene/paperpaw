//! Where a kernel handler runs: the kernel's shared state, the time of the
//! event and the part of the screen it has (#300, D209). The kernel's own
//! in place of the SDK's old `Env`.

use crate::kit::action::Actions;
use crate::kit::status::StatusItems;
use paperpaw_i18n::Translator;
use paperpaw_platform::{DeviceInfo, Instant, Rect, Storage, SystemStatus};
use paperpaw_sdk::{Capabilities, Fonts, Formats, Hyphenation, Metrics, Plugins, Settings};

use crate::Shared;

/// The kernel's state as one handler sees it: the time of its event and
/// the device and part of the screen it has.
#[derive(Clone, Copy)]
pub struct Place<'a> {
    shared: &'a Shared,
    device: &'a DeviceInfo,
    content: Rect,
    now: Instant,
}

impl core::fmt::Debug for Place<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Place")
            .field("device", &self.device.id)
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

impl Shared {
    /// The place of a handler of an event at `now`, with the whole content
    /// area.
    pub(crate) fn place(&self, now: Instant) -> Place<'_> {
        Place {
            shared: self,
            device: &self.device,
            content: self.content,
            now,
        }
    }
}

impl<'a> Place<'a> {
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
        Metrics::new(self.shared.device.display.dpi)
    }

    /// Its `settings`.
    pub fn settings(&self) -> &'a Settings {
        &self.shared.settings
    }

    /// The texts in the reader's language.
    pub fn i18n(&self) -> &'a Translator {
        &self.shared.translator
    }

    /// The direction of the interface: right to left where the catalog
    /// of its language says so ([`Translator::direction`]).
    pub fn direction(&self) -> paperpaw_sdk::lang::Direction {
        self.shared.translator.direction()
    }

    /// The language of the interface, e.g. `de` or `ar`.
    pub fn lang(&self) -> paperpaw_sdk::Lang {
        paperpaw_sdk::Lang::parse(self.shared.translator.language())
    }

    /// Its `fonts`.
    pub fn fonts(&self) -> &'a Fonts {
        &self.shared.fonts
    }

    /// Its `formats`.
    pub fn formats(&self) -> &'a Formats {
        &self.shared.formats
    }

    /// Hyphenation patterns by language.
    pub fn hyphenation(&self) -> &'a Hyphenation {
        &self.shared.hyphenation
    }

    /// Its menus, labelled by its actions and settings.
    pub fn menus(&self) -> crate::kit::menus::MenuView<'a> {
        crate::kit::menus::MenuView::new(
            &self.shared.menus,
            &self.shared.actions,
            &self.shared.settings,
        )
    }

    /// Every action, e.g. to offer them in a menu editor.
    pub fn actions(&self) -> &'a Actions {
        &self.shared.actions
    }

    /// Every plugin with its state, for plugin managers.
    pub fn plugins(&self) -> &'a Plugins {
        &self.shared.plugins
    }

    /// The time of day and the battery, as far as the device knows them
    /// (see [`time`](paperpaw_sdk::time) for the local time).
    pub fn status(&self) -> &'a SystemStatus {
        &self.shared.status
    }

    /// What the device did since PaperPaw started: refreshes, input (see
    /// [`usage`](paperpaw_sdk::usage)); what each plugin took is
    /// [`Place::plugin_usage`].
    pub fn usage(&self) -> &'a paperpaw_sdk::usage::Usage {
        &self.shared.usage
    }

    /// Items for status lines, from every plugin.
    pub fn status_items(&self) -> &'a StatusItems {
        &self.shared.status_items
    }

    /// Marks on the words of book pages, from every plugin (see
    /// [`decorations`](paperpaw_sdk::decorations)).
    pub fn decorators(&self) -> &'a crate::kit::decorations::Decorators {
        &self.shared.decorators
    }

    /// Plugins' stylesheets for books, in the order of the plugins (see
    /// `Items::add_stylesheet`).
    pub fn stylesheets(&self) -> &'a [alloc::borrow::Cow<'static, str>] {
        &self.shared.stylesheets
    }

    /// Folders of what plugins keep for each book
    /// (`Items::add_book_data`).
    pub fn book_data(&self) -> &'a [crate::kit::library::BookData] {
        &self.shared.book_data
    }

    /// Widgets for screens that show them, from every plugin.
    pub fn widgets(&self) -> &'a crate::kit::widgets::Widgets {
        &self.shared.widgets
    }

    /// The steps of the first-run setup, from every plugin (see
    /// `setup`).
    pub fn setup(&self) -> &'a crate::kit::setup::SetupSteps {
        &self.shared.setup
    }

    /// What plugins offer each other (see [`services`](paperpaw_sdk::services)).
    pub fn services(&self) -> &'a paperpaw_sdk::services::Services {
        &self.shared.services
    }

    /// What buttons, taps and swipes do (see [`keys`](paperpaw_sdk::keys)).
    pub fn keys(&self) -> &'a dyn paperpaw_sdk::keys::KeyBindings {
        &self.shared.keymap
    }

    /// The look a plugin gave the parts of screens, set with
    /// `Items::set_ui_look` (see
    /// [`ui::look`](crate::kit::ui::look)).
    pub fn look(&self) -> crate::kit::ui::look::Look {
        self.shared.look.0
    }

    /// What the device and its shell can do (network, speech, the sleep
    /// screen …).
    pub fn caps(&self) -> Capabilities {
        self.shared.caps
    }

    /// Wi-Fi, where PaperPaw manages it itself (see
    /// [`wifi`](paperpaw_sdk::wifi)); `None` where the system does.
    pub fn wifi(&self) -> Option<&'a paperpaw_sdk::wifi::Wifi> {
        self.shared.wifi.as_ref()
    }

    /// Bluetooth, where the kernel uses a radio (see
    /// [`bluetooth`](paperpaw_sdk::bluetooth)); `None` elsewhere.
    pub fn bluetooth(&self) -> Option<paperpaw_sdk::bluetooth::BluetoothView<'a>> {
        self.shared
            .bluetooth
            .as_ref()
            .map(|all| paperpaw_sdk::bluetooth::BluetoothView::new(all, &*self.shared.meter))
    }

    /// Firmware updates, where PaperPaw is the device's firmware (see
    /// [`firmware`](paperpaw_sdk::firmware)); `None` where it is an app.
    pub fn firmware(&self) -> Option<&'a paperpaw_sdk::firmware::Firmware> {
        self.shared.firmware.as_ref()
    }

    /// How native code plugins are loaded here, e.g. which builds fit
    /// (see [`native`](paperpaw_sdk::native)); `None` where PaperPaw loads none.
    pub fn native(&self) -> Option<&'a paperpaw_sdk::native::Native> {
        self.shared.native.as_ref()
    }

    /// The power level and mode in use (see [`power`](paperpaw_sdk::power)).
    pub fn power(&self) -> &'a paperpaw_sdk::power::Power {
        &self.shared.power
    }

    /// Where declared work stands, and plugins' notes (see
    /// [`work`](paperpaw_sdk::work)).
    pub fn work(&self) -> &'a paperpaw_sdk::work::WorkStates {
        &self.shared.work_states
    }

    /// What is handled now is the reader's own doing (input, a screen or
    /// an action they opened or ran, the answer to their request): act at
    /// once. Otherwise it is background work (an event, a rule's action,
    /// declared work), which may wait for a good moment (see
    /// [`work`](paperpaw_sdk::work)) and says nothing over reading.
    pub fn reader(&self) -> bool {
        self.shared.reader
    }

    /// Whether there is text on the clipboard, copied here
    /// ([`Ask::SetClipboard`](crate::Ask::SetClipboard)) or, where
    /// the system has a clipboard, in another app; read it with
    /// [`Ask::ReadClipboard`](crate::Ask::ReadClipboard).
    pub fn clipboard(&self) -> bool {
        self.shared.system_clipboard || !self.shared.clipboard.is_empty()
    }

    /// What the reader used last: buttons, a finger or a pen, e.g. to
    /// show a focus only after buttons were used, also on touch devices.
    pub fn last_input(&self) -> paperpaw_sdk::keys::InputMode {
        self.shared.last_input
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

    /// How much room the part of the screen this is for has
    /// ([`content`](Self::content)): its [class](crate::kit::ui::ScreenClass)
    /// by width, at the reader's size of text. Compact on the X4 in both
    /// orientations.
    pub fn screen_class(&self) -> crate::kit::ui::ScreenClass {
        crate::kit::ui::class_of(self, self.content)
    }

    /// Whether the part of the screen this is for
    /// ([`content`](Self::content)) is wider than high: the device on its
    /// side (or, for a list beside an item, the part it has).
    pub fn landscape(&self) -> bool {
        self.content.width > self.content.height
    }

    /// The open screens, the one shown last (see [`StackEntry`](paperpaw_sdk::StackEntry)):
    /// what is shown, e.g. for a rule or a refresh filter.
    pub fn stack(&self) -> &'a [paperpaw_sdk::StackEntry] {
        &self.shared.stack
    }

    /// The slots some screen is bound to (see [`Place::has_slot`]).
    pub fn slots(&self) -> &'a [paperpaw_sdk::Id] {
        &self.shared.slots
    }

    /// Every screen: its id and the translation key of its name, e.g. for
    /// settings that differ per screen (what gestures do there).
    pub fn screens(&self) -> crate::screen_names::ScreenNames<'a> {
        self.shared.registry.screens.list()
    }

    /// The folders reading fonts are found in at start-up, the first one
    /// PaperPaw's own (where new fonts go); empty where the device has
    /// only its built-in fonts.
    pub fn font_dirs(&self) -> &'a [alloc::string::String] {
        &self.shared.font_dirs
    }

    /// What plugin libraries call this kind of device, e.g. `x4`, `x4-pro`
    /// or `android` for paperpaw.org's (ADR 0023); `None` where the shell
    /// names none, and a store has nothing made for it.
    pub fn catalog_device(&self) -> Option<&'a str> {
        self.shared.catalog_device
    }

    /// Passwords and keys, kept for every plugin in one place (see
    /// [`secrets`](paperpaw_sdk::secrets)).
    pub fn secrets(&self) -> paperpaw_sdk::secrets::Visible<'a> {
        paperpaw_sdk::secrets::Visible::new(&self.shared.secrets, &*self.shared.meter)
    }

    /// Random bytes from the platform's source, for keys and tokens (see
    /// [`random`](paperpaw_sdk::random)).
    pub fn random(&self) -> paperpaw_sdk::random::Random {
        self.shared.random
    }

    /// How much memory is free now, where the shell can tell (see
    /// [`memory`](paperpaw_sdk::memory)).
    pub fn memory(&self) -> paperpaw_sdk::memory::Memory {
        self.shared.memory
    }

    /// A monotonic clock in microseconds, for measuring how long work
    /// takes (see [`time::Clock`](crate::kit::time::Clock)).
    pub fn clock(&self) -> crate::kit::time::Clock {
        self.shared.meter.clock()
    }

    /// How PaperPaw started this time, where the shell can tell (see
    /// [`power::Start`](paperpaw_sdk::power::Start)).
    pub fn started(&self) -> paperpaw_sdk::power::Start {
        self.shared.started
    }

    /// How long the device slept before it woke last, where the shell can
    /// tell: after [`events::RESUME`](paperpaw_sdk::builtin::events::RESUME), and
    /// after a start that ends a sleep in which the device was switched
    /// off, if its clock ran on. `None` before the first sleep and where
    /// it is not known: a device without a clock that keeps time switched
    /// off (the X4) cannot tell; a plugin that needs it keeps a stamp of
    /// its own. Where the clock is the chip's slow RTC clock (the X4's light
    /// sleep), expect an error of a few percent (1-5 %): right for "slept
    /// for two hours", not for a stopwatch.
    pub fn slept_for(&self) -> Option<core::time::Duration> {
        self.shared.slept_for
    }

    /// What the panel can do, and how the last frame went (see
    /// [`realtime`](paperpaw_sdk::realtime)): for screens that pace themselves.
    pub fn panel(&self) -> paperpaw_sdk::realtime::PanelInfo {
        self.shared.panel
    }

    /// The buttons held down now: for screens that take raw keys
    /// ([`ScreenMode::raw_keys`](paperpaw_sdk::realtime::ScreenMode::raw_keys)),
    /// also for the others.
    pub fn keys_held(&self) -> &'a [&'static str] {
        &self.shared.keys_held
    }

    /// The memory the screen on top asked for
    /// ([`ScreenMode::memory`](paperpaw_sdk::realtime::ScreenMode::memory)) and
    /// what it got.
    pub fn room(&self) -> paperpaw_sdk::realtime::Room {
        self.shared.room
    }

    /// The device's storage. Plugins keep their own files below
    /// [`data::plugin_dir`](paperpaw_sdk::data::plugin_dir).
    pub fn storage(&self) -> &'a dyn Storage {
        self.shared.storage.as_ref()
    }

    /// The name the device goes by (Settings > Device), else the shell's.
    pub fn device_name(&self) -> alloc::string::String {
        let set = self
            .shared
            .settings
            .read(&paperpaw_sdk::device_name::SETTING);
        paperpaw_sdk::device_name::chosen(set, self.device)
    }

    /// The device as a screen placed in `area` sees it: the display as large
    /// as the area.
    pub fn device_within(&self, area: Rect) -> DeviceInfo {
        let mut device = *self.device;
        device.display.width = area.width;
        device.display.height = area.height;
        device
    }

    /// This place for a screen in `area` of `device` (see
    /// [`Place::device_within`]).
    pub fn within<'b>(&self, device: &'b DeviceInfo, area: Rect) -> Place<'b>
    where
        'a: 'b,
    {
        Place {
            shared: self.shared,
            device,
            content: area,
            now: self.now,
        }
    }

    /// The screen chosen for `slot`, if any is offered.
    pub fn slot_screen(&self, slot: &str) -> Option<&'a paperpaw_sdk::Id> {
        self.shared.registry.slot_screen(slot)
    }

    /// Every screen offered for `slot`.
    pub fn slot_screens(&self, slot: &str) -> alloc::vec::Vec<&'a paperpaw_sdk::Id> {
        self.shared.registry.ordered_screens(slot)
    }

    /// Visits the screens offered for `slot` while `visit` says go on.
    pub fn each_slot_screen(
        &self,
        slot: &str,
        visit: &mut dyn FnMut(&'a paperpaw_sdk::Id) -> bool,
    ) {
        self.shared.registry.each_slot_screen(slot, visit);
    }

    /// The screen the reader chose for `slot`, if any.
    pub fn slot_choice(&self, slot: &str) -> Option<&'a paperpaw_sdk::Id> {
        self.shared.registry.slot_choice(slot)
    }

    /// The title of `slot`'s choice.
    pub fn slot_title(&self, slot: &str) -> Option<&'a str> {
        self.shared.registry.slot_title(slot)
    }

    /// The title of `screen`.
    pub fn screen_title(&self, screen: &paperpaw_sdk::Id) -> Option<&'a str> {
        self.shared.registry.screen_title(screen)
    }

    /// The screen on top.
    pub fn top(&self) -> Option<&'a paperpaw_sdk::StackEntry> {
        self.shared.stack.last()
    }

    /// Whether `slot` is one of the slots.
    pub fn has_slot(&self, slot: &paperpaw_sdk::Id) -> bool {
        self.shared.slots.contains(slot)
    }

    /// What each plugin took so far.
    pub fn plugin_usage(&self) -> alloc::vec::Vec<paperpaw_sdk::usage::PluginUsage> {
        self.shared.meter.table.borrow().clone()
    }
}
