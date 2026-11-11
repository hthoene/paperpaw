//! Links on the page: touched, chosen with the buttons, followed, and the way
//! back. See `native_links` for what a link is and how it is read.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error, Host,
        commands::CommandSender,
        document::LinkKind,
        fonts::Font,
        snapshot::Snapshot,
        ui::{Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Chips, Messages, Style as UiStyle,
    mem::{SERVICE_BUFFER, buffer},
    paint::{self, Paint},
    style::setting_text,
    text,
};

use super::{Ctx, Marking, Outcome, frame_words};
use crate::native_links::{self, PageLink, is_external, label_of, link_index_of, note_like};

/// The setting that says whether notes are shown over the page or gone to.
const FOOTNOTES: &str = "core.footnotes";
const FOOTNOTES_POPUP: &str = "core.footnotes.popup";

impl Marking {
    /// The links of the page, read for it once.
    fn links_of(&mut self, host: &Host<'_>, ctx: &Ctx<'_>) -> Result<&[PageLink], Error> {
        if self
            .links
            .as_ref()
            .is_none_or(|(at, _)| *at != ctx.page_start)
        {
            let read = match &ctx.doc {
                Some(doc) => native_links::read_links(host, doc, ctx.words)?,
                None => Vec::new(),
            };
            self.links = Some((ctx.page_start, read));
        }
        Ok(self.links.as_ref().map_or(&[], |(_, links)| links))
    }

    /// An action while no text is chosen: the way back, choosing links.
    pub(super) fn on_link_action(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        action: &str,
    ) -> Result<Outcome, Error> {
        if let Some(focus) = self.linking {
            return self.while_choosing(host, ctx, action, focus);
        }
        match action {
            super::LINKS => {
                let count = self.links_of(host, ctx)?.len();
                self.linking = Some((count > 0).then_some(0));
                Ok(Outcome::Redraw)
            }
            "core.back" => Ok(self.way_back()),
            _ => Ok(Outcome::Ignored),
        }
    }

    /// Back to where the last link was followed from, if one was.
    fn way_back(&mut self) -> Outcome {
        match self.returns.back() {
            Some(place) => Outcome::Jump {
                section: place.section,
                offset: place.offset,
            },
            None => Outcome::Ignored,
        }
    }

    /// Buttons while choosing a link: the next or previous one (on to the next or
    /// previous page past the last or first), *confirm* follows it, *back* ends.
    fn while_choosing(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        action: &str,
        focus: Option<usize>,
    ) -> Result<Outcome, Error> {
        let count = self.links_of(host, ctx)?.len();
        match action {
            "core.right" | "core.down" | "core.next" | "core.next-page" => match focus {
                Some(i) if i + 1 < count => {
                    self.linking = Some(Some(i + 1));
                    Ok(Outcome::Redraw)
                }
                _ => {
                    self.enter = Some(false);
                    Ok(Outcome::Turn { forward: true })
                }
            },
            "core.left" | "core.up" | "core.previous" | "core.prev-page" => match focus {
                Some(i) if i > 0 => {
                    self.linking = Some(Some(i - 1));
                    Ok(Outcome::Redraw)
                }
                _ => {
                    self.enter = Some(true);
                    Ok(Outcome::Turn { forward: false })
                }
            },
            "core.confirm" => {
                self.links_of(host, ctx)?;
                self.linking = None;
                match focus {
                    Some(i) => self.follow(host, ctx, i),
                    None => Ok(Outcome::Redraw),
                }
            }
            "core.back" | "core.menu" => {
                self.linking = None;
                Ok(Outcome::Redraw)
            }
            // Anything else ends choosing and goes on as usual.
            _ => {
                self.linking = None;
                Ok(Outcome::Ignored)
            }
        }
    }

    /// A tap with no text chosen: the way back, or a link under the finger.
    /// `None`: not a tap for the marking.
    pub(super) fn on_tap(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        at: Point,
        word: Option<usize>,
    ) -> Result<Option<Outcome>, Error> {
        if self.returns.offered_on(ctx.page_start)
            && self.back_button(host, ctx)?.is_some_and(|r| r.contains(at))
        {
            return Ok(Some(self.way_back()));
        }
        let Some(word) = word.and_then(|w| ctx.words.get(w)) else {
            return Ok(None);
        };
        let at = link_index_of(self.links_of(host, ctx)?, word);
        match at {
            Some(i) => Ok(Some(self.follow(host, ctx, i)?)),
            None => Ok(None),
        }
    }

