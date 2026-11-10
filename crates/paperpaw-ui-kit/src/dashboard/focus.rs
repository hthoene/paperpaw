//! The focus of the dashboard with the buttons, without the host: the targets of
//! the page (its tools, what its widgets answer, the footer's buttons) in
//! reading order, the first press that only shows where the focus is, and the
//! moves between them (the same rules as the interface-6 grid).
use paperpaw_sdk::{Rect, ui::step_focus};

/// A button below the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Button {
    /// The pencil (*Done* while editing).
    Pencil,
    /// *Add*, while editing.
    Plus,
    /// The footer's action of this number.
    Action(usize),
}

/// Whose a target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// The tool of this number on the page.
    Tool(usize),
    /// What the widget of this number on the page answered (`target` 0: the
    /// widget as a whole).
    Widget { item: usize, target: u32 },
    /// A button.
    Button(Button),
}

/// A place the focus can stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub rect: Rect,
    pub owner: Owner,
}

/// A button press the focus takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Up,
    Down,
    Left,
    Right,
    Next,
    Previous,
    Confirm,
}

impl Press {
    /// The press an action name stands for.
    #[must_use]
    pub fn of(action: &str) -> Option<Self> {
        Some(match action {
            "core.up" => Self::Up,
            "core.down" => Self::Down,
            "core.left" => Self::Left,
            "core.right" => Self::Right,
            "core.next" => Self::Next,
            "core.previous" => Self::Previous,
            "core.confirm" => Self::Confirm,
            _ => return None,
        })
    }
}

/// What a press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Not for the focus.
    Ignored,
    /// Taken; nothing moved (no targets, or nowhere to go).
    Held,
    /// The focus is shown where it was (the first press).
    Shown(usize),
    /// The focus moved to this target.
    Moved(usize),
    /// *Confirm*: run this target.
    Run(usize),
}

/// The focus: whether it is shown, and which target it stands on.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Focus {
    shown: bool,
    at: usize,
}

impl Focus {
    /// The target it stands on.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// Whether it is drawn.
    #[must_use]
    pub const fn shown(&self) -> bool {
        self.shown
    }

    /// Drawn or not from now on (the reader chose to see it at once).
    pub fn show(&mut self, shown: bool) {
        self.shown = shown;
    }

    /// On the target `at`, shown (an item picked up to be moved).
    pub fn to(&mut self, at: usize) {
        self.at = at;
        self.shown = true;
    }

    /// A page turned: it goes back to the first target.
    pub fn reset(&mut self) {
        self.at = 0;
    }

    /// Kept inside `len` targets after they changed.
    pub fn clamp(&mut self, len: usize) {
        self.at = self.at.min(len.saturating_sub(1));
    }

    /// Takes a press. `stays` is the reader's choice that the arrow keys stay
    /// on the page: past its first or last row they go round instead of
    /// stopping.
    pub fn press(&mut self, press: Press, targets: &[Target], stays: bool) -> Step {
        if !self.shown {
            // The first press shows where the focus is.
            self.shown = true;
            self.clamp(targets.len());
            return Step::Shown(self.at);
        }
        self.clamp(targets.len());
        if targets.is_empty() {
            return Step::Held;
        }
        let len = targets.len();
        let to = match press {
            Press::Confirm => return Step::Run(self.at),
            Press::Right | Press::Next => step_focus(self.at, 1, len),
            Press::Left | Press::Previous => step_focus(self.at, -1, len),
            Press::Up | Press::Down => {
                match vertical(targets, self.at, press == Press::Down, stays) {
                    Some(to) => to,
                    None => return Step::Held,
                }
            }
        };
        self.at = to;
        Step::Moved(to)
    }
}

