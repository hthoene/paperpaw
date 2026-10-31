//! The screen: three rows, the first a way on, the others switches.

use alloc::string::{String, ToString};

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::library::{self, service};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use crate::next::{self, Next};
use crate::{Kept, RATING};

/// The most stars.
const STARS: u8 = 5;

#[allow(missing_debug_implementations, reason = "a screen")]
pub(crate) struct EndScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    path: String,
    title: String,
    next: Option<Next>,
    stars: u8,
    finished: bool,
    /// What the rows say is kept for the book's closing.
    kept: Kept,
    /// Rows without notes and with short titles: the text is so large that
    /// the rows with them would not fit on one page.
    compact: bool,
}

impl EndScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params, kept: Kept) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let display = env.device().display;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let path = params.get(params::PATH).unwrap_or_default().to_string();
        let book = library::book(env.storage(), &path);
        let title = book
            .as_ref()
            .map(|b| b.shown_title(&path).into_owned())
            .unwrap_or_default();
        let series = book
            .as_ref()
            .and_then(|b| b.series.clone())
            .unwrap_or_default();
        let stars = book
            .as_ref()
            .and_then(|b| b.field(RATING))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let finished = kept
            .borrow()
            .as_ref()
            .filter(|s| s.path == path)
            .map_or_else(
                || book.as_ref().is_some_and(|b| b.progress == Some(100)),
                |s| s.finished,
            );
        let mut screen = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            next: next::after(env, &path, &series),
            path,
            title,
            stars,
            finished,
            kept,
            compact: false,
        };
        let high = screen.row_height(env, display.width);
        // Notes and all: three rows on a page, else without the notes.
        if 3 * high > u32::from(area.height) {
            screen.compact = true;
        }
        let high = screen.row_height(env, display.width);
        screen.list.set_row(high);
        screen
    }

    /// How high the rows are: the highest of the three.
    fn row_height(&self, env: &Env<'_>, width: u16) -> u32 {
        (0..3)
            .map(|row| {
                let (title, value, note) = self.row(env, row);
                u32::from(ui::Row::value(&title, &value).note(&note).height(
                    env,
                    &self.style,
                    width,
                ))
            })
            .max()
            .unwrap_or(0)
            .max(if self.compact {
                u32::from(self.style.row())
            } else {
                self.style.noted_row()
            })
    }

    fn stars_text(&self, env: &Env<'_>) -> String {
        if self.stars == 0 {
            return env.i18n().tr("end-of-book.rate.none").into();
        }
        alloc::format!("{} / {STARS}", self.stars)
    }

    /// The title, the value and the note of a row.
    fn row(&self, env: &Env<'_>, row: usize) -> (String, String, String) {
        let i18n = env.i18n();
        let (title, short, value, note): (&str, &str, String, String) = match row {
            0 => match &self.next {
                Some(next) => (
                    "end-of-book.next",
                    "end-of-book.next.short",
                    "\u{203a}".into(),
                    if next.number.is_empty() {
                        i18n.format(
                            "end-of-book.next.note.plain",
                            &[("title", Arg::Str(&next.title))],
                        )
                    } else {
                        i18n.format(
                            "end-of-book.next.note",
                            &[
                                ("title", Arg::Str(&next.title)),
                                ("number", Arg::Str(&next.number)),
                                ("total", Arg::Str(&next.total.to_string())),
                            ],
                        )
                    },
                ),
                None => (
                    "end-of-book.library",
                    "end-of-book.library.short",
                    "\u{203a}".into(),
                    i18n.tr("end-of-book.library.note").into(),
                ),
            },
            1 => (
                "end-of-book.rate",
                "end-of-book.rate.short",
                self.stars_text(env),
                i18n.format("end-of-book.rate.note", &[("title", Arg::Str(&self.title))]),
            ),
            _ => (
                "end-of-book.finished",
                "end-of-book.finished",
                i18n.tr(if self.finished {
                    "end-of-book.finished.yes"
                } else {
                    "end-of-book.finished.no"
                })
                .into(),
                i18n.tr("end-of-book.finished.note").into(),
            ),
        };
        if self.compact {
            // The short title, no note: "Next book", "Rating", "Finished".
            return (i18n.tr(short).into(), value, String::new());
        }
        (i18n.tr(title).into(), value, note)
    }

    fn open(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match row {
            0 => match &self.next {
                Some(next) => cx.command(paperpaw_sdk::readers::open(&next.path)),
                None => cx.command(Command::run(actions::LIBRARY)),
            },
            1 => {
                self.stars = (self.stars + 1) % (STARS + 1);
                let value = if self.stars == 0 {
                    String::new()
                } else {
                    self.stars.to_string()
                };
                library::set_field(env.storage(), &self.path, RATING, &value);
                cx.request_redraw();
            }
            _ => {
                self.finished = !self.finished;
                if let Some(session) = self.kept.borrow_mut().as_mut()
                    && session.path == self.path
                {
                    session.finished = self.finished;
                }
                cx.command(service::service(&env).set_finished(&env, &self.path, self.finished));
                cx.request_redraw();
            }
        }
    }
}

impl Screen for EndScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n().tr("end-of-book.title");
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, title, None, self.list.paging(3));
        let screen = &*self;
        self.list.draw(&mut ui, 3, |ui, row, rect, focused| {
            let (title, value, note) = screen.row(env, row);
            ui::Row::value(&title, &value)
                .note(&note)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        let rows = self.list.rows() as isize;
        if let Some(delta) = ui::list_step(action, rows) {
            let change = self.list.move_focus(delta, 3);
            change.request(cx);
            return Handled::Yes;
        }
        if *action == actions::CONFIRM {
            self.open(self.list.focus(), cx);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        if gesture.kind == GestureKind::Tap {
            let paging = self.list.paging(3);
            if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
                cx.command(Command::run(action));
                return Handled::Yes;
            }
        }
        let Some(row) = self.list.hit(gesture.start, 3) else {
            return Handled::No;
        };
        self.list.set_focus(row);
        if gesture.kind == GestureKind::Tap {
            self.open(row, cx);
            return Handled::Yes;
        }
        Handled::No
    }
}
