//! The notes vault's home widgets on interface 7: *Recent notes*, *Pinned note*,
//! *Note of the day*, *Random note*, *Note tags* and *Search notes*. The host
//! makes an instance when a screen shows one (`event.widget_made`), asks it for
//! its height and where each line sits through the `core.widget` stage, and
//! calls the plugin to draw it (`DrawPurpose::Widget`, `frame.id` the instance).
//!
//! They show what the index holds (made on the vault's page); before it is,
//! they say so, and a tap opens the page. Each line is a target: a tap on a
//! note opens it, a tap on a tag opens the notes that have it, a tap on *Search*
//! asks for words. The lines an instance shows are kept with what they were made
//! from (the index's state, the day, the widget's settings), so that the index
//! (hundreds of KiB on a big vault) is read again only when that changes.
use crate::native_card::with_card_or_none;
use crate::native_home::vault;
use crate::native_model::{
    index,
    note::{self, folder_in},
    page::{self, Order},
    source::Source,
};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, FrameRect, Input};
use paperpaw_sdk::native::commands::CommandSender;
use paperpaw_sdk::native::reply::Reply;
use paperpaw_sdk::native::snapshot::capture_snapshot;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::ui::{Instance, WidgetInputKind, WidgetTarget, stages::WidgetCall};
use paperpaw_sdk::native::{Error, Host, clock};
use paperpaw_sdk::{Rect, Storage};
use paperpaw_ui_kit::mem::{buffer, push};
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::layout::{caption, caption_height, label};
use paperpaw_ui_kit::widget::settings::{Settings, base};
use paperpaw_ui_kit::{Paint, geometry, paint, text};

/// The most bytes of a stage reply (a size, or the targets of up to 12 lines).
const REPLY: usize = 1280;
/// The longest text a widget setting holds (a folder, a note's title).
const SETTING_LIMIT: usize = 255;
/// The widget's lines at most (the *Note tags* setting's maximum).
const MAX_LINES: usize = 12;

/// Which widget an instance is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Recent,
    Pinned,
    Day,
    Random,
    Tags,
    Search,
}

impl Kind {
    /// The widget `widget` (an id with its placement) is.
    fn of(widget: &str) -> Option<Self> {
        Some(match base(widget) {
            "notes-vault.recent" => Self::Recent,
            "notes-vault.pinned" => Self::Pinned,
            "notes-vault.day" => Self::Day,
            "notes-vault.random" => Self::Random,
            "notes-vault.tags" => Self::Tags,
            "notes-vault.search" => Self::Search,
            _ => return None,
        })
    }

    /// The id of the widget, and the key of its heading.
    const fn id(self) -> &'static str {
        match self {
            Self::Recent => "notes-vault.recent",
            Self::Pinned => "notes-vault.pinned",
            Self::Day => "notes-vault.day",
            Self::Random => "notes-vault.random",
            Self::Tags => "notes-vault.tags",
            Self::Search => "notes-vault.search",
        }
    }
}

/// What a tap on a line does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tap {
    /// Opens the note at the path in the reader.
    Note(String),
    /// Opens the notes that have the tag.
    Tag(String),
    /// Opens the vault's page.
    Page,
    /// Asks for words to search the titles for.
    Search,
}

/// A line of a widget: a note (or a tag), and what a tap on it does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    title: String,
    /// Under the title, small.
    note: String,
    tap: Tap,
}

/// One widget made for a screen.
#[derive(Debug)]
struct Item {
    instance: Instance,
    kind: Kind,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// The lines, and what they were made from.
    made: Option<(u64, Vec<Line>)>,
}

/// The widgets made for screens, by the host's instance.
#[derive(Debug, Default)]
pub(crate) struct Widgets {
    items: Vec<Item>,
}

