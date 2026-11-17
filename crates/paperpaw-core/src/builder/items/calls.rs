//! What is added to PaperPaw: one contribution each, collected by the
//! [`Items`] a registering function is given ([`KernelBuilder::items`], the
//! kernel's own producers, an old [`Plugin`](super::Plugin)).
//!
//! `Items` only collects; the kernel applies the contributions once the
//! function is done (and parses key maps and translations then). Until
//! #244's slice D4b this was the SDK's `Registrar`: the calls and their
//! rules are unchanged.
//!
//! [`KernelBuilder::items`]: crate::KernelBuilder::items

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::setup::SetupStepSpec;
use paperpaw_sdk::status::{StatusItemSpec, StatusLineSpec};
use paperpaw_sdk::widgets::WidgetSpec;
use paperpaw_sdk::{
    ActionSpec, FontFace, FontSpec, FormatSpec, Id, MenuEntry, MenuNeed, MenuSection, Priority,
    RegisterError, SettingSpec, SlotSpec,
};

use paperpaw_sdk::Contribution;

/// Collects a plugin's contributions.
///
/// Every id a plugin adds must be in its namespace (`<plugin>.…`), and
/// once per kind; bindings, menu entries and key maps may name what other
/// plugins added.
#[derive(Debug)]
pub struct Items {
    plugin: Id,
    contributions: Vec<Contribution>,
    /// The host's own screens.
    kernel_screens: Vec<&'static [crate::host_screen::HostEntry]>,
    /// Tests' screens (feature `testing`).
    layers: Vec<crate::FakeSpec>,
    /// Handlers of events.
    hears: Vec<crate::registry::Subscribed>,
    /// Jobs: the steps of declared work.
    works: Vec<crate::work::WorkSpec>,
    /// The steps of a data plugin's actions, by action.
    data_actions: Vec<crate::registry::DataAction>,
}

/// What a plugin added, taken apart for the kernel to apply.
pub(crate) struct Parts {
    pub(crate) contributions: Vec<Contribution>,
    pub(crate) kernel_screens: Vec<&'static [crate::host_screen::HostEntry]>,
    pub(crate) layers: Vec<crate::FakeSpec>,
    pub(crate) hears: Vec<crate::registry::Subscribed>,
    pub(crate) works: Vec<crate::work::WorkSpec>,
    pub(crate) data_actions: Vec<crate::registry::DataAction>,
}

impl Items {
    /// A registrar for plugin `plugin`.
    pub fn new(plugin: impl Into<Id>) -> Self {
        let plugin = plugin.into();
        Self {
            plugin,
            contributions: Vec::new(),
            kernel_screens: Vec::new(),
            layers: Vec::new(),
            hears: Vec::new(),
            works: Vec::new(),
            data_actions: Vec::new(),
        }
    }

    /// The id of the plugin being registered.
    pub fn plugin(&self) -> &Id {
        &self.plugin
    }

    /// What was added so far, in order.
    pub fn contributions(&self) -> &[Contribution] {
        &self.contributions
    }

    /// What was added, for the kernel to apply.
    pub fn into_contributions(self) -> Vec<Contribution> {
        self.contributions
    }

    /// What was added, the host's own screens
    /// ([`add_host_screens`](Self::add_host_screens)) and tests' screens.
    pub(crate) fn into_parts(self) -> Parts {
        Parts {
            contributions: self.contributions,
            kernel_screens: self.kernel_screens,
            layers: self.layers,
            hears: self.hears,
            works: self.works,
            data_actions: self.data_actions,
        }
    }

    /// Adds `contribution`; every method below is a shorthand for one.
    pub fn add(&mut self, contribution: Contribution) -> Result<(), RegisterError> {
        self.add_checked(contribution)
    }

