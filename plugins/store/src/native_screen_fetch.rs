//! The `fetch` screen of the store (slice 7): what the reader sees while the
//! plugins another plugin asked for are fetched (the packs of letters a book
//! needs, say), while *Update all* goes through its queue, or while the list is
//! fetched. As the interface-6 screen of the requests (`fetch.rs`) showed it:
//! the names, a bar with the percent and the bytes so far, and how it ends:
//!
//! - all is here: the screen closes (the one who asked is told by the queue's
//!   end) and *Ready* is said (a request);
//! - a pack that has to wait for a restart before it works: *Restart now* or
//!   *Later*;
//! - something did not come (the connection went, no room): what it was, *Try
//!   again* or *Close*.
//!
//! The screen may be left (*Back*): the download goes on.
//!
//! It is opened with the parameters [`BY`] (who asked) and [`IDS`] (the plugins,
//! a line each; see [`open_next`]) for a request, with none while the queue is
//! at work (*Update all*) and with none while the list is fetched. What it
//! shows is read from the installs ([`Installs::step`], [`Installs::queued`])
//! and the loader ([`crate::native_screens::Loader::list_state`]), counted on every event: no timer.
//! The plugin queues the plugins **before** it opens the screen of a request
//! (an empty queue with nothing installed is a download that did not come).
//!
//! Interface 7 differences, on purpose: the end is read from the installs' steps
//! and the queue, not from the answer event to the one who asked (that goes to
//! the asker only); the bytes come from the downloads' steps, not from the sizes
//! of the files in the download's folder.
//!
//! The rules (what is counted, how it ends, what a row does) are in [`Core`],
//! which reads nothing of the host and is tested alone; [`View`] lays out,
//! draws and takes the input.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Handled;
use paperpaw_sdk::Storage;
use paperpaw_sdk::install::is_plugin_name;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
    events::Event,
    fonts::{Font, FontRole},
    formatting::{self, Arg},
    snapshot,
};
use paperpaw_sdk::{Point, Rect};
use paperpaw_ui_kit::{
    Messages, Page, Paint, Style, geometry, header, list,
    mem::push,
    paint, text,
    text::{concat, copy, decimal_u64},
};

use crate::native_ask::{Say, listing, names_up_to, shown_size};
use crate::native_model::Entry;
use crate::native_queue::{Queue, join_names};
use crate::native_request::MAX_WANTED;
use crate::native_screens::{Ctx, Installs, Next, Screen, Step, Trouble, ids};

/// The parameter with who asked (a plugin's id), as the interface-6 screen's.
pub const BY: &str = "id";
/// The parameter with the plugins asked for, a line each.
pub const IDS: &str = "text";
/// Plugins of the queue the screen counts at most.
const MOST_QUEUED: usize = 64;
/// Names said at most (more end in `…`).
const MOST_NAMES: usize = 8;
/// Lines a message wraps to at most.
const MOST_LINES: usize = 8;

/// The parameters of the screen that shows the download of `ids`, which `by`
/// asked for.
///
/// # Errors
/// `NoMemory`.
pub fn open_next(by: &str, ids: &[String]) -> Result<Next, Error> {
    let joined = join_names(ids.iter().map(String::as_str), "\n")?;
    let mut params = Vec::new();
    push(&mut params, (BY, copy(by)?))?;
    push(&mut params, (IDS, joined))?;
    Ok(Next::Open {
        screen: ids::FETCH,
        params,
    })
}

/// What the screen shows the fetch of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// The plugins another plugin asked for.
    Request,
    /// *Update all*: the plugins of the queue.
    All,
    /// The list of plugins.
    List,
}

/// Why a fetch did not come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// Plugins did not come (the connection went, no room).
    Missing,
    /// There is no network.
    Offline,
    /// The library did not answer, or what came is no list.
    Unreachable,
}

/// How far it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    /// Fetching.
    Working,
    /// All here; the screen closes.
    Done,
    /// What did not come.
    Failed(Why),
    /// Installed, and works after a restart.
    Restart,
}

/// What a row of the end does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Row {
    /// Fetch again.
    Retry,
    /// Close the screen.
    Close,
    /// Restart now.
    RestartNow,
    /// Restart later: close the screen.
    Later,
}

impl Row {
    /// The text of the row.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Row::Retry => "store.fetch.retry",
            Row::Close => "store.fetch.close",
            Row::RestartNow => "store.restart",
            Row::Later => "store.fetch.later",
        }
    }
}

