//! Widgets: small boxes plugins contribute for screens that show them,
//! such as the home screen's pages (a clock, the book being read, today's
//! reading, a grid of tools). Any plugin adds widgets with
//! `Staging::add_widget`; screens let
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
//! (`Staging::add_widget_place`):
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
//! (`Ask::PreviewSettings`). It is
//! not offered among the widgets for other screens.

mod colours;
pub mod grid;

pub use colours::{ColourSpec, PAPER_LIGHT_GRAY, PAPER_NONE, PAPER_NORMAL, colour_spec};

/// The kinds of widgets (`WidgetSpec::category`), each the translation
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

/// How many cells of a grid a widget takes (`WidgetSpec::cells`):
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
