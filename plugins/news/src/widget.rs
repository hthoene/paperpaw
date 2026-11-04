//! *News headlines*: the newest headlines of the feeds fetched last, in
//! the shared look of widgets (its frame, size and alignment), with
//! settings of its own: how many, a line or two each, the feed's name
//! below each, lines between them. A tap on a headline opens its feed's
//! book; without feeds the widget says how to add them, and a tap opens
//! the list of feeds.
//!
//! The headlines are read when the widget is updated (when shown and
//! after events), never as they are drawn.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetLook, WidgetSpec, WidgetTarget, Wrapped,
};
use paperpaw_sdk::{
    Canvas, Color, Command, Env, FontHandle, Id, Rect, RegisterError, Registrar, SettingSpec,
};

use crate::{OPEN, headlines};

pub(crate) const HEADLINES: Id = Id::new_static(NAME);
const NAME: &str = "news.widget.headlines";
/// Headlines shown at most (fewer where they do not fit).
const COUNTS: [&str; 5] = [
    "news.count.2",
    "news.count.3",
    "news.count.4",
    "news.count.6",
    "news.count.8",
];
/// The most of [`COUNTS`].
const MOST: usize = 8;
/// Lines a headline takes at most.
const LINES: [&str; 2] = ["news.lines.1", "news.lines.2"];

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 14] = [
    widgets::frame_spec("news.widget.headlines.frame", HEADLINES, Frame::None),
    widgets::padding_spec("news.widget.headlines.padding", HEADLINES, Padding::Auto),
    widgets::paper_spec("news.widget.headlines.paper", HEADLINES),
    widgets::ink_spec("news.widget.headlines.ink", HEADLINES),
    widgets::corners_spec("news.widget.headlines.corners", HEADLINES),
    widgets::frame_line_spec("news.widget.headlines.frame-line", HEADLINES),
    widgets::shadow_spec("news.widget.headlines.drop-shadow", HEADLINES),
    widgets::size_spec(
        "news.widget.headlines.size",
        HEADLINES,
        TextSize::Small,
        false,
    ),
    widgets::align_spec("news.widget.headlines.align", HEADLINES, Align::Left),
    SettingSpec::choice(
        "news.widget.headlines.count",
        "news.setting.count",
        &COUNTS,
        COUNTS[2],
        HEADLINES,
    )
    .description("news.setting.count.help"),
    SettingSpec::choice(
        "news.widget.headlines.lines",
        "news.setting.lines",
        &LINES,
        LINES[0],
        HEADLINES,
    )
    .description("news.setting.lines.help"),
    widgets::part_spec(
        "news.widget.headlines.caption",
        HEADLINES,
        "news.part.caption",
        true,
    ),
    widgets::part_spec(
        "news.widget.headlines.source",
        HEADLINES,
        "news.part.source",
        false,
    ),
    widgets::part_spec(
        "news.widget.headlines.rules",
        HEADLINES,
        "news.part.rules",
        true,
    ),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(HEADLINES, "news.widget.headlines");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    r.add_widget(
        WidgetSpec::new(HEADLINES, "news.widget.headlines", |_| {
            Box::new(Headlines::default())
        })
        .category(widgets::categories::WORLD),
    )
}

/// How many headlines it shows at most, by its setting.
fn count(env: &Env<'_>) -> usize {
    let value = env.settings().text(&alloc::format!("{NAME}.count"));
    value
        .rsplit('.')
        .next()
        .and_then(|n| n.parse().ok())
        .map_or(4, |n: usize| n.clamp(1, MOST))
}

/// Lines a headline takes at most, by its setting.
fn lines_each(env: &Env<'_>) -> u16 {
    if env.settings().text(&alloc::format!("{NAME}.lines")) == LINES[1] {
        2
    } else {
        1
    }
}

/// The feed a book stands for: its file's name, without the mark of the
/// feed's address (`News/Tagesschau (3fa9c2).html` is "Tagesschau").
fn source(book: &str) -> &str {
    let file = book.rsplit('/').next().unwrap_or(book);
    crate::without_mark(file.rsplit_once('.').map_or(file, |(stem, _)| stem))
}

/// The headlines widget: the last ones fetched, read when updated.
#[derive(Default)]
pub(crate) struct Headlines {
    /// Each one's title and its book.
    shown: Vec<(String, String)>,
    /// Where it is drawn, once placed, and what it says there without
    /// headlines.
    area: Option<Rect>,
    empty: Option<Wrapped>,
}

