//! What the keyboard does without the host: the text being typed, capitals,
//! the rows of keys and where they lie, and where the focus goes. The native
//! plugin reads the screen's numbers into a [`Look`] and the question's flags
//! into [`Asked`]; everything here is plain data and allocates fallibly.
use crate::hangul;
use crate::keys::{Key, Layer, Slot, button_scale, height_of, size_of, try_rows_for, units};
use crate::{SHAPE_ROUND, SHAPE_TALL, SHAPE_WIDE, SIZE_LARGE, SIZE_MEDIUM, SIZE_SMALL};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Point, Rect, native::Error};
use paperpaw_ui_kit::mem::wipe_text;

/// Longest text the keyboard takes, in characters: room for long web
/// addresses and keys; a request takes at most 24,576 bytes (8,192 secret).
pub const MAX_CHARS: usize = 2000;
/// How much of several lines (a message) may be typed.
pub const MAX_CHARS_MULTILINE: usize = 6000;
/// How many lines more than one the field of several lines grows by, at most.
const MORE_LINES: u16 = 10;
/// Two presses of *Shift* this close, in milliseconds, lock capitals.
pub const DOUBLE_MS: u64 = 800;
/// The focus "row" of the buttons in the text field.
pub const FIELD_ROW: usize = usize::MAX;
/// The least width of a letter's key where keys are tapped, in millimetres.
const LEAST_TAP_MM: u32 = 4;
/// Half the room between tapped keys, in tenths of a millimetre, twice.
const TOUCH_GAP_TENTHS: u32 = 6;
/// Half the room between keys chosen with buttons, in pixels.
const BUTTONS_SEAM: u16 = 1;
/// The least room between the screen's edge and the keys' visible edges.
const LEAST_SIDE: u16 = 4;

/// What *Shift* does to the next letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Off,
    /// The next letter is a capital.
    Once,
    /// Every letter is, until *Shift* is pressed again.
    Locked,
}

/// A button in the text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldButton {
    /// Type the text on another device.
    Phone,
    /// Insert the clipboard's text.
    Paste,
}

impl FieldButton {
    /// The catalog key of its label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Phone => "keyboard.phone",
            Self::Paste => "keyboard.paste",
        }
    }
}

/// How the question wants the text typed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Asked {
    /// Shown as dots, never on the clipboard or the phone.
    pub secret: bool,
    /// Several lines, a key for the line break.
    pub multiline: bool,
    /// Digits only.
    pub number: bool,
    /// Capitals may start sentences (plain text, several lines).
    pub plain: bool,
    /// The asker hears the text while it is typed.
    pub live: bool,
}

/// The screen's numbers the layout needs, read from the host by the plugin.
#[derive(Debug, Clone, Copy, Default)]
pub struct Look {
    pub width: u16,
    pub height: u16,
    pub dpi: u16,
    pub touch: bool,
    /// Text and small text sizes in pixels.
    pub text: f32,
    pub small: f32,
    /// A row's height (a finger's on touch screens), margins, gap, line width.
    pub row: u16,
    pub margin: u16,
    pub gap: u16,
    pub line: u16,
    /// Width of a capital M at one pixel of size: letters grow with it.
    pub m_width: f32,
    /// A text line's height at the text size.
    pub text_line: u16,
    /// The status line at the top of the screen stays in sight.
    pub status: u16,
}

/// The keyboard's own settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct Own<'a> {
    pub shape: &'a str,
    pub size: &'a str,
    pub auto_capitals: bool,
}

/// `value` tenths of a millimetre in pixels.
fn tenth_mm(dpi: u16, value: u32) -> u32 {
    (value * u32::from(dpi) + 127) / 254
}

/// Whether the next letter after `text` starts a sentence: at its start, or
/// after ". ", "! ", "? " or a new line.
#[must_use]
pub fn sentence_starts(text: &str) -> bool {
    let trimmed = text.trim_end_matches(' ');
    if trimmed.is_empty() || text.ends_with('\n') {
        return true;
    }
    trimmed.len() < text.len() && trimmed.ends_with(['.', '!', '?'])
}

