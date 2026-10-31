//! Choosing text on the page and what can be done with it.
//!
//! Touch: a long press on a word selects it and shows a bar at the bottom
//! (look up, highlight, copy, cancel); tapping more words extends the selection.
//! Buttons: *Select text* in the reader's menu, move to a word, confirm for
//! the bar, where *Extend* moves the end of the selection word by word.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{Color, Env, Id, Point, Rect};

use crate::SELECTION_MENU;
use crate::actions::{CANCEL, EXTEND, HIGHLIGHT, MORE, NOTE, TRANSLATE};
use crate::screen::WordBox;

/// What the buttons do with the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Moving the cursor; the selection is one word.
    Choosing,
    /// Moving the end; the start stays.
    Extending,
    /// The bar is shown.
    Bar,
}

/// An item of the bar: an action listed in the menu `reader.selection`
/// (see [`crate::SELECTION_MENU`]), so plugins and users can add, remove
/// and reorder them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    pub id: Id,
    /// Translation key of its label.
    pub title: String,
}

/// Selected words, as indexes into the page's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Selection {
    pub anchor: usize,
    pub cursor: usize,
    pub mode: Mode,
    /// The bar item focused by buttons (none after a touch).
    pub focus: Option<usize>,
}

impl Selection {
    /// One word, chosen by touch: the bar is shown at once.
    pub fn touched(word: usize) -> Self {
        Self {
            anchor: word,
            cursor: word,
            mode: Mode::Bar,
            focus: None,
        }
    }

    /// One word, to be moved with the buttons.
    pub fn choosing(word: usize) -> Self {
        Self {
            anchor: word,
            cursor: word,
            mode: Mode::Choosing,
            focus: None,
        }
    }

    /// The first and last selected word.
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }

    /// Moves the cursor for a navigation action; `None` if `action` is
    /// another one. Left and right go a word, up and down a line (the
    /// page buttons too, a device with two page buttons has no others).
    /// Words wrap round the page: left from the first word reaches the
    /// last, right from the last the first; lines stop at the ends.
    pub fn moved(&self, action: &Id, words: &[WordBox]) -> Option<usize> {
        let last = words.len().checked_sub(1)?;
        let a = action;
        Some(if [actions::LEFT, actions::PREVIOUS].contains(a) {
            self.cursor.checked_sub(1).unwrap_or(last)
        } else if [actions::RIGHT, actions::NEXT].contains(a) {
            if self.cursor >= last {
                0
            } else {
                self.cursor + 1
            }
        } else if [actions::UP, actions::PREV_PAGE].contains(a) {
            nearest_on_line(words, self.cursor, false)
        } else if [actions::DOWN, actions::NEXT_PAGE].contains(a) {
            nearest_on_line(words, self.cursor, true)
        } else {
            return None;
        })
    }
}

/// The bar's items: the selection menu's entries, as many as fit with
/// readable labels; if not all do, *More* before the last place opens the
/// whole menu as a list. *Cancel* always stays visible, at the end, since
/// tapping the page mostly hits words, which extends the selection.
/// *Highlight* reads *Remove highlight* on highlighted text (`highlight`
/// tells whether that has a note), *Note* reads *Edit note* on a noted
/// one. With buttons,
/// *Cancel* is left out (back cancels); on touch screens *Extend* is
/// (tapping words extends the selection).
pub(crate) fn items(env: &Env<'_>, highlight: Option<bool>) -> Vec<Item> {
    let translates = translates(env);
    let mut all: Vec<Item> = env
        .menus()
        .items(SELECTION_MENU.as_str())
        .into_iter()
        .filter(|item| {
            let touch = env.device().input.touch;
            !(*item.id == EXTEND && touch
                || *item.id == CANCEL && !touch
                || *item.id == TRANSLATE && !translates)
        })
        .map(|item| Item {
            id: item.id.clone(),
            title: if *item.id == HIGHLIGHT && highlight.is_some() {
                "reader.select.unhighlight".to_string()
            } else if *item.id == NOTE && highlight == Some(true) {
                "reader.select.edit-note".to_string()
            } else {
                item.title.to_string()
            },
        })
        .collect();
    let cancel: Vec<Item> = all
        .iter()
        .position(|item| item.id == CANCEL)
        .map(|at| all.remove(at))
        .into_iter()
        .collect();
    let style = UiStyle::new(env);
    let width = f32::from(items_area(env, &style).width);
    let fits = |items: &[Item]| natural_widths(env, &style, items).iter().sum::<f32>() <= width;
    if fits(&[all.as_slice(), &cancel].concat()) {
        all.extend(cancel);
        return all;
    }
    let more = Item {
        id: MORE,
        title: "reader.select.more".to_string(),
    };
    let mut shown: Vec<Item> = Vec::new();
    for item in &all {
        let mut next = shown.clone();
        next.push(item.clone());
        next.push(more.clone());
        next.extend(cancel.iter().cloned());
        if !fits(&next) {
            break;
        }
        shown.push(item.clone());
    }
    shown.push(more);
    shown.extend(cancel);
    shown
}

