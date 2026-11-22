//! What is marked on the page and what a touch or a key does with it: the words
//! chosen with their bar (look up, copy, extend), and the links of the page. One
//! [`Marking`] belongs to an open reading screen; the screen asks it first about
//! every input (a chosen word or link takes the arrows and the confirm key before
//! a page turn) and draws its [`overlay`](Marking::draw_overlay) over the page.
//!
//! The interface-6 screen (`screen/selecting.rs`, `selection.rs`) is the
//! reference; this is its host-free movement (`native_selection::moves`) with
//! the bar, the clipboard and the lookup done through the interface-7 services.
use crate::native_word::Word;
use alloc::{string::String, vec::Vec};
use core::cell::Cell;
use paperpaw_sdk::{
    Point, Rect,
    document::Locator,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        library::Library,
        snapshot::{self, Snapshot},
        ui::{ButtonHint, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages, Style as UiStyle,
    paint::{self, Paint},
};

use crate::native_links::{DocRef, PageLink, Returns};
use crate::native_marks::Span;
use crate::native_pages;
use crate::native_place::Progress;
use crate::native_selection::live::{At, Live, Move, Side, Went, lead_of};
use crate::native_selection::{Mode, line_boxes};
use crate::native_wordbox::word_at;

pub mod act;
pub mod bar;
mod linking;

use bar::{Bar, Item};

/// What the marking needs of the book's highlights: the ones held (to name the
/// bar's items), the library to change them in, and where a place is in the book.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Book<'a> {
    /// The highlights of the book.
    pub highlights: &'a [Span],
    /// The library the highlights are kept in (none: they cannot be changed).
    pub library: Option<&'a Library>,
    /// The conversion the places belong to.
    pub conversion: u64,
    /// The sections' lengths, for the progress of a place.
    pub progress: Option<&'a Progress>,
    /// The progress stored with the place, where `progress` is none.
    pub stored_ppm: u32,
}

/// What the screen knows of its page and book: everything [`Marking`] reads.
#[derive(Debug)]
#[non_exhaustive]
pub struct Ctx<'a> {
    /// Where the page on show starts: a chosen word belongs to a page.
    pub page_start: Locator,
    /// The words of the page with their boxes in the pane, in reading order
    /// (`native_wordbox::word_boxes`).
    pub words: &'a [Word],
    /// The book's file, for the word-looked-up stage.
    pub path: &'a str,
    /// The book's title where known.
    pub title: &'a str,
    /// The book's language where known (a tag such as `en`).
    pub language: &'a str,
    /// The book the page is of, for its links (none: no links are read).
    pub doc: Option<DocRef<'a>>,
    /// The content box the page is laid out in, in the pane.
    pub area: native_pages::Box,
    /// When the input was, on the event clock (milliseconds).
    pub now_ms: u64,
    /// The plugin's `core.ui` handle, for the clipboard and the screens opened
    /// (one is held for the plugin: none is opened here, none leaks).
    pub ui: Option<&'a Ui>,
    /// The book's highlights (none: the bar offers them, nothing is changed).
    pub book: Option<Book<'a>>,
}

impl<'a> Ctx<'a> {
    /// The context of the page starting at `page_start` with `words`.
    #[must_use]
    pub const fn new(
        page_start: Locator,
        words: &'a [Word],
        path: &'a str,
        title: &'a str,
        language: &'a str,
        area: native_pages::Box,
    ) -> Self {
        Self {
            page_start,
            words,
            path,
            title,
            language,
            doc: None,
            area,
            now_ms: 0,
            ui: None,
            book: None,
        }
    }

    /// With the book's highlights.
    #[must_use]
    pub const fn book(mut self, book: Book<'a>) -> Self {
        self.book = Some(book);
        self
    }

    /// With the plugin's `core.ui` handle.
    #[must_use]
    pub const fn ui(mut self, ui: &'a Ui) -> Self {
        self.ui = Some(ui);
        self
    }

    /// With the book the page is of, so its links are read.
    #[must_use]
    pub const fn doc(mut self, doc: DocRef<'a>) -> Self {
        self.doc = Some(doc);
        self
    }
}

