//! *Cards on the SD card*: the files of the folder merged into decks (see
//! [`sync`](crate::sync)), with how far it is, a question where a deck of
//! the file's name is there already, and what came of each file.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen,
};

use crate::SD_FOLDER;
use crate::sync::{self, FileReport, Job, Mode, Outcome, Question, Step};
use crate::table::Fault;

pub(crate) const SCREEN: Id = Id::new_static("vocabulary.sd-card");
/// The parameter that reads every file anew, not only the new and changed.
pub(crate) const FORCE: &str = "force";
/// The parameter of the screen the flash cards open by themselves: the
/// files that failed as they are are not tried again.
pub(crate) const AUTO: &str = "auto";
/// Rows of the result at most.
const MOST_ROWS: usize = 40;

/// The folder the setting names.
pub(crate) fn folder(env: &Env<'_>) -> String {
    sync::folder_of(env.settings().read(&SD_FOLDER))
}

#[derive(Debug)]
enum Phase {
    Working,
    Asking(Question),
    Done,
}

pub(crate) struct SdScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    phase: Phase,
    job: Option<Job>,
    folder: String,
    mode: Mode,
    /// The per mille (in tens) and the file shown last: the screen is drawn
    /// again, and the e-ink refreshed, when one of them changes.
    shown: (u16, String),
    /// What the result says: a title and a note for each row.
    rows: Vec<(String, String)>,
    started: bool,
}

/// Whether progress at `permille` of `file` differs from what `shown` says
/// (in tens of per mille and the file), and so is drawn; `shown` is then
/// what is drawn.
fn worth_drawing(shown: &mut (u16, String), permille: u16, file: &str) -> bool {
    if shown.0 == permille / 10 && shown.1 == file {
        return false;
    }
    *shown = (permille / 10, file.to_string());
    true
}

fn number(n: usize) -> String {
    n.to_string()
}

