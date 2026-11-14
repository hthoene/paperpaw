//! Mail's home widgets on interface 7: *Unread mail*, *Latest mail*, *Starred
//! mail*, *Mailbox* and *Write a mail*. The host makes an instance when a screen
//! shows one (`event.widget_made`), asks it for its height and where it sits
//! through the `core.widget` stage, and calls the plugin to draw it
//! (`DrawPurpose::Widget`, `frame.id` the instance). They read what the card
//! holds of the mail checked last (never the network); nothing of a callback is
//! kept but the instance and what it showed, to tell the host when that has
//! changed. A tap opens the folder, the list of folders, or a new mail.
use crate::native_model::{
    account::Account,
    browser::Words,
    fail::Fail,
    store::{self, Counts, Row},
};
use crate::native_values::SettingsValues;
use crate::native_words::HostWords;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, FrameRect, Input};
use paperpaw_sdk::native::commands::CommandSender;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::reply::Reply;
use paperpaw_sdk::native::snapshot::capture_snapshot;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::ui::{Instance, WidgetInputKind, WidgetTarget, stages::WidgetCall};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Rect, Storage};
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::layout::{caption, caption_height, label};
use paperpaw_ui_kit::widget::settings::{Settings, base};
use paperpaw_ui_kit::{Paint, geometry, mem::buffer, paint, text};

/// The most bytes of a stage reply (a size, or one target).
const REPLY: usize = 256;
/// The target of a tap: the whole widget.
const WHOLE: u32 = 1;
/// The longest folder name a widget setting holds.
const FOLDER_LIMIT: usize = 255;
/// The folder the widgets show unless one is set.
const INBOX: &str = "INBOX";

/// Which widget an instance is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Unread,
    Latest,
    Flagged,
    Folder,
    Write,
}

impl Kind {
    /// The widget `widget` (an id with its placement) is.
    fn of(widget: &str) -> Option<Self> {
        Some(match base(widget) {
            "mail.widget.unread" => Self::Unread,
            "mail.widget.latest" => Self::Latest,
            "mail.widget.flagged" => Self::Flagged,
            "mail.widget.folder" => Self::Folder,
            "mail.widget.write" => Self::Write,
            _ => return None,
        })
    }

    /// The id of the widget, and the key of its caption.
    const fn id(self) -> &'static str {
        match self {
            Self::Unread => "mail.widget.unread",
            Self::Latest => "mail.widget.latest",
            Self::Flagged => "mail.widget.flagged",
            Self::Folder => "mail.widget.folder",
            Self::Write => "mail.widget.write",
        }
    }
}

/// What a tap on a widget does.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Tap {
    /// A new mail.
    Write,
    /// The mails of a folder of an account.
    Folder(u8, String),
    /// The folders.
    Boxes,
}

/// One line of a widget.
#[derive(Debug, Default, PartialEq, Eq)]
struct Line {
    first: String,
    second: String,
    time: String,
}

/// What a widget shows, read from the card.
#[derive(Debug, Default, PartialEq, Eq)]
struct Shown {
    /// A line of large text (a count, "Write a mail").
    headline: String,
    lines: Vec<Line>,
}

/// One widget made for a screen.
#[derive(Debug)]
struct Item {
    instance: Instance,
    kind: Kind,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What the card held of it when it was last asked about.
    shown: Option<u64>,
}

/// The widgets made for screens, by the host's instance.
#[derive(Debug, Default)]
pub(crate) struct Widgets {
    items: Vec<Item>,
}

fn fail(fail: Fail) -> Option<Error> {
    match fail {
        Fail::NoMemory => Some(Error::NoMemory),
        _ => None,
    }
}

/// The accounts a widget's `account` setting means: all, or the one.
fn selected(settings: &Settings<'_, '_>, host: &Host<'_>) -> Result<Vec<u8>, Error> {
    let wanted = settings.int("account", 0)?;
    let mut numbers = Account::numbers(&SettingsValues(host))?;
    numbers.retain(|n| wanted == 0 || i64::from(*n) == wanted);
    Ok(numbers)
}

