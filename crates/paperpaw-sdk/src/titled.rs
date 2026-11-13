//! An id with the translation key of its name, as the kernel keeps an
//! interface-7 plugin's screens and actions (#248).

use alloc::borrow::Cow;
use alloc::boxed::Box;
use core::fmt;

use crate::id::Id;

/// An entry of a linked plugin's titles table, kept in the program: the id of
/// a screen or an action it declares and the translation key of its name
/// ([`fixed_titles_source`](crate::native::declare::fixed_titles_source)).
/// Made with [`TitleEntry::new`], so that it can grow later.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TitleEntry {
    /// The screen's or the action's id.
    pub id: Id,
    /// The translation key of its name.
    pub title: &'static str,
}

impl TitleEntry {
    /// The entry for `id`, called `title`; usable in `static` items.
    #[must_use]
    pub const fn new(id: Id, title: &'static str) -> Self {
        Self { id, title }
    }
}

/// An id and the translation key of its name (a screen's, an action's): an
/// entry kept in the program, which a linked plugin's build makes of its own
/// declarations ([`fixed_titles_source`](crate::native::declare::fixed_titles_source)),
/// or a copy of its own. Reads the same either way.
#[derive(Clone)]
pub struct Titled(Held);

#[derive(Clone)]
enum Held {
    Kept(&'static TitleEntry),
    Owned(Box<(Id, Cow<'static, str>)>),
}

impl Titled {
    /// The entry `entry`, kept in the program: nothing is copied.
    #[must_use]
    pub const fn kept(entry: &'static TitleEntry) -> Self {
        Self(Held::Kept(entry))
    }

    /// The id and title in `owned`, a copy of its own (the caller made the
    /// box, so that a refusal of the heap is its to handle).
    #[must_use]
    pub fn owned(owned: Box<(Id, Cow<'static, str>)>) -> Self {
        Self(Held::Owned(owned))
    }

    /// The id.
    #[must_use]
    pub fn id(&self) -> &Id {
        match &self.0 {
            Held::Kept(entry) => &entry.id,
            Held::Owned(owned) => &owned.0,
        }
    }

    /// The translation key of its name.
    #[must_use]
    pub fn title(&self) -> &str {
        match &self.0 {
            Held::Kept(entry) => entry.title,
            Held::Owned(owned) => &owned.1,
        }
    }

    /// The translation key of its name, as a key that outlives it (no copy
    /// where it is kept in the program).
    #[must_use]
    pub fn title_key(&self) -> Cow<'static, str> {
        match &self.0 {
            Held::Kept(entry) => Cow::Borrowed(entry.title),
            Held::Owned(owned) => owned.1.clone(),
        }
    }

    /// Whether it is kept in the program ([`Titled::kept`]), not a copy.
    #[must_use]
    pub const fn is_kept(&self) -> bool {
        matches!(self.0, Held::Kept(_))
    }
}

impl fmt::Debug for Titled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Titled")
            .field(self.id())
            .field(&self.title())
            .finish()
    }
}

impl PartialEq for Titled {
    /// By id and title, wherever each is kept.
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id() && self.title() == other.title()
    }
}

impl Eq for Titled {}
