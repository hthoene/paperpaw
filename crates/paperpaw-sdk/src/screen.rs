use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Instant, PointerEvent, Rect, RefreshMode};

use crate::command::Command;
use crate::env::Env;
use crate::gesture::Gesture;
use crate::id::Id;
use crate::params::Params;

/// Creates a screen instance when it is opened, with the parameters of the
/// command that opened it (e.g. the path of a book).
pub type ScreenFactory = Box<dyn Fn(&Env<'_>, &Params) -> Box<dyn Screen>>;

/// Makes the screens plugins added: what [`Env::make_screen`] asks (the
/// kernel's, for screens that host others). Present without the `kernel`
/// feature too: [`Env`] holds one, and a plugin built without the feature
/// must see `Env` as the kernel built it.
pub trait ScreenMaker {
    /// Screen `id` made with `params` for `env`, `None` where no plugin
    /// added it.
    fn make(&self, id: &Id, env: &Env<'_>, params: &Params) -> Option<Box<dyn Screen>>;

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
}

/// A screen contribution.
#[non_exhaustive]
pub struct ScreenSpec {
    pub id: Id,
    /// Translation key of the screen's display name.
    pub title: Cow<'static, str>,
    pub factory: ScreenFactory,
    /// How the panel is refreshed when it opens, in place of the kernel's
    /// choice (a full refresh, or a partial one where the user turned
    /// flashing off): e.g. [`RefreshMode::Fast`] for a quick sheet.
    pub refresh_on_open: Option<RefreshMode>,
    /// How many of it may be open at once: one for each set of params
    /// unless it says otherwise ([`Instances`]).
    pub instances: Instances,
}

impl ScreenSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, factory: ScreenFactory) -> Self {
        Self {
            id,
            title: title.into(),
            factory,
            refresh_on_open: None,
            instances: Instances::PER_PARAMS,
        }
    }

    /// How the panel is refreshed when it opens.
    pub fn refresh_on_open(mut self, mode: RefreshMode) -> Self {
        self.refresh_on_open = Some(mode);
        self
    }

    /// How many of it may be open at once.
    #[must_use]
    pub fn instances(mut self, instances: Instances) -> Self {
        self.instances = instances;
        self
    }
}

/// What makes a screen kept in the program ([`FixedScreen`]): a
/// function, not a closure.
pub type FixedFactory = fn(&Env<'_>, &Params) -> Box<dyn Screen>;

/// A screen contribution kept in the program: as a [`ScreenSpec`], in a
/// `static` list ([`Registrar::add_screens`](crate::Registrar::add_screens)),
/// so it takes next to no memory where a [`ScreenSpec`] takes some 60
/// bytes. Its factory is a function ([`FixedFactory`]); a screen whose
/// factory needs state of its own is a [`ScreenSpec`].
#[derive(Debug)]
pub struct FixedScreen {
    id: Id,
    title: &'static str,
    factory: FixedFactory,
    refresh_on_open: Option<RefreshMode>,
    instances: Instances,
}

impl FixedScreen {
    /// Screen `id`, called `title` (a translation key), made by `factory`.
    pub const fn new(id: Id, title: &'static str, factory: FixedFactory) -> Self {
        Self {
            id,
            title,
            factory,
            refresh_on_open: None,
            instances: Instances::PER_PARAMS,
        }
    }

    /// How the panel is refreshed when it opens ([`ScreenSpec::refresh_on_open`]).
    pub const fn refresh_on_open(mut self, mode: RefreshMode) -> Self {
        self.refresh_on_open = Some(mode);
        self
    }

    /// How many of it may be open at once ([`ScreenSpec::instances`]).
    pub const fn instances(mut self, instances: Instances) -> Self {
        self.instances = instances;
        self
    }

    pub const fn id(&self) -> &Id {
        &self.id
    }

    /// Translation key of its name.
    pub const fn title(&self) -> &'static str {
        self.title
    }

    /// The screen, made with `params`.
    pub fn make(&self, env: &Env<'_>, params: &Params) -> Box<dyn Screen> {
        (self.factory)(env, params)
    }

    /// How the panel is refreshed when it opens, if it says.
    pub const fn refresh(&self) -> Option<RefreshMode> {
        self.refresh_on_open
    }

    /// How many of it may be open at once.
    pub const fn how_many(&self) -> Instances {
        self.instances
    }
}