/// A text setting; one too long to be a folder's or a title is none.
fn text_setting(settings: &Settings<'_, '_>, key: &str) -> Result<String, Error> {
    match settings.string(key, SETTING_LIMIT) {
        Ok(raw) => text::copy(raw.trim()),
        Err(Error::NoSpace) => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// The widget's own folder (`a/b`, no slashes at its ends).
fn folder_of(settings: &Settings<'_, '_>) -> Result<String, Error> {
    let raw = text_setting(settings, "folder")?;
    text::copy(raw.trim_matches('/'))
}

/// The number of lines the widget's setting asks for.
fn count_of(settings: &Settings<'_, '_>, fallback: i64, most: i64) -> Result<usize, Error> {
    let value = settings.int("count", fallback)?;
    let value = if value >= 1 {
        value.min(most)
    } else {
        fallback
    };
    Ok(usize::try_from(value).unwrap_or(1).min(MAX_LINES))
}

/// Folds `part` into a hash (FNV-1a).
fn mix(hash: u64, part: &[u8]) -> u64 {
    part.iter().fold(hash, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A line that says why there is nothing to show; a tap opens the page.
fn waiting(cx: &mut Cx<'_>, key: &str) -> Result<Vec<Line>, Error> {
    let mut lines = Vec::new();
    push(
        &mut lines,
        Line {
            title: cx.tr(key)?,
            note: String::new(),
            tap: Tap::Page,
        },
    )?;
    Ok(lines)
}

/// What the lines of the widget are made from, as far as it can change them:
/// the index (its state, with the stamp that changes when a note comes, goes or
/// changes, and the size of the tags' file), the widget's settings, the day, the
/// hour (the random note stays for one) and the language. `None` where there is
/// nothing to read (no vault, no index, *Search*, which reads none).
fn signature(
    cx: &mut Cx<'_>,
    kind: Kind,
    data: &DataStorage<'_, '_>,
) -> Result<Option<u64>, Error> {
    let root = vault(cx.host)?;
    if kind == Kind::Search || root.is_empty() {
        return Ok(None);
    }
    let Some(state) = index::kept(data, &root)? else {
        return Ok(None);
    };
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    hash = mix(hash, root.as_bytes());
    hash = mix(hash, &state.built.to_le_bytes());
    hash = mix(hash, &state.notes.to_le_bytes());
    hash = mix(hash, &state.stamp.to_le_bytes());
    hash = mix(hash, &[u8::from(state.tagged)]);
    hash = mix(hash, &data.size(index::TAGS).unwrap_or(0).to_le_bytes());
    let snapshot = capture_snapshot(cx.host)?;
    let day = snapshot.unix_seconds().unwrap_or(0) / 86_400;
    hash = mix(hash, &day.to_le_bytes());
    if kind == Kind::Random {
        hash = mix(
            hash,
            &(clock::micros(cx.host)? / 3_600_000_000).to_le_bytes(),
        );
    }
    hash = mix(hash, folder_of(&cx.settings)?.as_bytes());
    hash = mix(hash, text_setting(&cx.settings, "title")?.as_bytes());
    hash = mix(hash, &count_of(&cx.settings, 5, 12)?.to_le_bytes());
    for part in ["where", "excerpt", "heading"] {
        hash = mix(hash, &[u8::from(cx.settings.shows(part)?)]);
    }
    hash = mix(hash, cx.plural("notes-vault.notes", 2, &[])?.as_bytes());
    Ok(Some(hash))
}

/// The first `count` notes of the index that `source` offers, latest first.
fn newest(
    data: &DataStorage<'_, '_>,
    source: &Source,
    count: usize,
) -> Result<Vec<page::Row>, Error> {
    // `Source::Notes` reads the index alone: the vault storage is not asked.
    let mut window = page::window(Order::Recent, None, &mut |emit| {
        source.feed(data, data, emit)
    })?;
    window.rows.truncate(count);
    Ok(window.rows)
}

/// How many notes `source` has.
fn total(data: &DataStorage<'_, '_>, source: &Source) -> Result<usize, Error> {
    let mut count = 0usize;
    source.feed(data, data, &mut |_| {
        count += 1;
        Ok(())
    })?;
    Ok(count)
}

/// The path of the `n`th note of `source`, in the index's own order.
fn nth(data: &DataStorage<'_, '_>, source: &Source, n: usize) -> Result<Option<String>, Error> {
    let (mut at, mut found) = (0usize, None);
    let mut failed = None;
    source.feed(data, data, &mut |row| {
        if at == n && found.is_none() {
            match text::copy(row.path) {
                Ok(path) => found = Some(path),
                Err(error) => failed = Some(error),
            }
        }
        at += 1;
        Ok(())
    })?;
    failed.map_or(Ok(found), Err)
}

/// The path of the note whose title is `title`, whatever the case.
fn find_by_title(data: &DataStorage<'_, '_>, title: &str) -> Result<Option<String>, Error> {
    let mut found = None;
    let mut failed = None;
    index::notes(data, &mut |_, path| {
        if note::title(path).eq_ignore_ascii_case(title) {
            match text::copy(path) {
                Ok(path) => found = Some(path),
                Err(error) => failed = Some(error),
            }
            return false;
        }
        true
    })?;
    failed.map_or(Ok(found), Err)
}

/// The line of the note at `path`: its title, and the beginning of its text
/// where that is shown.
fn note_line(cx: &mut Cx<'_>, path: &str) -> Result<Line, Error> {
    let excerpt = if cx.settings.shows("excerpt")? {
        with_card_or_none(cx.host, |vault| note::excerpt_of_file(vault, path))?
    } else {
        String::new()
    };
    Ok(Line {
        title: text::copy(note::title(path))?,
        note: excerpt,
        tap: Tap::Note(text::copy(path)?),
    })
}

fn one(line: Line) -> Result<Vec<Line>, Error> {
    let mut lines = Vec::new();
    push(&mut lines, line)?;
    Ok(lines)
}

/// The notes changed last.
fn recent(cx: &mut Cx<'_>, data: &DataStorage<'_, '_>, root: &str) -> Result<Vec<Line>, Error> {
    let source = Source::Notes {
        folder: folder_of(&cx.settings)?,
        query: String::new(),
    };
    let count = count_of(&cx.settings, 5, 10)?;
    let show_where = cx.settings.shows("where")?;
    let rows = newest(data, &source, count)?;
    let mut lines = Vec::new();
    lines
        .try_reserve_exact(rows.len())
        .map_err(|_| Error::NoMemory)?;
    for row in rows {
        lines.push(Line {
            note: if show_where {
                text::copy(folder_in(&row.path, root))?
            } else {
                String::new()
            },
            tap: Tap::Note(row.path),
            title: row.title,
        });
    }
    Ok(lines)
}

/// The note whose title is set.
fn pinned(cx: &mut Cx<'_>, data: &DataStorage<'_, '_>) -> Result<Vec<Line>, Error> {
    let wanted = text_setting(&cx.settings, "title")?;
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    match find_by_title(data, &wanted)? {
        Some(path) => one(note_line(cx, &path)?),
        None => Ok(Vec::new()),
    }
}

/// One note of the vault (or of a folder): the day's, or a random one.
fn one_of_the_vault(
    cx: &mut Cx<'_>,
    data: &DataStorage<'_, '_>,
    kind: Kind,
) -> Result<Vec<Line>, Error> {
    let source = Source::Notes {
        folder: folder_of(&cx.settings)?,
        query: String::new(),
    };
    let count = total(data, &source)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let unix = capture_snapshot(cx.host)?.unix_seconds().unwrap_or(0);
    let pick = if kind == Kind::Day {
        (unix / 86_400).unsigned_abs() as usize % count
    } else {
        let seed = clock::micros(cx.host)? ^ unix.unsigned_abs();
        ((seed.wrapping_mul(6_364_136_223_846_793_005) >> 33) as usize) % count
    };
    match nth(data, &source, pick)? {
        Some(path) => one(note_line(cx, &path)?),
        None => Ok(Vec::new()),
    }
}

/// The tags with most notes.
fn tags(cx: &mut Cx<'_>, data: &DataStorage<'_, '_>) -> Result<Vec<Line>, Error> {
    let (mut tags, _) = index::tag_counts(data)?;
    paperpaw_sdk::sort::sort_by(&mut tags, |a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let count = count_of(&cx.settings, 5, 12)?;
    let mut lines = Vec::new();
    for (tag, n) in tags.into_iter().take(count) {
        let note = cx.plural("notes-vault.notes", i64::from(n), &[])?;
        let title = text::concat(&["#", &tag])?;
        push(
            &mut lines,
            Line {
                title,
                note,
                tap: Tap::Tag(tag),
            },
        )?;
    }
    Ok(lines)
}

/// What the widget of `kind` shows now.
fn read(cx: &mut Cx<'_>, kind: Kind) -> Result<Vec<Line>, Error> {
    if kind == Kind::Search {
        return one(Line {
            title: cx.tr("notes-vault.widget.search")?,
            note: String::new(),
            tap: Tap::Search,
        });
    }
    let root = vault(cx.host)?;
    if root.is_empty() {
        return waiting(cx, "notes-vault.widget.no-vault");
    }
    let data = DataStorage::new(cx.host);
    if index::kept(&data, &root)?.is_none() {
        return waiting(cx, "notes-vault.widget.no-index");
    }
    let lines = match kind {
        Kind::Recent => recent(cx, &data, &root)?,
        Kind::Pinned => pinned(cx, &data)?,
        Kind::Day | Kind::Random => one_of_the_vault(cx, &data, kind)?,
        Kind::Tags => tags(cx, &data)?,
        Kind::Search => Vec::new(),
    };
    if lines.is_empty() {
        let key = match kind {
            Kind::Pinned => "notes-vault.widget.no-pin",
            Kind::Tags => "notes-vault.empty.tags",
            _ => "notes-vault.empty.recent",
        };
        return waiting(cx, key);
    }
    Ok(lines)
}

/// The text size and the heights of a line of it and of a small one.
fn sizes(cx: &Cx<'_>) -> Result<(f32, u16, u16), Error> {
    let size: f32 = cx.settings.size()?.text(cx.style());
    Ok((
        size,
        geometry::line_px(&cx.font, size)?,
        geometry::line_px(&cx.font, cx.style().small)?,
    ))
}

/// The room the heading takes, where the widget shows one.
fn heading_room(cx: &Cx<'_>) -> Result<u16, Error> {
    if cx.settings.shows("heading")? {
        caption_height(&cx.font, cx.style().small, cx.style().gap)
    } else {
        Ok(0)
    }
}

/// The height of a line (its title, and its note where it has one).
fn line_height(line: &Line, text_h: u16, small_h: u16) -> u16 {
    text_h.saturating_add(if line.note.is_empty() { 0 } else { small_h })
}

/// The height the widget takes and the least it can do with.
fn height(cx: &Cx<'_>, lines: &[Line]) -> Result<(u16, u16), Error> {
    let (_, text_h, small_h) = sizes(cx)?;
    let rows = lines
        .iter()
        .map(|l| line_height(l, text_h, small_h))
        .fold(0u16, u16::saturating_add);
    let frame = cx.shape.frame_height().saturating_add(heading_room(cx)?);
    Ok((
        frame.saturating_add(rows.max(text_h)),
        frame.saturating_add(text_h),
    ))
}

/// The rows the lines take in the area left under the heading, as far as they
/// fit: the same places the lines are drawn in and tapped on.
fn places(cx: &Cx<'_>, inner: Rect, lines: &[Line]) -> Result<Vec<Rect>, Error> {
    let (_, text_h, small_h) = sizes(cx)?;
    let bottom = i32::from(inner.y) + i32::from(inner.height);
    let mut rows = Vec::new();
    let mut y = inner.y;
    for line in lines {
        let h = line_height(line, text_h, small_h);
        if i32::from(y) + i32::from(h) > bottom {
            break;
        }
        push(&mut rows, Rect::new(inner.x, y, inner.width, h))?;
        y = y.saturating_add(h);
    }
    Ok(rows)
}

/// Draws the widget in `area`.
fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    kind: Kind,
    lines: &[Line],
) -> Result<(), Error> {
    // The snapshot does not tell the gray levels: a card is taken to be drawn
    // on a panel with grays.
    let look = cx.shape.paint(out, area, 256)?;
    let align = cx.settings.align()?;
    let mut inner = look.area;
    if cx.settings.shows("heading")? {
        let title = cx.tr(kind.id())?;
        inner = caption(
            out,
            &cx.font,
            (cx.style().small, cx.style().gap),
            (inner, align, look.faint),
            &title,
        )?;
    }
    let (size, text_h, small_h) = sizes(cx)?;
    let small = cx.style().small;
    let rows = places(cx, inner, lines)?;
    for (line, row) in lines.iter().zip(&rows) {
        label(
            out,
            &cx.font,
            (size, align),
            Rect::new(row.x, row.y, row.width, text_h),
            &line.title,
            look.ink,
        )?;
        if !line.note.is_empty() {
            label(
                out,
                &cx.font,
                (small, align),
                Rect::new(row.x, row.y.saturating_add(text_h), row.width, small_h),
                &line.note,
                look.faint,
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

/// The target of the line at `index`: targets are numbered from 1 (0 is none).
fn target_of(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}

impl Widgets {
    fn position(&self, instance: u32) -> Option<usize> {
        self.items
            .iter()
            .position(|item| item.instance.get() == instance)
    }

    /// The lines of the item at `at`, made anew where what they are made from
    /// has changed since they were.
    fn lines(&mut self, cx: &mut Cx<'_>, at: usize) -> Result<&[Line], Error> {
        let kind = self.items.get(at).ok_or(Error::NotFound)?.kind;
        let data = DataStorage::new(cx.host);
        let stamp = signature(cx, kind, &data)?;
        let item = self.items.get_mut(at).ok_or(Error::NotFound)?;
        let kept = matches!((&item.made, stamp), (Some((made, _)), Some(now)) if *made == now);
        if !kept {
            let lines = read(cx, kind)?;
            // Lines with no signature (no index) are made anew each time.
            item.made = Some((stamp.unwrap_or(0), lines));
        }
        item.made
            .as_ref()
            .map(|(_, lines)| lines.as_slice())
            .ok_or(Error::State)
    }

    /// A screen shows the widget `widget`: it gets an instance. A widget this
    /// plugin does not make is none of its business.
    pub(crate) fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        let Some(kind) = Kind::of(widget) else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            kind,
            widget: text::copy(widget)?,
            made: None,
        });
        Ok(())
    }

    /// The screen dropped the instance.
    pub(crate) fn dropped(&mut self, instance: Instance) {
        self.items.retain(|item| item.instance != instance);
    }

    /// Answers a call of the stage `core.widget`: the height, the place kept,
    /// the targets (one for each line that fits).
    pub(crate) fn stage(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, .. } => {
                let at = self.position(instance.get()).ok_or(Error::NotFound)?;
                let widget = text::copy(&self.items[at].widget)?;
                let mut cx = Cx::open(host, &widget)?;
                let lines = self.lines(&mut cx, at)?;
                let (natural, least) = height(&cx, lines)?;
                call.reply_size(u32::from(natural), u32::from(least), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.position(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                let at = self.position(instance.get()).ok_or(Error::NotFound)?;
                let widget = text::copy(&self.items[at].widget)?;
                let mut cx = Cx::open(host, &widget)?;
                let lines = self.lines(&mut cx, at)?;
                let look = cx.shape.look(area_of(rect));
                let mut inner = look.area;
                let room = heading_room(&cx)?.min(inner.height);
                inner = Rect::new(
                    inner.x,
                    inner.y.saturating_add(room),
                    inner.width,
                    inner.height - room,
                );
                let rows = places(&cx, inner, lines)?;
                let mut targets = Vec::new();
                for (index, row) in rows.iter().enumerate() {
                    push(
                        &mut targets,
                        WidgetTarget::new(target_of(index), paint::frame_rect(*row)),
                    )?;
                }
                call.reply_targets(&targets, &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// Draws the instance `frame.id` is.
    pub(crate) fn draw(&mut self, host: &Host<'_>, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let at = self.position(frame.id).ok_or(Error::NotFound)?;
        let widget = text::copy(&self.items[at].widget)?;
        let kind = self.items[at].kind;
        let mut cx = Cx::open(host, &widget)?;
        let lines = self.lines(&mut cx, at)?;
        let mut out = Vec::new();
        draw(&mut cx, &mut out, area_of(frame.rect), kind, lines)?;
        paint::replay_with(&mut frame, &cx.font, None, &out)
    }

    /// What `input` is, if a tap on a line of one of the widgets.
    ///
    /// # Errors
    /// Host refusals and `NoMemory`.
    pub(crate) fn tapped(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Option<Tap>, Error> {
        let Input::Widget {
            instance,
            target,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(None);
        };
        let Some(at) = self.position(instance.get()) else {
            return Ok(None);
        };
        let Some(index) = usize::try_from(*target).ok().and_then(|t| t.checked_sub(1)) else {
            return Ok(None);
        };
        let widget = text::copy(&self.items[at].widget)?;
        let mut cx = Cx::open(host, &widget)?;
        let lines = self.lines(&mut cx, at)?;
        Ok(lines.get(index).map(|line| line.tap.clone()))
    }

    /// Tells every maker its widget may show something else (a setting changed,
    /// the index grew); a refusal is left for the next call.
    pub(crate) fn refresh(&mut self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &self.items {
            // A refusal is the next refresh's.
            commands.widget_changed(item.instance).ok();
        }
    }
}
