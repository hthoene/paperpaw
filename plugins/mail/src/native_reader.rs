//! The reader of a mail (`mail.read`): its headers and text, fetched the first
//! time it is opened and kept on the card; *Reply*, *Reply all* and *Star*
//! below. Drawn here, from owned paint: the header of the screen, the lines of
//! the text from the line the reader is at, a status line and the buttons.
use crate::declarations::SCREEN_TOKEN;
use crate::native_model::fail::Fail;
use crate::native_model::mem::copy;
use crate::native_model::reader::{self, Layout, Step};
use crate::native_model::store::{self, Letter};
use crate::native_model::texts::{self, Say};
use crate::native_plugin::NativeMail;
use crate::native_values::HostValues;
use crate::native_words::HostWords;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot,
        storage::DataStorage,
        ui::{HandoverId, Instance},
    },
};
use paperpaw_ui_kit::{
    Messages, Page, Paint, Style,
    mem::{SERVICE_BUFFER, buffer, push},
    paint, text,
};

/// The token of the reader.
pub(crate) const READ_TOKEN: u32 = SCREEN_TOKEN + 2;
/// The most lines a mail is wrapped to.
const MAX_LINES: usize = 1024;
/// The buttons, by their catalog keys.
const BUTTONS: [&str; 3] = ["mail.reply", "mail.reply-all", "mail.star"];

/// One open reader.
#[derive(Debug)]
pub(crate) struct Reading {
    pub(crate) account: u8,
    pub(crate) folder: String,
    pub(crate) uid: u32,
    /// The subject, for the header; empty before the mail is there.
    title: String,
    /// The headers and the text; none before the mail is there.
    shown: Option<String>,
    /// The text wrapped to the width it was last drawn at.
    lines: Vec<String>,
    wrapped_at: u16,
    /// The first line drawn.
    top: usize,
    /// How many lines the last frame had room for.
    rows: usize,
    /// Which button has the focus; none while the text is scrolled.
    focus: Option<usize>,
    /// What the screen says above the buttons: how it went.
    pub(crate) status: String,
    starred: bool,
    /// The fetch was started (once, when the screen first shows).
    started: bool,
    /// Where the last frame put the header's taps and the buttons.
    header: Page,
    buttons: [Rect; 3],
    area: Rect,
}

impl Reading {
    fn new(account: u8, folder: String, uid: u32) -> Self {
        Self {
            account,
            folder,
            uid,
            title: String::new(),
            shown: None,
            lines: Vec::new(),
            wrapped_at: 0,
            top: 0,
            rows: 1,
            focus: None,
            status: String::new(),
            starred: false,
            started: false,
            header: Page::default(),
            buttons: [Rect::new(0, 0, 0, 0); 3],
            area: Rect::new(0, 0, 0, 0),
        }
    }
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

fn fail_error(fail: Fail) -> Error {
    match fail {
        Fail::NoMemory => Error::NoMemory,
        _ => Error::Io,
    }
}

impl NativeMail {
    /// A reader opened: the account, the folder and the mail come in the
    /// handover; the mail is read from the card if it is there.
    pub(crate) fn reader_opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let handover = handover.ok_or(Error::Invalid)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let (mut account, mut folder, mut uid) = (None, None, None);
        for param in self.ui()?.params(host, handover, &mut input, &mut out)? {
            let param = param?;
            match param.key() {
                "account" => account = param.value().parse::<u8>().ok(),
                "folder" => folder = Some(copy(param.value())?),
                "uid" => uid = param.value().parse::<u32>().ok(),
                _ => {}
            }
        }
        let (Some(account), Some(folder), Some(uid)) = (account, folder, uid) else {
            return Err(Error::Invalid);
        };
        let mut reading = Reading::new(account, folder, uid);
        read_kept(host, &mut reading)?;
        self.readers.open(instance, reading)
    }

