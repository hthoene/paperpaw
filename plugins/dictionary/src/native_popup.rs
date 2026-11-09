//! The lookup panel as a native overlay screen filling `core.lookup`: the word
//! and its entries from the providers (`core.lookup` service, one page per
//! provider) over the lower part of the screen, scrolled by button or tap.
use crate::native_panel::{Entry, MAX_ENTRIES, Panel, Step};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        formatting::Arg,
        lookup::{self, Lookup, Question},
        snapshot,
        ui::{HandoverId, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages, Paint, Style,
    mem::{SERVICE_BUFFER, buffer, push},
    paint, text,
};

/// How the panel is laid out in a frame, from the last draw.
#[derive(Debug, Default, Clone, Copy)]
struct Layout {
    region: Rect,
    rows: usize,
}

/// One open panel.
#[derive(Debug, Default)]
pub struct View {
    word: String,
    /// The folder the dictionaries are in, for the message when there are none.
    entries: Vec<Entry>,
    /// Built at the first draw, when the font is there to measure with.
    panel: Option<Panel>,
    layout: Layout,
}

impl View {
    /// The panel of the word and language in `handover`, with what the
    /// providers have to say.
    ///
    /// # Errors
    /// The host's refusals; `NoMemory`.
    pub fn open(host: &Host<'_>, ui: &Ui, handover: Option<HandoverId>) -> Result<Self, Error> {
        let mut view = Self::default();
        let mut language = String::new();
        if let Some(handover) = handover {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            for param in ui.params(host, handover, &mut input, &mut out)? {
                let param = param?;
                match param.key() {
                    "word" => view.word = text::copy(param.value())?,
                    "language" => language = text::copy(param.value())?,
                    _ => {}
                }
            }
        }
        view.entries = entries(host, &view.word, &language)?;
        Ok(view)
    }

    /// What an input did: scrolls, or closes the screen.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(panel) = self.panel.as_mut() else {
            return Ok(Handled::No);
        };
        match panel.decide(self.layout.rows, self.layout.region, input) {
            Step::Ignored => Ok(Handled::No),
            Step::Scroll(top) => {
                panel.set_top(top);
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
                Ok(Handled::Yes)
            }
            Step::Close => {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).close_screen()?;
                Ok(Handled::Yes)
            }
        }
    }

    /// Draws the panel over the lower part of the screen.
    ///
    /// # Errors
    /// Allocation, font, catalog and drawing errors; the layout of the last
    /// completed frame stays.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let top = height.saturating_mul(9) / 20;
        let region = Rect::new(0, top, width, height.saturating_sub(top));
        let line = paperpaw_ui_kit::geometry::line_px(&font, style.text)?.max(1);
        let header = style.row;
        let room = region
            .height
            .saturating_sub(header)
            .saturating_sub(style.gap);
        let rows = usize::from(room / line).max(1);
        let mut messages = Messages::new()?;
        if self.panel.is_none() {
            let empty = messages.format(
                host,
                "dictionary.not-found",
                &[Arg::Text {
                    name: "word",
                    value: &self.word,
                }],
            )?;
            let wide = f32::from(width.saturating_sub(2 * style.margin)).max(1.0);
            self.panel = Some(Panel::new(
                &self.entries,
                &empty,
                |t| font.measure_text(style.text, t),
                wide,
            )?);
        }
        let panel = self.panel.as_ref().ok_or(Error::State)?;
        let mut body = Vec::new();
        push(&mut body, Paint::Fill(region, style.paper))?;
        push(
            &mut body,
            Paint::Fill(
                Rect::new(region.x, region.y, region.width, style.line),
                style.rule,
            ),
        )?;
        let margin = f32::from(style.margin);
        let (page, pages) = panel.pages(rows);
        let note = if pages > 1 {
            messages.format(
                host,
                "dictionary.page",
                &[
                    Arg::Int {
                        name: "page",
                        value: page as i64,
                    },
                    Arg::Int {
                        name: "pages",
                        value: pages as i64,
                    },
                ],
            )?
        } else {
            String::new()
        };
        let head = Rect::new(region.x, region.y, region.width, header);
        let note_width = font.measure_text(style.small, &note)?;
        let word = text::fit(
            &font,
            style.title,
            &self.word,
            f32::from(width) - 3.0 * margin - note_width,
        )?;
        text::label(
            &mut body,
            &font,
            &word,
            style.title,
            margin,
            head,
            style.ink,
            None,
        )?;
        if !note.is_empty() {
            let x = f32::from(width) - margin - note_width;
            text::label(
                &mut body,
                &font,
                &note,
                style.small,
                x,
                head,
                style.muted,
                None,
            )?;
        }
        let mut y = region
            .y
            .saturating_add(header)
            .saturating_add(style.gap / 2);
        for (row, faint) in panel.page(rows) {
            let area = Rect::new(region.x, y, region.width, line);
            let color = if faint { style.muted } else { style.ink };
            text::label(&mut body, &font, row, style.text, margin, area, color, None)?;
            y = y.saturating_add(line);
        }
        paint::replay(frame, &font, body.iter())?;
        font.release()?;
        self.layout = Layout { region, rows };
        Ok(())
    }
}

/// What the providers answer for `word`, one page each, up to [`MAX_ENTRIES`];
/// providers that found nothing are left out. No providers is no entries.
fn entries(host: &Host<'_>, word: &str, language: &str) -> Result<Vec<Entry>, Error> {
    let mut found = Vec::new();
    if word.is_empty() {
        return Ok(found);
    }
    let lookup = match Lookup::open(host) {
        Ok(lookup) => lookup,
        Err(Error::Unsupported) => return Ok(found),
        Err(error) => return Err(error),
    };
    let result = ask_all(host, &lookup, word, language, &mut found);
    let released = lookup.release(host);
    result.and(released)?;
    Ok(found)
}

fn ask_all(
    host: &Host<'_>,
    lookup: &Lookup,
    word: &str,
    language: &str,
    found: &mut Vec<Entry>,
) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(lookup::BUFFER)?, buffer(lookup::BUFFER)?);
    for cursor in 0..MAX_ENTRIES as u32 {
        let question = Question::new(word)?.language(language)?.cursor(cursor);
        let Some(entry) = lookup.ask(host, &question, &mut input, &mut out)? else {
            break;
        };
        if entry.text().trim().is_empty() {
            continue;
        }
        push(
            found,
            Entry {
                source: text::copy(entry.title().trim()).and_then(|title| {
                    if title.is_empty() {
                        text::copy(entry.provider())
                    } else {
                        Ok(title)
                    }
                })?,
                text: text::copy(entry.text())?,
            },
        )?;
    }
    Ok(())
}
