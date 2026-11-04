//! "Highlight": one of one's own highlights, from the books read lately
//! or from all, another each day (or each time it shows): the text in
//! quotes, the book's title below. A tap opens the book at its place.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::annotations::{self, Kind};
use paperpaw_sdk::builtin::events::{self, Goto};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::library;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetLook, WidgetSpec, WidgetTarget, Wrapped,
};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar, Storage};

use super::{
    Generation, add_settings, below_caption, chosen, draw_line, draw_message, each_book,
    frame_height, mix, open, path_hash, today,
};
use crate::showings::Showings;
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.highlight";
/// Opens a book at a place: the book, then the place in it.
const OPEN_AT: Event<Goto> = Event::new("widgets.open-at");
/// Where highlights come from: the books read lately, or all.
const SOURCES: [&str; 2] = ["widgets.highlight.recent", "widgets.highlight.all"];
/// When another shows: each day, or each time the widget shows.
const CHANGES: [&str; 2] = ["widgets.highlight.daily", "widgets.highlight.shown"];
/// Books read lately looked at.
const RECENT_BOOKS: usize = 12;
/// Books with annotations looked at, at most.
const MAX_BOOKS: usize = 200;
/// The largest file of a book's annotations read: it is read whole.
const MAX_FILE: u64 = 128 * 1024;

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 16] = [
    widgets::align_spec("widgets.highlight.align", ID, Align::Center),
    widgets::frame_spec("widgets.highlight.frame", ID, Frame::None),
    widgets::padding_spec("widgets.highlight.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.highlight.paper", ID),
    widgets::ink_spec("widgets.highlight.ink", ID),
    widgets::corners_spec("widgets.highlight.corners", ID),
    widgets::frame_line_spec("widgets.highlight.frame-line", ID),
    widgets::shadow_spec("widgets.highlight.drop-shadow", ID),
    widgets::size_spec("widgets.highlight.size", ID, TextSize::Fit, true),
    SettingSpec::choice(
        "widgets.highlight.source",
        "widgets.books.setting.highlight-source",
        &SOURCES,
        SOURCES[0],
        ID,
    )
    .description("widgets.highlight.source.help"),
    SettingSpec::choice(
        "widgets.highlight.change",
        "widgets.books.setting.highlight-change",
        &CHANGES,
        CHANGES[0],
        ID,
    )
    .description("widgets.books.setting.highlight-change.help"),
    widgets::part_spec(
        "widgets.highlight.caption",
        ID,
        "widgets.books.part.caption",
        false,
    ),
    widgets::part_spec(
        "widgets.highlight.marks",
        ID,
        "widgets.books.part.marks",
        true,
    ),
    widgets::part_spec(
        "widgets.highlight.title",
        ID,
        "widgets.books.part.title",
        true,
    ),
    widgets::part_spec(
        "widgets.highlight.author",
        ID,
        "widgets.books.part.author",
        false,
    ),
    widgets::part_spec(
        "widgets.highlight.note",
        ID,
        "widgets.books.part.note",
        false,
    ),
];

pub(crate) fn register(r: &mut Registrar, generation: &Generation) -> Result<(), RegisterError> {
    r.on(OPEN_AT, |_, goto, out| {
        out.push(open(&goto.path));
        out.push(events::GOTO.emit(goto.clone()));
    })?;
    add_settings(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.highlight");
            &SPEC
        },
        &SETTINGS,
    )?;
    let kept: Kept = Rc::default();
    let shown = Rc::new(Showings::default());
    let generation = generation.clone();
    r.add_widget(
        WidgetSpec::new(ID, "widgets.highlight", move |env| {
            // Another each time it shows: made as it shows.
            let showing =
                (chosen(env, WIDGET, "change") == CHANGES[1]).then(|| shown.show(env.now()));
            Box::new(HighlightWidget {
                kept: kept.clone(),
                showing,
                generation: generation.clone(),
                quote: None,
                area: None,
                laid: None,
            })
        })
        .category(widgets::categories::WORDS),
    )
}

