//! *News headlines* on interface 7: the newest headlines of the feeds fetched
//! last, in the shared look of widgets, with settings of its own (how many, a
//! line or two each, the feed's name below each, lines between them). The host
//! makes an instance when a screen shows the widget (`event.widget_made`), asks
//! its height and its targets through the `core.widget` stage and calls the
//! plugin to draw it (`DrawPurpose::Widget`, `frame.id` the instance). The
//! headlines are read from the card when the widget is measured and drawn, never
//! from the network. A tap on a headline opens its feed's book; without
//! headlines the widget says how to add feeds, and a tap opens the list.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, FrameRect, Input};
use paperpaw_sdk::native::commands::CommandSender;
use paperpaw_sdk::native::reply::Reply;
use paperpaw_sdk::native::storage::{self, Root};
use paperpaw_sdk::native::ui::{Instance, WidgetInputKind, WidgetTarget, stages::WidgetCall};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Color, Rect};
use paperpaw_ui_kit::files::read_all_on;
use paperpaw_ui_kit::mem::{buffer, push};
use paperpaw_ui_kit::widget::Align;
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::layout::{caption, caption_height, label, wrap, wrap_whole};
use paperpaw_ui_kit::widget::settings::base;
use paperpaw_ui_kit::{Paint, geometry, paint, text};

use crate::declarations::LINES;
use crate::native_news::{HEADLINES_CAP, HEADLINES_FILE, without_mark};
use crate::native_plugin::DataVolume;

/// The widget's id, as the host names it without a placement.
const NAME: &str = "news.widget.headlines";
/// The most headlines any setting allows.
const MOST: usize = 8;
/// The most bytes of a stage reply (a size, or the targets of eight rows).
const REPLY: usize = 512;
/// The target of a tap on the empty widget: the whole of it.
const WHOLE: u32 = 1;

/// What a tap on the widget does.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Tap {
    /// Open the book at this path.
    Book(String),
    /// Open the list of feeds.
    List,
}

/// One headline: its title and its book.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Headline {
    title: String,
    book: String,
}

/// One widget made for a screen.
#[derive(Debug)]
struct Item {
    instance: Instance,
    /// The widget's id as the host names it, with its placement.
    widget: String,
}

/// The widgets made for screens, by the host's instance.
#[derive(Debug, Default)]
pub(crate) struct Widgets {
    items: Vec<Item>,
}

/// The feed a book stands for: its file's name, without the mark of the
/// feed's address (`News/Tagesschau (3fa9c2).html` is "Tagesschau").
fn source(book: &str) -> &str {
    let file = book.rsplit('/').next().unwrap_or(book);
    without_mark(file.rsplit_once('.').map_or(file, |(stem, _)| stem))
}

/// How many headlines the widget shows at most, by its setting.
fn count(cx: &Cx<'_>) -> Result<usize, Error> {
    cx.settings.with_text("count", |value| {
        value
            .rsplit('.')
            .next()
            .and_then(|n| n.parse().ok())
            .map_or(4, |n: usize| n.clamp(1, MOST))
    })
}

/// Lines a headline takes at most, by its setting.
fn lines_each(cx: &Cx<'_>) -> Result<usize, Error> {
    cx.settings
        .with_text("lines", |value| if value == LINES[1] { 2 } else { 1 })
}

/// The newest headlines and their books, at most the setting: those whose
/// book is still on the card.
fn read(cx: &Cx<'_>) -> Result<Vec<Headline>, Error> {
    let most = count(cx)?;
    let bytes = match read_all_on(&DataVolume(cx.host), HEADLINES_FILE, HEADLINES_CAP) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(Vec::new()),
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        // A list that cannot be read shows no headlines.
        Err(_) => return Ok(Vec::new()),
    };
    let file = String::from_utf8_lossy(&bytes);
    let mut shown = Vec::new();
    for line in file.lines() {
        if shown.len() >= most {
            break;
        }
        let Some((title, book)) = line.split_once('\t') else {
            continue;
        };
        if storage::size(cx.host, Root::Files, book).is_err() {
            continue;
        }
        push(
            &mut shown,
            Headline {
                title: text::copy(title)?,
                book: text::copy(book)?,
            },
        )?;
    }
    Ok(shown)
}

