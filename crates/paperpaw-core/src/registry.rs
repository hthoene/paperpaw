//! Contribution registries and plugin registration.

use alloc::borrow::Cow;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::kit::action::ActionSpec;
use crate::kit::action::Actions;
use crate::kit::contribution::item::Contribution;
use crate::kit::menus::MenuEntry;
use crate::kit::status::StatusItemSpec;
use crate::kit::status::StatusLineSpec;
use paperpaw_i18n::Catalog;
use paperpaw_sdk::collections::VecMap;
use paperpaw_sdk::{FontSpec, FormatSpec, Id, Priority, RegisterError, SettingSpec, SlotSpec};

pub(crate) const NATIVE_CAPACITY_OVERFLOW: &str = "native screen capacity overflow";
pub(crate) const NATIVE_CAPACITY_UNAVAILABLE: &str = "native screen capacity unavailable";

/// A candidate screen for a slot, proposed by a plugin.
#[derive(Debug)]
struct SlotCandidate {
    slot: Id,
    screen: Id,
    priority: Priority,
}

/// The screens plugins added: each id with the translation key of its name,
/// as [`Place::screens`](crate::Place) hands them out (not copied for it),
/// and apart from them what makes the screen. Three lists, each sorted by id,
/// no id in two of them: interface-6 plugins' screens, interface-7 plugins'
/// (their id and title a linked plugin's entry kept in the program, #248 B)
/// and those kept in the program.
#[derive(Default)]
pub(crate) struct Screens {
    /// Tests' screens (feature `testing`), with what makes each.
    titles: Vec<(Id, Cow<'static, str>)>,
    #[cfg(any(test, feature = "testing"))]
    made: Vec<AddedMade>,
    native: Vec<paperpaw_sdk::Titled>,
    native_made: Vec<crate::native_host::native_screen_factory::NativeMade>,
    /// Those kept in the program; the one added last of an id stays.
    fixed: Vec<Fixed>,
    /// Their ids and titles, in the same order ([`Screens::list`]).
    fixed_titles: Vec<&'static paperpaw_sdk::TitleEntry>,
}

/// One of the host's own screens (#293).
pub(crate) type Fixed = &'static crate::host_screen::HostEntry;

/// What makes a test's screen (`testing`).
#[cfg(any(test, feature = "testing"))]
pub(crate) struct AddedMade {
    pub(crate) factory: crate::testing::FakeFactory,
    pub(crate) flags: u32,
    pub(crate) refresh_on_open: Option<paperpaw_sdk::RefreshMode>,
    pub(crate) instances: paperpaw_sdk::Instances,
}

#[cfg(any(test, feature = "testing"))]
impl AddedMade {
    /// A native screen whose plugin is the test's screen made for `params`.
    fn make(
        &self,
        id: &Id,
        env: &crate::Place<'_>,
        params: &paperpaw_sdk::Params,
    ) -> Result<crate::screen::Screen, i32> {
        let fake = (self.factory)(env, params);
        let flags = self.flags | fake.flags();
        let fake = crate::testing::FakeInstance::new(fake);
        let lease = crate::native_host::native_instances::NativeLease::try_new(fake)?;
        let id = id
            .try_clone()
            .ok_or(paperpaw_native::host_glue::ERR_NO_MEMORY)?;
        let screen =
            crate::native_host::native_screen::NativeScreen::new(id, 0, lease).with_flags(flags);
        Ok(crate::screen::Screen::Native(alloc::boxed::Box::new(
            screen,
        )))
    }
}

/// What makes a screen of either kind.
#[derive(Clone, Copy)]
pub(crate) enum Maker<'a> {
    /// A test's screen, with its id.
    #[cfg(any(test, feature = "testing"))]
    Added(&'a Id, &'a AddedMade),
    /// With the screen's id, kept once, in the registry's titles.
    Native(
        &'a Id,
        &'a crate::native_host::native_screen_factory::NativeMade,
    ),
    Fixed(Fixed),
}

impl Maker<'_> {
    pub(crate) fn make(
        self,
        env: &crate::Place<'_>,
        params: &paperpaw_sdk::Params,
    ) -> Result<crate::screen::Screen, i32> {
        match self {
            #[cfg(any(test, feature = "testing"))]
            Maker::Added(id, made) => made.make(id, env, params),
            Maker::Fixed(fixed) => Ok(crate::screen::Screen::Host(fixed.make(env, params))),
            Maker::Native(id, made) => made.make(id, params),
        }
    }

    /// Native allocation must finish before the open path changes any state.
    pub(crate) fn prepare_native(
        self,
        params: &paperpaw_sdk::Params,
    ) -> Result<Option<crate::screen::Screen>, i32> {
        match self {
            Maker::Native(id, made) => made.make(id, params).map(Some),
            _ => Ok(None),
        }
    }

    /// Native wrappers keep their instance identity while the viewport changes.
    pub(crate) fn remake(
        self,
        screen: &mut crate::screen::Screen,
        env: &crate::Place<'_>,
        params: &paperpaw_sdk::Params,
    ) {
        if let Maker::Fixed(fixed) = self {
            *screen = crate::screen::Screen::Host(fixed.make(env, params));
        }
    }

    /// A test's screen (`testing`): its draws count as drawn, as the host's
    /// own do, with no plugin's callback to mark them.
    pub(crate) fn is_fake(self) -> bool {
        #[cfg(any(test, feature = "testing"))]
        if let Maker::Added(..) = self {
            return true;
        }
        false
    }

    pub(crate) fn is_native(self) -> bool {
        match self {
            Maker::Native(..) => true,
            #[cfg(any(test, feature = "testing"))]
            Maker::Added(..) => true,
            Maker::Fixed(_) => false,
        }
    }

    pub(crate) fn refresh_on_open(self) -> Option<paperpaw_sdk::RefreshMode> {
        match self {
            #[cfg(any(test, feature = "testing"))]
            Maker::Added(_, made) => made.refresh_on_open,
            Maker::Fixed(_) => None,
            Maker::Native(_, made) => made.refresh(),
        }
    }

    pub(crate) fn instances(self) -> paperpaw_sdk::Instances {
        match self {
            #[cfg(any(test, feature = "testing"))]
            Maker::Added(_, made) => made.instances,
            Maker::Fixed(_) => paperpaw_sdk::Instances::PER_PARAMS,
            Maker::Native(_, made) => made.instances(),
        }
    }
}

