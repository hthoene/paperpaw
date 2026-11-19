//! The reading screen's side of the page layers (`native_layers`): the book is
//! asked of both stages when it opens and when a setting changes; the text
//! stage's plugin is shown the blocks of a page before its layout, each once
//! for the book ([`Layers::prepare`], applied as the layout reads them,
//! [`Layers::apply_kept`]); the layer stage's plugin is shown each page once
//! after its layout, and its answer is drawn with the page. Without a plugin
//! nothing is kept (`Layers` holds no heap).
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_sdk::{
    Color, Point,
    document::{Block, Locator},
    native::{
        Error, Host,
        callbacks::FrameRect,
        canvas::Canvas,
        fonts::TextOrigin,
        stages::{Stage, StageRun},
    },
};

use super::{Open, View};
use crate::native_document::HostDocument;
use crate::native_draw::draw_run;
use crate::native_layers::{
    self as layers, Answers, Chosen, Kind,
    page::{self, Pages, Pen},
};
use crate::native_look::Look;
use crate::native_typeset::{HostFonts, Role};

/// The most blocks, and bytes of their text, asked ahead of one layout: more
/// than a page holds on any screen (what is missed is asked after the layout,
/// which is then made once more).
const AHEAD_BLOCKS: usize = 24;
const AHEAD_TEXT: usize = 8 * 1024;

/// What the plugins of both stages add to the book on show.
#[derive(Debug, Default)]
pub(crate) struct Layers(RefCell<Option<Box<Inner>>>);

#[derive(Debug, Default)]
struct Inner {
    text: Option<Chosen>,
    answers: Answers,
    /// The first block a layout read that was not asked yet.
    missed: Option<(u32, u32)>,
    layer: Option<Chosen>,
    pages: Pages,
}

fn reply_buffer() -> Result<Vec<u8>, Error> {
    let room = Stage::out_len(crate::stages::reader_text_v1::MAX_REPLY as u32);
    let mut out = Vec::new();
    out.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
    out.resize(room, 0);
    Ok(out)
}

/// The first subscriber of `kind`'s stage that says it adds to the book at
/// `path`; none where nobody does or the device runs no stages.
fn choose(host: &Host<'_>, kind: Kind, (path, language): (&str, &str)) -> Option<Chosen> {
    let input = layers::book_input(kind, path, language).ok()?;
    let stage = Stage::new(host, kind.key()).ok()?;
    let mut out = reply_buffer().ok()?;
    let mut run = stage.run(&input, &mut out);
    while let Ok(Some(reply)) = run.next() {
        if reply.status().is_err() {
            continue;
        }
        let at = (reply.index(), reply.subscriber(), reply.signature());
        if let Ok(Some(chosen)) = Chosen::from_reply(kind, at, reply.payload()) {
            return Some(chosen);
        }
    }
    None
}

/// The one reply of `chosen` in `run`: its payload where it answered,
/// `Err(State)` where another plugin is at its place now.
fn reply_of<'r>(run: &'r mut StageRun<'_>, chosen: &Chosen) -> Result<&'r [u8], Error> {
    let reply = run.next()?.ok_or(Error::State)?;
    if reply.subscriber() != chosen.id {
        return Err(Error::State);
    }
    reply.status()?;
    Ok(reply.payload())
}

impl Layers {
    /// Asks both stages for the book at `path` in `language`: whether what
    /// the plugins add changed (the pages are to be laid out and drawn anew).
    pub(super) fn ask(&self, host: &Host<'_>, book: (&str, &str)) -> bool {
        let text = choose(host, Kind::Text, book);
        let layer = choose(host, Kind::Layer, book);
        let Ok(mut held) = self.0.try_borrow_mut() else {
            return false;
        };
        let before = held.as_ref().map(|i| (i.text.clone(), i.layer.clone()));
        let after = (text, layer);
        if before.as_ref() == Some(&after) || (before.is_none() && after == (None, None)) {
            return false;
        }
        *held = match after {
            (None, None) => None,
            (text, layer) => Some(Box::new(Inner {
                text,
                layer,
                ..Inner::default()
            })),
        };
        true
    }

