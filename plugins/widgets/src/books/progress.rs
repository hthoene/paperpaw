//! "Book progress": the book being read, its cover large and beside it a
//! ring round the share read, the percentage inside; or the ring round a
//! small cover, the ring alone, or a bar below the title.

use alloc::boxed::Box;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetLook, WidgetTarget,
};
use paperpaw_sdk::{Canvas, Command, Env, Id, Point, Rect, RegisterError, Registrar};

use super::{
    Book, Covers, FitMemo, Lines, Role, THICKNESSES, add_settings, below_caption, caption, chosen,
    cover_size, current, draw_bar, draw_cover, draw_message, draw_percent_alone, draw_ring,
    draw_ring_percent, frame_height, percent_text, place_x, text_size, thickness,
};
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.progress";
/// How it is drawn: the cover with the ring beside it, the ring round a
/// small cover, the ring alone, the cover with a bar.
const STYLES: [&str; 4] = [
    "widgets.progress.beside",
    "widgets.progress.round",
    "widgets.progress.ring",
    "widgets.progress.bar",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 15] = [
    widgets::align_spec("widgets.progress.align", ID, Align::Center),
    widgets::frame_spec("widgets.progress.frame", ID, Frame::None),
    widgets::padding_spec("widgets.progress.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.progress.paper", ID),
    widgets::ink_spec("widgets.progress.ink", ID),
    widgets::corners_spec("widgets.progress.corners", ID),
    widgets::frame_line_spec("widgets.progress.frame-line", ID),
    widgets::shadow_spec("widgets.progress.drop-shadow", ID),
    widgets::size_spec("widgets.progress.size", ID, TextSize::Medium, false),
    SettingSpec::choice(
        "widgets.progress.style",
        "widgets.books.setting.style",
        &STYLES,
        STYLES[0],
        ID,
    )
    .description("widgets.progress.style.help"),
    SettingSpec::choice(
        "widgets.progress.thickness",
        "widgets.books.setting.thickness",
        &THICKNESSES,
        THICKNESSES[1],
        ID,
    )
    .description("widgets.progress.thickness.help"),
    widgets::part_spec(
        "widgets.progress.cover",
        ID,
        "widgets.books.part.cover",
        true,
    ),
    widgets::part_spec(
        "widgets.progress.title",
        ID,
        "widgets.books.part.title",
        true,
    ),
    widgets::part_spec(
        "widgets.progress.author",
        ID,
        "widgets.books.part.author",
        true,
    ),
    widgets::part_spec(
        "widgets.progress.caption",
        ID,
        "widgets.books.part.caption",
        false,
    ),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    add_settings(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.progress");
            &SPEC
        },
        &SETTINGS,
    )?;
    static WIDGET: [FixedWidget; 1] =
        [
            FixedWidget::new(ID, "widgets.progress", |_| Box::new(Progress::default()))
                .category(widgets::categories::BOOKS),
        ];
    r.add_widgets(&WIDGET)
}

/// The book being read, read when updated.
#[derive(Debug, Default)]
struct Progress {
    book: Option<Book>,
    /// Where it is drawn, once placed.
    area: Option<Rect>,
    covers: Covers,
    /// Its text made to fit, kept for the frames that follow.
    memo: FitMemo,
}

impl Progress {
    /// The cover shown, made small for where it goes.
    fn make_covers(&mut self, env: &Env<'_>) {
        self.covers = match (self.area, &self.book) {
            (Some(area), Some(book)) => {
                let look = widgets::widget_look(env, WIDGET, area);
                let area = below_caption(env, WIDGET, &look);
                let at = cover_at(env, &look, area, book, &self.memo);
                Covers::make(env, at.map(|at| (book, at)))
            }
            _ => Covers::default(),
        };
    }
}