    /// Opens a link: a note over the page where it looks like one and the reader
    /// shows notes that way; else a place in the book (the page is remembered for
    /// *Back*), or the web.
    fn follow(&mut self, host: &Host<'_>, ctx: &Ctx<'_>, index: usize) -> Result<Outcome, Error> {
        let Some(link) = self.links.as_ref().and_then(|(_, links)| links.get(index)) else {
            return Ok(Outcome::Handled);
        };
        let (href, ui) = (link.href.as_str(), ctx.ui.ok_or(Error::State)?);
        let Some(doc) = ctx.doc else {
            return Ok(Outcome::Handled);
        };
        let label = label_of(ctx.words, link)?;
        // An address too long to hand over is not followed (the input is taken).
        if link.kind == LinkKind::External || is_external(href) {
            return match open_web(host, ui, href, ctx.now_ms) {
                Ok(()) | Err(Error::NoSpace) => Ok(Outcome::Handled),
                Err(error) => Err(error),
            };
        }
        let popup = setting_text(host, FOOTNOTES).is_ok_and(|v| v == FOOTNOTES_POPUP);
        if popup
            && (link.kind == LinkKind::Footnote || note_like(&label))
            && let Some(note) = native_links::note_text(host, &doc, href)?
        {
            return match open_note(host, ui, &note, ctx.path, href) {
                Ok(()) | Err(Error::NoSpace) => Ok(Outcome::Handled),
                Err(error) => Err(error),
            };
        }
        let Some(target) = native_links::target_of(host, &doc, href)? else {
            return Ok(Outcome::Handled);
        };
        self.returns.followed(ctx.page_start)?;
        Ok(Outcome::Jump {
            section: target.section,
            offset: target.offset,
        })
    }

    /// Goes to the target of link `href` of the page (*Go to the note*): a place
    /// in the book, the page left remembered for *Back*.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub fn follow_target(
        &mut self,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        href: &str,
    ) -> Result<Outcome, Error> {
        let Some(doc) = ctx.doc else {
            return Ok(Outcome::Handled);
        };
        let Some(target) = native_links::target_of(host, &doc, href)? else {
            return Ok(Outcome::Handled);
        };
        self.returns.followed(ctx.page_start)?;
        Ok(Outcome::Jump {
            section: target.section,
            offset: target.offset,
        })
    }

    /// Where the page the link landed on is: the way back is offered there.
    pub fn landed(&mut self, page: paperpaw_sdk::document::Locator) {
        self.returns.landed_on(page);
    }

    /// The *Back* button over the page's top margin, on the left, where the way
    /// back is offered; none where it is not.
    fn back_button(&self, host: &Host<'_>, ctx: &Ctx<'_>) -> Result<Option<Rect>, Error> {
        if !self.returns.offered_on(ctx.page_start) {
            return Ok(None);
        }
        let font = Font::acquire_role(host, paperpaw_sdk::native::fonts::FontRole::Ui)?;
        let shown = back_chip(host, ctx, &font);
        let released = font.release();
        shown.and_then(|chip| released.map(|()| chip.and_then(|(c, _)| c.cell(0))))
    }

    /// The overlay of links: the one chosen, framed, with a bar at the bottom
    /// that says where it leads; and the *Back* button.
    pub(super) fn draw_links(
        &mut self,
        ops: &mut Vec<Paint>,
        host: &Host<'_>,
        ctx: &Ctx<'_>,
        font: &Font<'_>,
        style: &UiStyle,
        display: &Snapshot,
    ) -> Result<(), Error> {
        if let Some(focus) = self.linking {
            // On the page after a turn the first (or last) link has the focus.
            let count = self.links_of(host, ctx)?.len();
            let focus = match self.enter.take() {
                Some(last) => {
                    let at = if last {
                        count.checked_sub(1)
                    } else {
                        (count > 0).then_some(0)
                    };
                    self.linking = Some(at);
                    at
                }
                None => focus,
            };
            let link = focus.and_then(|i| self.links.as_ref()?.1.get(i));
            if let Some(link) = link {
                frame_words(ops, style, ctx.words, |i| {
                    ctx.words.get(i).is_some_and(|w| {
                        w.start.section == link.section
                            && w.end.offset > link.start
                            && w.start.offset < link.end
                    })
                })?;
            }
            link_bar(ops, host, font, style, display, link)?;
        }
        if let Some((chips, _)) =
            back_chip(host, ctx, font)?.filter(|_| self.returns.offered_on(ctx.page_start))
        {
            chips.prepare(ops, font, usize::MAX, None)?;
        }
        Ok(())
    }
}

