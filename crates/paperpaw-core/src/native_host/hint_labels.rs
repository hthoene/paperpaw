//! `command.set_hint` (1594, `docs/abi/ui.md` section 3): how a native screen
//! relabels, icons or hides the hint of the button bound to an action while it
//! shows (interface 6 `Screen::hint`).
//!
//! The labels live with the screen instance in the stack's host view
//! ([`StackRows`](super::stack_rows::StackRows)) and are dropped by its
//! `update` as soon as the instance is no longer on the stack. A command is
//! checked when it is submitted (kind, label, pinned actions, own top screen,
//! at most [`MAX_ACTIONS`] actions per instance) and its owned copy applied
//! after the callback, into room reserved when it was checked, so the
//! application cannot fail. Nothing here calls plugin code.
use alloc::{boxed::Box, vec::Vec};
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND,
};
use paperpaw_sdk::{Hint, builtin::actions};

/// Most actions one instance relabels.
pub(crate) const MAX_ACTIONS: usize = 8;
/// Longest action id (`command.set_hint.action`).
pub(crate) const ACTION_MAX: usize = 128;
/// Longest label (`command.set_hint.label`).
pub(crate) const LABEL_MAX: usize = 64;

/// The wire kinds of `command.set_hint`.
const DEFAULT: u32 = 0;
const WORDS: u32 = 1;
const ICON: u32 = 2;
const HIDDEN: u32 = 3;

/// The actions that are the user's way out: no plugin hides their hint
/// (`registry.md` section 4.2: the main menu's opener, the settings editor's
/// entry and the power actions; and *Back*).
const PINNED: [&str; 6] = [
    "core.back",
    "core.menu",
    "core.settings",
    "core.sleep",
    "core.power-off",
    "core.restart",
];

/// Whether `action` is one no plugin may hide.
pub(crate) fn pinned(action: &str) -> bool {
    PINNED.contains(&action) || action == actions::BACK.as_str()
}

/// What a label shows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shown {
    Words(Box<str>),
    Icon(&'static str),
    Hidden,
}

/// One checked `command.set_hint`, owned: applied after the callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HintSet {
    /// The screen instance (`ScreenKey` serial) it is for.
    pub(crate) instance: u32,
    action: Box<str>,
    /// `None`: the action's usual name again (kind 0).
    shown: Option<Shown>,
}

/// The icon a label names: a host icon (`paperpaw_sdk::icons`), else nothing.
fn icon_of(name: &str) -> Option<&'static str> {
    paperpaw_sdk::icons::names().find(|known| *known == name)
}

/// Checks the fields of a command.
///
/// # Errors
/// `ERR_INVALID` for another kind, an empty label of kinds 1 and 2, a label
/// of kinds 0 and 3, a text over its limit; `ERR_NOT_FOUND` for an unknown
/// icon; `ERR_DENIED` for hiding a pinned action; `ERR_NO_MEMORY`.
pub(crate) fn check(action: &str, kind: u32, label: &str) -> Result<(), i32> {
    if action.is_empty() || action.len() > ACTION_MAX || label.len() > LABEL_MAX {
        return Err(ERR_INVALID);
    }
    super::declarations::full_id(action)?;
    match kind {
        DEFAULT | HIDDEN if !label.is_empty() => Err(ERR_INVALID),
        WORDS | ICON if label.is_empty() => Err(ERR_INVALID),
        ICON => icon_of(label).map(|_| ()).ok_or(ERR_NOT_FOUND),
        HIDDEN if pinned(action) => Err(ERR_DENIED),
        DEFAULT | WORDS | HIDDEN => Ok(()),
        _ => Err(ERR_INVALID),
    }
}

/// Copies a checked command (see [`check`]), fallibly.
///
/// # Errors
/// What [`check`] says; `ERR_NO_MEMORY`.
pub(crate) fn make(instance: u32, action: &str, kind: u32, label: &str) -> Result<HintSet, i32> {
    check(action, kind, label)?;
    let shown = match kind {
        DEFAULT => None,
        WORDS => Some(Shown::Words(copy(label)?)),
        ICON => Some(Shown::Icon(icon_of(label).ok_or(ERR_NOT_FOUND)?)),
        _ => Some(Shown::Hidden),
    };
    Ok(HintSet {
        instance,
        action: copy(action)?,
        shown,
    })
}

fn copy(text: &str) -> Result<Box<str>, i32> {
    let mut owned = alloc::string::String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    Ok(owned.into_boxed_str())
}

/// One instance's labels: `(action, what it shows)`, at most 8.
type Labels = Vec<(Box<str>, Shown)>;

/// The labels of the open instances.
#[derive(Debug, Default)]
pub(crate) struct HintLabels {
    /// `(instance, its labels)`: at most one entry per instance on the stack.
    by_instance: Vec<(u32, Labels)>,
}