    /// What the text stage's plugin adds to the layout's key (0: none).
    pub(super) fn signature(&self) -> u64 {
        self.0
            .try_borrow()
            .ok()
            .and_then(|i| {
                i.as_ref()
                    .and_then(|i| i.text.as_ref().map(|c| c.signature))
            })
            .unwrap_or(0)
    }

    /// Whether the text stage is used for the book.
    fn texts(&self) -> bool {
        self.signature() != 0
    }

    /// The first block the last layout read without an answer, and forgets it.
    fn missed(&self) -> Option<(u32, u32)> {
        self.0
            .try_borrow_mut()
            .ok()
            .and_then(|mut i| i.as_mut().and_then(|i| i.missed.take()))
    }

    /// The answer kept for `block` of `section` applied to it; a block not
    /// asked yet is noted ([`missed`](Self::missed)) and laid out as it is.
    pub(crate) fn apply_kept(&self, section: u32, block: &mut Block) {
        let Ok(mut held) = self.0.try_borrow_mut() else {
            return;
        };
        let Some(inner) = held.as_mut().filter(|i| i.text.is_some()) else {
            return;
        };
        match inner.answers.reply(section, block.start) {
            Some(reply) => {
                layers::apply(reply, block).ok();
            }
            None => {
                inner.missed.get_or_insert((section, block.start));
            }
        }
    }

    /// Asks the text stage's plugin for the blocks from `start` on that are
    /// not asked yet, as many as a page holds, reading them from `document`.
    /// A refusal stops it: what is not asked is laid out as it is.
    fn prepare(&self, host: &Host<'_>, document: &HostDocument<'_>, start: (u32, u32)) {
        let Ok(mut held) = self.0.try_borrow_mut() else {
            return;
        };
        let Some(inner) = held.as_mut() else {
            return;
        };
        let Some(chosen) = inner.text.clone() else {
            return;
        };
        inner.missed = None;
        let Ok(stage) = Stage::new(host, Kind::Text.key()) else {
            return;
        };
        let (section, mut offset) = start;
        let (mut seen, mut out) = (0_usize, Vec::new());
        for _ in 0..AHEAD_BLOCKS {
            if seen >= AHEAD_TEXT {
                break;
            }
            if let Some((end, len)) = inner.answers.around(section, offset) {
                (seen, offset) = (seen + len as usize, end);
                continue;
            }
            let Ok(Some(block)) = document.unlayered_at(Locator::new(section, offset)) else {
                break;
            };
            if block.end <= offset {
                break;
            }
            if out.is_empty() {
                let Ok(buffer) = reply_buffer() else { break };
                out = buffer;
            }
            let Ok(input) = layers::block_input(section, &block) else {
                break;
            };
            let mut run = stage.run_from(chosen.cursor, &input, &mut out);
            let kept = match reply_of(&mut run, &chosen) {
                Ok(reply) => inner.answers.keep(section, &block, reply),
                // Another plugin at its place: the stage is not used until asked again.
                Err(Error::State) => {
                    inner.text = None;
                    inner.answers.clear();
                    break;
                }
                // A refusal is kept as no answer: the block is not asked again.
                Err(_) => inner.answers.keep(section, &block, &[]),
            };
            if kept.is_err() {
                break;
            }
            (seen, offset) = (seen + block.text.len(), block.end);
        }
    }

    /// The part of the content box `full` the book's text is laid out in:
    /// the layer stage's plugin's region, or all of it.
    pub(super) fn text_area(&self, full: crate::native_pages::Box) -> crate::native_pages::Box {
        self.region().map_or(full, |r| page::within(full, r))
    }