impl Widget for Progress {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let book = current(env.storage());
        let changed = book != self.book;
        self.book = book;
        if changed {
            self.make_covers(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_covers(env);
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
        draw(
            canvas,
            env,
            area,
            self.book.as_ref(),
            &self.covers,
            &self.memo,
        );
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = self
            .book
            .as_ref()
            .map_or(Command::run(actions::LIBRARY), Book::open);
        alloc::vec![WidgetTarget::new(area, command)]
    }
}

/// Too small for a ring, in `area`: the percentage alone shows.
fn percent_alone(env: &Env<'_>, area: Rect) -> bool {
    let style = UiStyle::new(env);
    chosen(env, WIDGET, "style") != STYLES[3]
        && f32::from(area.width.min(area.height)) < style.small() * 3.0
}

/// With the cover beside the ring (the first style), the room for the
/// ring in `area`: none where the cover is not shown or leaves too little.
fn beside_rest(env: &Env<'_>, area: Rect, book: &Book) -> Option<Rect> {
    let gap = UiStyle::new(env).gap();
    let (cw, _) = cover_size(book, area.width * 2 / 5, area.height);
    let rest_w = area.width.saturating_sub(cw + gap);
    (widgets::shows(env, WIDGET, "cover") && rest_w >= area.height * 6 / 10)
        .then(|| Rect::new(area.x + cw + gap, area.y, rest_w, area.height))
}

/// Where the cover goes in `area` (`look`'s, below the caption), if it
/// shows, as each style places it.
fn cover_at<'b>(
    env: &Env<'b>,
    look: &WidgetLook,
    area: Rect,
    book: &'b Book,
    memo: &'b FitMemo,
) -> Option<Rect> {
    if percent_alone(env, area) {
        return None;
    }
    let style = UiStyle::new(env);
    let gap = style.gap();
    match chosen(env, WIDGET, "style") {
        "widgets.progress.round" => {
            let mut lines = round_lines(env, look, book, memo);
            let ring = ring_layout(env, area, &mut lines).ring;
            let inner = ring_inner(env, ring) as u16;
            // Inside the ring, as large as fits the circle.
            (inner >= 8).then(|| {
                let (w, h) = cover_size(book, inner * 2 * 56 / 100, inner * 2 * 83 / 100);
                Rect::new(
                    ring.x + (ring.width - w) / 2,
                    ring.y + (ring.height - h) / 2,
                    w,
                    h,
                )
            })
        }
        "widgets.progress.bar" => {
            // On the left, where the text beside it has room.
            let (w, h) = cover_size(book, area.width * 45 / 100, area.height);
            let room = f32::from(area.width.saturating_sub(w + gap));
            (widgets::shows(env, WIDGET, "cover") && room >= style.small() * 7.0)
                .then(|| Rect::new(area.x, area.y + (area.height - h) / 2, w, h))
        }
        "widgets.progress.ring" => None,
        _ => {
            let rest = beside_rest(env, area, book)?;
            let (cw, ch) = cover_size(book, area.width * 2 / 5, area.height);
            let layout = ring_layout(env, rest, &mut texts(env, look, book, memo));
            // Without text, the cover and the ring together, as aligned;
            // else the cover on the left.
            let x = if layout.text.is_none() {
                let group = cw + gap + layout.ring.width;
                place_x(area, group, widgets::align(env, WIDGET))
            } else {
                area.x
            };
            Some(Rect::new(x, area.y + (area.height - ch) / 2, cw, ch))
        }
    }
}

fn draw<'b>(
    canvas: &mut Canvas<'_>,
    env: &Env<'b>,
    area: Rect,
    book: Option<&'b Book>,
    covers: &Covers,
    memo: &'b FitMemo,
) {
    let look = widgets::draw_frame(canvas, env, WIDGET, area);
    let Some(book) = book else {
        draw_message(canvas, env, &look, "widgets.books.nothing-read");
        return;
    };
    let caption_text = env.i18n().tr("widgets.progress.caption");
    let area = caption(canvas, env, WIDGET, &look, caption_text);
    let gap = UiStyle::new(env).gap();
    let mut lines = texts(env, &look, book, memo);
    // Too small for a ring: the percentage alone.
    if percent_alone(env, area) {
        let align = widgets::align(env, WIDGET);
        if area.width >= area.height * 4 && !lines.is_empty() {
            // Wide but low: the percentage, the title beside it.
            draw_percent_beside(canvas, env, &look, area, book, &mut lines);
        } else {
            draw_percent_alone(canvas, env, &look, area, align, book.percent());
        }
        return;
    }
    let cover = cover_at(env, &look, area, book, memo);
    if let Some(at) = cover {
        draw_cover(canvas, env, &look, at, book, covers);
    }
    match chosen(env, WIDGET, "style") {
        "widgets.progress.round" => draw_round(canvas, env, &look, area, book, memo),
        "widgets.progress.bar" => {
            // The text right of the cover.
            let rest = match cover {
                Some(at) => {
                    let shift = (at.width + gap).min(area.width);
                    Rect::new(area.x + shift, area.y, area.width - shift, area.height)
                }
                None => area,
            };
            draw_bar_and_text(canvas, env, &look, rest, book, memo);
        }
        "widgets.progress.ring" => {
            let layout = ring_layout(env, area, &mut lines);
            draw_ring_layout(canvas, env, &look, &layout, &mut lines, book, 0);
        }
        _ => {
            // The ring beside the cover where it has room, else alone.
            let (Some(rest), Some(at)) = (beside_rest(env, area, book), cover) else {
                let layout = ring_layout(env, area, &mut lines);
                draw_ring_layout(canvas, env, &look, &layout, &mut lines, book, 0);
                return;
            };
            let layout = ring_layout(env, rest, &mut lines);
            // Without text, moved to stand beside the cover.
            let shift = if layout.text.is_none() {
                i32::from(layout.ring.x) - i32::from(at.x + at.width + gap)
            } else {
                0
            };
            draw_ring_layout(canvas, env, &look, &layout, &mut lines, book, shift);
        }
    }
}