    /// A reader came on top: the mail is fetched when it is not on the card
    /// yet, once; the star is read again (a mail written above may have been
    /// starred).
    pub(crate) fn reader_shown(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        self.readers.shown(READ_TOKEN, instance)?;
        let storage = DataStorage::new(host);
        let Some(reading) = self.readers.get(instance) else {
            return Err(Error::State);
        };
        reading.starred = reader::starred(&storage, reading.account, &reading.folder, reading.uid)
            .map_err(fail_error)?;
        let fetch = reading.shown.is_none() && !reading.started;
        reading.started = true;
        if fetch {
            self.fetch(host, instance)?;
        }
        redraw(host)
    }

    /// A reader closed: its connection goes with it.
    pub(crate) fn reader_closed(&mut self, host: &Host<'_>, instance: Instance) {
        if self.check.as_ref().is_some_and(|c| c.owner == instance) {
            self.stop(host);
        }
        self.readers.close(instance);
    }

    /// Fetches the mail (and marks it read on the server, if the settings say
    /// so).
    fn fetch(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let reading = self.readers.get(instance).ok_or(Error::State)?;
        let (account, folder, uid) = (reading.account, copy(&reading.folder)?, reading.uid);
        self.services(host)?;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        let mark = {
            use crate::native_model::account::Values;
            // The setting is a switch: 1 where it is on.
            HostValues { host, secrets }
                .int("mail.mark-read")
                .map(|v| v != 0)?
        };
        let tasks = reader::fetch_tasks(account, &folder, uid, mark).map_err(fail_error)?;
        self.begin(host, instance, account, tasks)
    }

