//! The Pomodoro screen: a large countdown, the progress of the phase, the
//! dots of the set, the day's count, and Start/Pause, Skip and Reset.
//!
//! It asks for a redraw as rarely as the display needs ([`next_tick`]),
//! with a partial refresh, and holds the device awake only if the reader
//! asked for it and only while the timer runs on this screen.

use alloc::format;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::power::WakeHold;
use paperpaw_sdk::ui::{self, Ink, Label, Stack, TextRole, UiStyle};
use paperpaw_sdk::widgets::Align;
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode, Screen,
};

use crate::phase::{gone, next_tick, shown_secs};
use crate::state::Shared;
use crate::{AWAKE, DONE, lengths, widgets_changed};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Main,
    Skip,
    Reset,
}

const ORDER: [Focus; 3] = [Focus::Main, Focus::Skip, Focus::Reset];

pub struct PomodoroScreen {
    style: UiStyle,
    header: ui::Header,
    shared: Shared,
    focus: Focus,
    buttons: [Rect; 3],
}

impl PomodoroScreen {
    pub fn new(env: &Env<'_>, shared: Shared) -> Self {
        let style = UiStyle::new(env);
        Self {
            header: ui::Header::new(env, &style, true),
            style,
            shared,
            focus: Focus::Main,
            buttons: [Rect::new(0, 0, 0, 0); 3],
        }
    }

    /// Asks for the next redraw as rarely as the display needs. The
    /// phase's own timer ends it on time.
    fn tick(&self, cx: &mut Cx<'_, '_>) {
        let (running, left) = {
            let st = self.shared.borrow();
            let whole = lengths(cx.env()).ms(st.phase);
            (st.running, st.remaining(cx.env().now(), whole))
        };
        if !running {
            return;
        }
        let now = cx.env().now();
        cx.wake_at(now.add_millis(next_tick(left)));
        if cx.env().settings().read(&AWAKE) {
            cx.keep_awake(WakeHold::new(
                Id::new_static("pomodoro.awake"),
                "pomodoro.setting.awake",
            ));
        }
    }