/// `c`'s capital, where it has one of its own.
#[must_use]
pub fn capital(c: char) -> char {
    if let Some(double) = hangul::shifted(c) {
        return double;
    }
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// `c` small, where it has one small letter of its own.
#[must_use]
pub fn small(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// The variants of a letter, shown above it.
#[derive(Debug)]
pub struct Popup {
    pub chars: Vec<char>,
    pub area: Rect,
    /// The variant chosen with buttons.
    pub focus: Option<usize>,
}

impl Popup {
    /// Where variant `index` is.
    #[must_use]
    pub fn key(&self, index: usize) -> Rect {
        let width = self.area.width / u16::try_from(self.chars.len().max(1)).unwrap_or(1);
        Rect::new(
            self.area
                .x
                .saturating_add(width.saturating_mul(index as u16)),
            self.area.y,
            width,
            self.area.height,
        )
    }
}

/// A layout in use: the code on the key that switches, its letter rows and
/// the variants it adds.
pub type Layout = crate::layouts::Chosen;

/// The keyboard in use.
#[derive(Debug)]
pub struct Board {
    pub title: String,
    pub text: String,
    pub layer: Layer,
    pub layouts: Vec<Layout>,
    pub current: usize,
    pub asked: Asked,
    pub shift: Shift,
    pub shifted_at: Option<u64>,
    /// Capitals start the text and its sentences.
    pub auto: bool,
    pub rows: Vec<Vec<Slot>>,
    /// The key chosen with buttons, as `(row, index)`; none while tapping.
    pub focus: Option<(usize, usize)>,
    /// Where across the keys the focus came along a row, kept while it moves
    /// up and down.
    pub column: Option<u32>,
    pub popup: Option<Popup>,
    pub look: Look,
    pub region: Rect,
    pub field: Rect,
    pub keys: Rect,
    pub key_height: u16,
    /// The most units a row of keys holds.
    pub most: u32,
    pub seam: u16,
    pub side: u16,
    /// The height of the strip of the field's buttons under the text box.
    pub strip: u16,
    /// How large the letters are, in hundredths of the text.
    pub letters: u32,
    pub buttons: Vec<(FieldButton, Rect)>,
}

/// What a press of a key came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    /// The text or the keys changed.
    Changed,
    /// The text was cut at the most characters.
    Full,
    /// Typing is done.
    Done,
    /// Nothing changed.
    Nothing,
}

impl Board {
    /// The keyboard for `layouts` and the question `asked`, laid out for
    /// `look`; `kinds` are the field's buttons with their label widths.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when a list cannot grow.
    pub fn new(
        look: Look,
        own: Own<'_>,
        asked: Asked,
        layouts: Vec<Layout>,
        current: usize,
        buttons: &[(FieldButton, f32)],
    ) -> Result<Self, Error> {
        let touch = look.touch;
        let layer = if asked.number {
            Layer::Digits
        } else {
            Layer::Letters
        };
        let current = if current >= layouts.len() { 0 } else { current };
        let (high, scale) = if touch {
            (100, 100)
        } else {
            button_scale(own.size)
        };
        let (seam, side, most) = spacing(&look, scale);
        let letters = layouts.get(current).map_or("", |l| l.1.as_ref());
        let other = match layer {
            Layer::Letters => {
                try_rows_for(letters, Layer::Symbols, false, most, asked.multiline)?.len()
            }
            _ => 0,
        };
        let rows = try_rows_for(letters, layer, layouts.len() > 1, most, asked.multiline)?;
        let count = rows.len().max(other).max(4);
        let touch_row = look.row;
        let strip = if asked.multiline && !buttons.is_empty() {
            touch_row
        } else {
            0
        };
        let title = title_height(&look);
        let mut field_height = title + touch_row + strip;
        let widest = rows.iter().map(|row| units(row)).max().unwrap_or(20).max(1);
        let letter = (u32::from(look.width).saturating_sub(2 * u32::from(side))) * 2 / widest;
        let gap = look.gap / 2;
        let (key_height, room) = if touch {
            let room = side.saturating_sub(border(&look) as u16);
            let around = field_height + gap * 2 + room;
            (
                tapped_height(&look, own, letter, count as u16, around),
                room,
            )
        } else {
            let high = (u32::from(touch_row) * high / 100) as u16;
            (high, edge_room(&look, high))
        };
        let status = look.status;
        let room_for_keys = look
            .height
            .saturating_sub(field_height.saturating_add(gap * 2 + room + status));
        let keys_height = key_height.saturating_mul(count as u16).min(room_for_keys);
        if asked.multiline {
            let used = status + field_height + keys_height + gap * 2 + room;
            field_height += more_lines(&look, look.height.saturating_sub(used));
        }
        let height = (field_height + keys_height + gap * 2 + room).min(look.height);
        let region = Rect::new(0, look.height - height, look.width, height);
        let field = Rect::new(0, region.y + gap, look.width, field_height);
        let keys = Rect::new(
            0,
            (region.y + region.height).saturating_sub(keys_height + room),
            look.width,
            keys_height,
        );
        let mut board = Self {
            auto: own.auto_capitals && asked.plain,
            title: String::new(),
            text: String::new(),
            layer,
            layouts,
            current,
            asked,
            shift: Shift::Off,
            shifted_at: None,
            rows,
            focus: (!touch).then_some((0, 0)),
            column: None,
            popup: None,
            look,
            region,
            field,
            keys,
            key_height: keys_height / count as u16,
            most,
            seam,
            side,
            strip,
            letters: scale,
            buttons: Vec::new(),
        };
        board.place_buttons(buttons)?;
        board.shift = board.auto_shift();
        board.focus_phone();
        Ok(board)
    }

    /// With buttons alone a text is typed a letter at a time: the focus starts
    /// on *Phone*, where it can be typed on a phone instead.
    fn focus_phone(&mut self) {
        let phone = self
            .buttons
            .iter()
            .position(|(kind, _)| *kind == FieldButton::Phone);
        if let (false, Some(_), Some(index)) = (self.look.touch, self.focus, phone) {
            self.focus = Some((FIELD_ROW, index));
        }
    }

    /// The box of the text field: on the margins, like its title above.
    #[must_use]
    pub fn text_box(&self) -> Rect {
        let f = self.field;
        let title = title_height(&self.look);
        let m = self.look.margin;
        Rect::new(
            f.x + m,
            f.y + title,
            f.width.saturating_sub(2 * m),
            f.height.saturating_sub(title + self.strip),
        )
    }

    /// Where the field's buttons lie: at the end of the text's box, or of
    /// several lines in the strip under it.
    #[must_use]
    pub fn buttons_box(&self) -> Rect {
        let b = self.text_box();
        if self.strip == 0 {
            return b;
        }
        Rect::new(b.x, b.y + b.height, b.width, self.strip)
    }

    fn place_buttons(&mut self, kinds: &[(FieldButton, f32)]) -> Result<(), Error> {
        let b = self.buttons_box();
        let height = button_height(&self.look, b);
        let pad = b.height.saturating_sub(height) / 2;
        let mut edge = b.x + b.width - pad.min(b.width);
        let mut placed: Vec<(FieldButton, Rect)> = Vec::new();
        placed
            .try_reserve(kinds.len())
            .map_err(|_| Error::NoMemory)?;
        for &(kind, label) in kinds.iter().rev() {
            let width = (label + 1.0) as u16 + self.look.gap * 3;
            let left = edge.saturating_sub(width);
            placed.push((kind, Rect::new(left, b.y + pad, width, height)));
            edge = left.saturating_sub(pad);
        }
        placed.reverse();
        self.buttons = placed;
        Ok(())
    }

    /// Keys are drawn inset by this inside their area.
    #[must_use]
    pub fn key_inset(&self) -> u32 {
        u32::from(self.seam)
    }

    /// Where key `index` of row `row` is (the field's buttons for
    /// [`FIELD_ROW`]).
    #[must_use]
    pub fn key_area(&self, row: usize, index: usize) -> Rect {
        if row == FIELD_ROW {
            return self.buttons.get(index).map_or(self.field, |b| b.1);
        }
        let Some(keys) = self.rows.get(row) else {
            return self.keys;
        };
        let own = units(keys);
        let total = if self.look.touch {
            self.rows.iter().map(|r| units(r)).max().unwrap_or(own)
        } else {
            own
        }
        .max(1);
        let outside = u32::from(self.side).saturating_sub(self.key_inset());
        let width = u32::from(self.keys.width).saturating_sub(2 * outside);
        let x0 = u32::from(self.keys.x) + outside;
        let before = total.saturating_sub(own) / 2 + units(&keys[..index.min(keys.len())]);
        let wide = keys.get(index).map_or(0, |k| u32::from(k.1));
        let left = x0 + width * before / total;
        let right = x0 + width * (before + wide) / total;
        let y = self.keys.y + self.key_height * row as u16;
        Rect::new(left as u16, y, (right - left) as u16, self.key_height)
    }

    /// What a focus on `(row, index)` draws over: its key, or for a button of
    /// the field the field's box, as the button's focus frame goes past its
    /// edge (interface 6's `focus_area`).
    #[must_use]
    pub fn focus_area(&self, (row, index): (usize, usize)) -> Rect {
        if row == FIELD_ROW {
            self.buttons_box()
        } else {
            self.key_area(row, index)
        }
    }

    /// The key under `point`.
    #[must_use]
    pub fn key_at(&self, point: Point) -> Option<(usize, usize)> {
        if let Some(index) = self.buttons.iter().position(|b| b.1.contains(point)) {
            return Some((FIELD_ROW, index));
        }
        (0..self.rows.len()).find_map(|row| {
            (0..self.rows[row].len())
                .find(|&index| self.key_area(row, index).contains(point))
                .map(|index| (row, index))
        })
    }

    /// The key at `(row, index)`.
    #[must_use]
    pub fn key(&self, row: usize, index: usize) -> Option<Key> {
        self.rows.get(row)?.get(index).map(|k| k.0)
    }

    /// `c` as typed now: a capital after *Shift* (if it has one capital).
    #[must_use]
    pub fn cased(&self, c: char) -> char {
        if self.shift == Shift::Off {
            c
        } else {
            capital(c)
        }
    }

    fn letters_now(&self) -> &str {
        self.layouts.get(self.current).map_or("", |l| l.1.as_ref())
    }

    /// Whether the layout has letters with capitals.
    #[must_use]
    pub fn cases(&self) -> bool {
        self.letters_now().chars().any(char::is_lowercase)
    }

    /// Whether the layout types jamo, which join to syllables.
    #[must_use]
    pub fn composing(&self) -> bool {
        self.letters_now().chars().any(hangul::is_jamo)
    }

    /// What *Shift* is after the text changed: locked capitals stay, else one
    /// where a sentence starts and capitals come by themselves.
    #[must_use]
    pub fn auto_shift(&self) -> Shift {
        match self.shift {
            Shift::Locked => Shift::Locked,
            _ if self.auto
                && self.layer == Layer::Letters
                && self.cases()
                && sentence_starts(&self.text) =>
            {
                Shift::Once
            }
            _ => Shift::Off,
        }
    }

    /// The most characters that may be typed.
    #[must_use]
    pub fn max_chars(&self) -> usize {
        if self.asked.multiline {
            MAX_CHARS_MULTILINE
        } else {
            MAX_CHARS
        }
    }

    /// Adds text at the end, as far as there is room; jamo join to syllables
    /// where `join` says.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the text cannot grow.
    pub fn insert(&mut self, text: &str, join: bool) -> Result<Pressed, Error> {
        let room = self.max_chars().saturating_sub(self.text.chars().count());
        for c in text.chars().take(room) {
            self.text
                .try_reserve(c.len_utf8() + 4)
                .map_err(|_| Error::NoMemory)?;
            if join && hangul::is_jamo(c) {
                hangul::push(&mut self.text, c);
            } else {
                self.text.push(c);
            }
        }
        self.shift = self.auto_shift();
        if self.shift != Shift::Locked {
            self.shifted_at = None;
        }
        Ok(if text.chars().nth(room).is_some() {
            Pressed::Full
        } else {
            Pressed::Changed
        })
    }

    /// Removes the last character (one jamo of a syllable).
    pub fn delete(&mut self) -> Pressed {
        let taken = if self.composing() {
            hangul::pop(&mut self.text)
        } else {
            self.text.pop().is_some()
        };
        if taken {
            self.shift = self.auto_shift();
            Pressed::Changed
        } else {
            Pressed::Nothing
        }
    }

    /// Starts the text over (a long press on *Delete*).
    pub fn clear(&mut self) {
        self.text.clear();
        self.shift = self.auto_shift();
    }

    #[allow(dead_code, reason = "the host's text_typed event (draft 22) feeds it")]
    /// Text typed from outside: as the keys would add it, without what is no
    /// text.
    ///
    /// # Errors
    /// As [`Board::insert`].
    pub fn typed(&mut self, text: &str) -> Result<Pressed, Error> {
        let mut kept = String::new();
        kept.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
        kept.extend(
            text.chars()
                .filter(|c| !c.is_control() || (self.asked.multiline && *c == '\n'))
                .filter(|c| self.layer != Layer::Digits || c.is_ascii_digit()),
        );
        // What was typed may be a password: the copy is zeroed, not just freed.
        let result = self.insert(&kept, true);
        wipe_text(kept);
        result
    }

    /// Inserts pasted text: one line (several where lines are typed), digits
    /// only for numbers.
    ///
    /// # Errors
    /// As [`Board::insert`].
    pub fn pasted(&mut self, text: &str) -> Result<Pressed, Error> {
        let mut line = String::new();
        line.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let c = match c {
                '\r' if chars.peek() == Some(&'\n') => continue,
                '\r' => '\n',
                c => c,
            };
            let c = if c.is_control() && !(self.asked.multiline && c == '\n') {
                ' '
            } else {
                c
            };
            if self.layer != Layer::Digits || c.is_ascii_digit() {
                line.push(c);
            }
        }
        let result = self.insert(line.trim(), false);
        wipe_text(line);
        result
    }

    /// Replaces the text with pasted text (the phone's answer): the text so
    /// far stays when the new one cannot be put in.
    ///
    /// # Errors
    /// As [`Board::pasted`]; the board is then as it was.
    pub fn replace_pasted(&mut self, text: &str) -> Result<Pressed, Error> {
        let before = core::mem::take(&mut self.text);
        match self.pasted(text) {
            Ok(pressed) => {
                wipe_text(before);
                Ok(pressed)
            }
            Err(error) => {
                self.text = before;
                self.shift = self.auto_shift();
                Err(error)
            }
        }
    }

    /// Presses `key` at time `now` (milliseconds).
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the text or the keys cannot grow.
    pub fn press(&mut self, key: Key, now: u64) -> Result<Pressed, Error> {
        match key {
            Key::Char(c) => {
                let c = self.cased(c);
                let mut buffer = [0u8; 4];
                self.insert(c.encode_utf8(&mut buffer), true)
            }
            Key::Space => self.insert(" ", false),
            Key::Enter => self.insert("\n", false),
            Key::Delete => Ok(self.delete()),
            Key::Shift => {
                // One capital, locked when pressed again at once, then off.
                let soon = self.shifted_at.is_some_and(|at| now <= at + DOUBLE_MS);
                (self.shift, self.shifted_at) = match self.shift {
                    Shift::Off => (Shift::Once, Some(now)),
                    Shift::Once if soon => (Shift::Locked, None),
                    _ => (Shift::Off, None),
                };
                Ok(Pressed::Changed)
            }
            Key::Layer(layer) => {
                let before = core::mem::replace(&mut self.layer, layer);
                if let Err(error) = self.rebuild() {
                    self.layer = before;
                    return Err(error);
                }
                Ok(Pressed::Changed)
            }
            Key::NextLayout => {
                let before = self.current;
                self.current = (before + 1) % self.layouts.len().max(1);
                if let Err(error) = self.rebuild() {
                    self.current = before;
                    return Err(error);
                }
                Ok(Pressed::Changed)
            }
            Key::Done => Ok(Pressed::Done),
        }
    }

    /// The keys again, after the layer or layout changed.
    fn rebuild(&mut self) -> Result<(), Error> {
        let rows = try_rows_for(
            self.letters_now(),
            self.layer,
            self.layouts.len() > 1,
            self.most,
            self.asked.multiline,
        )?;
        // Nothing can be refused from here on.
        self.shift = Shift::Off;
        self.shift = self.auto_shift();
        self.rows = rows;
        if let Some((row, index)) = self.focus.filter(|(row, _)| *row != FIELD_ROW) {
            let row = row.min(self.rows.len().saturating_sub(1));
            let last = self.rows.get(row).map_or(0, |r| r.len().saturating_sub(1));
            self.focus = Some((row, index.min(last)));
        }
        Ok(())
    }

    /// The letters the layout holds on the letter `c` (German ä on a).
    fn held_of(&self, c: char) -> Option<&str> {
        let lower = small(c);
        self.layouts
            .get(self.current)?
            .2
            .iter()
            .find(|(base, _)| *base == lower)
            .map(|(_, letters)| letters.as_str())
    }

    /// The letters held on the focused letter, cased as typed now.
    #[must_use]
    pub fn held_letters(&self) -> Option<String> {
        let (row, index) = self.focus.filter(|(row, _)| *row != FIELD_ROW)?;
        let Some(Key::Char(c)) = self.key(row, index) else {
            return None;
        };
        let letters = self.held_of(c)?;
        let mut shown = String::new();
        shown.try_reserve(letters.len()).ok()?;
        shown.extend(letters.chars().map(|l| self.cased(l)));
        Some(shown)
    }

    /// Shows the variants of the letter at `(row, index)`, if it has any.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the list cannot grow.
    pub fn open_variants(&mut self, row: usize, index: usize) -> Result<bool, Error> {
        let Some(Key::Char(c)) = self.key(row, index) else {
            return Ok(false);
        };
        let own = self.held_of(c).unwrap_or("");
        let builtin = crate::layouts::variants(c).unwrap_or_default();
        let other = if self.shift == Shift::Off {
            capital(c)
        } else {
            small(c)
        };
        let mut chars: Vec<char> = Vec::new();
        if other != self.cased(c) {
            chars.try_reserve(1).map_err(|_| Error::NoMemory)?;
            chars.push(other);
        }
        for v in own.chars().chain(builtin.chars()).map(|v| self.cased(v)) {
            if !chars.contains(&v) && chars.len() <= 2 * crate::layouts::MAX_VARIANTS {
                chars.try_reserve(1).map_err(|_| Error::NoMemory)?;
                chars.push(v);
            }
        }
        if chars.is_empty() {
            return Ok(false);
        }
        let key = self.key_area(row, index);
        let margin = self.look.margin / 2;
        let room = self.region.width.saturating_sub(2 * margin);
        let width = key
            .width
            .max(self.key_height)
            .saturating_mul(chars.len() as u16)
            .min(room);
        let center = key.x + key.width / 2;
        let x = center
            .saturating_sub(width / 2)
            .clamp(margin, self.region.width.saturating_sub(margin + width));
        let y = key.y.saturating_sub(self.key_height).max(self.region.y);
        let focus = self.focus.map(|_| 0);
        self.popup = Some(Popup {
            chars,
            area: Rect::new(x, y, width, self.key_height),
            focus,
        });
        Ok(true)
    }

    /// The middle of key `index` of `row`, across.
    fn center(&self, row: usize, index: usize) -> u32 {
        let area = self.key_area(row, index);
        u32::from(area.x) + u32::from(area.width) / 2
    }

    /// The key of row `to` below the column the focus keeps, else below the
    /// middle of key `index` of `row`.
    fn nearest(&self, row: usize, index: usize, to: usize) -> (usize, usize) {
        let center = match self.column {
            Some(column) if row != FIELD_ROW && to != FIELD_ROW => column,
            _ => self.center(row, index),
        };
        let len = self.rows.get(to).map_or(0, Vec::len);
        let at = (0..len)
            .find(|&i| self.key_area(to, i).right() > center)
            .unwrap_or(len.saturating_sub(1));
        (to, at)
    }

    /// Where left, right, next or previous go from key `index` of `row`.
    fn along(&self, row: usize, index: usize, action: Move) -> (usize, usize) {
        let last_row = self.rows.len().saturating_sub(1);
        let len = |row: usize| self.rows.get(row).map_or(1, Vec::len);
        match action {
            Move::Left | Move::Previous => match index.checked_sub(1) {
                Some(index) => (row, index),
                None if action == Move::Previous => {
                    let row = row.checked_sub(1).unwrap_or(last_row);
                    (row, len(row) - 1)
                }
                None => (row, len(row) - 1),
            },
            _ if index + 1 < len(row) => (row, index + 1),
            Move::Next => (if row == last_row { 0 } else { row + 1 }, 0),
            _ => (row, 0),
        }
    }

    /// Moves the button focus. `None`: no focus yet, so it appears.
    /// Returns the keys (`from`, `to`) to draw again.
    pub fn move_focus(&mut self, go: Move) -> Option<((usize, usize), (usize, usize))> {
        let Some((row, index)) = self.focus else {
            self.focus = Some((0, 0));
            return Some(((0, 0), (0, 0)));
        };
        let last_row = self.rows.len().saturating_sub(1);
        let to = if row == FIELD_ROW {
            let last = self.buttons.len().saturating_sub(1);
            match go {
                Move::Right | Move::Next if index == last => {
                    self.column = Some(self.center(0, 0));
                    (0, 0)
                }
                Move::Down => self.nearest(row, index, 0),
                Move::Up => self.nearest(row, index, last_row),
                Move::Right | Move::Next => (row, step(index, 1, last + 1)),
                Move::Left | Move::Previous => (row, step(index, -1, last + 1)),
            }
        } else {
            match go {
                Move::Left | Move::Previous | Move::Right | Move::Next => {
                    let to = self.along(row, index, go);
                    self.column = Some(self.center(to.0, to.1));
                    to
                }
                Move::Up => match row.checked_sub(1) {
                    Some(above) => self.nearest(row, index, above),
                    None if !self.buttons.is_empty() => (FIELD_ROW, self.buttons.len() - 1),
                    None => self.nearest(row, index, last_row),
                },
                Move::Down => self.nearest(row, index, if row == last_row { 0 } else { row + 1 }),
            }
        };
        self.focus = Some(to);
        Some(((row, index), to))
    }

    /// Buttons while the variants are shown. `Some(true)` when a variant was
    /// chosen and typed.
    ///
    /// # Errors
    /// As [`Board::insert`].
    pub fn variants_move(&mut self, go: Move) -> Option<()> {
        let popup = self.popup.as_mut()?;
        let len = popup.chars.len();
        let focus = popup.focus.unwrap_or(0);
        popup.focus = Some(match go {
            Move::Left | Move::Previous | Move::Up => step(focus, -1, len),
            Move::Right | Move::Next | Move::Down => step(focus, 1, len),
        });
        Some(())
    }
}

