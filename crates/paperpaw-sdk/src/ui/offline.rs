//! The offer to go online, on a screen the reader opened for the network
//! (the store, a catalog, a transfer) while the power mode is Offline
//! (see [`power`](crate::power)): once, inline in the screen's own
//! layout, never as a pop-up over the page: "Offline mode. Go online for
//! this?" with *Go online* and *Not now*.
//!
//! *Go online* sends `Command::GoOnline`: the
//! network for that screen until it closes; the screen then does again
//! what needed the network. *Not now* leaves the screen as it is without
//! the network (it says so as it would without one). Either way it is
//! not offered again on that screen.
//!
//! Outside a screen of the network (a widget's *Update now*, a plugin's
//! own offer for what its background work would bring) the reader's
//! choice is `Command::GoOnlineFor`: the network for a while, whatever
//! shows.
//!
//! ```ignore
//! if self.offer.shows(env) {
//!     self.offer.draw(&mut ui, area);
//! }
//! // In `on_action` and `on_gesture`:
//! if let Some(Offer::Online) = self.offer.on_action(action, cx) {
//!     self.fetch(cx);
//! }
//! ```

use crate::power::PowerLevel;
use crate::{Env, Rect};

use super::{UiStyle, button_height, button_height_for, draw_button, draw_empty};

/// The keys of its buttons' words, *Go online* first.
const BUTTONS: [&str; 2] = ["core.offline.go-online", "core.offline.not-now"];

/// Where the offer's buttons go in an area `width` wide: each in a row
/// of its own, two margins in from the sides; where a word does not fit
/// so, one margin in, then a line higher for each more line it takes.
struct Buttons {
    /// The touch row each lies in, and a button's height in it.
    row: u16,
    height: u16,
    /// From the area's side, and how wide.
    inset: u16,
    width: u16,
    /// How much higher each is (and its row) for a word on more lines.
    extra: [u16; 2],
    /// The room the two take with the gaps around them.
    room: u16,
}

impl Buttons {
    fn new(env: &Env<'_>, style: &UiStyle, width: u16) -> Self {
        let row = style.touch_row(style.control_row) as u16;
        let height = button_height(style, Rect::new(0, 0, width, row));
        let margin = style.margin as u16;
        let gap = style.gap as u16 * 2;
        let fits = |width: u16| {
            BUTTONS
                .iter()
                .all(|key| button_height_for(env, style, width, env.i18n().tr(key)) <= height)
        };
        let narrow = width.saturating_sub(4 * margin);
        let (inset, width) = if fits(narrow) {
            (2 * margin, narrow)
        } else {
            (margin, width.saturating_sub(2 * margin))
        };
        let extra = BUTTONS.map(|key| {
            button_height_for(env, style, width, env.i18n().tr(key)).saturating_sub(height)
        });
        Buttons {
            row,
            height,
            inset,
            width,
            extra,
            room: 2 * row + extra[0] + extra[1] + gap,
        }
    }
}

/// What the reader chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Offer {
    /// *Go online*: `Command::GoOnline` was sent; do again what needed
    /// the network.
    Online,
    /// *Not now*: go on without the network.
    NotNow,
}

/// The offer on one screen: where its buttons are, which has the focus,
/// whether the reader chose already.
#[derive(Debug, Clone, Default)]
pub struct OfflineOffer {
    buttons: [Rect; 2],
    focus: usize,
    chosen: bool,
}

impl OfflineOffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether to show it: the mode is Offline and the reader has not
    /// chosen on this screen yet.
    pub fn shows(&self, env: &Env<'_>) -> bool {
        env.power().level == PowerLevel::Offline && !self.chosen
    }

    /// Draws it in `area`: what it is about, as an empty screen says it,
    /// and the two buttons below, *Go online* first.
    pub fn draw(&mut self, ui: &mut super::Ui<'_>, area: Rect) {
        let (env, style) = (ui.env(), ui.style());
        let canvas = ui.canvas();
        let buttons = Buttons::new(env, style, area.width);
        let gap = style.gap as u16 * 2;
        let text = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.saturating_sub(buttons.room + gap),
        );
        draw_empty(
            canvas,
            env,
            style,
            text,
            "wifi-off",
            env.i18n().tr("core.offline.title"),
            env.i18n().tr("core.offline.text"),
        );
        let mut top = text.y + text.height + gap;
        for (index, key) in BUTTONS.into_iter().enumerate() {
            let extra = buttons.extra[index];
            let y = top + (buttons.row - buttons.height) / 2;
            let rect = Rect::new(
                area.x + buttons.inset,
                y,
                buttons.width,
                buttons.height + extra,
            );
            top += buttons.row + extra + gap / 2;
            self.buttons[index] = rect;
            let focused = super::focus_shown(env) && self.focus == index;
            draw_button(
                canvas,
                env,
                style,
                rect,
                env.i18n().tr(key),
                index == 0,
                focused,
            );
        }
    }
}
