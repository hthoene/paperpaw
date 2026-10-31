//! What plugins add to PaperPaw: one contribution each, collected by the
//! [`Registrar`] a plugin is given in [`Plugin::register`](crate::Plugin).
//!
//! The registrar only collects; the kernel applies the contributions once
//! the plugin is done (and parses key maps and translations then). So
//! anything can look at what a plugin adds without running PaperPaw, e.g.
//! a plugin's own tests or a plugin library (these methods come with the
//! `kernel` feature, which such tests name as a dev-dependency):
//!
//! ```
//! # use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};
//! # fn check(plugin: &dyn Plugin) {
//! let mut registrar = Registrar::new(plugin.manifest().id);
//! plugin.register(&mut registrar).unwrap();
//! for contribution in registrar.contributions() {
//!     // e.g. list its screens and settings
//!     let _ = (contribution.kind(), contribution.id());
//! }
//! # }
//! ```

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::action::ActionSpec;
use crate::event::EventHandler;
use crate::filters::{InputFilterSpec, RefreshFilterSpec, TextFilterSpec};
use crate::fonts::{FontFace, FontSpec};
use crate::formats::FormatSpec;
use crate::hyphenation::HyphenationSpec;
use crate::id::Id;
use crate::menus::{MenuEntry, MenuNeed, MenuSection};
use crate::plugin::{Priority, RegisterError, SlotSpec};
use crate::presets::Preset;
use crate::screen::{HintVisibility, ScreenSpec};
use crate::settings::SettingSpec;
use crate::setup::SetupStepSpec;
use crate::status::{Place as StatusPlace, StatusItemSpec, StatusLineSpec};
use crate::widgets::WidgetSpec;

pub(crate) mod item;

use item::Contribution;

/// Collects a plugin's contributions (see the [module documentation](self)).
///
/// Every id a plugin adds must be in its namespace (`<plugin>.…`), and
/// once per kind; bindings, menu entries and key maps may name what other
/// plugins added.
#[derive(Debug)]
pub struct Registrar {
    plugin: Id,
    contributions: Vec<Contribution>,
}

impl Registrar {
    /// A registrar for plugin `plugin`.
    pub fn new(plugin: impl Into<Id>) -> Self {
        let plugin = plugin.into();
        Self {
            plugin,
            contributions: Vec::new(),
        }
    }

    /// The id of the plugin being registered.
    pub fn plugin(&self) -> &Id {
        &self.plugin
    }