/// Where the buttons move the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    Left,
    Right,
    Next,
    Previous,
}

impl Move {
    /// The move an action names (page buttons move up and down).
    #[must_use]
    pub fn of(action: &str) -> Option<Self> {
        Some(match action {
            "core.up" | "core.prev-page" => Self::Up,
            "core.down" | "core.next-page" => Self::Down,
            "core.left" => Self::Left,
            "core.right" => Self::Right,
            "core.next" => Self::Next,
            "core.previous" => Self::Previous,
            _ => return None,
        })
    }
}

/// `index` moved by `by` round a list of `len`.
#[must_use]
pub fn step(index: usize, by: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (index as isize + by).rem_euclid(len as isize) as usize
}

/// The line of a button's border, in pixels.
fn border(look: &Look) -> u32 {
    u32::from(look.line).max(1)
}

/// The height of the title above the text field.
#[must_use]
pub fn title_height(look: &Look) -> u16 {
    (look.small * 1.5) as u16
}

/// How high a button of the field is within `area`.
fn button_height(look: &Look, area: Rect) -> u16 {
    area.height
        .saturating_sub(2 * look.gap)
        .min(look.row)
        .max(1)
}

/// The room under keys chosen with buttons, `high` high.
fn edge_room(look: &Look, high: u16) -> u16 {
    let _ = high;
    look.gap / 2
}

