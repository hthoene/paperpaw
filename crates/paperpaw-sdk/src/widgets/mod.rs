//! Widgets: small boxes plugins contribute for screens that show them,
//! such as the home screen's pages (a clock, the book being read, today's
//! reading, a grid of tools). Any plugin adds widgets with
//! `Items::add_widget`; screens let
//! the user choose which to show and where.
//!
//! A screen that shows a widget makes it (`Widgets::make`) and keeps
//! it: the widget reads what it shows when the screen updates it (when
//! it shows, about each minute, after events), and draws from that, so
//! drawing stays quick ([`Widget`]). Widgets with nothing to read, such
//! as a clock, are a pair of functions ([`WidgetSpec::drawn`]).
//!
//! A widget may have targets: places that do something when tapped, which
//! buttons reach one after the other too (see [`WidgetTarget`]).
//!
//! A widget may have settings of its own, e.g. where its text sits
//! (`align_setting`): they go in a settings group with the widget's id
//! (`settings_group`). The settings list leaves such groups out; screens
//! that edit widgets open them (slot
//! [`SETTINGS`](crate::builtin::slots::SETTINGS) with parameter
//! [`ID`](crate::builtin::params::ID)).
//!
//! The kernel brings two widgets for room between the others, which a list
//! may hold more than once ([`repeatable`]): [`SPACE`], a gap of a set
//! height, and [`FILL`], the room the others leave. Screens that place
//! widgets one below the other give each fill its share of that room
//! (`fill_height`); elsewhere it is not there.
//!
//! Screens that show widgets in a grid, as the home screen does, use
//! [`grid`]: its layout as data, and a view that edits it in place.
//! Screens that show them above each other, as the sleep screen does, use
//! a `WidgetStack`. Either way the screen asks `refresh` what an event means
//! for the widgets it shows, and reads the live ones anew each minute
//! (`next_minute`).
//!
//! The places where widgets show are [`WidgetPlace`]s
//! (`Items::add_widget_place`):
//! the settings that list them, and whether they show in a grid or above
//! each other. A plugin asks whether any place shows its widget
//! ([`Widgets::shown`]), e.g. to fetch the weather only then; the slot
//! [`EDIT_WIDGETS`](crate::builtin::slots::EDIT_WIDGETS) edits a place's
//! list.
//!
//! Widgets share a look the reader shapes for each ([`Frame`], [`TextSize`],
//! [`draw_caption`], see [`widget_look`]), and say which kind they are
//! ([`categories`]), so that screens offering many of them group them.
//!
//! A widget may instead preview a group of settings
//! ([`WidgetSpec::previews`]), e.g. a sample of text laid out as the
//! reading settings say: settings pages show it above the group's
//! settings, and while a value is being chosen it shows that one
//! ([`Command::PreviewSettings`](crate::Command::PreviewSettings)). It is
//! not offered among the widgets for other screens.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_graphics::Canvas;
use paperpaw_platform::Rect;

use crate::env::Env;
use crate::id::Id;
use crate::params::Params;

mod colours;
mod elsewhere;
pub mod grid;
pub(crate) mod look;
mod shape;
mod stack;


pub use colours::{
    ColourSpec, PAPER_LIGHT_GRAY, PAPER_NONE,
    PAPER_NORMAL,     colour_spec};

pub use look::{
     Frame,       Wrapped};

/// The kinds of widgets ([`WidgetSpec::category`]), each the translation
/// key of its name; screens that offer widgets group them so.
pub mod categories {
    /// Clocks, the date, calendars.
    pub const TIME: &str = "core.widgets.time";
    /// The book being read and others: covers, progress, shelves.
    pub const BOOKS: &str = "core.widgets.books";
    /// Reading itself: time read, goals, streaks.
    pub const READING: &str = "core.widgets.reading";
    /// Words and texts: quotes, highlights, vocabulary.
    pub const WORDS: &str = "core.widgets.words";
    /// One's own: notes, to-dos, pictures.
    pub const PERSONAL: &str = "core.widgets.personal";
    /// From outside: news, weather.
    pub const WORLD: &str = "core.widgets.world";
    /// The device: battery, status.
    pub const DEVICE: &str = "core.widgets.device";
    /// Room between the others.
    pub const LAYOUT: &str = "core.widgets.layout";
    /// Any other.
    pub const OTHER: &str = "core.widgets.other";