    /// The region of the layer stage's plugin, if it asked for one.
    fn region(&self) -> Option<[u16; 4]> {
        self.0.try_borrow().ok().and_then(|i| {
            i.as_ref()
                .and_then(|i| i.layer.as_ref())
                .and_then(|c| c.region)
        })
    }

    /// What the region adds to the layout's key (0: none).
    pub(super) fn region_key(&self) -> u64 {
        self.region().map_or(0, |[x, y, w, h]| {
            u64::from(x) | u64::from(y) << 16 | u64::from(w) << 32 | u64::from(h) << 48
        })
    }

    /// Whether the layer stage's plugin follows reading aloud.
    pub(super) fn follows_aloud(&self) -> bool {
        self.0.try_borrow().ok().is_some_and(|i| {
            i.as_ref()
                .and_then(|i| i.layer.as_ref())
                .is_some_and(|c| c.aloud)
        })
    }

    /// Forgets the answers of the pages (their layout changed).
    pub(super) fn forget_pages(&self) {
        if let Ok(mut held) = self.0.try_borrow_mut()
            && let Some(inner) = held.as_mut()
        {
            inner.pages.clear();
        }
    }
}

impl Open {
    /// What the answer of the page on show is kept by, for `chosen`: the
    /// words read aloud are part of it where the plugin follows them.
    fn layer_key(&self, start: (u32, u32), chosen: Option<&Chosen>) -> page::Key {
        let spoken = chosen
            .filter(|c| c.aloud)
            .and(self.spoken.as_ref())
            .map(|w| (w.start as u32, w.end as u32));
        (start, spoken)
    }

    /// Asks the page layers' stages for the book: whether that changed what
    /// they add.
    pub(super) fn ask_layers(&mut self, host: &Host<'_>) -> bool {
        self.layers.ask(host, (&self.path, &self.language))
    }

    /// The blocks of the page from `start` asked ahead of its layout.
    pub(super) fn prepare_layers(
        &self,
        host: &Host<'_>,
        document: &HostDocument<'_>,
        start: (u32, u32),
    ) {
        if self.layers.texts() {
            self.layers.prepare(host, document, start);
        }
    }

    /// The first block the layout just made read unasked, asked now: `true`
    /// where the page is to be laid out once more.
    pub(super) fn prepare_missed(&self, host: &Host<'_>, document: &HostDocument<'_>) -> bool {
        match self.layers.missed() {
            Some(at) if self.layers.texts() => {
                self.layers.prepare(host, document, at);
                true
            }
            _ => false,
        }
    }

    /// The layer stage's plugin shown the page on show, once (its answer is
    /// kept with it), positions from the top left of the content box `area`.
    /// Never fails the draw: a refusal leaves the page without.
    pub(super) fn ask_layer(&self, host: &Host<'_>, area: crate::native_pages::Box) {
        let text = self.layers.text_area(area);
        let shift = (
            i32::from(text.x) - i32::from(area.x),
            i32::from(text.y) - i32::from(area.y),
        );
        let (Some(page), Ok(mut held)) = (self.page.as_ref(), self.layers.0.try_borrow_mut())
        else {
            return;
        };
        let Some(inner) = held.as_mut() else {
            return;
        };
        let Some(chosen) = inner.layer.clone() else {
            return;
        };
        let start = self.layer_key((page.start.section, page.start.offset), Some(&chosen));
        if inner.pages.get(start).is_some() || page.lines.is_empty() {
            return;
        }
        let aloud = start
            .1
            .and_then(|(a, b)| page::aloud(page, a as usize..b as usize));
        let (Ok(stage), Ok(rows), Ok(mut out)) = (
            Stage::new(host, Kind::Layer.key()),
            page::rows(page, &self.words, (area.x, area.y), shift),
            reply_buffer(),
        ) else {
            return;
        };
        let (mut replies, mut bytes, mut first) = (Vec::new(), 0, 0);
        let size = (u32::from(area.width), u32::from(area.height));
        while first < rows.len() {
            let Ok((input, next)) = page::fire(page, &rows, (first, aloud.as_ref()), size) else {
                break;
            };
            let mut run = stage.run_from(chosen.cursor, &input, &mut out);
            match reply_of(&mut run, &chosen) {
                Ok(reply) if reply.len() > 1 && bytes + reply.len() <= page::MOST_BYTES => {
                    let mut kept = Vec::new();
                    if kept.try_reserve_exact(reply.len()).is_err()
                        || replies.try_reserve(1).is_err()
                    {
                        break;
                    }
                    kept.extend_from_slice(reply);
                    bytes += kept.len();
                    replies.push(kept);
                }
                Ok(_) => {}
                Err(Error::State) => {
                    inner.layer = None;
                    inner.pages.clear();
                    return;
                }
                Err(_) => break,
            }
            first = next;
        }
        inner.pages.keep(start, replies).ok();
    }

