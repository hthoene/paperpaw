//! A field that searches: [`SearchField`].

use paperpaw_platform::Rect;

use crate::widgets::Align;

use super::{Ink, Label, Ui, UiStyle, focus_rect, icon_room};

/// A field that searches, as every screen shows one: a rounded outline
/// round the search icon at its start and a hint in gray (or what was
/// searched for); focused, the focus with both in its ink. The library's
/// search, the settings' and the store's are drawn with it, so they look
/// the same, in every theme: its inks are the theme's
/// ([`Ink::TEXT`] for the outline, icon and text, [`Ink::DETAIL`] for
/// the hint) and its sizes the style's. Choosing it usually asks for the
/// text ([`crate::ask::text`]); where the interface runs right to left
/// the icon is at the right.
///
/// What it shows is described as text, as a row's content is: the list
/// it is drawn in describes its row ([`super::Rows`]), and a screen that
/// places it alone describes a button round it
/// (`canvas.describe(Role::Button, "", rect)`), which takes its text.
///
/// It borrows its words; drawing it allocates nothing.
///
/// ```
/// use paperpaw_sdk::ui::SearchField;
///
/// let empty = SearchField::new("Search title or author").focused(true);
/// let typed = SearchField::new("Search").text("tolstoy");
/// # let _ = (empty, typed);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a search field does nothing until it is drawn"]
pub struct SearchField<'s> {
    hint: &'s str,
    text: &'s str,
    focused: bool,
}

impl<'s> SearchField<'s> {
    /// A field showing `hint` until something is searched for.
    pub const fn new(hint: &'s str) -> Self {
        Self {
            hint,
            text: "",
            focused: false,
        }
    }

    /// Showing `text`, what is searched for, instead of the hint (the
    /// hint where it is empty).
    pub const fn text(mut self, text: &'s str) -> Self {
        self.text = text;
        self
    }

    /// Focused (or not): drawn on the focus, in its ink.
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// How high a field is with `style`: a row, as high as a fingertip
    /// where the screen is touched ([`UiStyle::touch_row`]).
    pub fn height(style: &UiStyle) -> u16 {
        let height = style.touch_row(u32::from(style.row()));
        u16::try_from(height).unwrap_or(u16::MAX)
    }

    /// Draws it in `rect`, a row of the screen's width (its outline
    /// within the margins, where the focus of a row goes).
    pub fn draw(&self, ui: &mut Ui<'_>, rect: Rect) {
        let (env, style) = (ui.env(), ui.style());
        let focused = self.focused;
        let typed = !self.text.is_empty();
        let shown = if typed { self.text } else { self.hint };
        if focused {
            ui.focus(rect);
        } else {
            let field = focus_rect(style, rect);
            let radius = style.radius(u32::from(field.height));
            let line = u32::from(style.line()).max(1);
            let ink = style.ink(Ink::TEXT);
            ui.canvas().stroke_round(field, line, radius, ink);
        }
        let ink = style.focus_ink(focused);
        let room = icon_room(env, style, true);
        let rest = ui.row_icon(rect, room, Some("search"), ink);
        let margin = style.margin();
        let words = Rect::new(
            rest.x + margin,
            rest.y,
            rest.width.saturating_sub(2 * margin),
            rest.height,
        );
        let color = if typed {
            ink
        } else {
            style.focus_faint(focused, Ink::DETAIL)
        };
        let align = if style.is_rtl() {
            Align::Right
        } else {
            Align::Left
        };
        Label::new(shown)
            .px(style.text())
            .color(color)
            .align(align)
            .draw(ui, words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_replaces_the_hint_only_when_there_is_some() {
        let field = SearchField::new("Search");
        assert_eq!(field.text(""), field);
        assert_ne!(field.text("a"), field);
        assert!(!field.focused);
        assert!(field.focused(true).focused);
    }
}
