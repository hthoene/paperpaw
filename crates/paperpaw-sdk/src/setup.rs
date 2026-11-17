//! The first-run setup: a short guide through the settings that matter,
//! one a page, each with a good default already chosen.
//! Plugins add its steps (`Items::add_setup_step`): a setting
//! to choose, or a screen of their own (e.g. Wi-Fi's networks), with
//! whether it is asked on this device. The guide is a screen bound to the
//! slot [`slots::SETUP`](crate::builtin::slots::SETUP), so it can be
//! replaced as a whole.
//!
//! `Items::add_setup_step`: crate::Items::add_setup_step










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
