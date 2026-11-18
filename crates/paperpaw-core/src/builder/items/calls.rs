//! What is added to PaperPaw, staged ([`Staging`]): what a registering
//! function adds ([`KernelBuilder::items`], the kernel's own built-ins, a
//! data plugin) goes straight into the kernel's lists of its kinds, checked
//! as it is added (its namespace, once per kind, key maps and translations
//! parsed), and is merged whole once the function is done, or not at all
//! (#300, D209: no list of contributions in between).
//!
//! [`KernelBuilder::items`]: crate::KernelBuilder::items

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::kit::action::ActionSpec;
use crate::kit::menus::MenuEntry;
use crate::kit::menus::MenuSection;
use crate::kit::setup::SetupStepSpec;
use crate::kit::status::{StatusItemSpec, StatusLineSpec};
use crate::kit::widgets::WidgetSpec;
use crate::registry::Staging;
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::{
    FontFace, FontSpec, FormatSpec, Id, MenuNeed, Priority, RegisterError, SettingSpec, SlotSpec,
};

impl Staging {
    /// What plugin `plugin` registers, staged: merged into the kernel
    /// only once all of it is added (whole, or nothing of it).
    pub fn new(plugin: impl Into<Id>) -> Self {
        Self {
            plugin: Some(plugin.into()),
            ..Self::default()
        }
    }

    /// The id of the plugin being registered.
    pub fn plugin(&self) -> Option<&Id> {
        self.plugin.as_ref()
    }

    /// `id` is the plugin's own: in its namespace.
    fn own(&self, id: &Id) -> Result<(), RegisterError> {
        match &self.plugin {
            Some(plugin) if id.is_owned_by(plugin) => Ok(()),
            _ => Err(RegisterError::Namespace { id: id.clone() }),
        }
    }

    /// `id` is the plugin's own and not `taken` yet among its kind.
    fn new_id(&self, id: &Id, taken: bool) -> Result<(), RegisterError> {
        self.own(id)?;
        if taken {
            return Err(RegisterError::Duplicate { id: id.clone() });
        }
        Ok(())
    }

    /// The plugin registering, for what names it (a hidden entry, a
    /// subscription).
    fn owner(&self) -> Result<Id, RegisterError> {
        self.plugin.clone().ok_or(RegisterError::Invalid {
            message: Cow::Borrowed("staged without a plugin"),
        })
    }

    fn has_action(&self, id: &Id) -> bool {
        self.actions.iter().any(|a| a.id == *id)
            || self
                .fixed_actions
                .iter()
                .any(|list| list.iter().any(|a| a.id() == id))
    }

    fn has_setting(&self, id: &Id) -> bool {
        self.settings
            .iter()
            .flat_map(crate::registry::Staged::specs)
            .any(|s| s.id == *id)
    }

    fn has_preset(&self, id: &Id) -> bool {
        self.presets.iter().any(|p| p.id == *id)
            || self
                .fixed_presets
                .iter()
                .any(|list| list.iter().any(|p| p.id == *id))
    }

    /// Fonts, font files and faces share their ids.
    fn has_font(&self, id: &Id) -> bool {
        self.fonts.iter().any(|f| f.id == *id)
            || self.font_files.iter().any(|(f, _)| f == id)
            || self.font_faces.iter().any(|f| f.id == *id)
    }

    pub fn add_action(&mut self, action: ActionSpec) -> Result<(), RegisterError> {
        self.new_id(&action.id, self.has_action(&action.id))?;
        push(&mut self.actions, action)
    }

    /// Adds the host's own screens (`host_screen::ENTRIES`).
    pub(crate) fn add_host_screens(
        &mut self,
        screens: &'static [crate::host_screen::HostEntry],
    ) -> Result<(), RegisterError> {
        self.check_screens(screens.iter().map(crate::host_screen::HostEntry::id))?;
        self.fixed_screens
            .try_reserve(screens.len())
            .map_err(|_| no_room())?;
        self.fixed_screens.extend(screens.iter());
        Ok(())
    }

    /// Adds a test's screen, a native screen whose plugin is the test (see
    /// [`testing`](crate::testing)).
    #[cfg(any(test, feature = "testing"))]
    pub fn add_fake(&mut self, screen: crate::testing::FakeSpec) -> Result<(), RegisterError> {
        self.check_screens(core::iter::once(screen.id()))?;
        push(&mut self.layer_screens, screen)
    }

