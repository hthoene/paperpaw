//! Holding a row with the buttons alone: Select held and the menu button do
//! what a long press does on a touch screen, in every list whose rows offer
//! more when held.
use super::{ItemList, ItemStep};
use paperpaw_sdk::native::{Error, Host, callbacks::Input};

impl ItemList {
    /// [`Self::input`] for a list whose rows offer more when held: where
    /// `holds` (the row in focus offers more), Select held
    /// (`core.long-confirm`) and the menu button (`core.menu`) hold that row,
    /// as a long press does on a touch screen; with the buttons alone nothing
    /// else can. Elsewhere they stay the host's (its menu).
    ///
    /// # Errors
    /// As [`Self::input`].
    pub fn input_holding(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
        holds: bool,
    ) -> Result<ItemStep, Error> {
        match self.held_by_button(input, holds) {
            Some(step) => Ok(step),
            None => self.input(host, input),
        }
    }

    /// What Select held and the menu button mean to a list whose row in
    /// focus offers more where `holds`; `None` for every other input.
    pub(super) fn held_by_button(&self, input: &Input<'_>, holds: bool) -> Option<ItemStep> {
        let button = matches!(
            input,
            Input::Action {
                action: "core.long-confirm" | "core.menu",
                ..
            }
        );
        (button && holds && self.choosable(self.focus)).then_some(ItemStep::Held(self.focus))
    }
}
