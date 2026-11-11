//! The pictures of a plugin's page, one at a time, turned like pages (the Beta 3
//! `shots.rs` on interface 7). Each is fetched to a file of the data folder
//! when shown (`core.net`, at most [`FILE_MAX`] bytes) and decoded by the host
//! (`image_prepare`, to the size of the screen at most), which keeps the
//! decoded picture on the card and draws it in bands: the plugin holds one
//! picture's id at a time, and gives it back before it asks for the next, so
//! the pictures take no more memory than one.
//!
//! The pictures are those of the plugin's page (`Loader::page`): at most 4
//! ([`MAX_SHOTS`]), none past 2,560 px (the page's reader keeps no other); on a
//! device with little memory also none past 1,280 px or 300,000 bytes
//! ([`usable`]), as Beta 3 kept them out of the page.
//!
//! **For the wiring:** the screen asks `core.net` itself, with the request
//! number [`TOKEN`], and the host decodes with the cookies [`cookie`] makes:
//! the plugin hands the screen the `HttpDone` events of that token and the
//! `ImageReady` events (through `Screen::event`) and calls `Screen::closed`
//! when the screen ends. A picture's address is taken as the page gives it:
//! the fetch of the page has joined it to the page's own.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Handled, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::CommandSender,
        events::Event,
        fonts::{Font, FontRole},
        image::{ImageId, ImageReady, ImageStatus, ImageStyle, PrepareImage, PrepareRoot},
        net::{FileRoot, Header, HttpDone, Net, Request, RequestId},
        snapshot,
    },
    web::USER_AGENT,
};
use paperpaw_ui_kit::{
    Messages, Page, Paint, Style, header,
    mem::{buffer, push},
    paint, text,
};

use crate::native_item::{MAX_SHOT_SIDE, MAX_SHOTS, Shot};
use crate::native_screens::{Ctx, Next, Screen};

/// The number of the screen's requests in their `HttpDone` events.
pub const TOKEN: u32 = 0x5348_4f54;
/// The most a picture's file may be (Beta 3: 2 MiB).
pub const FILE_MAX: u64 = 2 * 1024 * 1024;
/// On a device with little memory a picture past this many pixels on a side, or
/// past this many bytes, is not shown (Beta 3's `SMALL_SHOT_*`).
pub const SMALL_SIDE: u32 = 1280;
pub const SMALL_BYTES: u64 = 300 * 1000;
/// The longest side the host decodes to (`image::SIDE_MAX`).
const DECODE_MAX: u32 = paperpaw_sdk::native::image::SIDE_MAX;
/// The size of the buffers of a request to the host.
const IO: usize = 2048;
/// The input of a cancel: one token.
const CANCEL_IN: usize = 64;
/// The parameter that names the plugin.
const ID: &str = "id";

/// Whether `shot` is shown where the device has `small` memory or not.
#[must_use]
pub fn usable(shot: &Shot, small: bool) -> bool {
    shot.width <= MAX_SHOT_SIDE
        && shot.height <= MAX_SHOT_SIDE
        && (!small
            || (shot.width <= SMALL_SIDE && shot.height <= SMALL_SIDE && shot.size <= SMALL_BYTES))
}

/// The pictures shown: the first [`MAX_SHOTS`] the page lists, those that are
/// [`usable`].
pub fn shown(shots: &[Shot], small: bool) -> impl Iterator<Item = &Shot> {
    shots
        .iter()
        .take(MAX_SHOTS)
        .filter(move |shot| usable(shot, small))
}

/// How many pictures are shown.
#[must_use]
pub fn count(shots: &[Shot], small: bool) -> usize {
    shown(shots, small).count()
}

/// The place `to` is reached from `at` by turning `by` pages, among `len`;
/// `None` past either end.
#[must_use]
pub fn turn(at: usize, by: isize, len: usize) -> Option<usize> {
    let to = if by < 0 {
        at.checked_sub(by.unsigned_abs())?
    } else {
        at.checked_add(by.unsigned_abs())?
    };
    (to < len && to != at).then_some(to)
}