    /// As [`add`](Self::add), for the shorthands below.
    pub(crate) fn add_checked(&mut self, contribution: Contribution) -> Result<(), RegisterError> {
        // What an entry shows is said by the plugin it belongs to.
        if let Contribution::ItemState { item, .. } = &contribution
            && !item.is_owned_by(&self.plugin)
        {
            return Err(RegisterError::Namespace { id: item.clone() });
        }
        // The clean-up removes what is named like a book's key there: a
        // folder outside PaperPaw's own would take the reader's files.
        if let Contribution::BookData(data) = &contribution
            && !in_own_folder(&data.folder)
        {
            return Err(RegisterError::Invalid {
                message: Cow::Owned(alloc::format!(
                    "book data in `{}`, outside `{}/`",
                    data.folder,
                    paperpaw_sdk::data::DIR
                )),
            });
        }
        if let Some(id) = contribution.id() {
            if !id.is_owned_by(&self.plugin) {
                return Err(RegisterError::Namespace { id: id.clone() });
            }
            let kind = contribution.id_kind();
            if self
                .contributions
                .iter()
                .any(|c| c.id_kind() == kind && c.id() == Some(id))
            {
                return Err(RegisterError::Duplicate { id: id.clone() });
            }
        }
        self.contributions
            .try_reserve(1)
            .map_err(|_| RegisterError::Invalid {
                message: Cow::Borrowed("contribution allocation failed"),
            })?;
        self.contributions.push(contribution);
        Ok(())
    }