impl Headlines {
    /// Lays out what it says without headlines, for where it is placed.
    fn lay_out(&mut self, env: &Env<'_>) {
        self.empty = match self.area {
            Some(area) if self.shown.is_empty() => Some(empty(env, area)),
            _ => None,
        };
    }
}

/// What the widget says without headlines in `area`: how to add feeds
/// and fetch them, or where that does not fit whole, the shorter "Add
/// feeds under News in the menu".
fn empty(env: &Env<'_>, area: Rect) -> Wrapped {
    let style = UiStyle::new(env);
    let (width, height) = message_room(env, area);
    // The sentence inside the spacing, the short form where it does not
    // fit whole.
    let inner = widgets::widget_look(env, NAME, area).area.height;
    let long = env.i18n().tr("news.widget.empty");
    let wrapped = Wrapped::fitting(env, long, style.text(), style.small(), width, inner);
    if wrapped.whole() {
        return wrapped;
    }
    let short = env.i18n().tr("news.widget.empty.short");
    Wrapped::fitting(env, short, style.text(), style.small(), width, height)
}

/// The room for a short message in `area` (the widget's place): as wide
/// as its spacing leaves, and as high as half of it, as the lines' own
/// room above and below the letters keeps them clear of the edges (two
/// lines fit a low place then).
fn message_room(env: &Env<'_>, area: Rect) -> (u16, u16) {
    let inner = widgets::widget_look(env, NAME, area).area;
    (inner.width, (inner.height + area.height) / 2)
}

/// Where a message goes in `area`: inside the spacing across, the whole
/// place from top to bottom (its lines in the middle of it).
fn message_area(look: &WidgetLook, area: Rect) -> Rect {
    Rect::new(look.area.x, area.y, look.area.width, area.height)
}

impl Widget for Headlines {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = headlines(env, count(env));
        let changed = shown != self.shown;
        self.shown = shown;
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
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let rows = self.shown.len().max(2) as u16;
        frame_height(env) + caption_room(env) + rows * row_height(env, &font, &style)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        if self.shown.is_empty() {
            // Laid out when placed; drawn elsewhere, laid out for that.
            let laid = match &self.empty {
                Some(laid) if self.area == Some(area) => alloc::borrow::Cow::Borrowed(laid),
                _ => alloc::borrow::Cow::Owned(empty(env, area)),
            };
            let at = message_area(&look, area);
            laid.draw(canvas, env, at, Align::Center, look.ink);
            return;
        }
        let rows = rows(env, &look, self.shown.len());
        let caption = env.i18n().tr("news.title");
        if let Some(top) = rows.0 {
            let at = look.within(Rect::new(
                look.area.x,
                top,
                look.area.width,
                look.area.height,
            ));
            widgets::draw_caption(canvas, env, NAME, &at, caption);
        }
        for (n, row) in rows.1.iter().enumerate() {
            let (title, book) = &self.shown[n];
            draw_row(canvas, env, &look, *row, title, source(book), n > 0);
        }
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let look = widgets::widget_look(env, NAME, area);
        if self.shown.is_empty() {
            return alloc::vec![WidgetTarget::new(area, Command::run(OPEN))];
        }
        rows(env, &look, self.shown.len())
            .1
            .into_iter()
            .zip(&self.shown)
            .map(|(row, (_, book))| {
                WidgetTarget::new(row, paperpaw_sdk::readers::open(book.to_string()))
            })
            .collect()
    }
}

/// The height the widget's frame takes around its content.
fn frame_height(env: &Env<'_>) -> u16 {
    widgets::frame_height(env, NAME)
}

/// The caption's height, if it shows.
fn caption_room(env: &Env<'_>) -> u16 {
    if widgets::shows(env, NAME, "caption") {
        widgets::caption_height(env)
    } else {
        0
    }
}

/// The headlines' text size.
fn text_size(env: &Env<'_>, style: &UiStyle) -> f32 {
    widgets::size(env, NAME).text(style)
}

fn line(font: &FontHandle<'_>, size: f32) -> u16 {
    font.line_metrics(size).line_height as u16
}

