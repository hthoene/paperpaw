//! The icons of the dashboard: a tool's icon above its name (in the state its
//! plugin shows it: on, faint), the icons of the footer's actions and the
//! pencil.
//!
//! The characters come from the host (`icon.glyph`, looked up when the page is
//! laid out) and are drawn with the bound icon font. Where the host has none
//! (no icon font, a name it does not know) a tool shows its name alone and the
//! pencil its word.
use crate::native_source::HostSource;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        fonts::{Font, IconGlyph, TextOrigin},
        i18n,
        text::{self as words, MAX_BREAKS, MAX_LANGUAGE},
    },
};
use paperpaw_ui_kit::{Paint, Style, geometry, icons, mem, paint, text};

/// Lines a tool's name takes at most.
pub const NAME_LINES: usize = 3;
/// The icon of a footer action whose menu row names none.
const FALLBACK: &str = "puzzle";
/// The icon of the pencil.
const PENCIL: &str = "pencil";

/// How a tool is drawn: its icon (if the host has one) and its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Face {
    pub glyph: Option<IconGlyph>,
    /// Shown as on: light on a dark ground.
    pub checked: bool,
    /// Else faint.
    pub enabled: bool,
}

/// The face of the tool of `action`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn face(host: &Host<'_>, src: &HostSource, action: &str) -> Result<Face, Error> {
    let look = src.tool_look(action);
    let glyph = if look.icon.is_empty() {
        None
    } else {
        icons::glyph(host, &look.icon)?
    };
    Ok(Face {
        glyph,
        checked: look.checked,
        enabled: look.enabled,
    })
}

/// The icon of each of the first `count` footer actions: its own, else the
/// puzzle piece.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn footer(
    host: &Host<'_>,
    src: &HostSource,
    rects: &[Rect],
) -> Result<Vec<(Rect, Option<IconGlyph>)>, Error> {
    let mut found = Vec::new();
    for (at, rect) in rects.iter().enumerate() {
        let name = match src.footer_looks.get(at) {
            Some(look) if !look.icon.is_empty() => look.icon.as_str(),
            _ => FALLBACK,
        };
        mem::push(&mut found, (*rect, icons::glyph(host, name)?))?;
    }
    Ok(found)
}

/// The pencil's icon.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn pencil(host: &Host<'_>) -> Result<Option<IconGlyph>, Error> {
    icons::glyph(host, PENCIL)
}

/// What room a name leaves at the sides of its cell, as interface 6 left.
const SIDE: u16 = 4;
/// The least share of the small size a name shrinks to, to fit one line.
const LEAST: f32 = 0.85;
/// How much smaller each size tried to fit a name on one line is.
const SIZE_STEP: f32 = 0.5;

/// A tool's name as it is drawn: its size and its lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Name {
    pub size: f32,
    pub lines: Vec<String>,
}

/// Where a word may break: fills `out` with the byte offsets into the word
/// before which a hyphen goes, ascending, and says how many (0: leave it
/// whole).
pub type Breaks<'a> = dyn FnMut(&str, &mut [u8; MAX_BREAKS]) -> usize + 'a;

/// The interface language's hyphenation, read once for a screen's names.
#[derive(Debug, Clone, Copy)]
pub struct Hyphens {
    language: [u8; MAX_LANGUAGE],
    len: usize,
}

impl Hyphens {
    /// The interface language (`core.language`); none where it cannot be
    /// read, and then no word breaks.
    #[must_use]
    pub fn of(host: &Host<'_>) -> Self {
        let mut found = Self {
            language: [0; MAX_LANGUAGE],
            len: 0,
        };
        let mut scratch = [0u8; 128];
        if let Ok(tag) = i18n::language(host, &mut scratch)
            && let Some(to) = found.language.get_mut(..tag.len())
        {
            to.copy_from_slice(tag.as_bytes());
            found.len = tag.len();
        }
        found
    }

    /// Where `word` breaks in the interface language ([`Breaks`]): only a
    /// word of letters is asked, and any refusal leaves it whole.
    pub fn breaks(&self, host: &Host<'_>, word: &str, out: &mut [u8; MAX_BREAKS]) -> usize {
        let language = self
            .language
            .get(..self.len)
            .and_then(|tag| core::str::from_utf8(tag).ok())
            .unwrap_or("");
        if language.is_empty() || word.is_empty() || !word.chars().all(char::is_alphabetic) {
            return 0;
        }
        words::hyphenate(host, language, word, 0, 0, out).unwrap_or(0)
    }
}

/// The name `name` as it fits `cell` (see [`fit_name`]); `breaks` says
/// where a word may be hyphenated.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn name_lines(
    font: &Font<'_>,
    style: &Style,
    cell: Rect,
    name: &str,
    breaks: &mut Breaks<'_>,
) -> Result<Name, Error> {
    let room = f32::from(cell.width.saturating_sub(SIDE)).max(1.0);
    fit_name(
        |size, line| font.measure_text(size, line),
        breaks,
        style.small,
        name,
        room,
    )
}

