//! What the guide holds and decides, without a host: the steps, the page,
//! what has the focus, and how a number or a choice moves.
use alloc::{string::String, vec::Vec};

/// What a step opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// A setting, asked on the page itself.
    Setting,
    /// A screen of its plugin, opened from the page.
    Screen,
}

/// One step as the registry lists it (those that apply now only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    pub kind: PageKind,
    /// The setting or the screen.
    pub page: String,
    /// Translated by the host.
    pub title: String,
}

/// The kinds of setting a page can ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int,
    /// A choice, also a language.
    Choice,
    Other,
}

/// A value as the registry holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Val {
    Bool(bool),
    Int(i64),
    Text(String),
}

/// The setting a page asks, read with its range and choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub key: String,
    pub kind: Kind,
    pub title: String,
    pub help: String,
    pub value: Option<Val>,
    pub min: i64,
    pub max: i64,
    pub step: i64,
    /// The unit's key, e.g. `core.unit.pt`.
    pub unit: String,
    /// Value and translated name of each option.
    pub choices: Vec<(String, String)>,
    /// A setting that has its own editor: not asked on the page.
    pub editor: bool,
}

/// What can be chosen on a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The answer's card.
    Card,
    Next,
    /// *Skip* on the first page, *Back* on the others.
    Other,
}

/// What turning to a page does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// The same page stays.
    Stay,
    /// Page `n` shows (the last, one past the steps, is the end).
    To(usize),
    /// Past the end: the guide is done.
    Finish,
}

/// Where a turn to `page` (may be below 0) goes, with `steps` steps.
#[must_use]
pub fn turn(page: isize, steps: usize) -> Turn {
    if page < 0 {
        return Turn::Stay;
    }
    let page = page as usize;
    if page > steps {
        Turn::Finish
    } else {
        Turn::To(page)
    }
}

/// The targets from top to bottom: the card where the page has one.
#[must_use]
pub fn targets(card: bool) -> &'static [Target] {
    if card {
        &[Target::Card, Target::Next, Target::Other]
    } else {
        &[Target::Next, Target::Other]
    }
}

/// The index after `delta` steps from `at` among `len`, around the ends.
#[must_use]
pub fn step_focus(at: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (at as isize + delta).rem_euclid(len as isize) as usize
}

impl Setting {
    /// The two options of a choice of two, as the card shows them side by side.
    #[must_use]
    pub fn two(&self) -> Option<[&(String, String); 2]> {
        match (self.kind, self.choices.as_slice()) {
            (Kind::Choice, [a, b]) if !self.editor => Some([a, b]),
            _ => None,
        }
    }

    /// The number moved by `steps` of its step, within its range; `None`
    /// where it is not a number, or does not move.
    #[must_use]
    pub fn moved(&self, steps: i64) -> Option<i64> {
        let Some(Val::Int(now)) = self.value else {
            return None;
        };
        if self.kind != Kind::Int {
            return None;
        }
        let step = self.step.max(1);
        let next = now
            .saturating_add(step.saturating_mul(steps))
            .clamp(self.min.min(self.max), self.max.max(self.min));
        (next != now).then_some(next)
    }

    /// The value of the next option, around the end (the last way to choose
    /// where there is no list to open).
    #[must_use]
    pub fn next_choice(&self) -> Option<&str> {
        let Some(Val::Text(now)) = &self.value else {
            return None;
        };
        let at = self.choices.iter().position(|(value, _)| value == now);
        let next = at.map_or(0, |at| (at + 1) % self.choices.len().max(1));
        self.choices.get(next).map(|(value, _)| value.as_str())
    }

    /// The name of the value held, as the card shows it.
    #[must_use]
    pub fn value_name(&self) -> Option<String> {
        match &self.value {
            Some(Val::Text(value)) => self
                .choices
                .iter()
                .find(|(v, _)| v == value)
                .map(|(_, name)| name.clone())
                .or_else(|| Some(value.clone())),
            _ => None,
        }
    }

    /// The size of the reading text in points where the number is one.
    #[must_use]
    pub fn sample_points(&self) -> Option<i64> {
        match (&self.value, self.unit.as_str()) {
            (Some(Val::Int(points)), "core.unit.pt") => Some(*points),
            _ => None,
        }
    }
}

/// The guide's state.
#[derive(Debug)]
pub struct Guide {
    pub steps: Vec<Step>,
    /// The page shown; one past the last step is the end.
    pub page: usize,
    pub focus: Target,
    /// Whether the focus shows (once a button was used; at once without touch).
    pub shown: bool,
    /// The number being changed with the buttons.
    pub editing: bool,
    /// The setting of the page, if it asks one.
    pub setting: Option<Setting>,
}

impl Guide {
    #[must_use]
    pub fn new(steps: Vec<Step>, shown: bool) -> Self {
        Self {
            steps,
            page: 0,
            focus: Target::Next,
            shown,
            editing: false,
            setting: None,
        }
    }

    /// The step of the page shown; `None` at the end.
    #[must_use]
    pub fn step(&self) -> Option<&Step> {
        self.steps.get(self.page)
    }

    #[must_use]
    pub fn at_end(&self) -> bool {
        self.page >= self.steps.len()
    }

    /// Whether the page has a card for the answer.
    #[must_use]
    pub fn has_card(&self) -> bool {
        self.step().is_some()
    }

    /// A new list of steps (a step came or went): the page shown stays
    /// where the step it was on still is.
    pub fn replace_steps(&mut self, steps: Vec<Step>) {
        let shown = self.step().map(|s| s.id.clone());
        self.steps = steps;
        self.page = shown
            .and_then(|id| self.steps.iter().position(|s| s.id == id))
            .unwrap_or(self.page.min(self.steps.len()));
    }

    /// Moves the focus by `delta`; the first use only shows it.
    pub fn move_focus(&mut self, delta: isize) {
        if !self.shown {
            self.shown = true;
            return;
        }
        let list = targets(self.has_card());
        let at = list.iter().position(|t| *t == self.focus).unwrap_or(0);
        self.focus = list[step_focus(at, delta, list.len())];
    }

    /// Goes to `page`, with the focus on *Next*.
    pub fn go(&mut self, page: usize) {
        self.page = page;
        self.focus = Target::Next;
        self.editing = false;
        self.setting = None;
    }
}

#[cfg(test)]
mod tests;
