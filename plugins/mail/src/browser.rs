//! The list screens: *Mail* (the folders of every account, with how many
//! mails wait) and a folder (its mails, newest first). Both check for mail
//! from their first row, hold the connection while they do, and ask before
//! trusting a server no authority vouches for.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::Distrust;
use paperpaw_sdk::builtin::{actions, params as core_params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{Confirmed, SocketNotice};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

use crate::account::{self, Account};
use crate::job::{Job, Run};
use crate::session::Task;
use crate::settings::FETCH;
use crate::store::{self, Counts, Row};
use crate::{BOXES_CHANNEL, COMPOSE, LIST, LIST_CHANNEL, READ, texts};

/// Parameters of the screens.
pub mod param {
    pub const ACCOUNT: &str = "account";
    pub const FOLDER: &str = "folder";
    pub const UID: &str = "uid";
}

/// What a line of the list does when chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Check,
    Folder { account: u8, raw: String },
    Message { uid: u32 },
    Write,
    Settings,
}

#[derive(Debug, Clone)]
struct Line {
    title: String,
    note: String,
    value: String,
    target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Boxes,
    Folder {
        account: u8,
        raw: String,
        name: String,
    },
}

/// The list screens.
pub(crate) struct Browser {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    note: Rect,
    mode: Mode,
    lines: Vec<Line>,
    job: Job,
    /// The socket's events: this screen's own.
    events: Event<SocketNotice>,
    /// A screen opened above ended the check: do it again when this one
    /// shows (the device holds one TLS session, which the screen above
    /// needs).
    interrupted: bool,
    /// Accounts still to check, after the one now.
    queue: Vec<u8>,
    /// How the last check went, in words.
    status: String,
    /// The reader was asked whether to trust a server; check again once
    /// they answered.
    asked: Option<u8>,
}

