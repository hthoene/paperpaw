//! The first-run setup: a short guide through the settings that matter,
//! one a page, each with a good default already chosen.
//! Plugins add its steps ([`Registrar::add_setup_step`]): a setting
//! to choose, or a screen of their own (e.g. Wi-Fi's networks), with
//! whether it is asked on this device. The guide is a screen bound to the
//! slot [`slots::SETUP`](crate::builtin::slots::SETUP), so it can be
//! replaced as a whole.
//!
//! [`Registrar::add_setup_step`]: crate::Registrar::add_setup_step

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::{Env, Id};

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

/// Whether a step is asked on this device.
pub type AppliesFn = Box<dyn Fn(&Env<'_>) -> bool>;

/// A step of the first-run setup.
#[non_exhaustive]
pub struct SetupStepSpec {
    pub id: Id,
    /// Translation key of what it asks; empty for the setting's title.
    pub title: Cow<'static, str>,
    /// Translation key of a short explanation; empty for the setting's
    /// description.
    pub help: Cow<'static, str>,
    /// Steps come in this order, smallest first: language 10, buttons 20,
    /// text size 30, time zone 40, Wi-Fi 50, keyboard 60, screen lock 70.
    pub order: i32,
    pub page: SetupPage,
    /// Asked only where this says so (e.g. Wi-Fi where PaperPaw runs the
    /// radio); always without it.
    pub applies: Option<AppliesFn>,
}

impl SetupStepSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, page: SetupPage) -> Self {
        Self {
            id,
            title: title.into(),
            page,
            help: Cow::Borrowed(""),
            order: 0,
            applies: None,
        }
    }

    pub fn help(mut self, help: impl Into<Cow<'static, str>>) -> Self {
        self.help = help.into();
        self
    }

    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    pub fn applies(mut self, applies: AppliesFn) -> Self {
        self.applies = Some(applies);
        self
    }
}

impl fmt::Debug for SetupStepSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupStepSpec")
            .field("id", &self.id)
            .field("order", &self.order)
            .field("page", &self.page)
            .finish_non_exhaustive()
    }
}

/// The steps every plugin added, in order.
#[derive(Debug, Default)]
pub struct SetupSteps {
    steps: Vec<SetupStepSpec>,
}

impl SetupSteps {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a step in its place.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, step: SetupStepSpec) {
        let at = self.steps.partition_point(|s| s.order <= step.order);
        self.steps.insert(at, step);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.steps.shrink_to_fit();
    }

    /// Every step, in order, whether or not it applies on this device
    /// (`core.registry` lists them with their applicability).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn iter(&self) -> impl Iterator<Item = &SetupStepSpec> {
        self.steps.iter()
    }

    /// The steps asked on this device, in order.
    pub fn applying<'a>(&'a self, env: &Env<'_>) -> Vec<&'a SetupStepSpec> {
        self.steps
            .iter()
            .filter(|s| s.applies.as_ref().is_none_or(|applies| applies(env)))
            .collect()
    }
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
