//! The font of each widget (#262): `core.widgets.font` for all of them and
//! `<widget>.font` for one, which the host gives every widget native plugins
//! declared that has settings of its own (a group of the widget's id). A
//! widget draws in it through the font key `widget:<id>`
//! (`abi/screens-input.md`); the families chosen are kept open.
//!
//! Of a widget's setting only its default, its flags and its name stay on
//! the heap (`paperpaw_sdk::Described`): the spec, with the families offered
//! now, is made when it is asked for.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::{
    DescribeError, Described, DescribedSettings, FontError, FontHandle, Id, SettingKind,
    SettingSpec, SettingValue, Settings,
};

use super::{choice_list, fallible_string};
use crate::Kernel;
use crate::native_host::registry_service::fixed::Fixed;
use crate::native_host::shared::{Shared, try_box};

/// The font of every widget whose own setting says [`ALL`].
pub(crate) const GLOBAL: &str = "core.widgets.font";
/// What follows a widget's id in the id of its own setting.
pub(crate) const SUFFIX: &str = ".font";
/// A widget's own: as `core.widgets.font` says (the default).
pub(crate) const ALL: &str = "core.widgets.font.all";
/// The menus' font (the default of `core.widgets.font`).
pub(crate) const MENUS: &str = "core.widgets.font.menus";
/// The book's font.
pub(crate) const BOOK: &str = "core.widgets.font.book";

/// The most families kept open for widgets beside the menus' and the book's:
/// each open pack keeps its index on the heap. Widgets set in one more show
/// in the menus' font.
pub(crate) const MOST_KEPT: usize = 3;

/// The most bytes of a setting id.
const KEY: usize = 192;
/// How far past its largest size a font pack still sets text (as a larger
/// face is chosen, `paperpaw_sdk::Fonts`).
const PAST_LARGEST: f32 = 2.0;

/// What a widget is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chosen<'a> {
    Menus,
    Book,
    /// A family by its name.
    Family(&'a str),
}

/// What `widget` (its id, with a placement's `@n`) is set in: its own
/// setting, or where that says [`ALL`] or there is none, the one for all.
pub(crate) fn chosen<'a>(settings: &'a Settings, widget: &str) -> Chosen<'a> {
    let own = Fixed::<KEY>::of(&[widget, SUFFIX]);
    let own = own
        .as_ref()
        .and_then(|id| settings.get(id.as_str()))
        .and_then(text)
        .filter(|value| !value.is_empty() && *value != ALL);
    match own.unwrap_or_else(|| settings.text(GLOBAL)) {
        BOOK => Chosen::Book,
        MENUS | ALL | "" => Chosen::Menus,
        family => Chosen::Family(family),
    }
}

fn text(value: &SettingValue) -> Option<&str> {
    match value {
        SettingValue::Text(text) => Some(text),
        _ => None,
    }
}

/// Whether `font` sets text of `size` pixels: a font pack only up to a
/// little past its largest size (the X4's card fonts are 20 to 36 pixels).
pub(crate) fn has_size(font: &FontHandle<'_>, size: f32) -> bool {
    font.largest_size()
        .is_none_or(|largest| size < largest + PAST_LARGEST)
}

/// Whether `id` is a setting that says what widgets are set in.
pub(crate) fn is_setting(id: &str) -> bool {
    id == GLOBAL || id.ends_with(SUFFIX)
}

/// The families offered for widgets now: shared by the kernel, which keeps
/// them up to date, and the settings of every plugin's widgets.
pub(crate) type Families = Shared<RefCell<Vec<String>>>;

/// The font settings of one plugin's widgets.
struct WidgetFonts {
    plugin: Id,
    /// The widgets' names (their ids after `<plugin>.`), a line each.
    names: String,
    families: Families,
}

impl WidgetFonts {
    fn widget(&self, at: usize) -> Result<&str, DescribeError> {
        self.names.split('\n').nth(at).ok_or(DescribeError::Gone)
    }
}

impl Described for WidgetFonts {
    fn count(&self) -> usize {
        self.names.split('\n').count()
    }

    fn name(&self, at: usize, visit: &mut dyn FnMut(&str)) -> Result<(), DescribeError> {
        let name = Fixed::<KEY>::of(&[self.widget(at)?, SUFFIX]).ok_or(DescribeError::Gone)?;
        visit(name.as_str());
        Ok(())
    }