impl Browser {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let note_h = style.row();
        let note = Rect::new(
            0,
            display.height.saturating_sub(note_h),
            display.width,
            note_h,
        );
        let area = Rect::new(0, top, display.width, note.y.saturating_sub(top));
        let mode = match params.get(param::ACCOUNT).and_then(|n| n.parse().ok()) {
            Some(account) => {
                let raw = params.get(param::FOLDER).unwrap_or("INBOX").to_string();
                let name = folder_name(env, account, &raw);
                Mode::Folder { account, raw, name }
            }
            None => Mode::Boxes,
        };
        let channel = match mode {
            Mode::Boxes => BOXES_CHANNEL,
            Mode::Folder { .. } => LIST_CHANNEL,
        };
        let mut screen = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            note,
            mode,
            lines: Vec::new(),
            job: Job::new(channel.socket, channel.events.clone().into()),
            events: channel.events,
            interrupted: false,
            queue: Vec::new(),
            status: String::new(),
            asked: None,
        };
        screen.load(env);
        screen
    }

    fn title(&self, env: &Env<'_>) -> String {
        match &self.mode {
            Mode::Boxes => env.i18n().tr("mail.title").to_string(),
            Mode::Folder { name, .. } => name.clone(),
        }
    }

    fn load(&mut self, env: &Env<'_>) {
        let accounts = Account::all(env);
        let i18n = env.i18n();
        let mut lines = Vec::new();
        if accounts.is_empty() {
            lines.push(Line {
                title: i18n.tr("mail.set-up").into(),
                note: i18n.tr("mail.set-up.help").into(),
                value: "›".into(),
                target: Target::Settings,
            });
            self.lines = lines;
            return;
        }
        lines.push(Line {
            title: i18n.tr("mail.check").into(),
            note: String::new(),
            value: String::new(),
            target: Target::Check,
        });
        let storage = env.storage();
        match &self.mode {
            Mode::Boxes => {
                for account in &accounts {
                    let folders =
                        store::read_folders(storage, &store::folders_file(account.number));
                    let folders: Vec<_> = if folders.is_empty() {
                        alloc::vec![store::Folder {
                            raw: "INBOX".into(),
                            name: i18n.tr("mail.inbox").into(),
                            flags: Vec::new()
                        }]
                    } else {
                        folders
                            .into_iter()
                            .filter(store::Folder::selectable)
                            .collect()
                    };
                    for mut folder in folders {
                        if let Some(key) = special_name(&folder) {
                            folder.name = i18n.tr(key).into();
                        }
                        let counts =
                            Counts::read(storage, &store::status_file(account.number, &folder.raw));
                        let unseen = counts.map_or(0, |c| i64::from(c.unseen));
                        let note = if accounts.len() > 1 {
                            account.address.clone()
                        } else {
                            String::new()
                        };
                        lines.push(Line {
                            title: folder.name.clone(),
                            note,
                            value: if unseen > 0 {
                                format!("{unseen}")
                            } else {
                                String::new()
                            },
                            target: Target::Folder {
                                account: account.number,
                                raw: folder.raw,
                            },
                        });
                    }
                }
                for (key, target) in [
                    ("mail.write", Target::Write),
                    ("mail.settings", Target::Settings),
                ] {
                    lines.push(Line {
                        title: i18n.tr(key).into(),
                        note: String::new(),
                        value: "›".into(),
                        target,
                    });
                }
            }
            Mode::Folder { account, raw, .. } => {
                let mut rows = store::read_index(storage, &store::index_file(*account, raw));
                paperpaw_sdk::sort::sort_by_key(&mut rows, |r| core::cmp::Reverse(r.uid));
                lines.extend(rows.iter().map(|row| message_line(env, row)));
                lines.push(Line {
                    title: i18n.tr("mail.write").into(),
                    note: String::new(),
                    value: "›".into(),
                    target: Target::Write,
                });
            }
        }
        // Rows as high as their notes need; plain ones where none has a note.
        let noted = lines.iter().any(|l| !l.note.is_empty());
        self.list.set_row(if noted {
            self.style.noted_row()
        } else {
            u32::from(self.style.row())
        });
        self.lines = lines;
        let len = self.lines.len();
        if self.list.focus() >= len {
            self.list.set_focus(len.saturating_sub(1));
        }
    }

    /// Starts checking the account at the front of the queue.
    fn next(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(&number) = self.queue.first() else {
            return;
        };
        let Some(account) = Account::all(&env).into_iter().find(|a| a.number == number) else {
            self.queue.remove(0);
            return self.next(cx);
        };
        let now = paperpaw_sdk::time::unix_seconds(&env).unwrap_or(0);
        let count = env
            .settings()
            .read(&FETCH)
            .clamp(1, i64::from(store::MAX_MESSAGES)) as u32;
        let sync = |raw: &str| Task::Sync {
            folder: raw.to_string(),
            count,
            index: store::index_file(number, raw),
            status: store::status_file(number, raw),
            now,
        };
        let tasks = match &self.mode {
            Mode::Boxes => alloc::vec![
                Task::Folders {
                    out: store::folders_file(number)
                },
                sync("INBOX")
            ],
            Mode::Folder { raw, account, .. } if *account == number => alloc::vec![sync(raw)],
            Mode::Folder { .. } => Vec::new(),
        };
        for command in self.job.start_imap(&account, tasks) {
            cx.command(command);
        }
        self.job.watch(cx);
        self.status.clear();
    }

    fn check(&mut self, cx: &mut Cx<'_, '_>) {
        if self.job.working() {
            return;
        }
        let env = *cx.env();
        self.queue = match &self.mode {
            Mode::Boxes => Account::all(&env).iter().map(|a| a.number).collect(),
            Mode::Folder { account, .. } => alloc::vec![*account],
        };
        self.next(cx);
        cx.request_redraw();
    }

    /// The job moved on: what it ended in.
    fn progressed(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.job.run.clone() {
            Run::Done => {
                if !self.queue.is_empty() {
                    self.queue.remove(0);
                }
                self.load(&env);
                self.status = env.i18n().tr("mail.up-to-date").to_string();
                self.job.run = Run::Idle;
                self.next(cx);
            }
            Run::Failed(failure) => {
                self.queue.clear();
                self.status = texts::failure(&env, &failure);
            }
            Run::Untrusted { smtp, key, why, .. } => {
                self.queue.clear();
                // A provider's server, or one whose trusted key changed, is
                // not offered with a tap: said so, not connected.
                if account::offer(&env, self.job.account, smtp) != account::Offer::Ask {
                    self.status = texts::untrusted(&env, self.job.account, smtp);
                    cx.request_redraw();
                    return;
                }
                self.asked = Some(self.job.account);
                let i18n = env.i18n();
                // The key is what is kept and what the next connection is
                // checked against.
                let shown = grouped(&format!("{key}"));
                let why = i18n.tr(match why {
                    Distrust::UnknownIssuer => "mail.pin.why.unknown-issuer",
                    Distrust::NotPinned => "mail.pin.why.not-pinned",
                    _ => "mail.pin.why.unknown-issuer",
                });
                cx.command(
                    paperpaw_sdk::ask::confirm(i18n.tr("mail.pin.title"), &crate::PIN_ANSWER)
                        .text(i18n.format(
                            "mail.pin.text",
                            &[("why", Arg::Str(why)), ("hash", Arg::Str(&shown))],
                        ))
                        .yes(i18n.tr("mail.pin.yes"))
                        .no(i18n.tr("mail.pin.no"))
                        .with("account", format!("{}", self.job.account))
                        .with("smtp", if smtp { "1" } else { "0" })
                        .with("key", format!("{key}"))
                        .command(),
                );
            }
            Run::Idle | Run::Working(_) => {}
        }
        cx.request_redraw();
    }

    /// Lets go of the connection for a screen that opens above: its own is
    /// the one the device can hold.
    fn pause(&mut self, cx: &mut Cx<'_, '_>) {
        for command in self.job.cancel() {
            self.interrupted = true;
            cx.command(command);
        }
    }

    fn open(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let Some(line) = self.lines.get(row) else {
            return;
        };
        let target = line.target.clone();
        if !matches!(target, Target::Check) {
            self.pause(cx);
        }
        match (&target, &self.mode) {
            (Target::Check, _) => self.check(cx),
            (Target::Folder { account, raw }, _) => cx.command(Command::OpenScreen(
                LIST,
                Params::new()
                    .with(param::ACCOUNT, format!("{account}"))
                    .with(param::FOLDER, raw.clone()),
            )),
            (Target::Message { uid }, Mode::Folder { account, raw, .. }) => {
                cx.command(Command::OpenScreen(
                    READ,
                    Params::new()
                        .with(param::ACCOUNT, format!("{account}"))
                        .with(param::FOLDER, raw.clone())
                        .with(param::UID, format!("{uid}")),
                ))
            }
            (Target::Write, mode) => {
                let account = match mode {
                    Mode::Folder { account, .. } => *account,
                    Mode::Boxes => 0,
                };
                cx.command(Command::OpenScreen(
                    COMPOSE,
                    Params::new().with(param::ACCOUNT, format!("{account}")),
                ));
            }
            (Target::Settings, _) => cx.command(Command::OpenSlot(
                slots::SETTINGS,
                Params::new().with(core_params::ID, crate::settings::MAIL.as_str()),
            )),
            _ => {}
        }
    }
}

