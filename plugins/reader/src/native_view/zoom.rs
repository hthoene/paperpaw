//! A picture of the page on its own, full screen (#92): holding Select on a
//! page with a picture (touch: holding the picture) shows the first picture
//! fit to the whole screen; Left and Right (page buttons, swipes) step through
//! the page's pictures, Select, Back or a tap return to the page, which was
//! never left: its place, words and pictures are as they were.
//!
//! The view draws the picture the page already has (`canvas.draw_image` into
//! the whole frame, scaled to fit): nothing is decoded again, and the host's
//! picture rule shows it in black and white first and in grays once the
//! device has rested. Zoom and pan inside a picture, and turning wide pictures,
//! need the host (a larger decode drawn into a rectangle that is turned) and
//! wait for interface 8 (#110).
use super::{Body, Open, View, redraw};
use crate::native_draw::{picture_boxes, picture_of};
use paperpaw_sdk::{
    Color, Handled, Point,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input, InputGestureKind},
        commands::RedrawMode,
        document::Documents,
        image::{ImageId, ImageStyle},
    },
};

/// The action that holding Select is bound to: the picture of the page full
/// screen where it has one, else the bookmark (what holding Select did before).
pub const HOLD: &str = "reader.hold-select";

impl View {
    /// What the full-screen picture takes of an input: opens on a hold, and
    /// while open takes the keys that step or close; `None` for any other.
    pub(super) fn zoom(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let Body::Open(open) = &mut self.body else {
            return Ok(None);
        };
        if open.zoom.is_some() {
            return open.zoom_input(host, docs, input).map(Some);
        }
        open.zoom_entry(host, docs, input)
    }
}

impl Open {
    /// The pictures of the page the host can draw: the place on the page and
    /// the host's id of each.
    fn viewable(&self) -> impl Iterator<Item = (usize, u32)> + '_ {
        self.page
            .iter()
            .flat_map(|page| page.images.iter().enumerate())
            .filter_map(|(at, image)| match picture_of(&image.image.path) {
                Some((handle @ 1.., _)) => Some((at, handle)),
                _ => None,
            })
    }

    /// The picture under `at` (a touch), where the page has one the host can
    /// draw.
    fn picture_at(&self, host: &Host<'_>, at: Point) -> Option<usize> {
        let page = self.page.as_ref()?;
        let area = self
            .settings
            .setup(host)
            .ok()
            .and_then(|setup| self.area(&setup))?;
        let origin = (f32::from(area.x), f32::from(area.y));
        let found = picture_boxes(page, origin).position(|b| inside(b, (at.x, at.y)))?;
        self.viewable().find(|(i, _)| *i == found).map(|(i, _)| i)
    }

    /// A hold opens the view: Select (the first picture of the page) or a
    /// touch on a picture. `None` where there is nothing to show: a hold on
    /// text goes on to choosing a word, and a page without pictures to the
    /// bookmark. Books whose pages are pictures keep their pages.
    pub(super) fn zoom_entry(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let touched = match *input {
            Input::Action { action: HOLD, .. } => None,
            Input::Gesture {
                kind: InputGestureKind::LongPress,
                position,
                ..
            } => Some(position),
            _ => return Ok(None),
        };
        if self.whole_pages || !self.has_pictures() {
            return Ok(None);
        }
        // A picture without a live id is asked for now, as the page does.
        self.ask_missing(host, docs);
        let first = match touched {
            None => self.viewable().next().map(|(at, _)| at),
            Some(at) => self.picture_at(host, at),
        };
        let Some(first) = first else {
            return Ok(None);
        };
        self.zoom = Some(first);
        redraw(host, RedrawMode::Partial)?;
        Ok(Some(Handled::Yes))
    }

    /// An input while the picture is on show: Left and Right (and the page
    /// buttons) step, Select, Menu, Back and a tap return to the page; any
    /// other action (home, the quick panel) is the host's.
    fn zoom_input(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let forward = match *input {
            Input::Action { action, .. } => match key(action) {
                Key::Close => return self.zoom_close(host, docs),
                Key::Step(forward) => forward,
                Key::Other => return Ok(Handled::No),
            },
            Input::Gesture {
                kind: InputGestureKind::Tap,
                ..
            } => return self.zoom_close(host, docs),
            // Holds and swipes the host did not map do nothing here.
            Input::Gesture { .. } => return Ok(Handled::Yes),
            _ => return Ok(Handled::No),
        };
        let here = self.zoom.unwrap_or(0);
        let other = stepped(self.viewable().map(|(at, _)| at), here, forward);
        if let Some(other) = other {
            self.ask_missing(host, docs);
            self.zoom = Some(other);
            redraw(host, RedrawMode::Partial)?;
        }
        Ok(Handled::Yes)
    }

    /// Back to the page, drawn again as a page turn shows it.
    fn zoom_close(&mut self, host: &Host<'_>, docs: &Documents) -> Result<Handled, Error> {
        self.zoom = None;
        self.redraw_page(host, docs, RedrawMode::Partial)?;
        Ok(Handled::Yes)
    }

    /// Draws the picture on show over the whole frame, fit to the screen;
    /// false where there is none (the page changed under it: the view ends
    /// and the page is drawn).
    ///
    /// # Errors
    /// The host's refusal to draw.
    pub(super) fn draw_zoom(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        frame: &mut DrawFrame<'_>,
    ) -> Result<bool, Error> {
        let Some(at) = self.zoom else {
            return Ok(false);
        };
        let Some(image) = self.zoomed(at) else {
            self.zoom = None;
            return Ok(false);
        };
        let (rect, counting) = (frame.rect, frame.pictures_only());
        let canvas = frame.draw_canvas()?;
        // The count of the host's picture rule draws the picture alone.
        if !counting {
            canvas.fill_rectangle(rect, Color::WHITE)?;
        }
        match canvas.draw_image(image, rect, ImageStyle::new()) {
            Err(Error::NotFound) => {
                // The host ended the id: asked for again, drawn the next frame.
                self.ask_missing(host, docs);
                redraw(host, RedrawMode::Partial)
            }
            Err(Error::Unsupported) => Ok(()),
            other => other,
        }?;
        Ok(true)
    }

    /// The host's id of the picture at `at` on the page, while it has one.
    fn zoomed(&self, at: usize) -> Option<ImageId> {
        let image = self.page.as_ref()?.images.get(at)?;
        match picture_of(&image.image.path)? {
            (handle @ 1.., _) => ImageId::new(handle).ok(),
            _ => None,
        }
    }
}

