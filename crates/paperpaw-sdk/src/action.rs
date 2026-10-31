use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::env::Env;
use crate::id::Id;
use crate::params::Params;
use crate::plugin::Priority;

/// Global handler of an action, called when the active screen does not
/// handle it, with the params it was run with
/// ([`Command::RunAction`](crate::Command::RunAction); none from inputs),
/// e.g. the profile to apply. It reacts by queuing commands.
pub type ActionHandler = Box<dyn Fn(&Env<'_>, &Params, &mut crate::Commands)>;

/// A handler put in place of an action's own
/// ([`Registrar::override_action`](crate::Registrar::override_action)).
#[non_exhaustive]
pub struct ActionOverride {
    pub action: Id,
    pub handler: ActionHandler,
    pub priority: Priority,
}

impl fmt::Debug for ActionOverride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionOverride")
            .field("action", &self.action)
            .field("priority", &self.priority)
            .finish_non_exhaustive()
    }
}

/// Sees every action run, or every event emitted, with its params
/// (see [`ObserverSpec`]).
pub type Observer = Box<dyn Fn(&Env<'_>, &Id, &Params, &mut crate::Commands)>;

/// Something that sees every action run
/// ([`Registrar::observe_actions`](crate::Registrar::observe_actions)) or
/// every event emitted
/// ([`Registrar::observe_events`](crate::Registrar::observe_events)): a
/// macro recorder, a rule on any event. It runs for each, so it does
/// little; an observer of events must not emit an event for every event
/// it sees.
#[non_exhaustive]
pub struct ObserverSpec {
    pub id: Id,
    pub observer: Observer,
}

impl ObserverSpec {
    pub fn new(
        id: impl Into<Id>,
        observer: impl Fn(&Env<'_>, &Id, &Params, &mut crate::Commands) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            observer: Box::new(observer),
        }
    }
}

impl fmt::Debug for ObserverSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObserverSpec")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// A command users can bind to buttons, taps and gestures.
///
/// Actions without a handler are *screen actions*: their meaning depends on
/// the active screen (e.g. `core.next-page` in a reader vs. in a list).
///
/// ```
/// use paperpaw_sdk::{ActionHint, ActionSpec, Command, Id};
///
/// const OPEN: Id = Id::new_static("demo.open");
/// let spec = ActionSpec::new(OPEN, "demo.open")
///     .handler(|_, _, out| out.push(Command::Back))
///     .hint(ActionHint::Label("demo.open.short".into()));
/// ```
#[non_exhaustive]
pub struct ActionSpec {
    pub id: Id,
    /// Translation key of the action's display name (for key map editors).
    pub title: Cow<'static, str>,
    pub handler: Option<ActionHandler>,
    /// How the row of hints shows it where a screen leaves that to the
    /// action ([`Hint::Default`](crate::Hint::Default)).
    pub hint: ActionHint,
    /// What few actions have, kept aside so that the others stay small.
    extra: Option<Box<ActionExtra>>,
}

/// What few actions have ([`ActionSpec`]).
#[derive(Default)]
struct ActionExtra {
    description: Option<Cow<'static, str>>,
    keywords: Option<Cow<'static, str>>,
    sealed: bool,
}

impl ActionSpec {
    /// A screen action `id`, called `title` (a translation key).
    pub fn new(id: impl Into<Id>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            handler: None,
            hint: ActionHint::Title,
            extra: None,
        }
    }

    fn extra(&mut self) -> &mut ActionExtra {
        self.extra.get_or_insert_with(Default::default)
    }

    /// What it does where the screen shown does not take it, with the
    /// params it was run with ([`Command::RunAction`](crate::Command::RunAction);
    /// none from inputs).
    pub fn handler(
        mut self,
        handler: impl Fn(&Env<'_>, &Params, &mut crate::Commands) + 'static,
    ) -> Self {
        self.handler = Some(Box::new(handler));
        self
    }

    /// How the row of hints shows it.
    pub fn hint(mut self, hint: ActionHint) -> Self {
        self.hint = hint;
        self
    }

    /// What it does, in a sentence (a translation key), for lists of
    /// actions to choose from.
    pub fn describe(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.extra().description = Some(description.into());
        self
    }

    /// Words it is found by in a search of actions (a translation key;
    /// its text separated by commas).
    pub fn keywords(mut self, keywords: impl Into<Cow<'static, str>>) -> Self {
        self.extra().keywords = Some(keywords.into());
        self
    }

    /// No handler of another plugin is put in its place
    /// ([`Registrar::override_action`](crate::Registrar::override_action)):
    /// for actions that keep the device safe, such as locking it.
    pub fn sealed(mut self) -> Self {
        self.extra().sealed = true;
        self
    }

    /// What it does ([`ActionSpec::describe`]).
    pub fn description(&self) -> Option<&str> {
        self.extra.as_ref()?.description.as_deref()
    }

    /// The words it is found by ([`ActionSpec::keywords`]).
    pub fn search_keywords(&self) -> Option<&str> {
        self.extra.as_ref()?.keywords.as_deref()
    }

    /// Whether no other handler is put in its place ([`ActionSpec::sealed`]).
    pub fn is_sealed(&self) -> bool {
        self.extra.as_ref().is_some_and(|extra| extra.sealed)
    }

    /// Whether it does something itself, without a screen that takes it.
    pub fn runs_itself(&self) -> bool {
        self.handler.is_some()
    }
}

impl fmt::Debug for ActionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("handler", &self.runs_itself())
            .field("hint", &self.hint)
            .field("description", &self.description())
            .field("sealed", &self.is_sealed())
            .finish()
    }
}

