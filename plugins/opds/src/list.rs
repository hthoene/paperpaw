//! The list of catalogs: *Sync catalogs* (while some are marked for it),
//! the catalogs, and *Add catalog*. The line at the bottom tells how a
//! sync goes, else how to reach a catalog's options.

use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::web::host;
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen,
};

use crate::catalogs::{self, Catalog};
use crate::sync::{self, Report, Sync};
use crate::{BROWSE_SCREEN, EDIT_SCREEN, ID, OPTION_LABELS};

/// Replies: the address of a new catalog, a choice made.
const ADDRESS: Event<Typed> = Event::new("opds.address");
const CHOSEN: Event<choice::Picked> = Event::new("opds.chosen");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Sync,
    Catalog(usize),
    Add,
}

pub(crate) struct ListScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// The line at the bottom.
    status: Rect,
    catalogs: Vec<Catalog>,
    /// The list could not be read: it is left as it is.
    unreadable: bool,
    sync: Rc<RefCell<Sync>>,
    message: Option<String>,
}

impl ListScreen {
    pub(crate) fn new(env: &Env<'_>, sync: Rc<RefCell<Sync>>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        // The note below as high as its words need, at least a row and a
        // half; a catalog's row above it whole.
        let hint = ui::tr_input(env, "opds.hint");
        let help = ui::note_line_height(env, &style, display.width, hint);
        let line = ((style.control_row()) * 3 / 2).max(help).min(
            display
                .height
                .saturating_sub(top + style.noted_row() as u16),
        );
        let status = Rect::new(0, display.height - line, display.width, line);
        let area = Rect::new(
            0,
            top,
            display.width,
            display.height.saturating_sub(top + line),
        );
        let loaded = catalogs::load(env);
        Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            status,
            unreadable: loaded.is_err(),
            catalogs: loaded.unwrap_or_default(),
            sync,
            message: None,
            style,
            header,
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.catalogs.iter().any(|c| c.sync) || self.sync.borrow().running() {
            rows.push(Row::Sync);
        }
        rows.extend((0..self.catalogs.len()).map(Row::Catalog));
        rows.push(Row::Add);
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Saves the list, unless it could not be read; `false` then.
    fn save(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) -> bool {
        if self.unreadable {
            self.message = Some(env.i18n().tr("opds.unreadable").to_string());
            self.redraw(cx);
            return false;
        }
        let mut out = paperpaw_sdk::Commands::new();
        catalogs::save(env, &self.catalogs, &mut out);
        for command in out {
            cx.command(command);
        }
        true
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.rows().get(index).copied() {
            Some(Row::Sync) => {
                let mut sync = self.sync.borrow_mut();
                let mut out = paperpaw_sdk::Commands::new();
                if sync.running() {
                    sync.stop(&mut out);
                } else if !env.caps().network {
                    drop(sync);
                    self.message = Some(env.i18n().tr("opds.no-network").to_string());
                } else {
                    sync.start(&env, crate::marked(&env), &mut out);
                }
                for command in out {
                    cx.command(command);
                }
                self.redraw(cx);
            }
            Some(Row::Catalog(i)) => cx.command(Command::OpenScreen(
                BROWSE_SCREEN,
                Params::new().with(ID, i.to_string()),
            )),
            Some(Row::Add) => cx.command(
                ask::text(env.i18n().tr("opds.address"), &ADDRESS)
                    .initial("https://")
                    .command(),
            ),
            None => {}
        }
    }

    /// What can be done with row `index`.
    fn offer(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some(Row::Catalog(i)) = self.rows().get(index).copied() else {
            return self.activate(index, cx);
        };
        let title = self.catalogs[i].name.clone();
        cx.command(
            paperpaw_sdk::ask::choice(&title, &CHOSEN)
                .about(i.to_string())
                .options(OPTION_LABELS, &["open", "sync", "edit", "remove"])
                .command(),
        );
    }

    fn chosen(&mut self, context: &str, option: &str, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(i) = context
            .parse::<usize>()
            .ok()
            .filter(|&i| i < self.catalogs.len())
        else {
            return;
        };
        match option {
            "open" => cx.command(Command::OpenScreen(
                BROWSE_SCREEN,
                Params::new().with(ID, i.to_string()),
            )),
            "sync" => {
                if !env.caps().network {
                    self.message = Some(env.i18n().tr("opds.no-network").to_string());
                    return self.redraw(cx);
                }
                let mut out = paperpaw_sdk::Commands::new();
                let catalog = self.catalogs[i].clone();
                self.sync
                    .borrow_mut()
                    .start(&env, Vec::from([catalog]), &mut out);
                for command in out {
                    cx.command(command);
                }
                self.redraw(cx);
            }
            "edit" => cx.command(Command::OpenScreen(
                EDIT_SCREEN,
                Params::new().with(ID, i.to_string()),
            )),
            "remove" => {
                let removed = self.catalogs.remove(i);
                if !self.save(&env, cx) {
                    self.catalogs.insert(i, removed);
                    return;
                }
                let len = self.rows().len();
                self.list.set_len(len);
                self.redraw(cx);
            }
            _ => {}
        }
    }

    /// The line at the bottom: a message, how a sync goes, or the hint.
    fn status_text(&self, env: &Env<'_>) -> String {
        if let Some(message) = &self.message {
            return message.clone();
        }
        match &self.sync.borrow().report {
            Some(report) => report_text(env, report),
            None if self.unreadable => env.i18n().tr("opds.unreadable").to_string(),
            None => ui::tr_input(env, "opds.hint").to_string(),
        }
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.rows().len();
        match self.list.move_focus_in(cx.env(), delta, len) {
            ListChange::Page => self.redraw(cx),
            change => change.request(cx),
        }
    }
}

/// How a sync goes or went.
pub(crate) fn report_text(env: &Env<'_>, report: &Report) -> String {
    let catalog = Arg::Str(&report.catalog);
    let mut text = if report.running {
        env.i18n().plural(
            "opds.syncing",
            report.loaded as i64,
            &[("catalog", catalog)],
        )
    } else {
        env.i18n().plural("opds.synced", report.loaded as i64, &[])
    };
    if report.failed > 0 {
        text.push(' ');
        text.push_str(&env.i18n().plural("opds.failed", report.failed as i64, &[]));
    }
    if report.only_lists > 0 {
        text.push(' ');
        text.push_str(
            &env.i18n()
                .plural("opds.only-lists", report.only_lists as i64, &[]),
        );
    }
    if report.unreachable > 0 {
        text.push(' ');
        text.push_str(
            &env.i18n()
                .plural("opds.unreachable", report.unreachable as i64, &[]),
        );
    }
    if report.skipped > 0 {
        text.push(' ');
        text.push_str(
            &env.i18n()
                .plural("opds.skipped", report.skipped as i64, &[]),
        );
    }
    text
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = self.rows();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("opds.title"),
            None,
            self.list.paging(rows.len()),
        );
        let catalogs = &self.catalogs;
        let running = self.sync.borrow().running();
        self.list
            .draw(&mut ui, rows.len(), |ui, index, rect, focused| {
                let (title, value, note) = match rows[index] {
                    Row::Sync if running => (
                        env.i18n().tr("opds.stop").to_string(),
                        String::new(),
                        env.i18n().tr("opds.stop.help").to_string(),
                    ),
                    Row::Sync => (
                        env.i18n().tr("opds.sync-all").to_string(),
                        String::new(),
                        env.i18n().tr("opds.sync-all.help").to_string(),
                    ),
                    Row::Catalog(i) => {
                        let catalog = &catalogs[i];
                        let mut note = host(&catalog.url).to_string();
                        if catalog.sync {
                            let folder = catalog.sync_folder(env);
                            note = env.i18n().format(
                                "opds.synced-to",
                                &[("host", Arg::Str(&note)), ("folder", Arg::Str(&folder))],
                            );
                        }
                        (catalog.name.clone(), String::from("›"), note)
                    }
                    Row::Add => (
                        env.i18n().tr("opds.add").to_string(),
                        String::from("+"),
                        String::new(),
                    ),
                };
                ui::Row::value(&title, &value)
                    .note(&note)
                    .focused(focused)
                    .draw(ui, rect);
            });
        ui.canvas().fill(self.status, Color::WHITE);
        let text = self.status_text(env);
        ui.note_line(self.status, &text);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Back from editing a catalog.
        let loaded = catalogs::load(cx.env());
        self.unreadable = loaded.is_err();
        self.catalogs = loaded.unwrap_or_default();
        let len = self.rows().len();
        self.list.set_len(len);
        self.redraw(cx);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if let Some(delta) = ui::list_step(a, self.list.rows() as isize) {
            self.move_focus(delta, cx);
        } else if *a == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else if *a == actions::MENU {
            self.offer(self.list.focus(), cx);
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows().len();
        let paging = self.list.paging(len);
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self
                .header
                .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.message = None;
        match gesture.kind {
            GestureKind::Tap => self.activate(index, cx),
            GestureKind::LongPress => self.offer(index, cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(typed) = ADDRESS.read(event, params) {
            let text = typed.text.trim();
            if text.is_empty() || text == "https://" {
                return;
            }
            let url = if text.contains("://") {
                text.to_string()
            } else {
                format!("https://{text}")
            };
            self.catalogs.push(Catalog::new(&url));
            if !self.save(&env, cx) {
                self.catalogs.pop();
                return;
            }
            let i = self.catalogs.len() - 1;
            cx.command(Command::OpenScreen(
                EDIT_SCREEN,
                Params::new().with(ID, i.to_string()),
            ));
        } else if let Some(picked) = CHOSEN.read(event, params) {
            self.chosen(&picked.path, &picked.option, cx);
        } else if *event == sync::CHANGED {
            self.message = None;
            self.redraw(cx);
        }
    }
}
