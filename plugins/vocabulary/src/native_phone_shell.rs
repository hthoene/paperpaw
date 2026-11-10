//! The phone page in the shell (`vocabulary.phone`): serving while the screen
//! is open, the key and the address, and the requests of the page
//! (`native_phone_web`), each answered whatever it asks: nothing a phone on the
//! network sends may fail the plugin. Closing the screen ends the serving.
use alloc::string::String;

use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::net::{
    FileRoot, MAX_INLINE_BODY, Net, REQUEST_INPUT, Reason, Response, ServedRequest, Server,
    ServerEvent, ServerState,
};
use paperpaw_sdk::native::secrets::{Secrets, wipe};
use paperpaw_sdk::native::storage::{self, DataStorage, Root};
use paperpaw_sdk::native::ui::Instance;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::qr::QrCode;
use paperpaw_sdk::served::Guard;
use paperpaw_sdk::{Instant, Storage};
use paperpaw_ui_kit::files::replace;
use paperpaw_ui_kit::mem::{buffer, wipe_text};
use paperpaw_ui_kit::text::{concat, format};

use crate::declarations::PHONE_TOKEN;
use crate::files::{DataFolder, FOLDER};
use crate::native_phone::{PhoneView, State, reason_word};
use crate::native_phone_page;
use crate::native_phone_web::{ANSWER_FILE, Answer, Body, MAX_IMPORT, PAGE_FILE, Request, answer};
use crate::native_plugin::NativeVocabulary;
use crate::native_words::{HostWords, Words};
use crate::{cards, native_key};

/// The plugin's number for its one server, and the port asked for (the host
/// takes the next free one if it is in use).
const SERVER: u32 = 1;
const PORT: u16 = 8080;
/// What the host's `serve`, `stop_serving` and `respond` reply with, at most.
const SERVE_OUT: usize = 1_536;

/// A copy of `words` as the catalog gives them for a failure `reason`.
fn failed(host: &Host<'_>, reason: &str) -> Result<String, Error> {
    use paperpaw_sdk::native::formatting::Arg;
    HostWords::new(host)?.format(
        "vocabulary.phone.failed",
        &[Arg::Text {
            name: "reason",
            value: reason,
        }],
    )
}

/// Why the plugin's own start failed.
const fn word(error: Error) -> &'static str {
    match error {
        Error::Busy => "busy",
        Error::Denied => "not allowed",
        Error::Unsupported => "not supported",
        Error::Full | Error::NoSpace => "no room",
        _ => "failed",
    }
}

