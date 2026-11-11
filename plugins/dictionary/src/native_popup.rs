//! The lookup panel as a native overlay screen filling `core.lookup`: the word
//! and its entries from the providers (`core.lookup` service, one page per
//! provider) over the lower part of the screen, scrolled by button or tap.
use crate::{
    native_disk::{HostRoot, Routed},
    native_folders::{self, Folders},
    native_model::{self, Listing},
    native_panel::{Entry, MAX_ENTRIES, Panel, Step},
    native_source::Dictionaries,
};
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
    /// What the dictionaries and the providers found.
    entries: Vec<Entry>,
    /// The folder, where none holds a dictionary and nothing else answered:
    /// the message says so.
    none: Option<String>,
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
            // A hand-over that cannot be read is a panel for no word, not a failed open:
            // the screen must exist for the draws that follow.
            let _ = read_params(host, ui, handover, &mut view.word, &mut language);
        }
        view.word = clean_word(&view.word)?;
        language = clean_language(language);
        let found = own_entries(host, &view.word, &mut view.entries)?;
        let own = core::mem::take(&mut view.entries);
        view.entries = entries(host, &view.word, &language, own)?;
        if found == 0 && view.entries.is_empty() && !view.word.is_empty() {
            let mut scratch = [0u8; 300];
            view.none = Some(text::copy(
                native_folders::folder_setting(host, &mut scratch)?.trim_matches('/'),
            )?);
        }
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
            Step::Edge => Ok(Handled::Yes),
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

    /// Wraps the entries at the first draw, when the font is there to measure with.
    fn ensure_panel(
        &mut self,
        host: &Host<'_>,
        font: &Font<'_>,
        style: &Style,
        width: u16,
        messages: &mut Messages,
    ) -> Result<(), Error> {
        if self.panel.is_some() {
            return Ok(());
        }
        let empty = match &self.none {
            Some(folder) => messages.format(
                host,
                "dictionary.none",
                &[Arg::Text {
                    name: "folder",
                    value: folder,
                }],
            )?,
            None => messages.format(
                host,
                "dictionary.not-found",
                &[Arg::Text {
                    name: "word",
                    value: &self.word,
                }],
            )?,
        };
        let wide = f32::from(width.saturating_sub(2 * style.margin)).max(1.0);
        let entries = core::mem::take(&mut self.entries);
        self.panel = Some(Panel::new(
            &entries,
            &empty,
            |t| font.measure_text(style.text, t),
            wide,
        )?);
        Ok(())
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
        let font = Font::acquire_role(host, FontRole::Ui)?;
        // The font goes back on every path, an error too.
        let drawn = self.render(host, frame, &font);
        let released = font.release();
        drawn.and(released)
    }

    fn render(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        font: &Font<'_>,
    ) -> Result<(), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let style = Style::read(host, &display, font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let top = height.saturating_mul(9) / 20;
        let region = Rect::new(0, top, width, height.saturating_sub(top));
        let line = paperpaw_ui_kit::geometry::line_px(font, style.text)?.max(1);
        let header = style.row;
        let room = region
            .height
            .saturating_sub(header)
            .saturating_sub(style.gap);
        let rows = usize::from(room / line).max(1);
        let mut messages = Messages::new()?;
        self.ensure_panel(host, font, &style, width, &mut messages)?;
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
            font,
            style.title,
            &self.word,
            f32::from(width) - 3.0 * margin - note_width,
        )?;
        text::label(
            &mut body,
            font,
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
                font,
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
            text::label(&mut body, font, row, style.text, margin, area, color, None)?;
            y = y.saturating_add(line);
        }
        paint::replay(frame, font, body.iter())?;
        self.layout = Layout { region, rows };
        Ok(())
    }
}

