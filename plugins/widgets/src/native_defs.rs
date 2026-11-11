//! What each widget of the pack does, by the token it was declared with.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{Error, Host},
};
use paperpaw_ui_kit::Paint;
use paperpaw_ui_kit::widget::Cx;

/// What a tap on a widget does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tap {
    /// Nothing.
    None,
    /// It opens the widget's settings: for widgets that show what the reader
    /// chose (a date, places).
    Settings,
    /// Its items tick off on a tap (a target for each); the heading, "more"
    /// and an empty list open the list's screen.
    TickItems,
    /// It turns the quote on to the next.
    TurnQuote,
    /// It turns the picture on to the next.
    TurnPicture,
    /// Each book on show is a target that opens it.
    Books,
    /// The book chosen (or being read) opens, or the library where there is none.
    Next,
    /// It opens the book of the highlight shown at its place.
    OpenHighlight,
    /// It runs the action with this id (the library's numbers open the
    /// library).
    Run(&'static str),
    /// It asks for the text of the setting `key` on the keyboard.
    Text {
        key: &'static str,
        title: &'static str,
    },
}

/// The behaviour of one widget.
#[derive(Debug, Clone, Copy)]
pub struct Behaviour {
    /// The natural height at a width; 0 shows nothing.
    pub height: fn(&mut Cx<'_>, u16) -> Result<u16, Error>,
    /// Lays the widget out in a rectangle.
    pub draw: fn(&mut Cx<'_>, &mut Vec<Paint>, Rect) -> Result<(), Error>,
    /// A number that changes when what it shows changes (0: nothing shown); it
    /// needs neither font nor look, so no drawing context is opened for it.
    pub shows: fn(&Host<'_>, &str) -> Result<u64, Error>,
    /// What a tap on it does.
    pub tap: Tap,
}

static DAY: Behaviour = Behaviour {
    height: crate::native_day::height,
    draw: crate::native_day::draw,
    shows: crate::native_day::shows,
    tap: Tap::None,
};
static TILES: Behaviour = Behaviour {
    height: crate::native_tiles::height,
    draw: crate::native_tiles::draw,
    shows: crate::native_tiles::shows,
    tap: Tap::None,
};
static CALENDAR: Behaviour = Behaviour {
    height: crate::native_calendar::height,
    draw: crate::native_calendar::draw,
    shows: crate::native_calendar::shows,
    tap: Tap::None,
};
static COUNTDOWN: Behaviour = Behaviour {
    height: crate::native_countdown::height,
    draw: crate::native_countdown::draw,
    shows: crate::native_countdown::shows,
    tap: Tap::Settings,
};
static YEAR: Behaviour = Behaviour {
    height: crate::native_year::height,
    draw: crate::native_year::draw,
    shows: crate::native_year::shows,
    tap: Tap::None,
};
static ANALOGUE: Behaviour = Behaviour {
    height: crate::native_analogue::height,
    draw: crate::native_analogue::draw,
    shows: crate::native_analogue::shows,
    tap: Tap::None,
};
static MOON: Behaviour = Behaviour {
    height: crate::native_moon::height,
    draw: crate::native_moon::draw,
    shows: crate::native_moon::shows,
    tap: Tap::None,
};
static WORLD: Behaviour = Behaviour {
    height: crate::native_world::height,
    draw: crate::native_world::draw,
    shows: crate::native_world::shows,
    tap: Tap::Settings,
};
static WORD_CLOCK: Behaviour = Behaviour {
    height: crate::native_word_clock::height,
    draw: crate::native_word_clock::draw,
    shows: crate::native_word_clock::shows,
    tap: Tap::None,
};
static GREETING: Behaviour = Behaviour {
    height: crate::native_greeting::height,
    draw: crate::native_greeting::draw,
    shows: crate::native_greeting::shows,
    tap: Tap::None,
};
static NOTE: Behaviour = Behaviour {
    height: crate::native_note::height,
    draw: crate::native_note::draw,
    shows: crate::native_note::shows,
    tap: Tap::Text {
        key: "text",
        title: "widgets.note.setting.text",
    },
};
static QUOTE: Behaviour = Behaviour {
    height: crate::native_quote::height,
    draw: crate::native_quote::draw,
    shows: crate::native_quote::shows,
    tap: Tap::TurnQuote,
};
static TODO: Behaviour = Behaviour {
    height: crate::native_todo::height,
    draw: crate::native_todo::draw,
    shows: crate::native_todo::shows,
    tap: Tap::TickItems,
};
static BATTERY: Behaviour = Behaviour {
    height: crate::native_battery::height,
    draw: crate::native_battery::draw,
    shows: crate::native_battery::shows,
    tap: Tap::None,
};

static LIBRARY: Behaviour = Behaviour {
    height: crate::native_numbers::height,
    draw: crate::native_numbers::draw,
    shows: crate::native_numbers::shows,
    tap: Tap::Run("core.library"),
};

static FAVOURITES: Behaviour = Behaviour {
    height: crate::native_favourites::height,
    draw: crate::native_favourites::draw,
    shows: crate::native_favourites::shows,
    tap: Tap::Books,
};
static SERIES: Behaviour = Behaviour {
    height: crate::native_series::height,
    draw: crate::native_series::draw,
    shows: crate::native_series::shows,
    tap: Tap::Books,
};

static NEXT: Behaviour = Behaviour {
    height: crate::native_next::height,
    draw: crate::native_next::draw,
    shows: crate::native_next::shows,
    tap: Tap::Next,
};

static PROGRESS: Behaviour = Behaviour {
    height: crate::native_progress::height,
    draw: crate::native_progress::draw,
    shows: crate::native_current::shows,
    tap: Tap::Next,
};

static READING: Behaviour = Behaviour {
    height: crate::native_reading::height,
    draw: crate::native_reading::draw,
    shows: crate::native_current::shows,
    tap: Tap::Next,
};

static HIGHLIGHT: Behaviour = Behaviour {
    height: crate::native_highlight::height,
    draw: crate::native_highlight::draw,
    shows: crate::native_highlight::shows,
    tap: Tap::OpenHighlight,
};

static LOGO: Behaviour = Behaviour {
    height: crate::native_logo::height,
    // The picture is drawn by the plugin over the paint (`native_plugin`).
    draw: |cx, out, area| crate::native_logo::draw_with(cx, out, area, None).map(|_| ()),
    shows: crate::native_logo::shows,
    tap: Tap::None,
};

static PICTURE: Behaviour = Behaviour {
    height: crate::native_picture::height,
    // The picture is drawn by the plugin over the paint (`native_plugin`).
    draw: |cx, out, area| {
        crate::native_picture::draw_with(cx, out, area, crate::native_picture::State::Loading)
            .map(|_| ())
    },
    shows: crate::native_picture::shows,
    tap: Tap::TurnPicture,
};

/// The behaviour of the widget declared with `token`.
#[must_use]
pub fn behaviour(token: u32) -> Option<&'static Behaviour> {
    match token {
        120 => Some(&READING),
        119 => Some(&PROGRESS),
        118 => Some(&NEXT),
        117 => Some(&SERIES),
        116 => Some(&FAVOURITES),
        115 => Some(&HIGHLIGHT),
        114 => Some(&LIBRARY),
        113 => Some(&BATTERY),
        112 => Some(&TODO),
        111 => Some(&QUOTE),
        110 => Some(&NOTE),
        109 => Some(&GREETING),
        108 => Some(&WORD_CLOCK),
        107 => Some(&WORLD),
        106 => Some(&MOON),
        105 => Some(&ANALOGUE),
        104 => Some(&YEAR),
        103 => Some(&COUNTDOWN),
        102 => Some(&CALENDAR),
        100 => Some(&DAY),
        101 => Some(&TILES),
        121 => Some(&LOGO),
        122 => Some(&PICTURE),
        _ => None,
    }
}

/// The token of the widget the host calls `widget` (`widgets.day`, or placed
/// `widgets.day@2`).
#[must_use]
pub fn token_of(widget: &str) -> Option<u32> {
    let base = paperpaw_ui_kit::widget::settings::base(widget);
    let name = base.strip_prefix("widgets.")?;
    crate::declarations::WIDGETS
        .iter()
        .find(|def| def.name == name)
        .map(|def| def.token)
}