/// The handler of an action kept in the program ([`FixedAction`]): a
/// function, not a closure, so the action can be in a `static` list.
pub type FixedHandler = fn(&Env<'_>, &Params, &mut crate::Commands);

/// An action kept in the program: as an [`ActionSpec`], in a `static`
/// list ([`Registrar::add_actions`](crate::Registrar::add_actions)), so
/// it takes next to no memory where an [`ActionSpec`] takes some 90
/// bytes. Its handler is a function ([`FixedHandler`]); an action whose
/// handler needs state of its own is an [`ActionSpec`].
///
/// ```
/// use paperpaw_sdk::{Command, FixedAction, Id};
///
/// static ACTIONS: [FixedAction; 1] = [FixedAction::new(Id::new_static("demo.open"), "demo.open")
///     .handler(|_, _, out| out.push(Command::Back))];
/// ```
#[derive(Debug)]
pub struct FixedAction {
    id: Id,
    title: &'static str,
    handler: Option<FixedHandler>,
    hint: ActionHint,
    description: Option<&'static str>,
    keywords: Option<&'static str>,
    sealed: bool,
}

impl FixedAction {
    /// A screen action `id`, called `title` (a translation key).
    pub const fn new(id: Id, title: &'static str) -> Self {
        Self {
            id,
            title,
            handler: None,
            hint: ActionHint::Title,
            description: None,
            keywords: None,
            sealed: false,
        }
    }

    /// What it does where the screen shown does not take it
    /// ([`ActionSpec::handler`]).
    pub const fn handler(mut self, handler: FixedHandler) -> Self {
        self.handler = Some(handler);
        self
    }

    /// How the row of hints shows it.
    pub const fn hint(mut self, hint: ActionHint) -> Self {
        // A `const` fn drops nothing: the hint before is a plain one.
        core::mem::forget(core::mem::replace(&mut self.hint, hint));
        self
    }

    /// What it does ([`ActionSpec::describe`]).
    pub const fn describe(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    /// Words it is found by ([`ActionSpec::keywords`]).
    pub const fn keywords(mut self, keywords: &'static str) -> Self {
        self.keywords = Some(keywords);
        self
    }

    /// No handler of another plugin is put in its place
    /// ([`ActionSpec::sealed`]).
    pub const fn sealed(mut self) -> Self {
        self.sealed = true;
        self
    }

    pub const fn id(&self) -> &Id {
        &self.id
    }

    /// Whether no other handler is put in its place.
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// An action as plugins added it, of either kind ([`Actions::get`]).
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Action<'a> {
    Spec(&'a ActionSpec),
    Fixed(&'static FixedAction),
}

impl<'a> Action<'a> {
    pub fn id(self) -> &'a Id {
        match self {
            Action::Spec(spec) => &spec.id,
            Action::Fixed(fixed) => &fixed.id,
        }
    }

    /// Translation key of its name.
    pub fn title(self) -> &'a str {
        match self {
            Action::Spec(spec) => &spec.title,
            Action::Fixed(fixed) => fixed.title,
        }
    }

    /// Translation key of its name, to keep.
    pub fn title_key(self) -> Cow<'static, str> {
        match self {
            Action::Spec(spec) => spec.title.clone(),
            Action::Fixed(fixed) => Cow::Borrowed(fixed.title),
        }
    }

    /// How the row of hints shows it.
    pub fn hint(self) -> &'a ActionHint {
        match self {
            Action::Spec(spec) => &spec.hint,
            Action::Fixed(fixed) => &fixed.hint,
        }
    }

    /// What it does ([`ActionSpec::describe`]).
    pub fn description(self) -> Option<&'a str> {
        match self {
            Action::Spec(spec) => spec.description(),
            Action::Fixed(fixed) => fixed.description,
        }
    }

    /// The words it is found by ([`ActionSpec::keywords`]).
    pub fn search_keywords(self) -> Option<&'a str> {
        match self {
            Action::Spec(spec) => spec.search_keywords(),
            Action::Fixed(fixed) => fixed.keywords,
        }
    }

    /// Whether no other handler is put in its place.
    pub fn is_sealed(self) -> bool {
        match self {
            Action::Spec(spec) => spec.is_sealed(),
            Action::Fixed(fixed) => fixed.sealed,
        }
    }

    /// Whether it does something itself, without a screen that takes it.
    pub fn runs_itself(self) -> bool {
        match self {
            Action::Spec(spec) => spec.runs_itself(),
            Action::Fixed(fixed) => fixed.handler.is_some(),
        }
    }

    /// Runs its own handler with `params` into `out`; whether it has one.
    pub fn run(self, env: &Env<'_>, params: &Params, out: &mut crate::Commands) -> bool {
        match self {
            Action::Spec(ActionSpec {
                handler: Some(handler),
                ..
            }) => handler(env, params, out),
            Action::Fixed(FixedAction {
                handler: Some(handler),
                ..
            }) => handler(env, params, out),
            _ => return false,
        }
        true
    }
}