    /// Starts `tasks` on `account` for the reader `instance`, which then owns
    /// the connection.
    fn begin(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        account: u8,
        tasks: Vec<crate::native_model::session::Task>,
    ) -> Result<(), Error> {
        let mut queue = Vec::new();
        queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
        queue.push(account);
        self.check = Some(crate::native_screens::Check::new(instance, queue));
        match self.start_imap(host, account, tasks) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.check = None;
                self.say_reader(host, instance, start_failure(error))
            }
        }
    }

    /// Sets what the reader says above the buttons, and draws it.
    fn say_reader(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        say: Say<'_>,
    ) -> Result<(), Error> {
        let status = HostWords::new(host)?.say(say)?;
        if let Some(reading) = self.readers.get(instance) {
            reading.status = status;
        }
        redraw(host)
    }

    /// The connection of a reader moved on: what it ended in.
    pub(crate) fn reader_progressed(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        use crate::native_model::job::Run;
        match self.job.run.clone() {
            Run::Working(stage) => self.say_reader(host, instance, Say::Key(texts::stage(stage))),
            Run::Done => {
                self.check = None;
                self.job.run = Run::Idle;
                self.fetched(host, instance)
            }
            Run::Failed(failure) => {
                self.check = None;
                self.say_reader(host, instance, texts::failure(&failure))
            }
            Run::Untrusted { smtp, .. } => {
                self.check = None;
                // A reader cannot ask: the list does (a key that changed, or a
                // provider's server, is not offered at all).
                let number = self.readers.get(instance).map(|r| r.account);
                let say = match number {
                    Some(number) => self.pin_say(host, number, smtp)?,
                    None => return Ok(()),
                };
                self.say_reader(host, instance, say)
            }
            _ => Ok(()),
        }
    }

    /// The fetch ended: the mail becomes a letter on the card and is shown,
    /// and marked read in the list where the settings say so. A star that was
    /// set only had to be sent.
    fn fetched(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let Some(reading) = self.readers.get(instance) else {
            return Ok(());
        };
        if reading.shown.is_none() {
            let (account, uid) = (reading.account, reading.uid);
            let folder = copy(&reading.folder)?;
            let storage = DataStorage::new(host);
            let raw = store::raw_file(account, &folder, uid).map_err(fail_error)?;
            let kept = store::text_file(account, &folder, uid).map_err(fail_error)?;
            let made = match store::make_letter(&storage, &raw, &kept) {
                Ok(letter) => match self.readers.get(instance) {
                    Some(reading) => show_letter(host, reading, &letter),
                    None => Err(Error::State),
                },
                Err(fail) => Err(fail_error(fail)),
            };
            match made {
                Ok(()) => self.mark_seen(host, instance)?,
                // Said on the screen, which would wait for the mail for ever.
                Err(Error::NoMemory) => {
                    return self.say_reader(host, instance, Say::Key("mail.fail.memory"));
                }
                Err(Error::State) => return Err(Error::State),
                Err(_) => return self.say_reader(host, instance, Say::Key("mail.fail.storage")),
            }
        }
        if let Some(reading) = self.readers.get(instance) {
            reading.status = String::new();
        }
        redraw(host)
    }

    /// Marks the mail seen in the folder's list where the settings say so.
    fn mark_seen(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        use crate::native_model::account::Values;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        let values = HostValues { host, secrets };
        if values.int("mail.mark-read")? == 0 {
            return Ok(());
        }
        let reading = self.readers.get(instance).ok_or(Error::State)?;
        let storage = DataStorage::new(host);
        reader::change_row(
            &storage,
            reading.account,
            &reading.folder,
            reading.uid,
            |row| {
                row.flags = reader::with_flag(&row.flags, 'S', true)?;
                Ok(())
            },
        )
        .map(|_| ())
        .map_err(fail_error)
    }

    /// Stars the mail or takes the star off: the list at once, the server
    /// after.
    fn toggle_star(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        if self.job.working() {
            return Ok(());
        }
        let reading = self.readers.get(instance).ok_or(Error::State)?;
        let on = !reading.starred;
        let (account, folder, uid) = (reading.account, copy(&reading.folder)?, reading.uid);
        let storage = DataStorage::new(host);
        reader::change_row(&storage, account, &folder, uid, |row| {
            row.flags = reader::with_flag(&row.flags, 'F', on)?;
            Ok(())
        })
        .map_err(fail_error)?;
        if let Some(reading) = self.readers.get(instance) {
            reading.starred = on;
        }
        let task = reader::flag_task(&folder, uid, "\\Flagged", on).map_err(fail_error)?;
        let mut tasks = Vec::new();
        push(&mut tasks, task)?;
        self.begin(host, instance, account, tasks)?;
        redraw(host)
    }

    /// Opens the compose window as a reply (to the sender, or to all).
    fn reply(&mut self, host: &Host<'_>, instance: Instance, all: bool) -> Result<(), Error> {
        let reading = self.readers.get(instance).ok_or(Error::State)?;
        if reading.shown.is_none() {
            return Ok(());
        }
        let (account, folder, uid) = (reading.account, copy(&reading.folder)?, reading.uid);
        // The device holds one TLS session: the writer above gets it.
        if self.check.as_ref().is_some_and(|c| c.owner == instance) {
            self.stop(host);
        }
        self.open_compose(host, account, Some((&folder, uid, all)))
    }

    fn press(&mut self, host: &Host<'_>, instance: Instance, button: usize) -> Result<(), Error> {
        match button {
            0 => self.reply(host, instance, false),
            1 => self.reply(host, instance, true),
            _ => self.toggle_star(host, instance),
        }
    }

    /// An input for the reader on top.
    pub(crate) fn input_reader(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(instance) = self.readers.top_instance(READ_TOKEN) else {
            return Ok(Handled::No);
        };
        let reading = self.readers.get(instance).ok_or(Error::State)?;
        let (top, rows, total) = (reading.top, reading.rows, reading.lines.len());
        match *input {
            Input::Action { action, .. } => {
                if action == "core.left" || action == "core.right" {
                    reading.focus =
                        Some(reader::focus_after(reading.focus, action == "core.right"));
                    redraw(host)?;
                    return Ok(Handled::Yes);
                }
                if action == "core.confirm" {
                    let button = reading.focus.unwrap_or(0);
                    self.press(host, instance, button)?;
                    return Ok(Handled::Yes);
                }
                let Some(step) = Step::of_action(action) else {
                    return Ok(Handled::No);
                };
                self.scroll(host, instance, reader::scrolled(top, rows, total, step))?;
                Ok(Handled::Yes)
            }
            Input::Gesture { position, kind, .. } => {
                if kind == InputGestureKind::Tap {
                    if let Some(action) = reading.header.action(position) {
                        let mut scratch = [0; 128];
                        CommandSender::new(host, &mut scratch).run_action(action)?;
                        return Ok(Handled::Yes);
                    }
                    if let Some(i) = reading.buttons.iter().position(|b| b.contains(position)) {
                        self.press(host, instance, i)?;
                        return Ok(Handled::Yes);
                    }
                }
                let area = reading.area;
                let middle = i32::from(area.y) + i32::from(area.height) / 2;
                let step = match kind {
                    InputGestureKind::SwipeUp if contains(area, position) => Step::PageDown,
                    InputGestureKind::SwipeDown if contains(area, position) => Step::PageUp,
                    InputGestureKind::Tap if contains(area, position) && position.y >= middle => {
                        Step::PageDown
                    }
                    InputGestureKind::Tap if contains(area, position) => Step::PageUp,
                    _ => return Ok(Handled::No),
                };
                self.scroll(host, instance, reader::scrolled(top, rows, total, step))?;
                Ok(Handled::Yes)
            }
            _ => Ok(Handled::No),
        }
    }

    fn scroll(&mut self, host: &Host<'_>, instance: Instance, to: usize) -> Result<(), Error> {
        let reading = self.readers.get(instance).ok_or(Error::State)?;
        // Scrolling puts the focus off the buttons.
        reading.focus = None;
        if reading.top != to {
            reading.top = to;
            redraw(host)?;
        }
        Ok(())
    }

    /// Draws the reader on top.
    pub(crate) fn draw_reader(
        &mut self,
        host: &Host<'_>,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let instance = self.readers.top_instance(READ_TOKEN).ok_or(Error::State)?;
        let working =
            self.job.working() && self.check.as_ref().is_some_and(|c| c.owner == instance);
        let reading = self.readers.get(instance).ok_or(Error::State)?;
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let mut messages = Messages::new()?;
        let (width, height) = (display.display_width(), display.display_height());
        let small = paperpaw_ui_kit::geometry::line_px(&font, style.small)?;
        let layout = reader::layout(
            width,
            height,
            style.header,
            style.gap,
            style.margin,
            style.row,
            small,
        );
        let mut ops = Vec::new();
        paint::push(
            &mut ops,
            Paint::Fill(Rect::new(0, 0, width, height), style.paper),
        )?;
        let mut header = Page::default();
        let title = if reading.title.is_empty() {
            messages.copy(host, "mail.title")?
        } else {
            copy(&reading.title)?
        };
        paperpaw_ui_kit::header::prepare(
            &mut header,
            host,
            &font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            0,
            0,
        )?;
        let note = if working && reading.status.is_empty() {
            None
        } else {
            Some(copy(&reading.status)?)
        };
        draw_body(
            &mut ops,
            &font,
            &style,
            reading,
            &layout,
            &mut messages,
            host,
        )?;
        draw_note(
            &mut ops,
            &font,
            &style,
            &layout,
            note.as_deref().unwrap_or(""),
        )?;
        draw_buttons(
            &mut ops,
            &font,
            &style,
            reading,
            &layout,
            &mut messages,
            host,
        )?;
        paint::replay(&mut frame, &font, header.paint.iter().chain(ops.iter()))?;
        font.release()?;
        header.paint = Vec::new();
        reading.header = header;
        reading.buttons = layout.buttons;
        reading.area = layout.area;
        Ok(())
    }
}