/// A mail as a line: the sender with the marks, the subject, the time.
fn message_line(env: &Env<'_>, row: &Row) -> Line {
    let mark = if !row.seen() { "\u{2022} " } else { "" };
    let flag = if row.flagged() { "* " } else { "" };
    let subject = if row.subject.is_empty() {
        env.i18n().tr("mail.no-subject")
    } else {
        &row.subject
    };
    Line {
        title: format!("{mark}{flag}{}", row.from),
        note: subject.to_string(),
        value: texts::when(env, row.date),
        target: Target::Message { uid: row.uid },
    }
}

/// The translation key of a folder the server marks for a purpose (or the
/// inbox), so that it reads in the reader's language; for servers that mark
/// none, the usual English names of those folders.
fn special_name(folder: &store::Folder) -> Option<&'static str> {
    if folder.is_inbox() {
        return Some("mail.inbox");
    }
    const KINDS: [(&str, &str, &str); 5] = [
        ("\\sent", "sent", "mail.folder.sent"),
        ("\\drafts", "drafts", "mail.folder.drafts"),
        ("\\trash", "trash", "mail.folder.trash"),
        ("\\junk", "junk", "mail.folder.junk"),
        ("\\archive", "archive", "mail.folder.archive"),
    ];
    let marked = KINDS
        .iter()
        .find(|(flag, _, _)| folder.flags.iter().any(|f| f == flag));
    let named = || {
        KINDS
            .iter()
            .find(|(_, name, _)| folder.name.eq_ignore_ascii_case(name))
    };
    marked.or_else(named).map(|(_, _, key)| *key)
}

fn folder_name(env: &Env<'_>, account: u8, raw: &str) -> String {
    let found = store::read_folders(env.storage(), &store::folders_file(account))
        .into_iter()
        .find(|f| f.raw == raw);
    match found {
        Some(folder) => {
            special_name(&folder).map_or(folder.name.clone(), |key| env.i18n().tr(key).to_string())
        }
        None if raw.eq_ignore_ascii_case("INBOX") => env.i18n().tr("mail.inbox").to_string(),
        None => raw.to_string(),
    }
}

