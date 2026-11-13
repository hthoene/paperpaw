//! The bar of the chosen text: what can be done with it, as buttons over the
//! bottom of the page. The items are fixed (look up, highlight, note, copy, then
//! *Extend* on a device with buttons, *Cancel* on a touch screen); the interface-6 bar was the
//! menu `reader.selection`, which a plugin could add items to (a named gap in
//! `PORTING-7.md`: the stage that passes the chosen text on is not defined).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{Error, Host, fonts::Font, snapshot::Snapshot},
};
use paperpaw_ui_kit::{Chips, Messages, Style, paint::Paint};

/// Most items a bar holds.
pub const MAX_ITEMS: usize = 6;

/// What a button of the bar does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Item {
    /// Looks the words up (the slot `core.lookup`).
    LookUp,
    /// Highlights the words.
    Highlight,
    /// Takes the highlight of the words away (they have one).
    Unhighlight,
    /// Writes a note on the words (making the highlight they lack).
    Note,
    /// Edits the note of the highlight the words are in.
    EditNote,
    /// Copies the words to the clipboard.
    Copy,
    /// Moves the end of the selection with the buttons.
    Extend,
    /// Lets go of the selection.
    Cancel,
}

impl Item {
    /// The key of its label.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::LookUp => "reader.select.look-up",
            Self::Highlight => "reader.select.highlight",
            Self::Unhighlight => "reader.select.unhighlight",
            Self::Note => "reader.select.note",
            Self::EditNote => "reader.select.edit-note",
            Self::Copy => "reader.select.copy",
            Self::Extend => "reader.select.extend",
            Self::Cancel => "reader.select.cancel",
        }
    }
}

/// The items of the bar, in order: a touch screen taps words to extend and has a
/// *Cancel*; buttons have *Extend* and Back cancels. `highlight` is how the
/// chosen words stand with the highlights: `None` where none overlaps (*Highlight*,
/// *Note*), `Some(false)` where one does (*Remove highlight*, *Note*) and
/// `Some(true)` where it has a note (*Remove highlight*, *Edit note*).
#[must_use]
pub fn items(touch: bool, highlight: Option<bool>) -> [Item; 5] {
    [
        Item::LookUp,
        if highlight.is_some() {
            Item::Unhighlight
        } else {
            Item::Highlight
        },
        if highlight == Some(true) {
            Item::EditNote
        } else {
            Item::Note
        },
        Item::Copy,
        if touch { Item::Cancel } else { Item::Extend },
    ]
}

/// The bar laid out on a display: the chips and where the bar is.
#[derive(Debug)]
pub struct Bar {
    chips: Chips,
    items: Vec<Item>,
    area: Rect,
}

impl Bar {
    /// The bar of `items` at the bottom of the display, as wide as the display.
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusals of the font.
    pub fn new(
        host: &Host<'_>,
        font: &Font<'_>,
        style: &Style,
        display: &Snapshot,
        items: &[Item],
    ) -> Result<Self, Error> {
        let finger = Chips::finger(display);
        let (width, tall) = (display.display_width(), display.display_height());
        if items.len() > MAX_ITEMS {
            return Err(Error::NoSpace);
        }
        let mut messages = Messages::new()?;
        let mut labels: Vec<String> = Vec::new();
        labels
            .try_reserve_exact(items.len())
            .map_err(|_| Error::NoMemory)?;
        for item in items {
            labels.push(messages.copy(host, item.key())?);
        }
        let mut shown = [""; MAX_ITEMS];
        for (slot, label) in shown.iter_mut().zip(&labels) {
            *slot = label.as_str();
        }
        // Chips that do not fit one row go on the next: the bar is as high as
        // its rows.
        let rows = Chips::height_for(
            font,
            style,
            finger,
            width.saturating_sub(2 * style.margin),
            &shown[..labels.len()],
        )?
        .max(Chips::height(style, finger));
        let height = rows.saturating_add(2 * style.margin);
        let top = tall.saturating_sub(height);
        let area = Rect::new(0, top, width, height);
        let inner = Rect::new(
            style.margin,
            top.saturating_add(style.margin),
            width.saturating_sub(2 * style.margin),
            height.saturating_sub(2 * style.margin),
        );
        let chips = Chips::new(font, style, finger, inner, &shown[..labels.len()])?;
        let mut kept = Vec::new();
        kept.try_reserve_exact(items.len())
            .map_err(|_| Error::NoMemory)?;
        kept.extend_from_slice(items);
        Ok(Self {
            chips,
            items: kept,
            area,
        })
    }

    /// Where the bar is.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.area
    }

    /// The item a touch at `point` is on, if the bar is there: a touch beside
    /// the chips, inside the bar, is the nearest one's.
    #[must_use]
    pub fn item_at(&self, point: Point) -> Option<Item> {
        if !self.area.contains(point) {
            return None;
        }
        let index = self.chips.hit(point).or_else(|| {
            (0..self.chips.len()).min_by_key(|&i| {
                self.chips
                    .cell(i)
                    .map_or(i64::MAX, |c| {
                        i64::from(c.x) + i64::from(c.width) / 2 - i64::from(point.x)
                    })
                    .abs()
            })
        })?;
        self.items.get(index).copied()
    }

    /// The item before or after `focus` for a navigation action (`None` for any
    /// other), staying within the bar.
    #[must_use]
    pub fn step(&self, focus: usize, action: &str) -> Option<usize> {
        self.chips.step(focus, action)
    }

    /// The item at `index`.
    #[must_use]
    pub fn item(&self, index: usize) -> Option<Item> {
        self.items.get(index).copied()
    }

    /// Draws the bar: a white strip with a line over it, the chips, the focus
    /// on `focus`.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals of the font.
    pub fn prepare(
        &self,
        paint: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        focus: Option<usize>,
    ) -> Result<(), Error> {
        paperpaw_ui_kit::paint::push(paint, Paint::Fill(self.area, style.paper))?;
        paperpaw_ui_kit::paint::push(
            paint,
            Paint::Fill(
                Rect::new(self.area.x, self.area.y, self.area.width, style.line.max(2)),
                style.ink,
            ),
        )?;
        // None is chosen: the focus frame says where the buttons are.
        self.chips.prepare(paint, font, usize::MAX, focus)
    }
}