/// A highlight as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Quote {
    pub path: String,
    pub title: String,
    pub author: String,
    pub text: String,
    pub note: String,
    pub start: Locator,
    pub end: Locator,
}

/// The highlight chosen and what for (the source, the seed, the
/// library's generation).
type Kept = Rc<RefCell<Option<((String, u64, u32), Option<Quote>)>>>;

/// Whether the book at `path` has annotations not too large to read.
fn annotated(storage: &dyn Storage, path: &str) -> bool {
    storage
        .size(&annotations::file(path))
        .is_ok_and(|size| size > 0 && size <= MAX_FILE)
}

/// The books highlights are chosen from: those read lately with
/// annotations, or every book of the library with annotations.
fn books(env: &Env<'_>, all: bool) -> Vec<String> {
    let mut books: Vec<String> = library::opened_books(env.storage())
        .into_iter()
        .take(RECENT_BOOKS)
        .filter(|path| annotated(env.storage(), path))
        .collect();
    if !all {
        return books;
    }
    // The books with annotations: their files are named by the hashes
    // of the books' paths.
    let file = annotations::file("");
    let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut hashes = Vec::new();
    let _ = env.storage().list(dir, &mut |entry| {
        let hash = entry
            .name
            .strip_suffix(".txt")
            .and_then(|h| u64::from_str_radix(h, 16).ok());
        if let Some(hash) = hash
            && entry.size <= MAX_FILE
            && hashes.len() < MAX_BOOKS * 4
        {
            hashes.push(hash);
        }
    });
    hashes.sort_unstable();
    each_book(env, &mut |record| {
        if books.len() < MAX_BOOKS
            && hashes.binary_search(&path_hash(&record.path)).is_ok()
            && !books.contains(&record.path)
        {
            books.push(record.path);
        }
    });
    books
}

/// The highlights of a book with text, as (start, end, text, note).
pub(crate) type Found = (Locator, Locator, String, String);

/// Picks by `seed` one of `books`, and one of its highlights; books
/// without any are passed over. `highlights` reads a book's.
pub(crate) fn pick(
    books: &[String],
    seed: u64,
    highlights: &mut dyn FnMut(&str) -> Vec<Found>,
) -> Option<(String, Found)> {
    let n = books.len();
    if n == 0 {
        return None;
    }
    let start = (mix(seed) % n as u64) as usize;
    for i in 0..n {
        let path = &books[(start + i) % n];
        let mut found = highlights(path);
        if found.is_empty() {
            continue;
        }
        let at = (mix(seed ^ 0x5851_f42d_4c95_7f2d) % found.len() as u64) as usize;
        return Some((path.clone(), found.swap_remove(at)));
    }
    None
}

/// The highlights of the book at `path` with text.
fn highlights_of(storage: &dyn Storage, path: &str) -> Vec<Found> {
    annotations::load(storage, path)
        .into_iter()
        .filter(|a| a.mark.kind == Kind::Highlight && !a.text.trim().is_empty())
        .map(|a| (a.mark.start, a.mark.end, a.text, a.note))
        .collect()
}

struct HighlightWidget {
    kept: Kept,
    /// The showing it was made for, where another shows each time.
    showing: Option<u32>,
    generation: Generation,
    quote: Option<Quote>,
    /// Where it is drawn, once placed, and the highlight laid out there.
    area: Option<Rect>,
    laid: Option<Laid>,
}

impl HighlightWidget {
    fn lay_out(&mut self, env: &Env<'_>) {
        self.laid = match (self.area, &self.quote) {
            (Some(area), Some(quote)) => lay_out(env, area, quote),
            _ => None,
        };
    }