/// The folder a widget shows: its setting, else the inbox.
fn folder_of(settings: &Settings<'_, '_>) -> Result<String, Error> {
    let raw = match settings.string("folder", FOLDER_LIMIT) {
        Ok(raw) => raw,
        // A name too long to be a folder's is none.
        Err(Error::NoSpace) => String::new(),
        Err(error) => return Err(error),
    };
    let raw = raw.trim();
    text::copy(if raw.is_empty() { INBOX } else { raw })
}

/// The folder a widget of `kind` shows (the inbox for the others).
fn folder_for(kind: Kind, settings: &Settings<'_, '_>) -> Result<String, Error> {
    match kind {
        Kind::Latest | Kind::Folder => folder_of(settings),
        _ => text::copy(INBOX),
    }
}

/// The newest `keep` mails of the selected accounts' folder that `wanted`
/// takes, newest first (a tie goes to the lower account). No more than `keep`
/// are held.
fn newest(
    storage: &dyn Storage,
    accounts: &[u8],
    folder: &str,
    keep: usize,
    wanted: fn(&Row) -> bool,
) -> Result<Vec<(u8, Row)>, Error> {
    let mut all: Vec<(u8, Row)> = Vec::new();
    all.try_reserve(keep.saturating_add(1))
        .map_err(|_| Error::NoMemory)?;
    for &account in accounts {
        let file = store::index_file(account, folder).map_err(|f| fail(f).unwrap_or(Error::Io))?;
        // An index that cannot be read shows no mails (as on interface 6).
        let rows = match store::read_index(storage, &file) {
            Ok(rows) => rows,
            Err(error) => match fail(error) {
                Some(error) => return Err(error),
                None => continue,
            },
        };
        for row in rows.into_iter().filter(wanted) {
            all.try_reserve(1).map_err(|_| Error::NoMemory)?;
            all.push((account, row));
        }
        paperpaw_sdk::sort::sort_by(&mut all, |a, b| {
            b.1.date
                .cmp(&a.1.date)
                .then(b.1.uid.cmp(&a.1.uid))
                .then(a.0.cmp(&b.0))
        });
        all.truncate(keep);
    }
    Ok(all)
}

/// A line of a widget for `row`.
fn line(cx: &mut Cx<'_>, words: &mut HostWords<'_>, row: &Row) -> Result<Line, Error> {
    Ok(Line {
        first: text::copy(&row.from)?,
        second: if row.subject.is_empty() {
            cx.tr("mail.no-subject")?
        } else {
            text::copy(&row.subject)?
        },
        time: words.when(row.date)?,
    })
}

fn one(line: Line) -> Result<Vec<Line>, Error> {
    let mut lines = Vec::new();
    lines.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
    lines.push(line);
    Ok(lines)
}

/// The count of unread mails, as its words say.
fn unread_words(cx: &mut Cx<'_>, unseen: i64) -> Result<String, Error> {
    cx.plural(
        "mail.widget.unread.count",
        unseen,
        &[Arg::Int {
            name: "count",
            value: unseen,
        }],
    )
}

/// *Mailbox*: one account's folder, how many are unread of how many.
fn mailbox(
    cx: &mut Cx<'_>,
    storage: &dyn Storage,
    accounts: &[u8],
    folder: &str,
) -> Result<Shown, Error> {
    let Some(&account) = accounts.first() else {
        return Ok(Shown {
            headline: cx.tr("mail.widget.set-up")?,
            ..Shown::default()
        });
    };
    let file = store::status_file(account, folder).map_err(|f| fail(f).unwrap_or(Error::Io))?;
    let counts = Counts::read(storage, &file);
    let headline = match counts {
        Some(c) => unread_words(cx, i64::from(c.unseen))?,
        None => String::new(),
    };
    let mut lines = Vec::new();
    if let Some(c) = counts.filter(|_| cx.settings.shows("total").unwrap_or(false)) {
        let total = i64::from(c.messages);
        let first = cx.plural(
            "mail.widget.total",
            total,
            &[Arg::Int {
                name: "count",
                value: total,
            }],
        )?;
        lines = one(Line {
            first,
            ..Line::default()
        })?;
    }
    Ok(Shown { headline, lines })
}