/// How high the keys are where they are tapped, for keys `letter` wide,
/// `count` rows of them and `around` pixels of the keyboard that are not keys.
fn tapped_height(look: &Look, own: Own<'_>, letter: u32, count: u16, around: u16) -> u16 {
    let border = 2 * border(look) as u16;
    let touch_row = look.row;
    let height = (letter * height_of(own.shape) / 100) as u16;
    let shaped = match own.shape {
        SHAPE_TALL => height.min(touch_row).max(look.text_line + border),
        SHAPE_ROUND => height.max(look.text as u16 + border),
        _ => height.min(touch_row).max(look.text as u16 + border),
    };
    let count = count.max(1);
    let medium = u32::from(around) + u32::from(count) * u32::from(shaped);
    let target = medium * size_of(own.size) / 100;
    let sized = (target.saturating_sub(u32::from(around)) / u32::from(count)) as u16;
    let half = (look.height / 2).saturating_sub(around) / count;
    let sized = if sized > shaped {
        sized.min(half.max(shaped))
    } else {
        sized
    };
    let least = (tenth_mm(look.dpi, LEAST_TAP_MM * 10) as u16).max(look.text as u16 + border);
    sized.max(least)
}

/// The room between keys, at the screen's sides, and the most units a row
/// holds, with letters `scale` hundredths of the text (as many units as leave
/// a letter, 2 units, its least width).
fn spacing(look: &Look, scale: u32) -> (u16, u16, u32) {
    let least = if look.touch {
        tenth_mm(look.dpi, LEAST_TAP_MM * 10)
    } else {
        let px = look.text * scale as f32 / 100.0;
        (look.m_width * px) as u32 + 2 * (border(look) + u32::from(BUTTONS_SEAM))
    };
    let unit = (least / 2).max(1);
    let seam = if look.touch {
        let half = tenth_mm(look.dpi, TOUCH_GAP_TENTHS) / 2;
        half.max(border(look)) as u16
    } else {
        BUTTONS_SEAM
    };
    let side = (look.margin / 4).max(LEAST_SIDE).max(seam);
    let most = (u32::from(look.width).saturating_sub(2 * u32::from(side))) / unit;
    (seam, side, most)
}

