//! The settings screens, generated from the settings every plugin declares
//! (`Registrar::add_setting`), so no plugin needs its own settings UI.
//!
//! - The main page lists the setting groups and pages other plugins add
//!   to the `core.settings-pages` menu (e.g. the plugin manager) with
//!   their icons, under the headings of the sections in that menu (by
//!   order), and a switch for advanced settings.
//! - A group page lists its settings with their values and units, each
//!   with its description below it, so nothing has to be tapped (and
//!   changed) to find out what it does; settings that name a section
//!   are listed under its heading.
//! - Choices and languages open a list to pick from.
//! - Each page ends with resetting its settings to their defaults, the
//!   main page with resetting all of them (but the language); both ask
//!   first.
//!
//! Changes apply immediately and are saved to `paperpaw.ini` by the
//! kernel.
//!
//! - On touch screens, numbers marked as sliders (`SettingExtra::slider`)
//!   show one below their title (`ui::slider`); with buttons alone they
//!   keep their compact row with − and +.
//!
//! Buttons: move to an item, *confirm* opens a group, toggles a switch,
//! opens a choice or starts changing a number; then left/right (or
//! up/down, page buttons) change it, shown at once but not kept yet,
//! *confirm* keeps it and *back* goes back to the value before.
//! Touch: tap an item to open, toggle or choose it; numbers show their −
//! and + buttons all the time, a tap on one changes the number at once;
//! a slider's knob is dragged, and kept where the finger is lifted.
//!
//! The main page is bound to the `core.settings` slot.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

mod chooser;
mod group_list;
mod page;
mod search;
mod secrets;
mod themes;
mod track;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, group_sections, groups, menus, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::widgets::Widget;
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, FixedScreen, Gesture, GestureKind, Handled, Hint, Id, Level,
    Manifest, Params, Plugin, PointerEvent, PointerPhase, Priority, Rect, RefreshMode,
    RegisterError, Registrar, Screen, SettingKind, SettingSpec, SettingValue,
};
use paperpaw_sdk::{FixedAction, MenuEntry};

use self::chooser::*;
use self::group_list::*;
use self::page::*;

pub const PLUGIN: Id = Id::new_static("settings");
/// The main page: setting groups.
pub const SCREEN: Id = Id::new_static("settings.list");
/// The settings of one group; parameter `id`.
pub const GROUP_SCREEN: Id = Id::new_static("settings.group");
/// Picks a value of a choice or language setting; parameter `id`.
pub const CHOOSE_SCREEN: Id = Id::new_static("settings.choose");
/// Show advanced settings too.
pub const ADVANCED: Setting<bool> = Setting::new("settings.advanced");

/// The answer: the group reset (its id; empty for all) and the option.
const CHOSEN: Event<choice::Picked> = Event::new("settings.chosen");

/// What a choice is about when it asks whether to restart.
const RESTART: &str = "restart";

const ID: &str = "id";
/// `1`: a group's advanced settings, one level down from its page
/// (parameter of [`GROUP_SCREEN`]).
pub const MORE: &str = "more";
/// A setting to focus on its group's page (parameter of [`SCREEN`] and
/// [`GROUP_SCREEN`]).
const FOCUS: &str = paperpaw_sdk::builtin::params::FOCUS;

/// The settings plugin.
#[derive(Debug, Default)]
pub struct SettingsPlugin;

/// Whether advanced settings show.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::switch(
    ADVANCED.static_str(),
    "settings.advanced",
    false,
    groups::SYSTEM,
)
.description("settings.advanced.help")];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(secrets::OPEN, "settings.secrets")
    .handler(|_, _, out| out.push(Command::OpenScreen(secrets::SCREEN, Params::new())))];
/// In the device's part, after the system.
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::SETTINGS, secrets::OPEN).order(95)];
static ICONS: [(Id, &str); 1] = [(secrets::OPEN, "key")];

static SCREENS: [FixedScreen; 7] = [
    FixedScreen::new(SCREEN, "settings.title", |env, params| {
        let group = params.get(ID).and_then(|id| Id::parse(id).ok());
        // A setting to focus: on its page, unless a group is named.
        let focus = params.get(FOCUS).and_then(|id| env.settings().spec(id));
        let group = group.or_else(|| focus.map(|spec| env.settings().group_of(spec)));
        match group {
            Some(group) => {
                let mut page = GroupPage::new(env, group);
                if let Some(spec) = focus {
                    page.focus_on(env, spec.id.as_str());
                }
                Box::new(page) as Box<dyn Screen>
            }
            None => Box::new(GroupList::new(env)),
        }
    }),
    FixedScreen::new(GROUP_SCREEN, "settings.title", |env, params| {
        let group = params.get(ID).and_then(|id| Id::parse(id).ok());
        let more = params.get(MORE) == Some("1");
        let mut page = GroupPage::of(env, group.unwrap_or(groups::OTHER), more);
        if let Some(focus) = params.get(FOCUS) {
            page.focus_on(env, focus);
        }
        Box::new(page)
    }),
    FixedScreen::new(themes::SCREEN, "settings.theme", |env, params| {
        Box::new(themes::Themes::new(env, params))
    }),
    FixedScreen::new(CHOOSE_SCREEN, "settings.title", |env, params| {
        let id = params.get(ID).unwrap_or_default();
        Box::new(Chooser::new(env, id))
    }),
    FixedScreen::new(search::SCREEN, "settings.search", |env, params| {
        Box::new(search::SearchScreen::new(env, params))
    }),
    FixedScreen::new(secrets::SCREEN, "settings.secrets", |env, _| {
        Box::new(secrets::SecretList::new(env))
    }),
    FixedScreen::new(secrets::SECRET_SCREEN, "settings.secrets", |env, params| {
        Box::new(secrets::SecretPage::new(env, params))
    }),
];