/// How far a page action turns: 1 on, -1 back.
#[must_use]
pub fn turn_of(action: &str) -> Option<isize> {
    match action {
        "core.next-page" | "core.right" | "core.down" | "core.next" => Some(1),
        "core.prev-page" | "core.left" | "core.up" | "core.previous" => Some(-1),
        _ => None,
    }
}

/// A tap's turn: the left half back, the right half on, as in a book.
#[must_use]
pub fn tap_turn(x: i32, width: u16) -> isize {
    if x < i32::from(width) / 2 { -1 } else { 1 }
}

/// The room for the picture: below the header, a margin at the sides, a gap
/// below.
#[must_use]
pub fn area_of(display: (u16, u16), header: u16, gap: u16, margin: u16) -> Rect {
    let top = header.saturating_add(gap);
    Rect::new(
        margin,
        top,
        display.0.saturating_sub(margin.saturating_mul(2)),
        display.1.saturating_sub(top.saturating_add(gap)),
    )
}

/// The size a picture of `size` takes in a room of `room`: scaled to fit, its
/// shape kept, centred (0 by 0 where there is no room or no picture).
#[must_use]
pub fn fit_in(size: (u32, u32), room: Rect) -> Rect {
    let (w, h) = (u64::from(size.0), u64::from(size.1));
    let (rw, rh) = (u64::from(room.width), u64::from(room.height));
    if w == 0 || h == 0 || rw == 0 || rh == 0 {
        return Rect::new(room.x, room.y, 0, 0);
    }
    // The width the room's height gives, or the height the room's width gives.
    let (width, height) = if w * rh <= h * rw {
        (w * rh / h, rh)
    } else {
        (rw, h * rw / w)
    };
    let to = |v: u64| u16::try_from(v).unwrap_or(u16::MAX);
    let (width, height) = (to(width.max(1)), to(height.max(1)));
    Rect::new(
        room.x.saturating_add(room.width.saturating_sub(width) / 2),
        room.y,
        width,
        height,
    )
}

/// The longest side to decode to: the screen's, at most what the host decodes.
#[must_use]
pub fn side_for(display: (u16, u16)) -> u32 {
    u32::from(display.0.max(display.1)).min(DECODE_MAX)
}

/// The file picture `at` is saved to (`shot-<at>.part`), written into `out`.
#[must_use]
pub fn part_name(at: usize, out: &mut [u8; 32]) -> &str {
    let mut digits = [0; 20];
    let number = text::decimal(at, &mut digits);
    let mut len = 0;
    for part in ["shot-", number, ".part"] {
        for byte in part.bytes() {
            if let Some(slot) = out.get_mut(len) {
                *slot = byte;
                len += 1;
            }
        }
    }
    out.get(..len)
        .and_then(|bytes| core::str::from_utf8(bytes).ok())
        .unwrap_or("shot.part")
}

/// The number the host gives back with picture `at` decoded in the
/// `generation`'th request: one asked for before is told apart.
#[must_use]
pub fn cookie(generation: u16, at: usize) -> u32 {
    u32::from(generation) << 16 | (u32::try_from(at).unwrap_or(0) & 0xffff)
}

/// What the screen shows of the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Picture {
    /// Being fetched or decoded.
    Loading,
    /// Decoded: the host's id and its size.
    Here(ImageId, u32, u32),
    /// Could not be had.
    Failed,
}

/// The screen.
#[derive(Debug)]
pub struct View {
    id: String,
    at: usize,
    count: usize,
    small: bool,
    picture: Picture,
    generation: u16,
    net: Option<Net>,
    /// The download going on and the picture it is for.
    pending: Option<(RequestId, usize)>,
    /// The cookie of the picture the host is decoding.
    preparing: Option<u32>,
    /// What the last frame's header takes a tap for.
    header: Page,
    width: u16,
    texts: Texts,
}