    /// Every kind, in the order screens show them.
    pub const ALL: &[&str] = &[
        TIME, BOOKS, READING, WORDS, PERSONAL, WORLD, DEVICE, LAYOUT, OTHER,
    ];

    /// The icon of kind `kind` among these, if it is one.
    pub fn icon(kind: &str) -> Option<&'static str> {
        Some(match kind {
            TIME => "clock",
            BOOKS => "books",
            READING => "chart-bar",
            WORDS => "letter-case",
            PERSONAL => "note",
            WORLD => "world",
            DEVICE => "device-tablet",
            LAYOUT => "layout",
            OTHER => "puzzle",
            _ => return None,
        })
    }
}

/// A kind of widgets of a plugin's own
/// (`Items::add_widget_category`),
/// beside [`categories`]: its id, the translation key of its name, and
/// its icon. Screens offering widgets list such kinds after the
/// kernel's.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub(crate) struct WidgetCategory {
    pub id: Id,
    pub icon: Cow<'static, str>,
}

/// A widget as a screen shows it: made for that screen by its spec
/// ([`WidgetSpec`]), it keeps what it shows.
///
/// What it shows is read in [`update`](Self::update), when the screen
/// asks: when it shows the widget, about each minute, and after events
/// that may change it (e.g. the library or a setting changed). Drawing
/// then only draws: a frame may be drawn in bands, so
/// [`draw`](Self::draw) runs several times for it and reads nothing from
/// the storage (but pictures, which are drawn from their files).
pub trait Widget {
    /// Reads what it shows anew; `true` if that changed since it was
    /// last read, so that the screen draws it again. Widgets with nothing
    /// to read keep the default: nothing changes.
    fn update(&mut self, env: &Env<'_>) -> bool {
        let _ = env;
        false
    }

    /// Whether what it shows changes by itself (the time, the battery):
    /// it is read anew each minute and when the device's status changes.
    /// Widgets that show what changes only after events (books, settings)
    /// say `false`, and are read when shown and after those events only:
    /// reading them each minute would cost time and battery for nothing.
    fn live(&self) -> bool {
        true
    }

    /// The height it needs at `width`, in pixels (0: nothing to show).
    fn height(&self, env: &Env<'_>, width: u16) -> u16;

    /// The least height it can do with at `width`, drawn smaller (e.g.
    /// covers in a row): where its full [`height`](Self::height) has no
    /// room, it is placed in less, down to this. Widgets that cannot
    /// shrink keep the default, their full height.
    fn min_height(&self, env: &Env<'_>, width: u16) -> u16 {
        self.height(env, width)
    }

    /// Where it is drawn from now on, before it is drawn there: screens
    /// that show it call this once they placed it, and again when its
    /// place changes. A widget that makes something for the size it is
    /// drawn at, such as a cover made small
    /// (`Thumbnail`, drawn in every band without
    /// reading the picture again), makes it here, and anew in
    /// [`update`](Self::update) when what it shows changed. Most widgets
    /// need nothing of it.
    fn place(&mut self, env: &Env<'_>, area: Rect) {
        let _ = (env, area);
    }

    /// Draws it into `area` (cleared before), from what it read.
    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect);

