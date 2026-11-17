use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::{Instant, Rect, RefreshMode};

use crate::command::Command;
use crate::id::Id;
use crate::params::Params;

/// Answers what the screens ask of the kernel's screens and slots.
/// Present without the `kernel` feature too: [`Env`] holds one, and a plugin built without the feature
/// must see `Env` as the kernel built it.
pub trait ScreenMaker {
    /// The screen slot `slot` shows, as the user or the plugins bound it.
    fn slot_screen(&self, slot: &str) -> Option<&Id> {
        let _ = slot;
        None
    }

    /// Every screen offered for slot `slot`: the highest priority first,
    /// those of one priority in the order offered.
    fn slot_screens(&self, slot: &str) -> Vec<&Id> {
        let _ = slot;
        Vec::new()
    }

    /// [`slot_screens`](Self::slot_screens) without a list: `visit` is
    /// called with each screen in that order until it returns `false`. The
    /// kernel's maker allocates nothing here; this default walks
    /// `slot_screens`.
    fn each_slot_screen<'s>(&'s self, slot: &str, visit: &mut dyn FnMut(&'s Id) -> bool) {
        for id in self.slot_screens(slot) {
            if !visit(id) {
                break;
            }
        }
    }

    /// The screen the user chose for slot `slot` (`[slots]`), where it is
    /// there; `None` for automatic (the plugins' choice).
    fn slot_choice(&self, slot: &str) -> Option<&Id> {
        let _ = slot;
        None
    }

    /// The translation key of slot `slot`'s name.
    fn slot_title(&self, slot: &str) -> Option<&str> {
        let _ = slot;
        None
    }

    /// The translation key of screen `screen`'s name.
    fn screen_title(&self, screen: &Id) -> Option<&str> {
        let _ = screen;
        None
    }

    /// A question asked after this release, by its key and parameters:
    /// what plugins ask of the screens later reaches a kernel built now,
    /// which answers `None` to keys it does not know. None is asked yet.
    fn extended(&self, key: &Id, params: &Params) -> Option<Params> {
        let _ = (key, params);
        None
    }
}

/// Every screen plugins added, with the translation key of its name
/// ([`Env::screens`](crate::Env::screens)): interface-6 plugins', interface-7
/// plugins' ([`Titled`](crate::Titled)) and those kept in the program.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScreenList<'a> {
    added: &'a [(Id, Cow<'static, str>)],
    native: &'a [crate::Titled],
    fixed: &'a [&'static crate::TitleEntry],
}

impl<'a> ScreenList<'a> {
    /// The screens of `added`, `native` and `fixed`, each sorted by id.
    #[cfg(feature = "kernel")]
    pub fn new(
        added: &'a [(Id, Cow<'static, str>)],
        native: &'a [crate::Titled],
        fixed: &'a [&'static crate::TitleEntry],
    ) -> Self {
        Self {
            added,
            native,
            fixed,
        }
    }

    /// Each screen's id and the translation key of its name: those plugins
    /// added in the order of their ids, then those kept in the program.
    pub fn iter(self) -> impl Iterator<Item = (&'a Id, &'a str)> + 'a {
        let mut added = self
            .added
            .iter()
            .map(|(id, title)| (id, title.as_ref()))
            .peekable();
        let mut native = self
            .native
            .iter()
            .map(|titled| (titled.id(), titled.title()))
            .peekable();
        let merged = core::iter::from_fn(move || match (added.peek(), native.peek()) {
            (Some(a), Some(n)) if n.0 < a.0 => native.next(),
            (Some(_), _) => added.next(),
            (None, _) => native.next(),
        });
        let fixed = self.fixed.iter().map(|f| (&f.id, f.title));
        merged.chain(fixed)
    }

    pub fn len(&self) -> usize {
        self.added.len() + self.native.len() + self.fixed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

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
/// (see [`StackEntry::key`], [`Cx::close`]).
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

/// An open screen, as [`Env::stack`](crate::Env::stack) lists them.
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

/// What a plugin asked the kernel to do during an event.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Output {
    pub commands: Vec<Command>,
    /// Requested refresh mode for the region drawn during the event.
    pub refresh: Option<RefreshMode>,
    /// The mode set exactly ([`Cx::set_refresh`]): it wins over requests.
    pub refresh_set: Option<RefreshMode>,
    /// Whether the whole screen should be redrawn.
    pub redraw: bool,
    /// Part of the screen to redraw, from the corner of the part of the
    /// screen the screen has ([`Env::content`]).
    pub redraw_area: Option<Rect>,
    /// When the screen wants `on_timer` to be called (see
    /// [`Cx::wake_at`]).
    pub wake_at: Option<Instant>,
}