impl core::fmt::Debug for Screens {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.list().iter().map(|(id, _)| id))
            .finish()
    }
}

impl Screens {
    fn find(&self, id: &str) -> Result<usize, usize> {
        self.titles
            .binary_search_by(|(other, _)| other.as_str().cmp(id))
    }

    fn find_native(&self, id: &str) -> Result<usize, usize> {
        self.native
            .binary_search_by(|other| other.id().as_str().cmp(id))
    }

    fn find_fixed(&self, id: &str) -> Result<usize, usize> {
        self.fixed_titles
            .binary_search_by(|f| f.id.as_str().cmp(id))
    }

    pub(crate) fn contains_key<Q: core::borrow::Borrow<str> + ?Sized>(&self, id: &Q) -> bool {
        let id = id.borrow();
        self.find(id).is_ok() || self.find_native(id).is_ok() || self.find_fixed(id).is_ok()
    }

    pub(crate) fn get<Q: core::borrow::Borrow<str> + ?Sized>(&self, id: &Q) -> Option<Maker<'_>> {
        let id = id.borrow();
        #[cfg(any(test, feature = "testing"))]
        if let Ok(at) = self.find(id) {
            return Some(Maker::Added(&self.titles[at].0, &self.made[at]));
        }
        if let Ok(at) = self.find_native(id) {
            return Some(Maker::Native(self.native[at].id(), &self.native_made[at]));
        }
        self.find_fixed(id)
            .ok()
            .map(|at| Maker::Fixed(self.fixed[at]))
    }

    /// The id and title of every interface-7 plugin's screen.
    pub(crate) fn native_titles(&self) -> impl Iterator<Item = &paperpaw_sdk::Titled> {
        self.native.iter()
    }

    pub(crate) fn remove_native_owner(&mut self, owner: &str) {
        let mut at = 0;
        while at < self.native.len() {
            if self.native[at].id().namespace() == owner {
                self.native.remove(at);
                self.native_made.remove(at);
            } else {
                at += 1;
            }
        }
    }

    fn title(&self, id: &str) -> Option<&str> {
        if let Ok(at) = self.find(id) {
            return Some(self.titles[at].1.as_ref());
        }
        if let Ok(at) = self.find_native(id) {
            return Some(self.native[at].title());
        }
        self.find_fixed(id)
            .ok()
            .map(|at| self.fixed_titles[at].title)
    }

    /// Forgets the added and the native screen `id` (one at most has it).
    fn forget(&mut self, id: &str) {
        if let Ok(at) = self.find(id) {
            self.titles.remove(at);
            #[cfg(any(test, feature = "testing"))]
            self.made.remove(at);
        }
        if let Ok(at) = self.find_native(id) {
            self.native.remove(at);
            self.native_made.remove(at);
        }
    }

    /// Adds a screen kept in the program, in place of one of its id.
    pub(crate) fn insert_fixed(&mut self, screen: Fixed) {
        let id = screen.id().as_str();
        self.forget(id);
        match self.find_fixed(id) {
            Ok(at) => {
                self.fixed[at] = screen;
                self.fixed_titles[at] = screen.entry();
            }
            Err(at) => {
                self.fixed.insert(at, screen);
                self.fixed_titles.insert(at, screen.entry());
            }
        }
    }

    /// No test's screen is added outside the feature `testing`.
    #[cfg(not(any(test, feature = "testing")))]
    fn insert_fake(&mut self, spec: crate::FakeSpec) {
        match spec {}
    }

    /// Adds the test's screen `spec`, in place of a screen of its id.
    #[cfg(any(test, feature = "testing"))]
    fn insert_fake(&mut self, spec: crate::testing::FakeSpec) {
        let crate::testing::FakeSpec {
            id,
            title,
            factory,
            flags,
            refresh_on_open,
            instances,
        } = spec;
        let made = AddedMade {
            factory,
            flags,
            refresh_on_open,
            instances,
        };
        self.insert_made(id, title, made);
    }

    #[cfg(any(test, feature = "testing"))]
    fn insert_made(&mut self, id: Id, title: Cow<'static, str>, made: AddedMade) {
        if let Ok(at) = self.find_fixed(id.as_str()) {
            self.fixed.remove(at);
            self.fixed_titles.remove(at);
        }
        if let Ok(at) = self.find_native(id.as_str()) {
            self.native.remove(at);
            self.native_made.remove(at);
        }
        match self.find(id.as_str()) {
            Ok(at) => {
                self.titles[at].1 = title;
                self.made[at] = made;
            }
            Err(at) => {
                self.titles.insert(at, (id, title));
                self.made.insert(at, made);
            }
        }
    }

    fn insert_native(
        &mut self,
        screen: crate::native_host::native_screen_factory::NativeScreenSpec,
    ) {
        let id = screen.titled.id().as_str();
        if let Ok(at) = self.find_fixed(id) {
            self.fixed.remove(at);
            self.fixed_titles.remove(at);
        }
        if let Ok(at) = self.find(id) {
            self.titles.remove(at);
            #[cfg(any(test, feature = "testing"))]
            self.made.remove(at);
        }
        match self.find_native(id) {
            Ok(at) => {
                self.native[at] = screen.titled;
                self.native_made[at] = screen.made;
            }
            Err(at) => {
                self.native.insert(at, screen.titled);
                self.native_made.insert(at, screen.made);
            }
        }
    }

    /// Room for `added` more interface-6 and `native` more interface-7
    /// screens, so that inserting them cannot fail for lack of memory.
    fn try_reserve(
        &mut self,
        added: usize,
        native: usize,
    ) -> Result<(), alloc::collections::TryReserveError> {
        self.titles.try_reserve(added)?;
        #[cfg(any(test, feature = "testing"))]
        self.made.try_reserve(added)?;
        self.native.try_reserve(native)?;
        self.native_made.try_reserve(native)
    }

    /// Every screen with the translation key of its name.
    pub(crate) fn list(&self) -> paperpaw_sdk::ScreenList<'_> {
        paperpaw_sdk::ScreenList::new(&self.titles, &self.native, &self.fixed_titles)
    }

    fn shrink_to_fit(&mut self) {
        self.titles.shrink_to_fit();
        #[cfg(any(test, feature = "testing"))]
        self.made.shrink_to_fit();
        self.native.shrink_to_fit();
        self.native_made.shrink_to_fit();
        self.fixed.shrink_to_fit();
        self.fixed_titles.shrink_to_fit();
    }
}

