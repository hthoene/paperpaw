//! Skimming through the book (R6b), what the screen decides without a host:
//! where the book is (the page's label, the chapter and the thousandths read),
//! the chapter marks along the bar, the focus (the bar, or one of four buttons)
//! and what each key or tap asks the reader to do. Beta 3's `skim.rs` is the
//! reference: with buttons the page buttons go ten pages, left and right a
//! chapter, up and down move between the bar and the buttons below it, confirm
//! presses the button the focus is on (on the bar it stays where it is), and back
//! returns to where skimming began. A tap on the bar goes to that place of the
//! book.
//!
//! The model asks; it moves nothing: a [`Move`] is for the reading screen. Every
//! allocation is fallible.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::copy;

/// The buttons, by the keys of their words: chapter back, where one was,
/// chapter on, page number.
pub const BUTTONS: [&str; 4] = [
    "reader.skim.chapter-back",
    "reader.skim.return",
    "reader.skim.chapter-on",
    "reader.skim.page",
];
/// Pages a page button goes.
pub const PAGES: i32 = 10;
/// Chapter marks along the bar, at most.
pub const MAX_MARKS: usize = 512;
/// The whole bar, in thousandths.
pub const WHOLE: u32 = 1000;

/// The hand-over parameters of the screen, besides the book's `path`.
pub mod params {
    /// The page's label (`3 / 120`, or the percent where pages are not counted).
    pub const PAGE: &str = "page";
    /// The chapter's title.
    pub const CHAPTER: &str = "chapter";
    /// Thousandths of the book read.
    pub const PERMILLE: &str = "permille";
    /// The chapters' places on the bar, in thousandths, comma separated.
    pub const MARKS: &str = "marks";
}

/// What skimming asks the reading screen to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Move {
    /// By so many pages (negative: back).
    Pages(i32),
    /// By so many chapters (negative: back).
    Chapters(i32),
    /// To where skimming began.
    Back,
    /// To this place of the book, in thousandths.
    Permille(u32),
    /// To a page whose number is typed (the dialog of `reader.go-to-page`).
    Number,
}

/// What an input did to the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Not an input of the screen.
    Ignored,
    /// The focus moved (or began to show): the screen is drawn again.
    Redraw,
    /// The reading screen is asked to move, if a move is given; the screen
    /// closes where `close` is.
    Do {
        /// What to ask, if anything.
        go: Option<Move>,
        /// Whether the screen closes.
        close: bool,
    },
}

/// The focus of the screen: the bar, or a button by its index in [`BUTTONS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// The bar across the book.
    Bar,
    /// A button.
    Button(usize),
}

/// One screen of skimming.
#[derive(Clone, Debug)]
pub struct Skim {
    label: String,
    chapter: String,
    permille: u32,
    marks: Vec<u32>,
    focus: Focus,
    /// The focus shows: buttons were used, or the device has no touch screen.
    keys: bool,
}

/// The chapter marks in `text` (thousandths, comma separated; a part that is
/// not a number is left out; at most [`MAX_MARKS`]).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn parse_marks(text: &str) -> Result<Vec<u32>, Error> {
    let mut marks = Vec::new();
    for part in text.split(',') {
        let Ok(mark) = part.trim().parse::<u32>() else {
            continue;
        };
        if marks.len() >= MAX_MARKS {
            break;
        }
        marks.try_reserve(1).map_err(|_| Error::NoMemory)?;
        marks.push(mark.min(WHOLE));
    }
    Ok(marks)
}

/// The thousandths of a bar `width` wide that a touch `x` pixels from its left
/// end means (0 to 1000).
#[must_use]
pub fn permille_at(x: i32, width: u16) -> u32 {
    let x = u64::from(u32::try_from(x.max(0)).unwrap_or(0)).min(u64::from(width));
    u32::try_from(x * u64::from(WHOLE) / u64::from(width.max(1))).unwrap_or(WHOLE)
}

/// The width of the bar's read part, for a track `width` wide.
#[must_use]
pub fn done_width(width: u16, permille: u32) -> u16 {
    let done = u64::from(width) * u64::from(permille.min(WHOLE)) / u64::from(WHOLE);
    u16::try_from(done).unwrap_or(width)
}

impl Skim {
    /// A screen with no place told yet. `keys` is where the focus shows from the
    /// start (a device without a touch screen).
    #[must_use]
    pub const fn new(keys: bool) -> Self {
        Self {
            label: String::new(),
            chapter: String::new(),
            permille: 0,
            marks: Vec::new(),
            focus: Focus::Bar,
            keys,
        }
    }