/// The *Back* chip and its label, in the top margin on the left of the content
/// box so it covers no text.
fn back_chip(
    host: &Host<'_>,
    ctx: &Ctx<'_>,
    font: &Font<'_>,
) -> Result<Option<(Chips, String)>, Error> {
    let display = paperpaw_sdk::native::snapshot::capture_snapshot(host)?;
    let style = UiStyle::read(host, &display, font)?;
    let label = Messages::new()?.copy(host, "reader.link-back")?;
    let finger = Chips::finger(&display);
    let top = ctx.area.y;
    let height = Chips::height(&style, finger)
        .min(top.saturating_sub(4))
        .max(style.line.max(8));
    let y = top.saturating_sub(height) / 2;
    let room = Rect::new(
        ctx.area.x,
        y,
        display.display_width().saturating_sub(ctx.area.x),
        height,
    );
    let chips = Chips::new(font, &style, finger, room, &[label.as_str()])?;
    Ok(Some((chips, label)))
}

/// The bar at the bottom while a link is chosen: where it leads (the host of an
/// address, or *In this book*) and what the buttons do.
fn link_bar(
    ops: &mut Vec<Paint>,
    host: &Host<'_>,
    font: &Font<'_>,
    style: &UiStyle,
    display: &Snapshot,
    link: Option<&PageLink>,
) -> Result<(), Error> {
    let mut messages = Messages::new()?;
    let hint = messages.copy(host, "reader.link.hint")?;
    let said = match link {
        Some(link) => {
            let place = if is_external(&link.href) {
                text::copy(paperpaw_sdk::web::host(&link.href))?
            } else {
                messages.copy(host, "reader.link.in-book")?
            };
            let mut line = place;
            line.try_reserve(hint.len() + 5)
                .map_err(|_| Error::NoMemory)?;
            line.push_str(" · ");
            line.push_str(&hint);
            line
        }
        None => messages.copy(host, "reader.link.none")?,
    };
    let finger = Chips::finger(display);
    let height = Chips::height(style, finger).saturating_add(2 * style.margin);
    let (width, tall) = (display.display_width(), display.display_height());
    let bar = Rect::new(0, tall.saturating_sub(height), width, height);
    paint::push(ops, Paint::Fill(bar, style.paper))?;
    paint::push(
        ops,
        Paint::Fill(
            Rect::new(bar.x, bar.y, bar.width, style.line.max(2)),
            style.ink,
        ),
    )?;
    let margin = f32::from(style.margin);
    let room = f32::from(width) - 2.0 * margin;
    let fitted = text::fit(font, style.small, &said, room)?;
    text::label(
        ops,
        font,
        &fitted,
        style.small,
        margin,
        bar,
        style.ink,
        Some(bar),
    )
}

/// Opens the web page in the browser (`core.browser`): told apart by the time, so
/// a page asked for again is loaded again, not the open browser brought forward.
/// Where no browser is there, nothing happens.
fn open_web(host: &Host<'_>, ui: &Ui, href: &str, now_ms: u64) -> Result<(), Error> {
    let mut id = String::new();
    id.try_reserve(20).map_err(|_| Error::NoMemory)?;
    push_number(&mut id, now_ms);
    let params = [Param::new("url", href)?, Param::new("id", &id)?];
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    match CommandSender::new(host, &mut scratch)
        .open(Open::slot("core.browser")?.handover(handover))
    {
        Ok(()) | Err(Error::Unsupported | Error::NotFound | Error::Denied) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Opens the note over the page (`core.footnote`).
fn open_note(host: &Host<'_>, ui: &Ui, note: &str, path: &str, url: &str) -> Result<(), Error> {
    let params = [
        Param::new("text", note)?,
        Param::new("path", path)?,
        Param::new("url", url)?,
    ];
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot("core.footnote")?.handover(handover))
}

/// Appends `n` in decimal.
fn push_number(to: &mut String, mut n: u64) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for d in &digits[at..] {
        to.push(char::from(*d));
    }
}