    fn make(&self, at: usize) -> Result<SettingSpec, DescribeError> {
        let widget = self.widget(at)?;
        let id = |parts: &[&str]| {
            let id = fallible_string(parts).ok_or(DescribeError::NoMemory)?;
            Id::try_parse(&id).map_err(|error| match error {
                paperpaw_sdk::IdError::Invalid => DescribeError::Gone,
                _ => DescribeError::NoMemory,
            })
        };
        let families = self
            .families
            .try_borrow()
            .map_err(|_| DescribeError::Gone)?;
        let choices = choice_list(&[ALL, MENUS, BOOK], families.iter().map(String::as_str))
            .ok_or(DescribeError::NoMemory)?;
        // In the widget's own group, after what the plugin declared there.
        Ok(SettingSpec::new(
            id(&[self.plugin.as_str(), ".", widget, SUFFIX])?,
            "core.setting.font",
            SettingKind::Choice(choices.into()),
            SettingValue::Text(Cow::Borrowed(ALL)),
            id(&[self.plugin.as_str(), ".", widget])?,
        )
        .description("core.setting.font.help"))
    }
}

/// The widgets of one plugin that get a font setting, while they are found.
struct Found {
    plugin: Id,
    names: String,
}

/// Notes the declared widget `id` in `found` if it gets a font setting: it
/// has settings (a group of its id) and no `<id>.font` of its plugin's own.
/// `None` where the heap has no room.
fn note(settings: &Settings, found: &mut Vec<Found>, id: &str) -> Option<()> {
    let Some((plugin, name)) = id.split_once('.') else {
        return Some(());
    };
    let Some(font) = Fixed::<KEY>::of(&[id, SUFFIX]) else {
        return Some(());
    };
    if settings.defines(font.as_str()) || !settings.groups().any(|group| group.id.as_str() == id) {
        return Some(());
    }
    if found
        .last()
        .is_none_or(|last| last.plugin.as_str() != plugin)
    {
        found.try_reserve(1).ok()?;
        found.push(Found {
            plugin: Id::try_parse(plugin).ok()?,
            names: String::new(),
        });
    }
    let names = &mut found.last_mut()?.names;
    names.try_reserve(name.len() + 1).ok()?;
    if !names.is_empty() {
        names.push('\n');
    }
    names.push_str(name);
    Some(())
}

/// The settings of `found`'s widgets, ready to be defined.
fn described(found: Found, families: &Families) -> Result<DescribedSettings, DescribeError> {
    let source = WidgetFonts {
        plugin: found.plugin.try_clone().ok_or(DescribeError::NoMemory)?,
        // No more room than the names take.
        names: fallible_string(&[&found.names]).ok_or(DescribeError::NoMemory)?,
        families: families.try_clone().ok_or(DescribeError::NoMemory)?,
    };
    let source: Box<dyn Described> = try_box(source).ok_or(DescribeError::NoMemory)?;
    DescribedSettings::new(found.plugin, source)
}

impl Kernel {
    /// Gives the widgets native plugins declared their font settings, those
    /// that have none yet: after each plugin was admitted, and once more
    /// when the start has hosted them all (the values the configuration has
    /// for them are set by the caller). Where the heap has no room a warning
    /// says so and the widgets left out follow `core.widgets.font`.
    pub(crate) fn offer_widget_fonts(&mut self) {
        if self.try_offer_widget_fonts().is_none() {
            self.try_warn_runtime(&[
                "not enough memory for the font setting of every widget; those without follow ",
                GLOBAL,
            ]);
        }
        // What was made to check them is not kept.
        self.rest_settings();
    }

    /// The start has hosted its native plugins: the settings a shortage
    /// left out, the values that waited for a family a later plugin brought
    /// (now refused with a warning if it is not there), and the families
    /// chosen open.
    pub(crate) fn widget_fonts_hosted(&mut self) {
        self.widget_fonts_admitted();
        self.apply_widget_fonts();
    }

    /// A native plugin was admitted: its widgets get their font settings and
    /// the values the configuration has for them. At the start this is
    /// before the shell reports the warnings, so "unknown setting" is taken
    /// back in time.
    pub(crate) fn widget_fonts_admitted(&mut self) {
        self.offer_widget_fonts();
        self.apply_late_config();
        // The specs the values were checked against are not kept.
        self.rest_settings();
    }

