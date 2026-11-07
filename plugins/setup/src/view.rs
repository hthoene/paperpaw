//! The guide's page as drawing operations, laid out first and replayed in
//! the draw callback: dots for how far it is, the question large with a word
//! on it below, the answer in a card (its value, a switch, two options side
//! by side, or − and + with a sample of the text), and *Next* as the button
//! that is always right; *Skip* on the first page, *Back* on the others.
use crate::model::{Guide, Kind, PageKind, Setting, Target, Val};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error, Host,
        fonts::{Font, FontRole},
        formatting::Arg,
    },
};
use paperpaw_ui_kit::{
    Messages, Paint, Style, geometry, paint,
    text::{self, copy, wrap_with},
};

/// Lines of the explanation at most; at the end, with no card, more.
const HELP_LINES: usize = 6;
const END_LINES: usize = 12;
/// The sample of the text shows at most this many lines.
const SAMPLE_LINES: usize = 6;

/// Where things are on a page.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub dots: Rect,
    pub title: Rect,
    pub help: Rect,
    pub card: Option<Rect>,
    /// − and + of a number, in the card.
    pub stepper: Option<(Rect, Rect)>,
    /// The two options of a choice of two, side by side in the card.
    pub segments: Option<(Rect, Rect)>,
    pub next: Rect,
    pub other: Rect,
    /// Laid out close, where the page did not fit with its roomy margins.
    pub tight: bool,
}

impl Layout {
    /// The target at `point`, if any.
    #[must_use]
    pub fn target_at(&self, point: Point) -> Option<Target> {
        if self.next.contains(point) {
            Some(Target::Next)
        } else if self.other.contains(point) {
            Some(Target::Other)
        } else if self.card.is_some_and(|c| c.contains(point)) {
            Some(Target::Card)
        } else {
            None
        }
    }

    /// What a focus on `target` draws over.
    #[must_use]
    pub fn area_of(&self, target: Target) -> Rect {
        match target {
            Target::Card => self.card.unwrap_or(self.next),
            Target::Next => self.next,
            Target::Other => self.other,
        }
    }
}

/// A page, ready to replay.
#[derive(Debug)]
pub struct Scene {
    pub layout: Layout,
    pub paint: Vec<Paint>,
}

/// The texts a page shows, translated.
struct Words {
    title: String,
    help: String,
    next: String,
    other: String,
    /// The card's label, for the kinds that have one.
    card: String,
    /// The sample of the reading text, where the number is its size.
    sample: String,
}

fn fill(ops: &mut Vec<Paint>, rect: Rect, color: Color) -> Result<(), Error> {
    paint::push(ops, Paint::Fill(rect, color))
}

fn round(
    ops: &mut Vec<Paint>,
    rect: Rect,
    radius: u32,
    stroke: Option<u32>,
    color: Color,
) -> Result<(), Error> {
    paint::push(ops, Paint::Focus(rect, radius, stroke, color))
}

/// One line of `text` at `x` (left) in `area`, vertically centred, clipped to it.
fn line(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    text: &str,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
) -> Result<(), Error> {
    text::label(ops, font, text, size, x, area, color, Some(area))
}

/// `text` centred in `area`.
fn centred(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    text: &str,
    size: f32,
    area: Rect,
    color: Color,
) -> Result<(), Error> {
    let width = font.measure_text(size, text)?.min(f32::from(area.width));
    let x = f32::from(area.x) + (f32::from(area.width) - width) / 2.0;
    line(ops, font, text, size, x, area, color)
}

/// The lines of `text` in `area`, at most `max`, left-aligned.
#[allow(clippy::too_many_arguments, reason = "one paragraph and its target")]
fn paragraph(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    lines: &[String],
    size: f32,
    area: Rect,
    color: Color,
) -> Result<(), Error> {
    let height = geometry::line_px(font, size)?;
    let mut top = area.y;
    for text in lines {
        let row = Rect::new(area.x, top, area.width, height);
        line(ops, font, text, size, f32::from(area.x), row, color)?;
        top = top.saturating_add(height);
    }
    Ok(())
}

