//! The kinds of [`Contribution`]: what the kernel applies of a plugin.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;

use crate::action::ActionSpec;
use crate::event::EventHandler;
use crate::fonts::{FontFace, FontSpec};
use crate::formats::FormatSpec;
use crate::id::Id;
use crate::menus::{MenuEntry, MenuNeed, MenuSection};
use crate::plugin::{Priority, SlotSpec};
use crate::presets::Preset;
use crate::settings::SettingSpec;
use crate::setup::SetupStepSpec;
use crate::status::{StatusItemSpec, StatusLineSpec};
use crate::widgets::WidgetSpec;

/// One thing a plugin adds. New kinds come in minor versions. The large
/// ones are boxed: a plugin's list stays small on small devices.
#[non_exhaustive]
pub enum Contribution {
    Action(Box<ActionSpec>),
    /// Actions kept in the program (a `static` list).
    FixedActions(&'static [crate::action::FixedAction]),
    Slot(SlotSpec),
    /// A screen proposed for a slot (see `Items::bind_slot`).
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
    /// `Items::set_setting_default`).
    SettingDefault {
        setting: Id,
        value: Cow<'static, str>,
    },
    /// Another value of any plugin's choice setting (see
    /// `Items::add_choice`).
    Choice {
        setting: Id,
        value: Cow<'static, str>,
    },
    /// Default key bindings (see `Items::add_keymap`).
    Keymap(Cow<'static, str>),
    KeymapPreset {
        id: Id,
        source: Cow<'static, str>,
    },
    Stylesheet(Cow<'static, str>),
    BookData(crate::library::BookData),
    Font(Box<FontSpec>),
    FontFile {
        id: Id,
        path: String,
    },
    FontExtension {
        id: Id,
        path: String,
        extends: Id,
    },
    /// A face of a family in a file (see `Items::add_font_face`).
    FontFace(Box<FontFace>),
    FontBinding {
        role: Id,
        font: Id,
        priority: Priority,
    },
    Format(Box<FormatSpec>),
    MenuEntry(MenuEntry),
    /// Menu entries kept in the program (a `static` list).
    FixedMenuEntries(&'static [MenuEntry]),
    MenuSection(MenuSection),
    /// Icons kept in the program (a `static` list of items and icons).
    FixedIcons(&'static [(Id, &'static str)]),
    Icon {
        item: Id,
        icon: Cow<'static, str>,
    },
    MenuNeed {
        item: Id,
        need: MenuNeed,
    },
    HideMenuEntry {
        menu: Id,
        item: Id,
    },
    ItemState {
        item: Id,
        state: crate::ItemStateFn,
    },
    Subscription {
        event: Id,
        handler: EventHandler,
    },
    StatusItem(Box<StatusItemSpec>),
    StatusLine(Box<StatusLineSpec>),
    Decorator(Box<crate::decorations::DecoratorSpec>),
    /// A job: work done in steps (see [`work`](crate::work)).
    Work(Box<crate::work::WorkSpec>),
    Widget(Box<WidgetSpec>),
    SetupStep(Box<SetupStepSpec>),
    /// A preset made at start (see [`presets`](crate::presets)).
    Preset(Box<Preset>),
    /// Presets kept in the program.
    FixedPresets(&'static [Preset]),
    /// A service with the name of its type (see `Items::provide`).
    Published {
        id: Id,
        service: crate::services::Service,
    },
    /// An event declared (`Items::declare_event`).
    EventDeclared(Box<crate::services::EventSpec>),
    /// Text for a point another plugin defined (see
    /// [`services`](crate::services)).
    ToPoint {
        point: Id,
        text: Cow<'static, str>,
    },
    /// A translation catalog (see `paperpaw-i18n`).
    Translations(Cow<'static, str>),
    /// A compiled translation catalog on the storage, read as needed
    /// (see `Items::add_translation_file`).
    TranslationFile(String),
}

impl Contribution {
    /// What kind it is, e.g. `action` or `setting`.
    pub fn kind(&self) -> &'static str {
        match self {
            Contribution::Action(_) | Contribution::FixedActions(_) => "action",
            Contribution::Slot(_) => "slot",
            Contribution::SlotBinding { .. } => "slot-binding",
            Contribution::Setting(_) | Contribution::FixedSettings(_) => "setting",
            Contribution::SettingGroup(_) => "setting-group",
            Contribution::SettingDefault { .. } => "setting-default",
            Contribution::Choice { .. } => "choice",
            Contribution::Keymap(_) => "keymap",
            Contribution::KeymapPreset { .. } => "keymap-preset",
            Contribution::Stylesheet(_) => "stylesheet",
            Contribution::BookData(_) => "book data",
            Contribution::Font(_) => "font",
            Contribution::FontFile { .. } => "font-file",
            Contribution::FontExtension { .. } => "font-extension",
            Contribution::FontFace(_) => "font-face",
            Contribution::FontBinding { .. } => "font-binding",
            Contribution::Format(_) => "format",
            Contribution::MenuEntry(_) | Contribution::FixedMenuEntries(_) => "menu-entry",
            Contribution::MenuSection(_) => "menu-section",
            Contribution::Icon { .. } | Contribution::FixedIcons(_) => "icon",
            Contribution::MenuNeed { .. } => "menu-need",
            Contribution::HideMenuEntry { .. } => "hidden-menu-entry",
            Contribution::ItemState { .. } => "item-state",
            Contribution::Subscription { .. } => "subscription",
            Contribution::StatusItem(_) => "status-item",
            Contribution::StatusLine(_) => "status-line",
            Contribution::Decorator(_) => "decorator",
            Contribution::Work(_) => "work",
            Contribution::Widget(_) => "widget",
            Contribution::SetupStep(_) => "setup-step",
            Contribution::Preset(_) | Contribution::FixedPresets(_) => "preset",
            Contribution::Published { .. } => "service",
            Contribution::ToPoint { .. } => "to-point",
            Contribution::EventDeclared(_) => "event",
            Contribution::Translations(_) | Contribution::TranslationFile(_) => "translations",
        }
    }

    /// The id of what it adds, for those that add something of their own
    /// (an action, a screen …): in the plugin's namespace and once per
    /// kind (fonts and font files share one). `None` for bindings, menu
    /// entries and the like, which may name what other plugins added.
    pub fn id(&self) -> Option<&Id> {
        Some(match self {
            Contribution::Action(a) => &a.id,
            Contribution::Slot(s) => &s.id,
            Contribution::Setting(s) => &s.id,
            Contribution::SettingGroup(g) => &g.id,
            Contribution::KeymapPreset { id, .. } => id,
            Contribution::Font(f) => &f.id,
            Contribution::FontFile { id, .. } => id,
            Contribution::FontExtension { id, .. } => id,
            Contribution::FontFace(f) => &f.id,
            Contribution::Format(f) => &f.id,
            Contribution::MenuSection(s) => &s.id,
            Contribution::StatusItem(i) => &i.id,
            Contribution::StatusLine(l) => &l.id,
            Contribution::Decorator(d) => &d.id,
            Contribution::Work(w) => &w.id,
            Contribution::Widget(w) => &w.id,
            Contribution::SetupStep(s) => &s.id,
            Contribution::Preset(p) => &p.id,
            Contribution::Published { id, .. } => id,
            Contribution::EventDeclared(e) => &e.id,
            _ => return None,
        })
    }

    /// The kind its id is unique within.
    pub fn id_kind(&self) -> &'static str {
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