    fn choose(&self, env: &Env<'_>) -> Option<Quote> {
        let id = WIDGET;
        let source = chosen(env, id, "source").to_string();
        // Read anew (after events, a book closed), it shows the same
        // until it shows again.
        let seed = match self.showing {
            Some(showing) if chosen(env, id, "change") == CHANGES[1] => {
                (today(env) as u64).rotate_left(32) ^ u64::from(showing)
            }
            _ => today(env) as u64,
        };
        let key = (source.clone(), seed, self.generation.get());
        if let Some((k, quote)) = self.kept.borrow().as_ref()
            && *k == key
        {
            return quote.clone();
        }
        let books = books(env, source == SOURCES[1]);
        let quote = pick(&books, seed, &mut |path| highlights_of(env.storage(), path)).map(
            |(path, (start, end, text, note))| {
                let book = library::book(env.storage(), &path).unwrap_or_default();
                Quote {
                    title: book.shown_title(&path).into_owned(),
                    author: book.author.clone().unwrap_or_default(),
                    path,
                    text,
                    note,
                    start,
                    end,
                }
            },
        );
        *self.kept.borrow_mut() = Some((key, quote.clone()));
        quote
    }
}

impl Widget for HighlightWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let quote = self.choose(env);
        let changed = quote != self.quote;
        self.quote = quote;
        if changed {
            self.lay_out(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.lay_out(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        (u32::from(UiStyle::new(env).row()) * 3) as u16 + frame_height(env, WIDGET)
    }

    fn min_height(&self, env: &Env<'_>, _: u16) -> u16 {
        (u32::from(UiStyle::new(env).row()) * 2) as u16 + frame_height(env, WIDGET)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WIDGET, area);
        let Some(quote) = &self.quote else {
            draw_message(canvas, env, &look, "widgets.highlight.none");
            return;
        };
        // Laid out when placed; drawn elsewhere, laid out for that.
        let laid = match &self.laid {
            Some(laid) if self.area == Some(area) => Cow::Borrowed(laid),
            _ => match lay_out(env, area, quote) {
                Some(laid) => Cow::Owned(laid),
                None => return,
            },
        };
        draw(canvas, env, &look, quote, &laid);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let Some(quote) = &self.quote else {
            return Vec::new();
        };
        let goto = Goto::to(quote.path.clone(), quote.start).marking(quote.end);
        alloc::vec![WidgetTarget::new(area, OPEN_AT.emit(goto))]
    }
}

/// A highlight laid out in a widget's area, drawn as it is in every band.
#[derive(Debug, Clone, PartialEq)]
struct Laid {
    /// The mark, the text, the note and the book together, in the middle.
    block: Rect,
    /// A large opening mark above the text, in the upper part of its
    /// line: half a line high.
    marks: bool,
    mark_h: u16,
    /// The text, as said, and in its lines at its size.
    text: String,
    lines: Vec<String>,
    size: f32,
    text_h: u16,
    /// The note below the text, if shown.
    note: bool,
    /// The book (and its author) below, if shown.
    source: Option<String>,
}