    /// Each of `ids` in the plugin's namespace, once among its screens.
    fn check_screens<'a>(
        &self,
        ids: impl Iterator<Item = &'a Id> + Clone,
    ) -> Result<(), RegisterError> {
        let added = |id: &Id| {
            self.fixed_screens.iter().any(|s| s.id() == id)
                || self.layer_screens.iter().any(|s| s.id() == id)
        };
        for (n, id) in ids.clone().enumerate() {
            self.new_id(
                id,
                added(id) || ids.clone().take(n).any(|other| other == id),
            )?;
        }
        Ok(())
    }

    pub fn add_slot(&mut self, slot: SlotSpec) -> Result<(), RegisterError> {
        self.new_id(&slot.id, self.slots.iter().any(|s| s.id == slot.id))?;
        push(&mut self.slots, slot)
    }

    /// Proposes `screen` for `slot`. The binding with the highest priority
    /// wins unless the user configured the slot explicitly.
    pub fn bind_slot(
        &mut self,
        slot: impl Into<Id>,
        screen: impl Into<Id>,
        priority: Priority,
    ) -> Result<(), RegisterError> {
        let candidate = crate::registry::SlotCandidate {
            slot: slot.into(),
            screen: screen.into(),
            priority,
        };
        push(&mut self.candidates, candidate)
    }

    pub fn add_setting(&mut self, setting: SettingSpec) -> Result<(), RegisterError> {
        self.new_id(&setting.id, self.has_setting(&setting.id))?;
        push(
            &mut self.settings,
            crate::registry::Staged::Owned(Box::new(setting)),
        )
    }

    /// Adds settings kept in the program (a `static` list), e.g. a
    /// `static` list of a widget's: they take next to no memory, where
    /// each added with [`add_setting`](Self::add_setting) takes about
    /// 200 bytes.
    pub fn add_settings(&mut self, settings: &'static [SettingSpec]) -> Result<(), RegisterError> {
        for (n, setting) in settings.iter().enumerate() {
            let id = &setting.id;
            let taken = self.has_setting(id) || settings[..n].iter().any(|s| s.id == *id);
            self.new_id(id, taken)?;
        }
        let table = crate::registry::Staged::Table(settings, crate::registry::Picked::All);
        push(&mut self.settings, table)
    }

    pub fn add_keymap(
        &mut self,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let source = source.into();
        crate::registry::validate_keymap(&source)?;
        push(&mut self.keymaps, source)
    }

    /// A stylesheet for books, applied after a book's own (where the
    /// format reads CSS, e.g. EPUB), e.g. for a theme that sets books
    /// apart; see [`Place::stylesheets`](crate::Place).
    pub fn add_stylesheet(
        &mut self,
        css: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        push(&mut self.stylesheets, css.into())
    }

    /// A folder where the plugin keeps the reader's data for each book
    /// (a place, notes, settings for the book), named by the book's key
    /// ([`library::key`](crate::kit::library::key)) after `prefix` (alone, or
    /// followed by `.` and more, a file or a folder): the kernel moves it
    /// with its book and removes it with it. What is left of books no
    /// longer found goes only when the reader agrees: a book renamed on a
    /// computer or on a card not inserted is not found either
    /// ([`library::BookData`](crate::kit::library::BookData)). For what is
    /// made again from the book, [`add_book_cache`](Self::add_book_cache).
    /// The folder must be inside `.paperpaw/` (usually
    /// [`data::plugin_dir`](paperpaw_sdk::data::plugin_dir)): one elsewhere is
    /// refused with [`RegisterError::Invalid`], so that no file of the
    /// reader's is taken for a book's data.
    pub fn add_book_data(
        &mut self,
        folder: impl Into<Cow<'static, str>>,
        prefix: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let kind = crate::kit::library::BookDataKind::UserData;
        self.book_data(folder.into(), prefix.into(), kind)
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
        let kind = crate::kit::library::BookDataKind::Cache;
        self.book_data(folder.into(), prefix.into(), kind)
    }

    /// Adds a key map preset (same format as [`Staging::add_keymap`]) that
    /// users can select with the `core.keymap-preset` setting. Its title
    /// is the translation of its id.
    pub fn add_keymap_preset(
        &mut self,
        id: impl Into<Id>,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let id = id.into();
        let source = source.into();
        self.new_id(&id, self.keymap_presets.iter().any(|(p, _)| *p == id))?;
        crate::registry::validate_keymap(&source)?;
        push(&mut self.keymap_presets, (id, source))
    }

    /// Adds a font. Bitmap packs work everywhere; scalable fonts need the
    /// `outline` feature.
    pub fn add_font(&mut self, font: FontSpec) -> Result<(), RegisterError> {
        self.new_id(&font.id, self.has_font(&font.id))?;
        push(&mut self.fonts, font)
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
        self.new_id(&id, self.has_font(&id))?;
        push(&mut self.font_files, (id, path.into()))
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
        self.new_id(&id, self.font_extensions.iter().any(|(e, ..)| *e == id))?;
        push(&mut self.font_extensions, (id, path.into(), extends.into()))
    }

    /// Adds a face of a family in a file on the storage: a font pack
    /// (`.ppfp`), drawn on every device, or an OpenType/TrueType file,
    /// drawn where scalable fonts are (see [`FontFace`]). It is loaded
    /// only while its family is chosen, for reading or the status lines.
    pub fn add_font_face(&mut self, face: FontFace) -> Result<(), RegisterError> {
        self.new_id(&face.id, self.has_font(&face.id))?;
        push(&mut self.font_faces, face)
    }

    /// Proposes a font for a role such as
    /// [`builtin::fonts::UI`](paperpaw_sdk::builtin::fonts::UI).
    pub fn bind_font(
        &mut self,
        role: impl Into<Id>,
        font: impl Into<Id>,
        priority: Priority,
    ) -> Result<(), RegisterError> {
        push(&mut self.font_roles, (role.into(), font.into(), priority))
    }

    /// Puts an action into a menu. Menus can be extended by any plugin and
    /// redefined by the user.
    /// Adds actions kept in the program (a `static` list of
    /// [`FixedAction`](crate::kit::action::FixedAction)s): they take next to no memory,
    /// where each added with [`add_action`](Self::add_action) takes some
    /// 90 bytes.
    pub fn add_actions(
        &mut self,
        actions: &'static [crate::kit::action::FixedAction],
    ) -> Result<(), RegisterError> {
        for (n, action) in actions.iter().enumerate() {
            let id = action.id();
            let taken = self.has_action(id) || actions[..n].iter().any(|a| a.id() == id);
            self.new_id(id, taken)?;
        }
        push(&mut self.fixed_actions, actions)
    }

    pub fn add_menu_entry(&mut self, entry: MenuEntry) -> Result<(), RegisterError> {
        push(&mut self.menu_entries, entry)
    }

    /// Puts actions into menus from a list kept in the program (a
    /// `static` list, [`MenuEntry::new`] being `const`): it takes next to
    /// no memory, where each added with
    /// [`add_menu_entry`](Self::add_menu_entry) takes some 50 bytes.
    pub fn add_menu_entries(&mut self, entries: &'static [MenuEntry]) -> Result<(), RegisterError> {
        push(&mut self.fixed_menu_entries, entries)
    }

    /// Puts `item` into `menu` at `place`, beside other entries (e.g.
    /// right after another plugin's); where another plugin puts the same
    /// entry there by order, this moves it.
    pub fn place_menu_entry(
        &mut self,
        menu: impl Into<Id>,
        item: impl Into<Id>,
        place: crate::kit::menus::MenuPlace,
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
        let plugin = self.owner()?;
        push(&mut self.hidden_entries, (menu.into(), item.into(), plugin))
    }

    /// What menus show of `item` now, where it changes: another label, a
    /// switch, faint, hidden, a badge (see
    /// [`ItemState`](crate::kit::menus::ItemState)). Asked whenever a menu
    /// shows it: it says it from memory. Only for the plugin's own
    /// actions and settings ([`RegisterError::Namespace`] otherwise).
    pub fn set_item_state(
        &mut self,
        item: impl Into<Id>,
        state: crate::kit::menus::ItemStateFn,
    ) -> Result<(), RegisterError> {
        // What an entry shows is said by the plugin it belongs to.
        let item = item.into();
        self.own(&item)?;
        push(&mut self.item_states, (item, state))
    }

    /// Adds a heading to a menu; entries ordered after it, up to the next
    /// heading, belong to it.
    pub fn add_menu_section(&mut self, section: MenuSection) -> Result<(), RegisterError> {
        let taken = self.menu_sections.iter().any(|s| s.id == section.id);
        self.new_id(&section.id, taken)?;
        push(&mut self.menu_sections, section)
    }

    /// Gives an action or setting an icon by name (see
    /// [`icons`](paperpaw_sdk::icons)), shown with it in menus. Any item may get
    /// one, e.g. by an icon theme; the last registered wins.
    pub fn add_icon(
        &mut self,
        item: impl Into<Id>,
        icon: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        push(&mut self.icons, (item.into(), icon.into()))
    }

    /// Gives items icons from a list kept in the program (a `static` list
    /// of items and icon names): next to no memory, where each added with
    /// [`add_icon`](Self::add_icon) takes some 30 bytes. Icons added with
    /// [`add_icon`](Self::add_icon) win over these (an icon theme's).
    pub fn add_icons(&mut self, icons: &'static [(Id, &'static str)]) -> Result<(), RegisterError> {
        push(&mut self.fixed_icons, icons)
    }

    /// Offers an action or setting in menus only on devices that meet
    /// `need`, e.g. reading aloud only where the device can speak.
    pub fn set_menu_need(
        &mut self,
        item: impl Into<Id>,
        need: MenuNeed,
    ) -> Result<(), RegisterError> {
        push(&mut self.menu_needs, (item.into(), need))
    }

    /// Adds a group for the settings screen: best a `static` one, kept
    /// by reference (`r.add_setting_group(&GROUP)`), which takes next to
    /// no memory; one made at run time is kept on the heap.
    pub fn add_setting_group(
        &mut self,
        group: impl Into<paperpaw_sdk::SettingGroup>,
    ) -> Result<(), RegisterError> {
        let group = group.into();
        let taken = self.setting_groups.iter().any(|g| g.id == group.id);
        self.new_id(&group.id, taken)?;
        push(&mut self.setting_groups, group)
    }

    /// Adds a file format.
    pub fn add_format(&mut self, format: FormatSpec) -> Result<(), RegisterError> {
        self.new_id(&format.id, self.formats.iter().any(|f| f.id == format.id))?;
        push(&mut self.formats, format)
    }

    /// Subscribes to an event by its id, e.g. one known only at run time;
    /// [`Staging::on`] names an [`Event`](paperpaw_sdk::event::Event).
    pub fn subscribe(
        &mut self,
        event: impl Into<Id>,
        run: crate::Hear,
    ) -> Result<(), RegisterError> {
        let plugin = self.owner()?;
        let event = event.into();
        push(
            &mut self.subscriptions,
            (plugin, crate::registry::Subscribed { event, run }),
        )
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
        push(
            &mut self.data_actions,
            crate::registry::DataAction { id, steps },
        )
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
        self.new_id(&item.id, self.status_items.iter().any(|i| i.id == item.id))?;
        push(&mut self.status_items, item)
    }

    /// Adds a decorator: marks on the words of book pages, drawn by every
    /// reader (see [`decorations`](paperpaw_sdk::decorations)).
    pub fn add_decorator(
        &mut self,
        decorator: crate::kit::decorations::DecoratorSpec,
    ) -> Result<(), RegisterError> {
        let taken = self.decorators.iter().any(|d| d.id == decorator.id);
        self.new_id(&decorator.id, taken)?;
        push(&mut self.decorators, decorator)
    }

    /// Adds a job: the steps of the work declared with its id (see
    /// [`work`](paperpaw_sdk::work)).
    pub fn add_work(&mut self, work: crate::work::WorkSpec) -> Result<(), RegisterError> {
        self.new_id(&work.id, self.works.iter().any(|w| w.id == work.id))?;
        push(&mut self.works, work)
    }

    /// Adds a widget, e.g. for the home screen (see
    /// [`widgets`](paperpaw_sdk::widgets)).
    pub fn add_widget(&mut self, widget: WidgetSpec) -> Result<(), RegisterError> {
        let taken = self.widgets.iter().any(|w| match w {
            crate::registry::StagedWidgets::One(one) => one.id == widget.id,
        });
        self.new_id(&widget.id, taken)?;
        let widget = crate::registry::StagedWidgets::One(Box::new(widget));
        push(&mut self.widgets, widget)
    }

    /// Adds a step to the first-run setup (see `setup`).
    pub fn add_setup_step(&mut self, step: SetupStepSpec) -> Result<(), RegisterError> {
        self.new_id(&step.id, self.setup_steps.iter().any(|s| s.id == step.id))?;
        push(&mut self.setup_steps, step)
    }

    /// Adds a preset, a ready-made set of setting values (see
    /// [`presets`](paperpaw_sdk::presets)), e.g. from a data plugin's file.
    pub fn add_preset(&mut self, preset: Preset) -> Result<(), RegisterError> {
        self.new_id(&preset.id, self.has_preset(&preset.id))?;
        push(&mut self.presets, preset)
    }

    /// Adds presets kept in the program ([`Preset::fixed`]), a `static`
    /// list, each in the plugin's namespace (they may be of any plugin's
    /// kind).
    pub fn add_presets(&mut self, presets: &'static [Preset]) -> Result<(), RegisterError> {
        for (n, preset) in presets.iter().enumerate() {
            let id = &preset.id;
            let taken = self.has_preset(id) || presets[..n].iter().any(|p| p.id == *id);
            self.new_id(id, taken)?;
        }
        push(&mut self.fixed_presets, presets)
    }

    /// Adds a whole status line, drawn by the plugin (see
    /// [`status`](paperpaw_sdk::status)).
    pub fn add_status_line(&mut self, line: StatusLineSpec) -> Result<(), RegisterError> {
        self.new_id(&line.id, self.status_lines.iter().any(|l| l.id == line.id))?;
        push(&mut self.status_lines, line)
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
        self.new_id(&id, self.services.iter().any(|(s, _)| *s == id))?;
        let service = paperpaw_sdk::services::Service::new(service, version);
        push(&mut self.services, (id, service))
    }

    /// Declares an event of one's own, with the params it carries: rules
    /// and macro editors offer it.
    pub fn declare_event(
        &mut self,
        event: paperpaw_sdk::services::EventSpec,
    ) -> Result<(), RegisterError> {
        self.new_id(&event.id, self.events.iter().any(|e| e.id == event.id))?;
        push(&mut self.events, event)
    }

    /// Adds `contribution` to `point`, a point another plugin defined
    /// (see [`services`](paperpaw_sdk::services)).
    pub fn contribute(
        &mut self,
        point: impl Into<Id>,
        contribution: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        push(&mut self.contributions, (point.into(), contribution.into()))
    }

    /// Adds a translation catalog (see `paperpaw-i18n`).
    pub fn add_translations(
        &mut self,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let catalog = crate::registry::catalog(source.into())?;
        push(&mut self.catalogs, catalog)
    }

    /// Adds a compiled translation catalog on the storage (`.ppcat`, made
    /// by `paperpaw-sign catalog` from an `.ini` catalog): its messages
    /// stay there and are read as they are needed, so a language of any
    /// size takes about a kilobyte of memory (ADR 0033). One that cannot
    /// be opened is reported and its language falls back.
    pub fn add_translation_file(&mut self, path: impl Into<String>) -> Result<(), RegisterError> {
        push(&mut self.catalog_files, path.into())
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
        push(&mut self.setting_defaults, (setting.into(), value.into()))
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
        push(&mut self.choices, (setting.into(), value.into()))
    }

    /// A book data folder ([`add_book_data`](Self::add_book_data),
    /// [`add_book_cache`](Self::add_book_cache)).
    fn book_data(
        &mut self,
        folder: Cow<'static, str>,
        prefix: Cow<'static, str>,
        kind: crate::kit::library::BookDataKind,
    ) -> Result<(), RegisterError> {
        // The clean-up removes what is named like a book's key there: a
        // folder outside PaperPaw's own would take the reader's files.
        if !in_own_folder(&folder) {
            return Err(RegisterError::Invalid {
                message: Cow::Owned(alloc::format!(
                    "book data in `{folder}`, outside `{}/`",
                    paperpaw_sdk::data::DIR
                )),
            });
        }
        let data = crate::kit::library::BookData::new(folder, prefix, kind);
        push(&mut self.book_data, data)
    }
}