/// What the screen does after an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// Not an input for the marking: the screen goes on with it (a page turn).
    Ignored,
    /// Taken; nothing to draw again.
    Handled,
    /// Taken; the page's overlay changed, draw it again.
    Redraw,
    /// Taken: a highlight was made, removed or noted (or could not be, and it was
    /// said): the screen reads the highlights again and draws the page again.
    Edited,
    /// Taken: go to the page that holds this place (a followed link, or the way
    /// back). The page left is the marking's to return to; the screen keeps its
    /// own trail as for any jump.
    Jump {
        /// The section.
        section: u32,
        /// The offset in it.
        offset: u32,
    },
    /// Taken: turn the page (on, or back) and go on choosing links there.
    Turn {
        /// The page after this one.
        forward: bool,
    },
}

/// A page turn the selection asked for, made by the screen.
#[derive(Clone, Copy, Debug)]
struct Pending {
    /// The end that goes on over the edge (none: a touch turned the page and
    /// the selection stays where it is).
    side: Option<Side>,
    forward: bool,
    /// The page the turn is made from.
    page: Locator,
}

/// The state of the marking on an open page.
#[derive(Debug, Default)]
pub struct Marking {
    /// The chosen text, by place in the book: it holds across page turns.
    selection: Option<Live>,
    /// The start of the text of a selection that goes over pages, kept when the
    /// page of its first word is left (the highlight's text): at most 512 bytes
    /// and the only text of a selection that is kept.
    lead: Option<(At, String)>,
    /// A page turn that is made, and where the selection goes on after it.
    pending: Option<Pending>,
    /// What the strip at the bottom says instead of the legend (the selection
    /// is at its page limit, or at the book's start or end) until the next key:
    /// a notice would outlast Back.
    limit: Option<&'static str>,
    /// Whether the key layer of the selection is put in front.
    layer: bool,
    /// Where the page changes by a move, to redraw only that.
    dirty: Option<Rect>,
    /// The links of a page, read once for it.
    links: Option<(Locator, Vec<PageLink>)>,
    /// Choosing a link with the buttons: which has the focus (none: the page
    /// has none).
    linking: Option<Option<usize>>,
    /// Which link takes the focus on the page that follows a turn: the first, or
    /// the last when the choosing went back.
    enter: Option<bool>,
    /// Where followed links were left from.
    returns: Returns,
    /// The note that is to be asked for (see [`Marking::take_note`]).
    note: Option<act::NoteAsk>,
    /// Whether a screen is offered for `core.translate`, asked once for the
    /// selection (the bar's *Translate* is there only then).
    translate: Cell<Option<bool>>,
}

/// The action that starts choosing text with the buttons.
pub const SELECT: &str = "reader.select-text";
/// The action that starts choosing a link with the buttons.
pub const LINKS: &str = "reader.links";
/// The key layer put in front while text is chosen: the hold keys mean other
/// things there (declared in `declarations::SELECTING_KEYS`).
pub const LAYER: &str = "reader.selecting";
/// Holding Select while choosing: one step wider (word, sentence, paragraph).
pub const WIDEN: &str = "reader.sel-widen";
/// Holding Left: the first word of the page, or the start a word earlier.
pub const HOLD_LEFT: &str = "reader.sel-hold-left";
/// Holding Right: the last word of the page, or the start a word later.
pub const HOLD_RIGHT: &str = "reader.sel-hold-right";
/// Holding the page-back button: a paragraph up.
pub const HOLD_UP: &str = "reader.sel-hold-up";
/// Holding the page-forward button: a paragraph down.
pub const HOLD_DOWN: &str = "reader.sel-hold-down";

/// The navigation action as a constant, of the ones that move a word or a line.
fn nav(action: &str) -> Option<&'static str> {
    Some(match action {
        "core.left" => "core.left",
        "core.right" => "core.right",
        "core.up" => "core.up",
        "core.down" => "core.down",
        "core.next" => "core.next",
        "core.previous" => "core.previous",
        "core.next-page" => "core.next-page",
        "core.prev-page" => "core.prev-page",
        _ => return None,
    })
}

/// Whether the navigation `action` goes on in the book.
fn goes_on(action: &str) -> bool {
    matches!(
        action,
        "core.right" | "core.down" | "core.next" | "core.next-page" | HOLD_DOWN
    )
}

