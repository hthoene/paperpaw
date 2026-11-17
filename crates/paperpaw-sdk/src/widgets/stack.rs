//! Widgets above each other, as a place of kind
//! [`Stack`](super::PlaceKind::Stack) shows them (the sleep screen's).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_graphics::Canvas;
use paperpaw_platform::Rect;

use super::{FILL, Widget, WidgetTarget, fill_height};
use crate::env::Env;

/// Widgets listed in a setting (ids separated by commas), above each
/// other between the margins of an area: each as high as it needs, two
/// gaps between them, together in the middle or where free spaces
/// ([`FILL`]) push them. Those that do not fit below the others are left
/// out, as are those with nothing to show. The stack keeps the widgets
/// it made: `update` reads them anew, [`draw`](Self::draw)
/// draws them.
pub struct WidgetStack {
    widgets: Vec<Shown>,
}

/// A widget made for a stack, with its id and its height.
type Made = (String, Box<dyn Widget>, u16);

/// A widget of a stack, with its id and where it goes.
struct Shown {
    id: String,
    widget: Box<dyn Widget>,
    area: Rect,
}

impl fmt::Debug for WidgetStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<&str> = self.widgets.iter().map(|w| w.id.as_str()).collect();
        f.debug_struct("WidgetStack")
            .field("widgets", &ids)
            .finish()
    }
}

impl WidgetStack {
    /// The widgets of `list` in `area`, what they show read.
    pub fn new(env: &Env<'_>, list: &str, area: Rect) -> Self {
        let style = crate::ui::UiStyle::new(env);
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let width = area.width.saturating_sub(2 * margin);
        // Free spaces (`None`) share the room the others leave.
        let mut made: Vec<Option<Made>> = Vec::new();
        let mut total = 0u16;
        let mut shown = 0;
        for id in crate::status::item_ids(list) {
            if id == FILL {
                made.push(None);
                continue;
            }
            let Some(widget) = env.widgets().make(id, env) else {
                continue;
            };
            let height = widget.height(env, width);
            if height == 0 {
                continue;
            }
            let added = height + if shown == 0 { 0 } else { gap * 2 };
            if total + added > area.height {
                break;
            }
            total += added;
            shown += 1;
            made.push(Some((String::from(id), widget, height)));
        }
        if shown == 0 {
            return Self {
                widgets: Vec::new(),
            };
        }
        let fills = made.iter().filter(|m| m.is_none()).count();
        let fill = fill_height(u32::from(area.height), u32::from(total), fills);
        let mut y = area.y
            + if fills == 0 {
                (area.height - total) / 2
            } else {
                0
            };
        let mut widgets = Vec::new();
        for made in made {
            let Some((id, mut widget, height)) = made else {
                y += fill;
                continue;
            };
            if !widgets.is_empty() {
                y += gap * 2;
            }
            let at = Rect::new(area.x + margin, y, width, height);
            widget.place(env, at);
            widget.context(
                env,
                &super::WidgetContext::new(super::SizeClass::of(env, at)),
            );
            widgets.push(Shown {
                id,
                widget,
                area: at,
            });
            y += height;
        }
        Self { widgets }
    }

    /// Whether no widget shows.
    pub fn is_empty(&self) -> bool {
        self.widgets.is_empty()
    }

    /// The ids of the widgets shown, in their order (for
    /// [`refresh`](super::refresh)).
    pub fn ids(&self) -> Vec<String> {
        self.widgets.iter().map(|w| w.id.clone()).collect()
    }

    /// Draws the widgets.
    pub fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        for shown in &self.widgets {
            shown.widget.draw(canvas, env, shown.area);
        }
    }

    /// The places in the widgets that do something, in reading order.
    pub fn targets(&self, env: &Env<'_>) -> Vec<WidgetTarget> {
        self.widgets
            .iter()
            .flat_map(|shown| shown.widget.targets(env, shown.area))
            .collect()
    }
}