/// The wrapped lines of `text` at `size` in `width`, at most `max`.
fn wrapped(
    font: &Font<'_>,
    text: &str,
    size: f32,
    width: u16,
    max: usize,
) -> Result<Vec<String>, Error> {
    wrap_with(|s| font.measure_text(size, s), text, f32::from(width), max)
}

/// A number with its unit (`{count}` is replaced in the host's text, else the
/// number comes first).
fn number_text(
    host: &Host<'_>,
    messages: &mut Messages,
    setting: &Setting,
    value: i64,
) -> Result<String, Error> {
    if !setting.unit.is_empty() {
        let template = messages.copy(host, &setting.unit)?;
        if template.contains("{count}") {
            return messages.format(
                host,
                &setting.unit,
                &[Arg::Int {
                    name: "count",
                    value,
                }],
            );
        }
        let digits = digits_of(host, messages, value)?;
        return text::concat(&[&digits, " ", &template]);
    }
    digits_of(host, messages, value)
}

/// `value` in the language's digits.
fn digits_of(host: &Host<'_>, messages: &mut Messages, value: i64) -> Result<String, Error> {
    let mut buffer = [0; 20];
    let digits = text::decimal_u64(value.unsigned_abs(), &mut buffer);
    let digits = messages.digits(host, digits)?;
    if value < 0 {
        text::concat(&["−", &digits])
    } else {
        Ok(digits)
    }
}

/// The words of the page.
fn words(host: &Host<'_>, guide: &Guide, messages: &mut Messages) -> Result<Words, Error> {
    let end = guide.at_end();
    let setting = guide.setting.as_ref();
    let (title, help) = match guide.step() {
        None => (
            messages.copy(host, "setup.end.title")?,
            messages.copy(host, "setup.end.help")?,
        ),
        Some(step) => {
            let title = if step.title.is_empty() {
                setting.map_or(String::new(), |s| s.title.clone())
            } else {
                step.title.clone()
            };
            (title, setting.map_or(String::new(), |s| s.help.clone()))
        }
    };
    let next = messages.copy(host, if end { "setup.done" } else { "setup.next" })?;
    let other = messages.copy(host, "setup.back")?;
    let card = match (guide.step().map(|s| s.kind), setting) {
        (Some(PageKind::Screen), _) => text::concat(&[&messages.copy(host, "setup.open-page")?, " ›"])?,
        (_, Some(s)) => match (&s.value, s.kind) {
            (Some(Val::Int(v)), Kind::Int) => number_text(host, messages, s, *v)?,
            (_, Kind::Bool) => String::new(),
            _ => {
                let name = s.value_name().unwrap_or_default();
                if s.editor || s.choices.len() > 2 {
                    text::concat(&[&name, " ›"])?
                } else {
                    name
                }
            }
        },
        _ => String::new(),
    };
    let sample = messages.copy(host, "setup.sample")?;
    Ok(Words {
        title,
        help,
        next,
        other,
        card,
        sample,
    })
}

/// Pixels of `points` at `dpi`.
fn points(points: i64, dpi: u16) -> f32 {
    points as f32 * f32::from(dpi) / 72.0
}

/// Two columns of `row` `gap` apart.
fn two_columns(row: Rect, gap: u16) -> (Rect, Rect) {
    let half = row.width.saturating_sub(gap) / 2;
    (
        Rect::new(row.x, row.y, half, row.height),
        Rect::new(row.x + half + gap, row.y, row.width - half - gap, row.height),
    )
}

/// A page laid out, with the lines of its texts.
struct Laid {
    layout: Layout,
    sample: Vec<String>,
    title: Vec<String>,
    help: Vec<String>,
    /// Whether all of it fits.
    fits: bool,
}

/// What a page is laid out from.
struct Metrics<'a, 'f> {
    font: &'a Font<'f>,
    reading: Option<&'a Font<'f>>,
    style: &'a Style,
    words: &'a Words,
    guide: &'a Guide,
    width: u16,
    height: u16,
    dpi: u16,
}