    /// Takes over where the book is now.
    ///
    /// # Errors
    /// [`Error::NoMemory`] (what was shown stays).
    pub fn update(&mut self, label: &str, chapter: &str, permille: &str) -> Result<(), Error> {
        let (label, chapter) = (copy(label)?, copy(chapter)?);
        self.label = label;
        self.chapter = chapter;
        self.permille = permille.parse::<u32>().unwrap_or(0).min(WHOLE);
        Ok(())
    }

    /// Takes the chapter marks.
    pub fn set_marks(&mut self, marks: Vec<u32>) {
        self.marks = marks;
    }

    /// The page's label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The chapter's title (empty where unknown).
    #[must_use]
    pub fn chapter(&self) -> &str {
        &self.chapter
    }

    /// Thousandths of the book read.
    #[must_use]
    pub const fn permille(&self) -> u32 {
        self.permille
    }

    /// The chapters' places on the bar, in thousandths.
    #[must_use]
    pub fn marks(&self) -> &[u32] {
        &self.marks
    }

    /// The focus, and whether it shows.
    #[must_use]
    pub const fn focus(&self) -> (Focus, bool) {
        (self.focus, self.keys)
    }

    /// The button the focus is on, if one is.
    #[must_use]
    pub const fn button(&self) -> Option<usize> {
        match self.focus {
            Focus::Button(i) => Some(i),
            Focus::Bar => None,
        }
    }

    const fn ask(go: Move) -> Step {
        Step::Do {
            go: Some(go),
            close: false,
        }
    }

    const fn done(go: Option<Move>) -> Step {
        Step::Do { go, close: true }
    }

    /// What pressing button `index` asks.
    #[must_use]
    pub const fn press(index: usize) -> Step {
        match index {
            0 => Self::ask(Move::Chapters(-1)),
            1 => Self::done(Some(Move::Back)),
            2 => Self::ask(Move::Chapters(1)),
            // The reading screen asks for the number; this one makes room.
            _ => Self::done(Some(Move::Number)),
        }
    }

    /// A tap on button `index`: it is pressed, the focus stays off the buttons.
    pub const fn tap_button(&mut self, index: usize) -> Step {
        self.focus = Focus::Bar;
        self.keys = false;
        Self::press(index)
    }

    /// A tap on the bar `x` pixels from the left end of a track `width` wide.
    #[must_use]
    pub fn tap_bar(x: i32, width: u16) -> Step {
        Self::ask(Move::Permille(permille_at(x, width)))
    }

    /// A tap elsewhere: outside the screen is done skimming, here.
    #[must_use]
    pub const fn tap_outside() -> Step {
        Self::done(None)
    }

    /// What the action `action` (a key's, resolved by the host) does.
    pub fn action(&mut self, action: &str) -> Step {
        let direction = matches!(
            action,
            "core.previous" | "core.next" | "core.left" | "core.right" | "core.up" | "core.down"
        );
        if !self.keys && direction {
            self.keys = true;
        }
        match action {
            "core.prev-page" | "core.previous" => Self::ask(Move::Pages(-PAGES)),
            "core.next-page" | "core.next" => Self::ask(Move::Pages(PAGES)),
            "core.left" => Self::ask(Move::Chapters(-1)),
            "core.right" => Self::ask(Move::Chapters(1)),
            "core.up" | "core.down" => {
                self.focus = self.moved(action == "core.down");
                Step::Redraw
            }
            "core.confirm" => match self.button() {
                Some(i) => Self::press(i),
                None => Self::tap_outside(),
            },
            "core.back" => Self::done(Some(Move::Back)),
            "core.menu" => Self::tap_outside(),
            _ => Step::Ignored,
        }
    }

    /// The focus after up or down: the bar, then the buttons, then the bar.
    const fn moved(&self, down: bool) -> Focus {
        let last = BUTTONS.len() - 1;
        match (self.focus, down) {
            (Focus::Bar, true) => Focus::Button(0),
            (Focus::Bar, false) => Focus::Button(last),
            (Focus::Button(i), true) => {
                if i < last {
                    Focus::Button(i + 1)
                } else {
                    Focus::Bar
                }
            }
            (Focus::Button(0), false) => Focus::Bar,
            (Focus::Button(i), false) => Focus::Button(i - 1),
        }
    }
}

#[cfg(test)]
mod tests;
