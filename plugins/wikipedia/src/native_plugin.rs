//! The interface-7 Wikipedia lookup: the summary screen, an overlay that
//! fills the slot `core.lookup-online` and fetches the summary through
//! `core.net`, and the lookup provider beside the dictionaries, which answers
//! *pending*, fetches and tells the askers when its answer is kept
//! (`native_provider`). What the plugin decides without a host is in
//! `native_model` and `native_page`, tested without one.
use crate::{
    declarations::{ABOUT_TOKEN, FETCHED_TOKEN, PROVIDER_TOKEN, SCREEN_TOKEN},
    native_provider::Provider,
    native_view::View,
    stages::about_lines_v1 as lines,
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

mod image {
    include!(concat!(env!("OUT_DIR"), "/wikipedia_declarations.rs"));
}

/// The stage a lookup provider is called through.
const LOOKUP_STAGE: &str = paperpaw_sdk::native::stage_schemas::core_lookup_v1::KEY;

/// The interface-7 Wikipedia lookup.
#[derive(Debug, Default)]
pub struct NativeWikipedia {
    ui: Option<Ui>,
    net: Option<Net>,
    screens: Screens<View>,
    provider: Provider,
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
        // The whole summary the provider kept for this word (the entry the
        // panel showed was cut to 2,048 bytes, the hand-over to 1,024).
        if let Some((title, text)) = self.provider.summary_for(view.word())? {
            view.show(title, text);
        }
        if view.is_loading() {
            self.ensure_net(host)?;
            match &self.net {
                Some(net) => view.fetch(host, net)?,
                None => view.offline(),
            }
        }
        self.screens.open(instance, view)
    }

    /// Opens the network service at first use; it stays `None` where there
    /// is none (no radio, or the plugin is not allowed it).
    fn ensure_net(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.net.is_none() {
            self.net = match Net::open(host) {
                Ok(net) => Some(net),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => None,
            };
        }
        Ok(())
    }

    /// A request ended: the screen waiting for it shows the answer.
    fn fetched(
        &mut self,
        host: &Host<'_>,
        done: &paperpaw_sdk::native::net::HttpDone,
    ) -> Result<(), Error> {
        let Some(net) = &self.net else {
            return Ok(());
        };
        if self.provider.done(host, net, done)? {
            return Ok(());
        }
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
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

impl NativeWikipedia {
    /// Answers `about.lines` with the line that names the source and the
    /// licence (CC BY-SA asks for both).
    fn about(host: &Host<'_>, payload: &[u8], reply: &mut Reply<'_>) -> Result<(), Error> {
        // An ask that is not one of the stage's is not answered.
        lines::input_decode(payload).map_err(|_| Error::Invalid)?;
        let text = paperpaw_ui_kit::Messages::new()?.copy(host, "wikipedia.about")?;
        let mut out = paperpaw_ui_kit::mem::buffer(lines::MAX_REPLY)?;
        let length = lines::reply_encode(
            &[lines::Record::LinesLine(lines::LinesLine::new(&text))],
            &mut out,
        )
        .map_err(|_| Error::Invalid)?;
        reply.write(&out[..length])
    }
}

impl Plugin for NativeWikipedia {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREEN
            .iter()
            .chain(image::PROVIDER)
            .chain(image::ABOUT)
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
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::HttpDone,
            kinds::StageCall
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
            Event::StageCall { stage, payload, .. }
                if token == ABOUT_TOKEN && stage == lines::KEY =>
            {
                Self::about(host, payload, reply)
            }
            Event::StageCall { stage, payload, .. }
                if token == PROVIDER_TOKEN && stage == LOOKUP_STAGE =>
            {
                self.ensure_net(host)?;
                self.provider.ask(host, self.net.as_ref(), payload, reply)
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        view.draw(host, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match self.screens.top(token) {
            Some(view) => view.input(host, &input, self.net.as_ref()),
            None => Ok(Handled::No),
        }
    }
}
