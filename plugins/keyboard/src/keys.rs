//! The keys: their kinds, the rows of a layer (split only where its
//! letters would not fit), and how wide and high they are.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the plugin, which shares its imports"
)]
use super::*;
use paperpaw_ui_kit::mem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    Letters,
    Symbols,
    /// Only digits, for numbers (mode `number`).
    Digits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    Char(char),
    Shift,
    Delete,
    Space,
    Layer(Layer),
    /// Switches to the next chosen layout.
    NextLayout,
    /// A line break (mode `multiline`).
    Enter,
    Done,
}

/// A key and its width in units (a letter is 2).
pub(crate) type Slot = (Key, u8);

/// How high keys chosen with buttons are, and how large their letters, in
/// hundredths of a row and of the text, by the keyboard's size
/// ([`KEY_SIZE`]). Their rows stay a computer keyboard's whatever the size:
/// the letters grow less, as ten keys a row leave them little room on a
/// narrow screen (the X4).
pub(crate) fn button_scale(size: &str) -> (u32, u32) {
    match size {
        SIZE_SMALL => (100, 100),
        SIZE_LARGE => (150, 110),
        _ => (125, 105),
    }
}

/// The least width of a letter's key, in pixels: tapped, what a finger
/// hits; chosen with buttons, room for its letter (`scale` hundredths of
/// the text) inside the focus frame, so that a row is split only where its
/// letters would not fit.
pub(crate) fn least_letter(env: &Env<'_>, style: &UiStyle, touch: bool, scale: u32) -> u32 {
    if touch {
        return env.metrics().tenth_mm(LEAST_TAP_MM * 10);
    }
    let px = style.text() * scale as f32 / 100.0;
    let widest = style.font(env).map_or(px, |font| font.measure(px, "M"));
    // Inside the key's outline and the seams beside it (see `key_inset`).
    widest as u32 + 2 * (ui::button_border(style) + u32::from(BUTTONS_SEAM))
}

/// Half the room between two keys, in pixels ((`KeyboardScreen::seam`)).
pub(crate) fn seam(env: &Env<'_>, style: &UiStyle, touch: bool) -> u16 {
    if touch {
        let half = env.metrics().tenth_mm(TOUCH_GAP_TENTHS) / 2;
        half.max(ui::button_border(style)) as u16
    } else {
        BUTTONS_SEAM
    }
}

/// The least room, in pixels, between the screen's edge and the keys'
/// visible edges: a thin margin, so that the keys look set in the screen.
pub(crate) const LEAST_SIDE: u16 = 4;

/// The room between the screen's edge and the keys' visible edges: a
/// quarter of the margin other screens keep, so that the keys get nearly
/// the whole width, but not less than the seam round a key (the key's tap
/// area then reaches the screen's edge) or [`LEAST_SIDE`].
pub(crate) fn side_margin(style: &UiStyle, seam: u16) -> u16 {
    (style.margin() / 4).max(LEAST_SIDE).max(seam)
}

/// The room between keys that are tapped, in tenths of a millimetre: as
/// much as keeps a finger on one key and shows them apart.
pub(crate) const TOUCH_GAP_TENTHS: u32 = 6;

/// Half the room between keys chosen with buttons, in pixels: the focus
/// fills a key, so they need no room for a frame round it, and the keys of
/// a computer's rows get the width (two pixels between keys).
pub(crate) const BUTTONS_SEAM: u16 = 1;

/// The least width of a letter's key where keys are tapped, in
/// millimetres: as much as a finger hits reliably.
pub(crate) const LEAST_TAP_MM: u32 = 4;

/// How high a key that is tapped is, in hundredths of a letter key's
/// width, by its shape ([`KEY_SHAPE`]): nearly round, round or a little
/// wider, so the keyboard leaves much of the screen above it in view.
pub(crate) fn height_of(shape: &str) -> u32 {
    match shape {
        SHAPE_ROUND => 100,
        SHAPE_WIDE => 85,
        _ => 110,
    }
}

/// How high the keyboard is where keys are tapped, in hundredths of its
/// height at Medium, by its size ([`KEY_SIZE`]): the keys take what
/// changes. With round keys on the X4 Pro and the Boox, the keyboard
/// then takes about 30 %, 40 % and 50 % of the screen.
pub(crate) fn size_of(size: &str) -> u32 {
    match size {
        SIZE_SMALL => 75,
        SIZE_LARGE => 125,
        _ => 100,
    }
}