/// `name` in `room` as interface 6 fitted a tool's name: on one line, a
/// little smaller where that is what it takes (down to `LEAST` of
/// `small`); else on up to [`NAME_LINES`] lines broken between words or
/// after a hyphen written in one; else so, and also hyphenated where
/// `breaks` lets a word break (as Beta 3: "Kartei-" / "karten"); else on one
/// line that ends in "…". `measure` gives a text's width at a size.
///
/// # Errors
/// Errors of `measure` and [`Error::NoMemory`].
pub fn fit_name(
    mut measure: impl FnMut(f32, &str) -> Result<f32, Error>,
    breaks: &mut Breaks<'_>,
    small: f32,
    name: &str,
    room: f32,
) -> Result<Name, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(Name {
            size: small,
            lines: Vec::new(),
        });
    }
    let least = small * LEAST;
    let mut size = small;
    while size >= least {
        if measure(size, name)? <= room {
            let mut lines = Vec::new();
            mem::push(&mut lines, text::copy(name)?)?;
            return Ok(Name { size, lines });
        }
        size -= SIZE_STEP;
    }
    for size in [small, least] {
        if let Some(lines) = broken(|line| measure(size, line), None, name, room)? {
            return Ok(Name { size, lines });
        }
    }
    for size in [small, least] {
        let hyphens = Some(&mut *breaks);
        if let Some(lines) = broken(|line| measure(size, line), hyphens, name, room)? {
            return Ok(Name { size, lines });
        }
    }
    let lines = text::wrap_with(|line| measure(least, line), name, room, 1)?;
    Ok(Name { size: least, lines })
}

/// `name` broken between its words, or after a hyphen inside one, and
/// where `breaks` is given also hyphenated inside a word that no line holds,
/// into at most [`NAME_LINES`] lines no wider than `room`; `None` where a
/// part is wider than a line or the lines are too few.
fn broken(
    mut measure: impl FnMut(&str) -> Result<f32, Error>,
    mut breaks: Option<&mut Breaks<'_>>,
    name: &str,
    room: f32,
) -> Result<Option<Vec<String>>, Error> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in name.split_whitespace() {
        let mut first = true;
        for whole in word.split_inclusive('-') {
            let mut glue = if first && !line.is_empty() { " " } else { "" };
            first = false;
            let mut part = whole;
            if let Some(breaks) = breaks.as_deref_mut() {
                let mut out = [0u8; MAX_BREAKS];
                let core = whole.strip_suffix('-').unwrap_or(whole);
                let count = breaks(core, &mut out).min(MAX_BREAKS);
                let places = out.get(..count).unwrap_or(&[]);
                let fits = |text: &str, measure: &mut dyn FnMut(&str) -> Result<f32, Error>| {
                    Ok::<bool, Error>(measure(text)? <= room)
                };
                loop {
                    if fits(&text::concat(&[&line, glue, part])?, &mut measure)?
                        || (!line.is_empty() && fits(part, &mut measure)?)
                    {
                        break;
                    }
                    let base = whole.len() - part.len();
                    let mut best = None;
                    for &at in places.iter().rev() {
                        let Some(head) = whole.get(base..usize::from(at)) else {
                            continue;
                        };
                        if head.is_empty() {
                            break;
                        }
                        let hyphened = text::concat(&[&line, glue, head, "-"])?;
                        if fits(&hyphened, &mut measure)? {
                            best = Some((hyphened, usize::from(at)));
                            break;
                        }
                    }
                    match best {
                        Some((hyphened, at)) => {
                            mem::push(&mut lines, hyphened)?;
                            line = String::new();
                            part = whole.get(at..).unwrap_or("");
                        }
                        None if !line.is_empty() => {
                            mem::push(&mut lines, core::mem::take(&mut line))?;
                        }
                        None => break,
                    }
                    glue = "";
                }
            }
            let joined = text::concat(&[&line, glue, part])?;
            if measure(&joined)? <= room {
                line = joined;
                continue;
            }
            if measure(part)? > room {
                return Ok(None);
            }
            if !line.is_empty() {
                mem::push(&mut lines, core::mem::take(&mut line))?;
            }
            line = text::copy(part)?;
        }
    }
    if !line.is_empty() {
        mem::push(&mut lines, line)?;
    }
    Ok((lines.len() <= NAME_LINES).then_some(lines))
}