/// The texts the screen says, read once when it opens: a frame borrows them and
/// reads none.
#[derive(Debug)]
struct Texts {
    title: String,
    loading: String,
    failed: String,
}

impl Texts {
    fn read(host: &Host<'_>) -> Result<Self, Error> {
        let mut messages = Messages::new()?;
        Ok(Self {
            title: messages.copy(host, "store.shots")?,
            loading: messages.copy(host, "store.loading-plugin")?,
            failed: messages.copy(host, "store.shot-failed")?,
        })
    }
}

fn io() -> Result<(Vec<u8>, Vec<u8>), Error> {
    Ok((buffer(IO)?, buffer(IO)?))
}

impl View {
    /// The `n`'th picture shown of the page.
    fn shot<'a>(&self, ctx: &'a Ctx<'_>, n: usize) -> Option<&'a Shot> {
        let page = ctx.loader.page(&self.id)?;
        shown(&page.more.shots, self.small).nth(n)
    }

    /// Gives back what the picture on show holds: the download is stopped,
    /// the picture's id released, its file removed. It allocates nothing (the
    /// cancel's two small buffers are on the stack), so it cannot be refused:
    /// what a closed screen holds is always given back.
    fn drop_current(&mut self, ctx: &Ctx<'_>) {
        if self.pending.take().is_some()
            && let Some(net) = &self.net
        {
            cancel_request(net, ctx.host);
        }
        if let Picture::Here(id, ..) = self.picture {
            release(ctx.host, id);
        }
        self.picture = Picture::Loading;
        self.preparing = None;
        let mut name = [0; 32];
        // A card that does not answer leaves a file the next fetch replaces.
        let _ = ctx.data.remove(part_name(self.at, &mut name));
    }

    /// Asks for picture `at`, after the one on show was given back.
    fn fetch(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        self.drop_current(ctx);
        let Some(url) = self.shot(ctx, self.at).map(|shot| shot.url.as_str()) else {
            self.picture = Picture::Failed;
            return Ok(());
        };
        match self.request(ctx.host, url) {
            Ok(id) => self.pending = Some((id, self.at)),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // No network, no room for a request: no picture.
            Err(_) => self.picture = Picture::Failed,
        }
        Ok(())
    }

    fn request(&mut self, host: &Host<'_>, url: &str) -> Result<RequestId, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        let net = self.net.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = io()?;
        let mut name = [0; 32];
        let headers = [Header::new("User-Agent", USER_AGENT)?];
        let request = Request::new(TOKEN, "GET", url)?
            .headers(&headers)?
            .save_to(FileRoot::Data, part_name(self.at, &mut name))?
            .max_file(FILE_MAX)?;
        net.request(host, &request, &mut input, &mut out)
    }

