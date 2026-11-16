//! Filters: stages where plugins look at what passes and may change it.
//!
//! - [Text filters](TextFilterSpec): between a book's format and the
//!   layout of its pages.
//! - [Input filters](InputFilterSpec): every button and finger before
//!   anything else sees it, e.g. to lock the screen, or touch while
//!   reading.
//! - [Refresh filters](RefreshFilterSpec): how the panel is refreshed,
//!   e.g. always in full while reading.
//!
//! Text filters are a stage between a book's format and the layout of its
//! pages. Each filter gets every block before it is laid out and may
//! change it: style parts of it (e.g. bold word beginnings), replace text
//! ([`Block::replace`](crate::document::Block::replace) keeps positions in
//! the book right), or leave it.
//!
//! Plugins add filters with
//! `Items::add_text_filter`; a
//! filter decides from the settings whether it does anything. Its
//! signature tells readers when its effect changed, so pages are laid out
//! anew (and kept apart from pages laid out without it). A block filter
//! ([`TextFilterSpec::block`]) also knows where the block is (the book,
//! its language, the place: [`FilterContext`]) and may leave the block
//! out ([`Verdict::Drop`]), e.g. a web page's adverts or a book's
//! printed page numbers.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::document::{Block, Locator};
use crate::env::Env;
use crate::id::Id;
use crate::{InputEvent, Rect, RefreshMode};

/// Changes a block before it is laid out.
pub type TextFilterFn = Box<dyn Fn(&Env<'_>, &mut Block)>;
/// What a filter does now, as a number that changes when its effect does
/// (e.g. [`Fnv1a64`](crate::digest::Fnv1a64) of its settings): 0 while it
/// does nothing.
pub type TextFilterSignatureFn = Box<dyn Fn(&Env<'_>) -> u64>;
/// Changes a block before it is laid out, knowing where it is, and says
/// whether it stays.
pub type BlockFilterFn = Box<dyn Fn(&Env<'_>, &FilterContext<'_>, &mut Block) -> Verdict>;

/// Where a block a filter gets is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FilterContext<'a> {
    /// The book (empty where it is not known).
    pub path: &'a str,
    /// Its language, e.g. `de` (empty where unknown).
    pub language: &'a str,
    /// Where the block starts.
    pub at: Locator,
}

impl<'a> FilterContext<'a> {
    pub fn new(path: &'a str, language: &'a str, at: Locator) -> Self {
        Self { path, language, at }
    }
}

/// Whether a block stays ([`BlockFilterFn`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    Keep,
    /// Left out of the layout (and of what is searched).
    Drop,
}

/// A text filter contribution.
#[non_exhaustive]
pub struct TextFilterSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    pub filter: TextFilterFn,
    pub signature: TextFilterSignatureFn,
    /// A block filter, run after `filter` (see [`TextFilterSpec::block`]).
    pub block: Option<BlockFilterFn>,
}

impl TextFilterSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        filter: TextFilterFn,
        signature: TextFilterSignatureFn,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            filter,
            signature,
            block: None,
        }
    }

    /// A filter that knows where each block is and may leave it out.
    pub fn block(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        filter: BlockFilterFn,
        signature: TextFilterSignatureFn,
    ) -> Self {
        Self {
            block: Some(filter),
            ..Self::new(id, title, Box::new(|_, _| {}), signature)
        }
    }
}

impl fmt::Debug for TextFilterSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextFilterSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The registered text filters.
#[derive(Debug, Default)]
pub struct TextFilters {
    filters: Vec<TextFilterSpec>,
}

impl TextFilters {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, filter: TextFilterSpec) {
        self.filters.push(filter);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.filters.shrink_to_fit();
    }

    /// Runs every filter that does something on `block` (block filters
    /// with no place known, their verdicts not asked).
    pub fn apply(&self, env: &Env<'_>, block: &mut Block) {
        let nowhere = FilterContext::new("", "", Locator::START);
        let _ = self.apply_in(env, &nowhere, block);
    }

    /// Runs every filter on `block`, which is at `cx`: whether it stays.
    /// A block left out goes to no filter after.
    pub fn apply_in(&self, env: &Env<'_>, cx: &FilterContext<'_>, block: &mut Block) -> Verdict {
        for filter in &self.filters {
            (filter.filter)(env, block);
            if let Some(block_filter) = &filter.block {
                match block_filter(env, cx, block) {
                    Verdict::Keep => {}
                    Verdict::Drop => return Verdict::Drop,
                }
            }
        }
        Verdict::Keep
    }

    /// What the filters do now together; 0 if none does anything.
    pub fn signature(&self, env: &Env<'_>) -> u64 {
        let mut hash = crate::digest::Fnv1a64::new();
        let mut any = false;
        for filter in &self.filters {
            let part = (filter.signature)(env);
            if part != 0 {
                hash.feed(filter.id.as_str().as_bytes());
                hash.feed(&part.to_le_bytes());
                any = true;
            }
        }
        if any { hash.value() } else { 0 }
    }
}

