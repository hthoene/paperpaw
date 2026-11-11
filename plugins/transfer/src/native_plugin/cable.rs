//! The screen `transfer.cable` (slot `core.cable`): the host opens it when a
//! computer talks over the USB cable and no server runs. While it shows, the
//! plugin serves port 0 (the cable) and answers the requests as it answers
//! the page's, with `serve` and `respond`; the rules are `native_cable`'s. The
//! reader is asked with a confirm question (`core.ui` `ask`, the answer is
//! `event.answered`) before anything is written. *Back* closes the screen and
//! ends the permission, and so does a time without a request (an exact timer
//! of `core.work`). A `respond` the host cannot take yet (`Busy`) is kept and
//! tried again on the next callback or the retry timer.
use super::{NativeTransfer, SERVE_OUT, failed, handle, reason_word, redraw, word};
use crate::{
    declarations::{CABLE, SERVED_REQUEST, SERVER_EVENT, WORK},
    native_cable::{
        self, Answer, Cable, Call, Handles, HostCable, IDLE_MS, Permission, Reply, Said,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        device::{DeviceOutcome, InstallKind},
        events::{AnswerOutcome, Answered, Event, InstallDone},
        formatting::Arg,
        net::{
            MAX_SERVED_BODY, Net, REQUEST_INPUT, Response, ServedRequest, Server, ServerEvent,
            ServerState,
        },
        ui::{Instance, QuestionKind},
        work::WorkSpec,
    },
};
use paperpaw_ui_kit::{
    Footer, ItemList, Messages,
    mem::buffer,
    text::{concat, format},
};

/// The plugin's number for the cable's server.
pub(super) const CABLE_SERVER: u32 = 3;
/// The key of the timer that ends a screen nobody talks to.
const IDLE_KEY: &str = "transfer.cable-idle";
/// The key of the timer that tries a refused answer again.
const RETRY_KEY: &str = "transfer.cable-retry";
/// The wait before an answer the host could not take is sent again.
const RETRY_MS: u64 = 250;
/// Answers kept for another try at most (the host takes at most 8 of its own).
const MOST_PENDING: usize = 4;
/// Tries an answer gets (some 100 s: the host ends the request at 120 s).
const MOST_TRIES: u16 = 400;

/// An answer the host refused as busy.
#[derive(Debug)]
struct Pending {
    number: u32,
    reply: Reply,
    tries: u16,
}

/// The open screen of the cable.
#[derive(Debug)]
pub(super) struct CableScreen {
    instance: Instance,
    /// The empty list under the header: its back arrow and its draw.
    list: ItemList,
    state: Cable,
    /// Whether serving was asked for (once for each time the screen opens).
    started: bool,
    /// Whether the cable's server runs, and whether a computer has spoken.
    running: bool,
    spoken: bool,
    /// Why the server did not start or stopped, in the reader's words.
    failed: Option<String>,
    pending: Vec<Pending>,
    /// The host's clock as of the last event that said it.
    clock: u64,
}

/// What a send came to.
enum Sent {
    Done,
    Busy,
}

