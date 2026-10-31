//! Contribution registries and plugin registration.

use alloc::borrow::Cow;
use alloc::string::ToString;
use alloc::vec::Vec;

use paperpaw_i18n::Catalog;
use paperpaw_sdk::collections::VecMap;
use paperpaw_sdk::{
    ActionSpec, Actions, Contribution, EventHandler, FontSpec, FormatSpec, HyphenationSpec, Id,
    MenuEntry, Priority, RegisterError, ScreenSpec, SettingSpec, SlotSpec, StatusItemSpec,
    StatusLineSpec,
};

/// A candidate screen for a slot, proposed by a plugin.
#[derive(Debug)]
struct SlotCandidate {
    slot: Id,
    screen: Id,
    priority: Priority,
}

/// The screens plugins added, sorted by id: each id with the translation
/// key of its name, as [`Env::screens`](paperpaw_sdk::Env) hands them out
/// (not copied for it), and apart from them what makes the screen.
#[derive(Default)]
pub(crate) struct Screens {
    titles: Vec<(Id, Cow<'static, str>)>,
    made: Vec<Made>,
    /// Those kept in the program, sorted by id; none of an id `titles`
    /// has (the one added last stays).
    fixed: Vec<&'static paperpaw_sdk::FixedScreen>,
}

/// What makes a screen, of its [`ScreenSpec`].
pub(crate) struct Made {
    pub(crate) factory: paperpaw_sdk::ScreenFactory,
    pub(crate) refresh_on_open: Option<paperpaw_sdk::RefreshMode>,
    pub(crate) instances: paperpaw_sdk::Instances,
}

/// What makes a screen of either kind.
#[derive(Clone, Copy)]
pub(crate) enum Maker<'a> {
    Added(&'a Made),
    Fixed(&'static paperpaw_sdk::FixedScreen),
}

impl Maker<'_> {
    pub(crate) fn make(
        self,
        env: &paperpaw_sdk::Env<'_>,
        params: &paperpaw_sdk::Params,
    ) -> alloc::boxed::Box<dyn paperpaw_sdk::Screen> {
        match self {
            Maker::Added(made) => (made.factory)(env, params),
            Maker::Fixed(fixed) => fixed.make(env, params),
        }
    }

    pub(crate) fn refresh_on_open(self) -> Option<paperpaw_sdk::RefreshMode> {
        match self {
            Maker::Added(made) => made.refresh_on_open,
            Maker::Fixed(fixed) => fixed.refresh(),
        }
    }

    pub(crate) fn instances(self) -> paperpaw_sdk::Instances {
        match self {
            Maker::Added(made) => made.instances,
            Maker::Fixed(fixed) => fixed.how_many(),
        }
    }
}

impl core::fmt::Debug for Screens {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.titles.iter().map(|(id, _)| id))
            .finish()
    }
}

impl Screens {
    fn find(&self, id: &str) -> Result<usize, usize> {
        self.titles
            .binary_search_by(|(other, _)| other.as_str().cmp(id))
    }

    fn find_fixed(&self, id: &str) -> Result<usize, usize> {
        self.fixed.binary_search_by(|f| f.id().as_str().cmp(id))
    }

    pub(crate) fn contains_key<Q: core::borrow::Borrow<str> + ?Sized>(&self, id: &Q) -> bool {
        self.find(id.borrow()).is_ok() || self.find_fixed(id.borrow()).is_ok()
    }

    pub(crate) fn get<Q: core::borrow::Borrow<str> + ?Sized>(&self, id: &Q) -> Option<Maker<'_>> {
        match self.find(id.borrow()) {
            Ok(at) => Some(Maker::Added(&self.made[at])),
            Err(_) => self
                .find_fixed(id.borrow())
                .ok()
                .map(|at| Maker::Fixed(self.fixed[at])),
        }
    }

    fn title(&self, id: &str) -> Option<&str> {
        match self.find(id) {
            Ok(at) => Some(self.titles[at].1.as_ref()),
            Err(_) => self.find_fixed(id).ok().map(|at| self.fixed[at].title()),
        }
    }

    /// Adds screens kept in the program, in place of those of their ids.
    fn insert_fixed(&mut self, screens: &'static [paperpaw_sdk::FixedScreen]) {
        for screen in screens {
            if let Ok(at) = self.find(screen.id().as_str()) {
                self.titles.remove(at);
                self.made.remove(at);
            }
            match self.find_fixed(screen.id().as_str()) {
                Ok(at) => self.fixed[at] = screen,
                Err(at) => self.fixed.insert(at, screen),
            }
        }
    }

    /// Adds `spec`, in place of one of its id.
    fn insert(&mut self, spec: ScreenSpec) {
        // Each part kept: a part added to `ScreenSpec` must be kept here
        // too.
        let ScreenSpec {
            id,
            title,
            factory,
            refresh_on_open,
            instances,
            ..
        } = spec;
        let made = Made {
            factory,
            refresh_on_open,
            instances,
        };
        if let Ok(at) = self.find_fixed(id.as_str()) {
            self.fixed.remove(at);
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

    /// Every screen with the translation key of its name.
    pub(crate) fn list(&self) -> paperpaw_sdk::ScreenList<'_> {
        paperpaw_sdk::ScreenList::new(&self.titles, &self.fixed)
    }

    fn shrink_to_fit(&mut self) {
        self.titles.shrink_to_fit();
        self.made.shrink_to_fit();
        self.fixed.shrink_to_fit();
    }
}