/// What an input filter does with an input. Filters see what is shown
/// in [`Env::stack`](crate::Env::stack).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputVerdict {
    /// On to the next filter, then the screens and the key maps.
    Pass,
    /// Taken: nothing else sees it.
    Take,
    /// Taken, and this action runs instead.
    Run(Id),
    /// Taken, and what was begun with it forgotten: the finger or button
    /// is no longer followed (no long press comes of it, the screen that
    /// took the finger lets it go), as if it had not come down.
    Cancel,
    /// This input in its place, for the filters after it and everything
    /// else (another button, a finger moved).
    Replace(InputEvent),
}

/// Looks at an input before anything else does.
pub type InputFilterFn = Box<dyn Fn(&Env<'_>, &InputEvent) -> InputVerdict>;

/// An input filter contribution: it sees every input (buttons pressed and
/// released, fingers down, moving and up) before screens, gestures and key
/// maps do, in the order the plugins registered. One that takes a finger
/// takes it whole, from down to up; one that takes a button, its release
/// too.
#[non_exhaustive]
pub struct InputFilterSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    pub filter: InputFilterFn,
}

impl InputFilterSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, filter: InputFilterFn) -> Self {
        Self {
            id,
            title: title.into(),
            filter,
        }
    }
}

impl fmt::Debug for InputFilterSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputFilterSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The registered input filters.
#[derive(Debug, Default)]
pub struct InputFilters {
    filters: Vec<InputFilterSpec>,
}

impl InputFilters {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, filter: InputFilterSpec) {
        self.filters.push(filter);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.filters.shrink_to_fit();
    }

    /// What the first filter that does not pass it does with `event`.
    pub fn apply(&self, env: &Env<'_>, event: &InputEvent) -> InputVerdict {
        self.filter(env, event).0
    }

    /// What the first filter that does not pass it does with `event`, and
    /// the input it did it with (another where a filter before it
    /// replaced it).
    pub fn filter(&self, env: &Env<'_>, event: &InputEvent) -> (InputVerdict, InputEvent) {
        self.filter_by(env, event, None)
    }

    /// As [`InputFilters::filter`], by the filters of plugin `only` alone
    /// where it is given: while a screen guards the device, those of its
    /// plugin (no other sees a PIN typed).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn filter_of(
        &self,
        env: &Env<'_>,
        event: &InputEvent,
        only: Option<&str>,
    ) -> (InputVerdict, InputEvent) {
        self.filter_by(env, event, only)
    }

    fn filter_by(
        &self,
        env: &Env<'_>,
        event: &InputEvent,
        only: Option<&str>,
    ) -> (InputVerdict, InputEvent) {
        let mut seen = *event;
        let mine = |filter: &&InputFilterSpec| only.is_none_or(|ns| filter.id.namespace() == ns);
        for filter in self.filters.iter().filter(mine) {
            match (filter.filter)(env, &seen) {
                InputVerdict::Pass => {}
                InputVerdict::Replace(other) => seen = other,
                verdict => return (verdict, seen),
            }
        }
        if seen == *event {
            (InputVerdict::Pass, seen)
        } else {
            (InputVerdict::Replace(seen), seen)
        }
    }
}

/// Why the panel is refreshed, as the kernel or the screen that drew
/// tells ([`Cx::refresh_cause`](crate::Cx::refresh_cause)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RefreshCause {
    /// Anything else, or not told.
    #[default]
    Other,
    /// A page turned (the reader tells).
    PageTurn,
    /// A screen opened or closed.
    Screens,
    /// A notice came or went.
    Notice,
    /// What a status line or a bar shows changed (a clock, the battery).
    Status,
}

