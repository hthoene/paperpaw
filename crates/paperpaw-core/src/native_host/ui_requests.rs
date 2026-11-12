//! Requests: the questions `core.ui` op 8 records and their answers
//! (`docs/abi/ui.md` 3), kept in the one [`UiStore`] with the one id allocator.
//!
//! A request is **asked** while the asker's callback runs (it may still write
//! the initial text), **starting** once that callback returned and the stack
//! opens the dialog, **open** while its dialog instance is up, and **done**
//! once it has a final answer (or was cancelled) and waits to be delivered to
//! the asker as `event.answered`. The request keeps its own answer and pending
//! mark, so a full held-event store never drops it. Question text, answers and
//! texts may be secret: they are wiped when freed and never in a `Debug` or a
//! log.
use super::ui_store::{Inner, PART_MAX, Stamp, Text, UiStore, Wiped, WriteRules, write_part};
use paperpaw_native::host_glue::{
    ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
};

/// Largest host text of a request that is not secret, and of a secret one.
pub(crate) const TEXT_MAX: usize = 24_576;
pub(crate) const SECRET_TEXT_MAX: usize = 8_192;
/// A folder answer: a path on root 1.
const FOLDER_MAX: usize = 512;
/// Kinds and flags of `ui_ask` (ui.md 3.1).
const KINDS: u32 = 4;
const LIVE: u32 = 1;
pub(super) const SECRET: u32 = 1 << 2;
const ONLY_YES: u32 = 1 << 8;
const TEXT_FLAGS: u32 = 0xFF;
const CONFIRM_FLAGS: u32 = 0x300;
const MODES: u32 = 4;
/// Outcomes of an answer.
const ANSWERED: u32 = 0;
const CANCELLED: u32 = 1;
const TYPING: u32 = 2;
/// `event.answered.flags`.
const DIALOG_CLOSED: u32 = 1;
const NOT_STARTED: u32 = 1 << 1;
const TYPING_STOPPED: u32 = 1 << 2;

/// A question as `ui_ask` brings it.
#[derive(Clone, Copy)]
pub(crate) struct Question<'a> {
    pub(crate) kind: u32,
    pub(crate) flags: u32,
    pub(crate) remote: bool,
    pub(crate) title: &'a str,
    pub(crate) text: &'a str,
    pub(crate) choices: &'a str,
}

impl Question<'_> {
    /// The form of the question (ui.md 3.1), before any limit of the device.
    pub(crate) fn check(&self) -> Result<(), i32> {
        if self.kind >= KINDS {
            return Err(ERR_INVALID);
        }
        let allowed = match self.kind {
            0 => TEXT_FLAGS,
            2 => CONFIRM_FLAGS,
            _ => 0,
        };
        if self.flags & !allowed != 0 {
            return Err(ERR_INVALID);
        }
        if self.kind == 0 && (self.flags >> 4) & 0xF > MODES {
            return Err(ERR_INVALID);
        }
        match self.kind {
            1 => {
                if self.choices.is_empty() {
                    return Err(ERR_INVALID);
                }
                let mut count = 0;
                for line in self.choices.split('\n') {
                    if line.is_empty() {
                        return Err(ERR_INVALID);
                    }
                    if line.len() > 128 {
                        return Err(ERR_NO_SPACE);
                    }
                    count += 1;
                }
                if count > 32 || self.choices.len() > 2_048 {
                    return Err(ERR_NO_SPACE);
                }
            }
            2 if !self.choices.is_empty() => {
                let mut words = self.choices.split('\n');
                let (yes, no) = (words.next(), words.next());
                let (Some(yes), Some(no), None) = (yes, no, words.next()) else {
                    return Err(ERR_INVALID);
                };
                if yes.len() > 128 || no.len() > 128 {
                    return Err(ERR_NO_SPACE);
                }
                if self.flags & ONLY_YES != 0 && !no.is_empty() {
                    return Err(ERR_INVALID);
                }
            }
            2 => {}
            _ if !self.choices.is_empty() => return Err(ERR_INVALID),
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The asker's callback (this epoch) is running.
    Asked { epoch: u64 },
    /// The callback returned; the stack opens the dialog.
    Starting,
    /// The dialog instance is up.
    Open { dialog: u32 },
    /// Final answer or cancellation; waits for delivery.
    Done,
}

#[derive(Clone, Copy)]
struct Answer {
    outcome: u32,
    choice: u32,
    length: u32,
    flags: u32,
}

pub(super) struct Request {
    id: u32,
    asker: u64,
    /// The asker's screen instance, if it asked from one: its closing cancels.
    tie: Option<u32>,
    kind: u32,
    flags: u32,
    secret: bool,
    remote: bool,
    phase: Phase,
    title: Wiped,
    question: Wiped,
    choices: Wiped,
    pub(super) text: Text,
    /// What the question (title, text, choices) is charged to the question
    /// pool.
    asked_bytes: usize,
    /// The nested question this request's dialog asked, and the parent of a
    /// nested one.
    child: Option<u32>,
    parent: Option<u32>,
    /// What is waiting to be delivered, and what was delivered as typing.
    pending: Option<Answer>,
    /// The dialog instance that was up, kept after the final answer so its
    /// closing can still be noted (`dialog_closed`, bit 0).
    dialog: Option<u32>,
    dialog_gone: bool,
    delivering: bool,
}

/// What the stack opens for a request whose asking callback returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Start {
    pub(crate) request: u32,
    /// The plugin that asked.
    pub(crate) asker: u64,
    pub(crate) kind: u32,
    pub(crate) remote: bool,
    pub(crate) secret: bool,
}