/// The percentage as large as the height allows, and the title in the
/// rest of the width beside it (where it fits), the two together placed
/// as the alignment says.
fn draw_percent_beside(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    area: Rect,
    book: &Book,
    lines: &mut Lines<'_>,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let gap = style.gap();
    let text = percent_text(env, book.percent());
    let size = widgets::largest_size(style.title() * 2.0, style.small(), |s| {
        widgets::line_height(env, s) <= area.height
    })
    .unwrap_or(style.small());
    let percent_w = (font.measure(size, &text) + 1.0) as u16;
    let rest = area.width.saturating_sub(percent_w + gap);
    lines.fit(env, rest, area.height);
    let text_w = lines
        .lines
        .iter()
        .map(|l| font.measure(l.size, &l.text) as u16)
        .max()
        .unwrap_or(0);
    let total = if lines.is_empty() {
        percent_w
    } else {
        percent_w + gap + text_w
    };
    let x = place_x(area, total.min(area.width), widgets::align(env, WIDGET));
    let at = Rect::new(x, area.y, percent_w, area.height);
    widgets::draw_line(canvas, env, at, Align::Left, size, size, &text, look.ink);
    if !lines.is_empty() {
        let beside = Rect::new(x + percent_w + gap, area.y, text_w, area.height);
        lines.draw(canvas, env, beside, Align::Left);
    }
}

/// The title and the author, as far as shown.
fn texts<'b>(env: &Env<'_>, look: &WidgetLook, book: &'b Book, memo: &'b FitMemo) -> Lines<'b> {
    let style = UiStyle::new(env);
    let mut lines = Lines::remembering(memo);
    if widgets::shows(env, WIDGET, "title") {
        lines.add(
            Role::Title,
            text_size(env, WIDGET),
            book.title.as_str(),
            look.ink,
        );
    }
    if widgets::shows(env, WIDGET, "author") {
        lines.add(Role::Note, style.small(), book.author.as_str(), look.faint);
    }
    lines
}

/// Where a ring and its text go.
struct RingLayout {
    ring: Rect,
    /// Where the text goes, if it has room; beside the ring (`true`) or
    /// below it.
    text: Option<(Rect, bool)>,
}

/// The ring in `area`: with the title and the author beside it where
/// `area` is wide, else below it; the text left out where it has no room.
fn ring_layout(env: &Env<'_>, area: Rect, lines: &mut Lines<'_>) -> RingLayout {
    let style = UiStyle::new(env);
    let gap = style.gap();
    let align = widgets::align(env, WIDGET);
    let wide = area.width >= area.height * 3 / 2;
    if wide {
        // Beside the ring, room for a title of some length.
        let d = area.height.min(area.width * 2 / 5);
        let room = area.width.saturating_sub(d + gap * 3 / 2);
        if f32::from(room) >= style.small() * 6.0 && !lines.is_empty() {
            let ring = Rect::new(area.x, area.y + (area.height - d) / 2, d, d);
            let text = Rect::new(area.x + area.width - room, area.y, room, area.height);
            return RingLayout {
                ring,
                text: Some((text, true)),
            };
        }
    }
    // Below the ring where it is as wide as the place (the text keeps the
    // rest of the height) or half as high: the text in half the height,
    // else in one line.
    for budget in [area.height / 2, 0] {
        let budget = if budget > 0 {
            budget
        } else {
            let first = lines.lines.iter().find(|l| l.role == Role::Title);
            match first.or(lines.lines.first()) {
                Some(line) => widgets::line_height(env, line.size.min(style.text())),
                None => break,
            }
        };
        lines.fit(env, area.width, budget);
        let text_h = lines.height(env);
        let below = text_h + gap / 2;
        let d = area.width.min(area.height.saturating_sub(below));
        if (d >= area.height / 2 || d >= area.width) && d > 0 && !lines.is_empty() {
            let top = area.y + (area.height - (d + below).min(area.height)) / 2;
            let ring = Rect::new(place_x(area, d, align), top, d, d);
            let text = Rect::new(area.x, top + d + gap / 2, area.width, text_h);
            return RingLayout {
                ring,
                text: Some((text, false)),
            };
        }
    }
    let d = area.width.min(area.height);
    let ring = Rect::new(
        place_x(area, d, align),
        area.y + (area.height - d) / 2,
        d,
        d,
    );
    RingLayout { ring, text: None }
}

