//! The phone's screen on interface 7: while it is open the device serves the
//! page to the local network (`core.net` `serve`), shows the address as text and
//! as a QR code, and answers the page's requests (`native_phone`). The address
//! carries a key drawn anew each time from the device's random source
//! (`core.secrets`); wrong keys lock the page for a while. Closing the screen
//! ends the serving, as on interface 6.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled, Instant,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        files::{self, Files},
        formatting::Arg,
        net::{FileRoot, Net, REQUEST_INPUT, Reason, Response, ServedRequest, Server, ServerState},
        secrets::{Secrets, wipe},
        storage::{self, DataStorage, Root},
        ui::Instance,
    },
    qr::QrCode,
    served::Guard,
};
use paperpaw_ui_kit::{
    Footer, ItemList, Messages, Screens,
    files::replace,
    mem::{buffer, push, wipe_text},
    text::{copy, format},
};

use crate::cards;
use crate::files::{DataFolder, FOLDER};
use crate::native_key;
use crate::native_phone::{
    self as phone, Answer, Body, Done, INLINE, MAX_IMPORT, PAGE_FILE, PORT, Request, SERVER, TEXTS,
};
use crate::native_phone_draw::draw_address;

/// The screen's callback token.
pub(crate) use crate::declarations::PHONE_TOKEN as TOKEN;

/// What the host's `serve` and `stop_serving` reply with, at most.
const SERVE_OUT: usize = 1536;
/// What `core.secrets` `random` replies with, at most: a smaller buffer is `ERR_NO_SPACE`.
const RANDOM_REPLY: usize = 1280;
/// Answers too long to send from memory that wait in files at most; the
/// oldest goes when another is made.
const REPLIES: usize = 4;

/// What the screen shows.
#[derive(Debug)]
enum View {
    /// Waiting for the server to start.
    Starting,
    /// Serving at `url`; the code carries the same.
    Serving { url: String, qr: Option<QrCode> },
    /// Not serving, with the words for it.
    Failed(String),
}

/// The open phone screen.
#[derive(Debug)]
pub(crate) struct PhoneScreen {
    view: View,
    /// The empty list under the header: its back arrow and its draw.
    list: ItemList,
    /// The key of this time of serving and the wrong ones tried.
    guard: Option<Guard>,
    /// What the phone did last, as said on the screen.
    last: Option<String>,
    /// Whether serving was asked for (once for each time the screen opens).
    started: bool,
    /// The requests whose long answers are in files, oldest first.
    replies: Vec<u32>,
}

/// The phone's screen and the handles it uses (opened on first use).
#[derive(Debug, Default)]
pub(crate) struct Phone {
    screens: Screens<PhoneScreen>,
    net: Option<Net>,
    secrets: Option<Secrets>,
    files: Option<Files>,
}

fn handle<T>(slot: &mut Option<T>, open: impl FnOnce() -> Result<T, Error>) -> Result<&T, Error> {
    if slot.is_none() {
        *slot = Some(open()?);
    }
    slot.as_ref().ok_or(Error::State)
}

fn redraw(host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(mode)
}

