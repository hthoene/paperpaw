//! Reviewing flash cards: the cards due (see [`cards::due`]), one at a
//! time. The front first; *Select* or a tap shows the back and the
//! example, and four answers: *Again*, *Hard*, *Good*, *Easy*, each with
//! when the card would come again. With buttons, the focus starts on
//! *Good* and goes left and right, *Select* answers; by touch, a tap on
//! one does.
//!
//! A card answered *Again* comes again in this review once its time has
//! come (the 10 minutes its button says), before the next card due;
//! while only such cards are left, the screen says when the next comes
//! and shows it then (or later, from home). Each answer is kept at once
//! (see [`cards::record`]); the decks are written with them when the
//! review closes.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, CloseCause, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id,
    Params, Point, Rect, RefreshMode, Screen,
};

use crate::cards::{self, Card, Day, Due};
use crate::fsrs::{Fx, Grade};
use crate::{NEW_PER_DAY, RETENTION};

/// The review screen; parameter [`DECK`] names the deck, else all are.
pub(crate) const SCREEN: Id = Id::new_static("vocabulary.review");
/// The deck to review.
pub(crate) const DECK: &str = "deck";
/// The card to show first, `<deck> <id>` (the one shown asleep).
pub(crate) const FIRST: &str = "first";
/// Cards due taken into a review at most.
const MOST: usize = 200;

const ANSWER_KEYS: [&str; 4] = [
    "vocabulary.review.again",
    "vocabulary.review.hard",
    "vocabulary.review.good",
    "vocabulary.review.easy",
];

pub(crate) struct ReviewScreen {
    style: UiStyle,
    header: ui::Header,
    queue: VecDeque<Due>,
    /// Cards answered *Again*, until their time has come (`state.due`).
    waiting: Vec<Due>,
    /// The card shown and its texts.
    current: Option<(Due, Card)>,
    revealed: bool,
    /// The answer chosen with buttons.
    focus: usize,
    answered: usize,
    retention: Fx,
    /// When the card shown would come again after each answer, once its
    /// back shows; what the screen says once none is left. Both made
    /// before they are drawn: drawing reads nothing.
    waits: [String; 4],
    message: String,
}