/// The events plugins subscribed to, plugin by plugin in the order they
/// registered: each plugin's id once, with how many of the handlers after
/// those of the plugin before are its (a plugin subscribes to a few
/// events; there are some hundred and fifty subscriptions).
#[derive(Default)]
pub(crate) struct Subscriptions {
    plugins: Vec<(Id, u16)>,
    handlers: Vec<(Id, paperpaw_sdk::EventHandler)>,
}

impl Subscriptions {
    /// Adds `subscriptions` (event, plugin, handler), emptying it.
    pub(crate) fn append(&mut self, subscriptions: &mut Vec<(Id, Id, paperpaw_sdk::EventHandler)>) {
        for (event, plugin, handler) in subscriptions.drain(..) {
            match self.plugins.last_mut() {
                Some((last, count)) if *last == plugin && *count < u16::MAX => *count += 1,
                _ => self.plugins.push((plugin, 1)),
            }
            self.handlers.push((event, handler));
        }
    }

    /// Every subscription: the event, the plugin, its handler.
    pub(crate) fn iter(
        &self,
    ) -> impl Iterator<Item = (&Id, &Id, &paperpaw_sdk::EventHandler)> + '_ {
        self.plugins
            .iter()
            .scan(0, |at, (plugin, count)| {
                let from = *at;
                *at += usize::from(*count);
                Some((plugin, from..*at))
            })
            .flat_map(move |(plugin, handlers)| {
                self.handlers[handlers]
                    .iter()
                    .map(move |(event, handler)| (event, plugin, handler))
            })
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        self.plugins.shrink_to_fit();
        self.handlers.shrink_to_fit();
    }
}

/// All active contributions except settings, key maps and translations,
/// which live in their own stores.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    pub(crate) screens: Screens,
    pub(crate) slots: VecMap<Id, SlotSpec>,
    /// When screens show the row of hints, where they said.
    pub(crate) hints: VecMap<Id, paperpaw_sdk::HintVisibility>,
    candidates: Vec<SlotCandidate>,
    /// Slot bindings from the user's configuration.
    user_bindings: VecMap<Id, Id>,
}

impl Registry {
    /// Releases unused capacity once registration is complete.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.screens.shrink_to_fit();
        self.slots.shrink_to_fit();
        self.hints.shrink_to_fit();
        self.candidates.shrink_to_fit();
        self.user_bindings.shrink_to_fit();
    }

    /// When `screen` shows the row of hints.
    pub(crate) fn hints_of(&self, screen: &Id) -> paperpaw_sdk::HintVisibility {
        self.hints.get(screen).copied().unwrap_or_default()
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
    fn make(
        &self,
        id: &Id,
        env: &paperpaw_sdk::Env<'_>,
        params: &paperpaw_sdk::Params,
    ) -> Option<alloc::boxed::Box<dyn paperpaw_sdk::Screen>> {
        let maker = self.screens.get(id.as_str())?;
        Some(maker.make(env, params))
    }

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
        candidates.sort_by_key(|c| core::cmp::Reverse(c.priority));
        let mut screens: Vec<&Id> = Vec::new();
        for candidate in candidates {
            if !screens.contains(&&candidate.screen) {
                screens.push(&candidate.screen);
            }
        }
        screens
    }
}