    /// The places in it that do something, drawn in `area`, in reading
    /// order; without them it only shows something.
    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let _ = (env, area);
        Vec::new()
    }

    /// Where it is shown, told once it is placed (after
    /// [`place`](Self::place)): its placement (a to-do list's `@2`, to
    /// tell its own taps from another's), the screen showing it, how
    /// large it is drawn.
    fn context(&mut self, env: &Env<'_>, context: &WidgetContext<'_>) {
        let _ = (env, context);
    }

    /// Input on it beside taps on its targets: a swipe that started on it
    /// (a list turning its page), the buttons' focus coming to one of its
    /// targets or leaving it. `true` if it took it, the commands it asks
    /// for in `out`: the screen draws it again; after a swipe it took, it
    /// also asks its [`targets`](Self::targets) anew (its page turned).
    /// Widgets that take none keep the default.
    ///
    /// The buttons send swipes too: the page buttons, while one of its
    /// targets has the focus (the next page as a swipe to the left). A
    /// widget refuses a swipe it cannot do (at its last page): the screen
    /// then turns its own page. Left and Right always move the focus on,
    /// out of the widget after its last target, so a widget that pages
    /// without end never keeps it. Where its targets are the only ones,
    /// the page buttons are the screen's and Left and Right are the
    /// swipes, at its first and last target.
    fn on_input(
        &mut self,
        env: &Env<'_>,
        input: WidgetInput,
        area: Rect,
        out: &mut crate::Commands,
    ) -> bool {
        let _ = (env, input, area, out);
        false
    }

    /// Draws the focus on its target `target` its own way; `false` (the
    /// default) for the screen's frame round it.
    fn draw_focus(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, target: Rect) -> bool {
        let _ = (canvas, env, target);
        false
    }

    /// A call that comes after this release, by its key and parameters:
    /// what PaperPaw asks of widgets later reaches widgets built now,
    /// which answer [`Handled::No`](crate::Handled::No) to keys they do
    /// not know.
    fn on_extended(&mut self, key: &Id, params: &Params, env: &Env<'_>) -> crate::Handled {
        let _ = (key, params, env);
        crate::Handled::No
    }
}

/// Where a widget is shown ([`Widget::context`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct WidgetContext<'a> {
    /// Its placement, e.g. `notes.todo@2`, where it is one; `None` for
    /// the widget itself.
    pub placement: Option<&'a str>,
    /// The screen showing it, where the screen says.
    pub host: Option<&'a str>,
    /// How large it is drawn.
    pub size: SizeClass,
}

/// Input a widget may take beside taps ([`Widget::on_input`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetInput {
    /// A swipe that started on it, this way; also a page button while one
    /// of its targets has the focus (see [`Widget::on_input`]).
    Swipe(crate::gesture::Direction),
    /// The buttons' focus came to its target in this area, or (`None`)
    /// left it.
    Focus(Option<Rect>),
}

/// Makes a widget for a screen that shows it.
pub(crate) type WidgetFactory = fn(&Env<'_>) -> Box<dyn Widget>;

/// How a widget ([`WidgetSpec`], `FixedWidget`) is made: by a function of
/// its own, or as a [`Drawn`] widget from what it draws with.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum Make {
    Factory(WidgetFactory),
    Drawn(Drawn),
}

impl fmt::Debug for Make {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Make::Factory(_) => "Factory",
            Make::Drawn(_) => "Drawn",
        })
    }
}

/// A place in a widget that does something: a tap on it, or confirm
/// once buttons moved the focus there, runs `command` (a tap alone where
/// it is [`touch`](Self::touch)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WidgetTarget {
    pub area: Rect,
    pub command: crate::Command,
    /// Tapped only: the buttons' focus does not stop there (what it does
    /// is reached otherwise, e.g. by an action bound to a button).
    pub touch: bool,
    /// What a long press on it runs, if anything (else a long press does
    /// what it does on the screen, e.g. editing the page).
    pub hold: Option<crate::Command>,
}

/// A widget contribution: its id, its name and how it is made.
#[non_exhaustive]
pub struct WidgetSpec {
    pub id: Id,
    /// Translation key of its name, e.g. "Clock".
    pub title: Cow<'static, str>,
    /// How it is made.
    pub make: Make,
    /// Which kind it is, one of [`categories`] (or a translation key of
    /// a plugin's own).
    pub category: Cow<'static, str>,
    /// The group of settings it previews, if it is made for that (see
    /// [`WidgetSpec::previews`]).
    pub previews: Option<Id>,
    /// The cells it takes in a grid ([`WidgetSpec::cells`]); without it,
    /// the grid's width and the rows it needs.
    pub cells: Option<CellSpan>,
    /// Reserved: zero now. A later release gives its bits a meaning, each
    /// with a setter and a getter of its own (never by reading a set bit
    /// another way), so specs can grow without a new plugin interface.
    #[allow(dead_code, reason = "reserved for later releases")]
    spare: u32,
}