/// A widget a plugin added.
pub(crate) enum StagedWidgets {
    One(alloc::boxed::Box<crate::kit::widgets::WidgetSpec>),
}

/// The events plugins subscribed to, plugin by plugin in the order they
/// registered: each plugin's id once, with how many of the entries after
/// those of the plugin before are its (a plugin subscribes to a few
/// events; there are some hundred and fifty subscriptions).
#[derive(Default)]
pub(crate) struct Subscriptions {
    plugins: Vec<(Id, u16)>,
    entries: Vec<Subscribed>,
}

/// An action of a data plugin: the steps the kernel runs for it.
#[derive(Debug)]
pub(crate) struct DataAction {
    pub(crate) id: Id,
    pub(crate) steps: alloc::boxed::Box<[crate::data_plugin::formats::Step]>,
}

/// A built-in's subscription: its event and handler.
pub(crate) struct Subscribed {
    pub(crate) event: Id,
    pub(crate) run: crate::Hear,
}

impl core::fmt::Debug for Subscribed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Subscribed").field(&self.event).finish()
    }
}

impl Subscriptions {
    /// Adds `subscriptions` (plugin, entry), emptying it.
    pub(crate) fn append(&mut self, subscriptions: &mut Vec<(Id, Subscribed)>) {
        for (plugin, entry) in subscriptions.drain(..) {
            match self.plugins.last_mut() {
                Some((last, count)) if *last == plugin && *count < u16::MAX => *count += 1,
                _ => self.plugins.push((plugin, 1)),
            }
            self.entries.push(entry);
        }
    }

    /// Every subscription: the event, the plugin, its handler.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&Id, &Id, crate::Hear)> + '_ {
        self.plugins
            .iter()
            .scan(0, |at, (plugin, count)| {
                let from = *at;
                *at += usize::from(*count);
                Some((plugin, from..*at))
            })
            .flat_map(move |(plugin, entries)| {
                self.entries[entries]
                    .iter()
                    .map(move |entry| (&entry.event, plugin, entry.run))
            })
    }

    /// The next subscription to `event` from `*at` on, its plugin and
    /// handler, moving `*at` past it: the handler may then be called with
    /// the kernel, which a borrowing iterator would hold.
    pub(crate) fn next_of(&self, at: &mut usize, event: &Id) -> Option<(Id, crate::Hear)> {
        while let Some(entry) = self.entries.get(*at) {
            let index = *at;
            *at += 1;
            if entry.event != *event {
                continue;
            }
            let mut end = 0;
            for (plugin, count) in &self.plugins {
                end += usize::from(*count);
                if index < end {
                    return Some((plugin.clone(), entry.run));
                }
            }
            return None;
        }
        None
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        self.plugins.shrink_to_fit();
        self.entries.shrink_to_fit();
    }
}

/// All active contributions except settings, key maps and translations,
/// which live in their own stores.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    pub(crate) native_declarations:
        Option<crate::native_host::native_declarations::DirectoryHandle>,
    /// The families the widgets' own font settings offer (#262), once a
    /// widget has one.
    pub(crate) widget_fonts: Option<crate::font_refresh::widget_fonts::Families>,
    /// The stand-ins of native formats in the kernel's `Formats` (their files
    /// are books of the library).
    pub(crate) native_book_formats: crate::native_host::native_book_formats::NativeBookFormats,
    pub(crate) screens: Screens,
    pub(crate) slots: VecMap<Id, SlotSpec>,
    candidates: Vec<SlotCandidate>,
    /// Slot bindings from the user's configuration.
    user_bindings: VecMap<Id, Id>,
}

