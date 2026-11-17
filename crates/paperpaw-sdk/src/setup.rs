//! The first-run setup: a short guide through the settings that matter,
//! one a page, each with a good default already chosen.
//! Plugins add its steps (`Items::add_setup_step`): a setting
//! to choose, or a screen of their own (e.g. Wi-Fi's networks), with
//! whether it is asked on this device. The guide is a screen bound to the
//! slot [`slots::SETUP`](crate::builtin::slots::SETUP), so it can be
//! replaced as a whole.
//!
//! `Items::add_setup_step`: crate::Items::add_setup_step

use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;

use crate::Id;

/// What a step asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SetupPage {
    /// A setting, chosen on the guide's page (or in its editor).
    Setting(Id),
    /// A screen of the step's plugin, opened from the guide's page (e.g.
    /// the Wi-Fi networks to join).
    Screen(Id),
}







#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &'static str, order: i32) -> SetupStepSpec {
        SetupStepSpec {
            id: Id::new_static(id),
            title: Cow::Borrowed(""),
            help: Cow::Borrowed(""),
            order,
            page: SetupPage::Setting(Id::new_static(id)),
            applies: None,
        }
    }

    #[test]
    fn steps_keep_their_order() {
        let mut steps = SetupSteps::new();
        steps.add(step("a.late", 70));
        steps.add(step("a.early", 10));
        steps.add(step("a.middle", 40));
        steps.add(step("a.middle-too", 40));
        let ids: Vec<&str> = steps.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a.early", "a.middle", "a.middle-too", "a.late"]);
    }
}