    fn try_offer_widget_fonts(&mut self) -> Option<()> {
        let Some(declared) = self.shared.registry.native_declarations.as_ref() else {
            return Some(());
        };
        let mut found: Vec<Found> = Vec::new();
        let mut room = Some(());
        let settings = &self.shared.settings;
        crate::native_host::registry_service::declared_widget_names(
            declared,
            None,
            &mut |id, _, _| {
                if room.is_some() {
                    room = note(settings, &mut found, id);
                }
            },
        );
        if found.is_empty() {
            return room;
        }
        let families = self.widget_font_families()?;
        for plugin in found {
            let described = match described(plugin, &families) {
                Ok(described) => described,
                Err(DescribeError::NoMemory) => return None,
                // Nothing of a plugin's own data is read: an id that is none.
                Err(_) => continue,
            };
            let settings = &mut self.shared.settings;
            settings.try_reserve_described(described.len(), 0).ok()?;
            // A name the plugin declared meanwhile, or no place left: the
            // widgets follow the setting for all.
            let _ = settings.define_described(described);
        }
        room
    }

    /// The families offered for widgets, made at the first use.
    fn widget_font_families(&mut self) -> Option<Families> {
        if self.shared.registry.widget_fonts.is_none() {
            let families = Shared::try_new(RefCell::new(Vec::new())).ok()?;
            self.shared.registry.widget_fonts = Some(families);
            self.offer_widget_families();
        }
        self.shared.registry.widget_fonts.as_ref()?.try_clone()
    }

    /// The families the widgets' own settings offer from now on: those
    /// `core.widgets.font` offers. Where the heap has no room they stay.
    pub(crate) fn offer_widget_families(&mut self) {
        let Some(shared) = self.shared.registry.widget_fonts.as_ref() else {
            return;
        };
        let offered = match self.shared.settings.spec(GLOBAL).map(|spec| &spec.kind) {
            Some(SettingKind::Choice(choices)) => choices,
            _ => return,
        };
        let fixed = |value: &&str| ![MENUS, BOOK].contains(value);
        let mut families: Vec<String> = Vec::new();
        for family in offered.iter().filter(fixed) {
            let made = families
                .try_reserve(1)
                .ok()
                .and_then(|()| fallible_string(&[family]));
            match made {
                Some(family) => families.push(family),
                None => return,
            }
        }
        if let Ok(mut kept) = shared.try_borrow_mut() {
            *kept = families;
        }
        // A spec made with the families of before is let go.
        self.shared.settings.rest();
    }

    /// Opens the families widgets are set in (`core.widgets.font` and every
    /// `<widget>.font` the reader chose) and closes those set in no more.
    /// Where the heap is short the ones open stay, and a widget whose
    /// family is not open shows in the menus' font.
    pub(crate) fn apply_widget_fonts(&mut self) {
        match self.try_apply_widget_fonts() {
            Ok(true) => {}
            Ok(false) => self.try_warn_runtime(&[
                "more fonts chosen for widgets than are kept open at once; \
                 the widgets of the others show in the menus' font",
            ]),
            Err(_) => self.try_warn_runtime(&["not enough memory to open the widgets' fonts; \
                 widgets whose font is not open show in the menus' font"]),
        }
    }

    /// `false` where more families are chosen than are kept open.
    fn try_apply_widget_fonts(&mut self) -> Result<bool, FontError> {
        let settings = &self.shared.settings;
        let fonts = &self.shared.fonts;
        let storage = self.shared.storage.as_ref();
        let mut kept: Vec<Id> = Vec::new();
        kept.try_reserve_exact(MOST_KEPT)
            .map_err(|_| FontError::OutOfMemory)?;
        let mut all = true;
        let ids = settings
            .valued()
            .map(Id::as_str)
            .filter(|id| id.ends_with(SUFFIX));
        for id in core::iter::once(GLOBAL).chain(ids) {
            let family = match settings.get(id).and_then(text) {
                Some(ALL | MENUS | BOOK | "") | None => continue,
                Some(family) => family,
            };
            if !fonts.family_there(family, storage) {
                continue;
            }
            let Some(font) = fonts.family_regular(family) else {
                continue;
            };
            if kept.contains(font) {
                continue;
            }
            if kept.len() == MOST_KEPT {
                all = false;
                continue;
            }
            kept.push(font.try_clone().ok_or(FontError::OutOfMemory)?);
        }
        let failed = self
            .shared
            .fonts
            .try_keep_open(&kept, &self.shared.storage)?;
        self.warn_fonts_failed(failed);
        Ok(all)
    }
}
