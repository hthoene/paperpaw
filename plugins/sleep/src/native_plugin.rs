//! The interface-7 sleep screen: one overlay screen (`sleep.screen`, the slot
//! `core.sleep`) over `core.ui`, with the widgets it composes
//! (`native_screen`, `native_stack`) and what it decides (`native_sleep`).
//!
//! The device waking closes it (`event.resume`); a screen still there when the
//! device is awake goes at the next press, so that it never keeps an awake
//! device to itself. While asleep every press but *sleep* and *home* is taken.
use crate::{
    declarations::{EVENTS, SCREEN},
    native_screen::Sleeping,
};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::{Event, Incoming, kinds},
        registrations,
        reply::Reply,
        ui::{HandoverId, Instance, Ui},
    },
};
use paperpaw_ui_kit::{Screens, read_event};

mod image {
    include!(concat!(env!("OUT_DIR"), "/sleep_declarations.rs"));
}

/// The interface-7 sleep plugin.
#[derive(Debug, Default)]
pub struct NativeSleep {
    ui: Option<Ui>,
    screens: Screens<Sleeping>,
}

impl Plugin for NativeSleep {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::Resume,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed
        ) else {
            return Ok(());
        };
        match event {
            Event::ScreenOpened {
                instance, handover, ..
            } if token == SCREEN => self.opened(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.shown(host, instance),
            Event::ScreenClosed { instance, .. } if token == SCREEN => {
                self.screens.close(instance);
                Ok(())
            }
            Event::Resume { .. } if token == EVENTS => self.woke(host),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token != SCREEN {
            return Err(Error::Invalid);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        self.screens
            .top(token)
            .ok_or(Error::State)?
            .draw(host, ui, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // Awake and still here: gone at the next press.
        if view.woke {
            close(host)?;
            return Ok(Handled::Yes);
        }
        // Nothing to do while asleep but sleep again or go home.
        Ok(match input {
            Input::Action {
                action: "core.sleep" | "core.home",
                ..
            } => Handled::No,
            _ => Handled::Yes,
        })
    }
}

/// Closes the screen on top; one that is not the plugin's any more is gone.
fn close(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    match CommandSender::new(host, &mut scratch).close_screen() {
        Ok(()) | Err(Error::State) => Ok(()),
        Err(error) => Err(error),
    }
}

impl NativeSleep {
    fn opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        // What cannot be read shows the message: a device that cannot show what
        // was asked still shows that it sleeps.
        let view = match Sleeping::open(host, ui, handover) {
            Ok(view) => view,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => Sleeping::plain(false),
        };
        self.screens.open(instance, view)
    }

    fn shown(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.screens.shown(SCREEN, instance)?;
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        // Shown again after waking, once what opened above it closed: it goes
        // too.
        if view.woke {
            return close(host);
        }
        let mut scratch = [0; 64];
        view.refresh(&mut CommandSender::new(host, &mut scratch))
    }

    /// The device woke: every open sleep screen goes.
    fn woke(&mut self, host: &Host<'_>) -> Result<(), Error> {
        for view in self.screens.all() {
            view.woke = true;
        }
        // Only closed, as in interface 6: no redraw of what is going.
        close(host)
    }
}