/// A request that was cancelled and whose dialog, if up, must close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cancelled {
    pub(crate) request: u32,
    pub(crate) dialog: Option<u32>,
}

/// What `event.answered` carries (the host adds `now_ms`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Delivery {
    pub(crate) request: u32,
    pub(crate) kind: u32,
    pub(crate) outcome: u32,
    pub(crate) choice: u32,
    pub(crate) length: u32,
    pub(crate) flags: u32,
}

/// The question a dialog reads with op 2.
pub(crate) struct Shown<'a> {
    pub(crate) kind: u32,
    pub(crate) flags: u32,
    pub(crate) max_bytes: u32,
    pub(crate) title: &'a str,
    pub(crate) text: &'a str,
    pub(crate) choices: &'a str,
}

fn as_str(bytes: &Wiped) -> &str {
    core::str::from_utf8(&bytes.0).unwrap_or_default()
}

fn lines(choices: &str) -> usize {
    if choices.is_empty() {
        0
    } else {
        choices.split('\n').count()
    }
}

impl Request {
    pub(super) fn instances(&self) -> [Option<u32>; 2] {
        [self.tie, self.dialog]
    }

    fn open(&self) -> bool {
        self.phase != Phase::Done
    }
}

impl UiStore {
    fn request_at(inner: &Inner, id: u32) -> Option<usize> {
        inner.requests.iter().position(|request| request.id == id)
    }

    /// Whether `id` names a request (else it may name a hand-over).
    pub(crate) fn is_request(&self, id: u32) -> bool {
        self.inner()
            .is_ok_and(|inner| Self::request_at(&inner, id).is_some())
    }

    /// Requests open now (a nested one is not counted), and whether `asker`
    /// has one: the figures the admission rules take.
    pub(crate) fn open_requests(&self, asker: u64) -> (u8, bool) {
        let Ok(inner) = self.inner() else {
            return (0, false);
        };
        let open = inner
            .requests
            .iter()
            .filter(|request| request.open() && request.parent.is_none());
        let total = open.clone().count();
        let has = open.clone().any(|request| request.asker == asker);
        (u8::try_from(total).unwrap_or(u8::MAX), has)
    }

    /// The request whose dialog is `instance`, as the parent a nested question
    /// asks under: its `secret`, whether it is nested itself, and whether its
    /// one nested question is open.
    pub(crate) fn dialog_of(&self, instance: u32) -> Option<(u32, bool, bool, bool)> {
        let inner = self.inner().ok()?;
        inner
            .requests
            .iter()
            .find(|r| r.phase == (Phase::Open { dialog: instance }))
            .map(|r| (r.id, r.secret, r.parent.is_some(), r.child.is_some()))
    }