/// How the row of hints below the screen shows an action, unless the
/// screen says otherwise ([`Screen::hint`](crate::Screen::hint)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActionHint {
    /// Its title.
    #[default]
    Title,
    /// A word shorter than its title (a translation key), e.g. *Select*
    /// for confirming.
    Label(Cow<'static, str>),
    /// An icon ([`icons`](crate::icons)), e.g. an arrow for moving.
    Icon(&'static str),
    /// Nothing, e.g. for an action that does nothing.
    Hidden,
    /// A value a later build adds, by its name (a constant of that build);
    /// a build that doesn't know it: it shows no hint.
    Other(&'static str),
}

/// Every registered action, e.g. for menu and key map editors.
#[derive(Default)]
pub struct Actions {
    /// Sorted by id; each spec's own id is the key (not kept twice: there
    /// are some hundred).
    specs: Vec<ActionSpec>,
    /// Those kept in the program, sorted by id; none of an id `specs`
    /// has (the one added last stays).
    fixed: Vec<&'static FixedAction>,
    /// Handlers plugins put in place of actions' own, the highest
    /// priority first: the action, the plugin, the handler.
    overrides: Vec<(Id, Priority, Id, ActionHandler)>,
}

impl fmt::Debug for Actions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let overrides: Vec<&Id> = self.overrides.iter().map(|(action, ..)| action).collect();
        f.debug_struct("Actions")
            .field("specs", &self.specs)
            .field("fixed", &self.fixed)
            .field("overrides", &overrides)
            .finish()
    }
}

impl Actions {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn insert(&mut self, action: ActionSpec) {
        if let Ok(at) = self.find_fixed(action.id.as_str()) {
            self.fixed.remove(at);
        }
        match self.find(action.id.as_str()) {
            Ok(at) => self.specs[at] = action,
            Err(at) => self.specs.insert(at, action),
        }
    }