/// Whether selected text can be translated: a translator is installed
/// and there is a network.
pub(crate) fn translates(env: &Env<'_>) -> bool {
    env.caps().network && env.has_slot(&paperpaw_sdk::builtin::slots::TRANSLATE)
}

/// Where the bar is: over the status line at the bottom.
pub(crate) fn bar_area(env: &Env<'_>, style: &UiStyle) -> Rect {
    let display = env.device().display;
    // Its buttons grow on touch screens, and keep the side margin from
    // the bottom edge too.
    let height = ui::edge_bar_height(style);
    Rect::new(0, display.height - height, display.width, height)
}

/// Where the bar's items go: between the margins, the buttons' own room
/// in their parts included, so that what fits here is drawn whole.
fn items_area(env: &Env<'_>, style: &UiStyle) -> Rect {
    let full = bar_area(env, style);
    let side = (style.margin()).saturating_sub(button_pad(style));
    Rect::new(
        full.x + side,
        full.y,
        full.width.saturating_sub(2 * side),
        full.height,
    )
}

/// Space around a button in its part of the bar.
fn button_pad(style: &UiStyle) -> u16 {
    (u32::from(style.gap()) / 4) as u16 + ui::button_border(style) as u16 * 3
}

/// How wide the items need to be: their labels with the room a button
/// keeps around them (see [`ui::draw_button`]), so none is shortened.
fn natural_widths(env: &Env<'_>, style: &UiStyle, items: &[Item]) -> Vec<f32> {
    let font = style.font(env);
    // The button's room at each side of its label, and the part's own
    // room around the button: the spare width is shared out after.
    let around = 2.0 * f32::from(button_pad(style));
    items
        .iter()
        .map(|item| {
            let label = font.as_ref().map_or(0.0, |f| {
                f.measure(style.small(), env.i18n().tr(&item.title))
            });
            ui::button_width(style, label) + around
        })
        .collect()
}

/// The rectangles of the bar's items: each as wide as its label needs,
/// the rest of the bar shared equally.
fn item_areas(env: &Env<'_>, style: &UiStyle, items: &[Item]) -> Vec<Rect> {
    let bar = items_area(env, style);
    let natural = natural_widths(env, style, items);
    let spare =
        (f32::from(bar.width) - natural.iter().sum::<f32>()).max(0.0) / items.len().max(1) as f32;
    let mut left = f32::from(bar.x);
    let mut areas = Vec::with_capacity(items.len());
    for (index, width) in natural.iter().enumerate() {
        let right = if index + 1 == natural.len() {
            f32::from(bar.x) + f32::from(bar.width)
        } else {
            left + width + spare
        };
        let (x, end) = ((left + 0.5) as u16, (right + 0.5) as u16);
        areas.push(Rect::new(x, bar.y, end.saturating_sub(x), bar.height));
        left = right;
    }
    areas
}