/// Every screen plugins added, with the translation key of its name
/// ([`Env::screens`](crate::Env::screens)): those kept in the program
/// and the others.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScreenList<'a> {
    added: &'a [(Id, Cow<'static, str>)],
    fixed: &'a [&'static FixedScreen],
}

impl<'a> ScreenList<'a> {
    /// The screens of `added` and `fixed`, each by id.
    #[cfg(feature = "kernel")]
    pub fn new(added: &'a [(Id, Cow<'static, str>)], fixed: &'a [&'static FixedScreen]) -> Self {
        Self { added, fixed }
    }

    /// Each screen's id and the translation key of its name.
    pub fn iter(self) -> impl Iterator<Item = (&'a Id, &'a str)> + 'a {
        let added = self.added.iter().map(|(id, title)| (id, title.as_ref()));
        let fixed = self.fixed.iter().map(|f| (f.id(), f.title()));
        added.chain(fixed)
    }

    pub fn len(&self) -> usize {
        self.added.len() + self.fixed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// How many of a screen may be open at once ([`ScreenSpec::instances`]).
/// Opening one that is open already goes back to it: the screens above it
/// close, as with [`Command::BackTo`], and it hears the params it was
/// opened with ([`Screen::on_reopen`]). *Back* then leaves it in one
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

impl fmt::Debug for ScreenSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScreenSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// When a screen shows the row of hints for the buttons below it, on
/// devices that have such a row (see
/// [`Registrar::set_screen_hints`](crate::Registrar::set_screen_hints)).
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
/// [`Screen::hint`]).
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

/// What an overlay lets show through it ([`Screen::shows_through`]).
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
    /// [`Screen::region`] was when the stack last changed.
    pub overlay: bool,
    /// This opening of it.
    pub key: ScreenKey,
    /// It guards the device ([`Screen::guards`], a lock): what is below
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
/// [`Screen::on_close_request`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CloseCause {
    /// *Back* ([`Command::Back`], the back button or gesture).
    Back,
    /// A tap outside an overlay (see [`Screen::outside`]).
    TapOutside,
    /// Its slot, or a screen below it, shown again: the screens above it
    /// go ([`Command::OpenSlot`], [`Command::BackTo`]).
    Slot,
}

/// What a tap outside an overlay does (see [`Screen::outside`]).
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