    /// Op 8: records the question for `who`, after the stack's admission. A
    /// request already open for the asker is cancelled first when `replace`;
    /// the nested question hangs under `parent`. Allocated fallibly; a failure
    /// changes nothing.
    pub(crate) fn ask(
        &self,
        who: Stamp,
        tie: Option<u32>,
        question: &Question<'_>,
        secret: bool,
        parent: Option<u32>,
        replace: bool,
    ) -> Result<(u32, Option<Cancelled>), i32> {
        question.check()?;
        let asked_bytes = question.title.len() + question.text.len() + question.choices.len();
        let mut inner = self.inner()?;
        // Rows kept at once are bounded, done and cancelled ones waiting for
        // their event included: a replaced request stays one until delivered.
        // Both limits are checked before anything is copied: an over-cap ask
        // is `ERR_FULL` and allocates nothing.
        if inner.requests.len() >= self.limits.requests_kept {
            return Err(ERR_FULL);
        }
        // Every kept question is charged to the question pool (a question is up
        // to 3.3 KiB): `ERR_FULL` when it does not fit, nothing changed.
        if inner.question_memory.saturating_add(asked_bytes) > self.limits.question_memory {
            return Err(ERR_FULL);
        }
        let title = Wiped::copy(question.title.as_bytes())?;
        let text = Wiped::copy(question.text.as_bytes())?;
        let choices = Wiped::copy(question.choices.as_bytes())?;
        inner.requests.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        if let Some(parent) = parent
            && Self::request_at(&inner, parent).is_none_or(|at| inner.requests[at].child.is_some())
        {
            return Err(ERR_FULL);
        }
        let id = self.alloc_id()?;
        let cancelled = if replace {
            Self::cancel_asker(&mut inner, who.owner, None)
        } else {
            None
        };
        if let Some(parent) = parent
            && let Some(at) = Self::request_at(&inner, parent)
        {
            inner.requests[at].child = Some(id);
        }
        inner.question_memory += asked_bytes;
        inner.requests.push(Request {
            id,
            asker: who.owner,
            tie,
            kind: question.kind,
            flags: question.flags,
            secret,
            remote: question.remote,
            phase: Phase::Asked { epoch: who.epoch },
            title,
            question: text,
            choices,
            text: Text::default(),
            asked_bytes,
            child: None,
            parent,
            pending: None,
            dialog: None,
            dialog_gone: false,
            delivering: false,
        });
        Ok((id, cancelled))
    }

    /// Cancels the open requests of `asker` (all, or the one tied to
    /// `instance`); each gets outcome 1 pending and is returned with its dialog.
    fn cancel_asker(inner: &mut Inner, asker: u64, instance: Option<u32>) -> Option<Cancelled> {
        let at = inner.requests.iter().position(|r| {
            r.asker == asker
                && r.open()
                && r.parent.is_none()
                && instance.is_none_or(|i| r.tie == Some(i))
        })?;
        Some(Self::cancel_at(inner, at, 0))
    }

    /// Cancels request `at` and its nested question; the answer is outcome 1.
    fn cancel_at(inner: &mut Inner, at: usize, flags: u32) -> Cancelled {
        let dialog = match inner.requests[at].phase {
            Phase::Open { dialog } => Some(dialog),
            _ => None,
        };
        let id = inner.requests[at].id;
        let child = inner.requests[at].child.take();
        {
            let request = &mut inner.requests[at];
            request.phase = Phase::Done;
            request.pending = Some(Answer {
                outcome: CANCELLED,
                choice: 0,
                length: 0,
                flags,
            });
        }
        if let Some(child) = child
            && let Some(child_at) = Self::request_at(inner, child)
            && inner.requests[child_at].open()
        {
            Self::cancel_at(inner, child_at, 0);
        }
        Cancelled {
            request: id,
            dialog,
        }
    }