/// What the plugin's own dictionaries say of `word`, every definition whole
/// (the entry limit of the lookup service is for what other plugins are given;
/// the panel reads what a lookup reads at most, [`BUDGET`] bytes, together),
/// kept in `found`; how many dictionaries the folder holds, ready or not. A
/// folder that cannot be listed holds none.
///
/// # Errors
/// `NoMemory` and the host's refusal to open the files.
fn own_entries(host: &Host<'_>, word: &str, found: &mut Vec<Entry>) -> Result<usize, Error> {
    if word.is_empty() {
        return Ok(0);
    }
    let mut scratch = [0u8; 300];
    let folder = native_folders::folder_setting(host, &mut scratch)?;
    let (files, data) = (HostRoot::files(host), HostRoot::data(host));
    let storage = Routed {
        files: &files,
        data: &data,
    };
    let mut folders = Folders::open(host)?;
    let outcome = read_own(&mut folders, &storage, folder, word, found);
    let released = folders.finish();
    let count = outcome?;
    released?;
    Ok(count)
}

/// The bytes of definitions the panel reads at most, over all dictionaries.
pub const BUDGET: usize = paperpaw_stardict::LOOKUP_BUDGET;

/// The panel's reading of the own dictionaries over any listing and storage.
///
/// # Errors
/// `NoMemory` only.
pub fn read_own(
    listing: &mut impl Listing,
    storage: &dyn paperpaw_sdk::Storage,
    folder: &str,
    word: &str,
    found: &mut Vec<Entry>,
) -> Result<usize, Error> {
    let mut source = Dictionaries::new(listing, storage, folder)?;
    let count = match source.found() {
        Ok(count) => count,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(0),
    };
    let mut budget = BUDGET;
    for definition in native_model::all_within(&mut source, word, &mut budget)? {
        push(
            found,
            Entry {
                source: definition.title,
                text: definition.text,
            },
        )?;
    }
    Ok(count)
}

/// What the providers answer for `word`, one page each, up to [`MAX_ENTRIES`],
/// after the entries already in `found`; providers that found nothing are left
/// out. No providers is no entries.
fn entries(
    host: &Host<'_>,
    word: &str,
    language: &str,
    mut found: Vec<Entry>,
) -> Result<Vec<Entry>, Error> {
    if word.is_empty() {
        return Ok(found);
    }
    let lookup = match Lookup::open(host) {
        Ok(lookup) => lookup,
        Err(Error::Unsupported) => return Ok(found),
        Err(error) => return Err(error),
    };
    // Whatever a provider answers wrong or fails with is no entry: the panel
    // opens with what was found, and says so when that is nothing.
    let _ = ask_all(host, &lookup, word, language, &mut found);
    let _ = lookup.release(host);
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

/// The word and language of `handover`, as the caller gave them.
fn read_params(
    host: &Host<'_>,
    ui: &Ui,
    handover: HandoverId,
    word: &mut String,
    language: &mut String,
) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    for param in ui.params(host, handover, &mut input, &mut out)? {
        let param = param?;
        match param.key() {
            "word" => *word = text::copy(param.value())?,
            "language" => *language = text::copy(param.value())?,
            _ => {}
        }
    }
    Ok(())
}

/// The word as a lookup takes it: at most [`lookup::MAX_TEXT`] bytes, cut at
/// a character.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn clean_word(word: &str) -> Result<String, Error> {
    let mut end = word.len().min(lookup::MAX_TEXT);
    while !word.is_char_boundary(end) {
        end -= 1;
    }
    text::copy(word.get(..end).unwrap_or(""))
}

/// The language if a lookup takes it, else none.
#[must_use]
pub fn clean_language(language: String) -> String {
    let fine = !language.is_empty()
        && Question::new("x")
            .and_then(|q| q.language(&language))
            .is_ok();
    if fine { language } else { String::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_word_is_cut_at_a_character_and_never_over_the_limit() {
        let long = "é".repeat(1_000);
        let cut = clean_word(&long).unwrap();
        assert!(cut.len() <= lookup::MAX_TEXT);
        assert!(cut.chars().all(|c| c == 'é'));
        assert_eq!(clean_word("cat").unwrap(), "cat");
        assert_eq!(clean_word("").unwrap(), "");
    }

    #[test]
    fn a_bad_or_long_language_is_dropped_and_a_good_one_kept() {
        assert_eq!(clean_language("en-GB".into()), "en-GB");
        assert_eq!(clean_language("de".into()), "de");
        assert_eq!(clean_language("not a tag".into()), "");
        assert_eq!(clean_language("x".into()), "");
        assert_eq!(clean_language("a".repeat(36)), "");
        assert_eq!(clean_language(String::new()), "");
    }
}