/// What a key does while the picture is on show.
#[derive(Debug, PartialEq, Eq)]
enum Key {
    /// Back to the page.
    Close,
    /// The next picture (`true`) or the one before.
    Step(bool),
    /// Not the view's: the host's.
    Other,
}

fn key(action: &str) -> Key {
    match action {
        "core.back" | "core.menu" | "core.confirm" => Key::Close,
        "core.left" | "core.previous" | "core.up" | "core.prev-page" => Key::Step(false),
        "core.right" | "core.next" | "core.down" | "core.next-page" => Key::Step(true),
        _ => Key::Other,
    }
}

/// The place of the picture after (`forward`) or before `here` among
/// `places`, in order; none at the ends of the page.
fn stepped(mut places: impl Iterator<Item = usize>, here: usize, forward: bool) -> Option<usize> {
    if forward {
        places.find(|at| *at > here)
    } else {
        places.filter(|at| *at < here).last()
    }
}

/// Whether the point is in the box `(x, y, width, height)`.
fn inside((x, y, width, height): (i32, i32, u32, u32), (px, py): (i32, i32)) -> bool {
    px >= x
        && py >= y
        && i64::from(px) < i64::from(x) + i64::from(width)
        && i64::from(py) < i64::from(y) + i64::from(height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holding_select_is_the_views_action_and_the_keys_step_or_close() {
        assert_eq!(HOLD, "reader.hold-select");
        assert_eq!(key("core.back"), Key::Close);
        assert_eq!(key("core.confirm"), Key::Close);
        assert_eq!(key("core.left"), Key::Step(false));
        assert_eq!(key("core.right"), Key::Step(true));
        // Home and the panels stay the host's.
        assert_eq!(key("core.home"), Key::Other);
    }

    #[test]
    fn the_pictures_of_a_page_are_stepped_through_and_stop_at_the_ends() {
        // Pictures 0 and 2 can be drawn; 1 has no id.
        let places = || [0, 2].into_iter();
        assert_eq!(stepped(places(), 0, true), Some(2));
        assert_eq!(stepped(places(), 2, false), Some(0));
        assert_eq!(stepped(places(), 2, true), None);
        assert_eq!(stepped(places(), 0, false), None);
    }

    #[test]
    fn a_touch_hits_the_picture_it_is_in() {
        let b = (40, 100, 300, 200);
        assert!(inside(b, (40, 100)));
        assert!(inside(b, (339, 299)));
        assert!(!inside(b, (340, 150)));
        assert!(!inside(b, (100, 300)));
        assert!(!inside(b, (39, 150)));
    }
}