impl Marking {
    /// Nothing marked.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selection: None,
            lead: None,
            pending: None,
            layer: false,
            dirty: None,
            links: None,
            linking: None,
            enter: None,
            returns: Returns::new(),
            note: None,
            limit: None,
            translate: Cell::new(None),
        }
    }

    /// Lets go of everything marked on the page (the page jumped): the chosen
    /// words and the links read for it. Choosing links with the buttons goes on
    /// (it turns pages), and the way back stays.
    pub fn reset(&mut self) {
        self.selection = None;
        self.lead = None;
        self.pending = None;
        self.limit = None;
        self.translate.set(None);
        self.links = None;
    }

    /// The turn that was asked for did not move the page (the last one): the
    /// focus stays on the link it was on, a selection says it is at the book's
    /// end or start.
    pub fn stay(&mut self, host: &Host<'_>) {
        self.enter = None;
        if let Some(pending) = self.pending.take() {
            // In the legend's strip, not a notice: Back takes it away with the selection.
            self.limit = Some(if pending.forward {
                "reader.select.book-end"
            } else {
                "reader.select.book-start"
            });
            let mut scratch = [0; 64];
            // A redraw that is refused leaves the line for the next draw.
            CommandSender::new(host, &mut scratch)
                .queue_redraw(RedrawMode::Partial)
                .ok();
        }
    }

    /// The note *Note* started: the stretch it belongs to and the note it has,
    /// to be asked for once (taken).
    pub fn take_note(&mut self) -> Option<act::NoteAsk> {
        self.note.take()
    }

    /// Whether text is chosen.
    #[must_use]
    pub const fn is_selecting(&self) -> bool {
        self.selection.is_some()
    }

    /// Whether text or a link is being chosen: a tap is the marking's then.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.selection.is_some() || self.linking.is_some()
    }

    /// Whether a bar is shown over the status line.
    #[must_use]
    pub fn shows_bar(&self) -> bool {
        self.selection.is_some_and(|s| s.mode == Mode::Bar)
    }

    /// Takes an input if it is the marking's.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        input: &Input<'_>,
    ) -> Result<Outcome, Error> {
        let ctx = &Ctx {
            now_ms: match *input {
                Input::Action { now_ms, .. } | Input::Gesture { now_ms, .. } => now_ms,
                _ => 0,
            },
            ..*ctx
        };
        // The page turned since the last input: the selection goes on on it.
        self.settle(ctx);
        self.dirty = None;
        let had_limit = self.limit.take().is_some();
        let mut outcome = match *input {
            Input::Action { action, .. } => self.on_action(host, ctx, action)?,
            Input::Gesture { position, kind, .. } => self.on_gesture(host, ctx, position, kind)?,
            _ => Outcome::Ignored,
        };
        // The page limit's line goes with the next key.
        if had_limit && self.limit.is_none() && outcome == Outcome::Handled {
            outcome = Outcome::Redraw;
        }
        self.sync_layer(host);
        if matches!(outcome, Outcome::Redraw | Outcome::Edited) {
            let mut scratch = [0; 64];
            let mut sender = CommandSender::new(host, &mut scratch);
            // A move on the page redraws the boxes it changed, not the page.
            let area = self.dirty.take().filter(|_| outcome == Outcome::Redraw);
            let asked = area.is_some_and(|r| {
                let area = FrameRect::new(
                    i32::from(r.x) - 8,
                    i32::from(r.y) - 6,
                    u32::from(r.width) + 16,
                    u32::from(r.height) + 12,
                );
                sender.queue_redraw_area(RedrawMode::Partial, area).is_ok()
            });
            if !asked {
                sender.queue_redraw(RedrawMode::Partial)?;
            }
        }
        Ok(outcome)
    }

    /// Whether a word is chosen (or being chosen) on the page.
    #[must_use]
    pub fn chosen(&self) -> bool {
        self.selection.is_some()
    }

    /// Puts the key layer of the selection in front while text is chosen, and
    /// takes it away after.
    fn sync_layer(&mut self, host: &Host<'_>) {
        let want = self.selection.is_some();
        if want == self.layer {
            return;
        }
        let mut scratch = [0; 256];
        let layer = if want { LAYER } else { "" };
        if CommandSender::new(host, &mut scratch)
            .set_key_layer(layer)
            .is_ok()
        {
            self.layer = want;
            name_select(host, !want);
        }
    }

    fn set(&mut self, to: Option<Live>) -> Outcome {
        self.selection = to;
        if to.is_none() {
            self.lead = None;
            self.pending = None;
            self.limit = None;
            self.translate.set(None);
        }
        Outcome::Redraw
    }

    /// Whether the bar has *Translate*: the translate plugin is on.
    fn translate_on(&self, host: &Host<'_>) -> bool {
        self.translate.get().unwrap_or_else(|| {
            let offered = act::translate_offered(host).unwrap_or(false);
            self.translate.set(Some(offered));
            offered
        })
    }

    /// The page on show is the one a turn was asked for: the selection's end
    /// goes on at its first (or last) word. Keeps the start of the text of the
    /// stretch where the page has its first word.
    fn settle(&mut self, ctx: &Ctx<'_>) {
        let Some(mut live) = self.selection else {
            self.pending = None;
            return;
        };
        if ctx.words.is_empty() {
            return;
        }
        if let Some(pending) = self.pending
            && pending.page != ctx.page_start
        {
            self.pending = None;
            live.pages.turned(pending.forward);
            if let Some(side) = pending.side {
                live.land(ctx.words, side, pending.forward);
            }
            self.selection = Some(live);
        }
        // Choosing one word moves it at every key: no text is held for it.
        if live.mode != Mode::Choosing
            && let Some(first) = live.from.index_in(ctx.words)
            && self.lead.as_ref().is_none_or(|(at, _)| *at != live.from)
        {
            self.lead = lead_of(ctx.words, first).ok().map(|text| (live.from, text));
        }
    }

    /// The selection turns the page (`side`: the end that goes on; none: it
    /// stays), if it may take in one page more.
    fn turn(
        &mut self,
        ctx: &Ctx<'_>,
        live: Live,
        (side, forward): (Option<Side>, bool),
    ) -> Outcome {
        if !live.pages.allows(forward) {
            self.limit = Some("reader.select.page-limit");
            return Outcome::Redraw;
        }
        // The start of the text is kept before its page is left.
        if let Some(first) = live.from.index_in(ctx.words)
            && live.mode != Mode::Choosing
        {
            self.lead = lead_of(ctx.words, first).ok().map(|text| (live.from, text));
        }
        self.pending = Some(Pending {
            side,
            forward,
            page: ctx.page_start,
        });
        Outcome::Turn { forward }
    }

    fn on_action(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        action: &str,
    ) -> Result<Outcome, Error> {
        let Some(live) = self.selection else {
            if action == SELECT || action == HOLD_LEFT {
                // Choose with the buttons, from the first word of the page.
                let first = ctx.words.iter().position(|w| !w.lookup_text().is_empty());
                return Ok(first
                    .and_then(|w| ctx.words.get(w))
                    .map_or(Outcome::Handled, |word| {
                        self.set(Some(Live::choosing(At::of(word))))
                    }));
            }
            return self.on_link_action(host, ctx, action);
        };
        if action == "core.back" || action == "core.menu" {
            return Ok(self.set(None));
        }
        // The selection's own actions: declared, so they are run (#265: a
        // highlight asked for this way was answered and never made).
        if let Some(item) = Item::of_action(action) {
            return self.activate(host, ctx, live, item);
        }
        match live.mode {
            Mode::Choosing | Mode::Extending => Ok(self.on_moving(host, ctx, live, action)),
            Mode::Bar => self.on_bar_action(host, ctx, live, action),
        }
    }

    /// A key while the cursor or the end is moved: Left and Right a word, the
    /// page buttons a line, holding them the page's ends, the start and the
    /// paragraph; holding Select widens; Select is done.
    fn on_moving(&mut self, host: &Host<'_>, ctx: &Ctx<'_>, live: Live, action: &str) -> Outcome {
        if action == "core.confirm" {
            return self.show_bar(host, ctx, live);
        }
        let choosing = live.mode == Mode::Choosing;
        let (mv, side) = match action {
            WIDEN => {
                let mut wider = live;
                return if wider.widen(ctx.words) {
                    self.moved(ctx, live, wider)
                } else {
                    Outcome::Handled
                };
            }
            HOLD_LEFT if choosing => (Move::Page(false), Side::Both),
            HOLD_RIGHT if choosing => (Move::Page(true), Side::Both),
            HOLD_LEFT => (Move::Nav("core.left"), Side::Start),
            HOLD_RIGHT => (Move::Nav("core.right"), Side::Start),
            HOLD_UP => (Move::Paragraph(false), live.arrows()),
            HOLD_DOWN => (Move::Paragraph(true), live.arrows()),
            other => match nav(other) {
                Some(name) => (Move::Nav(name), live.arrows()),
                None => return Outcome::Ignored,
            },
        };
        // A page without words (a picture): the buttons go on to the next page.
        if ctx.words.is_empty() {
            return self.turn(ctx, live, (Some(side), goes_on(action)));
        }
        match live.step(ctx.words, mv, side) {
            Went::Stays => Outcome::Handled,
            Went::To(index) => {
                let mut next = live;
                next.put(ctx.words, side, index);
                self.moved(ctx, live, next)
            }
            Went::Edge(forward) => self.turn(ctx, live, (Some(side), forward)),
        }
    }

    /// The selection changed from `old` to `new` on this page: drawn again, only
    /// where it changed if the bar and the legend stay.
    fn moved(&mut self, ctx: &Ctx<'_>, old: Live, new: Live) -> Outcome {
        if new == old {
            return Outcome::Handled;
        }
        if new.mode == old.mode && new.unit == old.unit {
            self.dirty = box_of(ctx.words, &old)
                .zip(box_of(ctx.words, &new))
                .map(|(a, b)| a.union(&b));
        }
        self.set(Some(new))
    }

    /// Select: done, the bar with *Highlight* in focus.
    fn show_bar(&mut self, host: &Host<'_>, ctx: &Ctx<'_>, live: Live) -> Outcome {
        let touch = snapshot::capture_snapshot(host).is_ok_and(|s| s.has_touch());
        let items = bar_items(ctx, &live, touch, self.translate_on(host));
        self.set(Some(Live {
            mode: Mode::Bar,
            focus: Some(items.highlight_at()),
            ..live
        }))
    }

    fn on_bar_action(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        live: Live,
        action: &str,
    ) -> Result<Outcome, Error> {
        let touch = snapshot::capture_snapshot(host)?.has_touch();
        let items = bar_items(ctx, &live, touch, self.translate_on(host));
        let items = items.as_slice();
        let focus = live.focus.unwrap_or(0);
        let last = items.len() - 1;
        let step = |forward: bool| Live {
            focus: Some(if forward {
                if focus >= last { 0 } else { focus + 1 }
            } else {
                focus.checked_sub(1).unwrap_or(last)
            }),
            ..live
        };
        match action {
            "core.confirm" => match items.get(focus) {
                Some(&item) => self.activate(host, ctx, live, item),
                None => Ok(Outcome::Handled),
            },
            // On a touch screen a swipe turns the page and the selection stays;
            // a tap on the new page moves its end.
            "core.prev-page" | "core.next-page" if touch => {
                Ok(self.turn(ctx, live, (None, action == "core.next-page")))
            }
            "core.left" | "core.up" | "core.previous" | "core.prev-page" => {
                Ok(self.set(Some(step(false))))
            }
            "core.right" | "core.down" | "core.next" | "core.next-page" => {
                Ok(self.set(Some(step(true))))
            }
            _ => Ok(Outcome::Ignored),
        }
    }

    fn on_gesture(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        at: Point,
        kind: InputGestureKind,
    ) -> Result<Outcome, Error> {
        // Any touch ends choosing links, as in Beta 3.
        if self.linking.is_some() {
            self.linking = None;
            return Ok(Outcome::Redraw);
        }
        let word = word_at(ctx.words, at.x, at.y);
        if self.selection.is_none()
            && kind == InputGestureKind::Tap
            && let Some(outcome) = self.on_tap(host, ctx, at, word)?
        {
            return Ok(outcome);
        }
        match (kind, self.selection) {
            (InputGestureKind::LongPress, _) => Ok(match word.and_then(|w| ctx.words.get(w)) {
                Some(w) => self.set(Some(Live::touched(At::of(w)))),
                None => Outcome::Ignored,
            }),
            (InputGestureKind::Tap, Some(live)) => {
                let bar = self.hit_bar(host, ctx, live, at)?;
                if let Some(item) = bar {
                    return self.activate(host, ctx, live, item);
                }
                Ok(match word {
                    Some(w) => {
                        let mut next = live;
                        next.touch_to(ctx.words, w);
                        self.set(Some(Live {
                            mode: Mode::Bar,
                            focus: None,
                            ..next
                        }))
                    }
                    None => self.set(None),
                })
            }
            _ => Ok(Outcome::Ignored),
        }
    }

    /// The bar item a touch at `at` is on, where the bar is shown.
    fn hit_bar(
        &self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        live: Live,
        at: Point,
    ) -> Result<Option<Item>, Error> {
        if live.mode != Mode::Bar {
            return Ok(None);
        }
        let (font, display) = (
            Font::acquire_role(host, FontRole::Ui)?,
            snapshot::capture_snapshot(host)?,
        );
        let found = UiStyle::read(host, &display, &font)
            .and_then(|style| {
                let items = bar_items(ctx, &live, display.has_touch(), self.translate_on(host));
                Bar::new(host, &font, &style, &display, items.as_slice())
            })
            .map(|bar| bar.item_at(at));
        let released = font.release();
        found.and_then(|item| released.map(|()| item))
    }

    /// The highlight the chosen text is: its stretch, and its text (all of it
    /// from the page, or the start the selection kept where it runs over pages).
    fn highlight_of(&mut self, ctx: &Ctx<'_>, live: &Live) -> Result<(Span, String), Error> {
        let span = act::span_of(live.span());
        let text = match live.on_page(ctx.words) {
            Some((lo, hi)) => act::text_of(ctx.words, lo, hi)?,
            None => match self.lead.take() {
                Some((at, text)) if at == live.from => text,
                _ => String::new(),
            },
        };
        Ok((span, text))
    }

    fn activate(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        live: Live,
        item: Item,
    ) -> Result<Outcome, Error> {
        // Look up, translate and copy take the words of the page: only where all are on it.
        let one_page = live.on_page(ctx.words).and_then(|(lo, hi)| {
            let (first, last) = (ctx.words.get(lo)?, ctx.words.get(hi)?);
            Some((lo, hi, first.start, last.end))
        });
        match item {
            Item::LookUp => {
                if let Some((lo, hi, start, end)) = one_page {
                    act::look_up(host, ctx, lo, hi, (start, end))?;
                }
                // The words stay chosen under the lookup, as in Beta 3.
                Ok(Outcome::Handled)
            }
            Item::Translate => {
                if let Some((lo, hi, ..)) = one_page
                    && self.translate_on(host)
                {
                    act::translate(host, ctx, lo, hi)?;
                }
                // The words stay chosen under the window, as under the lookup.
                Ok(Outcome::Handled)
            }
            Item::Copy => {
                let Some((lo, hi, ..)) = one_page else {
                    return Ok(Outcome::Handled);
                };
                let text = act::text_of(ctx.words, lo, hi)?;
                act::copy(host, ctx.ui.ok_or(Error::State)?, &text)?;
                Ok(self.set(None))
            }
            Item::Extend => Ok(self.set(Some(Live {
                mode: Mode::Extending,
                focus: None,
                ..live
            }))),
            Item::Cancel => Ok(self.set(None)),
            Item::Highlight | Item::Unhighlight => {
                let (span, text) = self.highlight_of(ctx, &live)?;
                act::highlight(host, ctx, span, &text)?;
                self.set(None);
                Ok(Outcome::Edited)
            }
            Item::Note | Item::EditNote => {
                let (span, text) = self.highlight_of(ctx, &live)?;
                self.note = act::note(host, ctx, span, &text)?;
                self.set(None);
                Ok(Outcome::Edited)
            }
        }
    }

    /// Draws what is marked over the page: a frame round the chosen words, and
    /// the bar (or the legend of the keys) where the selection is.
    ///
    /// # Errors
    /// Allocation, font and drawing errors.
    pub fn draw_overlay(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        // A turn made by the selection: it goes on at the page on show.
        self.settle(ctx);
        let way_back = self.returns.offered_on(ctx.page_start);
        let selecting = self.selection.is_some();
        if !selecting && self.linking.is_none() && !way_back {
            return Ok(());
        }
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let drawn = self.draw_with(host, ctx, frame, &font);
        let released = font.release();
        drawn.and(released)
    }

    fn draw_with(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        frame: &mut DrawFrame<'_>,
        font: &Font<'_>,
    ) -> Result<(), Error> {
        let display: Snapshot = snapshot::capture_snapshot(host)?;
        let style = UiStyle::read(host, &display, font)?;
        let mut ops: Vec<Paint> = Vec::new();
        if let Some(live) = self.selection {
            frame_words(&mut ops, &style, ctx.words, |i| {
                ctx.words.get(i).is_some_and(|w| live.covers(w))
            })?;
            if live.mode == Mode::Bar {
                let items = bar_items(ctx, &live, display.has_touch(), self.translate_on(host));
                let bar = Bar::new(host, font, &style, &display, items.as_slice())?;
                bar.prepare(&mut ops, font, &style, live.focus)?;
            } else {
                let mut messages = Messages::new()?;
                let (first, second) = if live.mode == Mode::Choosing {
                    ("reader.select.legend.word", None)
                } else {
                    (
                        "reader.select.legend.range",
                        Some("reader.select.legend.range2"),
                    )
                };
                let first = self.limit.unwrap_or(first);
                let one = messages.copy(host, first)?;
                let two = second.map(|key| messages.copy(host, key)).transpose()?;
                let lines = [one.as_str(), two.as_deref().unwrap_or("")];
                let shown = if two.is_some() && self.limit.is_none() {
                    &lines[..]
                } else {
                    &lines[..1]
                };
                linking::bottom_line(&mut ops, font, &style, &display, (shown, true))?;
            }
        }
        self.draw_links(&mut ops, host, ctx, font, &style, &display)?;
        paint::replay(frame, font, ops.iter())
    }
}

