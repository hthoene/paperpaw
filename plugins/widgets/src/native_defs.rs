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
    /// It turns the quote on to the next.
    TurnQuote,
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

/// The behaviour of the widget declared with `token`.
#[must_use]
pub fn behaviour(token: u32) -> Option<&'static Behaviour> {
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
    match token {
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