/// Reads the mail from the card if it is there.
fn read_kept(host: &Host<'_>, reading: &mut Reading) -> Result<(), Error> {
    let storage = DataStorage::new(host);
    let file =
        store::text_file(reading.account, &reading.folder, reading.uid).map_err(fail_error)?;
    let letter = match store::read_letter(&storage, &file) {
        Ok(Some(letter)) => letter,
        Ok(None) => return Ok(()),
        Err(Fail::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(()),
    };
    show_letter(host, reading, &letter)
}

fn show_letter(host: &Host<'_>, reading: &mut Reading, letter: &Letter) -> Result<(), Error> {
    let mut words = HostWords::new(host)?;
    reading.shown = Some(reader::shown(&mut words, letter).map_err(fail_error)?);
    reading.title = copy(&letter.subject)?;
    reading.lines = Vec::new();
    reading.wrapped_at = 0;
    reading.top = 0;
    Ok(())
}

fn contains(area: Rect, point: Point) -> bool {
    area.contains(point)
}

fn start_failure(error: Error) -> Say<'static> {
    Say::Key(match error {
        Error::Unsupported => "core.secrets-unsupported",
        Error::NoMemory => "mail.fail.memory",
        Error::NotFound => "mail.no-account",
        Error::Busy => "mail.fail.busy",
        _ => "mail.fail.lost",
    })
}