/// A refresh about to be made, as the kernel decided it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refresh {
    pub mode: RefreshMode,
    /// What is refreshed.
    pub region: Rect,
    /// The screen shown on top.
    pub screen: Option<Id>,
    /// The slot it was opened for.
    pub slot: Option<Id>,
    /// Why.
    pub cause: RefreshCause,
    /// How many refreshes that leave ghosting were made since the panel
    /// was last cleaned by one that flashes.
    pub since_full: u32,
    /// The kernel made it full to clean the panel, as every
    /// [`core.full-refresh-interval`](crate::builtin::settings::FULL_REFRESH_INTERVAL)-th
    /// is: a filter that keeps its own count may make it partial again.
    pub periodic: bool,
    /// Shown in grays where the panel needs a refresh of its own for
    /// them (the X4).
    pub grays: bool,
    /// Large pictures are shown, or were before.
    pub pictures: bool,
}

impl Refresh {
    /// A refresh of `region` in `mode`, `screen` on top; the rest as
    /// nothing is known of it.
    pub fn new(mode: RefreshMode, region: Rect, screen: Option<Id>) -> Self {
        Self {
            mode,
            region,
            screen,
            slot: None,
            cause: RefreshCause::Other,
            since_full: 0,
            periodic: false,
            grays: false,
            pictures: false,
        }
    }
}

/// What a refresh filter makes of a refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefreshVerdict {
    /// As it is, for the filters after it.
    Keep,
    /// In this mode (one that flashes covers the whole screen).
    Mode(RefreshMode),
    /// Shown in grays, or not, where the panel needs a refresh of its own
    /// for them.
    Grays(bool),
    /// Not now: what changed is refreshed with the next refresh (no
    /// refresh for a clock that ticked on a page shown in grays). Not
    /// for a screen opened or closed, nor while a screen guards the
    /// device, nor more than three times in a row: then it is kept.
    Defer,
}

/// Changes how the panel is refreshed.
pub type RefreshFilterFn = Box<dyn Fn(&Env<'_>, &Refresh) -> RefreshVerdict>;

/// A refresh filter contribution: it sees every refresh the kernel
/// decided (after the periodic full refresh, [`Refresh::periodic`]) and
/// may change it; later filters see the change. A refresh that flashes
/// then covers the whole screen. The kernel counts the refreshes as they
/// are made, after the filters.
#[non_exhaustive]
pub struct RefreshFilterSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    pub filter: RefreshFilterFn,
}

impl RefreshFilterSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, filter: RefreshFilterFn) -> Self {
        Self {
            id,
            title: title.into(),
            filter,
        }
    }
}

impl fmt::Debug for RefreshFilterSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshFilterSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The registered refresh filters.
#[derive(Debug, Default)]
pub struct RefreshFilters {
    filters: Vec<RefreshFilterSpec>,
}

impl RefreshFilters {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, filter: RefreshFilterSpec) {
        self.filters.push(filter);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.filters.shrink_to_fit();
    }

    /// `refresh` as the filters change it, `None` where one defers it; one
    /// that flashes covers `screen`.
    pub fn apply(&self, env: &Env<'_>, refresh: Refresh, screen: Rect) -> Option<Refresh> {
        self.apply_with(env, refresh, screen, true, None)
    }

    /// As [`RefreshFilters::apply`], by the filters of plugin `only` alone
    /// where it is given (while a screen guards the device, its plugin's);
    /// where it `may_defer` not (a screen opened, a lock shown, refreshes
    /// deferred too often), a filter that defers keeps the refresh.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn apply_limited(
        &self,
        env: &Env<'_>,
        refresh: Refresh,
        screen: Rect,
        may_defer: bool,
        only: Option<&str>,
    ) -> Option<Refresh> {
        self.apply_with(env, refresh, screen, may_defer, only)
    }

    fn apply_with(
        &self,
        env: &Env<'_>,
        mut refresh: Refresh,
        screen: Rect,
        may_defer: bool,
        only: Option<&str>,
    ) -> Option<Refresh> {
        let mine = |filter: &&RefreshFilterSpec| only.is_none_or(|ns| filter.id.namespace() == ns);
        for filter in self.filters.iter().filter(mine) {
            match (filter.filter)(env, &refresh) {
                RefreshVerdict::Mode(mode) => {
                    refresh.mode = mode;
                    if mode.cleans() {
                        refresh.region = screen;
                    }
                }
                RefreshVerdict::Grays(grays) => refresh.grays = grays,
                RefreshVerdict::Defer if may_defer => return None,
                _ => {}
            }
        }
        Some(refresh)
    }
}
