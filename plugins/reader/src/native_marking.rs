//! What is marked on the page and what a touch or a key does with it: the words
//! chosen with their bar (look up, copy, extend), and the links of the page. One
//! [`Marking`] belongs to an open reading screen; the screen asks it first about
//! every input (a chosen word or link takes the arrows and the confirm key before
//! a page turn) and draws its [`overlay`](Marking::draw_overlay) over the page.
//!
//! The interface-6 screen (`screen/selecting.rs`, `selection.rs`) is the
//! reference; this is its host-free movement (`native_selection::moves`) with
//! the bar, the clipboard and the lookup done through the interface-7 services.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point,
    document::Locator,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        library::Library,
        snapshot::{self, Snapshot},
        ui::Ui,
    },
    reading::WordBox,
};
use paperpaw_ui_kit::{
    Style as UiStyle,
    paint::{self, Paint},
};

use crate::native_links::{DocRef, PageLink, Returns};
use crate::native_marks::Span;
use crate::native_pages;
use crate::native_place::Progress;
use crate::native_selection::{Mode, Selection, line_boxes};
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
    pub words: &'a [WordBox],
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
        words: &'a [WordBox],
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

/// The state of the marking on an open page.
#[derive(Debug, Default)]
pub struct Marking {
    selection: Option<Selection>,
    /// The page the selection is on.
    on: Option<Locator>,
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
}

/// The action that starts choosing text with the buttons.
pub const SELECT: &str = "reader.select-text";
/// The action that starts choosing a link with the buttons.
pub const LINKS: &str = "reader.links";