    pub fn add_action(&mut self, action: ActionSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Action(Box::new(action)))
    }

    /// Adds the host's own screens (`host_screen::ENTRIES`).
    pub(crate) fn add_host_screens(
        &mut self,
        screens: &'static [crate::host_screen::HostEntry],
    ) -> Result<(), RegisterError> {
        self.check_screens(screens.iter().map(crate::host_screen::HostEntry::id))?;
        self.kernel_screens.push(screens);
        Ok(())
    }

    /// Adds a test's screen, a native screen whose plugin is the test (see
    /// [`testing`](crate::testing)).
    #[cfg(any(test, feature = "testing"))]
    pub fn add_fake(&mut self, screen: crate::testing::FakeSpec) -> Result<(), RegisterError> {
        self.check_screens(core::iter::once(screen.id()))?;
        self.layers.push(screen);
        Ok(())
    }

    /// Each of `ids` in the plugin's namespace, once among its screens.
    fn check_screens<'a>(
        &self,
        ids: impl Iterator<Item = &'a Id> + Clone,
    ) -> Result<(), RegisterError> {
        let added = |id: &Id| {
            self.kernel_screens
                .iter()
                .any(|list| list.iter().any(|s| s.id() == id))
                || self.layers.iter().any(|s| s.id() == id)
        };
        for (n, id) in ids.clone().enumerate() {
            if !id.is_owned_by(&self.plugin) {
                return Err(RegisterError::Namespace { id: id.clone() });
            }
            if added(id) || ids.clone().take(n).any(|other| other == id) {
                return Err(RegisterError::Duplicate { id: id.clone() });
            }
        }
        Ok(())
    }

    pub fn add_slot(&mut self, slot: SlotSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Slot(slot))
    }

    /// Proposes `screen` for `slot`. The binding with the highest priority
    /// wins unless the user configured the slot explicitly.
    pub fn bind_slot(
        &mut self,
        slot: impl Into<Id>,
        screen: impl Into<Id>,
        priority: Priority,
    ) -> Result<(), RegisterError> {
        let slot = slot.into();
        let screen = screen.into();
        self.add_checked(Contribution::SlotBinding {
            slot,
            screen,
            priority,
        })
    }

    pub fn add_setting(&mut self, setting: SettingSpec) -> Result<(), RegisterError> {
        let fixed = self.contributions.iter().any(|c| {
            matches!(c, Contribution::FixedSettings(list) if list.iter().any(|s| s.id == setting.id))
        });
        if fixed {
            return Err(RegisterError::Duplicate { id: setting.id });
        }
        self.add_checked(Contribution::Setting(Box::new(setting)))
    }

    /// Adds settings kept in the program (a `static` list), e.g. a
    /// `static` list of a widget's: they take next to no memory, where
    /// each added with [`add_setting`](Self::add_setting) takes about
    /// 200 bytes.
    pub fn add_settings(&mut self, settings: &'static [SettingSpec]) -> Result<(), RegisterError> {
        // Each in the plugin's namespace, and once.
        let added = |id: &Id| {
            self.contributions.iter().any(|c| match c {
                Contribution::Setting(s) => s.id == *id,
                Contribution::FixedSettings(list) => list.iter().any(|s| s.id == *id),
                _ => false,
            })
        };
        for (n, setting) in settings.iter().enumerate() {
            let id = &setting.id;
            if !id.is_owned_by(&self.plugin) {
                return Err(RegisterError::Namespace { id: id.clone() });
            }
            if added(id) || settings[..n].iter().any(|s| s.id == *id) {
                return Err(RegisterError::Duplicate { id: id.clone() });
            }
        }
        self.contributions
            .push(Contribution::FixedSettings(settings));
        Ok(())
    }

    pub fn add_keymap(
        &mut self,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let source = source.into();
        self.add_checked(Contribution::Keymap(source))
    }

    /// A stylesheet for books, applied after a book's own (where the
    /// format reads CSS, e.g. EPUB), e.g. for a theme that sets books
    /// apart; see [`Env::stylesheets`](paperpaw_sdk::Env).
    pub fn add_stylesheet(
        &mut self,
        css: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let css = css.into();
        self.add_checked(Contribution::Stylesheet(css))
    }

    /// A folder where the plugin keeps the reader's data for each book
    /// (a place, notes, settings for the book), named by the book's key
    /// ([`library::key`](paperpaw_sdk::library::key)) after `prefix` (alone, or
    /// followed by `.` and more, a file or a folder): the kernel moves it
    /// with its book and removes it with it. What is left of books no
    /// longer found goes only when the reader agrees: a book renamed on a
    /// computer or on a card not inserted is not found either
    /// ([`library::BookData`](paperpaw_sdk::library::BookData)). For what is
    /// made again from the book, [`add_book_cache`](Self::add_book_cache).
    /// The folder must be inside `.paperpaw/` (usually
    /// [`Env::plugin_dir`](paperpaw_sdk::Env::plugin_dir)): one elsewhere is
    /// refused with [`RegisterError::Invalid`], so that no file of the
    /// reader's is taken for a book's data.
    pub fn add_book_data(
        &mut self,
        folder: impl Into<Cow<'static, str>>,
        prefix: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::BookData(
            paperpaw_sdk::library::BookData::new(
                folder.into(),
                prefix.into(),
                paperpaw_sdk::library::BookDataKind::UserData,
            ),
        ))
    }

    /// As [`add_book_data`](Self::add_book_data), for what the plugin
    /// makes again from the book when needed (pages laid out, a cover
    /// made small): what is left of books no longer found is cleaned up
    /// by itself.
    pub fn add_book_cache(
        &mut self,
        folder: impl Into<Cow<'static, str>>,
        prefix: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::BookData(
            paperpaw_sdk::library::BookData::new(
                folder.into(),
                prefix.into(),
                paperpaw_sdk::library::BookDataKind::Cache,
            ),
        ))
    }

    /// Adds a key map preset (same format as [`Items::add_keymap`]) that
    /// users can select with the `core.keymap-preset` setting. Its title
    /// is the translation of its id.
    pub fn add_keymap_preset(
        &mut self,
        id: impl Into<Id>,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let id = id.into();
        let source = source.into();
        self.add_checked(Contribution::KeymapPreset { id, source })
    }

    /// Adds a font. Bitmap packs work everywhere; scalable fonts need the
    /// `outline` feature.
    pub fn add_font(&mut self, font: FontSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Font(Box::new(font)))
    }

    /// Adds a font file on the storage (OpenType/TrueType): its family and
    /// style are read from its header, and it is loaded only when its
    /// family is chosen for reading. Needs scalable font support.
    pub fn add_font_file(
        &mut self,
        id: impl Into<Id>,
        path: impl Into<String>,
    ) -> Result<(), RegisterError> {
        let id = id.into();
        let path = path.into();
        self.add_checked(Contribution::FontFile { id, path })
    }

    /// Adds a font pack on the storage (`.ppfp`) with characters font
    /// `extends` lacks, e.g. Cyrillic for a font made for Latin; read a
    /// glyph at a time (see `Fonts::add_extension`).
    pub fn add_font_extension(
        &mut self,
        id: impl Into<Id>,
        path: impl Into<String>,
        extends: impl Into<Id>,
    ) -> Result<(), RegisterError> {
        let id = id.into();
        let path = path.into();
        let extends = extends.into();
        self.add_checked(Contribution::FontExtension { id, path, extends })
    }

    /// Adds a face of a family in a file on the storage: a font pack
    /// (`.ppfp`), drawn on every device, or an OpenType/TrueType file,
    /// drawn where scalable fonts are (see [`FontFace`]). It is loaded
    /// only while its family is chosen, for reading or the status lines.
    pub fn add_font_face(&mut self, face: FontFace) -> Result<(), RegisterError> {
        self.add_checked(Contribution::FontFace(Box::new(face)))
    }

    /// Proposes a font for a role such as
    /// [`builtin::fonts::UI`](paperpaw_sdk::builtin::fonts::UI).
    pub fn bind_font(
        &mut self,
        role: impl Into<Id>,
        font: impl Into<Id>,
        priority: Priority,
    ) -> Result<(), RegisterError> {
        let role = role.into();
        let font = font.into();
        self.add_checked(Contribution::FontBinding {
            role,
            font,
            priority,
        })
    }

    /// Puts an action into a menu. Menus can be extended by any plugin and
    /// redefined by the user.
    /// Adds actions kept in the program (a `static` list of
    /// [`FixedAction`](paperpaw_sdk::FixedAction)s): they take next to no memory,
    /// where each added with [`add_action`](Self::add_action) takes some
    /// 90 bytes.
    pub fn add_actions(
        &mut self,
        actions: &'static [paperpaw_sdk::FixedAction],
    ) -> Result<(), RegisterError> {
        // Each in the plugin's namespace, and once.
        let added = |id: &Id| {
            self.contributions.iter().any(|c| match c {
                Contribution::Action(a) => a.id == *id,
                Contribution::FixedActions(list) => list.iter().any(|a| a.id() == id),
                _ => false,
            })
        };
        for (n, action) in actions.iter().enumerate() {
            let id = action.id();
            if !id.is_owned_by(&self.plugin) {
                return Err(RegisterError::Namespace { id: id.clone() });
            }
            if added(id) || actions[..n].iter().any(|a| a.id() == id) {
                return Err(RegisterError::Duplicate { id: id.clone() });
            }
        }
        self.contributions.push(Contribution::FixedActions(actions));
        Ok(())
    }

    pub fn add_menu_entry(&mut self, entry: MenuEntry) -> Result<(), RegisterError> {
        self.add_checked(Contribution::MenuEntry(entry))
    }

    /// Puts actions into menus from a list kept in the program (a
    /// `static` list, [`MenuEntry::new`] being `const`): it takes next to
    /// no memory, where each added with
    /// [`add_menu_entry`](Self::add_menu_entry) takes some 50 bytes.
    pub fn add_menu_entries(&mut self, entries: &'static [MenuEntry]) -> Result<(), RegisterError> {
        self.add_checked(Contribution::FixedMenuEntries(entries))
    }

    /// Puts `item` into `menu` at `place`, beside other entries (e.g.
    /// right after another plugin's); where another plugin puts the same
    /// entry there by order, this moves it.
    pub fn place_menu_entry(
        &mut self,
        menu: impl Into<Id>,
        item: impl Into<Id>,
        place: paperpaw_sdk::MenuPlace,
    ) -> Result<(), RegisterError> {
        self.add_menu_entry(MenuEntry::new(menu.into(), item.into()).place(place))
    }

    /// Takes `item` out of `menu` as plugins contribute it (also another
    /// plugin's, e.g. an entry a plugin replaces with its own); a user
    /// who lists it keeps it, and the menu editor tells which plugin took
    /// it out.
    pub fn hide_menu_entry(
        &mut self,
        menu: impl Into<Id>,
        item: impl Into<Id>,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::HideMenuEntry {
            menu: menu.into(),
            item: item.into(),
        })
    }

    /// What menus show of `item` now, where it changes: another label, a
    /// switch, faint, hidden, a badge (see
    /// [`ItemState`](paperpaw_sdk::ItemState)). Asked whenever a menu
    /// shows it: it says it from memory. Only for the plugin's own
    /// actions and settings ([`RegisterError::Namespace`] otherwise).
    pub fn set_item_state(
        &mut self,
        item: impl Into<Id>,
        state: paperpaw_sdk::ItemStateFn,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::ItemState {
            item: item.into(),
            state,
        })
    }

    /// Adds a heading to a menu; entries ordered after it, up to the next
    /// heading, belong to it.
    pub fn add_menu_section(&mut self, section: MenuSection) -> Result<(), RegisterError> {
        self.add_checked(Contribution::MenuSection(section))
    }

    /// Gives an action or setting an icon by name (see
    /// [`icons`](paperpaw_sdk::icons)), shown with it in menus. Any item may get
    /// one, e.g. by an icon theme; the last registered wins.
    pub fn add_icon(
        &mut self,
        item: impl Into<Id>,
        icon: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let icon = icon.into();
        self.add_checked(Contribution::Icon {
            item: item.into(),
            icon,
        })
    }

    /// Gives items icons from a list kept in the program (a `static` list
    /// of items and icon names): next to no memory, where each added with
    /// [`add_icon`](Self::add_icon) takes some 30 bytes. Icons added with
    /// [`add_icon`](Self::add_icon) win over these (an icon theme's).
    pub fn add_icons(&mut self, icons: &'static [(Id, &'static str)]) -> Result<(), RegisterError> {
        self.add_checked(Contribution::FixedIcons(icons))
    }

    /// Offers an action or setting in menus only on devices that meet
    /// `need`, e.g. reading aloud only where the device can speak.
    pub fn set_menu_need(
        &mut self,
        item: impl Into<Id>,
        need: MenuNeed,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::MenuNeed {
            item: item.into(),
            need,
        })
    }

    /// Adds a group for the settings screen: best a `static` one, kept
    /// by reference (`r.add_setting_group(&GROUP)`), which takes next to
    /// no memory; one made at run time is kept on the heap.
    pub fn add_setting_group(
        &mut self,
        group: impl Into<paperpaw_sdk::SettingGroup>,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::SettingGroup(group.into()))
    }

    /// Adds a file format.
    pub fn add_format(&mut self, format: FormatSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Format(Box::new(format)))
    }

    /// Subscribes to an event by its id, e.g. one known only at run time;
    /// [`Items::on`] names an [`Event`](paperpaw_sdk::event::Event).
    pub fn subscribe(
        &mut self,
        event: impl Into<Id>,
        run: crate::Hear,
    ) -> Result<(), RegisterError> {
        self.hears.push(crate::registry::Subscribed {
            event: event.into(),
            run,
        });
        Ok(())
    }

    /// Adds an action of a data plugin (`actions.ini`): the kernel runs its
    /// `steps`.
    pub(crate) fn add_data_action(
        &mut self,
        spec: ActionSpec,
        steps: Box<[crate::data_plugin::formats::Step]>,
    ) -> Result<(), RegisterError> {
        let id = spec.id.clone();
        self.add_action(spec)?;
        self.data_actions
            .push(crate::registry::DataAction { id, steps });
        Ok(())
    }

    /// Hears `event`: `run` reads what it carries with
    /// [`hear::data`](crate::hear::data).
    pub fn on<P: paperpaw_sdk::event::EventData>(
        &mut self,
        event: paperpaw_sdk::event::Event<P>,
        run: crate::Hear,
    ) -> Result<(), RegisterError> {
        self.subscribe(event.id().clone(), run)
    }

    /// Adds an item that status lines can show, e.g. a clock or the
    /// reading time left.
    pub fn add_status_item(&mut self, item: StatusItemSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::StatusItem(Box::new(item)))
    }

    /// Adds a decorator: marks on the words of book pages, drawn by every
    /// reader (see [`decorations`](paperpaw_sdk::decorations)).
    pub fn add_decorator(
        &mut self,
        decorator: paperpaw_sdk::decorations::DecoratorSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Decorator(Box::new(decorator)))
    }

    /// Adds a job: the steps of the work declared with its id (see
    /// [`work`](paperpaw_sdk::work)).
    pub fn add_work(&mut self, work: crate::work::WorkSpec) -> Result<(), RegisterError> {
        if !work.id.is_owned_by(&self.plugin) {
            return Err(RegisterError::Namespace { id: work.id });
        }
        if self.works.iter().any(|w| w.id == work.id) {
            return Err(RegisterError::Duplicate { id: work.id });
        }
        self.works.push(work);
        Ok(())
    }

    /// Adds a widget, e.g. for the home screen (see
    /// [`widgets`](paperpaw_sdk::widgets)).
    pub fn add_widget(&mut self, widget: WidgetSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Widget(Box::new(widget)))
    }

    /// Adds a step to the first-run setup (see [`setup`](paperpaw_sdk::setup)).
    pub fn add_setup_step(&mut self, step: SetupStepSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::SetupStep(Box::new(step)))
    }

    /// Adds a preset, a ready-made set of setting values (see
    /// [`presets`](paperpaw_sdk::presets)), e.g. from a data plugin's file.
    pub fn add_preset(&mut self, preset: Preset) -> Result<(), RegisterError> {
        let fixed = self.contributions.iter().any(|c| {
            matches!(c, Contribution::FixedPresets(list) if list.iter().any(|p| p.id == preset.id))
        });
        if fixed {
            return Err(RegisterError::Duplicate { id: preset.id });
        }
        self.add_checked(Contribution::Preset(Box::new(preset)))
    }

    /// Adds presets kept in the program ([`Preset::fixed`]), a `static`
    /// list, each in the plugin's namespace (they may be of any plugin's
    /// kind).
    pub fn add_presets(&mut self, presets: &'static [Preset]) -> Result<(), RegisterError> {
        let added = |id: &Id| {
            self.contributions.iter().any(|c| match c {
                Contribution::Preset(p) => p.id == *id,
                Contribution::FixedPresets(list) => list.iter().any(|p| p.id == *id),
                _ => false,
            })
        };
        for (n, preset) in presets.iter().enumerate() {
            let id = &preset.id;
            if !id.is_owned_by(&self.plugin) {
                return Err(RegisterError::Namespace { id: id.clone() });
            }
            if added(id) || presets[..n].iter().any(|p| p.id == *id) {
                return Err(RegisterError::Duplicate { id: id.clone() });
            }
        }
        self.contributions.push(Contribution::FixedPresets(presets));
        Ok(())
    }

    /// Adds a whole status line, drawn by the plugin (see
    /// [`status`](paperpaw_sdk::status)).
    pub fn add_status_line(&mut self, line: StatusLineSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::StatusLine(Box::new(line)))
    }

    /// Provides `service` for other plugins as `id` in `version`: a type
    /// with a contract ([`services::ServiceType`](paperpaw_sdk::services::ServiceType),
    /// usually a struct holding a trait object), found by its key also by
    /// native plugins built apart, and by the version others need
    /// ([`Services::get`](paperpaw_sdk::services::Services::get)).
    pub fn provide<T: paperpaw_sdk::services::ServiceType>(
        &mut self,
        id: impl Into<Id>,
        service: T,
        version: paperpaw_sdk::Version,
    ) -> Result<(), RegisterError> {
        let id = id.into();
        self.add_checked(Contribution::Published {
            id,
            service: paperpaw_sdk::services::Service::new(service, version),
        })
    }

    /// Declares an event of one's own, with the params it carries: rules
    /// and macro editors offer it.
    pub fn declare_event(
        &mut self,
        event: paperpaw_sdk::services::EventSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::EventDeclared(Box::new(event)))
    }

    /// Adds `contribution` to `point`, a point another plugin defined
    /// (see [`services`](paperpaw_sdk::services)).
    pub fn contribute(
        &mut self,
        point: impl Into<Id>,
        contribution: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let point = point.into();
        let contribution = contribution.into();
        self.add_checked(Contribution::ToPoint {
            point,
            text: contribution,
        })
    }

    /// Adds a translation catalog (see `paperpaw-i18n`).
    pub fn add_translations(
        &mut self,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let source = source.into();
        self.add_checked(Contribution::Translations(source))
    }

    /// Adds a compiled translation catalog on the storage (`.ppcat`, made
    /// by `paperpaw-sign catalog` from an `.ini` catalog): its messages
    /// stay there and are read as they are needed, so a language of any
    /// size takes about a kilobyte of memory (ADR 0033). One that cannot
    /// be opened is reported and its language falls back.
    pub fn add_translation_file(&mut self, path: impl Into<String>) -> Result<(), RegisterError> {
        let path = path.into();
        self.add_checked(Contribution::TranslationFile(path))
    }

    /// Changes the default of any plugin's setting, written as in
    /// `paperpaw.ini` (e.g. a theme's text size). Device defaults and the
    /// user's choice still win; unknown settings are reported once all
    /// plugins are registered.
    pub fn set_setting_default(
        &mut self,
        setting: impl Into<Id>,
        value: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let setting = setting.into();
        let value = value.into();
        self.add_checked(Contribution::SettingDefault { setting, value })
    }

    /// Offers `value` among the choices of any plugin's choice setting,
    /// after its own: e.g. a sleep mode of one's own in `sleep.show`. The
    /// value is in this plugin's namespace and the translation key of its
    /// name; what it means is the setting's plugin's to say (the sleep
    /// screen shows the screen of that id). Settings that are no choice,
    /// or keep the device safe, take none.
    pub fn add_choice(
        &mut self,
        setting: impl Into<Id>,
        value: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Choice {
            setting: setting.into(),
            value: value.into(),
        })
    }
}