impl NativeVocabulary {
    /// Whether the phone's row is offered: where `core.net` is (the handle is
    /// kept for the screen).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn phone_offered(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        match self.net_handle(host) {
            Ok(_) => Ok(true),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(false),
        }
    }

    fn net_handle(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    /// The screen is opened: the decks as they are (the answers given in them,
    /// the words looked up as cards), and the screen saying it starts.
    pub(crate) fn opened_phone(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        let data = DataStorage::new(host);
        let storage = DataFolder(&data);
        let _ = cards::fold(&storage);
        let _ = cards::take_lookups(&storage, &crate::native_plugin::lookup_deck(host)?);
        let view = PhoneView::new(&mut HostWords::new(host)?)?;
        self.phones.open(instance, view)
    }

    /// The screen is shown the first time: the page is written and the server
    /// asked for (a server lives on a screen that shows, `network.md` 5.1).
    pub(crate) fn shown_phone(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.phones.shown(PHONE_TOKEN, instance)?;
        let Some(view) = self.phones.top(PHONE_TOKEN) else {
            return Ok(());
        };
        if view.started {
            return Ok(());
        }
        view.started = true;
        match self.start_phone(host) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(error) => {
                let words = failed(host, word(error))?;
                self.phone_show(host, State::Message(words))
            }
            Ok(()) => Ok(()),
        }
    }

    /// A key for this time of serving: none where the source was not fully
    /// random; an error where there is no source.
    fn draw_key(&mut self, host: &Host<'_>) -> Result<Option<String>, Error> {
        native_key::key(&mut |bytes: &mut [u8]| {
            if self.secrets.is_none() {
                self.secrets = Some(Secrets::open(host)?);
            }
            let secrets = self.secrets.as_ref().ok_or(Error::State)?;
            let (mut input, mut out) = (buffer(128)?, buffer(512)?);
            let drawn = secrets.random(host, bytes.len(), &mut input, &mut out);
            let strong = drawn.map(|drawn| {
                bytes.copy_from_slice(drawn.bytes());
                drawn.strong()
            });
            wipe(&mut out);
            strong
        })
    }

    /// Checks that a key can be drawn, writes the page and starts the server.
    fn start_phone(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // A device with no source of random numbers serves nothing: no weaker key.
        match self.draw_key(host) {
            Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                return self.phone_no_random(host);
            }
            Err(error) => return Err(error),
            Ok(key) => key.map_or((), wipe_text),
        }
        native_phone_page::write(host)?;
        let server = Server::new(SERVER, PORT)?.max_body(MAX_IMPORT)?;
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        self.net_handle(host)?
            .serve(host, &server, &mut input, &mut out)
    }

    fn phone_no_random(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let words = HostWords::new(host)?.copy("vocabulary.phone.no-random")?;
        self.phone_show(host, State::Message(words))
    }

    /// Shows `state` on the screen that is open, and draws it.
    fn phone_show(&mut self, host: &Host<'_>, state: State) -> Result<(), Error> {
        if let Some(view) = self.phones.top(PHONE_TOKEN) {
            view.show(&mut HostWords::new(host)?, state)?;
            redraw(host, RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The server started, failed or stopped.
    pub(crate) fn phone_server(
        &mut self,
        host: &Host<'_>,
        event: &ServerEvent<'_>,
    ) -> Result<(), Error> {
        if event.server() != SERVER {
            return Ok(());
        }
        let state = event.state();
        let serving = matches!(self.phones.top(PHONE_TOKEN), Some(view) if view.serving());
        match state {
            ServerState::Started { address } => self.phone_serving(host, address),
            ServerState::Failed { reason } => self.phone_not_serving(host, reason),
            // Asked for by the plugin: nothing to say. Ended by the network
            // while it served: the address on the screen is dead.
            ServerState::Stopped { reason } if serving && reason != Reason::Ok => {
                self.phone_not_serving(host, reason)
            }
            _ => Ok(()),
        }
    }

    /// The server runs at `address`: the key is drawn now (on the X4 the radio
    /// makes it fully random) and the address shown with it.
    fn phone_serving(&mut self, host: &Host<'_>, address: &str) -> Result<(), Error> {
        let key = match self.draw_key(host) {
            Ok(Some(key)) => key,
            Ok(None) | Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                self.phone_stop(host)?;
                return self.phone_no_random(host);
            }
            Err(error) => return Err(error),
        };
        let url = concat(&["http://", address, "/?k=", &key])?;
        let qr = QrCode::new(url.as_bytes());
        if let Some(view) = self.phones.top(PHONE_TOKEN) {
            view.guard = Some(Guard::new(key));
        }
        self.phone_show(host, State::Serving { url, qr })
    }

    fn phone_stop(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(net) = &self.net else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        match net.stop_serving(host, SERVER, &mut input, &mut out) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            _ => Ok(()),
        }
    }

    /// The server did not start or ended: no network is its own text.
    fn phone_not_serving(&mut self, host: &Host<'_>, reason: Reason) -> Result<(), Error> {
        if let Some(view) = self.phones.top(PHONE_TOKEN) {
            view.guard = None;
        }
        let words = if reason == Reason::NoNetwork {
            HostWords::new(host)?.copy("vocabulary.phone.no-network")?
        } else {
            failed(host, reason_word(reason))?
        };
        self.phone_show(host, State::Message(words))
    }

    /// The screen closed: the server stops (the host does it too; stopping is
    /// idempotent) and the page and the last long answer are removed.
    pub(crate) fn closed_phone(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        if self.phones.get(instance).is_none() {
            return Ok(());
        }
        self.phones.close(instance);
        self.phone_stop(host)?;
        for file in [PAGE_FILE, ANSWER_FILE] {
            if let Err(Error::NoMemory) = storage::remove(host, Root::Data, file) {
                return Err(Error::NoMemory);
            }
        }
        Ok(())
    }

    /// A request came to the page. Whatever it asks, the answer is sent and
    /// the callback ends well.
    pub(crate) fn phone_request(
        &mut self,
        host: &Host<'_>,
        request: &ServedRequest<'_>,
    ) -> Result<(), Error> {
        if request.server() != SERVER {
            return Ok(());
        }
        match self.phone_answer(host, request) {
            Ok(()) => Ok(()),
            Err(error) => {
                let status = if error == Error::NoMemory { 503 } else { 500 };
                let refused = Answer::text(status, "failed, try again");
                // Nothing more to do where even that is not sent.
                let _ = self.phone_send(host, request.request(), &refused);
                Ok(())
            }
        }
    }

    fn phone_answer(&mut self, host: &Host<'_>, request: &ServedRequest<'_>) -> Result<(), Error> {
        let data = DataStorage::new(host);
        let storage = DataFolder(&data);
        // The body the host saved (`.received/<server>/<request>`) is read
        // through the layer's paths, and removed once answered.
        let body = match request.body_len() {
            Some(_) => Some(format(format_args!(
                "{FOLDER}.received/{}/{}",
                request.server(),
                request.request()
            ))?),
            None => None,
        };
        let asked = Request {
            method: request.method(),
            target: request.target(),
            body: body.as_deref(),
            at: Instant(request.now_ms()),
        };
        let guard = self
            .phones
            .top(PHONE_TOKEN)
            .and_then(|view| view.guard.as_mut());
        let (answer, done) = match guard {
            Some(guard) => answer(&storage, guard, &asked),
            // Before the key was drawn (or after the screen closed): not served.
            None => (Answer::text(503, "not ready"), None),
        };
        if let Some(body) = &body {
            let _ = storage.remove(body);
        }
        self.phone_send(host, request.request(), &answer)?;
        if let Some(done) = done
            && let Some(view) = self.phones.top(PHONE_TOKEN)
        {
            view.say(&mut HostWords::new(host)?, &done)?;
            redraw(host, RedrawMode::Fast)?;
        }
        Ok(())
    }

    /// Sends `answer` to request `number`: inline where it fits, else from a
    /// file of the data folder.
    fn phone_send(&mut self, host: &Host<'_>, number: u32, answer: &Answer) -> Result<(), Error> {
        let mut response = Response::new(number, answer.status)?.content_type(answer.kind)?;
        response = match &answer.body {
            Body::Text(words) => response.body(words.as_bytes())?,
            Body::Owned(words) if words.len() <= MAX_INLINE_BODY => {
                response.body(words.as_bytes())?
            }
            Body::Owned(words) => {
                replace(host, Root::Data, ANSWER_FILE, words.as_bytes())?;
                response.send_file(FileRoot::Data, ANSWER_FILE)?
            }
            Body::Page => response.send_file(FileRoot::Data, PAGE_FILE)?,
        };
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(SERVE_OUT)?);
        match self
            .net_handle(host)?
            .respond(host, &response, &mut input, &mut out)
        {
            // Ended already (the phone went, or the 120 s passed).
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn redraw(host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(mode)
}
