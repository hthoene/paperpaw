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
    /// Translates the words (the slot `core.translate`).
    Translate,
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
    /// The item an action of the selection (`reader.selection.*`, as the
    /// reader declares them: a key bound to one, a remote) asks for. Beta 3 ran
    /// them from its selection menu; here they do what the bar's button does,
    /// whether the bar is shown yet or not. *Highlight* is the one action for
    /// making and removing, as the button is one place. The action the bar has
    /// no button for (read aloud from here) is none.
    #[must_use]
    pub fn of_action(action: &str) -> Option<Self> {
        Some(match action.strip_prefix("reader.selection.")? {
            "look-up" => Self::LookUp,
            "translate" => Self::Translate,
            "highlight" => Self::Highlight,
            "note" => Self::Note,
            "copy" => Self::Copy,
            "extend" => Self::Extend,
            "cancel" => Self::Cancel,
            _ => return None,
        })
    }

    /// The key of its label.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::LookUp => "reader.select.look-up",
            Self::Translate => "reader.select.translate",
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

/// The items of a bar: up to six, in order.
#[derive(Clone, Copy, Debug)]
pub struct Items {
    items: [Item; MAX_ITEMS],
    len: usize,
}

impl Items {
    /// The items as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[Item] {
        &self.items[..self.len]
    }

    /// The place of the highlight button: the one the bar opens on, so Select
    /// highlights (#235).
    #[must_use]
    pub fn highlight_at(&self) -> usize {
        self.as_slice()
            .iter()
            .position(|i| matches!(i, Item::Highlight | Item::Unhighlight))
            .unwrap_or(0)
    }
}

/// The items of the bar, in order: a touch screen taps words to extend and has a
/// *Cancel*; buttons have *Extend* and Back cancels. `highlight` is how the
/// chosen words stand with the highlights: `None` where none overlaps (*Highlight*,
/// *Note*), `Some(false)` where one does (*Remove highlight*, *Note*) and
/// `Some(true)` where it has a note (*Remove highlight*, *Edit note*). *Look up*,
/// *Translate* and *Copy* are there only while the words are on one page
/// (`one_page`): the text of a longer stretch is not kept. *Translate* is there
/// only where a screen is offered for `core.translate` (`translate`).
#[must_use]
pub fn items(touch: bool, highlight: Option<bool>, one_page: bool, translate: bool) -> Items {
    let all = [
        Item::LookUp,
        Item::Translate,
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
    ];
    let mut kept = Items { items: all, len: 0 };
    for item in all {
        let text = matches!(item, Item::LookUp | Item::Translate | Item::Copy);
        if (one_page || !text) && (translate || item != Item::Translate) {
            kept.items[kept.len] = item;
            kept.len += 1;
        }
    }
    kept
}

/// Steps a style is made smaller in for the chips to fit one row.
const TIGHTER: usize = 5;

/// The style and fingertip the chips of `labels` are laid out with: `style`
/// where they fit one row `room` wide, else lower chips, closer together and
/// without a fingertip's height (the bar keeps it, and a tap in it is the
/// nearest chip's), step by step until they fit one row or are as low as
/// their text allows. The text keeps its size: it stays readable, and long
/// labels take a second row.
///
/// # Errors
/// The host's refusals of the font, `NoMemory`.
fn one_row(
    font: &Font<'_>,
    style: &Style,
    finger: u16,
    room: u16,
    labels: &[&str],
) -> Result<(Style, u16), Error> {
    let mut tight = style.clone();
    if Chips::height_for(font, &tight, finger, room, labels)? <= Chips::height(&tight, finger) {
        return Ok((tight, finger));
    }
    // The lowest chip that holds its text with room above and below it.
    let least = (style.small * 1.8) as u16;
    for _ in 0..TIGHTER {
        if Chips::height_for(font, &tight, 0, room, labels)? <= Chips::height(&tight, 0) {
            break;
        }
        let row = tight.row.saturating_mul(9) / 10;
        if row.saturating_mul(7) / 10 < least {
            break;
        }
        tight.row = row;
        tight.gap = (tight.gap.saturating_mul(3) / 4).max(2);
    }
    Ok((tight, 0))
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
        // All chips in one row where they fit (#448): smaller, the whole bar
        // is still a fingertip high and a tap in it is the nearest chip's.
        // Chips that do not fit even so go on the next row: the bar is as
        // high as its rows.
        let room = width.saturating_sub(2 * style.margin);
        let shown = &shown[..labels.len()];
        let (tight, tight_finger) = one_row(font, style, finger, room, shown)?;
        let rows = Chips::height_for(font, &tight, tight_finger, room, shown)?
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
        let chips = Chips::new(font, &tight, tight_finger, inner, shown)?;
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
        paperpaw_ui_kit::sheet::edge(paint, style, self.area, style.ink)?;
        // None is chosen: the focus frame says where the buttons are.
        self.chips.prepare(paint, font, usize::MAX, focus)
    }
}