/// The rows of a phase.
#[must_use]
pub const fn rows(phase: Phase) -> &'static [Row] {
    match phase {
        Phase::Working | Phase::Done => &[],
        Phase::Failed(_) => &[Row::Retry, Row::Close],
        Phase::Restart => &[Row::RestartNow, Row::Later],
    }
}

/// What the installs say of the plugins the screen counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tally {
    /// Bytes here: a plugin installed whole, one downloading as far as it is.
    pub done: u64,
    /// Plugins installed.
    pub whole: usize,
    /// One is downloading or being installed.
    pub busy: bool,
    /// One ended in a reason.
    pub failed: bool,
    /// One is not installed and nothing is going on for it.
    pub missing: bool,
    /// One is installed and works after a restart.
    pub restart: bool,
}

/// Counts the plugins `ids` (the library lists `sizes` of them, a byte count each)
/// by the installs' steps. A plugin installed counts whole, the one downloading
/// by the bytes so far, never more than its size.
#[must_use]
pub fn tally(ids: &[String], sizes: &[u64], installs: &dyn Installs) -> Tally {
    let mut tally = Tally::default();
    for (id, size) in ids.iter().zip(sizes) {
        match installs.step(id) {
            Step::Installed => {
                tally.done = tally.done.saturating_add(*size);
                tally.whole = tally.whole.saturating_add(1);
                tally.restart |= installs.restart_needed(id);
            }
            Step::Downloading { done, .. } => {
                tally.busy = true;
                tally.done = tally.done.saturating_add(u64::from(done).min(*size));
            }
            Step::Installing => {
                tally.busy = true;
                tally.done = tally.done.saturating_add(*size);
            }
            Step::Failed(_) => tally.failed = true,
            Step::Idle => tally.missing = true,
        }
    }
    tally
}

/// The permille shown (never whole before the end: 990 at most) and the bytes
/// done, of `count` plugins of `total` bytes together. Where the sizes are not
/// known the plugins installed are the share.
#[must_use]
pub fn progress(tally: &Tally, total: u64, count: usize) -> (u16, u64) {
    let whole = tally.whole as u64 * 1000;
    let permille = tally
        .done
        .min(total)
        .saturating_mul(1000)
        .checked_div(total)
        .unwrap_or_else(|| whole / (count.max(1) as u64));
    (u16::try_from(permille.min(990)).unwrap_or(990), tally.done)
}

/// How a fetch stands: the list by the loader's word; plugins by the installs
/// (the queue still holds plugins, or one is going on: working; else all are
/// installed, or some are not). *Update all* ends well where none ended in a
/// reason (a plugin set aside is the queue's to say).
#[must_use]
pub fn settle(kind: Kind, tally: &Tally, queued: bool, list: Trouble) -> Phase {
    if kind == Kind::List {
        return match list {
            Trouble::Loading => Phase::Working,
            Trouble::None => Phase::Done,
            Trouble::Offline => Phase::Failed(Why::Offline),
            Trouble::Failed | Trouble::Unsupported => Phase::Failed(Why::Unreachable),
        };
    }
    if queued || tally.busy {
        return Phase::Working;
    }
    let ok = !tally.failed && (kind == Kind::All || !tally.missing);
    if !ok {
        Phase::Failed(Why::Missing)
    } else if tally.restart {
        Phase::Restart
    } else {
        Phase::Done
    }
}

/// What a row does. Fetching again is what the screen was opened for.
///
/// # Errors
/// `NoMemory`.
pub fn activate(kind: Kind, by: &str, ids: &[String], row: Row) -> Result<Next, Error> {
    Ok(match row {
        Row::Retry => match kind {
            Kind::Request => Next::Again {
                by: copy(by)?,
                ids: paperpaw_ui_kit::mem::copy_strings(ids)?,
            },
            Kind::All => Next::UpdateAll,
            Kind::List => Next::FetchList,
        },
        Row::RestartNow => Next::Restart,
        Row::Close | Row::Later => Next::Close,
    })
}

/// The notice at the end of a request: the reader's own book has its words.
#[must_use]
pub fn ready_key(by: &str) -> &'static str {
    if by == "reader" {
        "store.fetch.ready.reader"
    } else {
        "store.fetch.ready"
    }
}

/// The row the focus goes to from `focus` by `delta` (one step, round the
/// ends) of `len` rows.
#[must_use]
pub fn step_focus(focus: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let at = focus.min(len - 1);
    if delta < 0 {
        at.checked_sub(1).unwrap_or(len - 1)
    } else if at + 1 >= len {
        0
    } else {
        at + 1
    }
}

