use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::collections::VecMap;
use crate::id::{Id, Text};

/// Where an entry goes in its menu, beside others (see
/// [`MenuEntry::place`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuPlace {
    /// By order with the others placed so, lower first.
    Order(i16),
    /// Right after this entry (or heading).
    After(Id),
    /// Right before it.
    Before(Id),
    /// Last in the part below this heading ([`MenuSection`]).
    InSection(Id),
    /// Before everything else.
    First,
    /// After everything else.
    Last,
    /// A value a later build adds, by its name (a constant of that build);
    /// a build that doesn't know it: an entry for a place it doesn't know is not shown.
    Other(&'static str),
}

/// What a menu shows of an entry now, where it changes (see
/// [`Registrar::set_item_state`](crate::Registrar::set_item_state)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ItemState {
    /// In place of its title, as shown (translated).
    pub label: Option<Cow<'static, str>>,
    /// Shown with a switch, on or off.
    pub checked: Option<bool>,
    /// It can be chosen; else it is shown faint.
    pub enabled: bool,
    /// It is shown at all.
    pub shown: bool,
    /// A short text at its right (a count, `new`).
    pub badge: Option<Cow<'static, str>>,
}

impl Default for ItemState {
    fn default() -> Self {
        Self::new()
    }
}

impl ItemState {
    /// Shown as it is, and can be chosen.
    pub const fn new() -> Self {
        Self {
            label: None,
            checked: None,
            enabled: true,
            shown: true,
            badge: None,
        }
    }

    pub fn label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        self.checked = Some(on);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn shown(mut self, shown: bool) -> Self {
        self.shown = shown;
        self
    }

    pub fn badge(mut self, badge: impl Into<Cow<'static, str>>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// What a place with room for one short line (a bar, a panel's
    /// button, a tile) shows: the label in place of `title` where there
    /// is one, the badge after it.
    pub fn text(&self, title: &str) -> String {
        let label = self.label.as_deref().unwrap_or(title);
        match &self.badge {
            Some(badge) => alloc::format!("{label} · {badge}"),
            None => String::from(label),
        }
    }
}

/// What an entry shows now.
pub type ItemStateFn = Box<dyn Fn(&crate::Env<'_>) -> ItemState>;

struct StateFn(ItemStateFn);

impl fmt::Debug for StateFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ItemStateFn")
    }
}

/// Puts an action or a setting into a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuEntry {
    /// The menu, e.g. [`builtin::menus::MAIN`](crate::builtin::menus::MAIN)
    /// or `reader.menu`.
    pub menu: Id,
    /// An action, run when the entry is chosen, or a setting, shown with
    /// its current value and changed in place. Its title is the label. An
    /// action named like a menu with entries is shown as that submenu.
    pub item: Id,
    /// Position; lower comes first.
    pub order: i16,
    /// Where it goes beside other entries, in place of its order.
    pub place: Option<MenuPlace>,
}

impl MenuEntry {
    /// Made of what it needs; the rest as set with its methods. A `const`
    /// fn, so that a plugin's entries can be a `static` list
    /// ([`Registrar::add_menu_entries`](crate::Registrar::add_menu_entries)).
    pub const fn new(menu: Id, item: Id) -> Self {
        Self {
            menu,
            item,
            order: 0,
            place: None,
        }
    }

    pub const fn order(mut self, order: i16) -> Self {
        self.order = order;
        self
    }

    /// Placed beside other entries, e.g. right after another plugin's.
    pub fn place(mut self, place: MenuPlace) -> Self {
        self.place = Some(place);
        self
    }

    /// Put into `menu` at `place`, as [`MenuEntry::place`] puts it; for
    /// `static` lists.
    pub const fn placed(menu: Id, item: Id, place: MenuPlace) -> Self {
        Self {
            menu,
            item,
            order: 0,
            place: Some(place),
        }
    }

    /// Where it goes as entries placed by order go.
    fn by_order(&self) -> Option<i16> {
        match &self.place {
            None => Some(self.order),
            Some(MenuPlace::Order(order)) => Some(*order),
            Some(_) => None,
        }
    }
}

