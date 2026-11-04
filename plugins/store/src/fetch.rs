//! What the reader sees while the plugins another plugin asked for are
//! fetched ([`crate::request`]): the packs of letters a book needs, say.
//! The dialog's *Download* opens this screen over the book: the names, a
//! bar with the percent and the bytes so far, and how it ends:
//!
//! - all is here: the screen closes, the book shows its letters (the one
//!   who asked is told, as always) and *Ready* is said;
//! - a pack that has to wait for a restart before it works: *Restart now*
//!   or *Later* (the packs of letters are data plugins, which work as soon
//!   as they are installed, so this is for a pack that is not);
//! - something did not come (the connection went, no room): what it was,
//!   *Try again* or *Close*.
//!
//! The bytes are those of the files in the download's folder against the
//! sizes the library lists, read every few seconds: a big file moves the
//! bar as it comes where the shell writes it as it comes, else when it is
//! whole. The screen may be left (*Back*): the download goes on, and the
//! book is laid out anew when it is done, as before.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::events::{self, PluginsAnswered};
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Commands, Cx, Env, Gesture, GestureKind, Handled, Id, Instant,
    Params, Rect, Screen, Storage, files,
};

use crate::catalog::Entry;
use crate::{DOWNLOADED, FETCH_SCREEN, INSTALLER, request};

/// How often the bytes are counted while the screen is shown (ms).
const TICK_MS: u64 = 2_000;

/// The command that shows the download of `ids`, which `by` asked for.
pub(crate) fn open(by: &Id, ids: &[String]) -> Command {
    Command::OpenScreen(
        FETCH_SCREEN,
        Params::new()
            .with(params::ID, by.to_string())
            .with(params::TEXT, ids.join("\n")),
    )
}

/// How far it is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Fetching.
    Working,
    /// All here; closes when shown (the screen was covered meanwhile).
    Done,
    /// What did not come, as the store says it.
    Failed(String),
    /// Installed, and works after a restart.
    Restart,
}

/// What a row of the end does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Retry,
    Close,
    RestartNow,
    Later,
}

pub(crate) struct FetchScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// Who asked, and for what.
    by: Id,
    ids: Vec<String>,
    /// Their names, joined, and what each takes.
    names: String,
    sizes: Vec<(String, u64)>,
    phase: Phase,
    /// The permille and the bytes shown.
    permille: u16,
    done: u64,
    total: u64,
    shown: bool,
    /// The height the rows below were laid out for.
    laid_for: u16,
}

