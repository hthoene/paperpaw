//! What a part did with an input ([`Response`]), and whether a focus is
//! shown ([`focus_shown`]): the contract every part of the interface that
//! takes the buttons' actions (`on_action`) and gestures (`on_gesture`)
//! keeps.

use crate::env::Env;
use crate::keys::InputMode;

/// What a part did with an action or a gesture: not its own, taken with
/// nothing to draw, taken and changed (draw it again), or taken and done
/// with a result (a button chosen, a value kept).
///
/// ```
/// use paperpaw_sdk::ui::Response;
///
/// let moved: Response = Response::changed_if(true);
/// assert!(moved.taken() && moved.changed());
/// assert!(!Response::<()>::Ignored.taken());
/// ```
#[must_use = "an input the part did not take is the screen's to use"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Response<T = ()> {
    /// Not the part's: the screen may use it.
    #[default]
    Ignored,
    /// Taken; nothing changed (e.g. a step past the end).
    Same,
    /// Taken; the part changed: draw it again.
    Changed,
    /// Taken; the part is done with this result.
    Done(T),
}

impl<T> Response<T> {
    /// [`Response::Changed`] where `changed`, else [`Response::Same`].
    pub fn changed_if(changed: bool) -> Self {
        if changed { Self::Changed } else { Self::Same }
    }

    /// Whether the part took the input.
    pub fn taken(&self) -> bool {
        !matches!(self, Self::Ignored)
    }

    /// Whether the part changed and is to be drawn again.
    pub fn changed(&self) -> bool {
        matches!(self, Self::Changed)
    }
}

/// Whether a focus is shown: always on a device without touch, and on
/// one with touch once the buttons were used last
/// ([`Env::last_input`]). A finger points at what it means; a focus
/// would only stand in its way.
pub fn focus_shown(env: &Env<'_>) -> bool {
    !env.device().input.touch || env.last_input() == InputMode::Buttons
}