/// The lines of the mail, from the one the reader is at; before the mail is
/// there, what the screen waits for.
fn draw_body(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    reading: &mut Reading,
    layout: &Layout,
    messages: &mut Messages,
    host: &Host<'_>,
) -> Result<(), Error> {
    let Some(shown) = &reading.shown else {
        if reading.status.is_empty() {
            let waiting = messages.copy(host, "mail.stage.message")?;
            text::message(ops, font, style, layout.area, &waiting)?;
        }
        return Ok(());
    };
    let area = layout.area;
    let line = paperpaw_ui_kit::geometry::line_px(font, style.small.max(style.text * 0.85))?;
    let size = style.small.max(style.text * 0.85);
    if reading.wrapped_at != area.width || reading.lines.is_empty() {
        reading.lines = wrap(font, size, shown, f32::from(area.width))?;
        reading.wrapped_at = area.width;
    }
    reading.rows = usize::from((area.height / line.max(1)).max(1));
    reading.top = reading
        .top
        .min(reading.lines.len().saturating_sub(reading.rows));
    let mut y = area.y;
    for part in reading.lines.iter().skip(reading.top).take(reading.rows) {
        let row = Rect::new(area.x, y, area.width, line);
        text::label(
            ops,
            font,
            part,
            size,
            f32::from(area.x),
            row,
            style.ink,
            None,
        )?;
        y = y.saturating_add(line);
    }
    Ok(())
}

/// `shown` wrapped to `width`, a blank line kept as one.
fn wrap(font: &Font<'_>, size: f32, shown: &str, width: f32) -> Result<Vec<String>, Error> {
    let mut lines = Vec::new();
    for paragraph in shown.split('\n') {
        if lines.len() >= MAX_LINES {
            break;
        }
        if paragraph.trim().is_empty() {
            push(&mut lines, String::new())?;
            continue;
        }
        for part in text::wrap_with(
            |l| font.measure_text(size, l),
            paragraph,
            width.max(1.0),
            MAX_LINES,
        )? {
            // The lines of all the paragraphs together are at most MAX_LINES.
            if lines.len() >= MAX_LINES {
                break;
            }
            push(&mut lines, part)?;
        }
    }
    Ok(lines)
}

fn draw_note(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    layout: &Layout,
    note: &str,
) -> Result<(), Error> {
    if note.is_empty() {
        return Ok(());
    }
    let at = layout.note;
    let wide = font.measure_text(style.small, note)?;
    let x = f32::from(at.x) + (f32::from(at.width) - wide).max(0.0) / 2.0;
    text::label(ops, font, note, style.small, x, at, style.note, Some(at))
}

fn draw_buttons(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    reading: &Reading,
    layout: &Layout,
    messages: &mut Messages,
    host: &Host<'_>,
) -> Result<(), Error> {
    for (i, key) in BUTTONS.iter().enumerate() {
        let key = if i == 2 && reading.starred {
            "mail.unstar"
        } else {
            key
        };
        let label = messages.copy(host, key)?;
        let rect = layout.buttons[i];
        let focused = reading.focus == Some(i);
        let thickness = u32::from(style.line) * if focused { 2 } else { 1 };
        paint::push(
            ops,
            Paint::Focus(rect, style.corner, Some(thickness.max(1)), style.ink),
        )?;
        let room = f32::from(rect.width.saturating_sub(style.margin)).max(1.0);
        let fitted = text::fit(font, style.small, &label, room)?;
        let wide = font.measure_text(style.small, &fitted)?;
        let x = f32::from(rect.x) + (f32::from(rect.width) - wide).max(0.0) / 2.0;
        text::label(
            ops,
            font,
            &fitted,
            style.small,
            x,
            rect,
            style.ink,
            Some(rect),
        )?;
    }
    Ok(())
}
