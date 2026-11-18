//! A picture of the page on its own, full screen (#92): holding Select on a
//! page with a picture (touch: holding the picture) shows the first picture
//! fit to the whole screen. Left and Right (page buttons, swipes) step through
//! the page's pictures; Back returns to the page, which was never left: its
//! place, words and pictures are as they were.
//!
//! Select cycles the zoom, fit, 2x, 3x (touch: a double tap). Zoomed, Left and
//! Right pan sideways and the page buttons (swipes) go on in reading order,
//! across and then down, by four fifths of the screen, clamped at the
//! picture's edges; Back returns to fit, and from fit to the page (touch: hold
//! to leave). The page's picture is not decoded again for fit; a zoom asks the
//! document for the picture at the size it needs (`ask_image` with a side,
//! at most [`SIDE_MAX`]) and draws it into a rectangle larger than the screen,
//! which the host clips: nothing is held in memory but the host's one drawing
//! block. A picture the host cannot decode larger (no memory, too big) stays at
//! fit with a short note. The host's picture rule shows every frame in black
//! and white first and in grays once the device has rested.
use super::{Body, Open, View, helpers::note, redraw};
use crate::native_draw::{picture_boxes, picture_of};
use paperpaw_sdk::{
    Color, Handled, Point,
    native::{
        Error, Host,
        callbacks::{DrawFrame, FrameRect, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        document::Documents,
        image::{ImageId, ImageInfo, ImageStyle, SIDE_MAX},
    },
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// The action that holding Select is bound to: the picture of the page full
/// screen where it has one, else the bookmark (what holding Select did before).
pub const HOLD: &str = "reader.hold-select";

/// The zoom levels: fit, 2x, 3x (the level is the factor less one).
const LEVELS: u8 = 3;
/// The note that a picture cannot be shown larger, and the rows it takes.
const TOO_BIG: &str = "reader.zoom.too-big";
const NOTE_ROWS: u16 = 64;
/// Two taps this close in time (ms) and place (pixels) are a double tap.
const DOUBLE_MS: u64 = 500;
const DOUBLE_PX: i32 = 80;

/// The picture drawn larger than fit: the host's id of its larger decode.
#[derive(Debug, Clone, Copy)]
struct Big {
    image: ImageId,
    /// The id is the view's own (the host gave a new one), not the page's.
    owned: bool,
}

/// The picture on show.
#[derive(Debug)]
pub(super) struct Zoom {
    /// Its place on the page.
    at: usize,
    /// 0 fit, 1 2x, 2 3x.
    level: u8,
    /// The highest level the host could decode (a picture too big stops
    /// the cycle below it).
    most: u8,
    /// Where the picture's top left is, from the top left of the frame, and
    /// how large it is drawn there (zoomed only).
    pan: (i32, i32),
    size: (i32, i32),
    big: Option<Big>,
    /// A zoom was refused: the note shows until the next input.
    note: bool,
    /// The last tap, for a double tap.
    tap: Option<(u64, Point)>,
}

impl Zoom {
    const fn new(at: usize) -> Self {
        Self {
            at,
            level: 0,
            most: LEVELS - 1,
            pan: (0, 0),
            size: (0, 0),
            big: None,
            note: false,
            tap: None,
        }
    }

    /// Gives back the larger decode, if the view asked for one.
    fn release(&mut self, host: &Host<'_>) {
        if let Some(Big {
            image, owned: true, ..
        }) = self.big.take()
        {
            let mut scratch = [0; 32];
            CommandSender::new(host, &mut scratch)
                .image_release(image)
                .ok();
        }
    }
}

impl View {
    /// What the full-screen picture takes of an input: opens on a hold, and
    /// while open takes the keys that step, zoom, pan or close; `None` for any
    /// other.
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
        self.zoom = Some(Zoom::new(first));
        redraw(host, RedrawMode::Partial)?;
        Ok(Some(Handled::Yes))
    }

    /// An input while the picture is on show; any action the view does not
    /// know (home, the quick panel) is the host's.
    fn zoom_input(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(zoom) = self.zoom.as_mut() else {
            return Ok(Handled::No);
        };
        zoom.note = false;
        let (level, most) = (zoom.level, zoom.most);
        let key = match *input {
            Input::Action { action, .. } => key(action),
            Input::Gesture {
                kind: InputGestureKind::Tap,
                now_ms,
                position,
            } => {
                if !double_tap(&mut zoom.tap, now_ms, position) {
                    return Ok(Handled::Yes);
                }
                Key::Cycle
            }
            // A hold leaves, as a hold opened; other gestures the host did
            // not map do nothing here.
            Input::Gesture {
                kind: InputGestureKind::LongPress,
                ..
            } => Key::Close,
            Input::Gesture { .. } => return Ok(Handled::Yes),
            _ => return Ok(Handled::No),
        };
        match key {
            Key::Other => return Ok(Handled::No),
            Key::Close => return self.zoom_close(host, docs),
            Key::Back if level == 0 => return self.zoom_close(host, docs),
            Key::Back => self.zoom_to(host, docs, 0),
            Key::Cycle => self.zoom_to(host, docs, next_level(level, most)),
            _ if level == 0 => self.zoom_step(host, docs, &key),
            _ => self.zoom_pan(host, &key),
        }
        redraw(host, RedrawMode::Partial)?;
        Ok(Handled::Yes)
    }

    /// Another picture of the page, at fit.
    fn zoom_step(&mut self, host: &Host<'_>, docs: &Documents, key: &Key) {
        let Some(zoom) = self.zoom.as_ref() else {
            return;
        };
        let here = zoom.at;
        let places = self.viewable().map(|(at, _)| at);
        let other = stepped(
            places,
            here,
            matches!(key, Key::Right | Key::Down | Key::Next),
        );
        if let Some(other) = other {
            self.ask_missing(host, docs);
            self.zoom = Some(Zoom::new(other));
        }
    }

    /// Pans the zoomed picture: four fifths of the screen, kept in the frame.
    fn zoom_pan(&mut self, host: &Host<'_>, key: &Key) {
        let (Some(screen), Some(zoom)) = (self.screen(host), self.zoom.as_mut()) else {
            return;
        };
        let (dx, dy) = (screen.0 * 4 / 5, screen.1 * 4 / 5);
        zoom.pan = match key {
            Key::Left => panned(zoom.pan, (dx, 0), zoom.size, screen),
            Key::Right => panned(zoom.pan, (-dx, 0), zoom.size, screen),
            Key::Up => panned(zoom.pan, (0, dy), zoom.size, screen),
            Key::Down => panned(zoom.pan, (0, -dy), zoom.size, screen),
            Key::Prev => scanned(zoom.pan, (dx, dy), zoom.size, screen, false),
            Key::Next => scanned(zoom.pan, (dx, dy), zoom.size, screen, true),
            _ => zoom.pan,
        };
    }

    /// The screen's size in pixels.
    fn screen(&self, host: &Host<'_>) -> Option<(i32, i32)> {
        let setup = self.settings.setup(host).ok()?;
        Some((
            i32::from(setup.display.display_width()),
            i32::from(setup.display.display_height()),
        ))
    }

    /// The zoom `level`: fit gives the larger decode back; a level asks the
    /// document for the picture at the side it is drawn at and keeps the
    /// middle of the view where it was. A picture the host cannot give that
    /// large stays where it is, with the note, and the cycle stops below it.
    fn zoom_to(&mut self, host: &Host<'_>, docs: &Documents, level: u8) {
        let Some(screen) = self.screen(host) else {
            return;
        };
        let Some((at, was)) = self.zoom.as_ref().map(|zoom| (zoom.at, zoom.level)) else {
            return;
        };
        if level == 0 {
            if let Some(zoom) = self.zoom.as_mut() {
                zoom.release(host);
                zoom.level = 0;
            }
            return;
        }
        let Some(image) = self.page.as_ref().and_then(|page| page.images.get(at)) else {
            return;
        };
        let outcome = enlarge(host, docs, self.id, &image.image.path, (screen, level));
        let Some(zoom) = self.zoom.as_mut() else {
            return;
        };
        let Ok((big, fit)) = outcome else {
            zoom.note = true;
            zoom.most = was;
            return;
        };
        let size = zoomed_size(fit, level);
        let (from, old) = if was == 0 {
            (((screen.0 - fit.0) / 2, (screen.1 - fit.1) / 2), fit)
        } else {
            (zoom.pan, zoom.size)
        };
        zoom.pan = recentred(from, old, size, screen);
        zoom.size = size;
        zoom.level = level;
        if let Some(old) = zoom.big.replace(big)
            && old.owned
            && old.image != big.image
        {
            let mut scratch = [0; 32];
            CommandSender::new(host, &mut scratch)
                .image_release(old.image)
                .ok();
        }
    }

    /// Back to the page, drawn again as a page turn shows it.
    fn zoom_close(&mut self, host: &Host<'_>, docs: &Documents) -> Result<Handled, Error> {
        if let Some(mut zoom) = self.zoom.take() {
            zoom.release(host);
        }
        self.redraw_page(host, docs, RedrawMode::Partial)?;
        Ok(Handled::Yes)
    }

    /// Draws the picture on show over the whole frame, fit to the screen or
    /// zoomed; false where there is none (the page changed under it: the
    /// view ends and the page is drawn).
    ///
    /// # Errors
    /// The host's refusal to draw.
    pub(super) fn draw_zoom(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        frame: &mut DrawFrame<'_>,
    ) -> Result<bool, Error> {
        let Some(at) = self.zoom.as_ref().map(|zoom| zoom.at) else {
            return Ok(false);
        };
        let Some(fit) = self.zoomed(at) else {
            if let Some(mut zoom) = self.zoom.take() {
                zoom.release(host);
            }
            return Ok(false);
        };
        let Some(zoom) = self.zoom.as_mut() else {
            return Ok(false);
        };
        let (rect, counting) = (frame.rect, frame.pictures_only());
        // Fit: the page's picture in the whole frame; zoomed: the larger
        // decode in a rectangle the host clips.
        let (image, area) = match zoom.big.filter(|_| zoom.level > 0) {
            Some(big) => (
                big.image,
                FrameRect::new(
                    rect.x.saturating_add(zoom.pan.0),
                    rect.y.saturating_add(zoom.pan.1),
                    zoom.size.0.unsigned_abs(),
                    zoom.size.1.unsigned_abs(),
                ),
            ),
            None => (fit, rect),
        };
        let showing_note = zoom.note && !counting;
        let canvas = frame.draw_canvas()?;
        // The count of the host's picture rule draws the picture alone.
        if !counting {
            canvas.fill_rectangle(rect, Color::WHITE)?;
        }
        let drawn = canvas.draw_image(image, area, ImageStyle::new());
        match drawn {
            Err(Error::NotFound) if zoom.level > 0 => {
                // The host ended the larger decode: back to fit with the note.
                zoom.release(host);
                zoom.level = 0;
                zoom.most = 0;
                zoom.note = true;
                redraw(host, RedrawMode::Partial)?;
            }
            Err(Error::NotFound) => {
                // The host ended the id: asked for again, drawn the next frame.
                self.ask_missing(host, docs);
                redraw(host, RedrawMode::Partial)?;
            }
            Err(Error::Unsupported) => {}
            other => other?,
        }
        if showing_note {
            note(host, frame, TOO_BIG, NOTE_ROWS)?;
        }
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

/// Asks the document for picture `path` at the size zoom `level` draws it at:
/// the larger decode and the picture's size at fit. Any refusal (the format has
/// no such index, a picture too big, no memory, every id out) is an error.
fn enlarge(
    host: &Host<'_>,
    docs: &Documents,
    document: paperpaw_sdk::native::document::DocId,
    path: &str,
    (screen, level): ((i32, i32), u8),
) -> Result<(Big, (i32, i32)), Error> {
    let (handle, Some(index)) = picture_of(path).ok_or(Error::Invalid)? else {
        return Err(Error::Invalid);
    };
    let page_size = ImageInfo::read(host, ImageId::new(handle)?)?;
    let fit = fit_size((page_size.width(), page_size.height()), screen);
    let side = side_for(zoomed_size(fit, level));
    let mut out = buffer(SERVICE_BUFFER)?;
    let mut input = [0; 64];
    let answer = docs.ask_image(host, document, index, side, &mut input, &mut out)?;
    // Id 0: not decoded (no room, a picture the format cannot decode larger).
    let image = ImageId::new(answer.image())?;
    Ok((
        Big {
            image,
            owned: image.get() != handle,
        },
        fit,
    ))
}

/// What a key does while the picture is on show.
#[derive(Debug, PartialEq, Eq)]
enum Key {
    /// Back to the page.
    Close,
    /// One level back: zoomed to fit, fit to the page.
    Back,
    /// The next zoom level.
    Cycle,
    Left,
    Right,
    Up,
    Down,
    /// The page buttons: reading order.
    Prev,
    Next,
    /// Not the view's: the host's.
    Other,
}

fn key(action: &str) -> Key {
    match action {
        "core.menu" => Key::Close,
        "core.back" => Key::Back,
        "core.confirm" => Key::Cycle,
        "core.left" => Key::Left,
        "core.right" => Key::Right,
        "core.up" => Key::Up,
        "core.down" => Key::Down,
        "core.previous" | "core.prev-page" => Key::Prev,
        "core.next" | "core.next-page" => Key::Next,
        _ => Key::Other,
    }
}

/// The level after `level`, the cycle ending below what the host could not decode.
const fn next_level(level: u8, most: u8) -> u8 {
    if level >= most || level + 1 >= LEVELS {
        0
    } else {
        level + 1
    }
}

/// Whether a tap at `at`, `now` ms, is the second of a double tap; the first
/// is kept for the next.
fn double_tap(last: &mut Option<(u64, Point)>, now: u64, at: Point) -> bool {
    let second = last.is_some_and(|(then, was)| {
        now.saturating_sub(then) <= DOUBLE_MS
            && (at.x - was.x).abs() <= DOUBLE_PX
            && (at.y - was.y).abs() <= DOUBLE_PX
    });
    *last = if second { None } else { Some((now, at)) };
    second
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

/// The size of a `picture` (width, height) fit into the `screen`.
fn fit_size((width, height): (u32, u32), screen: (i32, i32)) -> (i32, i32) {
    let (sw, sh) = (
        u64::from(screen.0.max(1) as u32),
        u64::from(screen.1.max(1) as u32),
    );
    let (w, h) = (u64::from(width), u64::from(height));
    if w == 0 || h == 0 {
        return (screen.0, screen.1);
    }
    let (fw, fh) = if w * sh >= h * sw {
        (sw, (h * sw / w).max(1))
    } else {
        ((w * sh / h).max(1), sh)
    };
    (fw as i32, fh as i32)
}

/// The picture's size at zoom `level` (1: twice the fit).
const fn zoomed_size(fit: (i32, i32), level: u8) -> (i32, i32) {
    let k = level as i32 + 1;
    (fit.0.saturating_mul(k), fit.1.saturating_mul(k))
}

/// The side to decode a picture drawn at `size` to: its longer side, at most
/// what the host decodes.
fn side_for(size: (i32, i32)) -> u32 {
    size.0.max(size.1).max(1).unsigned_abs().min(SIDE_MAX)
}

/// An offset on one axis kept in the frame: a picture wider than the screen
/// covers it, a smaller one is centred.
const fn clamp_axis(offset: i32, size: i32, screen: i32) -> i32 {
    if size <= screen {
        (screen - size) / 2
    } else if offset > 0 {
        0
    } else if offset < screen - size {
        screen - size
    } else {
        offset
    }
}

fn clamp_pan(pan: (i32, i32), size: (i32, i32), screen: (i32, i32)) -> (i32, i32) {
    (
        clamp_axis(pan.0, size.0, screen.0),
        clamp_axis(pan.1, size.1, screen.1),
    )
}

/// The picture moved by `by` pixels, kept in the frame.
fn panned(pan: (i32, i32), by: (i32, i32), size: (i32, i32), screen: (i32, i32)) -> (i32, i32) {
    clamp_pan(
        (pan.0.saturating_add(by.0), pan.1.saturating_add(by.1)),
        size,
        screen,
    )
}

/// One step in reading order: across the row, then to the start of the next
/// row (`forward`), or the way back.
fn scanned(
    pan: (i32, i32),
    (dx, dy): (i32, i32),
    size: (i32, i32),
    screen: (i32, i32),
    forward: bool,
) -> (i32, i32) {
    let (right, bottom) = (screen.0 - size.0, screen.1 - size.1);
    let moved = if forward {
        if pan.0 > right {
            (pan.0 - dx, pan.1)
        } else if pan.1 > bottom {
            (0, pan.1 - dy)
        } else {
            pan
        }
    } else if pan.0 < 0 {
        (pan.0 + dx, pan.1)
    } else if pan.1 < 0 {
        (right, pan.1 + dy)
    } else {
        pan
    };
    clamp_pan(moved, size, screen)
}

/// The offset that keeps the middle of the view on the same point of the
/// picture when it grows from `old` to `new`.
fn recentred(pan: (i32, i32), old: (i32, i32), new: (i32, i32), screen: (i32, i32)) -> (i32, i32) {
    let axis = |o: i32, was: i32, now: i32, s: i32| {
        let middle = i64::from(s / 2);
        let at = middle - i64::from(o);
        let moved = middle - at * i64::from(now) / i64::from(was.max(1));
        moved.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    clamp_pan(
        (
            axis(pan.0, old.0, new.0, screen.0),
            axis(pan.1, old.1, new.1, screen.1),
        ),
        new,
        screen,
    )
}

#[cfg(test)]
mod tests;
