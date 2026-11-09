//! The interface-7 Wikipedia lookup: the summary screen, an overlay that
//! fills the slot `core.lookup-online` and fetches the summary through
//! `core.net`. The provider is built into the image but not submitted: the
//! host refuses lookup providers until the stage `core.lookup` is live and
//! answers a provider that must fetch only with the asynchronous stage reply
//! (`PORTING-7.md`). What the plugin decides without a host is in
//! `native_model` and `native_page`, tested without one.
use crate::{
    declarations::{FETCHED_TOKEN, SCREEN_TOKEN},
    native_view::View,
};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, kinds},
        net::Net,
        registrations,
        reply::Reply,
        ui::Ui,
    },
};
use paperpaw_ui_kit::{Screens, read_event};

#[expect(
    dead_code,
    reason = "the provider is in the image, submitted once the host admits lookup providers"
)]
mod image {
    include!(concat!(env!("OUT_DIR"), "/wikipedia_declarations.rs"));
}

/// The interface-7 Wikipedia lookup.
#[derive(Debug, Default)]
pub struct NativeWikipedia {
    ui: Option<Ui>,
    net: Option<Net>,
    screens: Screens<View>,
}

impl NativeWikipedia {
    /// The screen opened: its summary is asked for.
    fn open(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let mut view = View::open(host, ui, handover)?;
        if self.net.is_none() {
            // No network service at all (no radio, or the plugin is not allowed it).
            self.net = match Net::open(host) {
                Ok(net) => Some(net),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => None,
            };
        }
        match &self.net {
            Some(net) => view.fetch(host, net)?,
            None => view.offline(),
        }
        self.screens.open(instance, view)
    }

    /// A request ended: the screen waiting for it shows the answer.
    fn fetched(
        &mut self,
        host: &Host<'_>,
        done: &paperpaw_sdk::native::net::HttpDone,
    ) -> Result<(), Error> {
        let (Some(net), Some(view)) = (&self.net, self.screens.top(SCREEN_TOKEN)) else {
            return Ok(());
        };
        if !view.waits_for(done.id()) {
            return Ok(());
        }
        view.done(host, net, done)?;
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }
}

impl Plugin for NativeWikipedia {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREEN {
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
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::HttpDone
        ) else {
            return Ok(());
        };
        match event {
            Event::ScreenOpened {
                instance, handover, ..
            } => self.open(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.screens.shown(SCREEN_TOKEN, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                if self.screens.is_empty()
                    && let Some(net) = &self.net
                {
                    // A summary nobody waits for is not fetched on.
                    let (mut input, mut out) = (
                        paperpaw_ui_kit::mem::buffer(paperpaw_sdk::native::net::REQUEST_INPUT)?,
                        paperpaw_ui_kit::mem::buffer(paperpaw_sdk::native::net::REPLY)?,
                    );
                    net.cancel(host, FETCHED_TOKEN, &mut input, &mut out).ok();
                }
                Ok(())
            }
            Event::HttpDone(done) if token == FETCHED_TOKEN => self.fetched(host, &done),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        view.draw(host, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match self.screens.top(token) {
            Some(view) => view.input(host, &input),
            None => Ok(Handled::No),
        }
    }
}