/// A heading in a menu: the entries after it, up to the next heading,
/// belong to it (see [`Registrar::add_menu_section`](crate::Registrar::add_menu_section)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuSection {
    /// `<plugin>.<name>`; also usable in the user's menu definitions.
    pub id: Id,
    /// Translation key of the heading.
    pub title: Cow<'static, str>,
    pub menu: Id,
    /// Position among the menu's entries; lower comes first.
    pub order: i16,
}

impl MenuSection {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, menu: Id) -> Self {
        Self {
            id,
            title: title.into(),
            menu,
            order: 0,
        }
    }

    pub fn order(mut self, order: i16) -> Self {
        self.order = order;
        self
    }
}

/// What a menu entry needs to be offered (see
/// [`Registrar::set_menu_need`](crate::Registrar::set_menu_need)), so
/// menus stay short on every device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenuNeed {
    /// A device that speaks ([`Capabilities::speech`](crate::Capabilities)): hidden
    /// elsewhere, also where the user listed it.
    Speech,
    /// A device where PaperPaw shows the sleep screen
    /// ([`Capabilities::sleep`](crate::Capabilities)), hidden elsewhere.
    Sleep,
    /// A device whose screen PaperPaw can turn
    /// ([`settings::ROTATION`](crate::builtin::settings::ROTATION)),
    /// hidden elsewhere.
    Rotation,
    /// A device without a touch screen, where it is the way to do what a
    /// tap or a long press does elsewhere (e.g. choosing a link). On touch
    /// devices it is left out of the menus plugins define; users can still
    /// list it.
    NoTouch,
    /// A device with a touch screen, e.g. for what a tap or a swipe does:
    /// hidden elsewhere, also where the user listed it.
    Touch,
    /// A device with buttons in a row below the screen, whose row of
    /// hints shows what they do
    /// ([`settings::BUTTON_HINTS`](crate::builtin::settings::BUTTON_HINTS)),
    /// hidden elsewhere.
    ButtonRow,
    /// A device whose shell shows the screen in other colours than black
    /// and white ([`settings::TEXT_COLOR`](crate::builtin::settings::TEXT_COLOR)),
    /// hidden elsewhere.
    Colors,
    /// A device where PaperPaw manages Wi-Fi itself
    /// ([`Env::wifi`](crate::Env)), hidden elsewhere.
    Wifi,
    /// A device whose firmware PaperPaw is, updated by PaperPaw
    /// ([`Env::firmware`](crate::Env)), hidden elsewhere.
    Firmware,
    /// A device whose panel shows black and white alone
    /// ([`Capabilities::black_and_white`](crate::Capabilities)), hidden
    /// elsewhere.
    BlackAndWhite,
    /// A front light PaperPaw sets ([`Capabilities::frontlight`](crate::Capabilities)).
    Frontlight,
    /// A Bluetooth radio the kernel uses ([`Env::bluetooth`](crate::Env)),
    /// hidden elsewhere.
    Bluetooth,
    /// A device that can do a [`Capability`](crate::Capability) (new
    /// needs come as capabilities, without a new plugin interface).
    Has(crate::Capability),
}

impl MenuNeed {
    /// Whether a device that can do `caps` has what it needs (also for
    /// [`ChromeSpec::need`](crate::chrome::ChromeSpec::need)).
    pub fn met(self, caps: crate::Capabilities) -> bool {
        match self {
            MenuNeed::Speech => caps.speech,
            MenuNeed::Sleep => caps.sleep,
            MenuNeed::Rotation => caps.rotation,
            MenuNeed::Colors => caps.colors,
            MenuNeed::ButtonRow => caps.button_row,
            MenuNeed::Wifi => caps.wifi,
            MenuNeed::Bluetooth => caps.bluetooth,
            MenuNeed::Firmware => caps.firmware,
            MenuNeed::BlackAndWhite => caps.black_and_white,
            MenuNeed::NoTouch => !caps.touch,
            MenuNeed::Touch => caps.touch,
            MenuNeed::Frontlight => caps.frontlight,
            MenuNeed::Has(capability) => caps.has(capability),
        }
    }
}

/// One entry of a resolved menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuItem<'a> {
    /// An action or a setting, or a section.
    pub id: &'a Id,
    /// Translation key of the label.
    pub title: &'a str,
    /// The icon's name ([`icons`](crate::icons)), if it has one.
    pub icon: Option<&'a str>,
    /// A heading rather than an entry ([`MenuSection`]).
    pub section: bool,
}

