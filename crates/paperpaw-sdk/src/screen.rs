use alloc::string::String;

use crate::id::Id;

/// How many of a screen may be open at once (`instances`).
/// Opening one that is open already goes back to it: the screens above it
/// close, as with [`Command::BackTo`], and it hears the params it was
/// opened with (`on_reopen`). *Back* then leaves it in one
/// press, as if it had been opened once. Constants of one type, so more
/// can come without a new plugin interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instances(u8);

impl Instances {
    /// One for each set of params (the default): opened again with the
    /// same params, the open one is shown; with others, another opens
    /// above (the details of another book, a folder in a folder).
    pub const PER_PARAMS: Self = Self(0);
    /// One at most, whatever the params: opened again, the open one is
    /// shown with the new params (a transfer screen, a settings root).
    pub const SINGLE: Self = Self(1);
    /// As many as are opened, each above the last.
    pub const MANY: Self = Self(2);
}

impl Default for Instances {
    fn default() -> Self {
        Self::PER_PARAMS
    }
}

/// When a screen shows the row of hints for the buttons below it, on
/// devices that have such a row (see
/// `Items::set_screen_hints`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum HintVisibility {
    /// Always, below it: the screen has the part above the row. Also for
    /// menus over a screen without it.
    #[default]
    Shown,
    /// For a few seconds after one of the buttons in the row was pressed
    /// (or [`Command::ShowHints`](crate::Command::ShowHints)), over its
    /// bottom edge: the screen has all of the screen, e.g. the reader.
    OnPress,
    /// Never.
    Hidden,
}

/// How the row of hints below the screen labels a button under it (see
/// `hint`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Hint {
    /// What the action is usually called.
    Default,
    /// A word for what the button does here.
    Text(String),
    /// An icon ([`icons`](crate::icons)), e.g. an arrow.
    Icon(&'static str),
    /// Nothing: the button does nothing here.
    Hidden,
    /// A value a later build adds, by its name (a constant of that build);
    /// a build that doesn't know it: it shows no hint.
    Other(&'static str),
}

impl Hint {
    /// For a screen where only *Back* does anything (a page to read, a
    /// code to scan): the other buttons' hints hidden.
    pub fn only_back(action: &Id) -> Hint {
        if *action == crate::builtin::actions::BACK {
            Hint::Default
        } else {
            Hint::Hidden
        }
    }
}

/// Tells one opened screen from every other, also from another opening
/// of the same screen: as long as it is open, through being made anew
/// (see [`StackEntry::key`], `Cx::close`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenKey(u32);

impl ScreenKey {
    /// The key numbered `serial`; the kernel numbers the screens it opens.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub const fn new(serial: u32) -> Self {
        Self(serial)
    }

    /// Its number, as the kernel gave it.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub const fn serial(self) -> u32 {
        self.0
    }
}

/// What an overlay lets show through it (`shows_through`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShowsThrough {
    /// Nothing: it paints all of its region.
    #[default]
    Nothing,
    /// Every screen below it, the overlays among them too (a sticker over
    /// a menu).
    All,
    /// The topmost full screen below it alone: the overlays between are
    /// not drawn under its region (a picture over the page read, not over
    /// the reader's menu left open).
    FullScreen,
}

/// An open screen, as `Env::stack` lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StackEntry {
    /// Which screen it is.
    pub screen: Id,
    /// The slot it was opened for, if any (e.g. the reader's).
    pub slot: Option<Id>,
    /// Shown over part of the screen below it (a menu, a dialog), as its
    /// `region` was when the stack last changed.
    pub overlay: bool,
    /// This opening of it.
    pub key: ScreenKey,
    /// It guards the device (`guards`, a lock): what is below
    /// it is not to be seen.
    pub guards: bool,
}

impl StackEntry {
    /// Made of its fields, e.g. by the kernel or a test.
    pub fn new(screen: Id, slot: Option<Id>, overlay: bool, key: ScreenKey) -> Self {
        Self {
            screen,
            slot,
            overlay,
            key,
            guards: false,
        }
    }

    /// The same, guarding the device or not ([`StackEntry::guards`]).
    pub fn guarding(mut self, guards: bool) -> Self {
        self.guards = guards;
        self
    }

    /// Whether it is screen `id`, or the screen of slot `id`.
    pub fn is(&self, id: &Id) -> bool {
        self.screen == *id || self.slot.as_ref() == Some(id)
    }
}

/// Why the kernel is about to close a screen (see
/// `on_close_request`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CloseCause {
    /// *Back* ([`Command::Back`], the back button or gesture).
    Back,
    /// A tap outside an overlay (see `outside`).
    TapOutside,
    /// Its slot, or a screen below it, shown again: the screens above it
    /// go ([`Command::OpenSlot`], [`Command::BackTo`]).
    Slot,
}

/// What a tap outside an overlay does (see `outside`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Outside {
    /// It closes the overlay, as *Back* does.
    #[default]
    Close,
    /// Nothing: the overlay stays, the tap is taken.
    Ignore,
    /// It goes to the screen below, as if the overlay were not there (a
    /// bar for skimming that leaves the page usable).
    PassThrough,
}

/// Whether an event was consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    Yes,
    No,
}

/// The revision of a screen's items in this build: what the kernel's
/// layers say in `revision`. 1: the items up to `on_timer`; 2:
/// `on_resize` and `pane`; 3: `times_under_overlays`.
pub const SCREEN_REVISION: u16 = 3;

/// How a screen shows where the screen is wide (`pane`).
/// Constants of one type, so more can come without a new plugin
/// interface: the kernel takes a value it does not know as
/// [`Pane::FULL`] ([`Pane::known`]), so a screen built with a later SDK
/// never hands it one it cannot place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pane(u8);

impl Pane {
    /// On all of its part of the screen, as any screen (the default).
    pub const FULL: Self = Self(0);
    /// One thing of the list the screen below it shows (a book's details,
    /// a message, a note): where the screen is wide enough, beside that
    /// list; elsewhere as [`Pane::FULL`].
    pub const DETAIL: Self = Self(1);

    /// This one where this SDK knows it, else [`Pane::FULL`]: what the
    /// kernel places by, comparing with the constants, never matching on
    /// the number.
    pub fn known(self) -> Self {
        if self == Self::DETAIL {
            self
        } else {
            Self::FULL
        }
    }
}

impl Default for Pane {
    fn default() -> Self {
        Self::FULL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_this_sdk_does_not_know_is_placed_as_full() {
        assert_eq!(Pane(7).known(), Pane::FULL);
        assert_eq!(Pane::DETAIL.known(), Pane::DETAIL);
        assert_eq!(Pane::FULL.known(), Pane::FULL);
        assert_eq!(Pane::default(), Pane::FULL);
    }
}
