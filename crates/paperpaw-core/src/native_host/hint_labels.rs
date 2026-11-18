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
//!
//! **Memory.** Each label is charged to the submitter as
//! `action + words + 4` bytes (`docs/abi/ui.md` section 3), summed over all the
//! open instances of one plugin, against [`label_budget`]: a label over it is
//! `ERR_NO_MEMORY` and the earlier labels stay. The charge is derived from
//! the labels held, so replacing, clearing, an instance closing and a plugin
//! stopping (its instances leave the stack) release it with the labels.
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

/// What one label costs beyond its action id: the host's bookkeeping per label
/// (`docs/abi/ui.md` section 3: `128 + 64 + 4` at most).
const LABEL_OVERHEAD: usize = 4;

/// Most bytes of labels one plugin holds over all its open instances: three
/// instances of the per-instance bound (`8 * (128 + 64 + 4)`) on the small
/// class (the X4), twelve elsewhere. A stand-in until the plugin's own heap
/// budget (`abi/heap-word.md` section 4, frame-owner charging) exists in core:
/// then this is the one function that reads it instead.
pub(crate) const fn label_budget(small: bool) -> usize {
    const PER_INSTANCE: usize = MAX_ACTIONS * (ACTION_MAX + LABEL_MAX + LABEL_OVERHEAD);
    if small {
        3 * PER_INSTANCE
    } else {
        12 * PER_INSTANCE
    }
}

/// What a label shows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shown {
    Words(Box<str>),
    /// The icon's character: its name does not outlive the command.
    Icon(char),
    Hidden,
}

/// One checked `command.set_hint`, owned: applied after the callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HintSet {
    /// The screen instance (`ScreenKey` serial) it is for.
    pub(crate) instance: u32,
    /// The plugin that submitted it, whom it is charged to.
    owner: Box<str>,
    action: Box<str>,
    /// `None`: the action's usual name again (kind 0).
    shown: Option<Shown>,
}

impl HintSet {
    /// Bytes charged for the label this keeps (0 for a removal).
    fn charge(&self) -> usize {
        self.shown
            .as_ref()
            .map_or(0, |shown| charge_of(&self.action, shown))
    }
}

/// Bytes one held label is charged: its action id, the words, the overhead.
fn charge_of(action: &str, shown: &Shown) -> usize {
    let words = match shown {
        Shown::Words(text) => text.len(),
        Shown::Icon(_) | Shown::Hidden => 0,
    };
    action.len() + words + LABEL_OVERHEAD
}

