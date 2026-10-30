//! Widgets: small boxes plugins contribute for screens that show them,
//! such as the home screen's pages (a clock, the book being read, today's
//! reading, a grid of tools). Any plugin adds widgets with
//! [`Registrar::add_widget`](crate::Registrar::add_widget); screens let
//! the user choose which to show and where.
//!
//! A screen that shows a widget makes it ([`Widgets::make`]) and keeps
//! it: the widget reads what it shows when the screen updates it (when
//! it shows, about each minute, after events), and draws from that, so
//! drawing stays quick ([`Widget`]). Widgets with nothing to read, such
//! as a clock, are a pair of functions ([`WidgetSpec::drawn`]).
//!
//! A widget may have targets: places that do something when tapped, which
//! buttons reach one after the other too (see [`WidgetTarget`]).
//!
//! A widget may have settings of its own, e.g. where its text sits
//! ([`align_setting`]): they go in a settings group with the widget's id
//! ([`settings_group`]). The settings list leaves such groups out; screens
//! that edit widgets open them (slot
//! [`SETTINGS`](crate::builtin::slots::SETTINGS) with parameter
//! [`ID`](crate::builtin::params::ID)).
//!
//! The kernel brings two widgets for room between the others, which a list
//! may hold more than once ([`repeatable`]): [`SPACE`], a gap of a set
//! height, and [`FILL`], the room the others leave. Screens that place
//! widgets one below the other give each fill its share of that room
//! ([`fill_height`]); elsewhere it is not there.
//!
//! Screens that show widgets in a grid, as the home screen does, use
//! [`grid`]: its layout as data, and a view that edits it in place.
//! Screens that show them above each other, as the sleep screen does, use
//! a [`WidgetStack`]. Either way the screen asks [`refresh`] what an event means
//! for the widgets it shows, and reads the live ones anew each minute
//! ([`next_minute`]).
//!
//! The places where widgets show are [`WidgetPlace`]s
//! ([`Registrar::add_widget_place`](crate::Registrar::add_widget_place)):
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
use crate::settings::{SettingGroupSpec, SettingKind, SettingSpec, SettingValue};

pub mod grid;
mod look;
mod stack;

pub use stack::WidgetStack;

pub use look::{
    ALL_FRAMES, FRAMES, Frame, OWN_FRAME, PADDINGS, Padding, SIZES, TextSize, WidgetLook, Wrapped,
    caption_height, choice_setting, draw_caption, draw_frame, draw_line, draw_message,
    draw_wrapped, fit_line, frame, frame_height, frame_setting, frame_spec, heading_line_height,
    ink_baseline, largest_size, line_height, padding, padding_setting, padding_spec, part_setting,
    part_spec, setting_id, shows, size, size_setting, size_spec, widget_look,
};

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
/// ([`Registrar::add_widget_category`](crate::Registrar::add_widget_category)),
/// beside [`categories`]: its id, the translation key of its name, and
/// its icon. Screens offering widgets list such kinds after the
/// kernel's.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WidgetCategory {
    pub id: Id,
    pub icon: Cow<'static, str>,
}

impl WidgetCategory {
    /// Kind `id` (its name's translation key), shown with `icon`.
    pub fn new(id: Id, icon: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            icon: icon.into(),
        }
    }
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
    /// ([`Thumbnail`](crate::ui::Thumbnail), drawn in every band without
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

impl<'a> WidgetContext<'a> {
    /// Drawn `size` large; the rest as set with its methods.
    pub fn new(size: SizeClass) -> Self {
        Self {
            placement: None,
            host: None,
            size,
        }
    }

    /// Shown by screen `host`.
    pub fn host(self, host: Option<&'a str>) -> Self {
        Self { host, ..self }
    }