/// Whether `folder` is a folder inside PaperPaw's own
/// ([`data::DIR`](paperpaw_sdk::data::DIR)), named plainly: no `..`, `.` or
/// empty parts, no backslash.
fn in_own_folder(folder: &str) -> bool {
    let Some(rest) = folder
        .strip_prefix(paperpaw_sdk::data::DIR)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    !rest.contains('\\')
        && rest
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &'static str) -> ActionSpec {
        ActionSpec::new(Id::new_static(id), "x")
    }

    #[test]
    fn ids_in_the_namespace_and_once() {
        let mut r = Items::new(Id::new_static("demo"));
        r.add_action(action("demo.go")).unwrap();
        assert_eq!(
            r.add_action(action("other.go")),
            Err(RegisterError::Namespace {
                id: Id::new_static("other.go")
            })
        );
        assert_eq!(
            r.add_action(action("demo.go")),
            Err(RegisterError::Duplicate {
                id: Id::new_static("demo.go")
            })
        );
        // Another kind may use the same id; bindings name others' ids.
        r.add_menu_section(MenuSection::new(
            Id::new_static("demo.go"),
            "x",
            Id::new_static("core.main"),
        ))
        .unwrap();
        r.add_icon(Id::new_static("other.go"), Cow::Borrowed("book"))
            .unwrap();
        r.add_font_file(Id::new_static("demo.serif"), "a.ttf")
            .unwrap();
        assert!(matches!(
            r.add_font_extension(
                Id::new_static("demo.serif"),
                "b.ppfp",
                Id::new_static("x.y")
            ),
            Ok(())
        ));
        // Faces share their ids with fonts and font files.
        assert!(matches!(
            r.add_font_face(FontFace::new(
                Id::new_static("demo.serif"),
                "c.ppfp",
                "Serif",
                false,
                false
            )),
            Err(RegisterError::Duplicate { .. })
        ));
        r.add_font_face(FontFace::new(
            Id::new_static("demo.serif-bold"),
            "c.ppfp",
            "Serif",
            true,
            false,
        ))
        .unwrap();
        let kinds: Vec<&str> = r.contributions().iter().map(Contribution::kind).collect();
        // Kept small: a plugin's list is held while it registers.
        assert!(core::mem::size_of::<Contribution>() <= 96);
        assert_eq!(
            kinds,
            [
                "action",
                "menu-section",
                "icon",
                "font-file",
                "font-extension",
                "font-face"
            ]
        );
    }

    /// The clean-up removes what is named like a book's key in a book
    /// data folder: one outside PaperPaw's own folder is refused, or it
    /// could take the reader's files.
    #[test]
    fn book_data_is_kept_only_in_paperpaws_folder() {
        let mut r = Items::new(Id::new_static("demo"));
        r.add_book_data(".paperpaw/plugins/demo", "").unwrap();
        r.add_book_cache(".paperpaw/plugins/demo/pages", "p-")
            .unwrap();
        for folder in [
            "books",
            "",
            ".paperpaw",
            ".paperpaw/",
            "/.paperpaw/plugins/demo",
            ".paperpaw/../books",
            ".paperpaw/plugins/../../books",
            ".paperpaw//x",
            ".paperpaw/./x",
            ".paperpaw\\..\\books",
            ".paperpawish/x",
        ] {
            assert!(
                matches!(
                    r.add_book_data(folder, ""),
                    Err(RegisterError::Invalid { .. })
                ),
                "{folder:?}"
            );
            assert!(
                matches!(
                    r.add_book_cache(folder, ""),
                    Err(RegisterError::Invalid { .. })
                ),
                "{folder:?}"
            );
            let data = Contribution::BookData(paperpaw_sdk::library::BookData::new(
                Cow::Owned(folder.into()),
                Cow::Borrowed(""),
                paperpaw_sdk::library::BookDataKind::UserData,
            ));
            assert!(r.add(data).is_err(), "{folder:?}");
        }
        assert_eq!(r.contributions().len(), 2);
    }
}
