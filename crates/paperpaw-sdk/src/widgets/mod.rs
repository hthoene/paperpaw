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

    /// Whether what it shows changes by itself (the time, the battery):
    /// it is read anew each minute and when the device's status changes.
    /// Widgets that show what changes only after events (books, settings)
    /// say `false`, and are read when shown and after those events only:
    /// reading them each minute would cost time and battery for nothing.
    fn live(&self) -> bool {
        true
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


/// The ids a setting listing widgets names: each item's first word, the
/// cells of a grid's items after it left out.
fn listed(text: &str) -> impl Iterator<Item = &str> {
    text.split(',')
        .filter_map(|item| item.split_whitespace().next())
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