/// The plugins a request names in `text` (a line each), as the screen takes them:
/// names of plugins, each once, at most [`MAX_WANTED`].
///
/// # Errors
/// `NoMemory`.
pub fn request_ids(text: &str) -> Result<Vec<String>, Error> {
    let mut ids: Vec<String> = Vec::new();
    for line in text.lines().filter(|line| is_plugin_name(line)) {
        if ids.len() >= MAX_WANTED {
            break;
        }
        if !ids.iter().any(|id| id == line) {
            push(&mut ids, copy(line)?)?;
        }
    }
    Ok(ids)
}

/// What the screen is about and how far it is, without the host.
#[derive(Debug)]
#[non_exhaustive]
pub struct Core {
    pub kind: Kind,
    /// Who asked (a plugin's id; the store's own where the parameter is none).
    pub by: String,
    pub ids: Vec<String>,
    sizes: Vec<u64>,
    total: u64,
    /// Their names, joined (the first few).
    pub names: String,
    pub phase: Phase,
    pub permille: u16,
    pub done: u64,
    pub focus: usize,
    /// The screen is on top.
    top: bool,
}

impl Core {
    /// A screen for `kind` of the plugins `ids`, as the library lists them
    /// (`listed`, see [`listing`]; none listed: the ids stand for themselves).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(
        kind: Kind,
        by: &str,
        ids: Vec<String>,
        listed: &[(String, Option<Entry>)],
    ) -> Result<Self, Error> {
        let mut sizes = Vec::new();
        sizes
            .try_reserve_exact(ids.len())
            .map_err(|_| Error::NoMemory)?;
        for id in &ids {
            let size = listed
                .iter()
                .find(|(listed, _)| listed == id)
                .and_then(|(_, entry)| entry.as_ref())
                .map_or(0, |entry| entry.size);
            sizes.push(size);
        }
        let total = sizes
            .iter()
            .fold(0u64, |all, size| all.saturating_add(*size));
        let names = if listed.is_empty() {
            join_names(ids.iter().take(MOST_NAMES).map(String::as_str), ", ")?
        } else {
            names_up_to(listed, MOST_NAMES)?
        };
        Ok(Self {
            kind,
            by: copy(by)?,
            ids,
            sizes,
            total,
            names,
            phase: Phase::Working,
            permille: 0,
            done: 0,
            focus: 0,
            top: true,
        })
    }

    /// Bytes of what is fetched, all together (0 where the library does not say).
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// Counts again and tells what to do: a redraw where the bar or the bytes
    /// moved or the end came; the end of a request says *Ready* and closes (said
    /// at once and closed on [`resume`](Self::resume) where the screen is covered).
    pub fn refresh(&mut self, installs: &dyn Installs, list: Trouble) -> Next {
        if self.phase == Phase::Done {
            return if self.top { Next::Close } else { Next::None };
        }
        if self.phase != Phase::Working {
            return Next::None;
        }
        let tally = tally(&self.ids, &self.sizes, installs);
        let phase = settle(self.kind, &tally, installs.queued(), list);
        let (permille, done) = progress(&tally, self.total, self.ids.len());
        let moved = permille != self.permille || done != self.done;
        self.permille = permille;
        self.done = done;
        match phase {
            Phase::Working if moved => Next::Redraw,
            Phase::Working => Next::None,
            Phase::Done => self.finish(),
            ended => {
                if ended == Phase::Restart {
                    self.permille = 1000;
                }
                self.phase = ended;
                self.focus = 0;
                Next::Redraw
            }
        }
    }

    fn finish(&mut self) -> Next {
        self.phase = Phase::Done;
        let ready = (self.kind == Kind::Request).then(|| ready_key(&self.by));
        match (self.top, ready) {
            (true, Some(key)) => Next::CloseNotify(key),
            (true, None) => Next::Close,
            (false, Some(key)) => Next::Notify(key),
            (false, None) => Next::None,
        }
    }

    /// The screen is on top again.
    pub fn resume(&mut self, installs: &dyn Installs, list: Trouble) -> Next {
        self.top = true;
        self.refresh(installs, list)
    }

    /// Another screen covers this one.
    pub fn hide(&mut self) {
        self.top = false;
    }

    /// What the row `index` does; fetching again starts the count anew.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn choose(&mut self, index: usize) -> Result<Next, Error> {
        let Some(row) = rows(self.phase).get(index).copied() else {
            return Ok(Next::None);
        };
        let next = activate(self.kind, &self.by, &self.ids, row)?;
        if row == Row::Retry {
            self.phase = Phase::Working;
            self.permille = 0;
            self.done = 0;
            self.focus = 0;
        }
        Ok(next)
    }

    /// An action: the focus moves, or the row in focus is chosen.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn action(&mut self, action: &str) -> Result<(Handled, Next), Error> {
        let len = rows(self.phase).len();
        if len == 0 {
            return Ok((Handled::No, Next::None));
        }
        let delta = match action {
            "core.up" | "core.left" | "core.previous" => -1,
            "core.down" | "core.right" | "core.next" => 1,
            "core.confirm" => return Ok((Handled::Yes, self.choose(self.focus)?)),
            _ => return Ok((Handled::No, Next::None)),
        };
        self.focus = step_focus(self.focus, len, delta);
        Ok((Handled::Yes, Next::Redraw))
    }

    /// The message below the header: the names, or what did not come.
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    pub fn message(&self, say: &mut dyn Say) -> Result<String, Error> {
        let names = Arg::Text {
            name: "names",
            value: &self.names,
        };
        match self.phase {
            Phase::Failed(Why::Missing) => say.format("store.request.failed", &[names]),
            Phase::Failed(Why::Offline) => say.text("store.offline"),
            Phase::Failed(Why::Unreachable) => say.text("store.unreachable"),
            Phase::Restart => say.format("store.fetch.restart.text", &[names]),
            Phase::Working | Phase::Done if self.kind == Kind::List => say.text("store.loading"),
            Phase::Working | Phase::Done => copy(&self.names),
        }
    }

    /// The line under the bar: the percent, and the bytes where they are known.
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    pub fn progress_text(&self, say: &mut dyn Say) -> Result<String, Error> {
        let mut digits = [0; 20];
        let percent = concat(&[
            decimal_u64(u64::from(self.permille / 10), &mut digits),
            " %",
        ])?;
        let percent = say.digits(&percent)?;
        if self.total == 0 {
            return Ok(percent);
        }
        let done = shown_size(say, self.done.min(self.total))?;
        let total = shown_size(say, self.total)?;
        say.format(
            "store.fetch.progress",
            &[
                Arg::Text {
                    name: "percent",
                    value: &percent,
                },
                Arg::Text {
                    name: "done",
                    value: &done,
                },
                Arg::Text {
                    name: "total",
                    value: &total,
                },
            ],
        )
    }

    /// Whether the bar and its line are shown (not for a list: its size is not
    /// known, and where the end rows are the bar has had its day).
    #[must_use]
    pub fn bar(&self) -> bool {
        self.kind != Kind::List && rows(self.phase).is_empty()
    }
}