impl Plugin for SettingsPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Settings", paperpaw_sdk::package_version!())
            .description("The settings pages, built from the settings of every plugin.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.on(secrets::ENTERED, secrets::entered)?;
        r.bind_slot(slots::SETTINGS, SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::CHOOSE_VALUE, CHOOSE_SCREEN, Priority::DEFAULT)?;
        r.add_settings(&SETTINGS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Whether a setting is shown with the current level.
/// Asks whether to reset the settings of `group` (all for `None`).
fn ask_reset(env: &Env<'_>, group: Option<(&Id, &str)>) -> Command {
    let question = match group {
        Some((_, title)) => env.i18n().format(
            "settings.reset-page.question",
            &[("page", paperpaw_sdk::Arg::Str(title))],
        ),
        None => env.i18n().tr("settings.reset-all.question").to_string(),
    };
    let context = group.map_or("", |(id, _)| id.as_str());
    paperpaw_sdk::ask::choice(&question, &CHOSEN)
        .about(context)
        .options("settings.option.", &["reset", "cancel"])
        .command()
}

/// Resets the settings `reset` chose: of its group, or all but the
/// language; the reader's own (accounts, folders, passwords) stay.
fn reset(env: &Env<'_>, picked: &choice::Picked, cx: &mut Cx<'_, '_>) {
    if picked.option != "reset" {
        return;
    }
    let group = Id::parse(&picked.path).ok();
    // A placement's: what it set goes, the widget's own values apply.
    if let Some(group) = group.as_ref().filter(|g| g.as_str().contains('@')) {
        for value in env.settings().placement_values(group.as_str()) {
            cx.command(Command::ResetSetting(value));
        }
        cx.command(Command::Notify(
            env.i18n().tr("settings.reset-done").to_string(),
        ));
        return;
    }
    let language = paperpaw_sdk::builtin::settings::LANGUAGE;
    let scope_only = paperpaw_sdk::builtin::settings::SCOPE_ONLY;
    let mut reset_scope = false;
    for spec in env
        .settings()
        .specs()
        .filter(|spec| !spec.is_kept_on_reset() && reset_has_value(env.settings(), &spec.id))
    {
        let chosen = match &group {
            Some(group) => env.settings().group_of(spec) == *group && visible(env, spec),
            None => spec.id != *language.id(),
        };
        if chosen {
            if spec.id == *scope_only.id() {
                // Clearing this first would send later scoped resets
                // to global settings instead of the book's values.
                reset_scope = true;
            } else {
                cx.command(Command::ResetSetting(spec.id.clone()));
            }
        }
    }
    if reset_scope {
        cx.command(Command::ResetSetting(scope_only.id().clone()));
    }
    cx.command(Command::Notify(
        env.i18n().tr("settings.reset-done").to_string(),
    ));
}

/// Unset defaults need no disk work, font reload or change event. A
/// book's reset removes only values kept in the scope it changes.
fn reset_has_value(settings: &paperpaw_sdk::Settings, id: &Id) -> bool {
    if settings.goes_to_scope(id.as_str()) {
        settings.scope().is_some_and(|scope| scope.contains_key(id))
    } else {
        settings.is_set(id.as_str())
    }
}

fn visible(env: &Env<'_>, spec: &SettingSpec) -> bool {
    offered(env, spec)
        && (spec.level == Level::Basic
            || env.settings().read(&ADVANCED)
            || env.settings().group_of(spec) == groups::DEVELOPER)
}

/// Whether `spec` is offered here at all, advanced or not: it means
/// something now and on this device.
fn offered(env: &Env<'_>, spec: &SettingSpec) -> bool {
    // "Only this book" and the like mean nothing outside a scope.
    let scope_only = spec.id == paperpaw_sdk::builtin::settings::SCOPE_ONLY;
    if scope_only && env.settings().scope().is_none() {
        return false;
    }
    // Nothing to choose (e.g. no plugin frames pages otherwise).
    if let paperpaw_sdk::SettingKind::Choice(choices) = &spec.kind
        && choices.len() < 2
    {
        return false;
    }
    // Not for this device (e.g. reading aloud without a speaker), or not
    // while another setting has another value.
    if !env.menus().available(spec.id.as_str()) || !env.settings().shows(spec) {
        return false;
    }
    // For developers, once they asked for it; then without the advanced
    // switch.
    if env.settings().group_of(spec) == groups::DEVELOPER {
        return env
            .settings()
            .bool(paperpaw_sdk::builtin::settings::DEVELOPER.as_str());
    }
    true
}

/// The header, list area and list for a screen; rows have notes if
/// `noted`.
fn frame(env: &Env<'_>, noted: bool) -> (UiStyle, ui::Header, ListView) {
    let (style, header, area) = page(env);
    let row = if noted {
        style.noted_row()
    } else {
        u32::from(style.row())
    };
    (style, header, ListView::new(area, row).focus_on_keys(env))
}

/// The style, header and the area below it for the list of a page.
fn page(env: &Env<'_>) -> (UiStyle, ui::Header, Rect) {
    let style = UiStyle::new(env);
    let display = env.device().display;
    let header = ui::Header::new(env, &style, true);
    let top = header.area().height + style.gap();
    let height = display.height.saturating_sub(top);
    (style, header, Rect::new(0, top, display.width, height))
}

/// Moves the focus of `rows` (lists with headings) by the navigation
/// actions; `None` if `action` is another. The first press on a touch
/// screen only shows the focus (`keys`), which taps hide.
fn navigate_rows(rows: &mut ui::Rows, keys: &mut bool, action: &Id) -> Option<ListChange> {
    let forward =
        if *action == actions::UP || *action == actions::LEFT || *action == actions::PREVIOUS {
            Some(false)
        } else if *action == actions::DOWN || *action == actions::RIGHT || *action == actions::NEXT
        {
            Some(true)
        } else {
            None
        };
    let page = if *action == actions::NEXT_PAGE {
        Some(true)
    } else if *action == actions::PREV_PAGE {
        Some(false)
    } else {
        None
    };
    if let Some(forward) = page {
        // Pages turn whether the focus shows or not.
        return Some(rows.turn(forward));
    }
    let forward = forward?;
    if !*keys {
        *keys = true;
        return Some(rows.focus_change());
    }
    Some(rows.step(forward))
}

/// Shows the values kept again, if values were shown without being kept.
fn end_preview(env: &Env<'_>, cx: &mut Cx<'_, '_>) {
    if env.settings().previewing() {
        cx.command(Command::PreviewSettings(Vec::new()));
    }
}

/// Whether a page is covered by a full screen and settings changed
/// meanwhile: it is made anew once it shows, not for each change.
#[derive(Debug, Default, Clone, Copy)]
struct Covered {
    hidden: bool,
    stale: bool,
}

impl Covered {
    /// A setting changed: `true` if the page is made anew now. Covered,
    /// it is noted and a timer asked for, which comes once the page shows
    /// again (see the pages' `on_timer`).
    fn changed(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.hidden && !self.stale {
            let now = cx.env().now();
            cx.wake_at(now);
        }
        self.stale |= self.hidden;
        !self.hidden
    }

    fn hide(&mut self) {
        self.hidden = true;
    }

    /// The page shows again: `true` if it is to be made anew.
    fn shown(&mut self) -> bool {
        self.hidden = false;
        core::mem::take(&mut self.stale)
    }
}

/// The page shown in a header: its number and the number of pages.
fn paging(rows: &ui::Rows) -> Option<ui::Paging> {
    Some(rows.paging())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn reset_uses_the_value_of_its_target_scope_not_the_global_value() {
        let id = Id::new_static("test.count");
        let only = paperpaw_sdk::builtin::settings::SCOPE_ONLY;
        let mut settings = paperpaw_sdk::Settings::new();
        settings
            .define(SettingSpec::number("test.count", "Count", 1, 20, 2, groups::OTHER).scoped())
            .unwrap();
        settings
            .define(
                SettingSpec::switch(
                    only.clone().static_str(),
                    "Only this book",
                    false,
                    groups::OTHER,
                )
                .scoped(),
            )
            .unwrap();
        assert!(!reset_has_value(&settings, &id));
        settings.set(id.as_str(), SettingValue::Int(9)).unwrap();
        assert!(reset_has_value(&settings, &id));
        let mut scope = paperpaw_sdk::collections::VecMap::new();
        scope.insert(only.id().clone(), SettingValue::Bool(true));
        settings.set_scope(Some(scope));
        assert!(
            !reset_has_value(&settings, &id),
            "the global value is outside this reset"
        );
        settings
            .set_in_scope(id.as_str(), SettingValue::Int(2))
            .unwrap();
        assert!(
            reset_has_value(&settings, &id),
            "an explicitly stored default still resets"
        );
        settings.reset(id.as_str()).unwrap();
        assert!(!reset_has_value(&settings, &id));
        assert_eq!(settings.int(id.as_str()), 9, "the global value remains");
    }
}