/// A plugin's setting before it is defined: built at start, or kept in
/// the program ([`SettingSpec::fixed`]).
pub(crate) enum Staged {
    Owned(alloc::boxed::Box<SettingSpec>),
    /// Spec `.1` of the list `.0`, kept by a reference to the list.
    Fixed(&'static [SettingSpec], usize),
}

impl Staged {
    pub(crate) fn spec(&self) -> &SettingSpec {
        match self {
            Staged::Owned(spec) => spec,
            Staged::Fixed(list, at) => &list[*at],
        }
    }
}

/// Contributions of one plugin, collected during `register` and merged only
/// if the whole registration succeeds; a failing plugin leaves no traces.
#[derive(Default)]
pub(crate) struct Staging {
    actions: Vec<ActionSpec>,
    screens: Vec<ScreenSpec>,
    slots: Vec<SlotSpec>,
    candidates: Vec<SlotCandidate>,
    pub(crate) settings: Vec<Staged>,
    pub(crate) keymaps: Vec<Cow<'static, str>>,
    /// Key maps that apply while a switch (the id) is on.
    pub(crate) keymaps_when: Vec<(Id, Cow<'static, str>)>,
    pub(crate) catalogs: Vec<Catalog>,
    pub(crate) fonts: Vec<FontSpec>,
    pub(crate) font_files: Vec<(Id, alloc::string::String)>,
    pub(crate) font_extensions: Vec<(Id, alloc::string::String, Id)>,
    pub(crate) font_faces: Vec<paperpaw_sdk::FontFace>,
    pub(crate) font_roles: Vec<(Id, Id, Priority)>,
    pub(crate) ui_looks: Vec<(&'static paperpaw_sdk::ui::look::Painters, Priority)>,
    pub(crate) formats: Vec<FormatSpec>,
    pub(crate) hyphenation: Vec<HyphenationSpec>,
    pub(crate) setting_groups: Vec<paperpaw_sdk::SettingGroup>,
    /// Events, the plugins that hear them, and their handlers.
    pub(crate) subscriptions: Vec<(Id, Id, EventHandler)>,
    pub(crate) overrides: Vec<(Id, Priority, paperpaw_sdk::ActionHandler)>,
    pub(crate) action_observers: Vec<paperpaw_sdk::ObserverSpec>,
    pub(crate) event_observers: Vec<paperpaw_sdk::ObserverSpec>,
    pub(crate) fixed_actions: Vec<&'static [paperpaw_sdk::FixedAction]>,
    pub(crate) fixed_screens: Vec<&'static [paperpaw_sdk::FixedScreen]>,
    pub(crate) menu_entries: Vec<MenuEntry>,
    pub(crate) fixed_menu_entries: Vec<&'static [MenuEntry]>,
    pub(crate) menu_sections: Vec<paperpaw_sdk::MenuSection>,
    pub(crate) points: Vec<paperpaw_sdk::services::PointSpec>,
    pub(crate) events: Vec<paperpaw_sdk::services::EventSpec>,
    pub(crate) icons: Vec<(Id, Cow<'static, str>)>,
    pub(crate) fixed_icons: Vec<&'static [(Id, &'static str)]>,
    pub(crate) menu_needs: Vec<(Id, paperpaw_sdk::MenuNeed)>,
    /// (menu, item, the plugin hiding it).
    pub(crate) hidden_entries: Vec<(Id, Id, Id)>,
    pub(crate) item_states: Vec<(Id, paperpaw_sdk::ItemStateFn)>,
    pub(crate) item_descriptions: Vec<(Id, Cow<'static, str>)>,
    pub(crate) screen_hints: Vec<(Id, paperpaw_sdk::HintVisibility)>,
    pub(crate) keymap_presets: Vec<(Id, Cow<'static, str>)>,
    pub(crate) setting_defaults: Vec<(Id, Cow<'static, str>)>,
    pub(crate) choices: Vec<(Id, Cow<'static, str>)>,
    pub(crate) status_items: Vec<StatusItemSpec>,
    pub(crate) status_lines: Vec<StatusLineSpec>,
    pub(crate) status_places: Vec<paperpaw_sdk::status::Place>,
    pub(crate) widget_places: Vec<paperpaw_sdk::widgets::WidgetPlace>,
    pub(crate) widget_categories: Vec<paperpaw_sdk::widgets::WidgetCategory>,
    pub(crate) widgets: Vec<paperpaw_sdk::widgets::WidgetSpec>,
    pub(crate) setup_steps: Vec<paperpaw_sdk::setup::SetupStepSpec>,
    pub(crate) presets: Vec<paperpaw_sdk::presets::Preset>,
    pub(crate) fixed_presets: Vec<&'static [paperpaw_sdk::presets::Preset]>,
    pub(crate) text_filters: Vec<paperpaw_sdk::filters::TextFilterSpec>,
    pub(crate) decorators: Vec<paperpaw_sdk::decorations::DecoratorSpec>,
    pub(crate) page_layers: Vec<paperpaw_sdk::layers::PageLayerSpec>,
    pub(crate) page_frames: Vec<paperpaw_sdk::layers::PageFrameSpec>,
    pub(crate) lookup_providers: Vec<paperpaw_sdk::lookup::LookupProviderSpec>,
    pub(crate) voices: Vec<paperpaw_sdk::voices::VoiceSpec>,
    pub(crate) readers: Vec<paperpaw_sdk::readers::ReaderSpec>,
    pub(crate) input_filters: Vec<paperpaw_sdk::filters::InputFilterSpec>,
    pub(crate) gatt_services: Vec<paperpaw_sdk::bluetooth::ServiceSpec>,
    pub(crate) refresh_filters: Vec<paperpaw_sdk::filters::RefreshFilterSpec>,
    pub(crate) chromes: Vec<paperpaw_sdk::chrome::ChromeSpec>,
    pub(crate) inputs: Vec<paperpaw_sdk::keys::InputSpec>,
    pub(crate) gestures: Vec<paperpaw_sdk::keys::GestureSpec>,
    /// Jobs: the steps of declared work (see [`paperpaw_sdk::work`]).
    pub(crate) works: Vec<paperpaw_sdk::work::WorkSpec>,
    pub(crate) stylesheets: Vec<Cow<'static, str>>,
    pub(crate) book_data: Vec<paperpaw_sdk::library::BookData>,
    pub(crate) book_fields: Vec<paperpaw_sdk::library::BookField>,
    pub(crate) book_badges: Vec<paperpaw_sdk::library::BookBadge>,
    pub(crate) services: Vec<(Id, paperpaw_sdk::services::Service)>,
    pub(crate) contributions: Vec<(Id, Cow<'static, str>)>,
}

impl Staging {
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
            .find(|id| actions.contains(id.as_str()))
            .or_else(|| {
                self.screens
                    .iter()
                    .map(|s| &s.id)
                    .chain(
                        self.fixed_screens
                            .iter()
                            .flat_map(|l| l.iter().map(|s| s.id())),
                    )
                    .find(|id| registry.screens.contains_key(id.as_str()))
            })
            .or_else(|| {
                self.slots
                    .iter()
                    .map(|s| &s.id)
                    .find(|id| registry.slots.contains_key(*id))
            });
        if let Some(id) = duplicate {
            return Err(RegisterError::Duplicate { id: id.clone() });
        }
        for action in self.actions.drain(..) {
            actions.insert(action);
        }
        for list in self.fixed_actions.drain(..) {
            actions.insert_fixed(list);
        }
        for screen in self.screens.drain(..) {
            registry.screens.insert(screen);
        }
        for list in self.fixed_screens.drain(..) {
            registry.screens.insert_fixed(list);
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
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub(crate) fn stage(
    plugin: &Id,
    contributions: Vec<Contribution>,
    staging: &mut Staging,
) -> Result<(), RegisterError> {
    for contribution in contributions {
        match contribution {
            Contribution::Action(action) => staging.actions.push(*action),
            Contribution::FixedActions(list) => staging.fixed_actions.push(list),
            Contribution::Screen(screen) => staging.screens.push(screen),
            Contribution::FixedScreens(list) => staging.fixed_screens.push(list),
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
                staging
                    .settings
                    .extend((0..list.len()).map(|at| Staged::Fixed(list, at)));
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
            Contribution::KeymapWhen {
                setting, source, ..
            } => {
                validate_keymap(&source)?;
                staging.keymaps_when.push((setting, source));
            }
            Contribution::Stylesheet(css) => staging.stylesheets.push(css),
            Contribution::BookData(data) => staging.book_data.push(data),
            Contribution::BookField(field) => staging.book_fields.push(*field),
            Contribution::BookBadge(badge) => staging.book_badges.push(*badge),
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
            Contribution::Hyphenation(spec) => {
                paperpaw_sdk::hyphen::Patterns::new(spec.patterns).map_err(|e| {
                    RegisterError::Invalid {
                        message: Cow::Owned(alloc::format!("{}: {e}", spec.id)),
                    }
                })?;
                staging.hyphenation.push(spec);
            }
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
            Contribution::ItemDescription { item, key, .. } => {
                staging.item_descriptions.push((item, key));
            }
            Contribution::ScreenHints { screen, hints, .. } => {
                staging.screen_hints.push((screen, hints));
            }
            Contribution::Subscription { event, handler, .. } => {
                staging.subscriptions.push((event, plugin.clone(), handler));
            }
            Contribution::StatusItem(item) => staging.status_items.push(*item),
            Contribution::StatusLine(line) => staging.status_lines.push(*line),
            Contribution::StatusPlace(place) => staging.status_places.push(*place),
            Contribution::WidgetPlace(place) => staging.widget_places.push(*place),
            Contribution::WidgetCategory(category) => staging.widget_categories.push(*category),
            Contribution::TextFilter(filter) => staging.text_filters.push(filter),
            Contribution::Decorator(decorator) => staging.decorators.push(*decorator),
            Contribution::PageLayer(layer) => staging.page_layers.push(*layer),
            Contribution::PageFrame(frame) => staging.page_frames.push(*frame),
            Contribution::LookupProvider(p) => staging.lookup_providers.push(*p),
            Contribution::Voice(voice) => staging.voices.push(*voice),
            Contribution::Reader(reader) => staging.readers.push(reader),
            Contribution::InputFilter(filter) => staging.input_filters.push(filter),
            Contribution::GattService(service) => staging.gatt_services.push(*service),
            Contribution::RefreshFilter(filter) => staging.refresh_filters.push(filter),
            Contribution::Chrome(chrome) => staging.chromes.push(*chrome),
            Contribution::Input(input) => staging.inputs.push(input),
            Contribution::ActionOverride(over) => {
                let over = *over;
                staging
                    .overrides
                    .push((over.action, over.priority, over.handler));
            }
            Contribution::ActionObserver(observer) => staging.action_observers.push(*observer),
            Contribution::EventObserver(observer) => staging.event_observers.push(*observer),
            Contribution::Gesture(gesture) => staging.gestures.push(*gesture),
            Contribution::Work(spec) => staging.works.push(*spec),
            Contribution::Widget(widget) => staging.widgets.push(*widget),
            Contribution::SetupStep(step) => staging.setup_steps.push(*step),
            Contribution::Preset(preset) => staging.presets.push(*preset),
            Contribution::FixedPresets(list) => staging.fixed_presets.push(list),
            Contribution::Published { id, service, .. } => staging.services.push((id, service)),
            Contribution::ToPoint { point, text, .. } => staging.contributions.push((point, text)),
            Contribution::Point(point) => staging.points.push(*point),
            Contribution::EventDeclared(event) => staging.events.push(*event),
            Contribution::Translations(source) => staging.catalogs.push(catalog(source)?),
            Contribution::UiLook {
                painters, priority, ..
            } => staging.ui_looks.push((painters, priority)),
            // Keys of later releases: none known yet, left as they are.
            Contribution::Extended { .. } => {}
            // Accepted, not run: the kernel does not call render hooks
            // yet (`Capability::RENDER_HOOK` is off).
            Contribution::RenderHook(_) => {}
            // A kind of a newer SDK than this kernel knows: said, not lost.
            other => {
                return Err(RegisterError::Invalid {
                    message: Cow::Owned(alloc::format!(
                        "a contribution of kind `{}` this PaperPaw does not know",
                        other.kind()
                    )),
                });
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::string::String;

    /// Subscriptions come back as they were added: in order, each with its
    /// event and plugin, a plugin's id kept once for its run of them.
    #[test]
    fn subscriptions_come_back_in_order_with_their_plugins() {
        let handler = || -> paperpaw_sdk::EventHandler { Box::new(|_, _, _| {}) };
        let id = |text: &str| Id::parse(text).unwrap();
        let mut subscriptions = Subscriptions::default();
        let mut first = alloc::vec![
            (id("e.one"), id("a"), handler()),
            (id("e.two"), id("a"), handler()),
            (id("e.one"), id("b"), handler()),
        ];
        subscriptions.append(&mut first);
        assert!(first.is_empty());
        subscriptions.append(&mut alloc::vec![(id("e.three"), id("b"), handler())]);
        subscriptions.append(&mut alloc::vec![(id("e.one"), id("a"), handler())]);
        let seen: Vec<(String, String)> = subscriptions
            .iter()
            .map(|(event, plugin, _)| (event.as_str().into(), plugin.as_str().into()))
            .collect();
        let pairs = [
            ("e.one", "a"),
            ("e.two", "a"),
            ("e.one", "b"),
            ("e.three", "b"),
            ("e.one", "a"),
        ];
        assert_eq!(seen, pairs.map(|(e, p)| (String::from(e), String::from(p))));
        assert_eq!(subscriptions.plugins.len(), 3);
    }
}