impl Metrics<'_, '_> {
    /// The height of the card and the lines of the sample in it.
    fn card_height(&self, inner: u16, button: u16) -> Result<(u16, Vec<String>), Error> {
        let s = self.style;
        let Some(setting) = self.guide.setting.as_ref().filter(|_| self.guide.has_card()) else {
            return Ok((
                if self.guide.has_card() {
                    button + s.gap * 2
                } else {
                    0
                },
                Vec::new(),
            ));
        };
        if setting.kind != Kind::Int || setting.two().is_some() {
            return Ok((button + s.gap * 2, Vec::new()));
        }
        // A number: − value +, and below it the text at that size, or room for a line.
        let (below, lines) = match (setting.sample_points(), self.reading) {
            (Some(pt), Some(reading)) => {
                let size = points(pt, self.dpi);
                let wrap = wrapped(reading, &self.words.sample, size, inner, SAMPLE_LINES)?;
                let line = geometry::line_px(reading, size)?;
                (line.saturating_mul(wrap.len().max(1) as u16), wrap)
            }
            _ => (geometry::line_px(self.font, s.text)?.saturating_mul(2), Vec::new()),
        };
        Ok((button + s.gap * 4 + below, lines))
    }

    fn layout(&self, tight: bool) -> Result<Laid, Error> {
        let s = self.style;
        let gap = s.gap;
        let margin = if tight { s.margin } else { s.margin + gap };
        let (after_dots, after_help) = if tight { (2, 1) } else { (4, 3) };
        let width = self.width.saturating_sub(2 * margin);
        let mut y = margin;
        let dots = Rect::new(margin, y, width, gap);
        y += dots.height + gap * after_dots;
        let title_size = s.title * 1.3;
        let title_lines = wrapped(self.font, &self.words.title, title_size, width, 2)?;
        let title_height = geometry::line_px(self.font, title_size)?;
        let title = Rect::new(
            margin,
            y,
            width,
            title_height.saturating_mul(title_lines.len().max(1) as u16),
        );
        y += title.height + gap;
        let help_max = if self.guide.has_card() { HELP_LINES } else { END_LINES };
        let help_lines = wrapped(self.font, &self.words.help, s.small, width, help_max)?;
        let help_height = geometry::line_px(self.font, s.small)?;
        let help = Rect::new(
            margin,
            y,
            width,
            help_height.saturating_mul(help_lines.len() as u16),
        );
        y += help.height + gap * after_help;
        let button = if tight { s.row.min(s.row * 4 / 5).max(s.line * 6) } else { s.row };
        let inner = width.saturating_sub(2 * s.margin);
        let (card_height, sample) = self.card_height(inner, button)?;
        let fits = |beside: bool| {
            let rows = if beside { 1 } else { 2 };
            let top = self
                .height
                .saturating_sub(margin + rows * button + (rows - 1) * gap * 2);
            y + card_height + gap <= top
        };
        let beside = tight && !fits(false);
        let fit = fits(beside);
        let bottom = self.height.saturating_sub(margin + button);
        let (next, other) = if beside {
            let half = width.saturating_sub(gap) / 2;
            (
                Rect::new(margin + half + gap, bottom, width - half - gap, button),
                Rect::new(margin, bottom, half, button),
            )
        } else {
            let above = bottom.saturating_sub(gap * 2 + button);
            (
                Rect::new(margin, above, width, button),
                Rect::new(margin, bottom, width, button),
            )
        };
        let (card, stepper, segments) = if self.guide.has_card() {
            let card = Rect::new(
                margin,
                y,
                width,
                card_height.min(next.y.saturating_sub(y + gap)),
            );
            let setting = self.guide.setting.as_ref();
            let stepper = setting
                .filter(|s| s.kind == Kind::Int)
                .map(|_| {
                    let top = card.y + gap;
                    let inset = s.margin;
                    (
                        Rect::new(card.x + inset, top, button, button),
                        Rect::new(card.x + card.width - inset - button, top, button, button),
                    )
                });
            let segments = setting.and_then(Setting::two).map(|_| {
                let inset = if tight { 0 } else { s.margin / 2 };
                let above = inset.min(gap).min(card.height / 6);
                let row = Rect::new(
                    card.x + inset,
                    card.y + above,
                    card.width.saturating_sub(2 * inset),
                    card.height.saturating_sub(2 * above),
                );
                two_columns(row, inset.max(gap / 2))
            });
            (Some(card), stepper, segments)
        } else {
            (None, None, None)
        };
        Ok(Laid {
            layout: Layout {
                dots,
                title,
                help,
                card,
                stepper,
                segments,
                next,
                other,
                tight,
            },
            sample,
            title: title_lines,
            help: help_lines,
            fits: fit,
        })
    }
}