impl Registry {
    /// Releases unused capacity once registration is complete.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.screens.shrink_to_fit();
        self.slots.shrink_to_fit();
        self.candidates.shrink_to_fit();
        self.user_bindings.shrink_to_fit();
    }

    /// How many screens are offered for slots.
    pub(crate) fn offers(&self) -> usize {
        self.candidates.len()
    }

    /// Every offer of a screen for a slot, in the order made: the slot, the
    /// screen, the priority (`registry_lines`).
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn offers_made(&self) -> impl Iterator<Item = (&Id, &Id, Priority)> + '_ {
        self.candidates
            .iter()
            .map(|c| (&c.slot, &c.screen, c.priority))
    }

    /// The priority of each offer, in the order made (for tests).
    #[cfg(test)]
    pub(crate) fn offer_priorities(&self) -> impl Iterator<Item = Priority> + '_ {
        self.candidates.iter().map(|c| c.priority)
    }

    /// Forgets the offers of `owner`'s screens, when it is unloaded.
    pub(crate) fn remove_native_offers(&mut self, owner: &str) {
        self.candidates
            .retain(|candidate| candidate.screen.namespace() != owner);
    }

    /// Every slot some screen is bound to.
    pub(crate) fn bound_slots(&self) -> Vec<Id> {
        let mut slots: Vec<Id> = Vec::new();
        for slot in self
            .candidates
            .iter()
            .map(|c| &c.slot)
            .chain(self.user_bindings.iter().map(|(slot, _)| slot))
        {
            if !slots.contains(slot) && self.resolve_slot(slot.as_str()).is_some() {
                slots.push(slot.clone());
            }
        }
        slots
    }

    /// [`Registry::bound_slots`] where the heap may be short: `None` when it
    /// has no room for the list (the caller keeps the list it has).
    pub(crate) fn try_bound_slots(&self) -> Option<Vec<Id>> {
        let mut slots: Vec<Id> = Vec::new();
        // At most one slot for each offer and binding: reserved once, so the
        // list never grows by doubling.
        slots
            .try_reserve_exact(self.candidates.len() + self.user_bindings.len())
            .ok()?;
        for slot in self
            .candidates
            .iter()
            .map(|c| &c.slot)
            .chain(self.user_bindings.iter().map(|(slot, _)| slot))
        {
            if !slots.contains(slot) && self.resolve_slot(slot.as_str()).is_some() {
                slots.push(slot.try_clone()?);
            }
        }
        Some(slots)
    }

    /// Keeps of `slots` those some screen is still bound to, without
    /// allocating: where the list cannot be made anew, a slot whose last offer
    /// went must not stay in it.
    pub(crate) fn retain_bound(&self, slots: &mut Vec<Id>) {
        slots.retain(|slot| self.resolve_slot(slot.as_str()).is_some());
    }

    /// The screen bound to `slot`: the user's choice if that screen exists,
    /// otherwise the candidate with the highest priority (the later
    /// registration wins a tie).
    pub(crate) fn resolve_slot(&self, slot: &str) -> Option<&Id> {
        if let Some(screen) = self.user_bindings.get(slot)
            && self.screens.contains_key(screen)
        {
            return Some(screen);
        }
        self.candidates
            .iter()
            .filter(|c| c.slot.as_str() == slot && self.screens.contains_key(&c.screen))
            .max_by_key(|c| c.priority)
            .map(|c| &c.screen)
    }

    /// Every screen offered for `slot`.
    pub(crate) fn slot_screens<'a>(&'a self, slot: &'a Id) -> impl Iterator<Item = &'a Id> {
        self.candidates
            .iter()
            .filter(move |c| c.slot == *slot && self.screens.contains_key(&c.screen))
            .map(|c| &c.screen)
    }

    pub(crate) fn bind_user(&mut self, slot: Id, screen: Id) {
        self.user_bindings.insert(slot, screen);
    }

    /// Back to what the plugins bind `slot` to.
    pub(crate) fn unbind_user(&mut self, slot: &Id) {
        self.user_bindings.remove(slot);
    }

    pub(crate) fn user_bindings(&self) -> impl Iterator<Item = (&Id, &Id)> {
        self.user_bindings.iter()
    }
}

impl paperpaw_sdk::ScreenMaker for Registry {
    fn slot_screen(&self, slot: &str) -> Option<&Id> {
        self.resolve_slot(slot)
    }

    fn slot_choice(&self, slot: &str) -> Option<&Id> {
        self.user_bindings
            .get(slot)
            .filter(|screen| self.screens.contains_key(screen.as_str()))
    }

    fn slot_title(&self, slot: &str) -> Option<&str> {
        self.slots.get(slot).map(|spec| spec.title.as_ref())
    }

    fn screen_title(&self, screen: &Id) -> Option<&str> {
        self.screens.title(screen.as_str())
    }

    /// The highest priority first, those of one priority in the order
    /// offered: the plugins' choice ([`Registry::resolve_slot`]) is the
    /// last of the first priority, so those before it bind the slot as
    /// high and those after it lower.
    fn slot_screens(&self, slot: &str) -> Vec<&Id> {
        let mut candidates: Vec<_> = self
            .candidates
            .iter()
            .filter(|c| c.slot.as_str() == slot && self.screens.contains_key(c.screen.as_str()))
            .collect();
        paperpaw_sort::sort_by_key(&mut candidates, |c| core::cmp::Reverse(c.priority));
        let mut screens: Vec<&Id> = Vec::new();
        for candidate in candidates {
            if !screens.contains(&&candidate.screen) {
                screens.push(&candidate.screen);
            }
        }
        screens
    }

    /// [`slot_screens`](Self::slot_screens)' order without a list: each
    /// priority from the highest down, those of one priority in the order
    /// offered, a screen visited once (at its first place). Nothing is
    /// allocated; a slot has few offers, so the repeated walks stay short.
    fn each_slot_screen<'s>(&'s self, slot: &str, visit: &mut dyn FnMut(&'s Id) -> bool) {
        let offered = |c: &SlotCandidate| {
            c.slot.as_str() == slot && self.screens.contains_key(c.screen.as_str())
        };
        let mut above: Option<Priority> = None;
        loop {
            let level = self
                .candidates
                .iter()
                .filter(|c| offered(c) && above.is_none_or(|a| c.priority < a))
                .map(|c| c.priority)
                .max();
            let Some(level) = level else {
                return;
            };
            for (at, candidate) in self.candidates.iter().enumerate() {
                if candidate.priority != level || !offered(candidate) {
                    continue;
                }
                let seen = self.candidates.iter().enumerate().any(|(before, other)| {
                    other.screen == candidate.screen
                        && offered(other)
                        && (other.priority > level || (other.priority == level && before < at))
                });
                if !seen && !visit(&candidate.screen) {
                    return;
                }
            }
            above = Some(level);
        }
    }
}

/// A plugin's setting before it is defined: built at start, or kept in
/// the program ([`SettingSpec::fixed`]).
pub(crate) enum Staged {
    Owned(alloc::boxed::Box<SettingSpec>),
    /// Some settings of the table `.0` kept in the program, a table's worth
    /// as one entry (not one for each of them).
    Table(&'static [SettingSpec], Picked),
}

/// Which settings of a table a [`Staged::Table`] stands for.
pub(crate) enum Picked {
    /// All of them.
    All,
    /// The entries at these places, in the order they are defined in (the
    /// list the plugin's admission made, kept as it is: no copy).
    Some(Vec<u16>),
}

impl Staged {
    /// How many settings this entry stands for.
    pub(crate) fn len(&self) -> usize {
        match self {
            Staged::Owned(_) => 1,
            Staged::Table(list, Picked::All) => list.len(),
            Staged::Table(_, Picked::Some(entries)) => entries.len(),
        }
    }