/// How high the keys are where they are tapped, for keys `letter` wide,
/// `count` rows of them and `around` pixels of the keyboard that are not
/// keys. The shape gives the height at Medium, and the size scales the
/// whole keyboard from there.
pub(crate) fn tapped_height(
    env: &Env<'_>,
    style: &UiStyle,
    letter: u32,
    count: u16,
    around: u16,
) -> u16 {
    let shape = env.settings().read(&KEY_SHAPE);
    let border = 2 * ui::button_border(style) as u16;
    let touch_row = style.touch_row(u32::from(style.row())) as u16;
    let height = (letter * height_of(shape) / 100) as u16;
    let shaped = match shape {
        // As before: no higher than a touch row, as high as a line.
        SHAPE_TALL => height
            .min(touch_row)
            .max(paperpaw_sdk::widgets::line_height(env, style.text()) + border),
        // As high as wide; high enough for the letter itself.
        SHAPE_ROUND => height.max(style.text() as u16 + border),
        _ => height.min(touch_row).max(style.text() as u16 + border),
    };
    let count = count.max(1);
    let medium = u32::from(around) + u32::from(count) * u32::from(shaped);
    let target = medium * size_of(env.settings().read(&KEY_SIZE)) / 100;
    let sized = (target.saturating_sub(u32::from(around)) / u32::from(count)) as u16;
    // Larger keys stop where the keyboard would take more than half the
    // screen.
    let half = (env.device().display.height / 2).saturating_sub(around) / count;
    let sized = if sized > shaped {
        sized.min(half.max(shaped))
    } else {
        sized
    };
    // Never smaller than a finger hits, nor than the letter on the key.
    let least = (env.metrics().mm(LEAST_TAP_MM) as u16).max(style.text() as u16 + border);
    sized.max(least)
}

/// The units of a row of keys.
pub(crate) fn units(row: &[Slot]) -> u32 {
    row.iter().map(|(_, w)| u32::from(*w)).sum()
}

/// `row` in rows of at most `most` units, as even as can be and in their
/// order ("qwert" above "yuiop"); as it is where it fits. The rows are
/// added to `out`.
///
/// # Errors
/// [`Error::NoMemory`] when a list cannot grow.
fn try_split(
    row: Vec<Slot>,
    most: u32,
    out: &mut Vec<Vec<Slot>>,
) -> Result<(), paperpaw_sdk::native::Error> {
    let total: u32 = row.iter().map(|(_, w)| u32::from(*w)).sum();
    if most == 0 || total <= most {
        return mem::push(out, row);
    }
    let parts = total.div_ceil(most);
    let first = out.len();
    let mut used = 0;
    for slot in row {
        // A new row once this one holds its share.
        let part = (used * parts / total) as usize;
        if out.len() - first <= part {
            mem::push(out, Vec::new())?;
        }
        used += u32::from(slot.1);
        if let Some(last) = out.last_mut() {
            mem::push(last, slot)?;
        }
    }
    Ok(())
}

/// The slots of the letters in `row`, each one a key of two units.
fn try_chars(row: &str) -> Result<Vec<Slot>, paperpaw_sdk::native::Error> {
    let mut slots = Vec::new();
    slots
        .try_reserve_exact(row.len())
        .map_err(|_| paperpaw_sdk::native::Error::NoMemory)?;
    for c in row.chars() {
        mem::push(&mut slots, (Key::Char(c), 2))?;
    }
    Ok(slots)
}

/// The rows of a layer; `letters` are the rows of the layout, `switch`
/// adds the key for the next layout; rows wider than `most` units are
/// split (0 for none).
#[cfg(test)]
pub(crate) fn rows(letters: &str, layer: Layer, switch: bool, most: u32) -> Vec<Vec<Slot>> {
    rows_for(letters, layer, switch, most, false)
}

/// [`try_rows_for`] for the interface-6 keyboard: no rows if memory is
/// short.
pub(crate) fn rows_for(
    letters: &str,
    layer: Layer,
    switch: bool,
    most: u32,
    enter: bool,
) -> Vec<Vec<Slot>> {
    try_rows_for(letters, layer, switch, most, enter).unwrap_or_default()
}

