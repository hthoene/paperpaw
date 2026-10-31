//! Finishing an update
//! ([`FINISHING_UPDATE`](paperpaw_sdk::builtin::screens::FINISHING_UPDATE)):
//! a firmware just installed runs a while on trial before it is kept,
//! then starts again to load the code plugins it started without.
//! Meanwhile this screen says so, with a bar that fills until the
//! restart, and takes no input: nothing begun is cut off by the restart.
//! The shell opens it with when the trial began and when it ends
//! ([`FROM`], [`UNTIL`], in milliseconds of [`Instant`]) and restarts
//! from it.

use paperpaw_sdk::ui::{self, Ink, Label, TextRole, UiStyle};
use paperpaw_sdk::widgets::Align;
use paperpaw_sdk::{
    Canvas, CloseCause, Color, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Instant,
    Params, PointerEvent, Rect, RefreshMode, Screen,
};

/// When the trial began, in milliseconds.
pub(crate) const FROM: &str = "from";
/// When the trial ends, in milliseconds.
pub(crate) const UNTIL: &str = "until";
/// The bar fills in this many steps: few refreshes of the panel.
const STEPS: u64 = 10;
/// How long after the trial it still holds on, waiting for the restart;
/// then it lets go, so a device whose restart does not come is not kept
/// on it.
const GRACE_MS: u64 = 60_000;

pub(crate) struct Finishing {
    from: u64,
    until: u64,
    /// It guards the device and takes no input: while there is a trial
    /// to show and a little after; never opened without one.
    holding: bool,
}

impl Finishing {
    pub(crate) fn new(params: &Params) -> Self {
        let read = |key| params.get(key).and_then(|v| v.parse().ok()).unwrap_or(0);
        let from = read(FROM);
        let until = read(UNTIL).max(from);
        Self {
            from,
            until,
            holding: until > 0,
        }
    }

    /// How far the trial is, in steps of [`STEPS`].
    fn step(&self, now: Instant) -> u64 {
        let total = self.until - self.from;
        let done = now.0.saturating_sub(self.from).min(total);
        (done * STEPS).checked_div(total).unwrap_or(STEPS)
    }

    /// When the bar fills its next step.
    fn next(&self, now: Instant) -> Option<Instant> {
        let step = self.step(now);
        (step < STEPS).then(|| Instant(self.from + (self.until - self.from) * (step + 1) / STEPS))
    }

    fn wake(&mut self, cx: &mut Cx<'_, '_>) {
        let now = cx.env().now();
        if now.0 >= self.until.saturating_add(GRACE_MS) {
            self.holding = false;
        }
        match self.next(now) {
            Some(at) => cx.wake_at(at),
            None if self.holding => cx.wake_at(Instant(self.until.saturating_add(GRACE_MS))),
            None => {}
        }
    }
}

impl Screen for Finishing {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        // All of its part of the screen: drawn in strips, each clipped.
        let content = env.content();
        let area = Rect::new(0, 0, content.width, content.height);
        canvas.fill(area, Color::WHITE);
        let style = UiStyle::new(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        let (margin, gap, row) = (style.margin(), style.gap(), style.row());
        let inner = area.width.saturating_sub(2 * margin);
        let middle = area.y + area.height / 2;
        let title = env.i18n().tr("core.finishing-update.title");
        Label::new(title)
            .role(TextRole::TITLE)
            .align(Align::Center)
            .draw(
                &mut ui,
                Rect::new(
                    area.x + margin,
                    middle.saturating_sub(row + gap),
                    inner,
                    row,
                ),
            );
        let bar = Rect::new(area.x + margin, middle, inner, gap);
        let permille = (self.step(env.now()) * 1000 / STEPS) as u16;
        ui.progress(bar, permille, Color::BLACK);
        let text = env.i18n().tr("core.finishing-update.text");
        let below = Rect::new(area.x + margin, middle + gap * 2, inner, row * 2);
        Label::new(text)
            .role(TextRole::SMALL)
            .ink(Ink::NOTE)
            .align(Align::Center)
            .wrap()
            .draw(&mut ui, below);
    }

    // While it holds on, nothing shows above it, nothing closes it, and
    // it takes no input.
    fn guards(&self) -> bool {
        self.holding
    }

    fn holds_edges(&self) -> bool {
        self.holding
    }

    fn hints(&self) -> Option<HintVisibility> {
        self.holding.then_some(HintVisibility::Hidden)
    }

    fn hint(&self, action: &Id, _: &Env<'_>) -> Hint {
        match self.holding {
            true => Hint::Hidden,
            false => Hint::only_back(action),
        }
    }

    fn on_close_request(&mut self, _: CloseCause, _: &mut Cx<'_, '_>) -> Handled {
        self.taken()
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.taken()
    }

    fn on_gesture(&mut self, _: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        self.taken()
    }

    fn on_pointer(&mut self, _: &PointerEvent, _: &mut Cx<'_, '_>) -> Handled {
        self.taken()
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.wake(cx);
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
        self.wake(cx);
    }
}

impl Finishing {
    /// Input and closing taken (and so passed over) while it holds on.
    fn taken(&self) -> Handled {
        match self.holding {
            true => Handled::Yes,
            false => Handled::No,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finishing(from: u64, until: u64) -> Finishing {
        Finishing::new(
            &Params::new()
                .with(FROM, alloc::format!("{from}"))
                .with(UNTIL, alloc::format!("{until}")),
        )
    }

    #[test]
    fn the_bar_fills_in_steps_until_the_trial_ends() {
        let f = finishing(1_000, 31_000);
        assert_eq!(f.step(Instant(1_000)), 0);
        assert_eq!(f.step(Instant(16_000)), 5);
        assert_eq!(f.step(Instant(31_000)), STEPS);
        assert_eq!(f.step(Instant(99_000)), STEPS);
        assert_eq!(f.next(Instant(1_000)), Some(Instant(4_000)));
        assert_eq!(f.next(Instant(31_000)), None);
    }

    #[test]
    fn without_its_times_it_shows_the_bar_full() {
        let f = Finishing::new(&Params::new());
        assert_eq!(f.step(Instant(5)), STEPS);
        assert_eq!(f.next(Instant(5)), None);
        // Opened without a trial, it holds nothing.
        assert!(!f.holding);
        assert!(finishing(1_000, 31_000).holding);
    }
}