/// The icon a label names: a host icon (`paperpaw_sdk::icons`) by its
/// name, not by a code point (`U+F8000`), else nothing.
fn icon_of(name: &str) -> Option<char> {
    if name.starts_with("U+") {
        return None;
    }
    paperpaw_sdk::icons::char(name)
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
pub(crate) fn make(
    owner: &str,
    instance: u32,
    action: &str,
    kind: u32,
    label: &str,
) -> Result<HintSet, i32> {
    check(action, kind, label)?;
    let shown = match kind {
        DEFAULT => None,
        WORDS => Some(Shown::Words(copy(label)?)),
        ICON => Some(Shown::Icon(icon_of(label).ok_or(ERR_NOT_FOUND)?)),
        _ => Some(Shown::Hidden),
    };
    Ok(HintSet {
        instance,
        owner: copy(owner)?,
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

/// One open instance's labels and the plugin they are charged to.
#[derive(Debug)]
struct Held {
    instance: u32,
    owner: Box<str>,
    labels: Labels,
}

impl Held {
    fn charged(&self) -> usize {
        self.labels
            .iter()
            .map(|(action, shown)| charge_of(action, shown))
            .sum()
    }
}

/// The labels of the open instances.
#[derive(Debug, Default)]
pub(crate) struct HintLabels {
    /// At most one entry per instance on the stack.
    by_instance: Vec<Held>,
}

impl HintLabels {
    fn of(&self, instance: u32) -> Option<&Held> {
        self.by_instance
            .iter()
            .find(|held| held.instance == instance)
    }

    /// How many labels `instance` has room for without allocating.
    #[cfg(test)]
    fn room(&self, instance: u32) -> usize {
        self.of(instance).map_or(0, |held| held.labels.capacity())
    }

    /// How many actions `instance` relabels now.
    #[cfg(test)]
    fn count(&self, instance: u32) -> usize {
        self.of(instance).map_or(0, |held| held.labels.len())
    }

    /// Bytes charged to the plugin `owner` for the labels it holds now.
    #[cfg(test)]
    pub(crate) fn charged(&self, owner: &str) -> usize {
        self.by_instance
            .iter()
            .filter(|held| &*held.owner == owner)
            .map(Held::charged)
            .sum()
    }

    /// Whether `set` (`shown` none: a removal) fits for its instance after the
    /// `staged` sets of this callback: at most [`MAX_ACTIONS`] distinct
    /// actions, counting what it relabels already (a removal frees a place),
    /// and the labels of all the owner's instances within `budget` bytes.
    ///
    /// # Errors
    /// `ERR_NO_SPACE` for a ninth action; `ERR_NO_MEMORY` over the budget (the
    /// labels held stay) or when the check itself finds no room.
    pub(crate) fn admits(
        &self,
        set: &HintSet,
        staged: &[HintSet],
        budget: usize,
    ) -> Result<(), i32> {
        // `(action, bytes charged)` of this instance after everything staged.
        let mut names: Vec<(&str, usize)> = Vec::new();
        names
            .try_reserve(MAX_ACTIONS + staged.len() + 1)
            .map_err(|_| ERR_NO_MEMORY)?;
        if let Some(held) = self.of(set.instance) {
            names.extend(
                held.labels
                    .iter()
                    .map(|(name, shown)| (&**name, charge_of(name, shown))),
            );
        }
        let later = staged
            .iter()
            .filter(|staged| staged.instance == set.instance)
            .chain(core::iter::once(set));
        for later in later {
            let name = &*later.action;
            names.retain(|(held, _)| *held != name);
            if later.shown.is_some() {
                names.push((name, later.charge()));
            }
        }
        if names.len() > MAX_ACTIONS {
            return Err(ERR_NO_SPACE);
        }
        let others: usize = self
            .by_instance
            .iter()
            .filter(|held| held.instance != set.instance && held.owner == set.owner)
            .map(Held::charged)
            .sum();
        let mine: usize = names.iter().map(|(_, bytes)| bytes).sum();
        if others.saturating_add(mine) > budget {
            return Err(ERR_NO_MEMORY);
        }
        Ok(())
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
                .position(|held| held.instance == set.instance)
            {
                Some(at) => at,
                None => {
                    self.by_instance.push(Held {
                        instance: set.instance,
                        owner: copy(&set.owner)?,
                        labels: Vec::new(),
                    });
                    self.by_instance.len() - 1
                }
            };
            // Up to the cap: a batch may add several actions for one instance.
            let labels = &mut self.by_instance[at].labels;
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
                .position(|held| held.instance == set.instance)
            else {
                continue;
            };
            let labels = &mut self.by_instance[at].labels;
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
        self.by_instance.retain(|held| !held.labels.is_empty());
    }

    /// What `instance` shows for `action`, if it relabels it.
    pub(crate) fn hint(&self, instance: u32, action: &str) -> Option<Hint> {
        let (_, shown) = self
            .of(instance)?
            .labels
            .iter()
            .find(|(held, _)| &**held == action)?;
        Some(match shown {
            Shown::Words(text) => {
                // A refusing heap shows the screen's own hint instead.
                let mut owned = alloc::string::String::new();
                owned.try_reserve_exact(text.len()).ok()?;
                owned.push_str(text);
                Hint::Text(owned)
            }
            // As a text the row shows as the icon; no memory, the screen's own.
            Shown::Icon(icon) => Hint::Text(paperpaw_sdk::icons::as_text(*icon)?),
            Shown::Hidden => Hint::Hidden,
        })
    }

    /// The stack changed: labels of instances no longer on it go, and with
    /// them the charge of a plugin that stopped (its instances left the stack).
    pub(crate) fn retain_open(&mut self, open: impl Fn(u32) -> bool) {
        self.by_instance.retain(|held| open(held.instance));
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
