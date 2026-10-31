//! The kinds of [`Contribution`]: what the kernel applies of a plugin.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;

use crate::action::ActionSpec;
use crate::event::EventHandler;
use crate::filters::{InputFilterSpec, RefreshFilterSpec, TextFilterSpec};
use crate::fonts::{FontFace, FontSpec};
use crate::formats::FormatSpec;
use crate::hyphenation::HyphenationSpec;
use crate::id::Id;
use crate::menus::{MenuEntry, MenuNeed, MenuSection};
use crate::plugin::{Priority, SlotSpec};
use crate::presets::Preset;
use crate::screen::{HintVisibility, ScreenSpec};
use crate::settings::SettingSpec;
use crate::setup::SetupStepSpec;
use crate::status::{Place as StatusPlace, StatusItemSpec, StatusLineSpec};
use crate::widgets::WidgetSpec;

/// One thing a plugin adds. New kinds come in minor versions. The large
/// ones are boxed: a plugin's list stays small on small devices.
#[non_exhaustive]
pub enum Contribution {
    Action(Box<ActionSpec>),
    /// Actions kept in the program (a `static` list).
    FixedActions(&'static [crate::action::FixedAction]),
    Screen(ScreenSpec),
    /// Screens kept in the program (a `static` list).
    FixedScreens(&'static [crate::screen::FixedScreen]),
    Slot(SlotSpec),
    /// A screen proposed for a slot (see [`Registrar::bind_slot`](crate::Registrar::bind_slot)).
    #[non_exhaustive]
    SlotBinding {
        slot: Id,
        screen: Id,
        priority: Priority,
    },
    Setting(Box<SettingSpec>),
    /// Settings kept in the program (a `static` list).
    FixedSettings(&'static [SettingSpec]),
    SettingGroup(crate::settings::SettingGroup),
    /// Another default for any plugin's setting (see
    /// [`Registrar::set_setting_default`](crate::Registrar::set_setting_default)).
    #[non_exhaustive]
    SettingDefault {
        setting: Id,
        value: Cow<'static, str>,
    },
    /// Another value of any plugin's choice setting (see
    /// [`Registrar::add_choice`](crate::Registrar::add_choice)).
    #[non_exhaustive]
    Choice {
        setting: Id,
        value: Cow<'static, str>,
    },
    /// Default key bindings (see [`Registrar::add_keymap`](crate::Registrar::add_keymap)).
    Keymap(Cow<'static, str>),
    #[non_exhaustive]
    KeymapPreset {
        id: Id,
        source: Cow<'static, str>,
    },
    /// Key bindings that apply while a switch is on (see
    /// [`Registrar::add_keymap_when`](crate::Registrar::add_keymap_when)).
    #[non_exhaustive]
    KeymapWhen {
        setting: Id,
        source: Cow<'static, str>,
    },
    Stylesheet(Cow<'static, str>),
    BookData(crate::library::BookData),
    /// A field kept with books ([`Registrar::add_book_field`](crate::Registrar::add_book_field)).
    BookField(Box<crate::library::BookField>),
    /// A badge on books' rows ([`Registrar::add_book_badge`](crate::Registrar::add_book_badge)).
    BookBadge(Box<crate::library::BookBadge>),
    Font(Box<FontSpec>),
    #[non_exhaustive]
    FontFile {
        id: Id,
        path: String,
    },
    #[non_exhaustive]
    FontExtension {
        id: Id,
        path: String,
        extends: Id,
    },
    /// A face of a family in a file (see [`Registrar::add_font_face`](crate::Registrar::add_font_face)).
    FontFace(Box<FontFace>),
    #[non_exhaustive]
    FontBinding {
        role: Id,
        font: Id,
        priority: Priority,
    },
    Format(Box<FormatSpec>),
    Hyphenation(HyphenationSpec),
    MenuEntry(MenuEntry),
    /// Menu entries kept in the program (a `static` list).
    FixedMenuEntries(&'static [MenuEntry]),
    MenuSection(MenuSection),
    /// Icons kept in the program (a `static` list of items and icons).
    FixedIcons(&'static [(Id, &'static str)]),
    #[non_exhaustive]
    Icon {
        item: Id,
        icon: Cow<'static, str>,
    },
    #[non_exhaustive]
    MenuNeed {
        item: Id,
        need: MenuNeed,
    },
    #[non_exhaustive]
    HideMenuEntry {
        menu: Id,
        item: Id,
    },
    #[non_exhaustive]
    ItemState {
        item: Id,
        state: crate::ItemStateFn,
    },
    #[non_exhaustive]
    ItemDescription {
        item: Id,
        key: Cow<'static, str>,
    },
    #[non_exhaustive]
    ScreenHints {
        screen: Id,
        hints: HintVisibility,
    },
    #[non_exhaustive]
    Subscription {
        event: Id,
        handler: EventHandler,
    },
    StatusItem(Box<StatusItemSpec>),
    StatusLine(Box<StatusLineSpec>),
    /// Boxed: four ids, larger than the other contributions.
    StatusPlace(Box<StatusPlace>),
    WidgetPlace(Box<crate::widgets::WidgetPlace>),
    WidgetCategory(Box<crate::widgets::WidgetCategory>),
    TextFilter(TextFilterSpec),
    Decorator(Box<crate::decorations::DecoratorSpec>),
    PageLayer(Box<crate::layers::PageLayerSpec>),
    PageFrame(Box<crate::layers::PageFrameSpec>),
    LookupProvider(Box<crate::lookup::LookupProviderSpec>),
    Voice(Box<crate::voices::VoiceSpec>),
    Reader(crate::readers::ReaderSpec),
    InputFilter(InputFilterSpec),
    RefreshFilter(RefreshFilterSpec),
    /// A service of the device's own GATT server (see
    /// [`bluetooth`](crate::bluetooth)).
    GattService(Box<crate::bluetooth::ServiceSpec>),
    /// A bar along an edge of the screen (see [`chrome`](crate::chrome)).
    Chrome(Box<crate::chrome::ChromeSpec>),
    /// A named input (see [`keys::InputSpec`](crate::keys::InputSpec)).
    Input(crate::keys::InputSpec),
    /// A handler put in place of an action's own, of any plugin
    /// ([`Registrar::override_action`](crate::Registrar::override_action)).
    ActionOverride(Box<crate::action::ActionOverride>),
    /// Sees every action run ([`Registrar::observe_actions`](crate::Registrar::observe_actions)).
    ActionObserver(Box<crate::action::ObserverSpec>),
    /// Sees every event emitted ([`Registrar::observe_events`](crate::Registrar::observe_events)).
    EventObserver(Box<crate::action::ObserverSpec>),
    /// A recognizer of gestures (see [`keys::GestureSpec`](crate::keys::GestureSpec)).
    Gesture(Box<crate::keys::GestureSpec>),
    /// A job: work done in steps (see [`work`](crate::work)).
    Work(Box<crate::work::WorkSpec>),
    Widget(Box<WidgetSpec>),
    SetupStep(Box<SetupStepSpec>),
    /// A preset made at start (see [`presets`](crate::presets)).
    Preset(Box<Preset>),
    /// Presets kept in the program.
    FixedPresets(&'static [Preset]),
    /// A service with the name of its type (see [`Registrar::provide`](crate::Registrar::provide)).
    #[non_exhaustive]
    Published {
        id: Id,
        service: crate::services::Service,
    },
    /// A point declared to contribute to ([`Registrar::add_point`](crate::Registrar::add_point)).
    Point(Box<crate::services::PointSpec>),
    /// An event declared ([`Registrar::declare_event`](crate::Registrar::declare_event)).
    EventDeclared(Box<crate::services::EventSpec>),
    /// Text for a point another plugin defined (see
    /// [`services`](crate::services)).
    #[non_exhaustive]
    ToPoint {
        point: Id,
        text: Cow<'static, str>,
    },
    /// A translation catalog (see `paperpaw-i18n`).
    Translations(Cow<'static, str>),
    /// A look for the parts of screens (see [`Registrar::set_ui_look`](crate::Registrar::set_ui_look)).
    #[non_exhaustive]
    UiLook {
        painters: &'static crate::ui::look::Painters,
        priority: Priority,
    },
    /// A contribution of a kind that comes after this release, by its key
    /// (see [`Registrar::add_extended`](crate::Registrar::add_extended)).
    #[non_exhaustive]
    Extended {
        key: Id,
        params: crate::Params,
    },
    /// A hook that changes a finished band of the screen (see
    /// [`render`](crate::render)).
    RenderHook(Box<crate::render::RenderHookSpec>),
}

impl Contribution {
    /// What kind it is, e.g. `action` or `setting`.
    pub fn kind(&self) -> &'static str {
        match self {
            Contribution::Action(_) | Contribution::FixedActions(_) => "action",
            Contribution::Screen(_) | Contribution::FixedScreens(_) => "screen",
            Contribution::Slot(_) => "slot",
            Contribution::SlotBinding { .. } => "slot-binding",
            Contribution::Setting(_) | Contribution::FixedSettings(_) => "setting",
            Contribution::SettingGroup(_) => "setting-group",
            Contribution::SettingDefault { .. } => "setting-default",
            Contribution::Choice { .. } => "choice",
            Contribution::Keymap(_) => "keymap",
            Contribution::KeymapPreset { .. } => "keymap-preset",
            Contribution::KeymapWhen { .. } => "keymap-when",
            Contribution::Stylesheet(_) => "stylesheet",
            Contribution::BookData(_) => "book data",
            Contribution::BookField(_) => "book field",
            Contribution::BookBadge(_) => "book badge",
            Contribution::Font(_) => "font",
            Contribution::FontFile { .. } => "font-file",
            Contribution::FontExtension { .. } => "font-extension",
            Contribution::FontFace(_) => "font-face",
            Contribution::FontBinding { .. } => "font-binding",
            Contribution::Format(_) => "format",
            Contribution::Hyphenation(_) => "hyphenation",
            Contribution::MenuEntry(_) | Contribution::FixedMenuEntries(_) => "menu-entry",
            Contribution::MenuSection(_) => "menu-section",
            Contribution::Icon { .. } | Contribution::FixedIcons(_) => "icon",
            Contribution::MenuNeed { .. } => "menu-need",
            Contribution::HideMenuEntry { .. } => "hidden-menu-entry",
            Contribution::ItemState { .. } => "item-state",
            Contribution::ItemDescription { .. } => "item-description",
            Contribution::ScreenHints { .. } => "screen-hints",
            Contribution::Subscription { .. } => "subscription",
            Contribution::StatusItem(_) => "status-item",
            Contribution::StatusLine(_) => "status-line",
            Contribution::StatusPlace(_) => "status-place",
            Contribution::WidgetPlace(_) => "widget-place",
            Contribution::WidgetCategory(_) => "widget-category",
            Contribution::TextFilter(_) => "text-filter",
            Contribution::Decorator(_) => "decorator",
            Contribution::PageLayer(_) => "page-layer",
            Contribution::PageFrame(_) => "page-frame",
            Contribution::LookupProvider(_) => "lookup-provider",
            Contribution::Voice(_) => "voice",
            Contribution::Reader(_) => "reader",
            Contribution::InputFilter(_) => "input-filter",
            Contribution::RefreshFilter(_) => "refresh-filter",
            Contribution::GattService(_) => "gatt-service",
            Contribution::Chrome(_) => "chrome",
            Contribution::Input(_) => "input",
            Contribution::ActionOverride(_) => "action-override",
            Contribution::ActionObserver(_) => "action-observer",
            Contribution::EventObserver(_) => "event-observer",
            Contribution::Gesture(_) => "gesture",
            Contribution::Work(_) => "work",
            Contribution::Widget(_) => "widget",
            Contribution::SetupStep(_) => "setup-step",
            Contribution::Preset(_) | Contribution::FixedPresets(_) => "preset",
            Contribution::Published { .. } => "service",
            Contribution::ToPoint { .. } => "to-point",
            Contribution::Point(_) => "point",
            Contribution::EventDeclared(_) => "event",
            Contribution::Translations(_) => "translations",
            Contribution::UiLook { .. } => "ui-look",
            Contribution::Extended { .. } => "extended",
            Contribution::RenderHook(_) => "render-hook",
        }
    }

    /// The id of what it adds, for those that add something of their own
    /// (an action, a screen …): in the plugin's namespace and once per
    /// kind (fonts and font files share one). `None` for bindings, menu
    /// entries and the like, which may name what other plugins added.
    pub fn id(&self) -> Option<&Id> {
        Some(match self {
            Contribution::Action(a) => &a.id,
            Contribution::Screen(s) => &s.id,
            Contribution::Slot(s) => &s.id,
            Contribution::Setting(s) => &s.id,
            Contribution::SettingGroup(g) => &g.id,
            Contribution::KeymapPreset { id, .. } => id,
            Contribution::Font(f) => &f.id,
            Contribution::FontFile { id, .. } => id,
            Contribution::FontExtension { id, .. } => id,
            Contribution::FontFace(f) => &f.id,
            Contribution::Format(f) => &f.id,
            Contribution::Hyphenation(h) => &h.id,
            Contribution::MenuSection(s) => &s.id,
            Contribution::StatusItem(i) => &i.id,
            Contribution::StatusLine(l) => &l.id,
            Contribution::StatusPlace(p) => &p.id,
            Contribution::WidgetPlace(p) => &p.id,
            Contribution::WidgetCategory(c) => &c.id,
            Contribution::TextFilter(f) => &f.id,
            Contribution::Decorator(d) => &d.id,
            Contribution::PageLayer(l) => &l.id,
            Contribution::PageFrame(f) => &f.id,
            Contribution::LookupProvider(p) => &p.id,
            Contribution::Voice(v) => &v.id,
            Contribution::Reader(r) => &r.id,
            Contribution::InputFilter(f) => &f.id,
            Contribution::RefreshFilter(f) => &f.id,
            Contribution::RenderHook(h) => &h.id,
            Contribution::Chrome(c) => &c.id,
            Contribution::Input(i) => &i.id,
            Contribution::ActionObserver(o) | Contribution::EventObserver(o) => &o.id,
            Contribution::Gesture(g) => &g.id,
            Contribution::Work(w) => &w.id,
            Contribution::BookField(f) => &f.id,
            Contribution::BookBadge(b) => &b.id,
            Contribution::Widget(w) => &w.id,
            Contribution::SetupStep(s) => &s.id,
            Contribution::Preset(p) => &p.id,
            Contribution::Published { id, .. } => id,
            Contribution::Point(p) => &p.id,
            Contribution::EventDeclared(e) => &e.id,
            _ => return None,
        })
    }

    /// The kind its id is unique within.
    pub(super) fn id_kind(&self) -> &'static str {
        match self {
            Contribution::FontFile { .. } | Contribution::FontFace(_) => "font",
            other => other.kind(),
        }
    }
}

impl fmt::Debug for Contribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = f.debug_struct("Contribution");
        out.field("kind", &self.kind());
        if let Some(id) = self.id() {
            out.field("id", id);
        }
        out.finish_non_exhaustive()
    }
}