    /// The picture's download ended.
    fn done(&mut self, ctx: &Ctx<'_>, done: &HttpDone) -> Result<Next, Error> {
        let Some((id, at)) = self.pending else {
            return Ok(Next::None);
        };
        // A request stopped for the one that took its place ends too.
        if done.id() != id {
            return Ok(Next::None);
        }
        self.pending = None;
        let mut name = [0; 32];
        let part = part_name(at, &mut name);
        if !(done.succeeded() && done.saved()) {
            let _ = ctx.data.remove(part);
            self.picture = Picture::Failed;
            return Ok(Next::Redraw);
        }
        self.generation = self.generation.wrapping_add(1);
        let number = cookie(self.generation, at);
        let size = self.size(ctx.host);
        let prepare = PrepareImage::new(PrepareRoot::Data, part, number).side(size);
        let mut scratch = [0; 256];
        match CommandSender::new(ctx.host, &mut scratch).image_prepare(&prepare) {
            Ok(()) => self.preparing = Some(number),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                let _ = ctx.data.remove(part);
                self.picture = Picture::Failed;
            }
        }
        Ok(Next::Redraw)
    }

    fn size(&self, host: &Host<'_>) -> u32 {
        snapshot::capture_snapshot(host)
            .map(|s| side_for((s.display_width(), s.display_height())))
            .unwrap_or(DECODE_MAX)
    }

    /// The host decoded a picture, or could not.
    fn ready(&mut self, ctx: &Ctx<'_>, ready: &ImageReady) -> Result<Next, Error> {
        let image = match (ready.status(), ready.image()) {
            (ImageStatus::Ready, Some(id)) => Some(id),
            _ => None,
        };
        if self.preparing != Some(ready.cookie()) {
            // A picture asked for before the one wanted: nobody wants it.
            if let Some(id) = image {
                release(ctx.host, id);
            }
            return Ok(Next::None);
        }
        self.preparing = None;
        let mut name = [0; 32];
        let _ = ctx.data.remove(part_name(self.at, &mut name));
        self.picture = match image {
            Some(id) => Picture::Here(id, ready.width(), ready.height()),
            None => Picture::Failed,
        };
        Ok(Next::Redraw)
    }

    /// The pictures' own drawing: a thin frame round the picture (the
    /// screenshot's own paper apart from ours) or what is said instead.
    fn body(
        &self,
        font: &Font<'_>,
        style: &Style,
        area: Rect,
        ops: &mut Vec<Paint>,
    ) -> Result<Option<(ImageId, Rect)>, Error> {
        let message = match self.picture {
            Picture::Here(id, w, h) => {
                let at = fit_in((w, h), area);
                let t = style.line.max(1);
                let (x, y) = (at.x.saturating_sub(t), at.y.saturating_sub(t));
                let (wide, high) = (
                    at.width.saturating_add(t.saturating_mul(2)),
                    at.height.saturating_add(t.saturating_mul(2)),
                );
                for edge in [
                    Rect::new(x, y, wide, t),
                    Rect::new(x, at.y.saturating_add(at.height), wide, t),
                    Rect::new(x, y, t, high),
                    Rect::new(at.x.saturating_add(at.width), y, t, high),
                ] {
                    push(ops, Paint::Fill(edge, style.rule))?;
                }
                return Ok(Some((id, at)));
            }
            Picture::Loading => self.texts.loading.as_str(),
            Picture::Failed => self.texts.failed.as_str(),
        };
        let wide = font.measure_text(style.text, message)?;
        let x = f32::from(area.x) + (f32::from(area.width) - wide).max(0.0) / 2.0;
        let line = Rect::new(area.x, area.y, area.width, area.height.min(style.row));
        text::label(ops, font, message, style.text, x, line, style.muted, None)?;
        Ok(None)
    }

    /// Turns by `by` pages; whether the picture changed.
    fn turned(&mut self, ctx: &Ctx<'_>, by: isize) -> Result<bool, Error> {
        let Some(to) = turn(self.at, by, self.count) else {
            return Ok(false);
        };
        // The one on show goes with its file, then the next is asked for.
        self.drop_current(ctx);
        self.at = to;
        self.fetch(ctx)?;
        Ok(true)
    }
}

/// The most the net service's `cancel` replies (`max_reply`): the host refuses a
/// smaller reply buffer.
const CANCEL_REPLY: usize = 1536;

/// Ends the request of the picture on the host. The reply buffer is the whole
/// `max_reply`, on the stack of this function alone so the frame stays under the
/// X4's 2,048 bytes and nothing is allocated; a cancel the host refuses ends by
/// itself with its request.
#[inline(never)]
fn cancel_request(net: &Net, host: &Host<'_>) {
    let (mut input, mut out) = ([0; CANCEL_IN], [0; CANCEL_REPLY]);
    let _ = net.cancel(host, TOKEN, &mut input, &mut out);
}

