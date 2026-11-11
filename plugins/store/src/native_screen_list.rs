//! The `list` screen of the store (stub: filled in by its slice of the port).
use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error,
    callbacks::{DrawFrame, Input},
};

use crate::native_screens::{Ctx, Next, Screen};

/// The screen.
#[derive(Debug, Default)]
pub struct View;

impl Screen for View {
    fn open(_: &Ctx<'_>, _: &[(&str, &str)]) -> Result<Self, Error> {
        Ok(Self)
    }

    fn draw(&mut self, _: &Ctx<'_>, _: &mut DrawFrame<'_>) -> Result<(), Error> {
        Ok(())
    }

    fn input(&mut self, _: &Ctx<'_>, _: &Input<'_>) -> Result<(Handled, Next), Error> {
        Ok((Handled::No, Next::None))
    }
}