/// The target nearest below (or above) the one at `from`; with `round` (the
/// arrow keys stay on the page), where there is none, the one nearest the top
/// (or bottom) edge below (or above) it.
fn vertical(targets: &[Target], from: usize, down: bool, round: bool) -> Option<usize> {
    let at = targets.get(from)?.rect;
    let cx = i32::from(at.x) + i32::from(at.width) / 2;
    let nearest = |past: &dyn Fn(&Rect) -> bool, line: i32| {
        targets
            .iter()
            .enumerate()
            .filter(|(_, t)| past(&t.rect))
            .min_by_key(|(_, t)| {
                let dy = (i32::from(t.rect.y) - line).abs();
                let dx = (i32::from(t.rect.x) + i32::from(t.rect.width) / 2 - cx).abs();
                dy * 4 + dx
            })
            .map(|(i, _)| i)
    };
    let next = if down {
        nearest(&|r| r.y >= at.y + at.height, i32::from(at.y))
    } else {
        nearest(&|r| r.y + r.height <= at.y, i32::from(at.y))
    };
    if next.is_some() || !round {
        return next;
    }
    // Round: from past the other edge, the row nearest it.
    let line = if down {
        0
    } else {
        targets.iter().map(|t| i32::from(t.rect.y)).max()?
    };
    nearest(&|_| true, line).filter(|&i| i != from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows of two tools and the pencil below.
    fn targets() -> [Target; 5] {
        let tool = |n: usize, x: u16, y: u16| Target {
            rect: Rect::new(x, y, 100, 100),
            owner: Owner::Tool(n),
        };
        [
            tool(0, 0, 0),
            tool(1, 100, 0),
            tool(2, 0, 100),
            tool(3, 100, 100),
            Target {
                rect: Rect::new(100, 300, 60, 60),
                owner: Owner::Button(Button::Pencil),
            },
        ]
    }

    #[test]
    fn the_first_press_only_shows_the_focus() {
        let mut focus = Focus::default();
        assert_eq!(focus.press(Press::Right, &targets(), false), Step::Shown(0));
        assert!(focus.shown());
        assert_eq!(focus.press(Press::Right, &targets(), false), Step::Moved(1));
        // Shown at once (the reader's choice): no first press.
        let mut at_once = Focus::default();
        at_once.show(true);
        assert_eq!(
            at_once.press(Press::Right, &targets(), false),
            Step::Moved(1)
        );
    }

    #[test]
    fn left_and_right_go_round_at_the_ends() {
        let t = targets();
        let mut focus = Focus::default();
        focus.show(true);
        assert_eq!(focus.press(Press::Left, &t, false), Step::Moved(4));
        assert_eq!(focus.press(Press::Next, &t, false), Step::Moved(0));
        assert_eq!(focus.press(Press::Previous, &t, false), Step::Moved(4));
    }

    #[test]
    fn up_and_down_go_to_the_nearest_target_in_that_direction() {
        let t = targets();
        let mut focus = Focus::default();
        focus.show(true);
        assert_eq!(focus.press(Press::Down, &t, false), Step::Moved(2));
        assert_eq!(focus.press(Press::Right, &t, false), Step::Moved(3));
        // Below the second row: the pencil.
        assert_eq!(focus.press(Press::Down, &t, false), Step::Moved(4));
        // Nothing below: it stays.
        assert_eq!(focus.press(Press::Down, &t, false), Step::Held);
        assert_eq!(focus.at(), 4);
        assert_eq!(focus.press(Press::Up, &t, false), Step::Moved(3));
    }

    #[test]
    fn where_the_arrows_stay_on_the_page_up_and_down_go_round() {
        let t = targets();
        let mut focus = Focus::default();
        focus.show(true);
        focus.at = 4;
        assert_eq!(focus.press(Press::Down, &t, true), Step::Moved(1));
        // From the top, up goes to the bottom.
        assert_eq!(focus.press(Press::Up, &t, true), Step::Moved(4));
    }

    #[test]
    fn up_and_down_go_round_at_the_page_edges_only_where_the_arrows_stay_on_it() {
        let t = targets();
        for (stays, top, bottom) in [
            (true, Step::Moved(4), Step::Moved(1)),
            (false, Step::Held, Step::Held),
        ] {
            // From the top row, up.
            let mut focus = Focus::default();
            focus.show(true);
            assert_eq!(focus.press(Press::Up, &t, stays), top, "up, stays {stays}");
            // From the bottom (the pencil), down.
            let mut focus = Focus::default();
            focus.show(true);
            focus.at = 4;
            assert_eq!(
                focus.press(Press::Down, &t, stays),
                bottom,
                "down, stays {stays}"
            );
            if !stays {
                // It did not move: the page buttons turn the page, not these.
                assert_eq!(focus.at(), 4);
            }
        }
    }

    #[test]
    fn an_item_picked_up_has_the_focus_shown_on_it() {
        let t = targets();
        let mut focus = Focus::default();
        focus.to(3);
        assert!(focus.shown());
        assert_eq!(focus.at(), 3);
        // The next press moves on from there (no first press that only shows).
        assert_eq!(focus.press(Press::Left, &t, false), Step::Moved(2));
    }

    #[test]
    fn confirm_runs_the_target_the_focus_stands_on() {
        let t = targets();
        let mut focus = Focus::default();
        focus.show(true);
        focus.press(Press::Right, &t, false);
        assert_eq!(focus.press(Press::Confirm, &t, false), Step::Run(1));
    }

    #[test]
    fn no_targets_hold_the_focus_and_a_page_turn_resets_it() {
        let mut focus = Focus::default();
        focus.show(true);
        assert_eq!(focus.press(Press::Right, &[], false), Step::Held);
        let t = targets();
        focus.press(Press::Right, &t, false);
        focus.press(Press::Right, &t, false);
        assert_eq!(focus.at(), 2);
        focus.reset();
        assert_eq!(focus.at(), 0);
        // Fewer targets than before: kept inside them.
        focus.at = 4;
        assert_eq!(focus.press(Press::Confirm, &t[..2], false), Step::Run(1));
    }

    #[test]
    fn the_actions_of_the_buttons_are_named() {
        assert_eq!(Press::of("core.confirm"), Some(Press::Confirm));
        assert_eq!(Press::of("core.left"), Some(Press::Left));
        assert_eq!(Press::of("core.next-page"), None);
        assert_eq!(Press::of("home.edit"), None);
    }
}