/// A full-screen UI unit (home, library, reader, …).
///
/// The kernel calls [`Screen::draw`] when the screen becomes visible and
/// whenever a redraw is requested. Event handlers never draw; they request
/// a redraw of the screen or of an area ([`Cx::request_redraw_area`]), and
/// the kernel refreshes exactly that region.
pub trait Screen {
    /// Draws the screen. For full screens the canvas has been cleared to
    /// white; overlays paint their own background, all of their
    /// [`region`](Self::region): where an overlay covers all of the part
    /// drawn, the screens below it are not drawn.
    ///
    /// The canvas is clipped to the part being drawn. On small devices a
    /// frame is drawn in bands, so `draw` may run several times per frame
    /// for different parts: it must not change state, and it can skip what
    /// lies outside [`Canvas::clip`].
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>);

    /// `None` for a full screen; `Some(region)` makes the screen an
    /// overlay (menu, dialog, popup) drawn above the screens below it,
    /// `region` from the corner of the part of the screen it has
    /// ([`Env::content`], that of the full screen below it). Tapping
    /// outside an overlay closes it.
    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        let _ = env;
        None
    }

    /// Whether what it shows now is best in grays (a page with a picture,
    /// a sleep screen's cover): where the panel has few but shows more
    /// with a refresh of its own (the X4: four grays, as slow as a full
    /// refresh), the whole screen is shown so while it is on top with
    /// nothing over it. Other panels refresh as asked.
    fn grays(&self) -> bool {
        false
    }

    /// Whether its pictures may be shown in grays where
    /// [`settings::PICTURES_IN_GRAYS`](crate::builtin::settings::PICTURES_IN_GRAYS)
    /// is on: when it is drawn whole and has pictures, it is shown so as
    /// if it said so ([`Screen::grays`]). Yes by default; a screen that
    /// decides itself, or whose pictures change often, says no.
    fn pictures_in_grays(&self) -> bool {
        true
    }

    /// For an overlay: whether what lies below it is washed out while it
    /// shows, so that it stands out and what it cuts through is not read
    /// as its own (menus, questions). Overlays looked through keep it as
    /// it is (a keyboard over the field it types into, a short notice, a
    /// bar for skimming the book).
    fn dims_below(&self) -> bool {
        false
    }

    /// For an overlay: what lies below it shows through it
    /// ([`ShowsThrough`]), as it draws only parts of its region or blends
    /// over it (a picture with transparency over the page read, see
    /// [`ui::draw_image_over`](crate::ui::draw_image_over)); what shows is
    /// drawn first, under all of its region. By default an overlay paints
    /// all of its region, and what lies below is not drawn.
    fn shows_through(&self) -> ShowsThrough {
        ShowsThrough::Nothing
    }

    /// Reacts to an action. Unhandled actions go to the action's global
    /// handler, if it has one.
    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled;

    /// Reacts to a recognized gesture (tap, long press, swipe). Unhandled
    /// gestures are resolved through the key map (tap zones, swipes).
    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (gesture, cx);
        Handled::No
    }

    /// Whether the screen takes the edges for itself now: swipes in from
    /// the edges (home, quick panel, back) wait until it no longer does,
    /// e.g. while things on it are arranged by dragging. Gestures a screen
    /// never wants are better switched off for it (`gestures.policy`).
    fn holds_edges(&self) -> bool {
        false
    }

    /// Whether the screen takes double taps: a second tap close to the
    /// first in time and place comes as [`GestureKind::DoubleTap`]
    /// (as a tap where it answers [`Handled::No`]). By default it comes
    /// as a tap, so that screens that take any gesture on a button do not
    /// lose quick taps.
    ///
    /// [`GestureKind::DoubleTap`]: crate::GestureKind::DoubleTap
    fn takes_double_taps(&self) -> bool {
        false
    }

    /// Whether the screen guards the device now, as a lock screen does
    /// while it is locked. Until it no longer does, nothing shows above
    /// it but the screens it opens itself (its keyboard) and those they
    /// open: screens others open meanwhile (a rule, a timer, a plugin at
    /// start) wait below it, others' [`Command::Back`] closes nothing,
    /// and notices wait until it lets go.
    fn guards(&self) -> bool {
        false
    }

    /// Whether what the screen shows is secret now, as a PIN or a
    /// password being typed: where its focus is, is not told to plugins
    /// ([`events::FOCUS_CHANGED`](crate::builtin::events::FOCUS_CHANGED)).
    /// Screens that guard the device ([`Screen::guards`]) count as secret
    /// too, and so do the screens above them.
    fn secret(&self) -> bool {
        false
    }

    /// What a tap outside it does, for an overlay: by default it closes
    /// it.
    fn outside(&self) -> Outside {
        Outside::Close
    }

    /// For an overlay: whether input it does not take (an action or a
    /// gesture it answers [`Handled::No`]) goes on to the screen below it,
    /// as for a bar or a panel that leaves the page usable. By default it
    /// goes to the action's handler and the key map, as on top.
    fn passes_through(&self) -> bool {
        false
    }

    /// Whether its taps and swipes go the other way: what the key map
    /// binds to the left side and to a swipe to the left happens on the
    /// right and for a swipe to the right, and the other way round, e.g.
    /// a book whose pages go right to left
    /// ([`PageProgression`](crate::document::PageProgression)). Buttons
    /// stay as they are.
    fn mirrored(&self) -> bool {
        false
    }

    /// When it shows the row of hints, where it knows better than the
    /// plugin said for all its openings
    /// ([`Registrar::set_screen_hints`](crate::Registrar::set_screen_hints)):
    /// read when it is opened, for the part of the screen it is made for,
    /// and whenever the row would come for a while ([`HintVisibility::OnPress`]).
    fn hints(&self) -> Option<HintVisibility> {
        None
    }

    /// Asked before the kernel closes it for `why` (*Back*, a tap
    /// outside, its slot shown again): [`Handled::Yes`] keeps it open,
    /// e.g. to ask first whether text typed is to be thrown away. Not
    /// asked where it closes itself ([`Cx::close`]) or its plugin is
    /// removed.
    fn on_close_request(&mut self, why: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (why, cx);
        Handled::No
    }

    /// Raw pointer samples, offered before gesture recognition and before
    /// swipes in from the edges. Returning [`Handled::Yes`] for a `Down`
    /// event captures that pointer: all its further samples go here and
    /// no gesture is recognized. Drawing and handwriting screens use this,
    /// and grips that move rows ([`ui::reorder::Reorder`](crate::ui::reorder::Reorder)).
    /// Take only fingers meant for the screen: one taken at an edge does
    /// not swipe in from it.
    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (event, cx);
        Handled::No
    }

    /// Whether the screen takes buttons by name now ([`Screen::on_button`]),
    /// e.g. while a device check or a key binding editor asks for one.
    /// While it does, a press counts when the button is let go, so a held
    /// one is told apart, also where nothing is bound to holding it.
    fn takes_buttons(&self) -> bool {
        false
    }

    /// A button pressed (`long`: held), by its name on the device
    /// ([`InputCaps::keys`](paperpaw_platform::InputCaps), e.g.
    /// `page-forward`), offered to the screen on top while it
    /// [takes buttons](Screen::takes_buttons), before the key map turns
    /// it into an action. Returning [`Handled::Yes`] takes it: no action
    /// runs, not even *Home* or *Sleep*. Take buttons only while asking
    /// for them, so the reader can always leave.
    fn on_button(&mut self, name: &str, long: bool, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (name, long, cx);
        Handled::No
    }

    /// How the screen runs while it is on top
    /// ([`ScreenMode`](crate::realtime::ScreenMode)): full screen, raw
    /// keys, its own refreshes, a pace. Read whenever it counts (each
    /// input, each frame), so it may change as the screen goes on; whether
    /// it is full screen counts when the screen is made. By default a
    /// screen of pages.
    fn mode(&self) -> crate::realtime::ScreenMode {
        crate::realtime::ScreenMode::new()
    }

    /// A button pressed or let go, for a screen that takes raw keys
    /// ([`ScreenMode::raw_keys`](crate::realtime::ScreenMode::raw_keys)),
    /// before the key map. A press it takes ([`Handled::Yes`]) brings its
    /// release here too; one it does not goes to the key map with its
    /// release, as on other screens. The power button never comes, and
    /// holding *Back* for [`ESCAPE_HOLD`](crate::realtime::ESCAPE_HOLD)
    /// leaves the screen.
    fn on_key(&mut self, key: &crate::realtime::KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (key, cx);
        Handled::No
    }

    /// A call that comes after this release, by its key (see
    /// [`Command::Extended`]): what PaperPaw asks of screens later reaches
    /// screens built now, which answer [`Handled::No`] to keys they do not
    /// know.
    fn on_extended(&mut self, key: &Id, params: &Params, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (key, params, cx);
        Handled::No
    }

    /// How the row of hints below the screen labels the button that runs
    /// `action` here, on devices with buttons in a row under the screen
    /// ([`settings::BUTTON_HINTS`](crate::builtin::settings::BUTTON_HINTS)):
    /// the action's usual name unless the screen says better what the
    /// button does, or that it does nothing.
    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        let _ = (action, env);
        Hint::Default
    }

    /// Called whenever the screen becomes the visible one: after it was
    /// opened and when the screens above it were closed. A good place to
    /// start background work with [`Cx::wake_at`].
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let _ = cx;
    }

    /// Called when the screen is opened again while it is open (see
    /// [`Instances`]), after the screens above it were closed (it then
    /// hears [`Screen::on_show`] after this): `params` are those it was
    /// opened with now, the same, or others for a screen of
    /// [`Instances::SINGLE`], which shows them as if it had been opened
    /// with them.
    fn on_reopen(&mut self, params: &Params, cx: &mut Cx<'_, '_>) {
        let _ = (params, cx);
    }

    /// Called while a screen above it needs memory
    /// ([`ScreenMode::memory`](crate::realtime::ScreenMode::memory)): it
    /// lets go of what it can make again (a book its pages laid out and
    /// its chapter read, pictures, lists), keeping what the user would
    /// miss (the place in the book, what was typed), and makes it again
    /// as it shows next ([`Screen::on_show`]). `Handled::Yes` where it let
    /// go of something. Screens that do not are closed where there is
    /// still too little, and opened again with their params later.
    fn on_squeeze(&mut self, squeeze: &crate::realtime::Squeeze, cx: &mut Cx<'_, '_>) -> Handled {
        let _ = (squeeze, cx);
        Handled::No
    }

    /// Called for every emitted event (after its subscribers), on every
    /// open screen, e.g. `core.setting-changed` to re-layout.
    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let _ = (event, params, cx);
    }

    /// Called when a full screen opens above it, which hides it until
    /// that one closes: a moment to let go of what it keeps only to draw
    /// itself, e.g. pictures made small, so that the screen above has the
    /// memory. It makes it again when it shows: in
    /// [`on_show`](Self::on_show) where it is on top again, or when it is
    /// drawn below an overlay left open above it.
    fn on_hide(&mut self) {}

    /// Called once when the screen is closed (gone back from, or replaced
    /// in its slot), before it is dropped: a moment to say so, e.g. the
    /// reader emits that the book was closed. Also when the screen is made
    /// anew for another size (turned, the row of hints changed): the old
    /// one is dropped then too, so it keeps here what it would lose (a
    /// position, text typed); the commands it gives then are not run, as
    /// the screen stays open, made anew.
    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        let _ = cx;
    }

    /// Called when a time requested with [`Cx::wake_at`] has come. Long
    /// work (e.g. paginating a book) is split into steps that each request
    /// the next wake-up, so input stays responsive.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let _ = cx;
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
    /// Why the panel is refreshed ([`Cx::refresh_cause`]).
    pub refresh_cause: Option<crate::filters::RefreshCause>,
    /// Whether the whole screen should be redrawn.
    pub redraw: bool,
    /// Part of the screen to redraw, from the corner of the part of the
    /// screen the screen has ([`Env::content`]).
    pub redraw_area: Option<Rect>,
    /// When the screen wants [`Screen::on_timer`] to be called (see
    /// [`Cx::wake_at`]).
    pub wake_at: Option<Instant>,
}