/// The menus as plugins read them ([`Env::menus`](crate::Env::menus)):
/// [`Menus`], with the labels of the actions and settings they list read
/// from those, not kept twice.
#[derive(Clone, Copy)]
pub struct MenuView<'a> {
    menus: &'a Menus,
    actions: &'a crate::Actions,
    settings: &'a crate::Settings,
}

impl fmt::Debug for MenuView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.menus, f)
    }
}

impl core::ops::Deref for MenuView<'_> {
    type Target = Menus;

    fn deref(&self) -> &Menus {
        self.menus
    }
}

impl<'a> MenuView<'a> {
    /// The menus, labelled by `actions` and `settings`.
    pub fn new(
        menus: &'a Menus,
        actions: &'a crate::Actions,
        settings: &'a crate::Settings,
    ) -> Self {
        Self {
            menus,
            actions,
            settings,
        }
    }

    /// The label of an item (a translation key): a heading's, an
    /// action's or a setting's.
    pub fn title(&self, item: &str) -> Option<&'a str> {
        self.menus
            .titles
            .get(item)
            .map(AsRef::as_ref)
            .or_else(|| self.actions.get(item).map(crate::Action::title))
            .or_else(|| self.settings.spec(item).map(|spec| spec.title.as_ref()))
    }

    /// The entries of `menu`, in display order. Items without a known
    /// title are skipped.
    pub fn items(&self, menu: &str) -> Vec<MenuItem<'a>> {
        self.menus
            .ids(menu)
            .into_iter()
            .filter_map(|id| self.item(id))
            .collect()
    }

    /// The entries of `menu` shown now: as [`MenuView::items`], without
    /// those their plugins hide at the moment ([`ItemState::shown`]).
    pub fn shown_items(&self, env: &crate::Env<'_>, menu: &str) -> Vec<MenuItem<'a>> {
        self.items(menu)
            .into_iter()
            .filter(|item| item.section || self.menus.state(env, item.id.as_str()).shown)
            .collect()
    }

    /// The menus themselves, for as long as they are lent.
    pub fn menus(&self) -> &'a Menus {
        self.menus
    }

    /// As [`Menus::icon`], for as long as the menus are lent.
    pub fn icon(&self, item: &str) -> Option<&'a str> {
        self.menus.icon(item)
    }

    /// As [`Menus::description`], for as long as the menus are lent.
    pub fn description(&self, item: &str) -> Option<&'a str> {
        self.menus.description(item)
    }

    /// As [`Menus::ids`], for as long as the menus are lent.
    pub fn ids(&self, menu: &str) -> Vec<&'a Id> {
        self.menus.ids(menu)
    }

    /// As [`Menus::known`], for as long as the menus are lent.
    pub fn known(&self, menu: &str) -> Option<&'a [Id]> {
        self.menus.known(menu)
    }

    /// As [`Menus::hidden_by`], for as long as the menus are lent.
    pub fn hidden_by(&self, menu: &str, item: &str) -> Option<&'a Id> {
        self.menus.hidden_by(menu, item)
    }

    /// As [`Menus::menu_ids`], for as long as the menus are lent.
    pub fn menu_ids(&self) -> Vec<&'a Id> {
        self.menus.menu_ids()
    }

    /// As [`Menus::default_ids`], for as long as the menus are lent.
    pub fn default_ids(&self, menu: &str) -> Vec<&'a Id> {
        self.menus.default_ids(menu)
    }

    /// As [`Menus::contributed_ids`], for as long as the menus are lent.
    pub fn contributed_ids(&self, menu: &str) -> Vec<&'a Id> {
        self.menus.contributed_ids(menu)
    }

    fn item(&self, id: &'a Id) -> Option<MenuItem<'a>> {
        self.title(id.as_str()).map(|title| MenuItem {
            id,
            title,
            icon: self.menus.icon(id.as_str()),
            section: self.menus.is_section(id.as_str()),
        })
    }
}