impl WidgetSpec {
    /// A widget of a type of its own ([`Widget`]), made by `factory`.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, factory: WidgetFactory) -> Self {
        Self::made(id, title, Make::Factory(factory))
    }

    fn made(id: Id, title: impl Into<Cow<'static, str>>, make: Make) -> Self {
        Self {
            id,
            title: title.into(),
            make,
            category: Cow::Borrowed(categories::OTHER),
            previews: None,
            cells: None,
            spare: 0,
        }
    }

    /// Takes `cells` in a grid: added, that many; resized, no fewer than
    /// its least.
    pub fn cells(self, cells: CellSpan) -> Self {
        Self {
            cells: Some(cells),
            ..self
        }
    }

    /// Of the kind `category`, one of [`categories`].
    pub fn category(self, category: impl Into<Cow<'static, str>>) -> Self {
        Self {
            category: category.into(),
            ..self
        }
    }

    /// A widget with nothing of its own to read: it draws from the
    /// settings, the clock and the like (see [`Drawn`]).
    pub fn drawn(id: Id, title: impl Into<Cow<'static, str>>, drawn: Drawn) -> Self {
        Self::made(id, title, Make::Drawn(drawn))
    }
}

impl fmt::Debug for WidgetSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WidgetSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("category", &self.category)
            .field("previews", &self.previews)
            .finish_non_exhaustive()
    }
}

/// A widget as the store holds it: made at the start, or one of a list
/// kept in the program.
#[derive(Clone, Copy)]
enum Found<'a> {
    Spec(&'a WidgetSpec),
}

impl<'a> Found<'a> {
    fn id(self) -> &'a Id {
        match self {
            Found::Spec(w) => &w.id,
        }
    }

    fn title(self) -> &'a str {
        match self {
            Found::Spec(w) => &w.title,
        }
    }

    fn category(self) -> &'a str {
        match self {
            Found::Spec(w) => &w.category,
        }
    }

    fn previews(self) -> Option<&'a Id> {
        match self {
            Found::Spec(w) => w.previews.as_ref(),
        }
    }

    fn cells(self) -> Option<CellSpan> {
        match self {
            Found::Spec(w) => w.cells,
        }
    }
}

/// What the store keeps of each contribution, in the order they came: a
/// list kept in the program in two words.
#[derive(Debug)]
enum Entry {
    One(Box<WidgetSpec>),
}