/// The page, laid out and drawn as operations.
///
/// # Errors
/// Font, host and allocation refusals.
pub fn scene(host: &Host<'_>, guide: &Guide, width: u16, height: u16, dpi: u16) -> Result<Scene, Error> {
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let reading = Font::acquire_role(host, FontRole::Reading).ok();
    let result = build(host, guide, width, height, dpi, &font, reading.as_ref());
    let released = font.release();
    if let Some(reading) = reading {
        let _ = reading.release();
    }
    let scene = result?;
    released?;
    Ok(scene)
}

fn build(
    host: &Host<'_>,
    guide: &Guide,
    width: u16,
    height: u16,
    dpi: u16,
    font: &Font<'_>,
    reading: Option<&Font<'_>>,
) -> Result<Scene, Error> {
    let snapshot = paperpaw_sdk::native::snapshot::capture_snapshot(host)?;
    let style = Style::read(host, &snapshot, font)?;
    let mut messages = Messages::new()?;
    let words = words(host, guide, &mut messages)?;
    let metrics = Metrics {
        font,
        reading,
        style: &style,
        words: &words,
        guide,
        width,
        height,
        dpi,
    };
    let roomy = metrics.layout(false)?;
    let laid = if roomy.fits { roomy } else { metrics.layout(true)? };
    let Laid {
        layout,
        sample,
        title: title_lines,
        help: help_lines,
        ..
    } = laid;
    let mut ops = Vec::new();
    fill(&mut ops, Rect::new(0, 0, width, height), style.paper)?;
    dots(&mut ops, &layout, guide, &style)?;
    paragraph(&mut ops, font, &title_lines, style.title * 1.3, layout.title, style.ink)?;
    paragraph(&mut ops, font, &help_lines, style.small, layout.help, style.note)?;
    if let Some(card) = layout.card {
        draw_card(&mut ops, font, reading, &layout, guide, &style, &words, &sample, card, dpi, &mut messages, host)?;
    }
    let focus = |t| guide.shown && guide.focus == t;
    let other = if guide.page == 0 && layout.tight {
        messages.copy(host, "setup.skip-short")?
    } else if guide.page == 0 {
        messages.copy(host, "setup.skip")?
    } else {
        copy(&words.other)?
    };
    button(&mut ops, font, &style, layout.next, &words.next, true, focus(Target::Next))?;
    button(&mut ops, font, &style, layout.other, &other, false, focus(Target::Other))?;
    Ok(Scene { layout, paint: ops })
}

/// Dots for how far the guide is: full up to the page shown, open after.
fn dots(ops: &mut Vec<Paint>, layout: &Layout, guide: &Guide, style: &Style) -> Result<(), Error> {
    let count = guide.steps.len() + 1;
    let r = (i32::from(layout.dots.height) / 2).max(3);
    let gap = r * 4;
    let mut x = i32::from(layout.dots.x) + i32::from(layout.dots.width) / 2 - (count as i32 - 1) * gap / 2;
    let y = i32::from(layout.dots.y) + r;
    for i in 0..count {
        let d = (r * 2) as u16;
        let rect = Rect::new((x - r).max(0) as u16, (y - r).max(0) as u16, d, d);
        let stroke = (i > guide.page).then_some(2);
        round(ops, rect, r as u32, stroke, style.ink)?;
        x += gap;
    }
    Ok(())
}

/// The radius of buttons and cards.
fn radius(style: &Style, rect: Rect) -> u32 {
    if style.pill {
        u32::from(rect.height / 2)
    } else {
        style.corner
    }
}

/// A button: filled where it is the one that is always right, else framed;
/// thicker frame where it has the focus.
fn button(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    label: &str,
    primary: bool,
    focus: bool,
) -> Result<(), Error> {
    let r = radius(style, rect);
    let line = u32::from(style.line).max(1);
    if primary {
        round(ops, rect, r, None, style.ink)?;
    } else {
        round(ops, rect, r, Some(line), style.ink)?;
    }
    if focus {
        round(ops, rect, r, Some(line * 3), style.ink)?;
    }
    let color = if primary { style.paper } else { style.ink };
    centred(ops, font, label, style.text, rect, color)
}

