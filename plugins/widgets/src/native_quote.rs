//! The quote of the day: a quote about reading, books and life, another each
//! day, in the reader's language, from those PaperPaw brings (`data/quotes.
//! <language>.txt`, in the plugin's install folder, or carried in the code
//! where the plugin is part of the app) and the reader's own (`quotes.txt` in
//! the plugin's data folder, a quote per line: `text | author`). A tap shows
//! the next one; set to change each time it shows, the showings are counted in
//! memory ([`made`]).
use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        fonts::{Font, TextOrigin},
        storage::Root,
    },
};
use paperpaw_ui_kit::{
    Paint, files, paint,
    text::{concat, copy, decimal_u64},
    widget::{
        Cx, Settings, TextSize,
        cx::{face_font, local_time_now},
        layout::{break_words, caption, label, line as line_of, wrap},
        text::{fit_line, ink_room, largest_size},
    },
};

/// Where the turns of the quotes are kept (see [`Turns`]).
const TURNS_FILE: &str = "quote.txt";
/// The reader's own quotes, in the data folder.
const OWN_FILE: &str = "quotes.txt";
/// The most bytes of a list of quotes read: PaperPaw's are 7 KiB, a reader's
/// own are cut off from this size on (the file is left out then).
const LIST_LIMIT: usize = 24 * 1024;
/// The setting of the face of the quote.
const FACE_LIMIT: usize = 96;

#[cfg(feature = "embedded-quotes")]
const ENGLISH: &str = include_str!("../data/quotes.en.txt");
#[cfg(feature = "embedded-quotes")]
const GERMAN: &str = include_str!("../data/quotes.de.txt");

/// Showings made within this many seconds of the last count as one.
const APART: u32 = 60;
/// How often a quote was shown since the device started, and when the last
/// showing was counted (in seconds; `u32::MAX`: none yet). A screen makes a
/// widget several times as it shows it (to measure it, to place it, anew
/// after a change); nothing is written for the count.
static SHOWINGS: AtomicU32 = AtomicU32::new(0);
static COUNTED_AT: AtomicU32 = AtomicU32::new(u32::MAX);

/// Counts a showing at `now_ms` (when the host made the widget), unless one was
/// counted less than a minute before.
pub fn made(now_ms: u64) {
    let now = u32::try_from(now_ms / 1000).unwrap_or(u32::MAX - 1);
    let last = COUNTED_AT.load(Ordering::Relaxed);
    if last == u32::MAX || now.saturating_sub(last) >= APART {
        COUNTED_AT.store(now, Ordering::Relaxed);
        SHOWINGS.store(
            SHOWINGS.load(Ordering::Relaxed).wrapping_add(1),
            Ordering::Relaxed,
        );
    }
}

/// Where a quote comes from: PaperPaw's, the reader's own, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Shipped,
    Own,
    Both,
}

impl Source {
    fn of(settings: &Settings<'_, '_>) -> Result<Self, Error> {
        settings.with_text("source", |value| match value.rsplit('.').next() {
            Some("shipped") => Self::Shipped,
            Some("own") => Self::Own,
            _ => Self::Both,
        })
    }
}

/// Whether another quote shows each day (else each time it shows).
fn daily(settings: &Settings<'_, '_>) -> Result<bool, Error> {
    settings.with_text("change", |value| !value.ends_with(".shown"))
}

/// The day number the time reads (0 where it is not known).
fn day_number(host: &Host<'_>) -> Result<i64, Error> {
    Ok(local_time_now(host)?.map_or(0, |t| {
        crate::native_dates::days_from_civil(t.year, t.month, t.day)
    }))
}

/// The one of `count` things shown on `day`: each of them once before any
/// comes again, neighbours far apart (the order of a file says little about
/// the order of the days).
fn of_the_day(day: i64, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    // A prime: with any count it does not divide, a step that reaches every
    // one.
    const STEP: u64 = 7919;
    let count = count as u64;
    let step = if count.is_multiple_of(STEP) { 1 } else { STEP };
    let day = day.rem_euclid(i64::try_from(count).unwrap_or(i64::MAX)) as u64;
    usize::try_from(day * step % count).unwrap_or(0)
}