/// What the widget of `kind` shows now.
fn read(cx: &mut Cx<'_>, kind: Kind) -> Result<Shown, Error> {
    if kind == Kind::Write {
        return Ok(Shown {
            headline: cx.tr("mail.widget.write.text")?,
            ..Shown::default()
        });
    }
    if Account::numbers(&SettingsValues(cx.host))?.is_empty() {
        return Ok(Shown {
            headline: cx.tr("mail.widget.set-up")?,
            ..Shown::default()
        });
    }
    let accounts = selected(&cx.settings, cx.host)?;
    let storage = DataStorage::new(cx.host);
    let count = usize::try_from(cx.settings.int("count", 3)?.clamp(1, 8)).unwrap_or(3);
    let mut words = HostWords::new(cx.host)?;
    match kind {
        Kind::Unread => {
            let mut unseen = 0i64;
            for &account in &accounts {
                let file =
                    store::status_file(account, INBOX).map_err(|f| fail(f).unwrap_or(Error::Io))?;
                unseen = unseen.saturating_add(
                    Counts::read(&storage, &file).map_or(0, |c| i64::from(c.unseen)),
                );
            }
            let mut lines = Vec::new();
            if cx.settings.shows("newest")?
                && let Some((_, row)) = newest(&storage, &accounts, INBOX, 1, |r| !r.seen())?
                    .into_iter()
                    .next()
            {
                lines = one(line(cx, &mut words, &row)?)?;
            }
            Ok(Shown {
                headline: unread_words(cx, unseen)?,
                lines,
            })
        }
        Kind::Latest | Kind::Flagged => {
            let folder = folder_for(kind, &cx.settings)?;
            let wanted: fn(&Row) -> bool = if kind == Kind::Latest {
                |_| true
            } else {
                Row::flagged
            };
            let rows = newest(&storage, &accounts, &folder, count, wanted)?;
            let mut lines = Vec::new();
            lines
                .try_reserve_exact(rows.len())
                .map_err(|_| Error::NoMemory)?;
            for (_, row) in &rows {
                lines.push(line(cx, &mut words, row)?);
            }
            Ok(Shown {
                lines,
                ..Shown::default()
            })
        }
        Kind::Folder => {
            let folder = folder_of(&cx.settings)?;
            mailbox(cx, &storage, &accounts, &folder)
        }
        Kind::Write => Ok(Shown::default()),
    }
}

/// Whether a second line (the subject, the newest mail's) is shown below the
/// first of each mail.
fn second_line(cx: &Cx<'_>, kind: Kind) -> bool {
    kind == Kind::Unread
        || (matches!(kind, Kind::Latest | Kind::Flagged)
            && cx.settings.shows("subject").unwrap_or(false))
}

/// The room the caption takes, where the widget shows one.
fn caption_room(cx: &Cx<'_>, kind: Kind) -> Result<u16, Error> {
    if kind != Kind::Write && cx.settings.shows("caption")? {
        caption_height(&cx.font, cx.style().small, cx.style().gap)
    } else {
        Ok(0)
    }
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

/// The height the widget takes and the least it can do with.
fn height(cx: &Cx<'_>, kind: Kind, shown: &Shown) -> Result<(u16, u16), Error> {
    let (_, text_h, small_h) = sizes(cx)?;
    let rows_wanted = {
        let headline = if shown.headline.is_empty() { 0 } else { text_h };
        let per = text_h.saturating_add(if second_line(cx, kind) { small_h } else { 0 });
        let items = u16::try_from(shown.lines.len()).unwrap_or(u16::MAX);
        headline.saturating_add(items.saturating_mul(per))
    };
    let empty = if shown.headline.is_empty() && shown.lines.is_empty() {
        geometry::line_px(&cx.font, cx.style().text)?
    } else {
        0
    };
    let frame = cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx, kind)?);
    Ok((
        frame.saturating_add(rows_wanted.max(empty)),
        frame.saturating_add(geometry::line_px(&cx.font, cx.style().text)?),
    ))
}