    /// The asker's callback returned (`failed`: with a failure status): each
    /// request it asked in this callback either starts (`start` is called for
    /// it, in asking order) or, when the callback failed or left the initial
    /// text begun but not committed, is cancelled with `not_started` before any
    /// dialog opens.
    pub(crate) fn callback_returned(
        &self,
        who: Stamp,
        failed: bool,
        start: &mut dyn FnMut(Start),
    ) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        for at in 0..inner.requests.len() {
            let request = &inner.requests[at];
            if request.asker != who.owner || request.phase != (Phase::Asked { epoch: who.epoch }) {
                continue;
            }
            let uncommitted = request.text.bytes > 0 && !request.text.committed;
            if failed || uncommitted {
                Self::cancel_at(&mut inner, at, NOT_STARTED);
            } else {
                inner.requests[at].phase = Phase::Starting;
                let r = &inner.requests[at];
                start(Start {
                    request: r.id,
                    asker: r.asker,
                    kind: r.kind,
                    remote: r.remote,
                    secret: r.secret,
                });
            }
        }
        Ok(())
    }

    /// The asker's callback of `who` returned a failure: what it asked in that
    /// callback never starts (`not_started`, no dialog opens).
    pub(crate) fn callback_failed(&self, who: Stamp) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        for at in 0..inner.requests.len() {
            let request = &inner.requests[at];
            if request.asker == who.owner && request.phase == (Phase::Asked { epoch: who.epoch }) {
                Self::cancel_at(&mut inner, at, NOT_STARTED);
            }
        }
        Ok(())
    }

    /// No callback is running (the dialog pump runs between callbacks): every
    /// request still asked has had its callback return, so it starts, or is
    /// cancelled with `not_started` when its initial text was left begun.
    pub(crate) fn callbacks_over(&self) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        for at in 0..inner.requests.len() {
            if !matches!(inner.requests[at].phase, Phase::Asked { .. }) {
                continue;
            }
            let request = &inner.requests[at];
            if request.text.bytes > 0 && !request.text.committed {
                Self::cancel_at(&mut inner, at, NOT_STARTED);
            } else {
                inner.requests[at].phase = Phase::Starting;
            }
        }
        Ok(())
    }

    /// The oldest request whose dialog the stack has yet to open.
    pub(crate) fn next_starting(&self) -> Option<Start> {
        let inner = self.inner.try_borrow().ok()?;
        inner
            .requests
            .iter()
            .find(|r| r.phase == Phase::Starting)
            .map(|r| Start {
                request: r.id,
                asker: r.asker,
                kind: r.kind,
                remote: r.remote,
                secret: r.secret,
            })
    }

    /// The stack is about to open the dialog of a starting request as the
    /// instance `instance` (the next serial): the request is open for it from
    /// here, so that the dialog reads its question (op 2) and its text in its
    /// own `screen_opened`, which runs inside the open.
    pub(crate) fn dialog_opening(&self, request: u32, instance: u32) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let at = Self::request_at(&inner, request).ok_or(ERR_NOT_FOUND)?;
        if inner.requests[at].phase != Phase::Starting {
            return Err(ERR_STATE);
        }
        inner.requests[at].phase = Phase::Open { dialog: instance };
        inner.requests[at].dialog = Some(instance);
        Ok(())
    }

    /// The stack opened the dialog of a starting request (or of one
    /// [`Self::dialog_opening`] named: it keeps the instance that was open
    /// first, or is bound to the one that opened).
    pub(crate) fn dialog_opened(&self, request: u32, instance: u32) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let at = Self::request_at(&inner, request).ok_or(ERR_NOT_FOUND)?;
        if !matches!(
            inner.requests[at].phase,
            Phase::Starting | Phase::Open { .. }
        ) {
            return Err(ERR_STATE);
        }
        inner.requests[at].phase = Phase::Open { dialog: instance };
        inner.requests[at].dialog = Some(instance);
        Ok(())
    }

    /// The dialog of a starting request could not open (the stack is full, a
    /// guard refused): cancelled with `not_started`, also where it was named
    /// open for the instance that did not come.
    pub(crate) fn dialog_failed(&self, request: u32) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        if let Some(at) = Self::request_at(&inner, request)
            && matches!(
                inner.requests[at].phase,
                Phase::Starting | Phase::Open { .. }
            )
        {
            Self::cancel_at(&mut inner, at, NOT_STARTED);
        }
        Ok(())
    }

    /// A dialog instance closed: its request, if still open, is cancelled; one
    /// already answered notes the dialog is gone.
    pub(crate) fn dialog_closed(&self, instance: u32) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        for at in 0..inner.requests.len() {
            match inner.requests[at].phase {
                Phase::Open { dialog } if dialog == instance => {
                    Self::cancel_at(&mut inner, at, DIALOG_CLOSED);
                }
                // Already answered: the answer still waits, and it will say that
                // its dialog is gone.
                Phase::Done if inner.requests[at].dialog == Some(instance) => {
                    inner.requests[at].dialog_gone = true;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// A screen instance closed: the requests it asked are cancelled; the
    /// dialogs to close are told to `close`.
    pub(crate) fn asker_instance_closed(
        &self,
        instance: u32,
        close: &mut dyn FnMut(Cancelled),
    ) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        while let Some(at) = inner
            .requests
            .iter()
            .position(|r| r.tie == Some(instance) && r.open() && r.parent.is_none())
        {
            let asker = inner.requests[at].asker;
            if let Some(done) = Self::cancel_asker(&mut inner, asker, Some(instance)) {
                close(done);
            }
        }
        Ok(())
    }

    /// Op 2: the question, for the request's dialog instance only.
    pub(crate) fn question<R>(
        &self,
        dialog: u32,
        request: u32,
        f: impl FnOnce(&Shown<'_>) -> R,
    ) -> Result<R, i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_STATE)?;
        let at = Self::request_at(&inner, request).ok_or(ERR_NOT_FOUND)?;
        let r = &inner.requests[at];
        if r.phase != (Phase::Open { dialog }) {
            return Err(ERR_STATE);
        }
        let max = if r.secret { SECRET_TEXT_MAX } else { TEXT_MAX };
        Ok(f(&Shown {
            kind: r.kind,
            flags: r.flags,
            max_bytes: u32::try_from(max).unwrap_or(u32::MAX),
            title: as_str(&r.title),
            text: as_str(&r.question),
            choices: as_str(&r.choices),
        }))
    }

    /// Op 9 on a request: by the asker in the callback that asked (the initial
    /// text), then only by the dialog instance until its final answer.
    pub(crate) fn request_text_write(
        &self,
        who: Stamp,
        caller_instance: Option<u32>,
        id: u32,
        offset: usize,
        data: &[u8],
        last: bool,
    ) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let at = Self::request_at(&inner, id).ok_or(ERR_NOT_FOUND)?;
        let allowed = match inner.requests[at].phase {
            Phase::Asked { epoch } => inner.requests[at].asker == who.owner && epoch == who.epoch,
            Phase::Open { dialog } => caller_instance == Some(dialog),
            _ => false,
        };
        if !allowed {
            return Err(ERR_STATE);
        }
        let secret = inner.requests[at].secret;
        let slots_free = Self::text_slots(&inner) < self.limits.texts;
        let text_cap = if secret { SECRET_TEXT_MAX } else { TEXT_MAX };
        let Inner {
            requests,
            text_memory,
            secret_memory,
            ..
        } = &mut *inner;
        let request = &mut requests[at];
        // A final answer is withdrawn only for typing; writing never touches it.
        if request.pending.is_some_and(|a| a.outcome == TYPING) {
            request.pending = None;
        }
        write_part(
            &mut request.text,
            // A secret text is charged to its own pool, never to the ordinary
            // one, so a password does not take room from unrelated text.
            if secret { secret_memory } else { text_memory },
            &WriteRules {
                slots_free,
                memory_cap: if secret {
                    self.limits.secret_memory
                } else {
                    self.limits.text_memory
                },
                text_cap,
            },
            offset,
            data,
            last,
        )
    }

    /// Op 11: the dialog's answer. `Err` leaves the request as it was.
    pub(crate) fn answer(
        &self,
        dialog: u32,
        request: u32,
        outcome: u32,
        choice: u32,
    ) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let at = Self::request_at(&inner, request).ok_or(ERR_NOT_FOUND)?;
        let typing_bytes = self.limits.typing_bytes;
        let r = &mut inner.requests[at];
        if r.phase != (Phase::Open { dialog }) {
            return Err(ERR_STATE);
        }
        if outcome > TYPING {
            return Err(ERR_INVALID);
        }
        let mut answer = Answer {
            outcome,
            choice: 0,
            length: 0,
            flags: 0,
        };
        if outcome != CANCELLED {
            match r.kind {
                0 | 3 => {
                    if !r.text.committed {
                        return Err(ERR_STATE);
                    }
                    if r.kind == 3 && r.text.bytes > FOLDER_MAX {
                        return Err(ERR_INVALID);
                    }
                    answer.length = u32::try_from(r.text.bytes).map_err(|_| ERR_INVALID)?;
                    if outcome == TYPING {
                        if r.kind != 0 || r.flags & LIVE == 0 {
                            return Err(ERR_INVALID);
                        }
                        if !r.secret && r.text.bytes > typing_bytes {
                            // The live text stopped; the final answer is whole.
                            r.pending = Some(Answer {
                                outcome: TYPING,
                                flags: TYPING_STOPPED,
                                ..answer
                            });
                            return Err(ERR_NO_SPACE);
                        }
                    }
                }
                1 => {
                    if outcome == TYPING {
                        return Err(ERR_INVALID);
                    }
                    if usize::try_from(choice).map_err(|_| ERR_INVALID)?
                        >= lines(as_str(&r.choices))
                    {
                        return Err(ERR_INVALID);
                    }
                    answer.choice = choice;
                }
                _ => {
                    if outcome == TYPING || choice > 1 {
                        return Err(ERR_INVALID);
                    }
                    answer.choice = choice;
                }
            }
        }
        if outcome == TYPING {
            r.pending = Some(answer);
            return Ok(());
        }
        let child = r.child.take();
        r.phase = Phase::Done;
        r.pending = Some(answer);
        if let Some(child) = child
            && let Some(child_at) = Self::request_at(&inner, child)
            && inner.requests[child_at].open()
        {
            Self::cancel_at(&mut inner, child_at, 0);
        }
        Ok(())
    }

    /// The next answer waiting for `asker`: marks the request as delivering (the
    /// asker may read its text in that callback). Typing updates replace each
    /// other; a final answer replaces a pending typing update.
    pub(crate) fn take_delivery(&self, asker: u64) -> Option<Delivery> {
        let mut inner = self.inner().ok()?;
        let at = inner
            .requests
            .iter()
            .position(|r| r.asker == asker && r.pending.is_some() && !r.delivering)?;
        let r = &mut inner.requests[at];
        let answer = r.pending.take()?;
        r.delivering = true;
        let mut flags = answer.flags;
        if r.dialog_gone || (answer.outcome != TYPING && answer.flags & DIALOG_CLOSED != 0) {
            flags |= DIALOG_CLOSED;
        }
        Some(Delivery {
            request: r.id,
            kind: r.kind,
            outcome: answer.outcome,
            choice: answer.choice,
            length: answer.length,
            flags,
        })
    }

    /// Whether an answer waits for `asker`.
    pub(crate) fn has_delivery(&self, asker: u64) -> bool {
        self.inner.try_borrow().is_ok_and(|inner| {
            inner
                .requests
                .iter()
                .any(|r| r.asker == asker && r.pending.is_some() && !r.delivering)
        })
    }

    /// The callback for `delivery` could not run (the plugin was busy or not
    /// known): the answer waits as it was, for the next pass.
    pub(crate) fn undo_delivery(&self, delivery: &Delivery) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        if let Some(at) = Self::request_at(&inner, delivery.request) {
            let r = &mut inner.requests[at];
            if r.delivering && r.pending.is_none() {
                r.delivering = false;
                r.pending = Some(Answer {
                    outcome: delivery.outcome,
                    choice: delivery.choice,
                    length: delivery.length,
                    flags: delivery.flags,
                });
            }
        }
        Ok(())
    }

    /// The callback that delivered `request` returned: a final answer frees the
    /// request and its text whatever the callback's status; typing keeps it.
    pub(crate) fn delivered(&self, request: u32) -> Result<(), i32> {
        let mut inner = self.inner_busy()?;
        if let Some(at) = Self::request_at(&inner, request) {
            inner.requests[at].delivering = false;
            if inner.requests[at].phase == Phase::Done && inner.requests[at].pending.is_none() {
                Self::free_request(&mut inner, at);
            }
        }
        Ok(())
    }

    /// A host dialog's answer was not taken (a choice outside its list): the
    /// pump logs it.
    pub(crate) fn note_refused(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.refused = inner.refused.saturating_add(1);
        }
    }

    /// How many answers were refused since the last call.
    pub(crate) fn take_refused(&self) -> u32 {
        self.inner
            .try_borrow_mut()
            .map_or(0, |mut inner| core::mem::take(&mut inner.refused))
    }

    /// The question of a request whose dialog the host is about to open: for
    /// the host's own dialogs, which are made with it.
    pub(crate) fn starting_question<R>(
        &self,
        request: u32,
        f: impl FnOnce(&Shown<'_>) -> R,
    ) -> Result<R, i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_STATE)?;
        let at = Self::request_at(&inner, request).ok_or(ERR_NOT_FOUND)?;
        let r = &inner.requests[at];
        if r.phase != Phase::Starting {
            return Err(ERR_STATE);
        }
        let max = if r.secret { SECRET_TEXT_MAX } else { TEXT_MAX };
        Ok(f(&Shown {
            kind: r.kind,
            flags: r.flags,
            max_bytes: u32::try_from(max).unwrap_or(u32::MAX),
            title: as_str(&r.title),
            text: as_str(&r.question),
            choices: as_str(&r.choices),
        }))
    }

    /// Op 10 on a request: the dialog while it is open, the asker in the
    /// callback that delivers the answer.
    pub(crate) fn request_text_read(
        &self,
        who: u64,
        caller_instance: Option<u32>,
        id: u32,
        offset: usize,
        out: &mut [u8],
    ) -> Result<(usize, usize), i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_STATE)?;
        let at = Self::request_at(&inner, id).ok_or(ERR_NOT_FOUND)?;
        let r = &inner.requests[at];
        // The dialog while it is open; the asker in the callback that delivers
        // an answer, a typing update included (the request is still open then).
        let allowed = (r.asker == who && r.delivering)
            || matches!(r.phase, Phase::Open { dialog } if caller_instance == Some(dialog));
        if !allowed || (!r.text.committed && r.text.bytes > 0) {
            return Err(ERR_STATE);
        }
        Self::read_text(&r.text, offset, out)
    }

    /// A plugin stopped: the requests it asked are freed with no event and its
    /// nested ones with them. A dialog of a request it owned is cancelled via
    /// [`UiStore::dialog_closed`] when the stack closes it.
    pub(crate) fn owner_dialogs(&self, owner: u64, visit: &mut dyn FnMut(u32)) -> Result<(), i32> {
        let inner = self.inner_busy()?;
        for request in &inner.requests {
            let parent_owned = request.parent.is_some_and(|id| {
                inner
                    .requests
                    .iter()
                    .any(|parent| parent.id == id && parent.asker == owner)
            });
            if (request.asker == owner || parent_owned)
                && let Some(dialog) = request.dialog
            {
                visit(dialog);
            }
        }
        Ok(())
    }

    /// Whether `owner` asked a request still open (#248, D3).
    pub(super) fn requests_owner_holds(inner: &Inner, owner: u64) -> bool {
        inner.requests.iter().any(|r| r.asker == owner)
    }

    pub(super) fn requests_owner_stopped(inner: &mut Inner, owner: u64) {
        // Each row the stopped plugin asked goes with the nested rows that hang
        // under it (asked by the keyboard, not by the stopped plugin). Rows of
        // others whose parent is merely gone (an answered parent already
        // delivered) are not touched: their event is still owed.
        while let Some(at) = inner.requests.iter().position(|r| r.asker == owner) {
            let id = inner.requests[at].id;
            Self::free_with_nested(inner, id);
        }
    }

    /// Frees request `id` and the rows hanging under it, nested ones first.
    fn free_with_nested(inner: &mut Inner, id: u32) {
        while let Some(at) = inner.requests.iter().position(|r| r.parent == Some(id)) {
            let child = inner.requests[at].id;
            Self::free_with_nested(inner, child);
        }
        if let Some(at) = Self::request_at(inner, id) {
            Self::free_request(inner, at);
        }
    }

    fn free_request(inner: &mut Inner, at: usize) {
        let request = inner.requests.remove(at);
        let pool = if request.secret {
            &mut inner.secret_memory
        } else {
            &mut inner.text_memory
        };
        *pool -= request.text.charged.min(*pool);
        inner.question_memory -= request.asked_bytes.min(inner.question_memory);
        if let Some(parent) = request.parent
            && let Some(parent_at) = Self::request_at(inner, parent)
        {
            inner.requests[parent_at].child = None;
        }
    }

    /// Bytes of the question a dialog may show at most for any request.
    #[cfg(test)]
    pub(crate) fn request_count(&self) -> usize {
        self.inner.borrow().requests.len()
    }
}

#[cfg(test)]
#[path = "ui_requests_tests.rs"]
mod tests;

const _: () = assert!(PART_MAX >= 3_072);