/// All menus: contributed entries, the user's menu definitions and the
/// headings' titles; the labels of actions and settings are theirs
/// ([`MenuView`]).
#[derive(Debug, Default)]
pub struct Menus {
    entries: Vec<MenuEntry>,
    /// Entries kept in the program (`static` lists of plugins').
    fixed: Vec<&'static [MenuEntry]>,
    /// Menus redefined by the user (`[menus] reader.menu = a, b, c`).
    custom: VecMap<Id, Vec<Id>>,
    /// The entries plugins contributed to a menu when the user redefined
    /// it, and the user left out (`[menus-known] reader.menu = …`): those
    /// that come later are added to the user's list.
    known: VecMap<Id, Vec<Id>>,
    /// Entries plugins took out of menus: (menu, item, the plugin).
    hidden: Vec<(Id, Id, Id)>,
    /// What entries show now, where it changes.
    states: VecMap<Id, StateFn>,
    /// A line under an entry (translation keys). Names of a hundred
    /// entries each, so in two words a name rather than three.
    descriptions: VecMap<Id, Text>,
    titles: VecMap<Id, Text>,
    icons: VecMap<Id, Text>,
    /// Icons kept in the program (`static` lists of plugins'); those of
    /// `icons` win, and a later list over an earlier one.
    fixed_icons: Vec<&'static [(Id, &'static str)]>,
    sections: Vec<Id>,
    needs: VecMap<Id, MenuNeed>,
    /// The device has a touch screen; it cannot speak; PaperPaw shows
    /// its sleep screen.
    /// What the device can do, for entries with a [`MenuNeed`].
    caps: crate::Capabilities,
}