/// A hash in two lines of groups of eight digits, to compare.
fn grouped(hex: &str) -> String {
    let mut out = String::new();
    for (i, chunk) in hex.as_bytes().chunks(8).enumerate() {
        if i == 4 {
            out.push('\n');
        } else if i > 0 {
            out.push(' ');
        }
        out.push_str(core::str::from_utf8(chunk).unwrap_or(""));
    }
    out
}

impl Screen for Browser {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = self.title(env);
        let len = self.lines.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, &title, None, self.list.paging(len));
        let lines = &self.lines;
        let empty_folder = matches!(self.mode, Mode::Folder { .. }) && lines.len() <= 2;
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let Some(line) = lines.get(row) else {
                return;
            };
            ui::Row::value(&line.title, &line.value)
                .note(&line.note)
                .focused(focused)
                .draw(ui, rect);
        });
        let status = match &self.job.run {
            Run::Working(stage) => texts::stage(env, *stage).to_string(),
            _ if !self.status.is_empty() => self.status.clone(),
            _ if empty_folder => env.i18n().tr("mail.empty-folder").to_string(),
            _ => String::new(),
        };
        if !status.is_empty() {
            ui.note_line(self.note, &status);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.load(&env);
        // Back from the question whether to trust the server: check again
        // if the reader said yes.
        if let Some(number) = self.asked.take() {
            let pinned = Account::all(&env)
                .iter()
                .any(|a| a.number == number && (a.imap.pin.is_some() || a.smtp.pin.is_some()));
            if pinned {
                self.job.run = Run::Idle;
                self.check(cx);
            }
        }
        if self.interrupted {
            self.interrupted = false;
            self.job.run = Run::Idle;
            self.check(cx);
        }
        cx.request_redraw();
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(notice) = self.events.read(event, params) {
            let env = *cx.env();
            for command in self.job.notice(notice, env.storage()) {
                cx.command(command);
            }
            self.job.watch(cx);
            self.progressed(cx);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if self.job.timer(cx) {
            self.progressed(cx);
        }
    }

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        for command in self.job.cancel() {
            cx.command(command);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.lines.len();
        let rows = self.list.rows() as isize;
        let Some(delta) = ui::list_step(action, rows) else {
            if *action == actions::CONFIRM {
                self.open(self.list.focus(), cx);
                return Handled::Yes;
            }
            return Handled::No;
        };
        self.list.move_focus_in(cx.env(), delta, len).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.lines.len();
        if gesture.kind == GestureKind::Tap {
            let paging = self.list.paging(len);
            if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
                cx.command(Command::run(action));
                return Handled::Yes;
            }
        }
        let Some(row) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(row);
        if gesture.kind == GestureKind::Tap {
            self.open(row, cx);
            cx.request_refresh(RefreshMode::Partial);
            return Handled::Yes;
        }
        Handled::No
    }
}

/// The answer to *Trust this server?*: keep its key.
pub fn pin_answered(answer: &Confirmed, out: &mut paperpaw_sdk::Commands) {
    if !answer.yes {
        return;
    }
    let ctx = &answer.context;
    let (Some(number), Some(key)) = (
        ctx.get("account")
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=account::MAX).contains(n)),
        ctx.get("key").and_then(paperpaw_sdk::KeyHash::parse),
    ) else {
        return;
    };
    out.push(crate::job::pin(number, ctx.get("smtp") == Some("1"), &key));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, flags: &[&str]) -> store::Folder {
        store::Folder {
            raw: name.into(),
            name: name.into(),
            flags: flags.iter().map(|f| (*f).into()).collect(),
        }
    }

    #[test]
    fn purposes_are_named_by_flag_or_by_the_usual_name() {
        assert_eq!(special_name(&folder("inbox", &[])), Some("mail.inbox"));
        assert_eq!(
            special_name(&folder("Gesendet", &["\\sent"])),
            Some("mail.folder.sent")
        );
        assert_eq!(
            special_name(&folder("Archive", &[])),
            Some("mail.folder.archive")
        );
        assert_eq!(
            special_name(&folder("Drafts", &[])),
            Some("mail.folder.drafts")
        );
        assert_eq!(special_name(&folder("Receipts", &[])), None);
        // The server's flag wins over the name.
        assert_eq!(
            special_name(&folder("Archive", &["\\trash"])),
            Some("mail.folder.trash")
        );
    }
}