/// How much higher the field of several lines is made in `spare` room: whole
/// lines, as many as fit up to [`MORE_LINES`].
fn more_lines(look: &Look, spare: u16) -> u16 {
    let line = f32::from(look.text_line.max(1));
    let more = ((f32::from(spare) / line) as u16).min(MORE_LINES);
    (f32::from(more) * line) as u16
}

/// Whether `shape` is one the keyboard knows.
#[must_use]
pub fn known_shape(shape: &str) -> bool {
    matches!(shape, SHAPE_ROUND | SHAPE_TALL | SHAPE_WIDE)
}

/// Whether `size` is one the keyboard knows.
#[must_use]
pub fn known_size(size: &str) -> bool {
    matches!(size, SIZE_SMALL | SIZE_LARGE | SIZE_MEDIUM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fail_alloc::sweep;
    use crate::layouts::chosen_for;
    use alloc::string::ToString;
    use alloc::vec;

    fn look(touch: bool) -> Look {
        Look {
            width: 480,
            height: 800,
            dpi: 220,
            touch,
            text: 20.0,
            small: 15.0,
            row: if touch { 60 } else { 40 },
            margin: 12,
            gap: 8,
            line: 2,
            m_width: 0.9,
            text_line: 26,
            status: 24,
        }
    }

    fn own(auto_capitals: bool) -> Own<'static> {
        Own {
            shape: "keyboard.key-shape.round",
            size: "keyboard.size.medium",
            auto_capitals,
        }
    }

    fn plain() -> Asked {
        Asked {
            plain: true,
            ..Asked::default()
        }
    }

    fn board(touch: bool, asked: Asked, auto: bool) -> Board {
        try_board(touch, asked, auto).unwrap()
    }

    fn try_board(touch: bool, asked: Asked, auto: bool) -> Result<Board, Error> {
        let layouts = chosen_for("en", "", "keyboard.layout.en, keyboard.layout.de", false)?;
        Board::new(look(touch), own(auto), asked, layouts, 0, &[])
    }

    fn type_all(board: &mut Board, text: &str) {
        for c in text.chars() {
            board.press(Key::Char(c), 0).unwrap();
        }
    }

    #[test]
    fn the_keyboard_lies_at_the_bottom_inside_the_screen() {
        for touch in [true, false] {
            let b = board(touch, plain(), false);
            let l = look(touch);
            assert_eq!(b.region.x + b.region.width, l.width);
            assert_eq!(b.region.y + b.region.height, l.height);
            assert!(b.keys.y >= b.field.y + b.field.height - 1);
            assert!(b.key_height > 0);
            for (row, keys) in b.rows.iter().enumerate() {
                for index in 0..keys.len() {
                    let a = b.key_area(row, index);
                    assert!(a.x + a.width <= l.width, "{row}/{index}");
                    assert!(a.y >= b.keys.y && a.y + a.height <= b.keys.y + b.keys.height);
                }
            }
            // The first key is found where it lies.
            let first = b.key_area(0, 0);
            let at = Point::new(
                i32::from(first.x + first.width / 2),
                i32::from(first.y + first.height / 2),
            );
            assert_eq!(b.key_at(at), Some((0, 0)));
        }
    }

    #[test]
    fn with_buttons_the_focus_starts_on_the_first_key() {
        assert_eq!(board(false, plain(), false).focus, Some((0, 0)));
        assert_eq!(board(true, plain(), false).focus, None);
    }

    #[test]
    fn shift_types_one_capital_and_twice_in_a_row_locks_them() {
        let mut b = board(true, plain(), false);
        b.press(Key::Shift, 100).unwrap();
        assert_eq!(b.shift, Shift::Once);
        type_all(&mut b, "ab");
        assert_eq!(b.text, "Ab");
        b.press(Key::Shift, 1000).unwrap();
        b.press(Key::Shift, 1200).unwrap();
        assert_eq!(b.shift, Shift::Locked);
        type_all(&mut b, "cd");
        assert_eq!(b.text, "AbCD");
        b.press(Key::Shift, 5000).unwrap();
        assert_eq!(b.shift, Shift::Off);
        // A second press too late only switches off.
        b.press(Key::Shift, 6000).unwrap();
        b.press(Key::Shift, 9000).unwrap();
        assert_eq!(b.shift, Shift::Off);
    }

    #[test]
    fn automatic_capitals_start_sentences_in_plain_text_only() {
        let mut b = board(true, plain(), true);
        assert_eq!(b.shift, Shift::Once);
        type_all(&mut b, "hi. ok");
        assert_eq!(b.text, "Hi. Ok");
        let secret = Asked {
            secret: true,
            plain: false,
            ..Asked::default()
        };
        assert_eq!(board(true, secret, true).shift, Shift::Off);
    }

    #[test]
    fn delete_takes_back_and_a_long_text_is_cut_with_a_notice() {
        let mut b = board(true, plain(), false);
        type_all(&mut b, "abc");
        assert_eq!(b.delete(), Pressed::Changed);
        assert_eq!(b.text, "ab");
        b.clear();
        assert_eq!(b.delete(), Pressed::Nothing);
        let long = "x".repeat(MAX_CHARS + 5);
        assert_eq!(b.insert(&long, false).unwrap(), Pressed::Full);
        assert_eq!(b.text.chars().count(), MAX_CHARS);
        let mut lines = board(
            true,
            Asked {
                multiline: true,
                ..plain()
            },
            false,
        );
        assert_eq!(lines.max_chars(), MAX_CHARS_MULTILINE);
        lines.press(Key::Enter, 0).unwrap();
        assert_eq!(lines.text, "\n");
    }

    #[test]
    fn pasted_text_is_one_line_and_numbers_take_digits_only() {
        let mut b = board(true, plain(), false);
        b.pasted("  a\r\nb\tc ").unwrap();
        assert_eq!(b.text, "a b c");
        let mut lines = board(
            true,
            Asked {
                multiline: true,
                ..plain()
            },
            false,
        );
        lines.pasted("a\r\nb").unwrap();
        assert_eq!(lines.text, "a\nb");
        let mut number = board(
            true,
            Asked {
                number: true,
                ..Asked::default()
            },
            false,
        );
        assert_eq!(number.layer, Layer::Digits);
        number.pasted("a1b2-3").unwrap();
        assert_eq!(number.text, "123");
        number.typed("x4").unwrap();
        assert_eq!(number.text, "1234");
    }

    #[test]
    fn layer_and_layout_keys_rebuild_the_rows() {
        let mut b = board(true, plain(), false);
        let letters = b.rows.clone();
        b.press(Key::Layer(Layer::Symbols), 0).unwrap();
        assert_ne!(b.rows, letters);
        b.press(Key::Layer(Layer::Letters), 0).unwrap();
        assert_eq!(b.rows, letters);
        // Two layouts: the key switches to German, which swaps y and z.
        b.press(Key::NextLayout, 0).unwrap();
        assert_eq!(b.current, 1);
        assert_ne!(b.rows, letters);
        assert!(b.held_letters().is_none() || b.focus.is_some());
    }

    #[test]
    fn the_focus_goes_round_the_rows_and_remembers_its_column() {
        let mut b = board(false, plain(), false);
        b.focus = Some((0, 0));
        b.move_focus(Move::Left);
        let last = b.rows[0].len() - 1;
        assert_eq!(b.focus, Some((0, last)));
        b.move_focus(Move::Right);
        assert_eq!(b.focus, Some((0, 0)));
        b.move_focus(Move::Down);
        assert_eq!(b.focus.map(|f| f.0), Some(1));
        b.move_focus(Move::Up);
        b.move_focus(Move::Up);
        // Up from the first row goes round to the last.
        assert_eq!(b.focus.map(|f| f.0), Some(b.rows.len() - 1));
    }

    #[test]
    fn variants_show_the_capital_first_and_close() {
        let mut b = board(true, plain(), false);
        let (row, index) = (
            0,
            b.rows[0]
                .iter()
                .position(|k| k.0 == Key::Char('e'))
                .unwrap(),
        );
        assert!(b.open_variants(row, index).unwrap());
        let popup = b.popup.as_ref().unwrap();
        assert_eq!(popup.chars[0], 'E');
        assert!(popup.chars.contains(&'é'.to_ascii_lowercase()) || popup.chars.len() > 1);
        assert!(popup.area.x + popup.area.width <= b.region.width);
        // A key without variants has no popup.
        let space = b
            .rows
            .last()
            .unwrap()
            .iter()
            .position(|k| k.0 == Key::Space)
            .unwrap();
        assert!(!b.open_variants(b.rows.len() - 1, space).unwrap());
    }

    #[test]
    fn german_holds_umlauts_on_their_letters() {
        let layouts = chosen_for("de", "", "keyboard.layout.auto", false).unwrap();
        let mut b = Board::new(look(false), own(false), plain(), layouts, 0, &[]).unwrap();
        let at = b.rows[1]
            .iter()
            .position(|k| k.0 == Key::Char('a'))
            .unwrap();
        b.focus = Some((1, at));
        assert_eq!(b.held_letters().as_deref(), Some("ä"));
        b.shift = Shift::Once;
        assert_eq!(b.held_letters().as_deref(), Some("Ä"));
    }

    #[test]
    fn layout_codes_come_from_the_language() {
        let layouts = chosen_for("de-AT", "", "", false).unwrap();
        assert_eq!(layouts[0].0, "DE");
        let own_rows = chosen_for("xx", "abc def ghi", "keyboard.layout.auto", false).unwrap();
        assert_eq!(own_rows[0].1, "abc def ghi");
        assert_eq!(own_rows[0].0, "XX");
        assert_eq!(vec!["x".to_string()].len(), 1);
    }

    /// Every allocation may be refused: the model ends in `NoMemory` or
    /// succeeds, and never aborts.
    #[test]
    fn allocation_refusals_end_in_no_memory() {
        sweep(|| {
            chosen_for("de", "", "keyboard.layout.de, keyboard.layout.auto", true).map(|l| l.len())
        });
        sweep(|| {
            let layouts = chosen_for("en", "", "keyboard.layout.en", false)?;
            Board::new(
                look(true),
                own(true),
                plain(),
                layouts,
                0,
                &[(FieldButton::Paste, 40.0)],
            )
            .map(|b| b.rows.len())
        });
        sweep(|| {
            let mut b = try_board(true, plain(), false)?;
            b.insert("héllo wörld", true)?;
            b.pasted("a\r\nb c")?;
            b.typed("xyz")?;
            b.press(Key::Shift, 0)?;
            b.press(Key::NextLayout, 0)?;
            b.open_variants(0, 0)?;
            Ok(b.text.len())
        });
    }

    #[test]
    fn a_refused_press_or_paste_leaves_the_board_as_it_was() {
        let mut b = board(true, plain(), false);
        sweep(|| {
            b.layer = Layer::Letters;
            b.current = 0;
            let (rows, shift) = (b.rows.len(), b.shift);
            let pressed = b.press(Key::Layer(Layer::Symbols), 0);
            if pressed.is_err() {
                assert_eq!(
                    (b.layer, b.rows.len(), b.shift),
                    (Layer::Letters, rows, shift)
                );
            }
            pressed
        });
        sweep(|| {
            b.layer = Layer::Letters;
            b.current = 0;
            let rows = b.rows.len();
            let pressed = b.press(Key::NextLayout, 0);
            if pressed.is_err() {
                assert_eq!((b.current, b.rows.len()), (0, rows));
            }
            pressed
        });
        sweep(|| {
            b.layer = Layer::Letters;
            b.text.clear();
            b.text.push_str("keep");
            let pasted = b.replace_pasted("new text");
            if pasted.is_err() {
                assert_eq!(b.text, "keep");
            }
            pasted
        });
        assert_eq!(b.text, "new text");
    }
}