/// Gives picture `id` back to the host; one it no longer has is no error.
fn release(host: &Host<'_>, id: ImageId) {
    let mut scratch = [0; 64];
    let _ = CommandSender::new(host, &mut scratch).image_release(id);
}

impl Screen for View {
    fn open(ctx: &Ctx<'_>, params: &[(&str, &str)]) -> Result<Self, Error> {
        let id = params
            .iter()
            .find(|(name, _)| *name == ID)
            .map(|(_, value)| *value)
            .filter(|id| !id.is_empty())
            .ok_or(Error::Invalid)?;
        let small = ctx.loader.small_memory();
        let total = ctx
            .loader
            .page(id)
            .map_or(0, |page| count(&page.more.shots, small));
        let mut view = Self {
            id: text::copy(id)?,
            at: 0,
            count: total,
            small,
            picture: Picture::Loading,
            generation: 0,
            net: None,
            pending: None,
            preparing: None,
            header: Page::default(),
            width: 0,
            texts: Texts::read(ctx.host)?,
        };
        view.fetch(ctx)?;
        Ok(view)
    }

    fn shown(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        // A picture that was left for another screen's use is asked for again.
        if self.picture == Picture::Loading && self.pending.is_none() && self.preparing.is_none() {
            self.fetch(ctx)?;
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let host = ctx.host;
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let mut head = Page::default();
        // The counter's text comes through stack buffers (`header::pager`); the
        // buffers asked for here stay empty unless a label did not fit them.
        let prepared = header::prepare(
            &mut head,
            host,
            &font,
            &style,
            &mut Messages::default(),
            &self.texts.title,
            width,
            display.has_touch(),
            self.at,
            self.count,
        );
        if let Err(error) = prepared {
            let _ = font.release();
            return Err(error);
        }
        let area = area_of((width, height), style.header, style.gap, style.margin);
        let mut ops = Vec::new();
        push(
            &mut ops,
            Paint::Fill(Rect::new(0, 0, width, height), Color::WHITE),
        )?;
        let picture = self.body(&font, &style, area, &mut ops);
        let result = picture.and_then(|picture| {
            paint::replay(frame, &font, head.paint.iter().chain(ops.iter()))?;
            Ok(picture)
        });
        font.release()?;
        head.paint = Vec::new();
        self.header = head;
        self.width = width;
        if let Some((id, at)) = result? {
            // A picture the host refuses (an id it ended) leaves the frame.
            if let Ok(canvas) = frame.draw_canvas() {
                let _ = canvas.draw_image(id, paint::frame_rect(at), ImageStyle::new());
            }
        }
        Ok(())
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        let by = match *input {
            Input::Action { action, .. } => match turn_of(action) {
                Some(by) => by,
                None => return Ok((Handled::No, Next::None)),
            },
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => {
                if let Some(action) = self.header.action(position) {
                    let mut scratch = [0; 128];
                    CommandSender::new(ctx.host, &mut scratch).run_action(action)?;
                    return Ok((Handled::Yes, Next::None));
                }
                tap_turn(position.x, self.width)
            }
            _ => return Ok((Handled::No, Next::None)),
        };
        let next = if self.turned(ctx, by)? {
            Next::Redraw
        } else {
            Next::None
        };
        Ok((Handled::Yes, next))
    }

    fn closed(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        // Nothing here can be refused for want of memory: the picture's id and
        // the network handle are given back whatever the heap is.
        self.drop_current(ctx);
        if let Some(net) = self.net.take() {
            let _ = net.release(ctx.host);
        }
        Ok(())
    }

    fn event(&mut self, ctx: &Ctx<'_>, event: &Event<'_>) -> Result<Next, Error> {
        match event {
            Event::HttpDone(done) if done.token() == TOKEN => self.done(ctx, done),
            Event::ImageReady(ready) => self.ready(ctx, ready),
            _ => Ok(Next::None),
        }
    }
}

/// The parameter that names the plugin: `("id", <plugin>)`.
pub const PARAM: &str = ID;

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(w: u32, h: u32, size: u64) -> Shot {
        Shot {
            url: "https://x/y.png".into(),
            width: w,
            height: h,
            size,
        }
    }