pub(crate) fn draw_bar(ui: &mut ui::Ui<'_>, items: &[Item], focus: Option<usize>) {
    let (env, style) = (ui.env(), *ui.style());
    let bar = bar_area(env, &style);
    ui.canvas().fill(bar, Color::WHITE);
    let line = u32::from(style.line()).max(2);
    ui.canvas().fill_rect(
        i32::from(bar.x),
        i32::from(bar.y),
        u32::from(bar.width),
        line,
        Color::BLACK,
    );
    let areas = item_areas(env, &style, items);
    for (index, (item, &area)) in items.iter().zip(&areas).enumerate() {
        // Buttons as everywhere; the whole part stays the target of taps.
        let band = ui::edge_bar_band(&style, bar, false);
        let pad = button_pad(&style);
        let button = Rect::new(
            area.x + pad,
            band.y,
            area.width.saturating_sub(2 * pad),
            band.height,
        );
        let label = env.i18n().tr(&item.title);
        ui.button(button, label, false, focus == Some(index));
    }
}

/// The bar item at `point`, if any.
pub(crate) fn item_at(env: &Env<'_>, items: &[Item], point: Point) -> Option<Item> {
    let style = UiStyle::new(env);
    if !bar_area(env, &style).contains(point) {
        return None;
    }
    let bar = bar_area(env, &style);
    let mut areas = item_areas(env, &style, items);
    // The first and last parts reach to the bar's ends: a tap beside
    // the margins is theirs.
    if let Some(first) = areas.first_mut() {
        let right = first.right();
        first.x = bar.x;
        first.width = (right - u32::from(bar.x)) as u16;
    }
    if let Some(last) = areas.last_mut() {
        last.width = (bar.right() - u32::from(last.x)) as u16;
    }
    let index = areas.iter().position(|area| area.contains(point))?;
    items.get(index).cloned()
}

/// Boxes around runs of words on one line, e.g. to draw a selection or
/// highlight as one shape per line.
pub(crate) fn line_boxes(words: &[WordBox], mut picked: impl FnMut(usize) -> bool) -> Vec<Rect> {
    let mut boxes: Vec<Rect> = Vec::new();
    let mut previous: Option<usize> = None;
    for (index, word) in words.iter().enumerate() {
        if !picked(index) {
            previous = None;
            continue;
        }
        match boxes.last_mut() {
            Some(last) if previous == Some(index.wrapping_sub(1)) && last.y == word.area.y => {
                last.width = word.area.right().saturating_sub(u32::from(last.x)) as u16;
            }
            _ => boxes.push(word.area),
        }
        previous = Some(index);
    }
    boxes
}

/// The word on the line above (or below) word `index` nearest to it.
fn nearest_on_line(words: &[WordBox], index: usize, below: bool) -> usize {
    let Some(current) = words.get(index).map(|w| &w.area) else {
        return index;
    };
    let center = |r: &Rect| i32::from(r.x) + i32::from(r.width) / 2;
    let line = words
        .iter()
        .map(|w| w.area.y)
        .filter(|&y| if below { y > current.y } else { y < current.y })
        .min_by_key(|&y| y.abs_diff(current.y));
    let Some(line) = line else {
        return index;
    };
    words
        .iter()
        .enumerate()
        .filter(|(_, w)| w.area.y == line)
        .min_by_key(|(_, w)| center(&w.area).abs_diff(center(current)))
        .map_or(index, |(i, _)| i)
}

#[cfg(test)]
mod moving_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::document::Locator;

    #[test]
    fn a_cursor_past_the_words_moves_without_panicking() {
        let word = |x: u16, y: u16| {
            WordBox::new(
                Rect::new(x, y, 10, 10),
                "word",
                Locator::START,
                Locator::START,
            )
        };
        let words = [word(0, 0), word(20, 0), word(0, 20)];
        // Chosen on a page with more words than this one.
        let selection = Selection::choosing(7);
        for action in [actions::UP, actions::DOWN] {
            assert_eq!(selection.moved(&action, &words), Some(7));
        }
        assert_eq!(
            Selection::choosing(1).moved(&actions::DOWN, &words),
            Some(2)
        );
    }
}