    /// As placement `placement`.
    pub fn placement(self, placement: Option<&'a str>) -> Self {
        Self { placement, ..self }
    }
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
pub type WidgetFactory = Box<dyn Fn(&Env<'_>) -> Box<dyn Widget>>;

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

impl WidgetTarget {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(area: Rect, command: crate::Command) -> Self {
        Self {
            area,
            command,
            touch: false,
            hold: None,
        }
    }

    /// A long press on it runs `command`.
    pub fn hold(self, command: crate::Command) -> Self {
        Self {
            hold: Some(command),
            ..self
        }
    }

    /// A place that does something when tapped, which the buttons' focus
    /// leaves out.
    pub fn touch(area: Rect, command: crate::Command) -> Self {
        Self {
            touch: true,
            ..Self::new(area, command)
        }
    }
}

/// A widget contribution: its id, its name and how it is made.
#[non_exhaustive]
pub struct WidgetSpec {
    pub id: Id,
    /// Translation key of its name, e.g. "Clock".
    pub title: Cow<'static, str>,
    pub factory: WidgetFactory,
    /// Which kind it is, one of [`categories`] (or a translation key of
    /// a plugin's own).
    pub category: Cow<'static, str>,
    /// The group of settings it previews, if it is made for that (see
    /// [`WidgetSpec::previews`]).
    pub previews: Option<Id>,
    /// The cells it takes in a grid ([`WidgetSpec::cells`]); without it,
    /// the grid's width and the rows it needs.
    pub cells: Option<CellSpan>,
}

impl WidgetSpec {
    /// A widget of a type of its own ([`Widget`]), made by `factory`.
    pub fn new(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        factory: impl Fn(&Env<'_>) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            factory: Box::new(factory),
            category: Cow::Borrowed(categories::OTHER),
            previews: None,
            cells: None,
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

    /// Made to preview the settings of `group` (see the [module
    /// documentation](self)): what it shows follows their values, those
    /// shown while one is chosen included. Settings pages show it above
    /// the group's settings, as wide as the screen; it is not offered
    /// among the widgets for other screens. Of several for one group, the
    /// one added last is shown.
    pub fn previews(self, group: Id) -> Self {
        Self {
            previews: Some(group),
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
        Self::new(id, title, move |_| {
            Box::new(DrawnWidget { drawn, shown: None })
        })
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
    pub fn new(
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
    pub fn targets(self, targets: fn(&Env<'_>, Rect) -> Vec<WidgetTarget>) -> Self {
        Self {
            targets: Some(targets),
            ..self
        }
    }

    /// Drawn again when what `shows` says changes.
    pub fn shows(self, shows: fn(&Env<'_>) -> String) -> Self {
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

/// A [`Drawn`] widget on a screen.
struct DrawnWidget {
    drawn: Drawn,
    shown: Option<String>,
}

impl Widget for DrawnWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let Some(shows) = self.drawn.shows else {
            return false;
        };
        let now = Some(shows(env));
        let changed = self.shown.is_some() && now != self.shown;
        self.shown = now;
        changed
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        (self.drawn.height)(env, width)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        (self.drawn.draw)(canvas, env, area);
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        self.drawn
            .targets
            .map_or_else(Vec::new, |targets| targets(env, area))
    }
}

/// Says that what widget `id` shows changed (e.g. its own data, after a
/// tap on it): screens showing it read it anew
/// ([`WIDGET_CHANGED`](crate::builtin::events::WIDGET_CHANGED)).
pub fn changed(id: &Id) -> crate::Command {
    use crate::builtin::events;
    events::WIDGET_CHANGED.emit(events::WidgetChanged { id: id.clone() })
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

impl SizeClass {
    /// The class of `area` on this screen.
    pub fn of(env: &Env<'_>, area: Rect) -> Self {
        let short = u32::from(area.width.min(area.height));
        if short < env.metrics().mm(20) {
            Self::Small
        } else if short <= env.metrics().mm(45) {
            Self::Medium
        } else {
            Self::Large
        }
    }
}

/// How a place holds its widgets ([`WidgetPlace::kind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaceKind {
    /// Above each other ([`WidgetStack`]): a setting of widget ids separated by
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
    /// Made of what it needs; the rest as set with its methods.
    pub const fn new(
        id: Id,
        title: &'static str,
        settings: &'static [Id],
        kind: PlaceKind,
    ) -> Self {
        Self {
            id,
            title: Cow::Borrowed(title),
            settings: Cow::Borrowed(settings),
            kind,
            shown_if: None,
        }
    }

    /// It shows only while setting `setting` has the value `value`, e.g.
    /// the sleep screen's widgets only while it shows widgets: a plugin
    /// asking [`Widgets::shown`] is told so only then.
    pub const fn shown_if(mut self, setting: Id, value: &'static str) -> Self {
        // Nothing to drop: made without one (`new`), in a const.
        core::mem::forget(self.shown_if.replace((setting, value)));
        self
    }

    /// Whether it shows now (see [`WidgetPlace::shown_if`]).
    pub fn shows(&self, env: &Env<'_>) -> bool {
        self.shown_if
            .as_ref()
            .is_none_or(|(setting, value)| env.settings().text(setting.as_str()) == *value)
    }

    /// The widgets it shows now: their ids, a placement's (`home.clock@2`)
    /// as it is, in the order of its settings.
    pub fn widgets<'e>(&self, env: &Env<'e>) -> Vec<&'e str> {
        self.settings
            .iter()
            .flat_map(|setting| listed(env.settings().text(setting.as_str())))
            .filter(|id| env.widgets().has(id))
            .collect()
    }
}

/// The ids a setting listing widgets names: each item's first word, the
/// cells of a grid's items after it left out.
fn listed(text: &str) -> impl Iterator<Item = &str> {
    text.split(',')
        .filter_map(|item| item.split_whitespace().next())
}

/// What a screen showing widgets does after an event ([`refresh`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refresh {
    /// Nothing: the event is not about them.
    Nothing,
    /// Makes them anew: how they are made changed (a setting of the
    /// screen, of the look, or of a widget shown).
    Remake,
    /// Reads what they show anew (a setting, the library changed).
    Read,
    /// Reads what this one shows anew ([`changed`]).
    ReadOne(Id),
    /// Reads the live ones anew ([`Widget::live`]): the device's status
    /// changed (the battery).
    Live,
}

/// What a screen that shows the widgets `shown` (their ids, a
/// placement's too), with settings of its own under `own` (e.g. `home`),
/// does after `event` with `params`: makes them anew after a setting of
/// its own, of the look (`core.`) or of a widget shown changed, reads
/// them anew after any other setting or the library changed, reads the
/// one that says so ([`changed`]), and the live ones after the device's
/// status changed.
pub fn refresh(event: &Id, params: &Params, own: &str, shown: &[String]) -> Refresh {
    use crate::builtin::events;
    if let Some(changed) = events::SETTING_CHANGED.read(event, params) {
        let id = changed.id.as_str();
        let of = |prefix: &str| {
            id.strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
        };
        let remakes =
            of(own) || of("core") || shown.iter().any(|widget| of(widget) || of(base(widget)));
        return if remakes {
            Refresh::Remake
        } else {
            Refresh::Read
        };
    }
    if *event == events::LIBRARY_CHANGED || *event == events::BOOK_MOVED {
        return Refresh::Read;
    }
    if let Some(changed) = events::WIDGET_CHANGED.read(event, params) {
        return Refresh::ReadOne(changed.id);
    }
    if *event == events::STATUS_CHANGED {
        return Refresh::Live;
    }
    Refresh::Nothing
}

/// When live widgets are read anew next: at the next full minute, when a
/// clock shows another time (a minute from now without a clock).
pub fn next_minute(env: &Env<'_>) -> crate::Instant {
    crate::time::next_minute(env).unwrap_or(env.now().add_millis(60_000))
}

/// The registered widgets.
#[derive(Debug, Default)]
pub struct Widgets {
    widgets: Vec<WidgetSpec>,
    places: Vec<WidgetPlace>,
    categories: Vec<WidgetCategory>,
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
        self.widgets.push(widget);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.widgets.shrink_to_fit();
        self.places.shrink_to_fit();
        self.categories.shrink_to_fit();
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_category(&mut self, category: WidgetCategory) {
        self.categories.push(category);
    }

    /// Every kind of widgets, in the order screens show them: the
    /// kernel's ([`categories::ALL`]), then plugins' own in the order they
    /// were added.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        categories::ALL
            .iter()
            .copied()
            .chain(self.categories.iter().map(|c| c.id.as_str()))
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

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_place(&mut self, place: WidgetPlace) {
        self.places.push(place);
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
    fn get(&self, id: &str) -> Option<&WidgetSpec> {
        let id = base(id);
        self.widgets.iter().find(|w| w.id.as_str() == id)
    }

    /// Every widget for screens to show: its id and the translation key
    /// of its name (not those that preview settings).
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &str)> {
        self.widgets
            .iter()
            .filter(|w| w.previews.is_none())
            .map(|w| (&w.id, w.title.as_ref()))
    }

    /// The widget that previews the settings of `group`, if one does
    /// ([`WidgetSpec::previews`]); made with [`Widgets::make`].
    pub fn preview(&self, group: &str) -> Option<&Id> {
        self.widgets
            .iter()
            .rev()
            .find(|w| w.previews.as_ref().is_some_and(|g| g.as_str() == group))
            .map(|w| &w.id)
    }

    /// The kind of widget `id`, one of [`categories`] (or a plugin's own).
    pub fn category(&self, id: &str) -> Option<&str> {
        self.get(id).map(|w| w.category.as_ref())
    }

    /// The cells widget `id` takes in a grid, where it says
    /// ([`WidgetSpec::cells`]).
    pub fn cells(&self, id: &str) -> Option<CellSpan> {
        self.get(id)?.cells
    }

    /// The translation key of the name of widget `id`.
    pub fn title(&self, id: &str) -> Option<&str> {
        self.get(id).map(|w| w.title.as_ref())
    }

    /// Whether widget `id` exists.
    pub fn has(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    /// Widget `id` for a screen, what it shows read; `None` if there is
    /// none. Screens keep it while they show it and
    /// [`update`](Widget::update) it.
    ///
    /// For a placement of a widget (`widgets.clock@2`, see
    /// [`Id::placement`]), the widget reads, measures and draws itself in
    /// the placement's own values of its settings
    /// ([`Settings::with_placement`](crate::Settings::with_placement)).
    pub fn make(&self, id: &str, env: &Env<'_>) -> Option<Box<dyn Widget>> {
        let spec = self.get(id)?;
        let mut widget = if id.contains('@') {
            let inner = env
                .settings
                .with_placement(Some(id), || (spec.factory)(env));
            Box::new(Placed {
                placement: String::from(id),
                inner,
            })
        } else {
            (spec.factory)(env)
        };
        widget.update(env);
        Some(widget)
    }
}

/// The widget a placement shows: `widgets.clock` for `widgets.clock@2`.
pub fn base(id: &str) -> &str {
    id.split_once('@').map_or(id, |(base, _)| base)
}

/// A widget in a placement of its own, in the placement's values.
struct Placed {
    placement: String,
    inner: Box<dyn Widget>,
}

impl Placed {
    fn with<R>(&self, env: &Env<'_>, f: impl FnOnce() -> R) -> R {
        env.settings().with_placement(Some(&self.placement), f)
    }
}

impl Widget for Placed {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let placement = core::mem::take(&mut self.placement);
        let changed = env
            .settings
            .with_placement(Some(&placement), || self.inner.update(env));
        self.placement = placement;
        changed
    }

    fn live(&self) -> bool {
        self.inner.live()
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        self.with(env, || self.inner.height(env, width))
    }

    fn min_height(&self, env: &Env<'_>, width: u16) -> u16 {
        self.with(env, || self.inner.min_height(env, width))
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        let placement = core::mem::take(&mut self.placement);
        env.settings()
            .with_placement(Some(&placement), || self.inner.place(env, area));
        self.placement = placement;
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        self.with(env, || self.inner.draw(canvas, env, area));
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        self.with(env, || self.inner.targets(env, area))
    }

    fn context(&mut self, env: &Env<'_>, context: &WidgetContext<'_>) {
        let placement = core::mem::take(&mut self.placement);
        let own = context.placement(Some(&placement));
        env.settings()
            .with_placement(Some(&placement), || self.inner.context(env, &own));
        self.placement = placement;
    }

    fn on_input(
        &mut self,
        env: &Env<'_>,
        input: WidgetInput,
        area: Rect,
        out: &mut crate::Commands,
    ) -> bool {
        let placement = core::mem::take(&mut self.placement);
        let taken = env.settings().with_placement(Some(&placement), || {
            self.inner.on_input(env, input, area, out)
        });
        self.placement = placement;
        taken
    }

    fn draw_focus(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, target: Rect) -> bool {
        self.with(env, || self.inner.draw_focus(canvas, env, target))
    }
}

/// A gap of the height [`SPACE_SIZE`] says.
pub const SPACE: &str = "core.space";
/// The room the other widgets leave, shared among the fills: its own
/// height is 0, screens that place widgets give it the rest.
pub const FILL: &str = "core.fill";
/// The height of [`SPACE`]: one of [`SPACE_SIZES`].
pub const SPACE_SIZE: crate::setting::Setting<crate::setting::Text> =
    crate::setting::Setting::new("core.space.size");
/// The values of [`SPACE_SIZE`]: small, medium, large.
pub const SPACE_SIZES: &[&str] = &["core.space.small", "core.space.medium", "core.space.large"];

/// A placement of widget `id` (`id@n`, see [`Id::placement`]) that
/// `used` does not name and that has no values set, for a widget added
/// again with settings of its own; `id` itself where the first 256 all
/// are.
pub fn free_placement(env: &Env<'_>, id: &str, used: impl Fn(&str) -> bool) -> String {
    let Ok(plain) = Id::parse(base(id)) else {
        return String::from(id);
    };
    (1..=256)
        .map(|n| plain.placement(n))
        .find(|placed| {
            !used(placed.as_str()) && env.settings().placement_values(placed.as_str()).is_empty()
        })
        .map_or_else(|| String::from(id), |placed| String::from(placed.as_str()))
}

/// Whether a list of widgets may hold `id` more than once.
pub fn repeatable(id: &str) -> bool {
    id == SPACE || id == FILL
}

/// The height of each of `fills` fills where widgets `used` pixels high
/// are placed in `room` pixels.
pub fn fill_height(room: u32, used: u32, fills: usize) -> u16 {
    if fills == 0 {
        return 0;
    }
    (room.saturating_sub(used) / fills as u32).min(u32::from(u16::MAX)) as u16
}

/// The values of a widget's alignment setting ([`align_setting`]).
pub const ALIGNMENTS: &[&str] = &["core.align.left", "core.align.center", "core.align.right"];

/// Where a widget's text sits in its place. [`Align::Start`] and
/// [`Align::End`] follow the interface's direction: at the left and the
/// right in left-to-right languages, the other way round in
/// right-to-left ones ([`Align::resolved`]).
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
    /// The alignment a value of [`ALIGNMENTS`] names; left for others.
    pub fn of(value: &str) -> Self {
        match ALIGNMENTS.iter().position(|a| *a == value) {
            Some(1) => Align::Center,
            Some(2) => Align::Right,
            _ => Align::Left,
        }
    }

    /// Its value in [`ALIGNMENTS`]; [`Align::Start`] and [`Align::End`]
    /// as left and right.
    pub const fn value(self) -> &'static str {
        match self {
            Align::Left | Align::Start => ALIGNMENTS[0],
            Align::Center => ALIGNMENTS[1],
            Align::Right | Align::End => ALIGNMENTS[2],
        }
    }

    /// Left, centre or right: [`Align::Start`] and [`Align::End`] for an
    /// interface right to left (`rtl`) or not.
    pub const fn resolved(self, rtl: bool) -> Align {
        match (self, rtl) {
            (Align::Start, false) | (Align::End, true) => Align::Left,
            (Align::Start, true) | (Align::End, false) => Align::Right,
            (other, _) => other,
        }
    }

    /// Where something `width` wide starts in `area`.
    pub fn x(self, area: Rect, width: f32) -> f32 {
        let left = f32::from(area.x);
        let room = (f32::from(area.width) - width).max(0.0);
        match self.resolved(false) {
            Align::Center => left + room / 2.0,
            Align::Right => left + room,
            _ => left,
        }
    }
}

/// The settings group of `widget`'s own settings, named as the widget.
pub fn settings_group(widget: &Id, title: impl Into<Cow<'static, str>>) -> SettingGroupSpec {
    SettingGroupSpec {
        id: widget.clone(),
        title: title.into(),
        order: 0,
        page: None,
        parent: None,
    }
}

/// The setting of where `widget`'s text sits, `<widget>.align`, in the
/// widget's settings group; read it with [`align`].
pub fn align_setting(widget: &Id, default: Align) -> SettingSpec {
    let id = Id::parse(&alloc::format!("{widget}.align")).unwrap_or_else(|_| widget.clone());
    SettingSpec::new(
        id,
        "core.setting.align",
        SettingKind::Choice(crate::settings::Choices::Fixed(ALIGNMENTS)),
        SettingValue::Text(Cow::Borrowed(default.value())),
        widget.clone(),
    )
    .description("core.setting.align.help")
}

/// [`align_setting`] kept in the program
/// (a `static` list), for a `static`
/// list of the widget's settings: `id` must be `<widget>.align`.
pub const fn align_spec(id: &'static str, widget: Id, default: Align) -> SettingSpec {
    SettingSpec::choice(
        id,
        "core.setting.align",
        ALIGNMENTS,
        default.value(),
        widget,
    )
    .description("core.setting.align.help")
}

/// Whether widget `id` has settings of its own (a settings group with its
/// id holding settings, see [`settings_group`]).
pub fn has_settings(env: &Env<'_>, id: &str) -> bool {
    let id = base(id);
    env.settings().groups().any(|group| group.id.as_str() == id)
        && env
            .settings
            .specs()
            .any(|spec| env.settings().group_of(spec).as_str() == id)
}

/// Where `widget`'s text sits, by its [`align_setting`]; left without one.
pub fn align(env: &Env<'_>, widget: &str) -> Align {
    Align::of(env.settings().text(&alloc::format!("{widget}.align")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignments_place_text_in_the_area() {
        let area = Rect::new(10, 0, 100, 20);
        assert_eq!(Align::Left.x(area, 40.0), 10.0);
        assert_eq!(Align::Center.x(area, 40.0), 40.0);
        assert_eq!(Align::Right.x(area, 40.0), 70.0);
        // Too wide: from the left edge.
        assert_eq!(Align::Right.x(area, 140.0), 10.0);
    }

    #[test]
    fn alignments_round_trip() {
        for align in [Align::Left, Align::Center, Align::Right] {
            assert_eq!(Align::of(align.value()), align);
        }
        assert_eq!(Align::of("sideways"), Align::Left);
    }

    #[test]
    fn fills_share_the_room_left() {
        assert_eq!(fill_height(1000, 400, 2), 300);
        assert_eq!(fill_height(1000, 1200, 1), 0);
        assert_eq!(fill_height(1000, 400, 0), 0);
        assert!(repeatable(FILL) && repeatable(SPACE) && !repeatable("home.clock"));
    }

    #[test]
    fn previews_go_with_their_group_only() {
        fn nothing(_: &Env<'_>, _: u16) -> u16 {
            0
        }
        fn draw(_: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}
        let drawn = Drawn::new(nothing, draw);
        let reading = Id::new_static("core.group.reading");
        let mut widgets = Widgets::new();
        widgets.add(WidgetSpec::drawn(
            Id::new_static("a.clock"),
            "a.clock",
            drawn,
        ));
        widgets.add(
            WidgetSpec::drawn(Id::new_static("a.sample"), "a.sample", drawn)
                .previews(reading.clone()),
        );
        widgets.add(
            WidgetSpec::drawn(Id::new_static("b.sample"), "b.sample", drawn).previews(reading),
        );
        // Not offered for screens; the one added last previews.
        let offered: Vec<&str> = widgets.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(offered, ["a.clock"]);
        assert_eq!(
            widgets.preview("core.group.reading").map(Id::as_str),
            Some("b.sample")
        );
        assert_eq!(widgets.preview("core.group.display"), None);
        assert!(widgets.has("a.sample"));
    }

    #[test]
    fn the_setting_belongs_to_the_widget() {
        let widget = Id::new_static("home.clock");
        let spec = align_setting(&widget, Align::Center);
        assert_eq!(spec.id.as_str(), "home.clock.align");
        assert_eq!(spec.group, widget);
        assert_eq!(
            spec.default,
            SettingValue::Text(Cow::Borrowed("core.align.center"))
        );
    }
}