impl NativeTransfer {
    /// What the cable does with an event: `None` where it is not the cable's
    /// (the plugin's other screens go on with it).
    pub(super) fn cable_event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: &Event<'_>,
    ) -> Option<Result<(), Error>> {
        Some(match event {
            Event::Answered { answer, .. } => self.cable_answered(host, answer),
            Event::ScreenOpened { instance, .. } if token == CABLE => {
                self.cable_opened(host, *instance)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == CABLE => self.cable_shown(host, *instance),
            Event::ScreenClosed { instance, .. } if token == CABLE => {
                self.cable_closed(host, *instance)
            }
            Event::Server(event) if token == SERVER_EVENT && event.server() == CABLE_SERVER => {
                self.cable_server(host, event)
            }
            Event::ServedRequest(request)
                if token == SERVED_REQUEST && request.server() == CABLE_SERVER =>
            {
                self.cable_request(host, request)
            }
            // The timers are the cable's and the Wi-Fi's own network's.
            Event::WorkDue(due) if token == WORK => {
                match self.cable_due(host, due.key(), due.now_ms()) {
                    Ok(()) => return None,
                    Err(error) => Err(error),
                }
            }
            _ => return None,
        })
    }

    /// The screen opened: what an earlier time left staged goes.
    pub(super) fn cable_opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        let files = handle(&mut self.files, || {
            paperpaw_sdk::native::files::Files::open(host)
        })?;
        native_cable::clean(host, files)?;
        self.cable = Some(CableScreen {
            instance,
            list: ItemList::new(Vec::new()),
            state: Cable::new(),
            started: false,
            running: false,
            spoken: false,
            failed: None,
            pending: Vec::new(),
            clock: 0,
        });
        Ok(())
    }

    /// The screen is shown: the server on the cable is asked for, once.
    pub(super) fn cable_shown(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let Some(screen) = self.cable.as_mut().filter(|s| s.instance == instance) else {
            return Ok(());
        };
        if screen.started {
            return Ok(());
        }
        screen.started = true;
        let started = (|| {
            let server = Server::cable(CABLE_SERVER).max_body(MAX_SERVED_BODY)?;
            let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
            handle(&mut self.net, || Net::open(host))?.serve(host, &server, &mut input, &mut out)
        })();
        match started {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(error) => {
                let words = failed(host, "transfer.failed", word(error))?;
                self.cable_failed(host, words)
            }
        }
    }

    fn cable_failed(&mut self, host: &Host<'_>, words: String) -> Result<(), Error> {
        if let Some(screen) = self.cable.as_mut() {
            screen.failed = Some(words);
            redraw(host, RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The screen closed: nothing is allowed any more, the server stops (the host
    /// does it too; stopping is idempotent) and what was staged goes.
    pub(super) fn cable_closed(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        if self.cable.as_ref().is_none_or(|s| s.instance != instance) {
            return Ok(());
        }
        let ended = self.with_cable(host, |cable, hc| {
            cable.close(hc);
            Ok(())
        });
        self.cable = None;
        if let Some(net) = &self.net {
            let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
            // Nothing to do where it was stopped already.
            if let Err(Error::NoMemory) = net.stop_serving(host, CABLE_SERVER, &mut input, &mut out)
            {
                return Err(Error::NoMemory);
            }
        }
        self.disarm(host, IDLE_KEY)?;
        self.disarm(host, RETRY_KEY)?;
        let files = handle(&mut self.files, || {
            paperpaw_sdk::native::files::Files::open(host)
        })?;
        native_cable::clean(host, files)?;
        ended
    }

    /// Runs `step` on the cable's state with the host's part.
    fn with_cable<R>(
        &mut self,
        host: &Host<'_>,
        step: impl FnOnce(&mut Cable, &mut HostCable<'_, '_>) -> Result<R, Error>,
    ) -> Result<R, Error> {
        handle(&mut self.files, || {
            paperpaw_sdk::native::files::Files::open(host)
        })?;
        handle(&mut self.device, || {
            paperpaw_sdk::native::device::Device::open(host)
        })?;
        handle(&mut self.ui, || paperpaw_sdk::native::ui::Ui::open(host))?;
        // Books need the library; without it only they are refused.
        if let Err(Error::NoMemory) = handle(&mut self.library, || {
            paperpaw_sdk::native::library::Library::open(host)
        }) {
            return Err(Error::NoMemory);
        }
        let (Some(files), Some(device), Some(ui)) = (&self.files, &self.device, &self.ui) else {
            return Err(Error::State);
        };
        let handles = Handles {
            files,
            device,
            ui,
            library: self.library.as_ref(),
        };
        let screen = self.cable.as_mut().ok_or(Error::State)?;
        let mut hc = HostCable::new(host, handles, &mut self.registry, &self.installs);
        step(&mut screen.state, &mut hc)
    }

    /// The server of the cable started, failed or stopped.
    pub(super) fn cable_server(
        &mut self,
        host: &Host<'_>,
        event: &ServerEvent<'_>,
    ) -> Result<(), Error> {
        let Some(screen) = self.cable.as_mut() else {
            return Ok(());
        };
        screen.clock = event.now_ms();
        match event.state() {
            ServerState::Started { .. } => {
                screen.running = true;
                self.arm(host, IDLE_KEY, event.now_ms().saturating_add(IDLE_MS))?;
                redraw(host, RedrawMode::Partial)
            }
            ServerState::Failed { reason } => {
                screen.running = false;
                let words = failed(host, "transfer.failed", reason_word(reason))?;
                self.cable_failed(host, words)
            }
            ServerState::Stopped { reason } if screen.running => {
                screen.running = false;
                let words = failed(host, "transfer.stopped", reason_word(reason))?;
                self.cable_failed(host, words)
            }
            _ => Ok(()),
        }
    }

    /// A request came over the cable. Whatever it asks, an answer is sent and the
    /// callback ends well: nothing a computer sends may fail the plugin.
    pub(super) fn cable_request(
        &mut self,
        host: &Host<'_>,
        request: &ServedRequest<'_>,
    ) -> Result<(), Error> {
        match self.cable_answer(host, request) {
            Ok(()) => Ok(()),
            Err(error) => {
                let status = if error == Error::NoMemory { 503 } else { 500 };
                let refused = Reply::text(status, "{\"why\":\"failed, try again\"}");
                // Nothing more to do where even that is not sent.
                let _ = self.deliver(host, request.request(), refused);
                Ok(())
            }
        }
    }

    fn cable_answer(&mut self, host: &Host<'_>, request: &ServedRequest<'_>) -> Result<(), Error> {
        let Some(screen) = self.cable.as_mut() else {
            return Ok(());
        };
        screen.clock = request.now_ms();
        screen.spoken = true;
        self.arm(host, IDLE_KEY, request.now_ms().saturating_add(IDLE_MS))?;
        // What the host refused as busy goes first.
        self.flush(host)?;
        let body = match request.body_len() {
            Some(_) => Some(format(format_args!(
                ".received/{}/{}",
                request.server(),
                request.request()
            ))?),
            None => None,
        };
        let call = Call {
            number: request.request(),
            method: request.method(),
            target: request.target(),
            body: body.as_deref(),
            body_len: request.body_len().unwrap_or(0),
        };
        let before = self.cable.as_ref().map(|s| s.state.shown());
        let answer = self.with_cable(host, |cable, hc| cable.handle(hc, &call))?;
        if let Answer::Now(reply) = answer {
            self.deliver(host, request.request(), reply)?;
        }
        let after = self.cable.as_ref().map(|s| s.state.shown());
        if before != after {
            redraw(host, RedrawMode::Fast)?;
        }
        Ok(())
    }

    /// The reader answered the question about the offer (`event.answered`).
    pub(super) fn cable_answered(
        &mut self,
        host: &Host<'_>,
        answer: &Answered,
    ) -> Result<(), Error> {
        if self.cable.is_none()
            || answer.kind() != QuestionKind::Confirm
            || answer.outcome() == AnswerOutcome::Typing
        {
            return Ok(());
        }
        // Closed without an answer counts as no (the dialog's cancel).
        let yes = answer.outcome() == AnswerOutcome::Answered && answer.choice() == 1;
        let request = answer.request().get();
        let changed = self.with_cable(host, |cable, hc| Ok(cable.answered(hc, request, yes)))?;
        if changed {
            let at = self.cable.as_ref().map_or(0, |s| s.clock);
            self.arm(host, IDLE_KEY, at.saturating_add(IDLE_MS))?;
            redraw(host, RedrawMode::Fast)?;
        }
        Ok(())
    }

    /// The host ended an install the cable asked for: the computer's request
    /// that waits for it is answered. `false` where it is not the cable's.
    pub(super) fn cable_install_done(
        &mut self,
        host: &Host<'_>,
        done: &InstallDone<'_>,
    ) -> Result<bool, Error> {
        let Some(request) = done.request() else {
            return Ok(false);
        };
        if done.kind() != InstallKind::Plugin || self.cable.is_none() {
            return Ok(false);
        }
        let result = if done.status().is_ok() && done.outcome() == DeviceOutcome::Ok {
            Ok(done.at_next_start())
        } else {
            Err(crate::native_install::why(done.status(), done.outcome()))
        };
        let ended = self.with_cable(host, |cable, hc| {
            cable.install_done(hc, request.get(), result)
        })?;
        let Some((number, reply)) = ended else {
            return Ok(false);
        };
        if let Some(screen) = self.cable.as_mut() {
            screen.clock = done.now_ms();
        }
        self.deliver(host, number, reply)?;
        redraw(host, RedrawMode::Fast)?;
        Ok(true)
    }

    /// A timer of the cable came due at `now_ms`.
    pub(super) fn cable_due(
        &mut self,
        host: &Host<'_>,
        key: &str,
        now_ms: u64,
    ) -> Result<(), Error> {
        let Some(screen) = self.cable.as_mut() else {
            return Ok(());
        };
        screen.clock = now_ms;
        match key {
            IDLE_KEY if screen.state.waiting() => {
                // The reader is asked, or the host asks about an install: the
                // time counts again from when it was answered.
                self.arm(host, IDLE_KEY, now_ms.saturating_add(IDLE_MS))
            }
            IDLE_KEY => {
                let instance = screen.instance;
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).close(instance)
            }
            RETRY_KEY => self.flush(host),
            _ => Ok(()),
        }
    }

    /// Sends the answer to request `number`; one the host cannot take now is kept
    /// for the next callback or the retry timer.
    fn deliver(&mut self, host: &Host<'_>, number: u32, reply: Reply) -> Result<(), Error> {
        if let Sent::Busy = self.respond(host, number, &reply)? {
            self.keep(
                host,
                Pending {
                    number,
                    reply,
                    tries: 0,
                },
            )?;
        }
        Ok(())
    }

    /// Keeps an answer for another try, the oldest going where too many wait.
    fn keep(&mut self, host: &Host<'_>, pending: Pending) -> Result<(), Error> {
        let Some(screen) = self.cable.as_mut() else {
            return Ok(());
        };
        if screen.pending.len() >= MOST_PENDING {
            screen.pending.remove(0);
        }
        screen.pending.try_reserve(1).map_err(|_| Error::NoMemory)?;
        screen.pending.push(pending);
        let at = screen.clock.saturating_add(RETRY_MS);
        self.arm(host, RETRY_KEY, at)
    }

    /// Tries again the answers the host refused as busy.
    fn flush(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let waiting = match self.cable.as_mut() {
            Some(screen) => core::mem::take(&mut screen.pending),
            None => return Ok(()),
        };
        for mut pending in waiting {
            if let Sent::Busy = self.respond(host, pending.number, &pending.reply)? {
                pending.tries += 1;
                if pending.tries < MOST_TRIES {
                    self.keep(host, pending)?;
                }
            }
        }
        Ok(())
    }

    /// `respond` with an inline body (the cable takes no file).
    fn respond(&mut self, host: &Host<'_>, number: u32, reply: &Reply) -> Result<Sent, Error> {
        let response = Response::new(number, reply.status)?
            .content_type("application/json")?
            .body(reply.body.bytes())?;
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(SERVE_OUT)?);
        let net = handle(&mut self.net, || Net::open(host))?;
        match net.respond(host, &response, &mut input, &mut out) {
            Ok(()) => Ok(Sent::Done),
            Err(Error::Busy) => Ok(Sent::Busy),
            // Ended already (the computer went, or the 120 s passed).
            Err(Error::NotFound) => Ok(Sent::Done),
            Err(error) => Err(error),
        }
    }

    /// Declares the exact timer `key` for `at_ms` (it replaces one of the same key).
    fn arm(&mut self, host: &Host<'_>, key: &str, at_ms: u64) -> Result<(), Error> {
        let Some(work) = self.work(host)? else {
            return Ok(());
        };
        let spec = WorkSpec::at(key, WORK, at_ms.max(1))?;
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        match work.declare(host, &spec, &mut input, &mut out) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // Without the timer the screen closes with *Back*.
            _ => Ok(()),
        }
    }

    fn disarm(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let Some(work) = self.work(host)? else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        match work.cancel(host, key, &mut input, &mut out) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            _ => Ok(()),
        }
    }

    /// Draws the screen: what the cable is doing, and the last thing said.
    pub(super) fn draw_cable(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let screen = self.cable.as_mut().ok_or(Error::State)?;
        let (head, words) = screen.words(host, &mut messages)?;
        screen.list.draw_with(
            host,
            frame,
            "transfer.cable.title",
            true,
            Footer::Empty {
                title: &head,
                text: &words,
            },
        )
    }

    /// An input on the cable's screen (the back arrow of its header).
    pub(super) fn input_cable(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(screen) = self.cable.as_mut() else {
            return Ok(Handled::No);
        };
        let step = screen.list.input(host, input)?;
        Ok(ItemList::handled(step))
    }
}