/// The most lines any name takes among the tools of the same row (the cells
/// at the same top), for each of `cells` (the top of each cell and its name's
/// lines): names of tools side by side keep to one line. One pass over the
/// cells and one over the distinct rows, which are few.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn row_lines(cells: &[(u16, usize)]) -> Result<Vec<usize>, Error> {
    let mut rows: Vec<(u16, usize)> = Vec::new();
    for (top, lines) in cells {
        if let Some(row) = rows.iter_mut().find(|(at, _)| at == top) {
            row.1 = row.1.max(*lines);
        } else {
            mem::push(&mut rows, (*top, *lines))?;
        }
    }
    let mut out = Vec::new();
    out.try_reserve_exact(cells.len())
        .map_err(|_| Error::NoMemory)?;
    for (top, _) in cells {
        let most = rows
            .iter()
            .find(|(at, _)| at == top)
            .map_or(0, |(_, lines)| *lines);
        out.push(most);
    }
    Ok(out)
}

/// The two fonts a tool is drawn with.
#[derive(Debug, Clone, Copy)]
pub struct Fonts<'a, 'f> {
    /// The interface font, for the name.
    pub ui: &'a Font<'f>,
    /// The icon font, where one is bound.
    pub icons: Option<&'a Font<'f>>,
    /// The circle's character, which frames every icon alike.
    pub circle: Option<&'a IconGlyph>,
}

/// What a tool shows: its name, its lines, its state.
#[derive(Debug)]
pub struct Tool<'a> {
    pub lines: &'a [String],
    /// The size its name is drawn at.
    pub size: f32,
    pub face: Face,
    /// The most lines a name takes among the tools beside it.
    pub row_lines: usize,
}