/// Whether `name` is a file an earlier time of serving made: the page, or a
/// long answer (`phone-<request>.json`).
fn is_left(name: &str) -> bool {
    name == PAGE_FILE
        || name
            .strip_prefix("phone-")
            .and_then(|rest| rest.strip_suffix(".json"))
            .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The name of the file a long answer to request `number` is in.
fn reply_file(number: u32) -> Result<String, Error> {
    format(format_args!("phone-{number}.json"))
}

impl Phone {
    /// The screen opens: the answers given in reviews are folded into the
    /// decks, and the words looked up become cards, so the page shows the decks
    /// as they are.
    pub(crate) fn opened(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let data = DataStorage::new(host);
        let storage = DataFolder(&data);
        self.sweep(host)?;
        let _ = cards::fold(&storage);
        let deck = crate::native_plugin::lookup_deck(host)?;
        let _ = cards::take_lookups(&storage, &deck);
        let page = PhoneScreen {
            view: View::Starting,
            list: ItemList::new(Vec::new()),
            guard: None,
            last: None,
            started: false,
            replies: Vec::new(),
        };
        self.screens.open(instance, page)
    }

    /// Removes what a time of serving left behind (a power cut stops it before
    /// the screen closes): the page and the long answers. Best effort: only a
    /// refused allocation is an error.
    fn sweep(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let files = match handle(&mut self.files, || Files::open(host)) {
            Ok(files) => files,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(()),
        };
        let (mut input, mut out) = (buffer(files::INPUT_BYTES)?, buffer(files::OUTPUT_BYTES)?);
        let mut left: Vec<String> = Vec::new();
        let mut cursor = 0;
        loop {
            let page = match files.list(host, Root::Data, "", cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // No folder yet, or none that can be read: nothing is left.
                Err(_) => break,
            };
            for entry in page.entries() {
                if !entry.is_folder() && is_left(entry.name()) {
                    push(&mut left, copy(entry.name())?)?;
                }
            }
            match page.next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        for name in &left {
            let removed = files.remove_tree(host, Root::Data, name, &mut input, &mut out);
            if removed == Err(Error::NoMemory) {
                return Err(Error::NoMemory);
            }
        }
        Ok(())
    }

    /// The screen is shown the first time: its page is written and the server
    /// asked for (a server lives on a screen that shows, `network.md` 5.1).
    pub(crate) fn shown(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.screens.shown(TOKEN, instance)?;
        let Some(page) = self.screens.top(TOKEN) else {
            return Ok(());
        };
        if page.started {
            return Ok(());
        }
        page.started = true;
        match self.start(host) {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(error) => {
                let words = failed(host, word(error))?;
                self.show(host, View::Failed(words))
            }
        }
    }

    /// The screen closed: the server stops (the host does it too; stopping is
    /// idempotent) and what was written for it is removed.
    pub(crate) fn closed(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        // Another screen's closing is not the phone's.
        if self.screens.get(instance).is_none() {
            return Ok(());
        }
        let replies = self
            .screens
            .get(instance)
            .map(|page| core::mem::take(&mut page.replies))
            .unwrap_or_default();
        self.screens.close(instance);
        if let Some(net) = &self.net {
            let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
            // Nothing to do where it was stopped already.
            if let Err(Error::NoMemory) = net.stop_serving(host, SERVER, &mut input, &mut out) {
                return Err(Error::NoMemory);
            }
        }
        let page = [String::from(PAGE_FILE)];
        let files = replies
            .iter()
            .map(|n| reply_file(*n))
            .chain(page.into_iter().map(Ok));
        for file in files {
            if let Err(Error::NoMemory) = storage::remove(host, Root::Data, &file?) {
                return Err(Error::NoMemory);
            }
        }
        Ok(())
    }

    /// A key for this time of serving: none where the source was not fully
    /// random; an error where there is no source.
    fn draw_key(&mut self, host: &Host<'_>) -> Result<Option<String>, Error> {
        let secrets = &mut self.secrets;
        native_key::key(&mut |bytes: &mut [u8]| {
            let secrets = handle(secrets, || Secrets::open(host))?;
            let (mut input, mut out) = (buffer(128)?, buffer(RANDOM_REPLY)?);
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
    fn start(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // A device with no source of random numbers serves nothing: no weaker key.
        match self.draw_key(host) {
            Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                let words = Messages::new()?.copy(host, "vocabulary.phone.no-random")?;
                return self.show(host, View::Failed(words));
            }
            Err(error) => return Err(error),
            Ok(key) => key.map_or((), wipe_text),
        }
        let page = phone::assemble(&page_strings(host)?)?;
        replace(host, Root::Data, PAGE_FILE, &page)?;
        let server = Server::new(SERVER, PORT)?.max_body(MAX_IMPORT)?;
        let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
        handle(&mut self.net, || Net::open(host))?.serve(host, &server, &mut input, &mut out)
    }

    /// Shows `view` on the screen that is open, and draws it.
    fn show(&mut self, host: &Host<'_>, view: View) -> Result<(), Error> {
        if let Some(page) = self.screens.top(TOKEN) {
            page.view = view;
            redraw(host, RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The server started, failed or stopped.
    pub(crate) fn server(&mut self, host: &Host<'_>, state: ServerState<'_>) -> Result<(), Error> {
        match state {
            ServerState::Started { address } => match self.serving(host, address) {
                // No room to make the address: the server is not left running
                // with nobody told how to reach it.
                Err(Error::NoMemory) => {
                    self.give_up(host);
                    Err(Error::NoMemory)
                }
                other => other,
            },
            ServerState::Failed { reason } => self.not_serving(host, reason),
            // Asked for by the plugin: nothing to say. Ended by the network
            // while the address was shown: it is dead, and the screen says so.
            ServerState::Stopped {
                reason: Reason::NoNetwork,
            } => self.not_serving(host, Reason::NoNetwork),
            _ => Ok(()),
        }
    }

    /// The server is stopped and the screen says it is not running, as far as
    /// the memory allows: nothing here may fail again.
    fn give_up(&mut self, host: &Host<'_>) {
        if let Some(page) = self.screens.top(TOKEN) {
            page.guard = None;
            page.view = View::Failed(String::new());
        }
        if let (Ok(net), Ok(mut input), Ok(mut out)) = (
            handle(&mut self.net, || Net::open(host)),
            buffer(64),
            buffer(SERVE_OUT),
        ) {
            match net.stop_serving(host, SERVER, &mut input, &mut out) {
                Ok(()) | Err(_) => {}
            }
        }
        match redraw(host, RedrawMode::Partial) {
            Ok(()) | Err(_) => {}
        }
    }

    /// The server runs at `address`: the key is drawn now (on the X4 the radio
    /// makes it fully random) and the address shown with it.
    fn serving(&mut self, host: &Host<'_>, address: &str) -> Result<(), Error> {
        let key = match self.draw_key(host) {
            Ok(Some(key)) => key,
            Ok(None) | Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                if let Some(net) = &self.net {
                    let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
                    net.stop_serving(host, SERVER, &mut input, &mut out)?;
                }
                let words = Messages::new()?.copy(host, "vocabulary.phone.no-random")?;
                return self.show(host, View::Failed(words));
            }
            Err(error) => return Err(error),
        };
        let url = format(format_args!("http://{address}/?k={key}"))?;
        let qr = QrCode::new(url.as_bytes());
        if let Some(page) = self.screens.top(TOKEN) {
            page.guard = Some(Guard::new(key));
        }
        self.show(host, View::Serving { url, qr })
    }

    /// The server did not start or ended: no network has its own words.
    fn not_serving(&mut self, host: &Host<'_>, reason: Reason) -> Result<(), Error> {
        if let Some(page) = self.screens.top(TOKEN) {
            page.guard = None;
        }
        let words = if reason == Reason::NoNetwork {
            Messages::new()?.copy(host, "vocabulary.phone.no-network")?
        } else {
            failed(host, reason_word(reason))?
        };
        self.show(host, View::Failed(words))
    }

    /// A request came to the page. Whatever it asks, the answer is sent and the
    /// callback ends well: nothing a computer on the network sends may fail the
    /// plugin.
    pub(crate) fn request(
        &mut self,
        host: &Host<'_>,
        request: &ServedRequest<'_>,
    ) -> Result<(), Error> {
        match self.answer(host, request) {
            Ok(()) => Ok(()),
            Err(error) => {
                let status = if error == Error::NoMemory { 503 } else { 500 };
                let refused = Answer {
                    status,
                    kind: "text/plain; charset=utf-8",
                    body: Body::Text("failed, try again"),
                };
                // Nothing more to do where even that is not sent.
                let _ = self.send(host, request.request(), &refused);
                Ok(())
            }
        }
    }

    fn answer(&mut self, host: &Host<'_>, request: &ServedRequest<'_>) -> Result<(), Error> {
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
            body_len: request.body_len().unwrap_or(0),
            at: Instant(request.now_ms()),
        };
        let Some(guard) = self.screens.top(TOKEN).and_then(|page| page.guard.as_mut()) else {
            // Before the key was drawn (or after the screen closed): not served.
            let words = Answer {
                status: 503,
                kind: "text/plain; charset=utf-8",
                body: Body::Text("not ready"),
            };
            return self.send(host, request.request(), &words);
        };
        let data = DataStorage::new(host);
        let (answer, done) = phone::answer(guard, &DataFolder(&data), &asked);
        self.send(host, request.request(), &answer)?;
        if let Some(done) = done {
            let said = said(host, &done)?;
            if let Some(page) = self.screens.top(TOKEN) {
                page.last = Some(said);
                redraw(host, RedrawMode::Fast)?;
            }
        }
        Ok(())
    }

    /// Sends `answer` to request `number`: what is short from memory, what is
    /// long from a file of the data folder.
    fn send(&mut self, host: &Host<'_>, number: u32, answer: &Answer) -> Result<(), Error> {
        let response = Response::new(number, answer.status)?.content_type(answer.kind)?;
        let kept;
        let response = match &answer.body {
            Body::Text(words) => response.body(words.as_bytes())?,
            Body::Owned(words) if words.len() <= INLINE => response.body(words.as_bytes())?,
            Body::Owned(words) => {
                kept = self.keep(host, number, words.as_bytes())?;
                response.send_file(FileRoot::Data, &kept)?
            }
            Body::Page => response.send_file(FileRoot::Data, PAGE_FILE)?,
        };
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(SERVE_OUT)?);
        let net = handle(&mut self.net, || Net::open(host))?;
        match net.respond(host, &response, &mut input, &mut out) {
            // Ended already (the client went, or the 120 s passed).
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Writes a long answer to its file; the oldest of [`REPLIES`] goes.
    fn keep(&mut self, host: &Host<'_>, number: u32, bytes: &[u8]) -> Result<String, Error> {
        let file = reply_file(number)?;
        replace(host, Root::Data, &file, bytes)?;
        let Some(page) = self.screens.top(TOKEN) else {
            return Ok(file);
        };
        page.replies.try_reserve(1).map_err(|_| Error::NoMemory)?;
        page.replies.push(number);
        if page.replies.len() > REPLIES {
            let old = page.replies.remove(0);
            if let Err(Error::NoMemory) = storage::remove(host, Root::Data, &reply_file(old)?) {
                return Err(Error::NoMemory);
            }
        }
        Ok(file)
    }

    /// Draws the screen under its header.
    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let page = self.screens.top(TOKEN).ok_or(Error::State)?;
        let mut messages = Messages::new()?;
        let title = "vocabulary.phone.title";
        let (head, words) = match &page.view {
            View::Serving { url, qr } => {
                page.list
                    .draw_with(host, frame, title, true, Footer::None)?;
                let hint = "vocabulary.phone.hint";
                return draw_address(host, frame, hint, url, qr.as_ref(), page.last.as_deref());
            }
            View::Starting => (
                messages.copy(host, "vocabulary.phone.starting")?,
                String::new(),
            ),
            View::Failed(words) => (
                messages.copy(host, "vocabulary.phone.failed.title")?,
                copy(words)?,
            ),
        };
        // The sentence is said wrapped, under a title, centred as an empty list says it.
        page.list.draw_with(
            host,
            frame,
            title,
            true,
            Footer::Empty {
                title: &head,
                text: &words,
            },
        )
    }

    /// Only back does anything here.
    pub(crate) fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(page) = self.screens.top(TOKEN) else {
            return Ok(Handled::No);
        };
        let step = page.list.input(host, input)?;
        Ok(ItemList::handled(step))
    }
}

/// The page's texts as a JavaScript object of [`TEXTS`], in the reader's language.
fn page_strings(host: &Host<'_>) -> Result<String, Error> {
    let mut messages = Messages::new()?;
    let mut json = String::new();
    json.try_reserve(4_096).map_err(|_| Error::NoMemory)?;
    json.push('{');
    for (i, (name, key)) in TEXTS.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(name);
        json.push(':');
        phone::push_json(&mut json, &messages.copy(host, key)?)?;
    }
    json.try_reserve(1).map_err(|_| Error::NoMemory)?;
    json.push('}');
    Ok(json)
}

/// `reason` in the words the failed text takes.
fn failed(host: &Host<'_>, reason: &str) -> Result<String, Error> {
    let arguments = [Arg::Text {
        name: "reason",
        value: reason,
    }];
    Messages::new()?.format(host, "vocabulary.phone.failed", &arguments)
}

/// The reason a server did not run, as interface 6 named it.
const fn reason_word(reason: Reason) -> &'static str {
    match reason {
        Reason::NoNetwork => "no network",
        Reason::Offline => "offline",
        Reason::Saving => "saving",
        Reason::NetworkOff => "network off",
        Reason::Refused => "refused",
        Reason::Ok => "ended",
        _ => "failed",
    }
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

/// What a request did, as said under the code.
fn said(host: &Host<'_>, done: &Done) -> Result<String, Error> {
    let mut messages = Messages::new()?;
    match done {
        Done::Deck(name) => messages.format(
            host,
            "vocabulary.phone.deck-saved",
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        ),
        Done::Card(front) => messages.format(
            host,
            "vocabulary.phone.card-saved",
            &[Arg::Text {
                name: "front",
                value: front,
            }],
        ),
        Done::Deleted => messages.copy(host, "vocabulary.phone.deleted"),
        Done::Imported(n) => messages.format(
            host,
            "vocabulary.phone.imported",
            &[Arg::Count(i64::try_from(*n).unwrap_or(i64::MAX))],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every text a screen says exists in both catalogs, and the plural one in
    /// both its forms.
    #[test]
    fn the_screens_texts_are_in_both_languages() {
        for catalog in [
            include_str!("../i18n/en.ini"),
            include_str!("../i18n/de.ini"),
        ] {
            for key in [
                "vocabulary.phone.title",
                "vocabulary.phone.starting",
                "vocabulary.phone.no-network",
                "vocabulary.phone.failed",
                "vocabulary.phone.failed.title",
                "vocabulary.phone.no-random",
                "vocabulary.phone.hint",
                "vocabulary.phone.deck-saved",
                "vocabulary.phone.card-saved",
                "vocabulary.phone.deleted",
                "vocabulary.phone.imported.one",
                "vocabulary.phone.imported.other",
            ] {
                assert!(
                    catalog
                        .lines()
                        .any(|l| l.starts_with(&alloc::format!("{key} "))),
                    "{key}"
                );
            }
        }
    }

    /// What an earlier serving left is the page and the long answers; the decks
    /// and what only looks alike are not.
    #[test]
    fn what_an_earlier_serving_left_is_told_from_what_looks_alike() {
        for name in ["phone.html", "phone-3.json", "phone-4096.json"] {
            assert!(is_left(name), "{name}");
        }
        for name in [
            "decks.txt",
            "phone-.json",
            "phone-x.json",
            "phone-7.json.keep",
            "Phone.html",
        ] {
            assert!(!is_left(name), "{name}");
        }
    }

    #[test]
    fn a_long_answer_has_a_file_of_its_request() {
        assert_eq!(reply_file(7).unwrap(), "phone-7.json");
    }
}