/// The items of the bar of `live`: *Look up*, *Translate* and *Copy* only on one
/// page, *Translate* only where the plugin is on (`translate`).
fn bar_items(ctx: &Ctx<'_>, live: &Live, touch: bool, translate: bool) -> bar::Items {
    bar::items(
        touch,
        act::standing(ctx, &act::span_of(live.span())),
        live.on_page(ctx.words).is_some(),
        translate,
    )
}

/// The box round the page's chosen words.
fn box_of(words: &[Word], live: &Live) -> Option<Rect> {
    words
        .iter()
        .filter(|w| live.covers(w))
        .map(|w| w.area)
        .reduce(|a, b| a.union(&b))
}

/// A frame round each run of the words `picked` on a line, as Beta 3 frames chosen
/// text and a link being chosen.
fn frame_words(
    ops: &mut Vec<Paint>,
    style: &UiStyle,
    words: &[Word],
    picked: impl Fn(usize) -> bool,
) -> Result<(), Error> {
    let line = u32::from(style.line).max(2);
    for b in line_boxes(words, picked) {
        let framed = paperpaw_sdk::Rect::new(
            b.x.saturating_sub(4),
            b.y.saturating_sub(2),
            b.width.saturating_add(8),
            b.height.saturating_add(4),
        );
        paint::push(ops, Paint::Focus(framed, 0, Some(line), style.ink))?;
    }
    Ok(())
}

/// The text of the page's words `lo..=hi`, a space between them.
///
/// # Errors
/// `NoMemory`.
pub fn text_of(words: &[Word], lo: usize, hi: usize) -> Result<String, Error> {
    act::text_of(words, lo, hi)
}

/// Names Select in the key bar of the reading screen: *Menu*, which it opens
/// there (as in Beta 3), or its usual name while text is chosen, where it
/// widens the choice. A hint the host refuses leaves the bar as it was. For a
/// callback that may send commands (an input or a show); its own frame.
#[inline(never)]
pub fn name_select(host: &Host<'_>, menu: bool) {
    const SELECT: &str = "core.confirm";
    let word = if menu {
        match Messages::new().and_then(|mut say| say.copy(host, "core.hint.core.menu")) {
            Ok(word) => Some(word),
            Err(_) => return,
        }
    } else {
        None
    };
    let hint = match &word {
        Some(word) => ButtonHint::words(SELECT, word),
        None => ButtonHint::usual(SELECT),
    };
    if let Ok(hint) = hint {
        match CommandSender::new(host, &mut [0; 256]).set_hint(hint) {
            Ok(()) | Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests;
