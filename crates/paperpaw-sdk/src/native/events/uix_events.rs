//! UIX events retain only validated host identifiers and borrowed strings.
use super::{Error, Event, wire};
use crate::native::ui::{HandoverId, Instance, QuestionKind, RequestId};

/// A request's current outcome. Unknown future outcomes cancel the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnswerOutcome {
    /// A complete answer is ready.
    Answered,
    /// Cancelled without applying an edit.
    Cancelled,
    /// A committed live-text update; the request remains open.
    Typing,
}

/// A host-stamped answer. Its text is read during this event's callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answered {
    request: RequestId,
    kind: QuestionKind,
    outcome: AnswerOutcome,
    choice: u32,
    length: u32,
    flags: u32,
}
impl Answered {
    /// The request assigned when the question was asked.
    #[must_use]
    pub const fn request(self) -> RequestId {
        self.request
    }
    /// Which kind of question was answered.
    #[must_use]
    pub const fn kind(self) -> QuestionKind {
        self.kind
    }
    /// Answered, cancelled or a live typing update.
    #[must_use]
    pub const fn outcome(self) -> AnswerOutcome {
        self.outcome
    }
    /// Chosen zero-based index, or 1 yes / 0 no for a confirmation.
    #[must_use]
    pub const fn choice(self) -> u32 {
        self.choice
    }
    /// The complete answer text length in bytes, never a truncated length.
    #[must_use]
    pub const fn text_bytes(self) -> u32 {
        self.length
    }
    /// The dialog closed itself after answering.
    #[must_use]
    pub const fn dialog_closed(self) -> bool {
        self.flags & 1 != 0
    }
    /// The host could not start the dialog.
    #[must_use]
    pub const fn not_started(self) -> bool {
        self.flags & 2 != 0
    }
    /// Live typing stopped at the host limit; this update is not the whole text.
    #[must_use]
    pub const fn typing_stopped(self) -> bool {
        self.flags & 4 != 0
    }
}

/// The completion status of speech requested with a cookie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpeechStatus {
    /// The text was spoken.
    Spoken,
    /// Replaced or stopped.
    Stopped,
    /// No voice for that language.
    NoVoice,
    /// This device has no speech. Unknown statuses use this safe result.
    Unsupported,
}

event_fn!(screen_opened, wire::EventScreenOpened<'_>, |r| {
    Event::ScreenOpened {
        now_ms: r.now_ms,
        screen: r.screen,
        instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
        slot: r.slot,
        handover: if r.handover == 0 {
            None
        } else {
            Some(HandoverId::new(r.handover).ok_or(Error::Invalid)?)
        },
        params: r.params,
        text_bytes: r.text_bytes,
        again: r.flags & 1 != 0,
        restored: r.flags & 2 != 0,
    }
});

event_fn!(screen_closed, wire::EventScreenClosed<'_>, |r| {
    Event::ScreenClosed {
        now_ms: r.now_ms,
        screen: r.screen,
        instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
    }
});

event_fn!(screen_hidden, wire::EventScreenHidden<'_>, |r| {
    Event::ScreenHidden {
        now_ms: r.now_ms,
        screen: r.screen,
        instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
    }
});

event_fn!(stack_changed, wire::EventStackChanged, |r| {
    Event::StackChanged { now_ms: r.now_ms }
});

event_fn!(answered, wire::EventAnswered, |r| Event::Answered {
    now_ms: r.now_ms,
    answer: Answered {
        request: RequestId::new(r.request).ok_or(Error::Invalid)?,
        kind: QuestionKind::decode(r.kind)?,
        outcome: match r.outcome {
            0 => AnswerOutcome::Answered,
            2 => AnswerOutcome::Typing,
            _ => AnswerOutcome::Cancelled,
        },
        choice: r.choice,
        length: r.length,
        flags: r.flags,
    },
});

event_fn!(widget_made, wire::EventWidgetMade<'_>, |r| {
    Event::WidgetMade {
        now_ms: r.now_ms,
        widget: r.widget,
        instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
        place: r.place,
    }
});

event_fn!(widget_dropped, wire::EventWidgetDropped, |r| {
    Event::WidgetDropped {
        now_ms: r.now_ms,
        instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
    }
});

event_fn!(widget_changed, wire::EventWidgetChanged, |r| {
    Event::WidgetChanged {
        now_ms: r.now_ms,
        instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
    }
});

event_fn!(screen_ready, wire::EventScreenReady<'_>, |r| {
    Event::ScreenReady {
        now_ms: r.now_ms,
        screen: r.screen,
        instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
    }
});

event_fn!(spoken, wire::EventSpoken, |r| Event::Spoken {
    now_ms: r.now_ms,
    cookie: r.cookie,
    status: match r.status {
        0 => SpeechStatus::Spoken,
        1 => SpeechStatus::Stopped,
        2 => SpeechStatus::NoVoice,
        _ => SpeechStatus::Unsupported,
    },
});
