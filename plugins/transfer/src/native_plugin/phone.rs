//! The phone's keyboard screen: the question the host opened it for, its page,
//! and the text that comes back as the dialog's answer.
use super::{NativeTransfer, handle, redraw};
use crate::{
    declarations::PHONE,
    native_phone::{self, Dialog, MAX_TEXT, Texts, Words},
    native_web::{Answer, Body, Request},
};
use alloc::string::String;
use paperpaw_sdk::{
    Instant,
    native::{
        Error, Host,
        commands::{CommandSender, RedrawMode},
        net::ServedRequest,
        storage::Root,
        ui::{Answer as UiAnswer, HandoverId, QuestionKind, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages,
    files::{read_all, replace},
    mem::{SERVICE_BUFFER, buffer, wipe_text},
    text::{copy, format},
    uitext,
};

/// The bodies of the phone's requests, read from the data folder.
struct HostTexts<'a, 'h> {
    host: &'a Host<'h>,
}

impl Texts for HostTexts<'_, '_> {
    fn read(&mut self, body: &str, limit: usize) -> Result<String, Error> {
        let bytes = read_all(self.host, Root::Data, body, limit)?.ok_or(Error::NotFound)?;
        String::from_utf8(bytes).map_err(|_| Error::Utf8)
    }
}

impl NativeTransfer {
    /// Reads the question the keyboard asked: its request, title, whether the
    /// text is a secret, and the text to start from.
    pub(super) fn open_phone(
        &mut self,
        host: &Host<'_>,
        handover: Option<HandoverId>,
    ) -> Result<Dialog, Error> {
        let ui = handle(&mut self.ui, || Ui::open(host))?;
        let handover = handover.ok_or(Error::Invalid)?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let mut request = None;
        for param in ui.params(host, handover, &mut input, &mut output)? {
            if let Some(id) = param?.request()? {
                request = Some(id);
            }
        }
        let request = request.ok_or(Error::Invalid)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let question = ui.question(host, request, &mut input, &mut output)?;
        if question.kind() != QuestionKind::Text {
            return Err(Error::Invalid);
        }
        let secret = question.is_secret();
        let title = copy(question.title())?;
        let limit = (question.max_bytes() as usize).min(MAX_TEXT);
        // The text to start from; none for a secret one, and none where a
        // longer one cannot go to the phone whole.
        let text = if secret {
            String::new()
        } else {
            match uitext::read_all(ui, host, request.text(), limit) {
                Ok(text) => text,
                Err(Error::State | Error::NoSpace) => String::new(),
                Err(error) => return Err(error),
            }
        };
        Ok(Dialog {
            request,
            secret,
            title,
            text,
            limit,
        })
    }

    /// Writes the phone's page, with what the keyboard asked, for `respond`.
    pub(super) fn write_phone_page(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(phone) = self.screens.top(PHONE).and_then(|page| page.phone.as_ref()) else {
            return Err(Error::State);
        };
        let mut messages = Messages::new()?;
        let send = messages.copy(host, "transfer.phone.send")?;
        let sent = messages.copy(host, "transfer.phone.sent")?;
        let failed = messages.copy(host, "transfer.web.failed")?;
        let show = messages.copy(host, "transfer.phone.show")?;
        let title = if phone.title.is_empty() {
            messages.copy(host, "transfer.phone.title")?
        } else {
            copy(&phone.title)?
        };
        let words = Words {
            send: &send,
            sent: &sent,
            failed: &failed,
            show: &show,
        };
        let page = native_phone::page(&words, &title, &phone.text, phone.secret)?;
        replace(host, Root::Data, native_phone::PAGE_FILE, &page)
    }

    /// A request came to the phone's page.
    pub(super) fn answer_phone(
        &mut self,
        host: &Host<'_>,
        request: &ServedRequest<'_>,
    ) -> Result<(), Error> {
        let body = match request.body_len() {
            Some(_) => Some(format(format_args!(
                ".received/{}/{}",
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
        let Some(page) = self.screens.top(PHONE) else {
            return Ok(());
        };
        let limit = page.phone.as_ref().map_or(MAX_TEXT, |phone| phone.limit);
        let Some(guard) = page.guard.as_mut() else {
            let words = Answer {
                status: 503,
                kind: "text/plain; charset=utf-8",
                body: Body::Text("not ready"),
            };
            return self.send(host, request.request(), &words);
        };
        let (answer, text) = native_phone::answer(&mut HostTexts { host }, guard, &asked, limit);
        let sent = self.send(host, request.request(), &answer);
        if let Some(body) = &body {
            // Nothing more to do where it cannot be removed: the host removes
            // what a server received when the server stops.
            let _ = paperpaw_sdk::native::storage::remove(host, Root::Data, body);
        }
        sent?;
        match text {
            Some(text) => self.give_text(host, text),
            None => Ok(()),
        }
    }

    /// The text from the phone is the keyboard's answer, and the screen closes.
    fn give_text(&mut self, host: &Host<'_>, text: String) -> Result<(), Error> {
        let Some(request) = self
            .screens
            .top(PHONE)
            .and_then(|page| page.phone.as_ref())
            .map(|phone| phone.request)
        else {
            wipe_text(text);
            return Ok(());
        };
        let ui = handle(&mut self.ui, || Ui::open(host))?;
        let written = uitext::write_all(ui, host, request.text(), &text);
        wipe_text(text);
        written?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match ui.answer(host, request, UiAnswer::Accept, &mut input, &mut out) {
            // The question ended meanwhile: the screen closes all the same.
            Ok(()) | Err(Error::State) => {}
            Err(error) => return Err(error),
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).back()?;
        if let Some(page) = self.screens.top(PHONE)
            && let Some(phone) = page.phone.as_mut()
        {
            phone.wipe();
        }
        redraw(host, RedrawMode::Partial)
    }
}