impl Marking {
    /// Nothing marked.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selection: None,
            on: None,
            links: None,
            linking: None,
            enter: None,
            returns: Returns::new(),
            note: None,
        }
    }

    /// Lets go of everything marked on the page (the page changed): the chosen
    /// words and the links read for it. Choosing links with the buttons goes on
    /// (it turns pages), and the way back stays.
    pub fn reset(&mut self) {
        self.selection = None;
        self.on = None;
        self.links = None;
    }

    /// The turn that was asked for did not move the page (the last one): the
    /// focus stays on the link it was on.
    pub fn stay(&mut self) {
        self.enter = None;
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
        // A chosen word belongs to the page it was chosen on.
        if self.on.is_some_and(|on| on != ctx.page_start) {
            self.reset();
        }
        let ctx = &Ctx {
            now_ms: match *input {
                Input::Action { now_ms, .. } | Input::Gesture { now_ms, .. } => now_ms,
                _ => 0,
            },
            ..*ctx
        };
        let outcome = match *input {
            Input::Action { action, .. } => self.on_action(host, ctx, action)?,
            Input::Gesture { position, kind, .. } => self.on_gesture(host, ctx, position, kind)?,
            _ => Outcome::Ignored,
        };
        if matches!(outcome, Outcome::Redraw | Outcome::Edited) {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        }
        Ok(outcome)
    }

    fn set(&mut self, ctx: &Ctx<'_>, to: Option<Selection>) -> Outcome {
        self.selection = to;
        self.on = to.map(|_| ctx.page_start);
        Outcome::Redraw
    }

    fn on_action(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        action: &str,
    ) -> Result<Outcome, Error> {
        let Some(selection) = self.selection else {
            if action == SELECT {
                // Choose with the buttons, from the first word of the page.
                let first = ctx.words.iter().position(|w| !w.lookup_text().is_empty());
                return Ok(first.map_or(Outcome::Handled, |w| {
                    self.set(ctx, Some(Selection::choosing(w)))
                }));
            }
            return self.on_link_action(host, ctx, action);
        };
        if ctx.words.is_empty() {
            self.reset();
            return Ok(Outcome::Redraw);
        }
        if action == "core.back" || action == "core.menu" {
            return Ok(self.set(ctx, None));
        }
        match selection.mode {
            Mode::Choosing | Mode::Extending => {
                if action == "core.confirm" {
                    let to = Selection {
                        mode: Mode::Bar,
                        focus: Some(0),
                        ..selection
                    };
                    return Ok(self.set(ctx, Some(to)));
                }
                let Some(cursor) = selection.moved(action, ctx.words) else {
                    return Ok(Outcome::Ignored);
                };
                let anchor = if selection.mode == Mode::Choosing {
                    cursor
                } else {
                    selection.anchor
                };
                Ok(self.set(
                    ctx,
                    Some(Selection {
                        anchor,
                        cursor,
                        ..selection
                    }),
                ))
            }
            Mode::Bar => self.on_bar_action(host, ctx, selection, action),
        }
    }

    fn on_bar_action(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        selection: Selection,
        action: &str,
    ) -> Result<Outcome, Error> {
        let touch = snapshot::capture_snapshot(host)?.has_touch();
        let items = bar::items(touch, act::standing(ctx, selection.range()));
        let focus = selection.focus.unwrap_or(0);
        let last = items.len() - 1;
        let step = |forward: bool| Selection {
            focus: Some(if forward {
                if focus >= last { 0 } else { focus + 1 }
            } else {
                focus.checked_sub(1).unwrap_or(last)
            }),
            ..selection
        };
        match action {
            "core.confirm" => match items.get(focus) {
                Some(&item) => self.activate(host, ctx, selection, item),
                None => Ok(Outcome::Handled),
            },
            "core.left" | "core.up" | "core.previous" | "core.prev-page" => {
                Ok(self.set(ctx, Some(step(false))))
            }
            "core.right" | "core.down" | "core.next" | "core.next-page" => {
                Ok(self.set(ctx, Some(step(true))))
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
            (InputGestureKind::LongPress, _) => Ok(match word {
                Some(w) => self.set(ctx, Some(Selection::touched(w))),
                None => Outcome::Ignored,
            }),
            (InputGestureKind::Tap, Some(selection)) => {
                let bar = self.hit_bar(host, ctx, selection, at)?;
                if let Some(item) = bar {
                    return self.activate(host, ctx, selection, item);
                }
                Ok(match word {
                    Some(w) => self.set(
                        ctx,
                        Some(Selection {
                            cursor: w,
                            mode: Mode::Bar,
                            ..selection
                        }),
                    ),
                    None => self.set(ctx, None),
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
        selection: Selection,
        at: Point,
    ) -> Result<Option<Item>, Error> {
        if selection.mode != Mode::Bar {
            return Ok(None);
        }
        let (font, display) = (
            Font::acquire_role(host, FontRole::Ui)?,
            snapshot::capture_snapshot(host)?,
        );
        let found = UiStyle::read(host, &display, &font)
            .and_then(|style| {
                let items = bar::items(display.has_touch(), act::standing(ctx, selection.range()));
                Bar::new(host, &font, &style, &display, &items)
            })
            .map(|bar| bar.item_at(at));
        let released = font.release();
        found.and_then(|item| released.map(|()| item))
    }

    fn activate(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        selection: Selection,
        item: Item,
    ) -> Result<Outcome, Error> {
        let (lo, hi) = selection.range();
        let (Some(first), Some(last)) = (ctx.words.get(lo), ctx.words.get(hi)) else {
            return Ok(self.set(ctx, None));
        };
        match item {
            Item::LookUp => {
                act::look_up(host, ctx, lo, hi, (first.start, last.end))?;
                // The words stay chosen under the lookup, as in Beta 3.
                Ok(Outcome::Handled)
            }
            Item::Copy => {
                let text = act::text_of(ctx.words, lo, hi)?;
                act::copy(host, ctx.ui.ok_or(Error::State)?, &text)?;
                Ok(self.set(ctx, None))
            }
            Item::Extend => Ok(self.set(
                ctx,
                Some(Selection {
                    mode: Mode::Extending,
                    focus: None,
                    ..selection
                }),
            )),
            Item::Cancel => Ok(self.set(ctx, None)),
            Item::Highlight | Item::Unhighlight => {
                act::highlight(host, ctx, (lo, hi))?;
                self.selection = None;
                self.on = None;
                Ok(Outcome::Edited)
            }
            Item::Note | Item::EditNote => {
                self.note = act::note(host, ctx, (lo, hi))?;
                self.selection = None;
                self.on = None;
                Ok(Outcome::Edited)
            }
        }
    }

    /// Draws what is marked over the page: a frame round the chosen words, and
    /// the bar where it is shown.
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
        let way_back = self.returns.offered_on(ctx.page_start);
        let selecting = self.selection.is_some() && self.on == Some(ctx.page_start);
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
        if let Some(selection) = self.selection.filter(|_| self.on == Some(ctx.page_start)) {
            let (lo, hi) = selection.range();
            frame_words(&mut ops, &style, ctx.words, |i| (lo..=hi).contains(&i))?;
            if selection.mode == Mode::Bar {
                let items = bar::items(display.has_touch(), act::standing(ctx, selection.range()));
                let bar = Bar::new(host, font, &style, &display, &items)?;
                bar.prepare(&mut ops, font, &style, selection.focus)?;
            }
        }
        self.draw_links(&mut ops, host, ctx, font, &style, &display)?;
        paint::replay(frame, font, ops.iter())
    }
}

/// A frame round each run of the words `picked` on a line, as Beta 3 frames chosen
/// text and a link being chosen.
fn frame_words(
    ops: &mut Vec<Paint>,
    style: &UiStyle,
    words: &[WordBox],
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
pub fn text_of(words: &[WordBox], lo: usize, hi: usize) -> Result<String, Error> {
    act::text_of(words, lo, hi)
}

#[cfg(test)]
mod tests;