impl FetchScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let by = params
            .get(params::ID)
            .and_then(|by| Id::parse(by).ok())
            .unwrap_or(crate::PLUGIN);
        let ids: Vec<String> = params
            .get(params::TEXT)
            .unwrap_or_default()
            .lines()
            .map(ToString::to_string)
            .collect();
        let listed = request::entries(env, &ids);
        let names = request::names(&listed);
        let sizes = listed
            .iter()
            .map(|(id, entry)| (id.clone(), entry.as_ref().map_or(0, |e: &Entry| e.size)))
            .collect::<Vec<_>>();
        let total = sizes.iter().map(|(_, size)| *size).sum();
        let mut screen = Self {
            list: ListView::new(Rect::new(0, 0, 0, 0), u32::from(style.row())),
            style,
            header,
            by,
            ids,
            names,
            sizes,
            phase: Phase::Working,
            permille: 0,
            done: 0,
            total,
            shown: false,
            laid_for: 0,
        };
        screen.layout(env);
        screen.count(env);
        screen
    }

    /// The rows of the end go below the bar.
    fn layout(&mut self, env: &Env<'_>) {
        let display = env.device().display;
        let top = self.bar(env).y + self.style.row() * 2;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        self.list = ListView::new(area, u32::from(self.style.row())).focus_on_keys(env);
        self.header = ui::Header::new(env, &self.style, true);
        self.laid_for = self.message_area(env).height;
    }

    /// What is said below the header: the names, or what did not come.
    fn message_text(&self, env: &Env<'_>) -> String {
        match &self.phase {
            Phase::Failed(text) => text.clone(),
            Phase::Restart => env.i18n().format(
                "store.fetch.restart.text",
                &[("names", Arg::Str(&self.names))],
            ),
            Phase::Working | Phase::Done => self.names.clone(),
        }
    }

    /// Where that is said: two rows, or the lines it needs at a large text.
    fn message_area(&self, env: &Env<'_>) -> Rect {
        let margin = self.style.margin();
        let width = self.header.area().width.saturating_sub(2 * margin);
        let top = self.header.area().height + self.style.gap();
        let needs = self.message_needs(env);
        Rect::new(margin, top, width, needs.max(self.style.row() * 2))
    }

    /// The height the lines of that need.
    fn message_needs(&self, env: &Env<'_>) -> u16 {
        let margin = self.style.margin();
        let width = self.header.area().width.saturating_sub(2 * margin);
        ui::message_height(env, &self.style, width, &self.message_text(env))
    }

    /// Where the hint at the bottom is: all its lines, from the bottom up.
    fn hint_area(&self, env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let hint = env.i18n().tr("store.fetch.hint");
        let height = ui::note_line_height(env, &self.style, display.width, hint)
            .max(self.style.row() * 3 / 2);
        Rect::new(
            0,
            display.height.saturating_sub(height),
            display.width,
            height,
        )
    }

    /// Where the bar is: below the header and the names, with room to
    /// spare, or close below them where the lines of the hint at a large
    /// text leave no room for the percent under it.
    fn bar(&self, env: &Env<'_>) -> Rect {
        let margin = self.style.margin();
        let gap = self.style.gap();
        let width = self.header.area().width.saturating_sub(2 * margin);
        let message = self.message_area(env);
        let spare = (self.header.area().height + self.style.row() * 5 / 2)
            .max(message.y + message.height + gap);
        let bar = self.style.row() / 2;
        let y = if spare + bar + gap + self.style.row() <= self.hint_area(env).y {
            spare
        } else {
            self.header.area().height + gap + self.message_needs(env) + gap
        };
        Rect::new(margin, y, width, bar)
    }

    /// The bytes of what is here: a plugin installed counts whole, the
    /// one being downloaded by the files in its folder.
    fn count(&mut self, env: &Env<'_>) -> bool {
        let storage = env.storage();
        let job = INSTALLER.progress(storage);
        let mut done = 0u64;
        let mut whole = 0usize;
        for (id, size) in &self.sizes {
            if env.plugins().get(id).is_some() {
                done += size;
                whole += 1;
            } else if let (Some(job), Some(folder)) = (&job, INSTALLER.folder(storage))
                && job.plugin == *id
            {
                done += folder_bytes(storage, &folder, 0).min(*size);
            }
        }
        let permille = (done.min(self.total) * 1000)
            .checked_div(self.total)
            .unwrap_or((whole * 1000 / self.ids.len().max(1)) as u64) as u16;
        let changed = permille != self.permille || done != self.done;
        // Never whole before the one who asked has been told.
        self.permille = permille.min(990);
        self.done = done;
        changed
    }

    fn rows(&self) -> Vec<Row> {
        match &self.phase {
            Phase::Working | Phase::Done => Vec::new(),
            Phase::Failed(_) => alloc::vec![Row::Retry, Row::Close],
            Phase::Restart => alloc::vec![Row::RestartNow, Row::Later],
        }
    }

    /// What the download came to: told by the store once it is over.
    fn answered(&mut self, answer: &PluginsAnswered, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if answer.error.is_empty() && answer.missing_ids().next().is_none() {
            let waits = answer.installed_ids().any(|id| {
                env.plugins()
                    .get(id.as_str())
                    .is_some_and(paperpaw_sdk::PluginInfo::restart_needed)
            });
            self.permille = 1000;
            if waits {
                self.phase = Phase::Restart;
                self.list.set_focus(0);
                self.list.set_len(2);
                return self.redraw(cx);
            }
            let text = env.i18n().tr(if self.by.as_str() == "reader" {
                "store.fetch.ready.reader"
            } else {
                "store.fetch.ready"
            });
            cx.command(Command::Notify(text.to_string()));
            if self.shown {
                cx.command(Command::Back);
            } else {
                self.phase = Phase::Done;
            }
            return;
        }
        // Declined (the dialog said *Not now* elsewhere) is not an end.
        if answer.declined {
            return;
        }
        let text = if answer.error.is_empty() {
            env.i18n()
                .format("store.request.failed", &[("names", Arg::Str(&self.names))])
        } else {
            answer.error.clone()
        };
        self.phase = Phase::Failed(text);
        self.list.set_focus(0);
        self.list.set_len(2);
        self.redraw(cx);
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(paperpaw_sdk::RefreshMode::Partial);
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.rows().get(index).copied() {
            Some(Row::Retry) => {
                let mut out = Commands::new();
                request::again(&env, &self.by, &self.ids, &mut out);
                for command in out {
                    cx.command(command);
                }
                self.phase = Phase::Working;
                self.permille = 0;
                self.count(&env);
                self.list.set_len(0);
                self.redraw(cx);
                cx.wake_at(Instant(env.now().millis() + TICK_MS));
            }
            Some(Row::RestartNow) => cx.command(Command::Restart),
            Some(Row::Close | Row::Later) => cx.command(Command::Back),
            None => {}
        }
    }

    /// The text of row `row`.
    fn row_text<'a>(env: &'a Env<'_>, row: Row) -> &'a str {
        env.i18n().tr(match row {
            Row::Retry => "store.fetch.retry",
            Row::Close => "store.fetch.close",
            Row::RestartNow => "store.restart",
            Row::Later => "store.fetch.later",
        })
    }
}