/// How far the quotes were turned on (to the next quote), kept in a file: the
/// day it was turned on and how often since.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Turns {
    day: i64,
    turns: u32,
}

impl Turns {
    /// As kept (`<day> <turns>`); none if not kept.
    fn parse(bytes: &[u8]) -> Self {
        let Ok(text) = core::str::from_utf8(bytes) else {
            return Self::default();
        };
        let mut numbers = text.split_whitespace();
        let day = numbers.next().and_then(|d| d.parse().ok()).unwrap_or(0);
        let turns = numbers.next().and_then(|t| t.parse().ok()).unwrap_or(0);
        Self { day, turns }
    }

    fn load(host: &Host<'_>) -> Result<Self, Error> {
        match files::read_all(host, Root::Data, TURNS_FILE, 64) {
            Ok(Some(bytes)) => Ok(Self::parse(&bytes)),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // None kept, or none that can be read: the first quote.
            Ok(None) | Err(_) => Ok(Self::default()),
        }
    }

    fn save(self, host: &Host<'_>) -> Result<(), Error> {
        let (mut day, mut turns) = ([0; 20], [0; 20]);
        let day = decimal_u64(u64::try_from(self.day).unwrap_or(0), &mut day);
        let turns = decimal_u64(u64::from(self.turns), &mut turns);
        let line = concat(&[day, " ", turns, "\n"])?;
        files::replace(host, Root::Data, TURNS_FILE, line.as_bytes())
    }

    /// The turns of `today`: none since the day began, where what is shown
    /// changes daily.
    const fn on(self, today: i64, daily: bool) -> u32 {
        if daily && self.day != today {
            0
        } else {
            self.turns
        }
    }
}

/// Turns the quote of the widget `widget` on to the next.
///
/// # Errors
/// The host's refusals.
pub fn turn(host: &Host<'_>, widget: &str) -> Result<(), Error> {
    let settings = Settings::new(host, widget);
    let day = day_number(host)?;
    let turns = Turns::load(host)?.on(day, daily(&settings)?);
    Turns {
        day,
        turns: turns.wrapping_add(1),
    }
    .save(host)
}

/// A quote and who said it (empty if no one is known).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Quote {
    text: String,
    author: String,
}

/// A quote as a line of a file has it, `text | author`; `None` for empty
/// lines and comments (`#`).
fn parse(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (text, author) = line.rsplit_once('|').unwrap_or((line, ""));
    let text = unquoted(text);
    (!text.is_empty()).then_some((text, author.trim()))
}

/// The marks a quote in `language` stands between.
fn marks(language: &str) -> (&'static str, &'static str) {
    let primary = language.split(['-', '_']).next().unwrap_or("");
    let is = |tags: &[&str]| tags.iter().any(|tag| primary.eq_ignore_ascii_case(tag));
    let swiss = language
        .len()
        .checked_sub(2)
        .and_then(|from| language.get(from..))
        .is_some_and(|end| end.eq_ignore_ascii_case("ch"));
    if is(&["de"]) && swiss {
        ("«", "»")
    } else if is(&["de", "cs", "sk", "sl", "hr", "et", "lt", "bg", "is", "ka"]) {
        ("„", "“")
    } else if is(&["pl", "hu", "ro"]) {
        ("„", "”")
    } else if is(&["fr"]) {
        ("«\u{202f}", "\u{202f}»")
    } else if is(&[
        "es", "it", "pt", "ru", "uk", "ca", "el", "nb", "nn", "no", "be",
    ]) {
        ("«", "»")
    } else if is(&["sv", "fi"]) {
        ("”", "”")
    } else if is(&["da"]) {
        ("»", "«")
    } else if is(&["ja", "zh"]) {
        ("「", "」")
    } else {
        ("“", "”")
    }
}

/// `text` between the marks of `language`.
fn quoted(text: &str, language: &str) -> Result<String, Error> {
    let (open, close) = marks(language);
    concat(&[open, unquoted(text), close])
}

/// `text` without quotation marks around it (the file's own, if any).
fn unquoted(text: &str) -> &str {
    const MARKS: &[char] = &['"', '“', '”', '„', '«', '»', '「', '」'];
    let text = text.trim();
    let inner = text
        .strip_prefix(MARKS)
        .and_then(|rest| rest.strip_suffix(MARKS));
    match inner {
        Some(inner) if !inner.contains(MARKS) => inner.trim(),
        _ => text,
    }
}

