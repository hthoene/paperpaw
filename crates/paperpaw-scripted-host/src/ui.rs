//! A scripted `core.ui` for questions (`docs/abi/ui.md` section 3): ops 8
//! `ask`, 9 `text_write` and 10 `text_read`, with the host's request ids, its
//! limits and its text rules. The test reads what was asked
//! ([`Dialogs::questions`]) and answers it ([`Dialogs::answer`]), which gives
//! the batch of the `event.answered` record to send the asker.
//!
//! **Modelled:** op 8's form checks (kind, slot, flag bits, choices per kind,
//! as `Question::check` in the host), request ids, op 9 and op 10's text rules
//! (append-only parts, start over at 0, commit by `last`, the caps), one open
//! request at a time (asking again supersedes it) and a failed start: an
//! asking callback that returns a failure, or leaves an initial text begun but
//! not committed, cancels the request before any dialog opens (`not_started`).
//!
//! **Not modelled:** the dialog's own side: op 2 `question`, op 11 `answer`
//! (play it with [`Dialogs::answer`]), the dialog instance and who may write or
//! read the text (any caller may), nested questions, the device-wide request
//! limits, guard checks, the X4 card and its typing limit.
use paperpaw_core::native_host::registrations::{
    EventAnswered, RawRecord, Record, ServiceUiAsk, ServiceUiRequest, ServiceUiText,
    ServiceUiTextAt, record_encode_one,
};
use paperpaw_native::host_glue::{
    ERR_FULL, ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Most bytes of one request text; a secret one has [`SECRET_TEXT_MAX`].
pub const TEXT_MAX: usize = 24_576;
/// Most bytes of a secret request text.
pub const SECRET_TEXT_MAX: usize = 8_192;
/// Most bytes of one `text_write` part and one `text_read` part.
pub const PART_MAX: usize = 3_072;
/// Most bytes of a folder answer (a path on root 1).
pub const FOLDER_MAX: usize = 512;
/// `event.answered.flags` bit 1: the request was cancelled before it started.
const NOT_STARTED: u32 = 1 << 1;

/// What the asker asked, as the host recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Question {
    /// The request id the asker was given (never 0).
    pub request: u32,
    /// 0 text, 1 choice, 2 confirm, 3 folder.
    pub kind: u32,
    /// The `ui_ask` flags.
    pub flags: u32,
    /// The slot (empty for the kind's own).
    pub slot: String,
    /// The title the asker formatted.
    pub title: String,
    /// The question text.
    pub text: String,
    /// The choice lines (kind 1) or the button words (kind 2).
    pub choices: String,
}

impl Question {
    /// Whether the text is a secret (kind 0, flags bit 2).
    #[must_use]
    pub fn secret(&self) -> bool {
        self.kind == 0 && self.flags & 4 != 0
    }
}

/// How the dialog answers.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Answer {
    /// Text and folder questions: the typed text or the chosen path.
    Text(String),
    /// A choice question: the line's index.
    Choice(u32),
    /// A confirm question: yes (`choice` 1) or no (0).
    Confirm(bool),
    /// The dialog closed without an answer (outcome 1).
    Cancelled,
    /// A live text question: the text so far (outcome 2); the request stays open.
    Typing(String),
}

#[derive(Default)]
struct Request {
    question: Option<Question>,
    /// The text so far and whether it is committed (the last part came).
    text: Vec<u8>,
    committed: bool,
    /// The final answer came: the request is closed for the dialog.
    closed: bool,
    /// The cancel event is still due, with these `event.answered` flags:
    /// the asker asked again (0) or the start failed (`not_started`).
    cancel_due: Option<u32>,
    /// A part of the initial text was written (committed or not).
    begun: bool,
    /// Asked in the callback that is still running (the dialog is not open).
    in_callback: bool,
    /// The asking callback returned well: the dialog opened.
    opened: bool,
}

struct Inner {
    next: u32,
    requests: BTreeMap<u32, Request>,
    /// Every secret answer text, so a test can look for it in the commands.
    secrets: Vec<Vec<u8>>,
}

/// The scripted questions; clones share the state with the installed service.
#[derive(Clone)]
pub struct Dialogs {
    inner: Rc<RefCell<Inner>>,
}

impl core::fmt::Debug for Dialogs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dialogs").finish_non_exhaustive()
    }
}