    fn changed(&self, cx: &mut Cx<'_, '_>) {
        for command in widgets_changed() {
            cx.command(command);
        }
        self.tick(cx);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn toggle(&mut self, cx: &mut Cx<'_, '_>) {
        let mut cmds = Vec::new();
        {
            let mut st = self.shared.borrow_mut();
            if st.running {
                st.pause(cx.env(), &mut cmds);
            } else {
                st.start(cx.env(), &mut cmds);
                let note = cx.env().i18n().tr("pomodoro.notice.slow");
                cmds.push(Command::Notify(note.into()));
            }
        }
        for c in cmds {
            cx.command(c);
        }
        self.changed(cx);
    }

    fn skip(&mut self, cx: &mut Cx<'_, '_>) {
        self.shared.borrow_mut().advance(cx.env(), false);
        cx.command(Command::CancelWork(DONE.id().clone(), None));
        self.changed(cx);
    }

    fn reset(&mut self, cx: &mut Cx<'_, '_>) {
        self.shared.borrow_mut().reset(cx.env());
        cx.command(Command::CancelWork(DONE.id().clone(), None));
        self.changed(cx);
    }

    /// Moves the focus to `to`, drawing only the two buttons it touches.
    fn move_focus(&mut self, to: Focus, cx: &mut Cx<'_, '_>) {
        let rect = |f: Focus| self.buttons[ORDER.iter().position(|o| *o == f).unwrap_or(0)];
        let (from, to_rect) = (rect(self.focus), rect(to));
        self.focus = to;
        let both = from.union(&to_rect);
        if both.width == 0 {
            cx.request_redraw();
        } else {
            // The focus ring reaches a little beyond the button.
            let grow = self.style.gap().max(10);
            let x = both.x.saturating_sub(grow);
            let y = both.y.saturating_sub(grow);
            let area = Rect::new(x, y, both.width + 2 * grow, both.height + 2 * grow);
            cx.request_redraw_area(area);
        }
        cx.request_refresh(RefreshMode::Partial);
    }

    fn press(&mut self, which: Focus, cx: &mut Cx<'_, '_>) {
        match which {
            Focus::Main => self.toggle(cx),
            Focus::Skip => self.skip(cx),
            Focus::Reset => self.reset(cx),
        }
    }
}

impl Screen for PomodoroScreen {
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.tick(cx);
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.tick(cx);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn on_event(&mut self, _event: &Id, _params: &Params, cx: &mut Cx<'_, '_>) {
        // The phase may have ended while this screen is open.
        self.tick(cx);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = &self.style;
        let mut ui = ui::Ui::new(canvas, env, style);
        let i18n = env.i18n();
        self.header
            .draw(&mut ui, i18n.tr("pomodoro.title"), None, None);

        let display = env.device().display;
        let margin = style.margin();
        let top = self.header.area().height;
        let body = Rect::new(
            margin,
            top,
            display.width.saturating_sub(2 * margin),
            display.height.saturating_sub(top + margin),
        );

        let st = self.shared.borrow();
        let set = lengths(env);
        let total_ms = set.ms(st.phase);
        let left = st.remaining(env.now(), total_ms).min(total_ms);
        let secs = shown_secs(left);
        let clock = format!("{:02}:{:02}", secs / 60, secs % 60);

        let mut stack = Stack::new(body, style.gap());
        let phase_area = stack.take(style.row());
        Label::new(i18n.tr(st.phase.key()))
            .role(TextRole::BODY)
            .align(Align::Center)
            .draw(&mut ui, phase_area);

        let clock_h = (body.height / 3).max(style.row());
        let clock_area = stack.take(clock_h);
        Label::new(&clock)
            .role(TextRole::NUMERAL)
            .min(12.0)
            .align(Align::Center)
            .draw(&mut ui, clock_area);

        let bar = stack.take((style.row() / 2).max(8));
        let permille = gone(left, total_ms);
        ui.progress(bar, permille, style.ink(Ink::TEXT));

        let dots_area = stack.take(style.row());
        crate::widget::dots(
            ui.canvas(),
            dots_area,
            set.rounds.max(2),
            st.done,
            style.ink(Ink::TEXT),
        );

        let today = crate::widget::count_line(env, &st.log);
        let line = stack.take(style.row());
        Label::new(&today)
            .role(TextRole::SMALL)
            .align(Align::Center)
            .draw(&mut ui, line);
        if let Some(week) = crate::widget::week_line(env, &st.log) {
            let line = stack.take(style.row());
            Label::new(&week)
                .role(TextRole::SMALL)
                .align(Align::Center)
                .draw(&mut ui, line);
        }

        let main_label = i18n.tr(if st.running {
            "pomodoro.pause"
        } else if st.left_ms != 0 {
            "pomodoro.resume"
        } else {
            "pomodoro.start"
        });
        let labels = [
            main_label,
            i18n.tr("pomodoro.skip"),
            i18n.tr("pomodoro.reset"),
        ];
        let cells_h = ui::button_cells_height(env, style, body.width, &labels, style.gap());
        let area = stack.take_bottom(cells_h);
        let cells = ui::button_cells(env, style, area, &labels, style.gap());
        let shown = ui::focus_shown(env);
        for (i, cell) in cells.iter().enumerate().take(3) {
            self.buttons[i] = *cell;
            ui.button(*cell, labels[i], i == 0, shown && self.focus == ORDER[i]);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let at = ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        if *action == actions::CONFIRM {
            let f = self.focus;
            self.press(f, cx);
        } else if *action == actions::LEFT || *action == actions::UP {
            self.move_focus(ORDER[(at + 2) % 3], cx);
        } else if *action == actions::RIGHT || *action == actions::DOWN {
            self.move_focus(ORDER[(at + 1) % 3], cx);
        } else if *action == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        for (i, rect) in self.buttons.iter().enumerate() {
            if rect.contains(gesture.start) {
                self.focus = ORDER[i];
                self.press(ORDER[i], cx);
                return Handled::Yes;
            }
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        Handled::No
    }
}