    /// What was added so far, in order.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn contributions(&self) -> &[Contribution] {
        &self.contributions
    }

    /// What was added, for the kernel to apply.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn into_contributions(self) -> Vec<Contribution> {
        self.contributions
    }

    /// Adds `contribution`; every method below is a shorthand for one.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, contribution: Contribution) -> Result<(), RegisterError> {
        self.add_checked(contribution)
    }

    /// As [`add`](Self::add), for the shorthands below.
    pub(crate) fn add_checked(&mut self, contribution: Contribution) -> Result<(), RegisterError> {
        // What an entry shows is said by the plugin it belongs to.
        if let Contribution::ItemState { item, .. } | Contribution::ItemDescription { item, .. } =
            &contribution
            && !item.is_owned_by(&self.plugin)
        {
            return Err(RegisterError::Namespace { id: item.clone() });
        }
        // No handler in place of a sealed action of one's own, in either
        // order.
        if let Contribution::Action(action) = &contribution
            && action.is_sealed()
            && self.contributions.iter().any(
                |c| matches!(c, Contribution::ActionOverride(over) if over.action == action.id),
            )
        {
            return Err(RegisterError::Invalid {
                message: Cow::Owned(alloc::format!("`{}` is sealed", action.id)),
            });
        }
        if let Contribution::ActionOverride(over) = &contribution {
            let sealed = self.contributions.iter().any(|c| match c {
                Contribution::Action(a) => a.id == over.action && a.is_sealed(),
                Contribution::FixedActions(list) => {
                    list.iter().any(|a| *a.id() == over.action && a.is_sealed())
                }
                _ => false,
            });
            if sealed {
                return Err(RegisterError::Invalid {
                    message: Cow::Owned(alloc::format!("`{}` is sealed", over.action)),
                });
            }
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
                    crate::data::DIR
                )),
            });
        }
        if let Some(id) = contribution.id() {
            // The kernel's services a plugin may stand in for (the
            // library) are published under the kernel's ids.
            let stands_in = matches!(contribution, Contribution::Published { .. })
                && crate::services::REPLACEABLE.contains(&id.as_str());
            if !id.is_owned_by(&self.plugin) && !stands_in {
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
        self.contributions.push(contribution);
        Ok(())
    }

    pub fn add_action(&mut self, action: ActionSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Action(Box::new(action)))
    }

    pub fn add_screen(&mut self, screen: ScreenSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Screen(screen))
    }

    /// Adds screens kept in the program (a `static` list of
    /// [`FixedScreen`](crate::FixedScreen)s): they take next to no memory,
    /// where each added with [`add_screen`](Self::add_screen) takes some
    /// 60 bytes.
    pub fn add_screens(
        &mut self,
        screens: &'static [crate::screen::FixedScreen],
    ) -> Result<(), RegisterError> {
        // Each in the plugin's namespace, and once.
        let added = |id: &Id| {
            self.contributions.iter().any(|c| match c {
                Contribution::Screen(s) => s.id == *id,
                Contribution::FixedScreens(list) => list.iter().any(|s| s.id() == id),
                _ => false,
            })
        };
        for (n, screen) in screens.iter().enumerate() {
            let id = screen.id();
            if !id.is_owned_by(&self.plugin) {
                return Err(RegisterError::Namespace { id: id.clone() });
            }
            if added(id) || screens[..n].iter().any(|s| s.id() == id) {
                return Err(RegisterError::Duplicate { id: id.clone() });
            }
        }
        self.contributions.push(Contribution::FixedScreens(screens));
        Ok(())
    }

    pub fn add_slot(&mut self, slot: SlotSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Slot(slot))
    }

    /// Gives the parts of every screen (switches, the focus, buttons,
    /// headers, …) the look `painters` paint (see
    /// [`ui::look`](crate::ui::look)). Of several looks the one with the
    /// highest priority is used, of equal ones the later.
    pub fn set_ui_look(
        &mut self,
        painters: &'static crate::ui::look::Painters,
        priority: Priority,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::UiLook { painters, priority })
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

    /// Adds default key bindings in the PaperPaw config format:
    ///
    /// ```ini
    /// [keymap.global]
    /// button.page-forward = core.next-page
    /// long.back = core.home
    /// tap.center = core.menu
    /// ```
    /// Adds a contribution of a kind that comes after this release, by its
    /// key: what PaperPaw takes from plugins later can be given by plugins
    /// built now, without a new plugin interface. A kernel that does not
    /// know the key leaves it.
    pub fn add_extended(
        &mut self,
        key: impl Into<Id>,
        params: crate::Params,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Extended {
            key: key.into(),
            params,
        })
    }

    pub fn add_keymap(
        &mut self,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let source = source.into();
        self.add_checked(Contribution::Keymap(source))
    }

    /// Adds key bindings (as [`Registrar::add_keymap`]) that apply only
    /// while the switch `setting` is on, e.g. a held button that does
    /// something users may turn off: with the switch off the binding is
    /// gone, so the button acts on its short press at once again.
    pub fn add_keymap_when(
        &mut self,
        setting: &crate::setting::Setting<bool>,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let source = source.into();
        self.add_checked(Contribution::KeymapWhen {
            setting: setting.into(),
            source,
        })
    }

    /// A stylesheet for books, applied after a book's own (where the
    /// format reads CSS, e.g. EPUB), e.g. for a theme that sets books
    /// apart; see [`Env::stylesheets`](crate::Env).
    pub fn add_stylesheet(
        &mut self,
        css: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        let css = css.into();
        self.add_checked(Contribution::Stylesheet(css))
    }

    /// A folder where the plugin keeps the reader's data for each book
    /// (a place, notes, settings for the book), named by the book's key
    /// ([`library::key`](crate::library::key)) after `prefix` (alone, or
    /// followed by `.` and more, a file or a folder): the kernel moves it
    /// with its book and removes it with it. What is left of books no
    /// longer found goes only when the reader agrees: a book renamed on a
    /// computer or on a card not inserted is not found either
    /// ([`library::BookData`](crate::library::BookData)). For what is
    /// made again from the book, [`add_book_cache`](Self::add_book_cache).
    /// The folder must be inside `.paperpaw/` (usually
    /// [`Env::plugin_dir`](crate::Env::plugin_dir)): one elsewhere is
    /// refused with [`RegisterError::Invalid`], so that no file of the
    /// reader's is taken for a book's data.
    pub fn add_book_data(
        &mut self,
        folder: impl Into<Cow<'static, str>>,
        prefix: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::BookData(crate::library::BookData::new(
            folder.into(),
            prefix.into(),
            crate::library::BookDataKind::UserData,
        )))
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
        self.add_checked(Contribution::BookData(crate::library::BookData::new(
            folder.into(),
            prefix.into(),
            crate::library::BookDataKind::Cache,
        )))
    }

    /// Adds a field plugins keep with books ([`library::set_field`](crate::library::set_field)):
    /// shown on a book's information page, kept in the catalog where
    /// [`in_catalog`](crate::library::BookField::in_catalog).
    pub fn add_book_field(
        &mut self,
        field: crate::library::BookField,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::BookField(Box::new(field)))
    }

    /// Adds a badge to books' rows in every view of the library (see
    /// [`library::service`](fn@crate::library::service)).
    pub fn add_book_badge(
        &mut self,
        badge: crate::library::BookBadge,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::BookBadge(Box::new(badge)))
    }

    /// Adds a key map preset (same format as [`Registrar::add_keymap`]) that
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
    /// [`builtin::fonts::UI`](crate::builtin::fonts::UI).
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
    /// [`FixedAction`](crate::FixedAction)s): they take next to no memory,
    /// where each added with [`add_action`](Self::add_action) takes some
    /// 90 bytes.
    pub fn add_actions(
        &mut self,
        actions: &'static [crate::action::FixedAction],
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
            // No handler in place of a sealed action of one's own.
            let overridden = self
                .contributions
                .iter()
                .any(|c| matches!(c, Contribution::ActionOverride(over) if over.action == *id));
            if action.is_sealed() && overridden {
                return Err(RegisterError::Invalid {
                    message: Cow::Owned(alloc::format!("`{id}` is sealed")),
                });
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
        place: crate::MenuPlace,
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
    /// [`ItemState`](crate::ItemState)). Asked whenever a menu
    /// shows it: it says it from memory. Only for the plugin's own
    /// actions and settings ([`RegisterError::Namespace`] otherwise).
    pub fn set_item_state(
        &mut self,
        item: impl Into<Id>,
        state: impl Fn(&crate::Env<'_>) -> crate::ItemState + 'static,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::ItemState {
            item: item.into(),
            state: Box::new(state),
        })
    }

    /// A line under `item` in menus, by its translation key; only for the
    /// plugin's own actions and settings.
    pub fn describe_item(
        &mut self,
        item: impl Into<Id>,
        key: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::ItemDescription {
            item: item.into(),
            key: key.into(),
        })
    }

    /// Adds a heading to a menu; entries ordered after it, up to the next
    /// heading, belong to it.
    pub fn add_menu_section(&mut self, section: MenuSection) -> Result<(), RegisterError> {
        self.add_checked(Contribution::MenuSection(section))
    }

    /// Gives an action or setting an icon by name (see
    /// [`icons`](crate::icons)), shown with it in menus. Any item may get
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

    /// When `screen` shows the row of hints for the buttons below it
    /// ([`HintVisibility::Shown`](crate::HintVisibility) unless set), and so the size it is
    /// made for.
    pub fn set_screen_hints(
        &mut self,
        screen: impl Into<Id>,
        hints: HintVisibility,
    ) -> Result<(), RegisterError> {
        let screen = screen.into();
        self.add_checked(Contribution::ScreenHints { screen, hints })
    }

    /// Adds a group for the settings screen: best a `static` one, kept
    /// by reference (`r.add_setting_group(&GROUP)`), which takes next to
    /// no memory; one made at run time is kept on the heap.
    pub fn add_setting_group(
        &mut self,
        group: impl Into<crate::settings::SettingGroup>,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::SettingGroup(group.into()))
    }

    /// Adds a file format.
    pub fn add_format(&mut self, format: FormatSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Format(Box::new(format)))
    }

    /// Adds hyphenation patterns for a language.
    pub fn add_hyphenation(&mut self, spec: HyphenationSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Hyphenation(spec))
    }

    /// Subscribes to an event by its id, with what it carries as named
    /// strings, e.g. for events known only at run time; [`Registrar::on`]
    /// hears an [`Event`](crate::event::Event) with its data.
    pub fn subscribe(
        &mut self,
        event: impl Into<Id>,
        handler: EventHandler,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Subscription {
            event: event.into(),
            handler,
        })
    }

    /// Hears `event` with what it carries (see [`event`](crate::event)):
    /// `handler` is not called for an emit of it that lacks what it needs.
    pub fn on<P: crate::event::EventData>(
        &mut self,
        event: crate::event::Event<P>,
        handler: impl Fn(&crate::Env<'_>, &P, &mut crate::Commands) + 'static,
    ) -> Result<(), RegisterError> {
        self.subscribe(
            event.id().clone(),
            // What the event carries as it is, without a copy; else as
            // written.
            Box::new(move |env, params, out| match params.carried::<P>() {
                Some(data) => handler(env, data, out),
                None => {
                    if let Some(data) = P::from_params(params) {
                        handler(env, &data, out);
                    }
                }
            }),
        )
    }

    /// Adds an item that status lines can show, e.g. a clock or the
    /// reading time left.
    pub fn add_status_item(&mut self, item: StatusItemSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::StatusItem(Box::new(item)))
    }

    /// Adds a filter of book text before it is laid out (see
    /// [`filters`](crate::filters)).
    pub fn add_text_filter(&mut self, filter: TextFilterSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::TextFilter(filter))
    }

    /// Adds a decorator: marks on the words of book pages, drawn by every
    /// reader (see [`decorations`](crate::decorations)).
    pub fn add_decorator(
        &mut self,
        decorator: crate::decorations::DecoratorSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Decorator(Box::new(decorator)))
    }

    /// Adds a layer: drawn on book pages by every reader, with room of its
    /// own beside the text if it reserves some (see
    /// [`layers`](crate::layers)).
    pub fn add_page_layer(
        &mut self,
        layer: crate::layers::PageLayerSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::PageLayer(Box::new(layer)))
    }

    /// Adds a frame: book pages set in columns, offered by the setting
    /// [`PAGE_FRAME`](crate::builtin::settings::PAGE_FRAME) (see
    /// [`layers`](crate::layers)).
    pub fn add_page_frame(
        &mut self,
        frame: crate::layers::PageFrameSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::PageFrame(Box::new(frame)))
    }

    /// Adds a lookup provider: where words are looked up, listed by the
    /// screen that shows a word's entries (see [`lookup`](crate::lookup)).
    pub fn add_lookup_provider(
        &mut self,
        provider: crate::lookup::LookupProviderSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::LookupProvider(Box::new(provider)))
    }

    /// Adds a voice: speaks text aloud in place of the system's while the
    /// setting [`VOICE`](crate::builtin::settings::VOICE) chooses it (see
    /// [`voices`](crate::voices)).
    pub fn add_voice(&mut self, voice: crate::voices::VoiceSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Voice(Box::new(voice)))
    }

    /// Adds a reading app: a screen of the plugin that shows books of
    /// some formats, offered where books open (see
    /// [`readers`](crate::readers)).
    /// Its screen is one the plugin added before.
    pub fn add_reader(&mut self, reader: crate::readers::ReaderSpec) -> Result<(), RegisterError> {
        if !reader.screen.is_owned_by(&self.plugin) {
            return Err(RegisterError::Namespace { id: reader.screen });
        }
        let added = self.contributions.iter().any(|c| match c {
            Contribution::Screen(s) => s.id == reader.screen,
            Contribution::FixedScreens(list) => list.iter().any(|s| *s.id() == reader.screen),
            _ => false,
        });
        if !added {
            return Err(RegisterError::Invalid {
                message: Cow::Owned(alloc::format!(
                    "`{}` reads in `{}`, a screen not added",
                    reader.id,
                    reader.screen
                )),
            });
        }
        self.add_checked(Contribution::Reader(reader))
    }

    /// Adds a filter of every input, before screens and key maps see it
    /// (see [`filters`](crate::filters)).
    pub fn add_input_filter(&mut self, filter: InputFilterSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::InputFilter(filter))
    }

    /// Adds a service to the device's own GATT server, whose centrals'
    /// reads, writes and subscriptions come as the spec's events (see
    /// [`bluetooth`](crate::bluetooth)).
    pub fn add_gatt_service(
        &mut self,
        service: crate::bluetooth::ServiceSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::GattService(Box::new(service)))
    }

    /// Adds a filter of how the panel is refreshed (see
    /// [`filters`](crate::filters)).
    pub fn add_refresh_filter(&mut self, filter: RefreshFilterSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::RefreshFilter(filter))
    }

    /// Adds a hook that changes finished bands of the screen (see
    /// [`render`](crate::render)). **Check
    /// [`Capability::RENDER_HOOK`](crate::Capability::RENDER_HOOK):** where
    /// the device lacks it the hook is accepted and never run.
    pub fn add_render_hook(
        &mut self,
        hook: crate::render::RenderHookSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::RenderHook(Box::new(hook)))
    }

    /// Puts `handler` in place of action `action`'s own, also of another
    /// plugin's (one with none too, e.g. a screen action where no screen
    /// takes it): of several, the one of the highest priority runs, as
    /// for slots. It gets the params the action was run with.
    pub fn override_action(
        &mut self,
        action: impl Into<Id>,
        handler: impl Fn(&crate::Env<'_>, &crate::Params, &mut crate::Commands) + 'static,
        priority: Priority,
    ) -> Result<(), RegisterError> {
        let action = action.into();
        self.add_checked(Contribution::ActionOverride(Box::new(
            crate::action::ActionOverride {
                action,
                handler: Box::new(handler),
                priority,
            },
        )))
    }

    /// Sees every action run, with its params, before it runs (see
    /// [`ObserverSpec`](crate::ObserverSpec)).
    pub fn observe_actions(
        &mut self,
        observer: crate::action::ObserverSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::ActionObserver(Box::new(observer)))
    }

    /// Sees every event emitted, with its params, after its subscribers
    /// (see [`ObserverSpec`](crate::ObserverSpec)).
    pub fn observe_events(
        &mut self,
        observer: crate::action::ObserverSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::EventObserver(Box::new(observer)))
    }

    /// Adds an input of one's own, `input.<id>`, bound in key maps as any
    /// other and raised with [`Command::Input`](crate::Command::Input)
    /// (see [`keys::InputSpec`](crate::keys::InputSpec)).
    pub fn add_input(&mut self, input: crate::keys::InputSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Input(input))
    }

    /// Adds a recognizer of gestures of one's own, `gesture.<id>` in key
    /// maps (see [`keys::GestureSpec`](crate::keys::GestureSpec)).
    pub fn add_gesture(&mut self, gesture: crate::keys::GestureSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Gesture(Box::new(gesture)))
    }

    /// Adds a bar that stays along an edge of the screen across the
    /// screens shown: tabs, a dock (see [`chrome`](crate::chrome)).
    pub fn add_chrome(&mut self, chrome: crate::chrome::ChromeSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Chrome(Box::new(chrome)))
    }

    /// Adds a job: the steps of the work declared with its id (see
    /// [`work`](crate::work)).
    pub fn add_work(&mut self, work: crate::work::WorkSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Work(Box::new(work)))
    }

    /// Adds a widget, e.g. for the home screen (see
    /// [`widgets`](crate::widgets)).
    pub fn add_widget(&mut self, widget: WidgetSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Widget(Box::new(widget)))
    }

    /// Adds a step to the first-run setup (see [`setup`](crate::setup)).
    pub fn add_setup_step(&mut self, step: SetupStepSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::SetupStep(Box::new(step)))
    }

    /// Adds a preset, a ready-made set of setting values (see
    /// [`presets`](crate::presets)), e.g. from a data plugin's file.
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
    /// [`status`](crate::status)).
    pub fn add_status_line(&mut self, line: StatusLineSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::StatusLine(Box::new(line)))
    }

    /// Adds a place where status items show: a line's left, centre and
    /// right as settings of the items listed there (see
    /// [`status::Place`](crate::status::Place)).
    pub fn add_status_place(&mut self, place: StatusPlace) -> Result<(), RegisterError> {
        self.add_checked(Contribution::StatusPlace(Box::new(place)))
    }

    /// Adds a place where widgets show: the settings that list them, in a
    /// grid or above each other (see
    /// [`widgets::WidgetPlace`](crate::widgets::WidgetPlace)).
    pub fn add_widget_place(
        &mut self,
        place: crate::widgets::WidgetPlace,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::WidgetPlace(Box::new(place)))
    }

    /// Adds a kind of widgets of the plugin's own, with its icon, for
    /// widgets that fit none of the kernel's
    /// ([`widgets::categories`](crate::widgets::categories)).
    pub fn add_widget_category(
        &mut self,
        category: crate::widgets::WidgetCategory,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::WidgetCategory(Box::new(category)))
    }

    /// Provides `service` for other plugins as `id` in `version`: a type
    /// with a contract ([`services::ServiceType`](crate::services::ServiceType),
    /// usually a struct holding a trait object), found by its key also by
    /// native plugins built apart, and by the version others need
    /// ([`Services::get`](crate::services::Services::get)).
    pub fn provide<T: crate::services::ServiceType>(
        &mut self,
        id: impl Into<Id>,
        service: T,
        version: crate::Version,
    ) -> Result<(), RegisterError> {
        let id = id.into();
        self.add_checked(Contribution::Published {
            id,
            service: crate::services::Service::new(service, version),
        })
    }

    /// Declares a point of one's own that others contribute to (see
    /// [`services`](crate::services)), with what a contribution there is.
    pub fn add_point(&mut self, point: crate::services::PointSpec) -> Result<(), RegisterError> {
        self.add_checked(Contribution::Point(Box::new(point)))
    }

    /// Declares an event of one's own, with the params it carries: rules
    /// and macro editors offer it.
    pub fn declare_event(
        &mut self,
        event: crate::services::EventSpec,
    ) -> Result<(), RegisterError> {
        self.add_checked(Contribution::EventDeclared(Box::new(event)))
    }

    /// Adds `contribution` to `point`, a point another plugin defined
    /// (see [`services`](crate::services)).
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
/// ([`data::DIR`](crate::data::DIR)), named plainly: no `..`, `.` or
/// empty parts, no backslash.
fn in_own_folder(folder: &str) -> bool {
    let Some(rest) = folder
        .strip_prefix(crate::data::DIR)
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
        let mut r = Registrar::new(Id::new_static("demo"));
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
        r.add_menu_section(MenuSection {
            menu: Id::new_static("core.main"),
            id: Id::new_static("demo.go"),
            title: Cow::Borrowed("x"),
            order: 0,
        })
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
        let mut r = Registrar::new(Id::new_static("demo"));
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
            let data = Contribution::BookData(crate::library::BookData::new(
                Cow::Owned(folder.into()),
                Cow::Borrowed(""),
                crate::library::BookDataKind::UserData,
            ));
            assert!(r.add(data).is_err(), "{folder:?}");
        }
        assert_eq!(r.contributions().len(), 2);
    }
}