/// A list of quotes, read from a file or carried in the code.
enum List {
    Read(Vec<u8>),
    Carried(&'static str),
}

impl List {
    fn as_str(&self) -> &str {
        match self {
            Self::Carried(text) => text,
            Self::Read(bytes) => match core::str::from_utf8(bytes) {
                Ok(text) => text,
                // Up to the first byte that is no text.
                Err(error) => core::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or(""),
            },
        }
    }

    /// How many quotes there are.
    fn count(&self) -> usize {
        self.as_str().lines().filter_map(parse).count()
    }

    /// The quote after `skip` others.
    fn nth(&self, skip: usize) -> Result<Option<Quote>, Error> {
        match self.as_str().lines().filter_map(parse).nth(skip) {
            Some((text, author)) => Ok(Some(Quote {
                text: copy(text)?,
                author: copy(author)?,
            })),
            None => Ok(None),
        }
    }
}

/// PaperPaw's quotes in `tongue` (`de` or `en`): the install folder's file,
/// else those the code carries; `None` for neither.
fn shipped(host: &Host<'_>, tongue: &str) -> Result<Option<List>, Error> {
    let name = concat(&["data/quotes.", tongue, ".txt"])?;
    match files::read_all(host, Root::Install, &name, LIST_LIMIT) {
        Ok(Some(bytes)) => return Ok(Some(List::Read(bytes))),
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        // No folder (the plugin is part of the app) or no file.
        Ok(None) | Err(_) => {}
    }
    Ok(carried(tongue).map(List::Carried))
}

#[cfg(feature = "embedded-quotes")]
fn carried(tongue: &str) -> Option<&'static str> {
    Some(if tongue == "de" { GERMAN } else { ENGLISH })
}

/// A plugin file for a reader carries none.
#[cfg(not(feature = "embedded-quotes"))]
fn carried(_tongue: &str) -> Option<&'static str> {
    None
}

/// The reader's own quotes; `None` without a file (or one too large).
fn own(host: &Host<'_>) -> Result<Option<List>, Error> {
    match files::read_all(host, Root::Data, OWN_FILE, LIST_LIMIT) {
        Ok(found) => Ok(found.map(List::Read)),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(None),
    }
}

/// The quote at `position` of the shipped ones and then the reader's own
/// (which of `total` is for `position` to say); `None` without any.
fn quote_at(
    shipped: Option<&List>,
    own: Option<&List>,
    position: impl Fn(usize) -> usize,
) -> Result<Option<Quote>, Error> {
    let from_shipped = shipped.map_or(0, List::count);
    let from_own = own.map_or(0, List::count);
    let total = from_shipped + from_own;
    if total == 0 {
        return Ok(None);
    }
    let at = position(total) % total;
    if at < from_shipped {
        return shipped.map_or(Ok(None), |list| list.nth(at));
    }
    own.map_or(Ok(None), |list| list.nth(at - from_shipped))
}

/// Today's quote, turned on as often as tapped (and, where another shows each
/// time, as often as shown).
fn quote_of_the_day(cx: &mut Cx<'_>, day: i64) -> Result<Option<Quote>, Error> {
    let source = Source::of(&cx.settings)?;
    let daily = daily(&cx.settings)?;
    let showing = if daily {
        0
    } else {
        SHOWINGS.load(Ordering::Relaxed)
    };
    let turns = Turns::load(cx.host)?.on(day, daily).wrapping_add(showing);
    let tag = cx.tr("widgets.quote.language")?;
    let tongue = if tag.starts_with("de") { "de" } else { "en" };
    let shipped = if source == Source::Own {
        None
    } else {
        shipped(cx.host, tongue)?
    };
    let own = if source == Source::Shipped {
        None
    } else {
        own(cx.host)?
    };
    quote_at(shipped.as_ref(), own.as_ref(), |total| {
        let first = if daily { of_the_day(day, total) } else { 0 };
        first.wrapping_add(turns as usize)
    })
}

