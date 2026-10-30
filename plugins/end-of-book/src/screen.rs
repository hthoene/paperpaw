//! The screen: three rows, the first a way on, the others switches.

use alloc::string::{String, ToString};

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::library::{self, service};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use crate::next::{self, Next};
use crate::RATING;

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
}

impl EndScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
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
        let series = book.as_ref().and_then(|b| b.series.clone()).unwrap_or_default();
        let stars = book
            .as_ref()
            .and_then(|b| b.field(RATING))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let mut screen = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            next: next::after(env, &path, &series),
            path,
            title,
            stars,
            finished: true,
        };
        let width = display.width;
        let high = (0..3)
            .map(|row| {
                let (title, value, note) = screen.row(env, row);
                u32::from(ui::Row::value(&title, &value).note(&note).height(
                    env,
                    &screen.style,
                    width,
                ))
            })
            .max()
            .unwrap_or(0)
            .max(screen.style.noted_row());
        screen.list.set_row(high);
        screen
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
        match row {
            0 => match &self.next {
                Some(next) => (
                    i18n.tr("end-of-book.next").into(),
                    "\u{203a}".into(),
                    i18n.format(
                        "end-of-book.next.note",
                        &[
                            ("title", Arg::Str(&next.title)),
                            ("number", Arg::Str(&next.number)),
                            ("total", Arg::Str(&next.total.to_string())),
                        ],
                    ),
                ),
                None => (
                    i18n.tr("end-of-book.library").into(),
                    "\u{203a}".into(),
                    i18n.tr("end-of-book.library.note").into(),
                ),
            },
            1 => (
                i18n.tr("end-of-book.rate").into(),
                self.stars_text(env),
                i18n.format("end-of-book.rate.note", &[("title", Arg::Str(&self.title))]),
            ),
            _ => (
                i18n.tr("end-of-book.finished").into(),
                i18n.tr(if self.finished {
                    "end-of-book.finished.yes"
                } else {
                    "end-of-book.finished.no"
                })
                .into(),
                i18n.tr("end-of-book.finished.note").into(),
            ),
        }
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