    /// The first page's layer, asked in the step after it is shown: whether
    /// it got something to draw (the page is drawn again then).
    pub(super) fn layered_now(&self, host: &Host<'_>) -> bool {
        let Some(area) = self
            .settings
            .setup(host)
            .ok()
            .and_then(|s| self.page_area(&s))
        else {
            return false;
        };
        self.ask_layer(host, area);
        let (Some(page), Ok(held)) = (self.page.as_ref(), self.layers.0.try_borrow()) else {
            return false;
        };
        let start = (page.start.section, page.start.offset);
        held.as_ref()
            .and_then(|i| i.pages.get(self.layer_key(start, i.layer.as_ref())))
            .is_some_and(|replies| !replies.is_empty())
    }

    /// Draws what the layer stage's plugin answered for the page on show over
    /// its text, the text area's top left at `at`.
    pub(super) fn draw_layer(
        &self,
        canvas: &Canvas<'_>,
        (fonts, look): (&HostFonts<'_>, &Look),
        at: (i32, i32),
    ) {
        let (Some(page), Ok(held)) = (self.page.as_ref(), self.layers.0.try_borrow()) else {
            return;
        };
        let start = (page.start.section, page.start.offset);
        let Some(replies) = held
            .as_ref()
            .and_then(|i| i.pages.get(self.layer_key(start, i.layer.as_ref())))
        else {
            return;
        };
        let mut pen = LayerPen {
            canvas,
            fonts,
            look,
        };
        page::draw(&mut pen, replies, at).ok();
    }
}

impl View {
    /// A setting of another plugin changed: the layers' stages are asked again
    /// for the book. Whether what they add changed.
    pub fn relayer(&mut self, host: &Host<'_>) -> bool {
        match &mut self.body {
            super::Body::Open(open) => open.ask_layers(host),
            _ => false,
        }
    }
}

/// The host's canvas and reading fonts, drawing a layer.
struct LayerPen<'a, 'h> {
    canvas: &'a Canvas<'a>,
    fonts: &'a HostFonts<'h>,
    look: &'a Look,
}

impl Pen for LayerPen<'_, '_> {
    fn fill(&mut self, (x, y): (i32, i32), (w, h): (u32, u32), gray: u8) -> Result<(), Error> {
        self.canvas
            .fill_rectangle(FrameRect::new(x, y, w, h), Color::gray(gray))
    }

    fn segment(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        width: u32,
        gray: u8,
    ) -> Result<(), Error> {
        self.canvas.draw_segment(
            Point::new(from.0, from.1),
            Point::new(to.0, to.1),
            width,
            Color::gray(gray),
        )
    }

    fn text(
        &mut self,
        (x, baseline): (i32, i32),
        (size, gray, italic): (f32, u8, bool),
        text: &str,
    ) -> Result<(), Error> {
        let role = if italic { Role::Italic } else { Role::Reading };
        draw_run(
            self.fonts.font_of(role),
            self.canvas,
            size,
            TextOrigin::new(x as f32, baseline),
            Color::gray(gray),
            text,
            self.look.ink(size),
        )
        .map(|_| ())
    }
}