/// What the settings make drawn change, as a number: the day (where it
/// changes daily), the turns and showings, and what is set.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let settings = Settings::new(host, widget);
    let daily = daily(&settings)?;
    let day = day_number(host)?;
    let showing = if daily {
        0
    } else {
        SHOWINGS.load(Ordering::Relaxed)
    };
    let turns = Turns::load(host)?.on(day, daily).wrapping_add(showing);
    let mut mark = crate::native_draw::fnv(
        1,
        &[
            u8::from(daily),
            Source::of(&settings).map_or(0, |s| s as u8),
            u8::from(settings.shows("author")?),
            u8::from(settings.shows("marks")?),
            u8::from(settings.shows("mark")?),
            u8::from(settings.shows("caption")?),
        ],
    );
    mark = crate::native_draw::fnv(mark, &turns.to_le_bytes());
    if daily {
        mark = crate::native_draw::fnv(mark, &day.to_le_bytes());
    }
    Ok(crate::native_draw::fnv(
        mark,
        settings.string("face", FACE_LIMIT)?.as_bytes(),
    ))
}

/// The height: three lines of the title size, a small line and the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let style = cx.style();
    let (title, small, gap) = (style.title, style.small, style.gap);
    Ok(paperpaw_ui_kit::geometry::line_px(&cx.font, title)?
        .saturating_mul(3)
        .saturating_add(paperpaw_ui_kit::geometry::line_px(&cx.font, small)?)
        .saturating_add(gap)
        .saturating_add(cx.shape.frame_height()))
}

/// How large the opening mark above the quote is, to the quote's size.
const MARK_SCALE: f32 = 2.4;

/// The large mark above a quote in `language`: a raised one, as low opening
/// marks look lost above the text; guillemets where the language has them.
fn ornament(language: &str) -> &'static str {
    match marks(language).0.trim() {
        "«" | "»" => "«",
        _ => "“",
    }
}

/// What goes with the quote's lines.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// A large opening mark above.
    mark: bool,
    /// The height of who said it, below (0 if not shown).
    author: f32,
}

impl Layout {
    /// The room of the large mark above the quote at `size`: the upper part of
    /// a line, where quotation marks are.
    fn mark_height(self, size: f32) -> f32 {
        if self.mark {
            size * MARK_SCALE * 0.42
        } else {
            0.0
        }
    }

    /// The height of it all with `lines` lines at `size`.
    fn height(self, font: &Font<'_>, size: f32, lines: usize) -> Result<f32, Error> {
        Ok(self.mark_height(size) + line_of(font, size)? * lines as f32 + self.author)
    }
}

/// A quote laid out: the layout, the size, its lines and who said it.
struct Plan<'q> {
    layout: Layout,
    size: f32,
    lines: Vec<String>,
    author: Option<&'q str>,
}

/// `shown` laid out in `area`, as large as the size setting says (or as fits);
/// where a line of it would not fit with them, the large mark and who said it
/// go.
fn plan<'q>(
    cx: &Cx<'_>,
    font: &Font<'_>,
    (shown, quote): (&str, &'q Quote),
    area: Rect,
) -> Result<Plan<'q>, Error> {
    let (small, gap) = (cx.style().small, cx.style().gap);
    let width = f32::from(area.width);
    let height = f32::from(area.height);
    // Who said it, where a line of the quote has room above it.
    let least = small * 0.85;
    let author_h =
        f32::from(paperpaw_ui_kit::geometry::line_px(&cx.font, small)?) + f32::from(gap) * 0.75;
    let author = if cx.settings.shows("author")?
        && !quote.author.is_empty()
        && line_of(font, least)? + author_h <= height
    {
        Some(quote.author.as_str())
    } else {
        None
    };
    let mut wished = Layout {
        mark: cx.settings.shows("mark")?,
        author: author.map_or(0.0, |_| author_h),
    };
    if wished.height(font, least, 1)? > height {
        wished.mark = false;
    }
    // Where the quote does not fit whole with it, the large mark goes; where
    // not even so, the quote is shortened at its end.
    let plain = Layout {
        mark: false,
        ..wished
    };
    let choices = [wished, plain];
    let whole = |layout: Layout, size: f32| -> Result<bool, Error> {
        let (lines, too_wide) = break_words(font, size, shown, width)?;
        Ok(!too_wide && layout.height(font, size, lines.len())? <= height)
    };
    let (layout, size) = match cx.settings.size()? {
        TextSize::Fit => {
            let mut found = None;
            for layout in choices {
                if let Some(size) =
                    largest_size(cx.style().title * 2.0, least, |s| whole(layout, s))?
                {
                    found = Some((layout, size));
                    break;
                }
            }
            found.unwrap_or((plain, least))
        }
        size => {
            let size = size.text(cx.style());
            let mut picked = plain;
            for layout in choices {
                if whole(layout, size)? {
                    picked = layout;
                    break;
                }
            }
            (picked, size)
        }
    };
    let room = height - layout.height(font, size, 0)?;
    // As many lines as fit; none where not even one has room.
    let fits = (room / line_of(font, size)?.max(1.0)).max(0.0) as usize;
    let lines = if fits == 0 {
        Vec::new()
    } else {
        wrap(font, size, shown, width, fits)?
    };
    Ok(Plan {
        author: author.filter(|_| layout.author > 0.0),
        layout,
        size,
        lines,
    })
}