/// The heights of the text lines of the widget.
struct Metrics {
    size: f32,
    text_h: u16,
    small_h: u16,
}

fn metrics(cx: &Cx<'_>) -> Result<Metrics, Error> {
    let size: f32 = cx.settings.size()?.text(cx.style());
    Ok(Metrics {
        size,
        text_h: geometry::line_px(&cx.font, size)?,
        small_h: geometry::line_px(&cx.font, cx.style().small)?,
    })
}

/// The height of a headline's row: its lines, its source, and room around a
/// rule between rows.
fn row_height(cx: &Cx<'_>, m: &Metrics) -> Result<u16, Error> {
    let lines = u16::try_from(lines_each(cx)?).unwrap_or(1);
    let source = if cx.settings.shows("source")? {
        m.small_h
    } else {
        0
    };
    let gap = cx.style().gap;
    let room = if cx.settings.shows("rules")? {
        gap
    } else {
        gap / 3
    };
    Ok(m.text_h
        .saturating_mul(lines)
        .saturating_add(source)
        .saturating_add(room))
}

/// The room the caption takes, if it shows.
fn caption_room(cx: &Cx<'_>) -> Result<u16, Error> {
    if cx.settings.shows("caption")? {
        caption_height(&cx.font, cx.style().small, cx.style().gap)
    } else {
        Ok(0)
    }
}

/// The height the widget takes and the least it can do with.
fn height(cx: &Cx<'_>, shown: usize) -> Result<(u16, u16), Error> {
    let m = metrics(cx)?;
    let row = row_height(cx, &m)?;
    let frame = cx.shape.frame_height().saturating_add(caption_room(cx)?);
    let rows = u16::try_from(shown.max(2)).unwrap_or(2);
    Ok((
        frame.saturating_add(rows.saturating_mul(row)),
        frame.saturating_add(row),
    ))
}

/// Where the caption starts (if it shows) and the rows of as many of `count`
/// headlines as fit in `area`, together in the middle. The caption is left out
/// where it is too wide or leaves no room for a headline; a single headline
/// takes what there is where its row is higher.
fn rows(
    cx: &Cx<'_>,
    area: Rect,
    count: usize,
    title: &str,
) -> Result<(Option<u16>, Vec<Rect>), Error> {
    let m = metrics(cx)?;
    let row = row_height(cx, &m)?.max(1);
    let fits = cx.font.measure_text(cx.style().small, title)? <= f32::from(area.width);
    let room = caption_room(cx)?;
    let caption = if fits && room.saturating_add(row) <= area.height {
        room
    } else {
        0
    };
    let fit = usize::from(area.height.saturating_sub(caption) / row);
    let n = count.min(fit).max(1);
    let row = row.min(area.height);
    let n16 = u16::try_from(n).unwrap_or(1);
    let total = caption
        .saturating_add(row.saturating_mul(n16))
        .min(area.height);
    let top = area.y.saturating_add((area.height - total) / 2);
    let mut rects = Vec::new();
    rects.try_reserve_exact(n).map_err(|_| Error::NoMemory)?;
    for i in 0..n16 {
        rects.push(Rect::new(
            area.x,
            top.saturating_add(caption)
                .saturating_add(row.saturating_mul(i)),
            area.width,
            row,
        ));
    }
    Ok(((caption > 0).then_some(top), rects))
}