    #[test]
    fn turning_stays_among_the_pictures() {
        assert_eq!(turn(0, 1, 3), Some(1));
        assert_eq!(turn(2, 1, 3), None);
        assert_eq!(turn(0, -1, 3), None);
        assert_eq!(turn(2, -1, 3), Some(1));
        assert_eq!(turn(0, 1, 0), None);
        assert_eq!(turn(usize::MAX, 1, usize::MAX), None);
        assert_eq!(turn(0, isize::MIN, 4), None);
    }

    #[test]
    fn the_actions_of_beta_3_turn() {
        for a in ["core.next-page", "core.right", "core.down", "core.next"] {
            assert_eq!(turn_of(a), Some(1));
        }
        for a in ["core.prev-page", "core.left", "core.up", "core.previous"] {
            assert_eq!(turn_of(a), Some(-1));
        }
        assert_eq!(turn_of("core.confirm"), None);
        assert_eq!(turn_of("core.back"), None);
    }

    #[test]
    fn a_tap_turns_by_the_half() {
        assert_eq!(tap_turn(10, 480), -1);
        assert_eq!(tap_turn(239, 480), -1);
        assert_eq!(tap_turn(240, 480), 1);
        assert_eq!(tap_turn(-5, 480), -1);
        assert_eq!(tap_turn(0, 0), 1);
    }

    #[test]
    fn at_most_four_pictures_and_small_devices_keep_the_small_ones() {
        let many: Vec<Shot> = (0..6).map(|_| shot(800, 600, 1000)).collect();
        assert_eq!(count(&many, false), 4);
        let mixed = [
            shot(800, 600, 1000),
            shot(1281, 600, 1000),
            shot(800, 1281, 1000),
            shot(800, 600, 300_001),
            shot(800, 600, 1000),
        ];
        // The cap of four counts what the page lists, then the device's filter.
        assert_eq!(count(&mixed, true), 1);
        assert_eq!(count(&mixed, false), 4);
        assert!(usable(&shot(1280, 1280, 300_000), true));
        assert!(!usable(&shot(2561, 10, 1), false));
        assert!(usable(&shot(2560, 2560, u64::MAX), false));
    }

    #[test]
    fn a_picture_fits_its_room_in_shape_and_is_centred() {
        let room = Rect::new(10, 50, 400, 600);
        let at = fit_in((800, 600), room);
        assert_eq!((at.width, at.height), (400, 300));
        assert_eq!((at.x, at.y), (10, 50));
        let tall = fit_in((100, 600), room);
        assert_eq!((tall.width, tall.height), (100, 600));
        assert_eq!(tall.x, 10 + 150);
        assert_eq!(fit_in((0, 5), room).width, 0);
        assert_eq!(fit_in((5, 5), Rect::new(0, 0, 0, 9)).height, 0);
        assert!(fit_in((u32::MAX, 1), room).width <= 400);
    }

    #[test]
    fn the_room_and_the_decoded_size() {
        let area = area_of((480, 800), 60, 8, 12);
        assert_eq!(
            (area.x, area.y, area.width, area.height),
            (12, 68, 456, 724)
        );
        assert_eq!(area_of((10, 10), 60, 8, 12).height, 0);
        assert_eq!(side_for((480, 800)), 800);
        assert_eq!(side_for((3000, 100)), 1600);
    }

    #[test]
    fn names_and_cookies() {
        let mut out = [0; 32];
        assert_eq!(part_name(3, &mut out), "shot-3.part");
        assert_eq!(part_name(usize::MAX, &mut out).len(), 5 + 20 + 5);
        assert_eq!(cookie(2, 3), 0x0002_0003);
        assert_ne!(cookie(1, 0), cookie(2, 0));
    }
}