impl HintLabels {
    /// How many labels `instance` has room for without allocating.
    #[cfg(test)]
    fn room(&self, instance: u32) -> usize {
        self.by_instance
            .iter()
            .find(|(open, _)| *open == instance)
            .map_or(0, |(_, labels)| labels.capacity())
    }

    /// How many actions `instance` relabels now.
    #[cfg(test)]
    fn count(&self, instance: u32) -> usize {
        self.by_instance
            .iter()
            .find(|(open, _)| *open == instance)
            .map_or(0, |(_, labels)| labels.len())
    }

    /// Whether the action `action` (`removes`: kind 0) fits for `instance`
    /// after the `staged` sets of this callback: at most [`MAX_ACTIONS`]
    /// distinct actions, counting what it relabels already; a removal frees
    /// a place.
    ///
    /// # Errors
    /// `ERR_NO_SPACE` for a ninth action, `ERR_NO_MEMORY`.
    pub(crate) fn admits(
        &self,
        instance: u32,
        staged: &[HintSet],
        action: &str,
        removes: bool,
    ) -> Result<(), i32> {
        let mut names: Vec<&str> = Vec::new();
        names
            .try_reserve(MAX_ACTIONS + staged.len() + 1)
            .map_err(|_| ERR_NO_MEMORY)?;
        if let Some((_, held)) = self.by_instance.iter().find(|(open, _)| *open == instance) {
            names.extend(held.iter().map(|(name, _)| &**name));
        }
        let later = staged
            .iter()
            .filter(|set| set.instance == instance)
            .map(|set| (&*set.action, set.shown.is_none()))
            .chain(core::iter::once((action, removes)));
        for (name, removes) in later {
            if removes {
                names.retain(|held| *held != name);
            } else if !names.contains(&name) {
                names.push(name);
            }
        }
        if names.len() > MAX_ACTIONS {
            Err(ERR_NO_SPACE)
        } else {
            Ok(())
        }
    }

    /// Room for `sets` taken ahead (so [`Self::apply`] allocates nothing),
    /// for the instances of a batch that was checked.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`; nothing changes except spare capacity.
    pub(crate) fn reserve(&mut self, sets: &[HintSet]) -> Result<(), i32> {
        self.by_instance
            .try_reserve(sets.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        for set in sets {
            let at = match self
                .by_instance
                .iter()
                .position(|(open, _)| *open == set.instance)
            {
                Some(at) => at,
                None => {
                    self.by_instance.push((set.instance, Vec::new()));
                    self.by_instance.len() - 1
                }
            };
            // Up to the cap: a batch may add several actions for one instance.
            let labels = &mut self.by_instance[at].1;
            labels
                .try_reserve_exact(MAX_ACTIONS.saturating_sub(labels.len()))
                .map_err(|_| ERR_NO_MEMORY)?;
        }
        Ok(())
    }

    /// Applies checked sets (see [`Self::reserve`]): a second one for an
    /// action replaces the first, kind 0 removes.
    #[allow(
        dead_code,
        reason = "the command runner (shell_commands) calls it, wired with the shell"
    )]
    pub(crate) fn apply(&mut self, sets: Vec<HintSet>) {
        for set in sets {
            let Some(at) = self
                .by_instance
                .iter()
                .position(|(open, _)| *open == set.instance)
            else {
                continue;
            };
            let labels = &mut self.by_instance[at].1;
            let place = labels.iter().position(|(held, _)| *held == set.action);
            match (place, set.shown) {
                (Some(place), Some(shown)) => labels[place].1 = shown,
                (Some(place), None) => {
                    labels.swap_remove(place);
                }
                (None, Some(shown)) if labels.len() < MAX_ACTIONS => {
                    // Room was reserved when the command was checked.
                    labels.push((set.action, shown));
                }
                _ => {}
            }
        }
        self.by_instance.retain(|(_, labels)| !labels.is_empty());
    }

    /// What `instance` shows for `action`, if it relabels it.
    pub(crate) fn hint(&self, instance: u32, action: &str) -> Option<Hint> {
        let (_, labels) = self
            .by_instance
            .iter()
            .find(|(open, _)| *open == instance)?;
        let (_, shown) = labels.iter().find(|(held, _)| &**held == action)?;
        Some(match shown {
            Shown::Words(text) => {
                // A refusing heap shows the screen's own hint instead.
                let mut owned = alloc::string::String::new();
                owned.try_reserve_exact(text.len()).ok()?;
                owned.push_str(text);
                Hint::Text(owned)
            }
            Shown::Icon(icon) => Hint::Icon(icon),
            Shown::Hidden => Hint::Hidden,
        })
    }

    /// The stack changed: labels of instances no longer on it go.
    pub(crate) fn retain_open(&mut self, open: impl Fn(u32) -> bool) {
        self.by_instance.retain(|(instance, _)| open(*instance));
    }

    /// Whether any label is kept (tests and gauges).
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.by_instance.is_empty()
    }
}

#[cfg(test)]
#[path = "hint_labels_tests.rs"]
mod tests;