/// Room for one more in `list`, then `value` in it.
fn push<T>(list: &mut Vec<T>, value: T) -> Result<(), RegisterError> {
    list.try_reserve(1).map_err(|_| no_room())?;
    list.push(value);
    Ok(())
}

fn no_room() -> RegisterError {
    RegisterError::Invalid {
        message: Cow::Borrowed("contribution allocation failed"),
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
#[path = "rules_tests.rs"]
#[allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]
mod rules_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &'static str) -> ActionSpec {
        ActionSpec::new(Id::new_static(id), "x")
    }

    #[test]
    fn ids_in_the_namespace_and_once() {
        let mut r = Staging::new(Id::new_static("demo"));
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
        let staged = [
            r.actions.len(),
            r.menu_sections.len(),
            r.icons.len(),
            r.font_files.len(),
            r.font_extensions.len(),
            r.font_faces.len(),
        ];
        assert_eq!(staged, [1; 6]);
    }

    /// The clean-up removes what is named like a book's key in a book
    /// data folder: one outside PaperPaw's own folder is refused, or it
    /// could take the reader's files.
    #[test]
    fn book_data_is_kept_only_in_paperpaws_folder() {
        let mut r = Staging::new(Id::new_static("demo"));
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
        }
        assert_eq!(r.book_data.len(), 2);
    }
}