impl ReviewScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let deck = params.get(DECK).and_then(|d| d.parse().ok());
        let now = cards::now(env);
        let day = Day::now(env, now);
        let per_day = env.settings().read(&NEW_PER_DAY).clamp(0, 1000) as u32;
        crate::take_lookups(env);
        let new = cards::new_left(env.storage(), day, per_day);
        let mut queue: VecDeque<Due> = cards::due(env.storage(), day.end, new, MOST, deck).into();
        // The card shown asleep first, if it is among them.
        let first = params.get(FIRST).and_then(|f| {
            let (deck, id) = f.split_once(' ')?;
            Some((deck.parse::<u32>().ok()?, id.parse::<u32>().ok()?))
        });
        if let Some(at) =
            first.and_then(|(d, id)| queue.iter().position(|q| q.deck == d && q.state.id == id))
            && let Some(card) = queue.remove(at)
        {
            queue.push_front(card);
        }
        let retention = env.settings().read(&RETENTION).clamp(70, 99);
        let mut screen = Self {
            header: ui::Header::new(env, &style, true),
            style,
            queue,
            waiting: Vec::new(),
            current: None,
            revealed: false,
            focus: 2,
            answered: 0,
            retention: Fx::hundredths(retention),
            waits: Default::default(),
            message: String::new(),
        };
        screen.next(env);
        screen
    }

    /// Shows the next card, if any is left: one answered *Again* whose
    /// time has come, else the next due.
    fn next(&mut self, env: &Env<'_>) {
        self.revealed = false;
        self.focus = 2;
        self.current = None;
        let now = cards::now(env);
        let ready = self
            .waiting
            .iter()
            .enumerate()
            .filter(|(_, due)| due.state.due <= now)
            .min_by_key(|(_, due)| due.state.due)
            .map(|(at, _)| at);
        let ready = ready.map(|at| self.waiting.remove(at));
        for due in ready
            .into_iter()
            .chain(core::iter::from_fn(|| self.queue.pop_front()))
        {
            // A card deleted or changed since is left out.
            if let Some(card) = cards::card_at(env.storage(), due.deck, due.offset)
                && card.state.id == due.state.id
            {
                self.current = Some((due, card));
                return;
            }
        }
        self.message = if let Some(at) = self.soonest() {
            env.i18n().format(
                "vocabulary.review.next-in",
                &[("wait", Arg::Str(&wait_text(env, at - now)))],
            )
        } else if self.answered > 0 {
            env.i18n()
                .plural("vocabulary.review.done", self.answered as i64, &[])
        } else {
            let now = cards::now(env);
            match cards::next_due(env.storage(), now) {
                Some(at) => env.i18n().format(
                    "vocabulary.review.nothing-until",
                    &[("wait", Arg::Str(&wait_text(env, at - now)))],
                ),
                None => env.i18n().tr("vocabulary.review.nothing").into(),
            }
        };
    }

    /// When the first card answered *Again* is due, if one waits.
    fn soonest(&self) -> Option<i64> {
        self.waiting.iter().map(|due| due.state.due).min()
    }

    /// Where only cards answered *Again* are left: a call when the first
    /// of them is due, to show it (the screen sleeps meanwhile).
    fn wake_for_next(&self, cx: &mut Cx<'_, '_>) {
        if self.current.is_some() {
            return;
        }
        let Some(at) = self.soonest() else {
            return;
        };
        let wait = (at - cards::now(cx.env())).max(0) as u64;
        let when = cx.env().now().add_millis(wait * 1000);
        cx.wake_at(when);
    }

    fn content(&self, env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let top = self.header.area().height + self.style.gap();
        let bar = ui::edge_bar_height(&self.style);
        Rect::new(
            0,
            top,
            display.width,
            display.height.saturating_sub(top + bar),
        )
    }

    fn bar(&self, env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let bar = ui::edge_bar_height(&self.style);
        let band = Rect::new(0, display.height.saturating_sub(bar), display.width, bar);
        ui::edge_bar_band(&self.style, band, false)
    }

    /// The buttons of the bar: one before the back is shown, four after.
    fn buttons(&self, env: &Env<'_>) -> Vec<Rect> {
        let band = self.bar(env);
        let margin = self.style.margin();
        let gap = self.style.gap();
        let n: u16 = if self.revealed { 4 } else { 1 };
        let width = band.width.saturating_sub(2 * margin + gap * (n - 1)) / n;
        (0..n)
            .map(|i| Rect::new(margin + i * (width + gap), band.y, width, band.height))
            .collect()
    }

    fn reveal(&mut self, cx: &mut Cx<'_, '_>) {
        if self.current.is_some() && !self.revealed {
            self.revealed = true;
            self.waits = self.waits(cx.env());
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    fn answer(&mut self, grade: Grade, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some((due, _)) = self.current.take() else {
            return;
        };
        let now = cards::now(&env);
        let after = due
            .state
            .answered(grade, now, same_day(&env, &due, now), self.retention);
        if cards::record(env.storage(), due.deck, due.state.due, &after).is_err() {
            let message = env.i18n().tr("vocabulary.save-failed").into();
            cx.command(Command::Notify(message));
        }
        if due.state.is_new() {
            cards::new_shown(env.storage(), Day::now(&env, now), now);
        }
        self.answered += 1;
        if grade == Grade::Again {
            self.waiting.push(Due {
                state: after,
                ..due
            });
        }
        self.next(&env);
        self.wake_for_next(cx);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// When the card would come again after each answer, as people say it.
    fn waits(&self, env: &Env<'_>) -> [String; 4] {
        let Some((due, _)) = &self.current else {
            return Default::default();
        };
        let now = cards::now(env);
        Grade::ALL.map(|grade| {
            let after = due
                .state
                .answered(grade, now, same_day(env, due, now), self.retention);
            wait_text(env, after.due - now)
        })
    }

    fn draw_card(&self, ui: &mut ui::Ui<'_>, area: Rect, card: &Card) {
        let style = self.style;
        let Some(font) = ui.font() else {
            return;
        };
        let margin = style.margin();
        let width = f32::from(area.width.saturating_sub(2 * margin));
        let large = style.text() * 1.5;
        let mut y = area.y + style.gap() * 2;
        let bottom = area.y + area.height;
        let lines = |ui: &mut ui::Ui<'_>, y: &mut u16, text: &str, size: f32, ink: Color| {
            let height = font.line_metrics(size).line_height as u16;
            for range in font.wrap(size, text, width) {
                if *y + height > bottom {
                    break;
                }
                let line = &text[range];
                let x = f32::from(margin) + (width - font.measure(size, line)).max(0.0) / 2.0;
                let row = Rect::new(margin, *y, area.width.saturating_sub(2 * margin), height);
                ui::Label::new(line)
                    .px(size)
                    .color(ink)
                    .draw_from(ui, x, row);
                *y += height;
            }
        };
        lines(ui, &mut y, &card.front, large, Color::BLACK);
        if !self.revealed {
            return;
        }
        y += style.gap();
        let rule = Rect::new(margin * 2, y, area.width.saturating_sub(4 * margin), 1);
        ui.canvas().fill(rule, style.faint(ui::Ink::NOTE));
        y += style.gap() * 2;
        lines(ui, &mut y, &card.back, style.text(), Color::BLACK);
        if !card.example.is_empty() {
            y += style.gap();
            lines(
                ui,
                &mut y,
                &card.example,
                style.small(),
                style.faint(ui::Ink::NOTE),
            );
        }
    }
}

/// Whether `due` was last answered on the local day `now` is in.
fn same_day(env: &Env<'_>, due: &Due, now: i64) -> bool {
    !due.state.is_new() && Day::now(env, due.state.last).number == Day::now(env, now).number
}

/// `seconds` as a wait: `10 min`, `3 d`, `5 mo`, `2 y`.
fn wait_text(env: &Env<'_>, seconds: i64) -> String {
    let minutes = seconds.max(60) / 60;
    let days = seconds / (24 * 3600);
    let (key, n) = match days {
        0 => ("vocabulary.wait.minutes", minutes),
        d if d < 60 => ("vocabulary.wait.days", d),
        d if d < 365 => ("vocabulary.wait.months", d / 30),
        d => ("vocabulary.wait.years", d / 365),
    };
    env.i18n().format(key, &[("n", Arg::Int(n))])
}

impl Screen for ReviewScreen {
    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        if *action == actions::BACK {
            Hint::Default
        } else if *action == actions::CONFIRM && self.current.is_some() {
            let key = if self.revealed {
                ANSWER_KEYS[self.focus]
            } else {
                "vocabulary.review.show"
            };
            Hint::Text(env.i18n().tr(key).into())
        } else if (*action == actions::LEFT || *action == actions::RIGHT) && self.revealed {
            Hint::Default
        } else {
            Hint::Hidden
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n().tr("vocabulary.review.title");
        let count = self.queue.len() + self.waiting.len() + usize::from(self.current.is_some());
        let left = env
            .i18n()
            .plural("vocabulary.review.left", count as i64, &[]);
        let note = self.current.is_some().then_some(left.as_str());
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, title, note, None);
        let area = self.content(env);
        let Some((_, card)) = &self.current else {
            ui.message(area, &self.message);
            return;
        };
        self.draw_card(&mut ui, area, card);
        let buttons = self.buttons(env);
        if !self.revealed {
            let label = env.i18n().tr("vocabulary.review.show");
            ui.button(buttons[0], label, false, true);
            return;
        }
        let waits = &self.waits;
        let Some(font) = style.font(env) else {
            return;
        };
        let small = font.line_metrics(style.small()).line_height as u16;
        for (i, rect) in buttons.iter().enumerate() {
            let label = env.i18n().tr(ANSWER_KEYS[i]);
            ui.button(*rect, label, false, i == self.focus);
            // When it would come again, above its button.
            let wait = ui::fit(&font, style.small(), &waits[i], f32::from(rect.width));
            let x = f32::from(rect.x)
                + (f32::from(rect.width) - font.measure(style.small(), &wait)) / 2.0;
            let above = Rect::new(
                rect.x,
                rect.y.saturating_sub(small + style.gap()),
                rect.width,
                small,
            );
            ui::Label::new(&wait)
                .px(style.small())
                .color(style.faint(ui::Ink::NOTE))
                .draw_from(&mut ui, x, above);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        if self.current.is_none() {
            return Handled::No;
        }
        if *action == actions::CONFIRM {
            if self.revealed {
                self.answer(Grade::ALL[self.focus], cx);
            } else {
                self.reveal(cx);
            }
            return Handled::Yes;
        }
        let step: isize = if *action == actions::LEFT || *action == actions::PREVIOUS {
            -1
        } else if *action == actions::RIGHT || *action == actions::NEXT {
            1
        } else {
            return Handled::No;
        };
        if self.revealed {
            self.focus = ui::step_focus(self.focus, step, 4);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        } else if step > 0 {
            self.reveal(cx);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let env = *cx.env();
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if self.current.is_none() {
            return Handled::No;
        }
        if !self.revealed {
            self.reveal(cx);
            return Handled::Yes;
        }
        let hit = |rect: &Rect, at: Point| rect.contains(at);
        if let Some(i) = self
            .buttons(&env)
            .iter()
            .position(|r| hit(r, gesture.start))
        {
            self.focus = i;
            self.answer(Grade::ALL[i], cx);
            return Handled::Yes;
        }
        Handled::No
    }

    /// A card answered *Again* is due: shown, if none is.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if self.current.is_some() {
            return;
        }
        let env = *cx.env();
        self.next(&env);
        self.wake_for_next(cx);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn on_close_request(&mut self, _: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        // The answers into the decks, once, as the review ends.
        let _ = cards::fold(cx.env().storage());
        Handled::No
    }
}
