//! A yes or no question in a box in the middle of the screen, e.g.
//! whether to delete a file: the screen of slot
//! [`CONFIRM`](crate::builtin::slots::CONFIRM), which
//! [`ask::confirm`](crate::ask::confirm) asks. The answer comes back as
//! the plugin's reply event, a [`Confirmed`](crate::replies::Confirmed) with the context it was
//! asked with. *Back*, or a tap outside the box, answers nothing. On a
//! screen wider than the readable column (`ui::readable`)
//! the box is as wide as the column at most, and what lies beside it is
//! outside.
//!
//! A screen of one's own bound to the slot reads the question from
//! [`params::TITLE`](crate::builtin::params::TITLE), [`params::TEXT`](crate::builtin::params::TEXT) (a line of text below it, may be
//! missing), [`YES_LABEL`] and [`NO_LABEL`] (the buttons' words, the
//! kernel's *Yes* and *No* where missing), [`FOCUS_YES`] (set: the focus
//! starts on yes, else on no) and [`params::REPLY`](crate::builtin::params::REPLY), and answers with
//! [`Confirmed::new`](crate::replies::Confirmed::new) and all of the parameters it was opened with.

/// The answer in a [`Confirmed`](crate::replies::Confirmed)'s parameters: `yes` or `no`.
pub(crate) const ANSWER: &str = "answer";