    /// Adds actions kept in the program.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn insert_fixed(&mut self, actions: &'static [FixedAction]) {
        for action in actions {
            if let Ok(at) = self.find(action.id.as_str()) {
                self.specs.remove(at);
            }
            match self.find_fixed(action.id.as_str()) {
                Ok(at) => self.fixed[at] = action,
                Err(at) => self.fixed.insert(at, action),
            }
        }
    }

    fn find(&self, id: &str) -> Result<usize, usize> {
        self.specs.binary_search_by(|spec| spec.id.as_str().cmp(id))
    }

    fn find_fixed(&self, id: &str) -> Result<usize, usize> {
        self.fixed
            .binary_search_by(|fixed| fixed.id.as_str().cmp(id))
    }

    pub fn get(&self, id: &str) -> Option<Action<'_>> {
        match self.find(id) {
            Ok(at) => Some(Action::Spec(&self.specs[at])),
            Err(_) => self
                .find_fixed(id)
                .ok()
                .map(|at| Action::Fixed(self.fixed[at])),
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.find(id).is_ok() || self.find_fixed(id).is_ok()
    }

    /// Every action, by id.
    pub fn iter(&self) -> impl Iterator<Item = Action<'_>> {
        let mut specs = self.specs.iter().map(Action::Spec).peekable();
        let mut fixed = self.fixed.iter().map(|f| Action::Fixed(f)).peekable();
        core::iter::from_fn(move || match (specs.peek(), fixed.peek()) {
            (Some(s), Some(f)) if s.id() <= f.id() => specs.next(),
            (Some(_), Some(_)) | (None, Some(_)) => fixed.next(),
            (Some(_), None) => specs.next(),
            (None, None) => None,
        })
    }

    /// Whether action `id` runs without a screen that takes it: it has a
    /// handler, or a plugin put one in place of its own. Else it is a
    /// screen action (or none), e.g. for a palette of actions to run.
    pub fn runnable(&self, id: &str) -> bool {
        self.override_of(id).is_some() || self.get(id).is_some_and(Action::runs_itself)
    }

    /// Puts `handler` of `plugin` in place of action `action`'s own,
    /// at `priority` (see
    /// [`Registrar::override_action`](crate::Registrar::override_action)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_override(
        &mut self,
        action: Id,
        priority: Priority,
        plugin: Id,
        handler: ActionHandler,
    ) {
        let at = self
            .overrides
            .iter()
            .position(|(_, p, ..)| *p < priority)
            .unwrap_or(self.overrides.len());
        self.overrides
            .insert(at, (action, priority, plugin, handler));
    }

    /// Lets go of the handlers put in place of sealed actions (made before
    /// the sealed action was known); the actions they were for.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn drop_sealed_overrides(&mut self) -> Vec<Id> {
        let mut dropped = Vec::new();
        let sealed: Vec<bool> = self
            .overrides
            .iter()
            .map(|(action, ..)| self.get(action.as_str()).is_some_and(Action::is_sealed))
            .collect();
        let mut sealed = sealed.into_iter();
        self.overrides.retain(|(action, ..)| {
            let sealed = sealed.next().unwrap_or(false);
            if sealed {
                dropped.push(action.clone());
            }
            !sealed
        });
        dropped
    }

    /// The handler put in place of action `id`'s own with the highest
    /// priority, and the plugin whose it is.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn override_of(&self, id: &str) -> Option<(&Id, &ActionHandler)> {
        self.first_override(id)
    }

    #[cfg(not(any(test, feature = "kernel")))]
    fn override_of(&self, id: &str) -> Option<(&Id, &ActionHandler)> {
        self.first_override(id)
    }

    fn first_override(&self, id: &str) -> Option<(&Id, &ActionHandler)> {
        if self.get(id).is_some_and(Action::is_sealed) {
            return None;
        }
        self.overrides
            .iter()
            .find(|(action, ..)| action.as_str() == id)
            .map(|(_, _, plugin, handler)| (plugin, handler))
    }

    /// The handlers put in place of action `id`'s own, the highest
    /// priority first, with their plugins; none for a sealed action.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn overrides_of<'a>(
        &'a self,
        id: &'a str,
    ) -> impl Iterator<Item = (&'a Id, &'a ActionHandler)> + 'a {
        let sealed = self.get(id).is_some_and(Action::is_sealed);
        self.overrides
            .iter()
            .filter(move |(action, ..)| !sealed && action.as_str() == id)
            .map(|(_, _, plugin, handler)| (plugin, handler))
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.specs.shrink_to_fit();
        self.fixed.shrink_to_fit();
        self.overrides.shrink_to_fit();
    }
}