/// Draws the ring of `layout`, moved `shift` pixels to the left, with the
/// percentage inside, and the text where it goes.
fn draw_ring_layout(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    layout: &RingLayout,
    lines: &mut Lines<'_>,
    book: &Book,
    shift: i32,
) {
    let mut ring = layout.ring;
    ring.x = (i32::from(ring.x) - shift).max(0) as u16;
    draw_ring_in(canvas, env, look, ring, book.percent(), true);
    let Some((text, beside)) = layout.text else {
        return;
    };
    let align = widgets::align(env, WIDGET);
    // Beside a ring, the text starts at its side.
    let align = if beside && align == Align::Center {
        Align::Left
    } else {
        align
    };
    lines.draw(canvas, env, text, align);
}
// A ring in the square `at`, with the percentage inside if `percent`
/// and there is room; returns the radius inside it.
fn draw_ring_in(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    at: Rect,
    progress: u8,
    percent: bool,
) -> u32 {
    let radius = u32::from(at.width.min(at.height) / 2);
    if radius < 4 {
        return 0;
    }
    let center = Point::new(
        i32::from(at.x) + radius as i32,
        i32::from(at.y) + radius as i32,
    );
    let thick = thickness(env, WIDGET, at.width.min(at.height));
    draw_ring(canvas, env, look, center, radius, thick, progress);
    let inner = ring_inner(env, at);
    if percent && inner >= 8 {
        draw_ring_percent(canvas, env, look, center, inner, progress);
    }
    inner
}

/// The radius inside a ring in the square `at`, 0 where there is none.
fn ring_inner(env: &Env<'_>, at: Rect) -> u32 {
    let radius = u32::from(at.width.min(at.height) / 2);
    if radius < 4 {
        return 0;
    }
    let thick = thickness(env, WIDGET, at.width.min(at.height));
    radius.saturating_sub(thick + (radius / 8).max(1))
}

/// The lines of the ring round a small cover: the percentage first
/// (where there is room for one line, that one), the title, the author.
fn round_lines<'b>(
    env: &Env<'b>,
    look: &WidgetLook,
    book: &'b Book,
    memo: &'b FitMemo,
) -> Lines<'b> {
    let style = UiStyle::new(env);
    let mut lines = Lines::remembering(memo);
    lines.add(
        Role::Main,
        style.small(),
        percent_text(env, book.percent()),
        look.faint,
    );
    lines.lines.extend(texts(env, look, book, memo).lines);
    lines
}

/// The ring round a small cover (drawn inside it before), the title,
/// the author and the percentage below it.
fn draw_round<'b>(
    canvas: &mut Canvas<'_>,
    env: &Env<'b>,
    look: &WidgetLook,
    area: Rect,
    book: &'b Book,
    memo: &'b FitMemo,
) {
    let mut lines = round_lines(env, look, book, memo);
    let layout = ring_layout(env, area, &mut lines);
    draw_ring_in(canvas, env, look, layout.ring, book.percent(), false);
    if let Some((text, beside)) = layout.text {
        let align = match widgets::align(env, WIDGET) {
            Align::Center if beside => Align::Left,
            align => align,
        };
        lines.draw(canvas, env, text, align);
    }
}

// The title and the author, and below them the percentage with a bar.
fn draw_bar_and_text<'b>(
    canvas: &mut Canvas<'_>,
    env: &Env<'b>,
    look: &WidgetLook,
    area: Rect,
    book: &'b Book,
    memo: &'b FitMemo,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let mut ui = paperpaw_sdk::ui::Ui::new(canvas, env, &style);
    let gap = style.gap();
    let align = widgets::align(env, WIDGET);
    let mut lines = texts(env, look, book, memo);
    let row = widgets::line_height(env, style.small());
    lines.fit(env, area.width, area.height.saturating_sub(row + gap));
    let text_h = lines.height(env);
    let total = (text_h + gap + row).min(area.height);
    let top = area.y + (area.height - total) / 2;
    lines.draw(
        ui.canvas(),
        env,
        Rect::new(area.x, top, area.width, text_h),
        align,
    );
    let y = (top + total).saturating_sub(row);
    let percent = percent_text(env, book.percent());
    let row_area = Rect::new(area.x, y, area.width, row);
    let text_w = font.measure(style.small(), &percent) as u16;
    paperpaw_sdk::ui::Label::new(&percent)
        .px(style.small())
        .color(look.faint)
        .draw_from(&mut ui, f32::from(area.x), row_area);
    let bar_h = (thickness(env, WIDGET, row * 4) as u16).min(row);
    let shift = (text_w + gap).min(area.width);
    let bar = Rect::new(
        area.x + shift,
        y + (row - bar_h) / 2,
        area.width - shift,
        bar_h,
    );
    draw_bar(ui.canvas(), env, look, bar, book.percent());
}