/// Where the parts of the screen go, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Layout {
    /// The names, or what did not come.
    pub message: Rect,
    /// The bar.
    pub bar: Rect,
    /// The first row of the end.
    pub rows_top: u16,
    /// The hint at the bottom.
    pub hint: Rect,
}

/// The numbers the layout is made of.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub width: u16,
    pub height: u16,
    pub header: u16,
    pub margin: u16,
    pub gap: u16,
    pub row: u16,
    /// Height the message needs, and the hint.
    pub message_needs: u16,
    pub hint_needs: u16,
}

impl Layout {
    /// The message below the header (two rows, or the lines it needs at a large
    /// text), the bar with room to spare below it, or close below where the
    /// lines of the hint at a large text leave no room for the percent under it,
    /// and the rows of the end under the bar.
    #[must_use]
    pub fn of(m: &Metrics) -> Self {
        let wide = m.width.saturating_sub(m.margin.saturating_mul(2));
        let top = m.header.saturating_add(m.gap);
        let message = Rect::new(
            m.margin,
            top,
            wide,
            m.message_needs.max(m.row.saturating_mul(2)),
        );
        let hint_height = m.hint_needs.max(m.row.saturating_mul(3) / 2);
        let hint = Rect::new(
            0,
            m.height.saturating_sub(hint_height),
            m.width,
            hint_height,
        );
        let spare = m.header.saturating_add(m.row.saturating_mul(5) / 2).max(
            message
                .y
                .saturating_add(message.height)
                .saturating_add(m.gap),
        );
        let bar_height = m.row / 2;
        let room = spare
            .saturating_add(bar_height)
            .saturating_add(m.gap)
            .saturating_add(m.row);
        let y = if room <= hint.y {
            spare
        } else {
            top.saturating_add(m.message_needs).saturating_add(m.gap)
        };
        let bar = Rect::new(m.margin, y, wide, bar_height);
        Self {
            message,
            bar,
            rows_top: bar.y.saturating_add(m.row.saturating_mul(2)),
            hint,
        }
    }
}