/// Draws a tool in `cell`: its icon above its name, centred as a group; the
/// cell filled dark where it is on, the ink faint where it is not enabled.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw_tool(
    out: &mut Vec<Paint>,
    fonts: &Fonts<'_, '_>,
    style: &Style,
    cell: Rect,
    tool: &Tool<'_>,
) -> Result<(), Error> {
    let face = tool.face;
    let faint = style.note;
    let ink = match (face.checked, face.enabled) {
        (true, _) => style.paper,
        (false, true) => style.ink,
        (false, false) => faint,
    };
    if face.checked {
        let back = if face.enabled { style.ink } else { faint };
        paint::push(out, Paint::Focus(cell, style.corner, None, back))?;
    }
    let font = fonts.ui;
    let named = !tool.lines.is_empty();
    let inner = i32::from(style.gap / 2);
    // The name's capitals above its baseline, its descenders below (by the ink
    // of letters that reach as far, so that names side by side line up).
    let (cap, descent) = if named {
        (
            match font.ink(style.small, "H")? {
                Some(ink) => -ink.top,
                None => (style.small * 0.7) as i32,
            },
            match font.ink(style.small, "g")? {
                Some(ink) => ink.bottom,
                None => (style.small * 0.25) as i32,
            },
        )
    } else {
        (0, 0)
    };
    let step = i32::from(geometry::line_px(font, style.small)?);
    let more = i32::try_from(tool.row_lines.max(tool.lines.len()).saturating_sub(1))
        .unwrap_or(0)
        .saturating_mul(step);
    let text_room = if named {
        inner
            .saturating_add(cap)
            .saturating_add(descent)
            .saturating_add(more)
    } else {
        0
    };
    let middle = f32::from(cell.x) + f32::from(cell.width) / 2.0;
    let drawn = match (fonts.icons, face.glyph.as_ref()) {
        (Some(icon_font), Some(glyph)) => {
            let room = (i32::from(cell.height) - text_room).max(1) as f32;
            let frame = icons::frame(icon_font, (fonts.circle, glyph), style.title, room)?;
            let group = frame.height().saturating_add(text_room);
            let y = i32::from(cell.y) + (i32::from(cell.height) - group).div_euclid(2);
            icons::draw(out, icon_font, glyph, &frame, (middle, y), ink)?;
            Some((y, frame.height()))
        }
        _ => None,
    };
    if !named {
        return Ok(());
    }
    let Some((y, height)) = drawn else {
        // The name alone, in the middle of the cell.
        let line = geometry::line_px(font, style.small)?;
        let total = u32::from(line) * u32::try_from(tool.lines.len()).unwrap_or(0);
        let mut top = u32::from(cell.y) + u32::from(cell.height).saturating_sub(total) / 2;
        for shown in tool.lines {
            let wide = font.measure_text(tool.size, shown)?;
            let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
            let row = Rect::new(
                cell.x,
                geometry::clamp_u16(i64::from(top)),
                cell.width,
                line,
            );
            text::label(out, font, shown, tool.size, x, row, ink, None)?;
            top = top.saturating_add(u32::from(line));
        }
        return Ok(());
    };
    // The name below the icon.
    let baseline = y
        .saturating_add(height)
        .saturating_add(inner)
        .saturating_add(cap);
    for (n, shown) in tool.lines.iter().enumerate() {
        let wide = font.measure_text(tool.size, shown)?;
        let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
        let at = i32::try_from(n).unwrap_or(0).saturating_mul(step);
        paint::push(
            out,
            Paint::Text {
                text: text::copy(shown)?,
                size: tool.size,
                origin: TextOrigin::new(x, baseline.saturating_add(at)),
                color: ink,
                clip: None,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every letter half as wide as the size: `name` fitted in 80 pixels at 17.
    /// Widths of 0.5 of the size a character: at 17, nine characters fit in
    /// 80; at 0.85 of it, eleven.
    fn fit(name: &str, breaks: &mut Breaks<'_>) -> Name {
        let measure = |size: f32, line: &str| Ok(line.chars().count() as f32 * size * 0.5);
        fit_name(measure, breaks, 17.0, name, 80.0).unwrap()
    }

    fn fitted(name: &str) -> Name {
        fit(name, &mut |_, _| 0)
    }

    /// A German table for the few words of the tests.
    fn german(word: &str, out: &mut [u8; MAX_BREAKS]) -> usize {
        let places: &[u8] = match word {
            "Einstellungen" => &[3, 7],
            "Karteikarten" => &[6, 9],
            "Übertragung" => &[5, 8],
            _ => &[],
        };
        out[..places.len()].copy_from_slice(places);
        places.len()
    }

    #[test]
    fn a_name_a_little_too_wide_shrinks_onto_one_line() {
        // #257 (x4-drawer-de): "Bibliothe" over "k".
        let name = fitted("Bibliothek");
        assert_eq!(name.lines, ["Bibliothek"]);
        assert!(
            name.size < 17.0 && name.size >= 17.0 * LEAST,
            "{}",
            name.size
        );
        assert_eq!(fitted("News").size, 17.0);
    }

    #[test]
    fn a_name_breaks_between_words_or_after_its_hyphen_never_inside_a_word() {
        assert_eq!(fitted("Notizen-Tresor").lines, ["Notizen-", "Tresor"]);
        assert_eq!(fitted("Plugin-Store").lines, ["Plugin-", "Store"]);
        assert_eq!(
            fitted("Wi-Fi transfer files").lines,
            ["Wi-Fi", "transfer", "files"]
        );
        // A word no line holds is cut short on one line, marked as cut.
        let long = fitted("Einstellungen");
        assert_eq!(long.lines.len(), 1, "{:?}", long.lines);
        assert!(long.lines[0].ends_with('…'), "{:?}", long.lines);
        assert!(fitted("").lines.is_empty());
    }

    #[test]
    fn a_word_no_line_holds_is_hyphenated_where_the_language_lets_it() {
        // #257 (x4-drawer-de): Beta 3 said "Einstel-" / "lungen", not "Einstellu…".
        let name = fit("Einstellungen", &mut german);
        assert_eq!(name.lines, ["Einstel-", "lungen"]);
        assert_eq!(name.size, 17.0);
        assert_eq!(
            fit("Karteikarten", &mut german).lines,
            ["Kartei-", "karten"]
        );
        // A word that fits a line a little smaller is kept whole.
        assert_eq!(
            fit("WLAN-Übertragung", &mut german).lines,
            ["WLAN-", "Übertragung"]
        );
        // A word that fits whole, shrunk, is not hyphenated; nor one the
        // language leaves whole.
        assert_eq!(fit("Bibliothek", &mut german).lines, ["Bibliothek"]);
        let whole = fit("Vokabeltrainer", &mut german);
        assert_eq!(whole.lines.len(), 1);
        assert!(whole.lines[0].ends_with('…'), "{:?}", whole.lines);
    }

    #[test]
    fn a_hyphenated_name_still_keeps_to_its_lines() {
        // Every two characters may break: still never more than the lines.
        let mut everywhere = |word: &str, out: &mut [u8; MAX_BREAKS]| {
            let mut count = 0;
            for (at, _) in word.char_indices().skip(2).step_by(2) {
                out[count] = at as u8;
                count += 1;
            }
            count
        };
        let name = fit(
            "Donaudampfschifffahrtsgesellschaftskapitän",
            &mut everywhere,
        );
        assert!(name.lines.len() <= NAME_LINES, "{:?}", name.lines);
        for line in &name.lines {
            assert!(
                line.chars().count() as f32 * name.size * 0.5 <= 80.0,
                "{line}"
            );
        }
    }

    #[test]
    fn tools_side_by_side_keep_to_the_most_lines_of_their_row() {
        // Two rows: tops 10 and 90.
        let cells = [(10, 1), (10, 3), (90, 2), (10, 2), (90, 1)];
        assert_eq!(row_lines(&cells).unwrap(), [3, 3, 2, 3, 2]);
        assert!(row_lines(&[]).unwrap().is_empty());
    }
}