impl Menus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.entries.shrink_to_fit();
        self.fixed.shrink_to_fit();
        self.fixed_icons.shrink_to_fit();
        self.custom.shrink_to_fit();
        for menu in self.custom.values_mut() {
            menu.shrink_to_fit();
        }
        self.known.shrink_to_fit();
        for known in self.known.values_mut() {
            known.shrink_to_fit();
        }
        self.hidden.shrink_to_fit();
        self.states.shrink_to_fit();
        self.descriptions.shrink_to_fit();
        self.titles.shrink_to_fit();
        self.icons.shrink_to_fit();
        self.sections.shrink_to_fit();
        self.needs.shrink_to_fit();
    }

    /// What the device can do, for entries with a [`MenuNeed`].
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_capabilities(&mut self, caps: crate::Capabilities) {
        self.caps = caps;
    }

    /// Offers `item` only where `need` is met.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_need(&mut self, item: Id, need: MenuNeed) {
        self.needs.insert(item, need);
    }

    /// Whether `item` is left out by the menu parameter
    /// [`HIDE`](crate::builtin::params::HIDE) of `params`.
    pub fn hidden(params: &crate::Params, item: &str) -> bool {
        params
            .get(crate::builtin::params::HIDE)
            .is_some_and(|hide| hide.split(',').any(|h| h.trim() == item))
    }

    /// Whether `item` is offered on this device by default, e.g. whether
    /// the settings screen lists a setting.
    pub fn available(&self, item: &str) -> bool {
        self.offered(item, false)
    }

    /// Whether `item` is offered here; `listed` if the user put it into
    /// the menu.
    fn offered(&self, item: &str, listed: bool) -> bool {
        let caps = &self.caps;
        match self.needs.get(item) {
            // Put into a menu by the user, it shows also on touch screens.
            Some(MenuNeed::NoTouch) => listed || !caps.touch,
            Some(need) => need.met(*caps),
            None => true,
        }
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, entry: MenuEntry) {
        self.entries.push(entry);
    }

    /// Adds entries kept in the program.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_fixed(&mut self, entries: &'static [MenuEntry]) {
        self.fixed.push(entries);
    }

    /// Adds icons kept in the program.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_fixed_icons(&mut self, icons: &'static [(Id, &'static str)]) {
        self.fixed_icons.push(icons);
    }

    /// Every entry: those kept in the program, then the others.
    fn all(&self) -> impl Iterator<Item = &MenuEntry> + Clone {
        self.fixed
            .iter()
            .flat_map(|list| list.iter())
            .chain(&self.entries)
    }

    /// Replaces a menu's entries with exactly `actions`, in this order.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn customize(&mut self, menu: Id, actions: Vec<Id>) {
        self.custom.insert(menu, actions);
    }

    /// The entries plugins contributed to `menu` when the user redefined
    /// it and the user left out: entries added later (neither these nor
    /// listed) are shown in the user's list too.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_known(&mut self, menu: Id, mut known: Vec<Id>) {
        known.shrink_to_fit();
        self.known.insert(menu, known);
    }

    /// The entries the user left out of `menu` when redefining it, if
    /// kept.
    pub fn known(&self, menu: &str) -> Option<&[Id]> {
        self.known.get(menu).map(Vec::as_slice)
    }

    /// Returns a menu to the entries plugins contribute.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn reset(&mut self, menu: &str) {
        self.custom.remove(menu);
        self.known.remove(menu);
    }

    /// Takes `item` out of `menu` as plugins contribute it (another
    /// plugin's entry, say), for plugin `by`; a user who lists it keeps it.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn hide(&mut self, menu: Id, item: Id, by: Id) {
        self.hidden.push((menu, item, by));
    }

    /// The plugin that took `item` out of `menu`, if one did (for menu
    /// editors to tell).
    pub fn hidden_by(&self, menu: &str, item: &str) -> Option<&Id> {
        self.hidden
            .iter()
            .find(|(m, i, _)| m.as_str() == menu && i.as_str() == item)
            .map(|(_, _, by)| by)
    }

    /// What `item` shows now, where it changes.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_state(&mut self, item: Id, state: ItemStateFn) {
        self.states.insert(item, StateFn(state));
    }

    /// A line under `item` (a translation key).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_description(&mut self, item: Id, key: Cow<'static, str>) {
        self.descriptions.insert(item, Text::new(key));
    }

    /// What `item` shows now: as it is unless its plugin says otherwise.
    pub fn state(&self, env: &crate::Env<'_>, item: &str) -> ItemState {
        self.states
            .get(item)
            .map_or_else(ItemState::new, |state| (state.0)(env))
    }

    /// The line under `item` (a translation key), if it has one.
    pub fn description(&self, item: &str) -> Option<&str> {
        self.descriptions.get(item).map(AsRef::as_ref)
    }

    /// Whether the user redefined `menu`.
    pub fn is_customized(&self, menu: &str) -> bool {
        self.custom.contains_key(menu)
    }

    /// Every menu: those plugins contribute to and those the user defined.
    pub fn menu_ids(&self) -> Vec<&Id> {
        let mut ids: Vec<&Id> = self
            .all()
            .map(|e| &e.menu)
            .chain(self.custom.keys())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The ids of `menu`'s entries as plugins contribute them, ignoring
    /// the user's definition: by order, then those placed beside others
    /// (an entry whose anchor is missing goes last).
    pub fn default_ids(&self, menu: &str) -> Vec<&Id> {
        self.contributed_ids(menu)
            .into_iter()
            .filter(|id| self.offered(id.as_str(), false))
            .collect()
    }

    /// Every entry plugins contribute to `menu` in their places, also
    /// those this device does not offer.
    pub fn contributed_ids(&self, menu: &str) -> Vec<&Id> {
        let in_menu = |e: &&MenuEntry| e.menu.as_str() == menu;
        // An entry placed beside others takes the place of the same entry
        // by order (another plugin's, say): it moves it.
        let placed_too = |e: &MenuEntry| {
            e.by_order().is_some()
                && self
                    .all()
                    .filter(in_menu)
                    .any(|other| other.item == e.item && other.by_order().is_none())
        };
        let entries: Vec<&MenuEntry> = self
            .all()
            .filter(in_menu)
            .filter(|e| !placed_too(e))
            .filter(|e| {
                !self
                    .hidden
                    .iter()
                    .any(|(m, i, _)| m.as_str() == menu && *i == e.item)
            })
            .collect();
        let mut ordered: Vec<&MenuEntry> = entries
            .iter()
            .copied()
            .filter(|e| e.by_order().is_some())
            .collect();
        ordered.sort_by_cached_key(|e| e.by_order());
        let mut ids: Vec<&Id> = ordered.into_iter().map(|e| &e.item).collect();
        let mut placed: Vec<&MenuEntry> = entries
            .into_iter()
            .filter(|e| e.by_order().is_none())
            .collect();
        // Those put first or after an entry, and where: several in one
        // place keep the order they were registered in.
        let mut put: Vec<(&Id, &MenuPlace)> = Vec::new();
        // Anchors placed themselves come first: as long as one finds its
        // anchor.
        loop {
            let before = placed.len();
            placed.retain(|e| {
                let e: &MenuEntry = e;
                let at = match e.place.as_ref() {
                    Some(MenuPlace::First) => Some(0),
                    Some(MenuPlace::After(anchor)) => {
                        ids.iter().position(|id| *id == anchor).map(|i| i + 1)
                    }
                    Some(MenuPlace::Before(anchor)) => ids.iter().position(|id| *id == anchor),
                    Some(MenuPlace::InSection(section)) => {
                        ids.iter().position(|id| *id == section).map(|start| {
                            ids[start + 1..]
                                .iter()
                                .position(|id| self.is_section(id.as_str()))
                                .map_or(ids.len(), |next| start + 1 + next)
                        })
                    }
                    _ => None,
                };
                match (at, e.place.as_ref()) {
                    (Some(mut at), Some(place)) => {
                        if matches!(place, MenuPlace::First | MenuPlace::After(_)) {
                            while ids
                                .get(at)
                                .is_some_and(|id| put.iter().any(|(p, pl)| p == id && *pl == place))
                            {
                                at += 1;
                            }
                            put.push((&e.item, place));
                        }
                        ids.insert(at, &e.item);
                        false
                    }
                    _ => true,
                }
            });
            if placed.len() == before {
                break;
            }
        }
        // Last, and those whose anchor is not there.
        ids.extend(placed.into_iter().map(|e| &e.item));
        ids
    }

    /// The ids of `menu`'s entries as shown, including entries without a
    /// label. A menu the user redefined shows the user's list, and the
    /// entries plugins added since after the entry before them.
    pub fn ids(&self, menu: &str) -> Vec<&Id> {
        let Some(custom) = self.custom.get(menu) else {
            return self.default_ids(menu);
        };
        // The user's, and those added since: offered as plugins'.
        let mut ids: Vec<(&Id, bool)> = custom.iter().map(|id| (id, true)).collect();
        if let Some(known) = self.known.get(menu) {
            let all = self.contributed_ids(menu);
            for (i, new) in all.iter().enumerate() {
                if known.contains(new) || ids.iter().any(|(id, _)| id == new) {
                    continue;
                }
                // Last where nothing after it is in the list (as a plugin
                // puts it last); else after the nearest entry before it
                // the list has.
                let listed = |id: &&Id| ids.iter().any(|(listed, _)| listed == id);
                let at = match all[i + 1..].iter().any(listed) {
                    false => ids.len(),
                    true => all[..i]
                        .iter()
                        .rev()
                        .find_map(|before| ids.iter().position(|(id, _)| id == before))
                        .map_or(0, |at| at + 1),
                };
                ids.insert(at, (new, false));
            }
        }
        ids.into_iter()
            .filter(|(id, listed)| self.offered(id.as_str(), *listed))
            .map(|(id, _)| id)
            .collect()
    }

    /// Registers the label of an item that is no action or setting.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_title(&mut self, item: Id, title: Cow<'static, str>) {
        self.titles.insert(item, Text::new(title));
    }

    /// Gives an action or setting an icon (by name; see
    /// [`icons`](crate::icons)), wherever it is shown.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_icon(&mut self, item: Id, icon: Cow<'static, str>) {
        self.icons.insert(item, Text::new(icon));
    }

    /// The icon of an action or setting, if it has one.
    pub fn icon(&self, item: &str) -> Option<&str> {
        self.icons.get(item).map(AsRef::as_ref).or_else(|| {
            self.fixed_icons
                .iter()
                .rev()
                .find_map(|list| list.iter().find(|(id, _)| id.as_str() == item))
                .map(|&(_, icon)| icon)
        })
    }

    /// Adds a heading to a menu.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_section(&mut self, section: MenuSection) {
        self.titles
            .insert(section.id.clone(), Text::new(section.title));
        self.sections.push(section.id.clone());
        self.entries.push(MenuEntry {
            menu: section.menu,
            item: section.id,
            order: section.order,
            place: None,
        });
    }

    /// The position of `item` in `menu` as plugins contribute it, e.g. to
    /// place it among other things listed with the menu's entries.
    pub fn order_of(&self, menu: &str, item: &str) -> Option<i16> {
        self.all()
            .find(|e| e.menu.as_str() == menu && e.item.as_str() == item)
            .map(|e| e.order)
    }

    /// Whether `item` is a heading.
    pub fn is_section(&self, item: &str) -> bool {
        self.sections.iter().any(|s| s.as_str() == item)
    }

    /// Every action or setting referenced by a menu, for validation.
    pub fn referenced_items(&self) -> impl Iterator<Item = &Id> {
        self.all()
            .map(|e| &e.item)
            .chain(self.custom.values().flatten())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn ids(menus: &Menus) -> Vec<&str> {
        menus.ids("m.menu").iter().map(|i| i.as_str()).collect()
    }

    fn entry(item: &'static str, order: i16) -> MenuEntry {
        MenuEntry::new(Id::new_static("m.menu"), Id::new_static(item)).order(order)
    }

    #[test]
    fn entries_go_where_they_are_placed() {
        let mut menus = Menus::new();
        menus.add(entry("m.b", 20));
        menus.add(entry("m.a", 10));
        let at = |item: &'static str, place: MenuPlace| {
            MenuEntry::new(Id::new_static("m.menu"), Id::new_static(item)).place(place)
        };
        // After one placed itself later; before one by order.
        menus.add(at("m.after-x", MenuPlace::After(Id::new_static("m.x"))));
        menus.add(at("m.x", MenuPlace::Before(Id::new_static("m.b"))));
        menus.add(at("m.first", MenuPlace::First));
        menus.add(at("m.last", MenuPlace::Last));
        menus.add(at("m.lost", MenuPlace::After(Id::new_static("m.nowhere"))));
        menus.add_section(
            MenuSection::new(Id::new_static("m.part"), "part", Id::new_static("m.menu")).order(15),
        );
        menus.add(at(
            "m.in-part",
            MenuPlace::InSection(Id::new_static("m.part")),
        ));
        assert_eq!(
            ids(&menus),
            // b (20) is in the part (15): last in it is after b.
            [
                "m.first",
                "m.a",
                "m.part",
                "m.x",
                "m.after-x",
                "m.b",
                "m.in-part",
                "m.last",
                "m.lost"
            ]
        );
        // Taken out by a plugin, which menu editors can tell.
        menus.hide(
            Id::new_static("m.menu"),
            Id::new_static("m.a"),
            Id::new_static("p"),
        );
        assert!(!ids(&menus).contains(&"m.a"));
        let by = menus.hidden_by("m.menu", "m.a");
        assert_eq!(by.map(Id::as_str), Some("p"));
        assert_eq!(menus.hidden_by("m.menu", "m.b"), None);
    }

    #[test]
    fn an_entry_placed_moves_the_same_one_by_order() {
        let mut menus = Menus::new();
        for (item, order) in [("m.a", 10), ("m.b", 20), ("m.c", 30)] {
            menus.add(entry(item, order));
        }
        // Another plugin puts c right after a: moved, not twice.
        menus.add(
            MenuEntry::new(Id::new_static("m.menu"), Id::new_static("m.c"))
                .place(MenuPlace::After(Id::new_static("m.a"))),
        );
        assert_eq!(ids(&menus), ["m.a", "m.c", "m.b"]);
    }

    #[test]
    fn several_in_one_place_keep_their_order() {
        let mut menus = Menus::new();
        menus.add(entry("m.a", 10));
        menus.add(entry("m.b", 20));
        let at = |item: &'static str, place: MenuPlace| {
            MenuEntry::new(Id::new_static("m.menu"), Id::new_static(item)).place(place)
        };
        let after_a = || MenuPlace::After(Id::new_static("m.a"));
        menus.add(at("m.first-1", MenuPlace::First));
        menus.add(at("m.after-1", after_a()));
        menus.add(at("m.first-2", MenuPlace::First));
        menus.add(at("m.after-2", after_a()));
        assert_eq!(
            ids(&menus),
            [
                "m.first-1",
                "m.first-2",
                "m.a",
                "m.after-1",
                "m.after-2",
                "m.b"
            ]
        );
    }

    #[test]
    fn a_users_list_shows_entries_added_since_after_the_one_before() {
        let mut menus = Menus::new();
        for (item, order) in [("m.a", 10), ("m.b", 20), ("m.c", 30)] {
            menus.add(entry(item, order));
        }
        // Without b, c before a, knowing a, b and c.
        let menu = Id::new_static("m.menu");
        menus.customize(
            menu.clone(),
            vec![Id::new_static("m.c"), Id::new_static("m.a")],
        );
        // Known: what was there and left out (b).
        menus.set_known(menu.clone(), vec![Id::new_static("m.b")]);
        // A plugin installed later adds two: after a, and first.
        menus.add(entry("m.new", 15));
        menus.add(entry("m.top", 1));
        assert_eq!(ids(&menus), ["m.top", "m.c", "m.a", "m.new"]);
        // A list kept without knowing stays as it is.
        menus.known.remove("m.menu");
        assert_eq!(ids(&menus), ["m.c", "m.a"]);
    }

    #[test]
    fn orders_entries_and_honours_user_menus() {
        let mut menus = Menus::new();
        for (action, order) in [("a.second", 20), ("a.first", 10), ("a.untitled", 5)] {
            menus.add(MenuEntry {
                menu: Id::new_static("a.menu"),
                item: Id::parse(action).unwrap(),
                order,
                place: None,
            });
        }
        menus.set_title(Id::new_static("a.first"), Cow::Borrowed("First"));
        menus.set_title(Id::new_static("a.second"), Cow::Borrowed("Second"));
        fn titles(menus: &Menus) -> Vec<String> {
            let (actions, settings) = (crate::Actions::new(), crate::Settings::new());
            let view = MenuView::new(menus, &actions, &settings);
            view.items("a.menu")
                .iter()
                .map(|i| String::from(i.title))
                .collect()
        }
        assert_eq!(titles(&menus), ["First", "Second"]);

        menus.customize(Id::new_static("a.menu"), vec![Id::new_static("a.second")]);
        assert_eq!(titles(&menus), ["Second"]);
        let (actions, settings) = (crate::Actions::new(), crate::Settings::new());
        assert!(
            MenuView::new(&menus, &actions, &settings)
                .items("other")
                .is_empty()
        );

        // Entries for devices without touch, or that speak.
        menus.reset("a.menu");
        menus.add(MenuEntry {
            menu: Id::new_static("a.menu"),
            item: Id::new_static("a.links"),
            order: 30,
            place: None,
        });
        menus.set_title(Id::new_static("a.links"), Cow::Borrowed("Links"));
        menus.set_need(Id::new_static("a.links"), MenuNeed::NoTouch);
        assert_eq!(titles(&menus).last().map(String::as_str), Some("Links"));
        menus.set_capabilities(crate::Capabilities {
            touch: true,
            ..Default::default()
        });
        assert_eq!(titles(&menus).last().map(String::as_str), Some("Second"));
        // One for touch screens shows only on them.
        menus.set_need(Id::new_static("a.second"), MenuNeed::Touch);
        assert_eq!(titles(&menus).last().map(String::as_str), Some("Second"));
        menus.set_capabilities(crate::Capabilities::default());
        assert_eq!(titles(&menus).last().map(String::as_str), Some("Links"));
        menus.set_capabilities(crate::Capabilities {
            touch: true,
            ..Default::default()
        });
        // Listed by the user, it shows; one that needs speech does not.
        menus.set_need(Id::new_static("a.second"), MenuNeed::Speech);
        menus.customize(
            Id::new_static("a.menu"),
            alloc::vec![Id::new_static("a.links"), Id::new_static("a.second")],
        );
        assert_eq!(titles(&menus), ["Links"]);

        // Left out where a menu is opened.
        let params = crate::Params::new().with(crate::builtin::params::HIDE, "a.first, a.links");
        assert!(Menus::hidden(&params, "a.links"));
        assert!(!Menus::hidden(&params, "a.second"));
        assert!(!Menus::hidden(&crate::Params::new(), "a.links"));
    }
}