/// The plugins of the queue the screen follows (*Update all*).
fn queued_ids(data: &dyn Storage) -> Result<Vec<String>, Error> {
    let queue = Queue::read(data)?;
    let mut ids = Vec::new();
    for line in queue.lines() {
        if ids.len() >= MOST_QUEUED {
            break;
        }
        if is_plugin_name(&line.id) {
            push(&mut ids, copy(&line.id)?)?;
        }
    }
    Ok(ids)
}

/// The height the lines of `text` need in `width`, in the small size.
fn needs(font: &Font<'_>, style: &Style, width: u16, text: &str) -> Result<u16, Error> {
    let room = f32::from(width.saturating_sub(style.margin.saturating_mul(4))).max(1.0);
    let lines = text::wrap_with(
        |l| font.measure_text(style.small, l),
        text,
        room,
        MOST_LINES,
    )?;
    let line = geometry::line_px(font, style.small)?;
    Ok(u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_mul(line))
}

/// The bar: an outline, and the share done inside it.
fn bar(body: &mut Vec<Paint>, style: &Style, bar: Rect, permille: u16) -> Result<(), Error> {
    let edge = style.line.max(1);
    let inner = bar.inset(edge, edge);
    let done = u32::from(inner.width) * u32::from(permille.min(1000)) / 1000;
    let done = u16::try_from(done).unwrap_or(inner.width);
    paint::push(body, Paint::Fill(bar, style.ink))?;
    paint::push(body, Paint::Fill(inner, style.paper))?;
    paint::push(
        body,
        Paint::Fill(Rect::new(inner.x, inner.y, done, inner.height), style.ink),
    )
}

/// The host's catalog said through stack buffers: nothing is held between
/// frames and a frame makes no 4 KiB buffer. A text or an argument that does not
/// fit them (a long list of names) is said through a `Messages` made for that
/// one text and dropped at once.
struct Held<'h, 'a> {
    host: &'h Host<'a>,
}

/// The stack buffers of one text: the message, and the arguments' encoding.
const OUT: usize = 512;
const ARGS: usize = 256;

fn owned(bytes: &[u8]) -> Result<String, Error> {
    copy(core::str::from_utf8(bytes).map_err(|_| Error::Utf8)?)
}

impl Say for Held<'_, '_> {
    fn lookup(&mut self, key: &str) -> Result<Option<String>, Error> {
        let mut out = [0; OUT];
        match formatting::copy_message(self.host, key, &mut out) {
            Ok(found) => Ok(Some(copy(found)?)),
            Err(Error::NotFound | Error::Invalid) => Ok(None),
            Err(Error::NoSpace) => match Messages::default().copy(self.host, key) {
                Ok(text) => Ok(Some(text)),
                Err(Error::NotFound | Error::Invalid) => Ok(None),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        }
    }

    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let (mut scratch, mut out) = ([0; ARGS], [0; OUT]);
        let mut buffers = formatting::FormatBuffers::new(&mut scratch, &mut out);
        match formatting::format_message(self.host, key, args, &mut buffers) {
            Ok(length) => owned(out.get(..length).ok_or(Error::NoSpace)?),
            Err(Error::NoSpace) => Messages::default().format(self.host, key, args),
            Err(error) => Err(error),
        }
    }

    fn digits(&mut self, text: &str) -> Result<String, Error> {
        let mut out = [0; 64];
        match formatting::digits(self.host, text, &mut out) {
            Ok(length) => owned(out.get(..length).ok_or(Error::NoSpace)?),
            Err(Error::NoSpace) => Messages::default().digits(self.host, text),
            Err(error) => Err(error),
        }
    }
}

/// The screen.
#[derive(Debug)]
pub struct View {
    core: Core,
    /// The header's back arrow of the last frame (touch devices).
    back: Option<Rect>,
    /// The rows of the end of the last frame.
    hits: Vec<Rect>,
}

impl View {
    /// What is going on, without the host.
    #[must_use]
    pub const fn core(&self) -> &Core {
        &self.core
    }

    fn tap(&mut self, position: Point) -> Result<(Handled, Next), Error> {
        if self.back.is_some_and(|back| back.contains(position)) {
            return Ok((Handled::Yes, Next::Close));
        }
        let Some(index) = self.hits.iter().position(|rect| rect.contains(position)) else {
            return Ok((Handled::No, Next::None));
        };
        self.core.focus = index;
        Ok((Handled::Yes, self.core.choose(index)?))
    }