/// The rows of a layer, with a key for a line break (`enter`) before
/// *Done* where text of several lines is typed.
///
/// # Errors
/// [`Error::NoMemory`] when a list cannot grow.
pub(crate) fn try_rows_for(
    letters: &str,
    layer: Layer,
    switch: bool,
    most: u32,
    enter: bool,
) -> Result<Vec<Vec<Slot>>, paperpaw_sdk::native::Error> {
    use paperpaw_sdk::native::Error as NoRoom;
    match layer {
        Layer::Letters | Layer::Symbols => {
            let mut lines: Vec<Vec<Slot>> = Vec::new();
            if layer == Layer::Letters {
                for row in letters.split_whitespace() {
                    mem::push(&mut lines, try_chars(row)?)?;
                }
            } else {
                for row in SYMBOLS {
                    mem::push(&mut lines, try_chars(row)?)?;
                }
            }
            if let Some(last) = lines.last_mut() {
                last.try_reserve(2).map_err(|_| NoRoom::NoMemory)?;
                if layer == Layer::Letters {
                    last.insert(0, (Key::Shift, 3));
                }
                last.push((Key::Delete, 3));
            }
            let mut rows: Vec<Vec<Slot>> = Vec::new();
            rows.try_reserve(lines.len() + 1)
                .map_err(|_| NoRoom::NoMemory)?;
            for row in lines {
                try_split(row, most, &mut rows)?;
            }
            let other = if layer == Layer::Letters {
                Layer::Symbols
            } else {
                Layer::Letters
            };
            let mut bottom: Vec<Slot> = Vec::new();
            bottom.try_reserve_exact(8).map_err(|_| NoRoom::NoMemory)?;
            bottom.push((Key::Layer(other), 3));
            // The key to the next layout as wide as the one to symbols, so
            // its code fits.
            if switch && layer == Layer::Letters {
                bottom.push((Key::NextLayout, 3));
            }
            // The space bar takes what leaves the row as wide as the
            // widest, five letters at least; "," and "." are as wide as a
            // letter.
            let widest = rows.iter().map(|row| units(row)).max().unwrap_or(20);
            let others = units(&bottom) + 2 + 2 + 3 + if enter { 3 } else { 0 };
            let least = if bottom.len() > 1 { 7 } else { 10 };
            let space = widest.saturating_sub(others).max(least) as u8;
            bottom.extend([
                (Key::Char(','), 2),
                (Key::Space, space),
                (Key::Char('.'), 2),
            ]);
            if enter {
                bottom.push((Key::Enter, 3));
            }
            bottom.push((Key::Done, 3));
            mem::push(&mut rows, bottom)?;
            Ok(rows)
        }
        Layer::Digits => {
            let mut rows: Vec<Vec<Slot>> = Vec::new();
            rows.try_reserve_exact(4).map_err(|_| NoRoom::NoMemory)?;
            for row in ["123", "456", "789"] {
                rows.push(try_chars(row)?);
            }
            let mut bottom: Vec<Slot> = Vec::new();
            bottom.try_reserve_exact(3).map_err(|_| NoRoom::NoMemory)?;
            bottom.extend([(Key::Delete, 2), (Key::Char('0'), 2), (Key::Done, 2)]);
            rows.push(bottom);
            Ok(rows)
        }
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    /// A row too wide for a narrow screen in two as even as can be, in
    /// its order; one that fits as it is.
    #[test]
    fn rows_too_wide_are_split_evenly_in_order() {
        let row = rows("qwertyuiop asdfghjkl zxcvbnm", Layer::Letters, false, 16);
        let names = |r: &[Slot]| {
            r.iter()
                .map(|(k, _)| match k {
                    Key::Char(c) => *c,
                    Key::Shift => '^',
                    Key::Delete => '<',
                    _ => '_',
                })
                .collect::<String>()
        };
        let shown: Vec<String> = row.iter().map(|r| names(r)).collect();
        assert_eq!(
            &shown[..6],
            ["qwert", "yuiop", "asdfg", "hjkl", "^zxcv", "bnm<"]
        );
        assert!(
            row[..6]
                .iter()
                .all(|r| r.iter().map(|(_, w)| u32::from(*w)).sum::<u32>() <= 16)
        );
        // Wide enough: the rows as they are.
        assert_eq!(
            rows("qwertyuiop asdfghjkl zxcvbnm", Layer::Letters, false, 40).len(),
            4
        );
    }
}

#[cfg(test)]
mod enter_tests {
    use super::*;

    /// A key for the line break only where lines are typed, before *Done*,
    /// and the space bar keeps its width.
    #[test]
    fn enter_is_a_key_of_multiline_text_only() {
        let letters = "qwertyuiop asdfghjkl zxcvbnm";
        for layer in [Layer::Letters, Layer::Symbols] {
            let plain = rows_for(letters, layer, false, 0, false);
            let lines = rows_for(letters, layer, false, 0, true);
            let has = |rows: &[Vec<Slot>]| rows.iter().flatten().any(|(k, _)| *k == Key::Enter);
            assert!(!has(&plain) && has(&lines));
            let bottom = lines.last().unwrap();
            assert_eq!(bottom[bottom.len() - 2].0, Key::Enter);
            assert_eq!(bottom.last().unwrap().0, Key::Done);
            // No wider than the rows above.
            let widest = lines.iter().map(|r| units(r)).max().unwrap();
            assert_eq!(units(bottom), widest);
        }
        assert_eq!(
            rows(letters, Layer::Letters, true, 0),
            rows_for(letters, Layer::Letters, true, 0, false)
        );
    }
}