/// The bytes of the files in `dir` and the folders in it.
fn folder_bytes(storage: &dyn Storage, dir: &str, depth: u8) -> u64 {
    let mut bytes = 0u64;
    let mut folders: Vec<String> = Vec::new();
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a folder that cannot be read counts for nothing: the bar waits"
    )]
    {
        let _ = storage.list(dir, &mut |entry| {
            if entry.is_dir {
                folders.push(entry.name.to_string());
            } else {
                bytes += entry.size;
            }
        });
    }
    if depth < 3 {
        for name in folders {
            bytes += folder_bytes(storage, &format!("{dir}/{name}"), depth + 1);
        }
    }
    bytes
}

impl Screen for FetchScreen {
    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        self.style = UiStyle::new(&env);
        self.layout(&env);
        cx.request_redraw();
        Handled::Yes
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let i18n = env.i18n();
        self.header
            .draw(&mut ui, i18n.tr("store.fetch.title"), None, None);
        let margin = style.margin();
        let width = self.header.area().width.saturating_sub(2 * margin);
        // What is fetched, or what did not come.
        let message = self.message_area(env);
        if message.height != self.laid_for {
            // Words that need more lines move the rows below them.
            self.layout(env);
        }
        let text = self.message_text(env);
        // Close above the bar where the room is short.
        let room = self.bar(env).y.saturating_sub(message.y + style.gap());
        let message = Rect::new(
            message.x,
            message.y,
            message.width,
            message.height.min(room).max(self.message_needs(env)),
        );
        ui.message(message, &text);
        if matches!(self.phase, Phase::Failed(_) | Phase::Restart) {
            let rows = self.rows();
            let texts: Vec<&str> = rows.iter().map(|r| Self::row_text(env, *r)).collect();
            self.list
                .draw(&mut ui, rows.len(), |ui, index, rect, focused| {
                    ui::Row::value(texts[index], "")
                        .focused(focused)
                        .draw(ui, rect);
                });
            return;
        }
        let bar = self.bar(env);
        ui.progress(bar, self.permille, Color::BLACK);
        let percent = i18n
            .localize_digits(&format!("{} %", self.permille / 10))
            .into_owned();
        let text = if self.total > 0 {
            i18n.format(
                "store.fetch.progress",
                &[
                    ("percent", Arg::Str(&percent)),
                    (
                        "done",
                        Arg::Str(&files::shown_size(env, self.done.min(self.total))),
                    ),
                    ("total", Arg::Str(&files::shown_size(env, self.total))),
                ],
            )
        } else {
            percent
        };
        let needs = ui::message_height(env, &style, width, &text);
        let line = Rect::new(
            margin,
            bar.y + bar.height + style.gap(),
            width,
            needs.max(style.row()),
        );
        ui.message(line, &text);
        // The hint keeps all its lines, from the bottom up.
        let note = self.hint_area(env);
        let hint = i18n.tr("store.fetch.hint");
        ui.note_line(note, hint);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.shown = true;
        if self.phase == Phase::Done {
            cx.command(Command::Back);
            return;
        }
        if self.count(&env) {
            self.redraw(cx);
        }
        if self.phase == Phase::Working {
            cx.wake_at(Instant(env.now().millis() + TICK_MS));
        }
    }

    fn on_hide(&mut self) {
        self.shown = false;
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if self.phase != Phase::Working || !self.shown {
            return;
        }
        let env = *cx.env();
        if self.count(&env) {
            self.redraw(cx);
        }
        cx.wake_at(Instant(env.now().millis() + TICK_MS));
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(answer) = events::PLUGINS_ANSWERED.read(event, params) {
            if answer.by == self.by && self.phase == Phase::Working {
                self.answered(&answer, cx);
            }
        } else if (DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED)
            && self.phase == Phase::Working
            && self.count(&env)
        {
            self.redraw(cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows().len();
        if len == 0 {
            return Handled::No;
        }
        if let Some(change) = self.list.navigate_in(cx.env(), action, len) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let len = self.rows().len();
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.activate(index, cx);
        Handled::Yes
    }
}