    /// The parts of the frame: the header, then the body.
    fn paint(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let host: &Host<'_> = ctx.host;
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let mut say = Held { host };
        let title = say.text("store.fetch.title")?;
        let mut page = Page::default();
        header::prepare(
            &mut page,
            host,
            &font,
            &style,
            // One title with no pager: the header asks the buffers for nothing.
            &mut Messages::default(),
            &title,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        let (message, hint) = (self.core.message(&mut say)?, say.text("store.fetch.hint")?);
        let wide = width.saturating_sub(style.margin.saturating_mul(2));
        let layout = Layout::of(&Metrics {
            width,
            height,
            header: style.header,
            margin: style.margin,
            gap: style.gap,
            row: style.row,
            message_needs: needs(&font, &style, wide, &message)?,
            hint_needs: needs(&font, &style, width, &hint)?,
        });
        let mut body = Vec::new();
        let below = Rect::new(0, style.header, width, height.saturating_sub(style.header));
        paint::push(&mut body, Paint::Fill(below, style.paper))?;
        text::message(&mut body, &font, &style, layout.message, &message)?;
        let mut hits = Vec::new();
        if self.core.bar() {
            bar(&mut body, &style, layout.bar, self.core.permille)?;
            let line = Rect::new(
                style.margin,
                layout
                    .bar
                    .y
                    .saturating_add(layout.bar.height)
                    .saturating_add(style.gap),
                wide,
                style.row,
            );
            let progress = self.core.progress_text(&mut say)?;
            text::message(&mut body, &font, &style, line, &progress)?;
        }
        let mut top = layout.rows_top;
        for (index, row) in rows(self.core.phase).iter().enumerate() {
            let rect = Rect::new(0, top, width, style.row);
            let label = say.text(row.key())?;
            let focused = index == self.core.focus;
            list::row(&mut body, &font, &style, rect, &label, "", focused)?;
            push(&mut hits, rect)?;
            top = top.saturating_add(style.row);
        }
        // The hint keeps all its lines, from the bottom up, under a light line.
        let rule = Rect::new(0, layout.hint.y, width, style.line.max(1));
        paint::push(&mut body, Paint::Fill(rule, style.separator_ink))?;
        text::message(&mut body, &font, &style, layout.hint, &hint)?;
        paint::replay(frame, &font, page.paint.iter().chain(body.iter()))?;
        font.release()?;
        // Only the tap region survives the draw.
        self.back = page.back;
        self.hits = hits;
        Ok(())
    }
}

impl Screen for View {
    fn open(ctx: &Ctx<'_>, params: &[(&str, &str)]) -> Result<Self, Error> {
        let (mut by, mut asked) = ("", "");
        for (name, value) in params {
            match *name {
                BY => by = value,
                IDS => asked = value,
                _ => {}
            }
        }
        let by = if is_plugin_name(by) { by } else { "store" };
        let mut ids = request_ids(asked)?;
        let mut kind = Kind::Request;
        if ids.is_empty() {
            kind = if ctx.installs.queued() {
                Kind::All
            } else {
                Kind::List
            };
            if kind == Kind::All {
                ids = queued_ids(ctx.data)?;
            }
            if ids.is_empty() {
                kind = Kind::List;
            }
        }
        let listed = listing(ctx.data, ctx.loader.index(), &ids)?;
        let mut core = Core::new(kind, by, ids, &listed)?;
        // What is here already; the end, if it came, is taken on the first event.
        drop(core.refresh(ctx.installs, ctx.loader.list_state()));
        Ok(Self {
            core,
            back: None,
            hits: Vec::new(),
        })
    }

    fn resume(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        Ok(self.core.resume(ctx.installs, ctx.loader.list_state()))
    }

    fn hidden(&mut self) {
        self.core.hide();
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        self.paint(ctx, frame)
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        if self.core.phase == Phase::Done {
            let next = self.core.refresh(ctx.installs, ctx.loader.list_state());
            return Ok((Handled::Yes, next));
        }
        match *input {
            Input::Action { action, .. } => self.core.action(action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(position),
            _ => Ok((Handled::No, Next::None)),
        }
    }

    fn event(&mut self, ctx: &Ctx<'_>, _event: &Event<'_>) -> Result<Next, Error> {
        Ok(self.core.refresh(ctx.installs, ctx.loader.list_state()))
    }
}