/// A widget that only draws, as functions ([`WidgetSpec::drawn`]): for
/// widgets with nothing to read from the storage, such as a clock.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct Drawn {
    /// The height it needs for a width.
    pub height: fn(&Env<'_>, u16) -> u16,
    /// Draws it into its area.
    pub draw: fn(&mut Canvas<'_>, &Env<'_>, Rect),
    /// What it shows now, as text (e.g. the time): when that changes, it
    /// is drawn again. Without it, it is drawn when its screen is.
    pub shows: Option<fn(&Env<'_>) -> String>,
    /// The places in it that do something, drawn in an area (see
    /// [`Widget::targets`]).
    pub targets: Option<fn(&Env<'_>, Rect) -> Vec<WidgetTarget>>,
}

impl Drawn {
    /// A widget `height` high, drawn by `draw`.
    pub const fn new(
        height: fn(&Env<'_>, u16) -> u16,
        draw: fn(&mut Canvas<'_>, &Env<'_>, Rect),
    ) -> Self {
        Self {
            height,
            draw,
            shows: None,
            targets: None,
        }
    }

    /// The places in it that do something, drawn in an area.
    pub const fn targets(self, targets: fn(&Env<'_>, Rect) -> Vec<WidgetTarget>) -> Self {
        Self {
            targets: Some(targets),
            ..self
        }
    }

    /// Drawn again when what `shows` says changes.
    pub const fn shows(self, shows: fn(&Env<'_>) -> String) -> Self {
        Self {
            shows: Some(shows),
            ..self
        }
    }
}

impl fmt::Debug for Drawn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Drawn").finish_non_exhaustive()
    }
}

/// How many cells of a grid a widget takes ([`WidgetSpec::cells`]):
/// added, it gets `w` by `h` (no more than the grid has), and resized
/// never less than `min_w` by `min_h`; a `square` one stays as wide as
/// it is high, in cells (a round clock, an icon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CellSpan {
    pub w: u8,
    pub h: u8,
    pub min_w: u8,
    pub min_h: u8,
    pub square: bool,
}

impl CellSpan {
    /// `w` by `h` cells, at least one by one.
    pub const fn new(w: u8, h: u8) -> Self {
        Self {
            w,
            h,
            min_w: 1,
            min_h: 1,
            square: false,
        }
    }

    /// Never less than `w` by `h` cells.
    pub const fn min(self, w: u8, h: u8) -> Self {
        Self {
            min_w: w,
            min_h: h,
            ..self
        }
    }

    /// As wide as it is high, in cells.
    pub const fn square(self) -> Self {
        Self {
            square: true,
            ..self
        }
    }

    /// `cells` as the hint keeps them: no less than its least, square
    /// where it is (the side `wide` changed, the other following it).
    pub fn keep(&self, cells: grid::Cells, wide: bool) -> grid::Cells {
        let mut w = cells.w.max(self.min_w).max(1);
        let mut h = cells.h.max(self.min_h).max(1);
        if self.square {
            if wide {
                h = w;
            } else {
                w = h;
            }
        }
        grid::Cells { w, h, ..cells }
    }
}

/// How large a widget is drawn, for widgets that show more where they
/// have the room (a day's forecast small, the week's large): by the
/// shorter side of its area, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SizeClass {
    /// Less than 20 mm on its shorter side: a figure, an icon.
    Small,
    /// Up to 45 mm.
    Medium,
    /// More.
    Large,
}

/// How a place holds its widgets ([`WidgetPlace::kind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaceKind {
    /// Above each other (`WidgetStack`): a setting of widget ids separated by
    /// commas, [`FILL`] and [`SPACE`] among them.
    Stack,
    /// In a grid of cells ([`grid`]): settings of items with their cells,
    /// one a page.
    Grid,
}

/// A place where widgets show: the settings that list them, and how it
/// holds them. Screens of any plugin register theirs, so that plugins
/// can ask whether their widget shows anywhere ([`Widgets::shown`]) and
/// editors know them.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WidgetPlace {
    pub id: Id,
    /// Translation key of its name, e.g. "Sleep screen".
    pub title: Cow<'static, str>,
    /// The settings that list its widgets (a grid's pages each one).
    pub settings: Cow<'static, [Id]>,
    pub kind: PlaceKind,
    /// It shows only while this setting has this value (a mode of a
    /// screen), see [`WidgetPlace::shown_if`].
    pub shown_if: Option<(Id, &'static str)>,
}

impl WidgetPlace {
    /// Whether it shows now (see [`WidgetPlace::shown_if`]).
    pub fn shows(&self, env: &Env<'_>) -> bool {
        self.shown_if
            .as_ref()
            .is_none_or(|(setting, value)| env.settings().text(setting.as_str()) == *value)
    }
}

/// The ids a setting listing widgets names: each item's first word, the
/// cells of a grid's items after it left out.
fn listed(text: &str) -> impl Iterator<Item = &str> {
    text.split(',')
        .filter_map(|item| item.split_whitespace().next())
}

/// The registered widgets.
#[derive(Debug, Default)]
pub struct Widgets {
    widgets: Vec<Entry>,
    places: Vec<WidgetPlace>,
    categories: Vec<WidgetCategory>,
    /// The widgets declared to the kernel that are not registered here.
    elsewhere: elsewhere::Elsewhere,
}