/// Draws widget `plan` in `area`, in `font` and `look`.
fn paint_plan(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    (font, plan, tag): (&Font<'_>, &Plan<'_>, &str),
    (area, look): (Rect, &paperpaw_ui_kit::widget::Look),
) -> Result<(), Error> {
    let (small, gap) = (cx.style().small, cx.style().gap);
    let align = cx.settings.align()?;
    let (layout, size) = (plan.layout, plan.size);
    let total = layout.height(font, size, plan.lines.len())?;
    let mut top = f32::from(area.y) + ((f32::from(area.height) - total) / 2.0).max(0.0);
    if layout.mark {
        // The opening mark large and faint above the quote, its top where the
        // block begins.
        let mark = ornament(tag);
        let big = size * MARK_SCALE;
        let x = align.x(area, font.measure_text(big, mark)?);
        // Quotation marks are as high as capitals, a third of them.
        let baseline = top + big * 0.72;
        paint::push(
            out,
            Paint::Text {
                text: copy(mark)?,
                size: big,
                origin: TextOrigin::new(x, baseline as i32),
                color: look.faint,
                clip: Some(ink_room(area)),
            },
        )?;
        top += layout.mark_height(size);
    }
    let row_h = paperpaw_ui_kit::geometry::line_px(font, size)?;
    for held in &plan.lines {
        let row = Rect::new(area.x, top as u16, area.width, row_h);
        label(out, font, (size, align), row, held, look.ink)?;
        top += f32::from(row_h);
    }
    if let Some(author) = plan.author {
        top += f32::from(gap) * 0.75;
        let row_h = paperpaw_ui_kit::geometry::line_px(&cx.font, small)?;
        let row = Rect::new(area.x, top as u16, area.width, row_h);
        // The whole name where it fits, else the last name alone ("Bacon"),
        // smaller where need be, then shortened.
        let width = f32::from(area.width);
        let name = if cx.font.measure_text(small, author)? <= width {
            author
        } else {
            author.rsplit(' ').next().unwrap_or(author)
        };
        let (size, name) = fit_line(&cx.font, small, small * 0.85, name, width)?;
        label(out, &cx.font, (size, align), row, &name, look.faint)?;
    }
    Ok(())
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let day = day_number(cx.host)?;
    let Some(quote) = quote_of_the_day(cx, day)? else {
        let message = cx.tr("widgets.quote.none")?;
        let small = cx.style().small;
        if paperpaw_ui_kit::geometry::line_px(&cx.font, small)? <= look.area.height {
            crate::native_draw::message_in(cx, out, &look, &message)?;
        }
        return Ok(());
    };
    let (small, gap) = (cx.style().small, cx.style().gap);
    let mut area = look.area;
    if cx.settings.shows("caption")? {
        let text = cx.tr("widgets.quote.caption")?;
        let align = cx.settings.align()?;
        area = caption(
            out,
            &cx.font,
            (small, gap),
            (area, align, look.faint),
            &text,
        )?;
    }
    let face = cx.settings.string("face", FACE_LIMIT)?;
    let font = face_font(cx.host, &face)?;
    let tag = cx.tr("widgets.quote.language")?;
    let shown = if cx.settings.shows("marks")? {
        quoted(&quote.text, &tag)?
    } else {
        copy(unquoted(&quote.text))?
    };
    let plan = plan(cx, &font, (&shown, &quote), area)?;
    paint_plan(cx, out, (&font, &plan, &tag), (area, &look))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_a_quote_with_its_author() {
        assert_eq!(parse("A book | Me"), Some(("A book", "Me")));
        assert_eq!(parse("  “Quoted” | You "), Some(("Quoted", "You")));
        assert_eq!(parse("No author"), Some(("No author", "")));
        assert_eq!(parse("# comment"), None);
        assert_eq!(parse("   "), None);
        assert_eq!(parse(" | nobody"), None);
    }

    #[test]
    fn marks_follow_the_language() {
        assert_eq!(marks("en"), ("“", "”"));
        assert_eq!(marks("de"), ("„", "“"));
        assert_eq!(marks("de-CH"), ("«", "»"));
        assert_eq!(marks("FR"), ("«\u{202f}", "\u{202f}»"));
        assert_eq!(marks(""), ("“", "”"));
        assert_eq!(
            marks("a-very-long-language-tag-that-does-not-fit-the-room"),
            ("“", "”")
        );
        assert_eq!(quoted("“Hi”", "de").unwrap(), "„Hi“");
        assert_eq!(ornament("fr"), "«");
        assert_eq!(ornament("en"), "“");
    }

    #[test]
    fn each_day_has_its_own_until_all_were_shown() {
        for count in [1usize, 2, 7, 60, 86, 7919] {
            let mut seen = alloc::vec![false; count];
            for day in 20_000..20_000 + count as i64 {
                let pick = of_the_day(day, count);
                assert!(!seen[pick], "{count}: day {day} repeats {pick}");
                seen[pick] = true;
            }
        }
        assert_eq!(of_the_day(5, 0), 0);
        assert!(of_the_day(-3, 10) < 10);
    }

    #[test]
    fn turns_are_read_and_begin_again_each_day() {
        let turns = Turns::parse(b"20375 3\n");
        assert_eq!(
            turns,
            Turns {
                day: 20_375,
                turns: 3
            }
        );
        assert_eq!(turns.on(20_375, true), 3);
        assert_eq!(turns.on(20_376, true), 0);
        assert_eq!(turns.on(20_376, false), 3);
        assert_eq!(Turns::parse(b"\xff\xfe"), Turns::default());
        assert_eq!(Turns::parse(b""), Turns::default());
    }

    #[test]
    fn the_shipped_quotes_come_first_then_the_readers_own() {
        let shipped = List::Carried("One | A\nTwo | B\n");
        let own = List::Read(b"# mine\nThree | C\n".to_vec());
        let at = |n: usize| {
            quote_at(Some(&shipped), Some(&own), |_| n)
                .unwrap()
                .unwrap()
                .text
        };
        assert_eq!(at(0), "One");
        assert_eq!(at(1), "Two");
        assert_eq!(at(2), "Three");
        // Past the end it starts again.
        assert_eq!(at(3), "One");
        assert!(quote_at(None, None, |_| 0).unwrap().is_none());
        let only = quote_at(None, Some(&own), |_| 7).unwrap().unwrap();
        assert_eq!((only.text.as_str(), only.author.as_str()), ("Three", "C"));
    }

    #[test]
    fn text_that_is_not_utf8_is_read_up_to_it() {
        let list = List::Read(b"Fine | Me\n\xff broken\n".to_vec());
        assert_eq!(list.count(), 1);
    }

    #[test]
    fn a_showing_counts_once_within_a_minute() {
        SHOWINGS.store(0, Ordering::Relaxed);
        COUNTED_AT.store(u32::MAX, Ordering::Relaxed);
        for ms in [0, 5, 400, 59_000] {
            made(ms);
            assert_eq!(SHOWINGS.load(Ordering::Relaxed), 1);
        }
        made(61_000);
        assert_eq!(SHOWINGS.load(Ordering::Relaxed), 2);
        made(3_600_000);
        assert_eq!(SHOWINGS.load(Ordering::Relaxed), 3);
    }
}