/// What the widget says without headlines in `area`: how to add feeds and
/// fetch them, or where that does not fit whole, the shorter "Add feeds under
/// News in the menu".
fn say_empty(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    inner: Rect,
    ink: Color,
) -> Result<(), Error> {
    let small = cx.style().small;
    let width = f32::from(inner.width);
    let long = cx.tr("news.widget.empty")?;
    let whole = wrap_whole(
        &cx.font,
        (cx.style().text, small),
        &long,
        (width, f32::from(inner.height)),
        usize::MAX,
    )?;
    let (size, lines, room) = if let Some((size, lines)) = whole {
        (size, lines, inner.height)
    } else {
        let short = cx.tr("news.widget.empty.short")?;
        // As high as half of the spacing leaves: two lines fit a low place.
        let room = inner.height.saturating_add(area.height) / 2;
        let h = geometry::line_px(&cx.font, small)?.max(1);
        let most = usize::from(room / h).max(1);
        (small, wrap(&cx.font, small, &short, width, most)?, room)
    };
    let h = geometry::line_px(&cx.font, size)?;
    let n = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let total = h.saturating_mul(n).min(room.max(h));
    let mut y = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    for line in &lines {
        let row = Rect::new(inner.x, y, inner.width, h);
        label(out, &cx.font, (size, Align::Center), row, line, ink)?;
        y = y.saturating_add(h);
    }
    Ok(())
}

/// Draws the widget in `area`.
fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    shown: &[Headline],
) -> Result<(), Error> {
    // The snapshot does not tell the gray levels: a card is taken to be drawn
    // on a panel with grays.
    let look = cx.shape.paint(out, area, 256)?;
    if shown.is_empty() {
        return say_empty(cx, out, area, look.area, look.ink);
    }
    let align = cx.settings.align()?;
    let title = cx.tr("news.title")?;
    let (top, places) = rows(cx, look.area, shown.len(), &title)?;
    if let Some(top) = top {
        let at = Rect::new(look.area.x, top, look.area.width, look.area.height);
        caption(
            out,
            &cx.font,
            (cx.style().small, cx.style().gap),
            (at, align, look.ink),
            &title,
        )?;
    }
    let m = metrics(cx)?;
    let rules = cx.settings.shows("rules")?;
    let with_source = cx.settings.shows("source")?;
    let two = lines_each(cx)? > 1;
    let gap = cx.style().gap;
    let room = if rules { gap } else { gap / 3 };
    let faint = look.faint;
    for (n, (row, headline)) in places.iter().zip(shown).enumerate() {
        if n > 0 && rules {
            // A thin rule, light.
            let gray = if look.paper.luma() < 128 {
                Color::gray(110)
            } else {
                Color::gray(150)
            };
            out.try_reserve(1).map_err(|_| Error::NoMemory)?;
            out.push(Paint::Fill(Rect::new(row.x, row.y, row.width, 1), gray));
        }
        let source_h = if with_source { m.small_h } else { 0 };
        let body = Rect::new(
            row.x,
            row.y.saturating_add(room / 2),
            row.width,
            row.height.saturating_sub(room.saturating_add(source_h)),
        );
        if two {
            let lines = wrap(&cx.font, m.size, &headline.title, f32::from(body.width), 2)?;
            let mut y = body.y;
            for line in &lines {
                let at = Rect::new(body.x, y, body.width, m.text_h);
                label(out, &cx.font, (m.size, align), at, line, look.ink)?;
                y = y.saturating_add(m.text_h);
            }
        } else {
            label(
                out,
                &cx.font,
                (m.size, align),
                body,
                &headline.title,
                look.ink,
            )?;
        }
        if source_h > 0 {
            let at = Rect::new(
                row.x,
                body.y.saturating_add(body.height),
                row.width,
                m.small_h,
            );
            label(
                out,
                &cx.font,
                (cx.style().small, align),
                at,
                source(&headline.book),
                faint,
            )?;
        }
    }
    Ok(())
}

/// The rectangle of a frame as the drawing area.
fn area_of(rect: FrameRect) -> Rect {
    Rect::new(
        geometry::clamp_u16(i64::from(rect.x)),
        geometry::clamp_u16(i64::from(rect.y)),
        geometry::clamp_u16(i64::from(rect.width)),
        geometry::clamp_u16(i64::from(rect.height)),
    )
}