impl Widgets {
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, widget: WidgetSpec) {
        self.widgets.push(Entry::One(Box::new(widget)));
    }

    /// The same, as it came in a contribution.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_boxed(&mut self, widget: Box<WidgetSpec>) {
        self.widgets.push(Entry::One(widget));
    }

    /// Every widget, in the order they came.
    fn all(&self) -> impl DoubleEndedIterator<Item = Found<'_>> {
        self.widgets.iter().map(|Entry::One(w)| Found::Spec(w))
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.widgets.shrink_to_fit();
        self.places.shrink_to_fit();
        self.categories.shrink_to_fit();
    }

    /// The icon of kind `kind`: the kernel's, a plugin's own, or a puzzle
    /// piece for a kind nobody added.
    pub fn category_icon(&self, kind: &str) -> &str {
        categories::icon(kind)
            .or_else(|| {
                self.categories
                    .iter()
                    .find(|c| c.id.as_str() == kind)
                    .map(|c| c.icon.as_ref())
            })
            .unwrap_or("puzzle")
    }

    /// Every place where widgets show.
    pub fn places(&self) -> impl Iterator<Item = &WidgetPlace> {
        self.places.iter()
    }

    /// Place `id`, if there is one.
    pub fn place(&self, id: &str) -> Option<&WidgetPlace> {
        self.places.iter().find(|p| p.id.as_str() == id)
    }

    /// Whether any place shows widget `id` now, or a placement of it:
    /// e.g. to fetch what it shows only then.
    pub fn shown(&self, env: &Env<'_>, id: &str) -> bool {
        self.places
            .iter()
            .filter(|place| place.shows(env))
            .any(|place| {
                place
                    .settings
                    .iter()
                    .flat_map(|setting| listed(env.settings().text(setting.as_str())))
                    .any(|listed| base(listed) == id)
            })
    }

    /// Widget `id`, also of a placement of it (`widgets.clock@2`).
    fn get(&self, id: &str) -> Option<Found<'_>> {
        let id = base(id);
        self.all().find(|w| w.id().as_str() == id)
    }

    /// Every widget for screens to show: its id and the translation key
    /// of its name (not those that preview settings).
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &str)> {
        self.all()
            .filter(|w| w.previews().is_none())
            .map(|w| (w.id(), w.title()))
    }

    /// The widget that previews the settings of `group`, if one does
    /// ([`WidgetSpec::previews`]); made with `Widgets::make`.
    pub fn preview(&self, group: &str) -> Option<&Id> {
        self.all()
            .rev()
            .find(|w| w.previews().is_some_and(|g| g.as_str() == group))
            .map(Found::id)
    }

    /// The kind of widget `id`, one of [`categories`] (or a plugin's own).
    pub fn category(&self, id: &str) -> Option<&str> {
        self.get(id).map(Found::category)
    }

    /// The cells widget `id` takes in a grid, where it says
    /// ([`WidgetSpec::cells`]).
    pub fn cells(&self, id: &str) -> Option<CellSpan> {
        self.get(id)?.cells()
    }

    /// The translation key of the name of widget `id`.
    pub fn title(&self, id: &str) -> Option<&str> {
        self.get(id).map(Found::title)
    }

    /// Whether widget `id` exists.
    pub fn has(&self, id: &str) -> bool {
        self.get(id).is_some()
    }
}

/// The widget a placement shows: `widgets.clock` for `widgets.clock@2`.
pub fn base(id: &str) -> &str {
    id.split_once('@').map_or(id, |(base, _)| base)
}




/// Where a widget's text sits in its place. [`Align::Start`] and
/// [`Align::End`] follow the interface's direction: at the left and the
/// right in left-to-right languages, the other way round in
/// right-to-left ones (`Align::resolved`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Align {
    Left,
    Center,
    Right,
    /// Where lines start: left, or right in a right-to-left interface.
    Start,
    /// Where lines end: right, or left in a right-to-left interface.
    End,
}

impl Align {

    /// Left, centre or right: [`Align::Start`] and [`Align::End`] for an
    /// interface right to left (`rtl`) or not.
    pub(crate) const fn resolved(self, rtl: bool) -> Align {
        match (self, rtl) {
            (Align::Start, false) | (Align::End, true) => Align::Left,
            (Align::Start, true) | (Align::End, false) => Align::Right,
            (other, _) => other,
        }
    }

    /// Where something `width` wide starts in `area`.
    pub(crate) fn x(self, area: Rect, width: f32) -> f32 {
        let left = f32::from(area.x);
        let room = (f32::from(area.width) - width).max(0.0);
        match self.resolved(false) {
            Align::Center => left + room / 2.0,
            Align::Right => left + room,
            _ => left,
        }
    }
}


#[cfg(test)]
mod tests;