    /// The `n`th setting of this entry, `n < self.len()`.
    fn get(&self, n: usize) -> &SettingSpec {
        match self {
            Staged::Owned(spec) => spec,
            Staged::Table(list, Picked::All) => &list[n],
            Staged::Table(list, Picked::Some(entries)) => &list[usize::from(entries[n])],
        }
    }

    /// The settings this entry stands for, in order.
    pub(crate) fn specs(&self) -> impl Iterator<Item = &SettingSpec> {
        (0..self.len()).map(|n| self.get(n))
    }

    /// Defines the settings this entry stands for in `settings`, which has
    /// room and no setting of their ids (checked by the caller).
    pub(crate) fn define_in(
        self,
        settings: &mut paperpaw_sdk::Settings,
    ) -> Result<(), RegisterError> {
        match self {
            Staged::Owned(spec) => settings.define(*spec),
            Staged::Table(list, Picked::All) => {
                (0..list.len()).try_for_each(|at| settings.define_fixed(list, at))
            }
            Staged::Table(list, Picked::Some(entries)) => entries
                .iter()
                .try_for_each(|&at| settings.define_fixed(list, usize::from(at))),
        }
    }
}

/// Contributions of one plugin, collected during `register` and merged only
/// if the whole registration succeeds; a failing plugin leaves no traces.
#[derive(Default)]
pub(crate) struct Staging {
    actions: Vec<ActionSpec>,
    /// Interface-7 plugins' actions, kept without a handler (#248).
    native_actions: Vec<crate::kit::action::NativeAction>,
    /// Tests' screens (none outside the feature `testing`).
    layer_screens: Vec<crate::FakeSpec>,
    native_screens: Vec<crate::native_host::native_screen_factory::NativeScreenSpec>,
    slots: Vec<SlotSpec>,
    candidates: Vec<SlotCandidate>,
    pub(crate) settings: Vec<Staged>,
    /// A native plugin's settings described from its own records, defined
    /// after the others (`native_host::described_settings`).
    pub(crate) described: Option<paperpaw_sdk::DescribedSettings>,
    pub(crate) keymaps: Vec<Cow<'static, str>>,
    pub(crate) catalogs: Vec<Catalog>,
    /// Compiled catalogs on the storage, opened as they are applied.
    pub(crate) catalog_files: Vec<alloc::string::String>,
    pub(crate) fonts: Vec<FontSpec>,
    pub(crate) font_files: Vec<(Id, alloc::string::String)>,
    pub(crate) font_extensions: Vec<(Id, alloc::string::String, Id)>,
    pub(crate) font_faces: Vec<paperpaw_sdk::FontFace>,
    pub(crate) font_roles: Vec<(Id, Id, Priority)>,
    pub(crate) formats: Vec<FormatSpec>,
    pub(crate) setting_groups: Vec<paperpaw_sdk::SettingGroup>,
    /// Events, the plugins that hear them, and their handlers.
    pub(crate) subscriptions: Vec<(Id, Subscribed)>,
    pub(crate) fixed_actions: Vec<&'static [crate::kit::action::FixedAction]>,
    pub(crate) fixed_screens: Vec<Fixed>,
    pub(crate) menu_entries: Vec<MenuEntry>,
    pub(crate) fixed_menu_entries: Vec<&'static [MenuEntry]>,
    pub(crate) menu_sections: Vec<crate::kit::menus::MenuSection>,
    pub(crate) events: Vec<paperpaw_sdk::services::EventSpec>,
    pub(crate) icons: Vec<(Id, Cow<'static, str>)>,
    pub(crate) fixed_icons: Vec<&'static [(Id, &'static str)]>,
    pub(crate) menu_needs: Vec<(Id, paperpaw_sdk::MenuNeed)>,
    /// (menu, item, the plugin hiding it).
    pub(crate) hidden_entries: Vec<(Id, Id, Id)>,
    pub(crate) item_states: Vec<(Id, crate::kit::menus::ItemStateFn)>,
    pub(crate) keymap_presets: Vec<(Id, Cow<'static, str>)>,
    pub(crate) setting_defaults: Vec<(Id, Cow<'static, str>)>,
    pub(crate) choices: Vec<(Id, Cow<'static, str>)>,
    pub(crate) status_items: Vec<StatusItemSpec>,
    pub(crate) status_lines: Vec<StatusLineSpec>,
    pub(crate) widgets: Vec<StagedWidgets>,
    pub(crate) setup_steps: Vec<crate::kit::setup::SetupStepSpec>,
    pub(crate) presets: Vec<paperpaw_sdk::presets::Preset>,
    pub(crate) fixed_presets: Vec<&'static [paperpaw_sdk::presets::Preset]>,
    pub(crate) decorators: Vec<crate::kit::decorations::DecoratorSpec>,
    /// Jobs: the steps of declared work (see [`paperpaw_sdk::work`]).
    pub(crate) works: Vec<crate::work::WorkSpec>,
    pub(crate) data_actions: Vec<DataAction>,
    pub(crate) stylesheets: Vec<Cow<'static, str>>,
    pub(crate) book_data: Vec<crate::kit::library::BookData>,
    pub(crate) services: Vec<(Id, paperpaw_sdk::services::Service)>,
    pub(crate) contributions: Vec<(Id, Cow<'static, str>)>,
}

fn push_native<T>(target: &mut Vec<T>, value: T) -> Result<(), i32> {
    target
        .try_reserve(1)
        .map_err(|_| paperpaw_native::host_glue::ERR_NO_MEMORY)?;
    target.push(value);
    Ok(())
}

impl Staging {
    /// A choice a native plugin adds to another plugin's choice setting.
    pub(crate) fn native_choice(
        &mut self,
        setting: Id,
        value: alloc::string::String,
    ) -> Result<(), i32> {
        push_native(&mut self.choices, (setting, Cow::Owned(value)))
    }

    pub(crate) fn native_screen(
        &mut self,
        owner: &Id,
        screen: crate::native_host::native_screen_factory::NativeScreenSpec,
    ) -> Result<(), i32> {
        let id = screen.titled.id();
        if id.namespace() != owner.as_str() {
            return Err(paperpaw_native::host_glue::ERR_INVALID);
        }
        if self
            .native_screens
            .iter()
            .any(|other| other.titled.id() == id)
            || self.layer_screens.iter().any(|other| other.id() == id)
            || self.fixed_screens.iter().any(|other| other.id() == id)
        {
            return Err(paperpaw_native::host_glue::ERR_INVALID);
        }
        push_native(&mut self.native_screens, screen)
    }

    /// Stages the plugin's action `action`, without infallible growth. An id
    /// of another plugin, or one staged already, is `ERR_INVALID`.
    pub(crate) fn native_action(
        &mut self,
        owner: &Id,
        action: crate::kit::action::NativeAction,
    ) -> Result<(), i32> {
        let id = crate::kit::action::Action::Native(&action).id();
        if id.namespace() != owner.as_str()
            || self.actions.iter().any(|other| &other.id == id)
            || self
                .native_actions
                .iter()
                .any(|other| crate::kit::action::Action::Native(other).id() == id)
        {
            return Err(paperpaw_native::host_glue::ERR_INVALID);
        }
        push_native(&mut self.native_actions, action)
    }

    /// How many native actions are staged: the room a publication makes in
    /// the kernel's actions before it merges.
    pub(crate) fn native_action_count(&self) -> usize {
        self.native_actions.len()
    }

    /// Stages the offer of the plugin's screen `screen` for `slot`, without
    /// infallible growth. A screen of another plugin is `ERR_INVALID`.
    pub(crate) fn native_offer(
        &mut self,
        owner: &Id,
        slot: Id,
        screen: Id,
        priority: Priority,
    ) -> Result<(), i32> {
        if screen.namespace() != owner.as_str() {
            return Err(paperpaw_native::host_glue::ERR_INVALID);
        }
        push_native(
            &mut self.candidates,
            SlotCandidate {
                slot,
                screen,
                priority,
            },
        )
    }

    /// How many actions, settings made on the heap and setting groups are
    /// staged: what a native publication must make room for before it merges
    /// (the settings kept in the program need only a place in the indexes:
    /// [`Self::native_fixed_count`]).
    pub(crate) fn native_counts(&self) -> (usize, usize, usize) {
        (
            self.actions.len(),
            self.settings
                .iter()
                .filter(|setting| matches!(setting, Staged::Owned(_)))
                .count(),
            self.setting_groups.len(),
        )
    }

    /// How many staged settings are kept in the program.
    pub(crate) fn native_fixed_count(&self) -> usize {
        self.settings
            .iter()
            .filter(|setting| matches!(setting, Staged::Table(..)))
            .map(Staged::len)
            .sum()
    }

    /// Stages the entries `entries` of `list`, a table in the program whose
    /// entries equal what the plugin declared, without a copy of them on the
    /// heap: one entry for all of them, the list taken as it is.
    pub(crate) fn native_fixed_entries(
        &mut self,
        list: &'static [SettingSpec],
        entries: Vec<u16>,
    ) -> Result<(), i32> {
        if entries.iter().any(|&at| usize::from(at) >= list.len()) {
            return Err(paperpaw_native::host_glue::ERR_INVALID);
        }
        push_native(
            &mut self.settings,
            Staged::Table(list, Picked::Some(entries)),
        )
    }

    /// Stages the setting group `group` of a native plugin, without
    /// infallible growth.
    pub(crate) fn native_group(&mut self, group: paperpaw_sdk::SettingGroup) -> Result<(), i32> {
        self.native(Contribution::SettingGroup(group))
    }

    /// Stage already decoded native contributions without infallible growth.
    /// The caller publishes only after every record has mapped successfully.
    #[allow(
        dead_code,
        reason = "native built-in registry adapter follows this staging primitive"
    )]
    pub(crate) fn native(&mut self, contribution: Contribution) -> Result<(), i32> {
        use paperpaw_native::host_glue::ERR_UNSUPPORTED;
        match contribution {
            Contribution::Action(action) => push_native(&mut self.actions, *action),
            Contribution::Setting(setting) => {
                push_native(&mut self.settings, Staged::Owned(setting))
            }
            Contribution::SettingGroup(group) => push_native(&mut self.setting_groups, group),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    /// Moves actions, screens, slots and bindings into `registry`.
    /// Settings, key maps and catalogs are left for the caller.
    pub(crate) fn merge_into(
        &mut self,
        registry: &mut Registry,
        actions: &mut Actions,
    ) -> Result<(), RegisterError> {
        let duplicate = self
            .actions
            .iter()
            .map(|a| &a.id)
            .chain(
                self.fixed_actions
                    .iter()
                    .flat_map(|list| list.iter().map(|a| a.id())),
            )
            .chain(
                self.native_actions
                    .iter()
                    .map(|a| crate::kit::action::Action::Native(a).id()),
            )
            .find(|id| actions.contains(id.as_str()))
            .or_else(|| {
                self.layer_screens
                    .iter()
                    .map(crate::FakeSpec::id)
                    .chain(self.native_screens.iter().map(|s| s.titled.id()))
                    .chain(self.fixed_screens.iter().map(|s| s.id()))
                    .find(|id| registry.screens.contains_key(id.as_str()))
            })
            .or_else(|| {
                self.slots
                    .iter()
                    .map(|s| &s.id)
                    .find(|id| registry.slots.contains_key(*id))
            });
        if let Some(id) = duplicate {
            if !self.native_screens.is_empty() {
                return Err(RegisterError::Invalid {
                    message: "native contribution already registered".into(),
                });
            }
            let id = id.try_clone().ok_or(RegisterError::Invalid {
                message: "duplicate contribution id could not be copied".into(),
            })?;
            return Err(RegisterError::Duplicate { id });
        }
        if !self.native_screens.is_empty() {
            registry
                .screens
                .try_reserve(self.layer_screens.len(), self.native_screens.len())
                .map_err(|_| RegisterError::Invalid {
                    message: NATIVE_CAPACITY_UNAVAILABLE.into(),
                })?;
        }
        // The offers' room, before anything is moved.
        registry
            .candidates
            .try_reserve(self.candidates.len())
            .map_err(|_| RegisterError::Invalid {
                message: NATIVE_CAPACITY_UNAVAILABLE.into(),
            })?;
        for action in self.actions.drain(..) {
            actions.insert(action);
        }
        for list in self.fixed_actions.drain(..) {
            actions.insert_fixed(list);
        }
        for action in self.native_actions.drain(..) {
            actions.insert_native(action);
        }
        for screen in self.layer_screens.drain(..) {
            registry.screens.insert_fake(screen);
        }
        for screen in self.native_screens.drain(..) {
            registry.screens.insert_native(screen);
        }
        for screen in self.fixed_screens.drain(..) {
            registry.screens.insert_fixed(screen);
        }
        for slot in self.slots.drain(..) {
            registry.slots.insert(slot.id.clone(), slot);
        }
        registry.candidates.append(&mut self.candidates);
        Ok(())
    }
}

/// Applies the contributions `plugin` made to `staging`, checking what its
/// registrar could not: key maps, translations, hyphenation patterns.
pub(crate) fn stage(
    plugin: &Id,
    items: crate::Items,
    staging: &mut Staging,
) -> Result<(), RegisterError> {
    let crate::builder::Parts {
        contributions,
        kernel_screens,
        layers,
        hears,
        works,
        data_actions,
    } = items.into_parts();
    staging.data_actions.extend(data_actions);
    staging.layer_screens.extend(layers);
    staging
        .subscriptions
        .extend(hears.into_iter().map(|hear| (plugin.clone(), hear)));
    staging.works.extend(works);
    for list in kernel_screens {
        staging.fixed_screens.extend(list.iter());
    }
    for contribution in contributions {
        match contribution {
            Contribution::Action(action) => staging.actions.push(*action),
            Contribution::FixedActions(list) => staging.fixed_actions.push(list),
            Contribution::Slot(slot) => staging.slots.push(slot),
            Contribution::SlotBinding {
                slot,
                screen,
                priority,
                ..
            } => staging.candidates.push(SlotCandidate {
                slot,
                screen,
                priority,
            }),
            Contribution::Setting(setting) => staging.settings.push(Staged::Owned(setting)),
            Contribution::FixedSettings(list) => {
                staging.settings.push(Staged::Table(list, Picked::All))
            }
            Contribution::SettingGroup(group) => staging.setting_groups.push(group),
            Contribution::SettingDefault { setting, value, .. } => {
                staging.setting_defaults.push((setting, value));
            }
            Contribution::Choice { setting, value, .. } => {
                staging.choices.push((setting, value));
            }
            Contribution::Keymap(source) => {
                validate_keymap(&source)?;
                staging.keymaps.push(source);
            }
            Contribution::KeymapPreset { id, source, .. } => {
                validate_keymap(&source)?;
                staging.keymap_presets.push((id, source));
            }
            Contribution::Stylesheet(css) => staging.stylesheets.push(css),
            Contribution::BookData(data) => staging.book_data.push(data),
            Contribution::Font(font) => staging.fonts.push(*font),
            Contribution::FontFile { id, path, .. } => staging.font_files.push((id, path)),
            Contribution::FontExtension {
                id, path, extends, ..
            } => {
                staging.font_extensions.push((id, path, extends));
            }
            Contribution::FontFace(face) => staging.font_faces.push(*face),
            Contribution::FontBinding {
                role,
                font,
                priority,
                ..
            } => staging.font_roles.push((role, font, priority)),
            Contribution::Format(format) => staging.formats.push(*format),
            Contribution::MenuEntry(entry) => staging.menu_entries.push(entry),
            Contribution::FixedMenuEntries(list) => staging.fixed_menu_entries.push(list),
            Contribution::FixedIcons(list) => staging.fixed_icons.push(list),
            Contribution::MenuSection(section) => staging.menu_sections.push(section),
            Contribution::Icon { item, icon, .. } => staging.icons.push((item, icon)),
            Contribution::MenuNeed { item, need, .. } => staging.menu_needs.push((item, need)),
            Contribution::HideMenuEntry { menu, item, .. } => {
                staging.hidden_entries.push((menu, item, plugin.clone()));
            }
            Contribution::ItemState { item, state, .. } => staging.item_states.push((item, state)),
            Contribution::StatusItem(item) => staging.status_items.push(*item),
            Contribution::StatusLine(line) => staging.status_lines.push(*line),
            Contribution::Decorator(decorator) => staging.decorators.push(*decorator),
            Contribution::Widget(widget) => staging.widgets.push(StagedWidgets::One(widget)),
            Contribution::SetupStep(step) => staging.setup_steps.push(*step),
            Contribution::Preset(preset) => staging.presets.push(*preset),
            Contribution::FixedPresets(list) => staging.fixed_presets.push(list),
            Contribution::Published { id, service, .. } => staging.services.push((id, service)),
            Contribution::ToPoint { point, text, .. } => staging.contributions.push((point, text)),
            Contribution::EventDeclared(event) => staging.events.push(*event),
            Contribution::Translations(source) => staging.catalogs.push(catalog(source)?),
            Contribution::TranslationFile(path) => staging.catalog_files.push(path),
        }
    }
    Ok(())
}

/// A translation catalog, its errors attributed to the plugin.
pub(crate) fn catalog(source: Cow<'static, str>) -> Result<Catalog, RegisterError> {
    Catalog::parse(source).map_err(|error| RegisterError::Parse {
        line: match &error {
            paperpaw_i18n::CatalogError::Syntax(e) => e.line,
            paperpaw_i18n::CatalogError::UnknownPluralRule { line }
            | paperpaw_i18n::CatalogError::UnexpectedSection { line } => *line,
            _ => 0,
        },
        message: Cow::Owned(error.to_string()),
    })
}

/// Checks a key map contribution so errors are attributed to the plugin.
fn validate_keymap(source: &str) -> Result<(), RegisterError> {
    let mut scratch = crate::keymap::Keymap::default();
    let problems = scratch.load(source, crate::keymap::Origin::Plugin, true);
    match problems.into_iter().next() {
        Some(problem) => Err(RegisterError::Parse {
            line: problem.line,
            message: problem.message,
        }),
        None => Ok(()),
    }
}

// The registry as text lines, for the tests that pin it (#244). Declared
// here: `lib.rs` is at its length limit.
#[cfg(any(test, feature = "testing"))]
#[path = "registry_lines.rs"]
mod registry_lines;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    #[test]
    fn a_screen_row_is_three_words() {
        // #248 B: a native screen's row keeps its token, flags and lease; how
        // many instances it has is read from its flags. Three words here (was
        // four), 16 bytes on the X4 (was 20).
        type Made = crate::native_host::native_screen_factory::NativeMade;
        assert!(
            core::mem::size_of::<Made>() <= 3 * core::mem::size_of::<usize>(),
            "{} bytes",
            core::mem::size_of::<Made>()
        );
    }

    #[test]
    fn a_native_screens_title_is_two_words() {
        // #248 B: an interface-7 plugin's screen keeps its id and title as a
        // linked plugin's entry kept in the program (or one box), not as an
        // id and a title of its own (five words).
        let words = core::mem::size_of::<usize>();
        assert!(core::mem::size_of::<paperpaw_sdk::Titled>() <= 2 * words);
    }

    /// Subscriptions come back as they were added: in order, each with its
    /// event and plugin, a plugin's id kept once for its run of them, the
    /// lists kept in the program among those made.
    /// The walk without a list visits what `slot_screens` lists, in its
    /// order (priorities down, offer order within one, each screen once,
    /// only screens that exist, only this slot), and stops when told.
    #[test]
    fn each_slot_screen_walks_as_slot_screens_lists() {
        use crate::host_screen::{HostEntry, Kind};
        use paperpaw_sdk::ScreenMaker;
        static SCREENS: [HostEntry; 4] = [
            HostEntry::new(Id::new_static("a.one"), "a", Kind::Placeholder),
            HostEntry::new(Id::new_static("b.two"), "b", Kind::Placeholder),
            HostEntry::new(Id::new_static("c.three"), "c", Kind::Placeholder),
            HostEntry::new(Id::new_static("d.four"), "d", Kind::Placeholder),
        ];
        let id = |text: &str| Id::parse(text).unwrap();
        let mut registry = Registry::default();
        for screen in &SCREENS {
            registry.screens.insert_fixed(screen);
        }
        let offers = [
            ("core.home", "c.three", Priority(0)),
            ("core.home", "a.one", Priority(5)),
            ("other.slot", "d.four", Priority(9)),
            ("core.home", "gone.screen", Priority(9)),
            ("core.home", "b.two", Priority(0)),
            ("core.home", "c.three", Priority(5)),
            ("core.home", "d.four", Priority::FALLBACK),
            ("core.home", "a.one", Priority(-3)),
        ];
        for (slot, screen, priority) in offers {
            registry.candidates.push(SlotCandidate {
                slot: id(slot),
                screen: id(screen),
                priority,
            });
        }
        let listed: Vec<&str> = ScreenMaker::slot_screens(&registry, "core.home")
            .into_iter()
            .map(Id::as_str)
            .collect();
        assert_eq!(listed, ["a.one", "c.three", "b.two", "d.four"]);
        let mut seen = Vec::new();
        ScreenMaker::each_slot_screen(&registry, "core.home", &mut |screen| {
            seen.push(screen.as_str());
            true
        });
        assert_eq!(seen, listed);
        seen.clear();
        ScreenMaker::each_slot_screen(&registry, "core.home", &mut |screen| {
            seen.push(screen.as_str());
            seen.len() < 2
        });
        assert_eq!(seen, ["a.one", "c.three"]);
        seen.clear();
        ScreenMaker::each_slot_screen(&registry, "no.slot", &mut |screen| {
            seen.push(screen.as_str());
            true
        });
        assert!(seen.is_empty());
    }

    #[test]
    fn subscriptions_come_back_in_order_with_their_plugins() {
        let made = |event: &str| Subscribed {
            event: Id::parse(event).unwrap(),
            run: |_, _| {},
        };
        let id = |text: &str| Id::parse(text).unwrap();
        let mut subscriptions = Subscriptions::default();
        let mut first = alloc::vec![
            (id("a"), made("e.one")),
            (id("a"), made("e.two")),
            (id("b"), made("e.one")),
        ];
        subscriptions.append(&mut first);
        assert!(first.is_empty());
        subscriptions.append(&mut alloc::vec![(id("b"), made("e.three"))]);
        subscriptions.append(&mut alloc::vec![(id("a"), made("e.one"))]);
        let seen: Vec<(String, String)> = subscriptions
            .iter()
            .map(|(event, plugin, _)| (event.as_str().into(), plugin.as_str().into()))
            .collect();
        let expected = [
            ("e.one", "a"),
            ("e.two", "a"),
            ("e.one", "b"),
            ("e.three", "b"),
            ("e.one", "a"),
        ];
        assert_eq!(
            seen,
            expected.map(|(e, p)| (String::from(e), String::from(p)))
        );
        assert_eq!(subscriptions.plugins.len(), 3);
    }
}