#[allow(clippy::too_many_arguments, reason = "one card and what it is drawn from")]
fn draw_card(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    reading: Option<&Font<'_>>,
    layout: &Layout,
    guide: &Guide,
    style: &Style,
    words: &Words,
    sample: &[String],
    card: Rect,
    dpi: u16,
    messages: &mut Messages,
    host: &Host<'_>,
) -> Result<(), Error> {
    let focused = guide.shown && guide.focus == Target::Card;
    let thin = u32::from(style.line).max(1);
    let r = radius(style, card);
    let setting = guide.setting.as_ref();
    // Two options side by side need no frame of their own, but for the focus.
    if focused || layout.segments.is_none() {
        round(ops, card, r, Some(if focused { thin * 3 } else { thin }), style.ink)?;
    }
    let inner = Rect::new(
        card.x + style.margin,
        card.y,
        card.width.saturating_sub(2 * style.margin),
        style.row,
    );
    if let (Some((left, right)), Some(two)) = (layout.segments, setting.and_then(Setting::two)) {
        let current = setting.and_then(|s| s.value.as_ref());
        for ((value, name), rect) in two.into_iter().zip([left, right]) {
            let on = matches!(current, Some(Val::Text(v)) if v == value);
            button(ops, font, style, rect, name, on, false)?;
        }
        return Ok(());
    }
    match setting {
        Some(s) if s.kind == Kind::Bool && guide.step().is_some_and(|x| x.kind == PageKind::Setting) => {
            let on = matches!(s.value, Some(Val::Bool(true)));
            let state = messages.copy(host, if on { "core.on" } else { "core.off" })?;
            line_text(ops, font, style, &s.title, inner, false)?;
            line_text(ops, font, style, &state, inner, true)?;
            return Ok(());
        }
        Some(s) if s.kind == Kind::Int => {
            let Some((minus, plus)) = layout.stepper else {
                return Ok(());
            };
            // Filled while the buttons change it.
            button(ops, font, style, minus, "−", guide.editing, false)?;
            button(ops, font, style, plus, "+", guide.editing, false)?;
            let between = Rect::new(
                minus.x + minus.width,
                minus.y,
                plus.x.saturating_sub(minus.x + minus.width),
                minus.height,
            );
            centred(ops, font, &words.card, style.title, between, style.ink)?;
            // A sample of the text at that size, where it is one.
            if let (Some(pt), Some(reading)) = (s.sample_points(), reading) {
                let size = points(pt, dpi);
                let height = geometry::line_px(reading, size)?;
                let top = minus.y + minus.height + style.gap;
                let area = Rect::new(
                    inner.x,
                    top,
                    inner.width,
                    (card.y + card.height).saturating_sub(top + style.gap),
                );
                let mut y = area.y;
                for (i, text) in sample.iter().enumerate() {
                    if y + height > area.y + area.height && i > 0 {
                        break;
                    }
                    let row = Rect::new(area.x, y, area.width, height);
                    line(ops, reading, text, size, f32::from(area.x), row, style.ink)?;
                    y = y.saturating_add(height);
                }
            }
            return Ok(());
        }
        _ => {}
    }
    // A value, or that the page opens a screen of its plugin.
    let row = Rect::new(inner.x, card.y, inner.width, card.height);
    centred_left(ops, font, style, &words.card, row)
}

/// `text` at the left (or right) of `area`, small where it would not fit.
fn line_text(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    text: &str,
    area: Rect,
    right: bool,
) -> Result<(), Error> {
    let x = if right {
        f32::from(area.x) + f32::from(area.width) - font.measure_text(style.text, text)?
    } else {
        f32::from(area.x)
    };
    line(ops, font, text, style.text, x, area, style.ink)
}

/// A card's label, left-aligned: the title size where it fits, else the small.
fn centred_left(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    label: &str,
    row: Rect,
) -> Result<(), Error> {
    let size = if font.measure_text(style.title, label)? <= f32::from(row.width) {
        style.title
    } else {
        style.small
    };
    line(ops, font, label, size, f32::from(row.x), row, style.ink)
}