impl Output {
    /// Records a refresh request; the strongest request wins.
    pub fn request_refresh(&mut self, mode: RefreshMode) {
        self.refresh = Some(self.refresh.map_or(mode, |m| m.max(mode)));
    }
}

/// Context passed to screen event handlers.
#[derive(Debug)]
pub struct Cx<'a, 'e> {
    env: &'a Env<'e>,
    output: &'a mut Output,
    /// The screen whose handler runs, if it is one of the stack's.
    key: Option<ScreenKey>,
    /// The params of the action that runs ([`Cx::params`]).
    params: &'a Params,
}

/// No params, for a context without an action's.
const NO_PARAMS: &Params = &Params::new();

impl<'a, 'e> Cx<'a, 'e> {
    /// Used by the kernel and by tests.
    pub fn new(env: &'a Env<'e>, output: &'a mut Output) -> Self {
        Self {
            env,
            output,
            key: None,
            params: NO_PARAMS,
        }
    }

    /// For an action run with `params` (the kernel's, or a test that
    /// plays it).
    pub fn with_params(mut self, params: &'a Params) -> Self {
        self.params = params;
        self
    }

    /// The params the action that [`Screen::on_action`] gets was run
    /// with ([`Command::RunAction`]); none from an input, and none outside
    /// `on_action`.
    pub fn params(&self) -> &Params {
        self.params
    }