impl Output {
    /// Records a refresh request; the strongest request wins.
    pub fn request_refresh(&mut self, mode: RefreshMode) {
        self.refresh = Some(self.refresh.map_or(mode, |m| m.max(mode)));
    }
}




#[cfg(test)]
mod tests {
    use super::*;

    /// A maker that offers three screens for `core.home` and makes none.
    struct Offers([Id; 3]);

    impl ScreenMaker for Offers {
        fn slot_screens(&self, slot: &str) -> Vec<&Id> {
            if slot == "core.home" {
                self.0.iter().collect()
            } else {
                Vec::new()
            }
        }
    }

    #[test]
    fn each_slot_screen_visits_in_order_and_stops_when_told() {
        let ids = ["a.one", "b.two", "c.three"].map(|id| Id::parse(id).unwrap());
        let maker = Offers(ids.clone());
        let mut seen = Vec::new();
        maker.each_slot_screen("core.home", &mut |id| {
            seen.push(id.as_str());
            true
        });
        assert_eq!(seen, ["a.one", "b.two", "c.three"]);
        seen.clear();
        maker.each_slot_screen("core.home", &mut |id| {
            seen.push(id.as_str());
            seen.len() < 2
        });
        assert_eq!(seen, ["a.one", "b.two"]);
        maker.each_slot_screen("core.other", &mut |_| panic!("no screen offered"));
    }

    #[test]
    fn a_pane_this_sdk_does_not_know_is_placed_as_full() {
        assert_eq!(Pane(7).known(), Pane::FULL);
        assert_eq!(Pane::DETAIL.known(), Pane::DETAIL);
        assert_eq!(Pane::FULL.known(), Pane::FULL);
        assert_eq!(Pane::default(), Pane::FULL);
    }

    /// Interface-6 and interface-7 plugins' screens come in the order of their
    /// ids, as when they were one list (#248, B), each with its title.
    #[cfg(feature = "kernel")]
    #[test]
    fn a_screen_list_reads_both_kinds_in_the_order_of_their_ids() {
        static KEPT: [crate::TitleEntry; 2] = [
            crate::TitleEntry::new(Id::new_static("b.screen"), "b.title"),
            crate::TitleEntry::new(Id::new_static("d.screen"), "d.title"),
        ];
        let added = [
            (Id::new_static("a.screen"), Cow::Borrowed("a.title")),
            (Id::new_static("c.screen"), Cow::Borrowed("c.title")),
        ];
        let owned = alloc::boxed::Box::new((Id::new_static("e.screen"), Cow::Borrowed("e.title")));
        let native = [
            crate::Titled::kept(&KEPT[0]),
            crate::Titled::kept(&KEPT[1]),
            crate::Titled::owned(owned),
        ];
        let list = ScreenList::new(&added, &native, &[]);
        let read: alloc::vec::Vec<(&str, &str)> = list
            .iter()
            .map(|(id, title)| (id.as_str(), title))
            .collect();
        assert_eq!(
            read,
            [
                ("a.screen", "a.title"),
                ("b.screen", "b.title"),
                ("c.screen", "c.title"),
                ("d.screen", "d.title"),
                ("e.screen", "e.title"),
            ]
        );
        assert_eq!(list.len(), 5);
        assert!(core::ptr::eq(list.iter().nth(1).unwrap().0, &KEPT[0].id));
    }
}