/// Draws the widget in `area`.
fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    kind: Kind,
    shown: &Shown,
) -> Result<(), Error> {
    // The snapshot does not tell the gray levels: a card is taken to be drawn
    // on a panel with grays.
    let look = cx.shape.paint(out, area, 256)?;
    let align = cx.settings.align()?;
    let small = cx.style().small;
    let mut inner = look.area;
    if kind != Kind::Write && cx.settings.shows("caption")? {
        let title = cx.tr(kind.id())?;
        inner = caption(
            out,
            &cx.font,
            (small, cx.style().gap),
            (inner, align, look.ink),
            &title,
        )?;
    }
    let (size, text_h, small_h) = sizes(cx)?;
    let bottom = i32::from(inner.y) + i32::from(inner.height);
    let mut y = inner.y;
    // One line of `text` `px` large, `h` high, if it fits above the bottom.
    let mut put =
        |cx: &Cx<'_>, out: &mut Vec<Paint>, text: &str, px: f32, h: u16| -> Result<bool, Error> {
            if i32::from(y) + i32::from(h) > bottom {
                return Ok(false);
            }
            let row = Rect::new(inner.x, y, inner.width, h);
            label(out, &cx.font, (px, align), row, text, look.ink)?;
            y = y.saturating_add(h);
            Ok(true)
        };
    if shown.headline.is_empty() && shown.lines.is_empty() {
        let nothing = cx.tr("mail.widget.nothing")?;
        put(
            cx,
            out,
            &nothing,
            cx.style().text,
            geometry::line_px(&cx.font, cx.style().text)?,
        )?;
        return Ok(());
    }
    if !shown.headline.is_empty() {
        put(cx, out, &shown.headline, size, text_h)?;
    }
    let with_second = second_line(cx, kind);
    let with_time = cx.settings.shows("time")?;
    for line in &shown.lines {
        let first = if with_time && !line.time.is_empty() {
            text::concat(&[&line.first, "  ", &line.time])?
        } else {
            text::copy(&line.first)?
        };
        let (px, h) = if shown.headline.is_empty() {
            (size, text_h)
        } else {
            (small, small_h)
        };
        if !put(cx, out, &first, px, h)? {
            break;
        }
        if with_second && !line.second.is_empty() && !put(cx, out, &line.second, small, small_h)? {
            break;
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

/// What the card holds of the widget of `kind`, as one number: the writes of
/// the indexes and the counts it reads, and the day (a mail's time becomes its
/// date at midnight). It changes when what the widget shows does.
fn stamp(host: &Host<'_>, kind: Kind, widget: &str) -> Result<u64, Error> {
    if kind == Kind::Write {
        return Ok(1);
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut mix = |part: u64| hash = (hash ^ part).wrapping_mul(0x0100_0000_01b3);
    let settings = Settings::new(host, widget);
    let accounts = selected(&settings, host)?;
    let folder = folder_for(kind, &settings)?;
    let storage = DataStorage::new(host);
    for &account in &accounts {
        let index =
            store::index_file(account, &folder).map_err(|f| fail(f).unwrap_or(Error::Io))?;
        let status =
            store::status_file(account, &folder).map_err(|f| fail(f).unwrap_or(Error::Io))?;
        mix(u64::from(account));
        mix(store::index_serial(&storage, &index));
        mix(storage.size(&index).unwrap_or(0));
        if let Some(c) = Counts::read(&storage, &status) {
            mix(u64::from(c.messages));
            mix(u64::from(c.unseen));
            mix(c.checked.unsigned_abs());
        }
    }
    let offset = capture_snapshot(host)?;
    let day = offset.unix_seconds().map_or(0, |unix| {
        unix.saturating_add(offset.utc_offset_minutes().map_or(0, i64::from) * 60)
            .div_euclid(86_400)
    });
    mix(day.unsigned_abs());
    // Never 0 (which is "nothing shown").
    Ok(hash | 1)
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
        let Some(kind) = Kind::of(widget) else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            kind,
            widget: text::copy(widget)?,
            shown: None,
        });
        Ok(())
    }

    /// The screen dropped the instance.
    pub(crate) fn dropped(&mut self, instance: Instance) {
        self.items.retain(|item| item.instance != instance);
    }

    /// Answers a call of the stage `core.widget`: the height, the place kept,
    /// the one target (the whole widget).
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
                let mut cx = Cx::open(host, &item.widget)?;
                let shown = read(&mut cx, item.kind)?;
                let (natural, least) = height(&cx, item.kind, &shown)?;
                call.reply_size(u32::from(natural), u32::from(least), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_targets(&[WidgetTarget::new(WHOLE, rect)], &mut out)?
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
        let shown = read(&mut cx, item.kind)?;
        let mut out = Vec::new();
        draw(&mut cx, &mut out, area_of(frame.rect), item.kind, &shown)?;
        paint::replay_icons(&mut frame, &cx.font, None, &out)
    }

    /// What `input` is, if a tap on one of the widgets.
    ///
    /// # Errors
    /// Host refusals and `NoMemory`.
    pub(crate) fn tapped(&self, host: &Host<'_>, input: &Input<'_>) -> Result<Option<Tap>, Error> {
        let Input::Widget {
            instance,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(None);
        };
        let Some(item) = self.find(instance.get()) else {
            return Ok(None);
        };
        if item.kind == Kind::Write {
            return Ok(Some(Tap::Write));
        }
        let settings = Settings::new(host, &item.widget);
        if Account::numbers(&SettingsValues(host))?.is_empty() {
            return Ok(Some(Tap::Boxes));
        }
        let accounts = selected(&settings, host)?;
        let Some(&first) = accounts.first() else {
            return Ok(Some(Tap::Boxes));
        };
        Ok(Some(match item.kind {
            // A folder of one account, where one is meant.
            Kind::Unread | Kind::Latest if accounts.len() == 1 => {
                Tap::Folder(first, folder_for(item.kind, &settings)?)
            }
            Kind::Folder => Tap::Folder(first, folder_for(item.kind, &settings)?),
            _ => Tap::Boxes,
        }))
    }

    /// Tells the makers whose widget shows something new since it was last
    /// asked: the card holds more mail, or the day changed. One that fails is
    /// left for the next tick and costs the others nothing.
    pub(crate) fn tick(&mut self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let Ok(now) = stamp(host, item.kind, &item.widget) else {
                continue;
            };
            let instance = item.instance;
            if item.shown.is_some_and(|before| before != now)
                && commands.widget_changed(instance).is_err()
            {
                continue;
            }
            item.shown = Some(now);
        }
    }

    /// Tells every maker its widget may show something else (a setting changed,
    /// a check ended); a refusal is left for the next tick.
    pub(crate) fn refresh(&mut self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &self.items {
            // A refusal is the next tick's.
            commands.widget_changed(item.instance).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    fn row(uid: u32, date: i64) -> Row {
        Row {
            uid,
            date,
            ..Row::default()
        }
    }

    fn keep_index(storage: &MemoryStorage, account: u8, rows: &[Row]) {
        let file = store::index_file(account, "INBOX").unwrap();
        store::write_index(storage, &file, rows).unwrap();
    }

    fn any(_: &Row) -> bool {
        true
    }

    #[test]
    fn the_newest_of_two_accounts_are_kept_not_the_oldest() {
        let storage = MemoryStorage::new();
        // Account 0 holds the older mails, account 1 the newest ones.
        keep_index(&storage, 0, &[row(3, 300), row(2, 200), row(1, 100)]);
        keep_index(&storage, 1, &[row(30, 3000), row(20, 2000), row(10, 1000)]);
        let got = newest(&storage, &[0, 1], "INBOX", 3, any).unwrap();
        let seen: Vec<(u8, u32)> = got.iter().map(|(a, r)| (*a, r.uid)).collect();
        assert_eq!(seen, [(1, 30), (1, 20), (1, 10)]);
        // One more than the second account holds: the first account's newest follows.
        let got = newest(&storage, &[0, 1], "INBOX", 4, any).unwrap();
        let seen: Vec<(u8, u32)> = got.iter().map(|(a, r)| (*a, r.uid)).collect();
        assert_eq!(seen, [(1, 30), (1, 20), (1, 10), (0, 3)]);
    }

    #[test]
    fn a_tie_goes_to_the_lower_account() {
        let storage = MemoryStorage::new();
        keep_index(&storage, 0, &[row(5, 500)]);
        keep_index(&storage, 1, &[row(5, 500)]);
        let got = newest(&storage, &[1, 0], "INBOX", 2, any).unwrap();
        let seen: Vec<u8> = got.iter().map(|(a, _)| *a).collect();
        assert_eq!(seen, [0, 1]);
    }
}