/// The height of a headline's row: its lines, its source, and room
/// around a rule between rows.
fn row_height(env: &Env<'_>, font: &FontHandle<'_>, style: &UiStyle) -> u16 {
    let text = line(font, text_size(env, style)) * lines_each(env);
    let source = if widgets::shows(env, NAME, "source") {
        line(font, style.small())
    } else {
        0
    };
    let room = if widgets::shows(env, NAME, "rules") {
        style.gap()
    } else {
        style.gap() / 3
    };
    text + source + room
}

/// Where the caption starts (if it shows) and the rows of as many of
/// `count` headlines as fit in `look`'s area, together in the middle. The
/// caption is left out where it is too wide or leaves no room for a
/// headline; a single headline takes what there is where its row is
/// higher.
fn rows(env: &Env<'_>, look: &WidgetLook, count: usize) -> (Option<u16>, Vec<Rect>) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return (None, Vec::new());
    };
    let area = look.area;
    let row = row_height(env, &font, &style).max(1);
    // The caption where it fits whole, with room for a headline below.
    let caption = caption_room(env);
    let fits = font.measure(style.small(), env.i18n().tr("news.title")) <= f32::from(area.width);
    let caption = if fits && caption + row <= area.height {
        caption
    } else {
        0
    };
    let fit = usize::from(area.height.saturating_sub(caption) / row);
    let n = count.min(fit).max(1);
    let row = row.min(area.height);
    let total = (caption + row * n as u16).min(area.height);
    let top = area.y + (area.height - total) / 2;
    let rows = (0..n)
        .map(|i| Rect::new(area.x, top + caption + row * i as u16, area.width, row))
        .collect();
    ((caption > 0).then_some(top), rows)
}

/// A headline in its row: a rule above it (but for the first) if rules
/// show, its lines, its source below.
fn draw_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    row: Rect,
    title: &str,
    source: &str,
    rule: bool,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let size = text_size(env, &style);
    let small = line(&font, style.small());
    let rules = widgets::shows(env, NAME, "rules");
    let room = if rules { style.gap() } else { style.gap() / 3 };
    let mut ui = ui::Ui::new(canvas, env, &style);
    if rule && rules {
        // A thin rule, dithered light where the panel has few grays.
        let thin = u32::from(style.line()).max(1) as u16;
        let gray = if look.paper.luma() < 128 {
            Color::gray(110)
        } else {
            Color::gray(150)
        };
        ui.canvas()
            .fill_shaded(Rect::new(row.x, row.y, row.width, thin), gray);
    }
    let align = widgets::align(env, NAME);
    let source_h = if widgets::shows(env, NAME, "source") {
        small
    } else {
        0
    };
    let text = Rect::new(
        row.x,
        row.y + room / 2,
        row.width,
        row.height.saturating_sub(room + source_h),
    );
    if lines_each(env) > 1 {
        widgets::draw_wrapped(ui.canvas(), env, text, align, size, title, look.ink);
    } else {
        let shown = ui::fit(&font, size, title, f32::from(text.width));
        let x = align.x(text, font.measure(size, &shown));
        ui::Label::new(&shown)
            .px(size)
            .color(look.ink)
            .draw_from(&mut ui, x, text);
    }
    if source_h > 0 {
        let at = Rect::new(row.x, text.y + text.height, row.width, small);
        let shown = ui::fit(&font, style.small(), source, f32::from(at.width));
        let x = align.x(at, font.measure(style.small(), &shown));
        ui::Label::new(&shown)
            .px(style.small())
            .color(look.faint)
            .draw_from(&mut ui, x, at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings kept in the program are named after the widget, as
    /// it reads them.
    #[test]
    fn settings_are_the_widgets() {
        for spec in &SETTINGS {
            assert!(spec.id.as_str().starts_with("news.widget.headlines."));
            assert_eq!(spec.group, HEADLINES);
        }
    }

    #[test]
    fn sources_are_named_by_their_books() {
        assert_eq!(source("books/News/Tagesschau.html"), "Tagesschau");
        assert_eq!(source("News/The Guardian.html"), "The Guardian");
        assert_eq!(source("News/The Guardian (0a9fc2).html"), "The Guardian");
        assert_eq!(source("plain"), "plain");
    }

    #[test]
    fn counts_are_numbers() {
        let counts: Vec<usize> = COUNTS
            .iter()
            .filter_map(|c| c.rsplit('.').next()?.parse().ok())
            .collect();
        assert_eq!(counts.len(), COUNTS.len());
        assert!(counts.iter().all(|&c| (1..=MOST).contains(&c)));
    }
}
