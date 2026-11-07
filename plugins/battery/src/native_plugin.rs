//! The flat Battery plugin. Its state owns data, never callback-scoped handles.
use crate::{native_view::NativeView, recording::Recorder};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::Event,
        registrations,
        reply::Reply,
    },
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/battery_declarations.rs"));
}

/// Battery's interface-7 entry state.
#[derive(Debug, Default)]
pub struct NativeBattery {
    recorder: Recorder,
    view: Option<NativeView>,
}

impl Plugin for NativeBattery {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::UI).chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::ScreenShown {
                screen: "battery.screen",
                ..
            } if token == 10 => {
                self.view = Some(NativeView::new(host, self.recorder.reading(host)?)?);
                Ok(())
            }
            Event::ActionRun {
                action: "battery.open",
                ..
            } if token == 11 => {
                let mut scratch = [0; 256];
                CommandSender::new(host, &mut scratch).open_screen("battery.screen")
            }
            _ if token == 1 => self.recorder.event(host, event),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token != 10 {
            return Err(Error::Invalid);
        }
        self.view
            .as_mut()
            .ok_or(Error::State)?
            .draw(host, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token != 10 {
            return Ok(Handled::No);
        }
        match self.view.as_mut() {
            Some(view) => view.input(host, input),
            None => Ok(Handled::No),
        }
    }
}