/// The number of the target of the headline at `index` (they start at 1).
fn target_of(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}

impl Widgets {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    /// A screen shows the widget `widget`: it gets an instance. A widget this
    /// plugin does not make is none of its business.
    pub(crate) fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        if base(widget) != NAME {
            return Ok(());
        }
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            widget: text::copy(widget)?,
        });
        Ok(())
    }

    /// The screen dropped the instance.
    pub(crate) fn dropped(&mut self, instance: Instance) {
        self.items.retain(|item| item.instance != instance);
    }

    /// Answers a call of the stage `core.widget`: the height, the place kept,
    /// the targets (a headline each, or the whole widget).
    pub(crate) fn stage(
        &self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, .. } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let cx = Cx::open(host, &item.widget)?;
                let shown = read(&cx)?;
                let (natural, least) = height(&cx, shown.len())?;
                call.reply_size(u32::from(natural), u32::from(least), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let shown = read(&cx)?;
                let title = cx.tr("news.title")?;
                let mut targets = Vec::new();
                if shown.is_empty() {
                    push(&mut targets, WidgetTarget::new(WHOLE, rect))?;
                } else {
                    let look = cx.shape.look(area_of(rect));
                    for (index, row) in rows(&cx, look.area, shown.len(), &title)?
                        .1
                        .iter()
                        .enumerate()
                    {
                        push(
                            &mut targets,
                            WidgetTarget::new(target_of(index), paint::frame_rect(*row)),
                        )?;
                    }
                }
                call.reply_targets(&targets, &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// Draws the instance `frame.id` is.
    pub(crate) fn draw(&self, host: &Host<'_>, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let shown = read(&cx)?;
        let mut out = Vec::new();
        draw(&mut cx, &mut out, area_of(frame.rect), &shown)?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    /// What `input` is, if a tap on one of the widgets.
    ///
    /// # Errors
    /// Host refusals and `NoMemory`.
    pub(crate) fn tapped(&self, host: &Host<'_>, input: &Input<'_>) -> Result<Option<Tap>, Error> {
        let Input::Widget {
            instance,
            target,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(None);
        };
        let Some(item) = self.find(instance.get()) else {
            return Ok(None);
        };
        let cx = Cx::open(host, &item.widget)?;
        let shown = read(&cx)?;
        if shown.is_empty() {
            return Ok(Some(Tap::List));
        }
        let Some(index) = usize::try_from(*target).ok().and_then(|t| t.checked_sub(1)) else {
            return Ok(None);
        };
        match shown.into_iter().nth(index) {
            Some(headline) => Ok(Some(Tap::Book(headline.book))),
            None => Ok(None),
        }
    }

    /// Tells every maker its widget may show something else (a setting changed,
    /// a fetch ended); a refusal is left for the next call.
    pub(crate) fn refresh(&self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &self.items {
            commands.widget_changed(item.instance).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_named_by_their_books() {
        assert_eq!(source("books/News/Tagesschau.html"), "Tagesschau");
        assert_eq!(source("News/The Guardian.html"), "The Guardian");
        assert_eq!(source("News/The Guardian (0a9fc2).html"), "The Guardian");
        assert_eq!(source("plain"), "plain");
    }

    #[test]
    fn counts_are_numbers() {
        let counts: Vec<usize> = crate::declarations::COUNTS
            .iter()
            .filter_map(|c| c.rsplit('.').next()?.parse().ok())
            .collect();
        assert_eq!(counts.len(), crate::declarations::COUNTS.len());
        assert!(counts.iter().all(|&c| (1..=MOST).contains(&c)));
    }

    #[test]
    fn targets_start_at_one() {
        assert_eq!(target_of(0), 1);
        assert_eq!(target_of(7), 8);
    }
}