    /// For the handler of the open screen `key` (the kernel's, or a test
    /// that plays it).
    pub fn for_screen(mut self, key: ScreenKey) -> Self {
        self.key = Some(key);
        self
    }

    /// Which opening of a screen this handler runs for: `None` outside
    /// the stack (a screen made aside, a notice).
    pub fn key(&self) -> Option<ScreenKey> {
        self.key
    }

    /// This screen as the stack lists it ([`Env::stack`](crate::Env::stack)).
    pub fn entry(&self) -> Option<&StackEntry> {
        let key = self.key?;
        self.env.stack().iter().find(|entry| entry.key == key)
    }

    /// Which screen this is.
    pub fn screen_id(&self) -> Option<&Id> {
        self.entry().map(|entry| &entry.screen)
    }

    /// The slot this screen was opened for, if any.
    pub fn slot(&self) -> Option<&Id> {
        self.entry().and_then(|entry| entry.slot.as_ref())
    }

    /// Closes this screen, wherever it is in the stack ([`Command::Close`]):
    /// the screens above it stay. Nothing outside the stack.
    pub fn close(&mut self) {
        if let Some(key) = self.key {
            self.output.commands.push(Command::Close(key));
        }
    }

    pub fn env(&self) -> &Env<'e> {
        self.env
    }

    /// Queues a command for the kernel.
    pub fn command(&mut self, command: Command) {
        self.output.commands.push(command);
    }

    /// Requests a refresh mode for what was drawn; the strongest request of
    /// an event wins.
    pub fn request_refresh(&mut self, mode: RefreshMode) {
        self.output.request_refresh(mode);
    }

    /// Sets the refresh mode for what was drawn exactly, over the requests
    /// of the event (the last one set wins): e.g. [`RefreshMode::Fast`]
    /// for an overlay opening, where the kernel would choose more. Refresh
    /// filters still see it.
    pub fn set_refresh(&mut self, mode: RefreshMode) {
        self.output.refresh_set = Some(mode);
    }

    /// Puts key map layer `layer` in front of the slot's for this screen
    /// while it is on top (see [`Command::SetKeyLayer`]); `None` takes it
    /// away. Nothing for a screen not of the stack.
    pub fn set_key_layer(&mut self, layer: Option<Id>) {
        if let Some(key) = self.key {
            self.output.commands.push(Command::SetKeyLayer(key, layer));
        }
    }

    /// Says why the panel is refreshed, for refresh filters (the reader
    /// tells [`RefreshCause::PageTurn`](crate::filters::RefreshCause::PageTurn)).
    pub fn refresh_cause(&mut self, cause: crate::filters::RefreshCause) {
        self.output.refresh_cause = Some(cause);
    }

    /// Requests a complete redraw via [`Screen::draw`].
    pub fn request_redraw(&mut self) {
        self.output.redraw = true;
    }

    /// Requests a redraw of `area` only, from the corner of the part of
    /// the screen the screen has ([`Env::content`]), as it draws; e.g. a
    /// status line or a focused item: [`Screen::draw`] runs clipped to it.
    pub fn request_redraw_area(&mut self, area: Rect) {
        self.output.redraw_area = Some(self.output.redraw_area.map_or(area, |a| a.union(&area)));
    }

    /// Requests a call to [`Screen::on_timer`] of this screen at `at` (the
    /// earliest request wins). Use `cx.env().now()` for "as soon as
    /// possible". It comes while the screen is on top: covered by another
    /// (a menu, a keyboard), the time waits until it is on top again. It
    /// is kept when the screen is made anew for another size.
    /// Keeps the device from going to sleep by itself while this screen
    /// is on top, for as long as `hold` lasts (renew it before it ends):
    /// a clock, a timer, a slideshow. It ends when the screen closes.
    pub fn keep_awake(&mut self, hold: crate::power::WakeHold) {
        let hold = hold.of_screen(self.key);
        self.output.commands.push(Command::KeepAwake(hold));
    }

    /// Lets go of the hold of `id`.
    pub fn let_sleep(&mut self, id: impl Into<Id>) {
        self.output.commands.push(Command::LetSleep(id.into()));
    }

    pub fn wake_at(&mut self, at: Instant) {
        self.output.wake_at = Some(self.output.wake_at.map_or(at, |w| w.min(at)));
    }
}