impl CableScreen {
    /// The heading and the words under it: the state, and what was said last.
    fn words(&self, host: &Host<'_>, messages: &mut Messages) -> Result<(String, String), Error> {
        if let Some(words) = &self.failed {
            return Ok((
                messages.copy(host, "transfer.failed.title")?,
                paperpaw_ui_kit::text::copy(words)?,
            ));
        }
        let (head, text) = match self.state.permission() {
            Permission::Asking => ("transfer.cable.asking", Some("transfer.cable.asking.text")),
            Permission::Allowed => (
                "transfer.cable.allowed.title",
                Some("transfer.cable.allowed"),
            ),
            Permission::Refused => (
                "transfer.cable.refused.title",
                Some("transfer.cable.refused"),
            ),
            _ if self.spoken => ("transfer.cable.linked", Some("transfer.cable.connected")),
            _ => ("transfer.cable.waiting", None),
        };
        let head = messages.copy(host, head)?;
        let text = match text {
            Some(key) => messages.copy(host, key)?,
            None => String::new(),
        };
        let words = match self.state.said() {
            Some(said) => {
                let said = say(host, messages, said)?;
                if text.is_empty() {
                    said
                } else {
                    concat(&[&text, "\n\n", &said])?
                }
            }
            None => text,
        };
        Ok((head, words))
    }
}

/// What was said, in the reader's words.
fn say(host: &Host<'_>, messages: &mut Messages, said: &Said) -> Result<String, Error> {
    let (key, name) = match said {
        Said::Receiving(id) => ("transfer.cable.receiving", id),
        Said::Received(name) => ("transfer.received", name),
        Said::Installing(id) => ("transfer.cable.installing", id),
        Said::Installed { id, restart: false } => ("transfer.plugin-installed", id),
        Said::Installed { id, restart: true } => ("transfer.cable.installed-code", id),
        Said::NotInstalled(id) => ("transfer.cable.not-installed", id),
    };
    messages.format(
        host,
        key,
        &[Arg::Text {
            name: "name",
            value: name,
        }],
    )
}