/// `quote` laid out in widget area `area`, below the caption: where the
/// text does not fit whole, the mark goes first, then the note, then the
/// book; what is left is shortened, broken between words only.
fn lay_out(env: &Env<'_>, area: Rect, quote: &Quote) -> Option<Laid> {
    let style = UiStyle::new(env);
    let id = WIDGET;
    let gap = style.gap();
    let look = widgets::widget_look(env, id, area);
    let area = below_caption(env, id, &look);
    // Below: the book (and the note), small.
    let source = {
        let title = widgets::shows(env, id, "title");
        let author = widgets::shows(env, id, "author") && !quote.author.is_empty();
        match (title, author) {
            (true, true) => format!("{} · {}", quote.title, quote.author),
            (true, false) => quote.title.clone(),
            (false, true) => quote.author.clone(),
            (false, false) => String::new(),
        }
    };
    let small = widgets::line_height(env, style.small());
    let mut note = widgets::shows(env, id, "note") && !quote.note.is_empty();
    let mut with_source = !source.is_empty();
    let mut marks = widgets::shows(env, id, "marks");
    let mark_size = style.title() * 2.0;
    let (max, min) = match widgets::size(env, id) {
        TextSize::Fit => (style.title() * 1.6, style.small()),
        other => (other.text(&style), other.text(&style)),
    };
    let (text, lines, size, mark_h, below) = loop {
        let below = u16::from(with_source) * small + u16::from(note) * small;
        let below = if below > 0 { below + gap / 2 } else { 0 };
        let mark_h = if marks {
            widgets::line_height(env, mark_size) / 2
        } else {
            0
        };
        let text = if marks {
            quote.text.clone()
        } else {
            env.i18n().format(
                "widgets.highlight.quoted",
                &[("text", Arg::Str(&quote.text))],
            )
        };
        let room = area.height.saturating_sub(below + mark_h);
        let wrapped = Wrapped::fitting(env, &text, max, min, area.width, room);
        if wrapped.whole() {
            let lines = wrapped.lines().to_vec();
            break (text, lines, wrapped.size(), mark_h, below);
        }
        if marks {
            marks = false;
        } else if note {
            note = false;
        } else if with_source {
            with_source = false;
        } else {
            // As much as fits, ending in "…" (a word too long shortened,
            // the rest left out).
            let wrapped = Wrapped::new(env, &text, min, area.width, room);
            break (text, wrapped.lines().to_vec(), min, mark_h, below);
        }
    };
    let room = area.height.saturating_sub(below + mark_h);
    let text_h = (lines.len() as u16 * widgets::line_height(env, size)).min(room);
    let total = (mark_h + text_h + below).min(area.height);
    let block = Rect::new(
        area.x,
        area.y + (area.height - total) / 2,
        area.width,
        total,
    );
    Some(Laid {
        block,
        marks,
        mark_h,
        text,
        lines,
        size,
        text_h,
        note,
        source: with_source.then_some(source),
    })
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, quote: &Quote, laid: &Laid) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = WIDGET;
    let align = widgets::align(env, id);
    let gap = style.gap();
    super::caption(
        canvas,
        env,
        id,
        look,
        env.i18n().tr("widgets.highlight.caption"),
    );
    let area = laid.block;
    let small = widgets::line_height(env, style.small());
    if laid.marks {
        let mark = "\u{201c}";
        let mark_size = style.title() * 2.0;
        let w = font.measure(mark_size, mark);
        let x = align.x(area, w);
        let metrics = font.line_metrics(mark_size);
        let baseline = i32::from(area.y) + metrics.ascent as i32 - (laid.mark_h / 4) as i32;
        font.draw(canvas, mark_size, x, baseline, mark, look.faint);
    }
    // The text's lines, told as one text.
    let text_area = Rect::new(area.x, area.y + laid.mark_h, area.width, laid.text_h);
    canvas.describe(Role::Text, &laid.text, text_area);
    let line = widgets::line_height(env, laid.size);
    let metrics = font.line_metrics(laid.size);
    for (n, shown) in laid.lines.iter().enumerate() {
        let y = text_area.y + line * n as u16;
        if y + line > text_area.y + text_area.height {
            break;
        }
        let baseline = f32::from(y) + (f32::from(line) + metrics.ascent - metrics.descent) / 2.0;
        let x = align.x(text_area, font.measure(laid.size, shown));
        font.draw(canvas, laid.size, x, baseline as i32, shown, look.ink);
    }
    let mut y = area.y + laid.mark_h + laid.text_h + gap / 2;
    if laid.note {
        let row = Rect::new(area.x, y, area.width, small);
        y += draw_line(
            canvas,
            env,
            &font,
            style.small(),
            row,
            align,
            &quote.note,
            look.ink,
        );
    }
    if let Some(source) = &laid.source {
        let row = Rect::new(area.x, y, area.width, small);
        draw_line(
            canvas,
            env,
            &font,
            style.small(),
            row,
            align,
            source,
            look.faint,
        );
    }
}