impl SdScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let display = env.device().display;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            phase: Phase::Working,
            job: None,
            folder: folder(env),
            mode: if params.get(FORCE).is_some() {
                Mode::All
            } else if params.get(AUTO).is_some() {
                Mode::Auto
            } else {
                Mode::Manual
            },
            shown: (u16::MAX, String::new()),
            rows: Vec::new(),
            started: false,
        }
    }

    fn content(&self, env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let top = self.header.area().height + self.style.gap();
        Rect::new(0, top, display.width, display.height.saturating_sub(top))
    }

    /// The folder looked at; made where it is not there.
    fn start(&mut self, env: &Env<'_>) {
        self.started = true;
        let storage = env.storage();
        let seen = sync::pending(storage, &self.folder);
        if !seen.folder {
            sync::make_folder(
                storage,
                &self.folder,
                &env.i18n().format("vocabulary.sd.readme", &[]),
            );
            let note = env
                .i18n()
                .format("vocabulary.sd.made", &[("folder", Arg::Str(&self.folder))]);
            self.finish(env, alloc::vec![(String::new(), note)]);
            return;
        }
        let job = Job::new(storage, &self.folder, self.mode);
        if job.is_empty() {
            let key = if seen.files == 0 {
                "vocabulary.sd.row.empty"
            } else {
                "vocabulary.sd.uptodate"
            };
            let note = env
                .i18n()
                .format(key, &[("folder", Arg::Str(&self.folder))]);
            self.finish(env, alloc::vec![(String::new(), note)]);
            return;
        }
        self.job = Some(job);
    }

    /// The rows shown when all is done.
    fn finish(&mut self, env: &Env<'_>, rows: Vec<(String, String)>) {
        self.rows = rows;
        self.rows.truncate(MOST_ROWS);
        self.phase = Phase::Done;
        self.set_rows(env);
    }

    /// The list of `rows`, each row as high as its note needs.
    fn set_rows(&mut self, env: &Env<'_>) {
        let area = self.content(env);
        let width = area.width;
        let high = self
            .rows
            .iter()
            .map(|(title, note)| {
                let title = if title.is_empty() { " " } else { title };
                u32::from(
                    ui::Row::value(title, "")
                        .note(note)
                        .height(env, &self.style, width),
                )
            })
            .max()
            .unwrap_or(0)
            .max(self.style.noted_row());
        self.list = ListView::new(area, high).focus_on_keys(env);
    }

    /// What a file's report says.
    fn report_rows(&self, env: &Env<'_>, report: &FileReport) -> Vec<(String, String)> {
        let i18n = env.i18n();
        let title = if report.deck.is_empty() {
            report.file.clone()
        } else {
            report.deck.clone()
        };
        let mut rows = Vec::new();
        let note = match report.outcome {
            Outcome::Merged => i18n.format(
                "vocabulary.sd.result",
                &[
                    ("added", Arg::Str(&number(report.added))),
                    ("changed", Arg::Str(&number(report.changed))),
                    ("same", Arg::Str(&number(report.same))),
                ],
            ),
            Outcome::Same => i18n.tr("vocabulary.sd.outcome.same").into(),
            Outcome::Unreadable => i18n.tr("vocabulary.sd.outcome.unreadable").into(),
            Outcome::TooManyDecks => i18n.tr("vocabulary.sd.outcome.too-many").into(),
            Outcome::Crowded => i18n.tr("vocabulary.sd.outcome.crowded").into(),
            Outcome::NoCards => i18n.tr("vocabulary.sd.outcome.no-cards").into(),
            Outcome::TooLarge => i18n.tr("vocabulary.sd.outcome.too-large").into(),
            Outcome::Utf16 => i18n.tr("vocabulary.sd.outcome.utf16").into(),
        };
        rows.push((title, note));
        if report.no_room > 0 {
            rows.push((
                String::new(),
                i18n.format(
                    "vocabulary.sd.no-room",
                    &[("count", Arg::Str(&number(report.no_room)))],
                ),
            ));
        }
        if report.skipped > 0 {
            rows.push((
                i18n.plural("vocabulary.sd.skipped", report.skipped as i64, &[]),
                String::new(),
            ));
        }
        for (line, fault) in &report.problems {
            let key = match fault {
                Fault::NoFront => "vocabulary.sd.fault.no-front",
                Fault::NotUtf8 => "vocabulary.sd.fault.not-utf8",
                Fault::TooLong => "vocabulary.sd.fault.too-long",
                Fault::OpenQuote => "vocabulary.sd.fault.open-quote",
            };
            rows.push((
                i18n.format(
                    "vocabulary.sd.line",
                    &[("line", Arg::Str(&number(*line as usize)))],
                ),
                i18n.tr(key).into(),
            ));
        }
        rows
    }

    /// One more step of the job, its screen told.
    fn work(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !self.started {
            self.start(&env);
        }
        let Some(job) = self.job.as_mut() else {
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
            return;
        };
        // Only a step that changes what is shown draws it again: a refresh
        // of the e-ink for each of the many steps of a big file would take
        // longer than the work.
        let mut show = true;
        match job.step(env.storage()) {
            Step::Working => {
                cx.wake_at(env.now());
                let (permille, file) = job.progress();
                show = worth_drawing(&mut self.shown, permille, file);
            }
            Step::Ask(question) => {
                let area = self.content(&env);
                let top = self.question_height(&env, &question) + 2 * self.style.gap();
                let lower = Rect::new(0, area.y + top, area.width, area.height.saturating_sub(top));
                self.list = ListView::new(lower, self.style.row().into()).focus_on_keys(&env);
                self.phase = Phase::Asking(question);
            }
            Step::Done => {
                let reports = self.job.take().map(|j| j.reports).unwrap_or_default();
                let mut rows = Vec::new();
                for report in &reports {
                    rows.extend(self.report_rows(&env, report));
                }
                if rows.is_empty() {
                    rows.push((
                        String::new(),
                        env.i18n().tr("vocabulary.sd.uptodate").into(),
                    ));
                }
                self.finish(&env, rows);
            }
        }
        if show {
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    fn question_text(env: &Env<'_>, question: &Question) -> String {
        env.i18n().format(
            "vocabulary.sd.ask",
            &[
                ("deck", Arg::Str(&question.deck)),
                ("file", Arg::Str(&question.file)),
            ],
        )
    }

    fn question_height(&self, env: &Env<'_>, question: &Question) -> u16 {
        ui::message_height(
            env,
            &self.style,
            env.device().display.width,
            &Self::question_text(env, question),
        )
    }
}

impl Screen for SdScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let area = self.content(env);
        let title = env.i18n().tr("vocabulary.sd.title");
        let mut ui = ui::Ui::new(canvas, env, &style);
        match &self.phase {
            Phase::Working => {
                self.header.draw(&mut ui, title, None, None);
                let (permille, file) = self.job.as_ref().map_or((0, ""), |job| job.progress());
                let text = env
                    .i18n()
                    .format("vocabulary.sd.reading", &[("file", Arg::Str(file))]);
                let high = ui::message_height(env, &style, area.width, &text);
                let gap = style.gap();
                ui.message(Rect::new(0, area.y + gap, area.width, high), &text);
                let bar = Rect::new(
                    2 * gap,
                    area.y + high + 3 * gap,
                    area.width.saturating_sub(4 * gap),
                    style.row() / 2,
                );
                ui.progress(bar, permille, Color::BLACK);
            }
            Phase::Asking(question) => {
                self.header.draw(&mut ui, title, None, None);
                let text = Self::question_text(env, question);
                let high = ui::message_height(env, &style, area.width, &text);
                ui.message(Rect::new(0, area.y + style.gap(), area.width, high), &text);
                self.list.draw(&mut ui, 2, |ui, row, rect, focused| {
                    let key = if row == 0 {
                        "vocabulary.sd.ask.link"
                    } else {
                        "vocabulary.sd.ask.new"
                    };
                    ui::Row::value(env.i18n().tr(key), "›")
                        .focused(focused)
                        .draw(ui, rect);
                });
            }
            Phase::Done if self.rows.len() == 1 && self.rows[0].0.is_empty() => {
                // One thing to say, and nothing to choose.
                self.header.draw(&mut ui, title, None, None);
                let high = ui::message_height(env, &style, area.width, &self.rows[0].1);
                ui.message(
                    Rect::new(0, area.y + style.gap(), area.width, high),
                    &self.rows[0].1,
                );
            }
            Phase::Done => {
                let len = self.rows.len();
                self.header
                    .draw(&mut ui, title, None, self.list.paging(len));
                let rows = &self.rows;
                self.list.draw(&mut ui, len, |ui, row, rect, focused| {
                    let Some((title, note)) = rows.get(row) else {
                        return;
                    };
                    ui::Row::value(if title.is_empty() { " " } else { title }, "")
                        .note(note)
                        .focused(focused)
                        .draw(ui, rect);
                });
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.started {
            cx.wake_at(cx.env().now());
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if matches!(self.phase, Phase::Working) {
            self.work(cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        let env = *cx.env();
        match self.phase {
            Phase::Working => Handled::Yes,
            Phase::Asking(_) => {
                if *action == actions::CONFIRM {
                    self.answer(self.list.focus() == 0, cx);
                    return Handled::Yes;
                }
                let Some(delta) = ui::list_step(action, self.list.rows() as isize) else {
                    return Handled::No;
                };
                let change = self.list.move_focus(delta, 2);
                change.request(cx);
                Handled::Yes
            }
            Phase::Done => {
                let len = self.rows.len();
                let Some(delta) = ui::list_step(action, self.list.rows() as isize) else {
                    return Handled::No;
                };
                let change = self.list.move_focus(delta, len);
                change.request(cx);
                let _ = env;
                Handled::Yes
            }
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = match self.phase {
            Phase::Asking(_) => 2,
            Phase::Done => self.rows.len(),
            Phase::Working => 0,
        };
        let paging = match self.phase {
            Phase::Done => self.list.paging(len),
            _ => None,
        };
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if matches!(self.phase, Phase::Asking(_))
            && let Some(row) = self.list.hit(gesture.start, 2)
        {
            self.answer(row == 0, cx);
            return Handled::Yes;
        }
        Handled::No
    }
}

impl SdScreen {
    /// The answer to the question: on with the job.
    fn answer(&mut self, link: bool, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(job) = self.job.as_mut() {
            job.answer(env.storage(), link);
        }
        self.phase = Phase::Working;
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
        cx.wake_at(env.now());
    }
}

/// Whether anything of the folder is to be done, for the list of decks.
pub(crate) fn pending(env: &Env<'_>) -> sync::Pending {
    sync::pending(env.storage(), &folder(env))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_that_shows_the_same_is_not_drawn() {
        let mut shown = (u16::MAX, String::new());
        assert!(worth_drawing(&mut shown, 0, "A.tsv"));
        // Many steps of one file between two whole per cent: no refresh.
        let drawn = (0..=1000u16)
            .filter(|p| worth_drawing(&mut shown, *p, "A.tsv"))
            .count();
        assert!(drawn <= 101, "{drawn}");
        assert!(!worth_drawing(&mut shown, 1000, "A.tsv"));
        // Another file is drawn even at the same per mille.
        assert!(worth_drawing(&mut shown, 1000, "B.tsv"));
    }
}