impl Dialogs {
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                next: 1,
                requests: BTreeMap::new(),
                secrets: Vec::new(),
            })),
        }
    }

    /// Every question asked, oldest first (answered ones included).
    #[must_use]
    pub fn questions(&self) -> Vec<Question> {
        let inner = self.inner.borrow();
        inner
            .requests
            .values()
            .filter_map(|request| request.question.clone())
            .collect()
    }

    /// The latest question asked.
    #[must_use]
    pub fn last(&self) -> Option<Question> {
        self.questions().pop()
    }

    /// The initial text the asker committed for `request` (kinds 0 and 3), if any.
    #[must_use]
    pub fn initial_text(&self, request: u32) -> Option<String> {
        let inner = self.inner.borrow();
        let request = inner.requests.get(&request)?;
        (request.committed && !request.closed)
            .then(|| String::from_utf8_lossy(&request.text).into_owned())
    }

    /// Every secret text the dialog answered with, final or `typing`, to
    /// assert that none of them is in a staged command ([`Callback::batches_contain`](crate::Callback::batches_contain)).
    #[must_use]
    pub fn secrets(&self) -> Vec<Vec<u8>> {
        self.inner.borrow().secrets.clone()
    }

    /// The dialog's answer to `request`: the batch of the `event.answered`
    /// record to send the asker (`ScriptedHost::event`/`Plugin::event`).
    /// A final answer closes the request for the dialog and leaves its text to
    /// the asker's `text_read`; a `Typing` one keeps it open.
    ///
    /// # Errors
    /// `ERR_NOT_FOUND` for an unknown request, `ERR_STATE` for one already
    /// answered, `ERR_INVALID` for an answer the kind does not take (a choice
    /// past the list, text for a choice question, typing without the `live`
    /// flag) and `ERR_NO_SPACE` for a text over its cap.
    pub fn answer(&self, request: u32, answer: &Answer) -> Result<Vec<u8>, i32> {
        let mut inner = self.inner.borrow_mut();
        let entry = inner.requests.get_mut(&request).ok_or(ERR_NOT_FOUND)?;
        let due = entry.cancel_due.take();
        if entry.closed && !(due.is_some() && matches!(answer, Answer::Cancelled)) {
            entry.cancel_due = due;
            return Err(ERR_STATE);
        }
        let question = entry.question.clone().ok_or(ERR_NOT_FOUND)?;
        let (outcome, choice, text) = match answer {
            Answer::Cancelled => (1, 0, None),
            Answer::Text(text) if matches!(question.kind, 0 | 3) => (0, 0, Some(text)),
            Answer::Typing(text) if question.kind == 0 && question.flags & 1 != 0 => {
                (2, 0, Some(text))
            }
            Answer::Choice(index)
                if question.kind == 1
                    && usize::try_from(*index)
                        .is_ok_and(|index| index < question.choices.lines().count()) =>
            {
                (0, *index, None)
            }
            Answer::Confirm(yes) if question.kind == 2 => (0, u32::from(*yes), None),
            _ => return Err(ERR_INVALID),
        };
        let mut length = 0;
        if let Some(text) = text {
            let cap = if question.secret() {
                SECRET_TEXT_MAX
            } else {
                TEXT_MAX
            };
            if text.len() > cap {
                return Err(ERR_NO_SPACE);
            }
            if question.kind == 3 && text.len() > FOLDER_MAX {
                return Err(ERR_INVALID);
            }
            entry.text = text.clone().into_bytes();
            entry.committed = true;
            length = u32::try_from(text.len()).map_err(|_| ERR_NO_SPACE)?;
        }
        entry.closed = outcome != 2;
        if outcome != 1 && question.secret() {
            let secret = entry.text.clone();
            inner.secrets.push(secret);
        }
        crate::event_batch(&Record::EventAnswered(EventAnswered {
            now_ms: 0,
            request,
            kind: question.kind,
            outcome,
            choice,
            length,
            flags: due.unwrap_or(0),
        }))
    }

    /// The requests whose `event.answered` cancel is still due: the asker
    /// asked again, or the asking callback failed (`not_started`). Send each
    /// with [`answer`](Self::answer)`(request, &Answer::Cancelled)`.
    #[must_use]
    pub fn due(&self) -> Vec<u32> {
        let inner = self.inner.borrow();
        inner
            .requests
            .iter()
            .filter(|(_, request)| request.cancel_due.is_some())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Whether the dialog of `request` opened: the asking callback returned
    /// well with its initial text, if any, committed.
    #[must_use]
    pub fn opened(&self, request: u32) -> bool {
        self.inner
            .borrow()
            .requests
            .get(&request)
            .is_some_and(|request| request.opened)
    }

    /// The asking callback returned `status`: every request it asked starts
    /// (the dialog opens) unless the status is a failure or an initial text was
    /// begun and not committed, which cancels it before any dialog opens.
    pub(crate) fn callback_ended(&self, status: i32) {
        for request in self.inner.borrow_mut().requests.values_mut() {
            if !request.in_callback {
                continue;
            }
            request.in_callback = false;
            if status < 0 || (request.begun && !request.committed) {
                request.closed = true;
                request.cancel_due = Some(NOT_STARTED);
            } else {
                request.opened = true;
            }
        }
    }

    /// The host freed `request` (after the callback that delivered its answer).
    pub fn release(&self, request: u32) {
        self.inner.borrow_mut().requests.remove(&request);
    }

    /// The service backend: ops 8, 9 and 10.
    pub(crate) fn call(&self, op: u32, input: &[u8]) -> Result<Vec<u8>, i32> {
        let raw = RawRecord::single(input)?;
        match op {
            8 => self.ask(&raw.decode::<ServiceUiAsk<'_>>()?),
            9 => self.write(&raw.decode::<ServiceUiText<'_>>()?),
            10 => self.read(&raw.decode::<ServiceUiTextAt>()?),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    fn ask(&self, ask: &ServiceUiAsk<'_>) -> Result<Vec<u8>, i32> {
        // The host's own check, not a copy of it: the two cannot drift.
        paperpaw_core::native_host::check_ask(ask)?;
        let mut inner = self.inner.borrow_mut();
        // One open request per asker: asking again cancels the open one. Its
        // `event.answered` (outcome 1) is [`Dialogs::answer`] with `Cancelled`.
        for request in inner.requests.values_mut() {
            if !request.closed {
                request.closed = true;
                request.cancel_due = Some(0);
            }
        }
        let request = inner.next;
        inner.next = inner.next.checked_add(1).ok_or(ERR_FULL)?;
        inner.requests.insert(
            request,
            Request {
                question: Some(Question {
                    request,
                    kind: ask.kind,
                    flags: ask.flags,
                    slot: ask.slot.to_owned(),
                    title: ask.title.to_owned(),
                    text: ask.text.to_owned(),
                    choices: ask.choices.to_owned(),
                }),
                in_callback: true,
                ..Request::default()
            },
        );
        let mut out = vec![0_u8; 64];
        let len = record_encode_one(
            &Record::ServiceUiRequest(ServiceUiRequest { request }),
            &mut out,
        )?;
        out.truncate(len);
        Ok(out)
    }

    fn write(&self, part: &ServiceUiText<'_>) -> Result<Vec<u8>, i32> {
        let mut inner = self.inner.borrow_mut();
        let request = inner.requests.get_mut(&part.text).ok_or(ERR_STATE)?;
        if request.closed {
            return Err(ERR_STATE);
        }
        let secret = request.question.as_ref().is_some_and(Question::secret);
        if part.data.len() > PART_MAX {
            return Err(ERR_NO_SPACE);
        }
        let offset = usize::try_from(part.offset).map_err(|_| ERR_INVALID)?;
        if offset == 0 {
            // A write at 0 starts the text over, also after `last`.
            request.text.clear();
            request.committed = false;
        } else if offset != request.text.len() {
            return Err(ERR_INVALID);
        }
        let cap = if secret { SECRET_TEXT_MAX } else { TEXT_MAX };
        if request.text.len() + part.data.len() > cap {
            return Err(ERR_NO_SPACE);
        }
        request.begun = true;
        request.text.extend_from_slice(part.data.as_bytes());
        request.committed = part.flags & 1 != 0;
        Ok(Vec::new())
    }

    fn read(&self, at: &ServiceUiTextAt) -> Result<Vec<u8>, i32> {
        let inner = self.inner.borrow();
        let request = inner.requests.get(&at.text).ok_or(ERR_STATE)?;
        // Only a committed text, and for the asker only once it was answered.
        if !request.committed || !request.closed {
            return Err(ERR_STATE);
        }
        let offset = usize::try_from(at.offset).map_err(|_| ERR_INVALID)?;
        let rest = request.text.get(offset..).ok_or(ERR_INVALID)?;
        let mut end = rest.len().min(PART_MAX);
        while !core::str::from_utf8(&rest[..end]).is_ok() && end > 0 {
            end -= 1;
        }
        let total = u32::try_from(request.text.len()).map_err(|_| ERR_NO_SPACE)?;
        let data = core::str::from_utf8(&rest[..end]).map_err(|_| ERR_INVALID)?;
        let mut out = vec![0_u8; PART_MAX + 64];
        let len = record_encode_one(
            &Record::ServiceUiText(ServiceUiText {
                text: at.text,
                offset: at.offset,
                total,
                flags: 0,
                data,
            }),
            &mut out,
        )?;
        out.truncate(len);
        Ok(out)
    }
}
