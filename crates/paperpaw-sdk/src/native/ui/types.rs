//! Borrowed questions and host-issued identifiers.
use super::{Error, wire};

macro_rules! identifier {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(core::num::NonZeroU32);
        impl $name {
            /// Validate a nonzero identifier received from the host.
            #[must_use]
            pub const fn new(value: u32) -> Option<Self> {
                match core::num::NonZeroU32::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
            /// The host-issued identifier, unchanged.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0.get()
            }
        }
    };
}
identifier!(
    Instance,
    "A screen or widget instance assigned by the host."
);
identifier!(
    RequestId,
    "A question request assigned by the host, also its text identifier."
);
identifier!(
    HandoverId,
    "A handover assigned by the host, also its text identifier."
);
identifier!(TextId, "A request or handover text assigned by the host.");
impl RequestId {
    /// The request's host text, readable only during the permitted callbacks.
    #[must_use]
    pub const fn text(self) -> TextId {
        TextId(self.0)
    }
}
impl HandoverId {
    /// The handover's host text, readable while its screen remains open.
    #[must_use]
    pub const fn text(self) -> TextId {
        TextId(self.0)
    }
}

/// The dialog used for a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum QuestionKind {
    /// Editable text.
    Text,
    /// One choice from a list.
    Choice,
    /// Yes or no.
    Confirm,
    /// A folder on root 1.
    Folder,
}
impl QuestionKind {
    pub(super) const fn encode(self) -> u32 {
        match self {
            Self::Text => 0,
            Self::Choice => 1,
            Self::Confirm => 2,
            Self::Folder => 3,
        }
    }
    pub(super) fn decode(value: u32) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Text),
            1 => Ok(Self::Choice),
            2 => Ok(Self::Confirm),
            3 => Ok(Self::Folder),
            _ => Err(Error::Invalid),
        }
    }
}

/// Keyboard hints for a text question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextMode {
    /// Plain text.
    Plain,
    /// A number.
    Number,
    /// An email address.
    Email,
    /// A URL.
    Url,
    /// A file name.
    FileName,
}
impl TextMode {
    const fn bits(self) -> u32 {
        match self {
            Self::Plain => 0,
            Self::Number => 1,
            Self::Email => 2,
            Self::Url => 3,
            Self::FileName => 4,
        }
    }
}

/// A question borrowing formatted display text. Initial answer text is written
/// separately through [`super::Ui::text_write`] before the callback returns.
#[derive(Clone, Copy, Debug)]
pub struct Question<'a> {
    kind: QuestionKind,
    flags: u32,
    slot: &'a str,
    title: &'a str,
    text: &'a str,
    choices: &'a str,
}
impl<'a> Question<'a> {
    const fn new(kind: QuestionKind, title: &'a str, text: &'a str) -> Self {
        Self {
            kind,
            flags: 0,
            slot: "",
            title,
            text,
            choices: "",
        }
    }
    /// Ask for text with the system keyboard.
    #[must_use]
    pub const fn text(title: &'a str, question: &'a str) -> Self {
        Self::new(QuestionKind::Text, title, question)
    }
    /// Ask for one of the newline-separated choices. Empty lines are refused.
    #[must_use]
    pub const fn choice(title: &'a str, question: &'a str, choices: &'a str) -> Self {
        Self {
            choices,
            ..Self::new(QuestionKind::Choice, title, question)
        }
    }
    /// Ask a yes-or-no question.
    #[must_use]
    pub const fn confirm(title: &'a str, question: &'a str) -> Self {
        Self::new(QuestionKind::Confirm, title, question)
    }
    /// Ask for a root-1 folder. Write the initial folder to the request text.
    #[must_use]
    pub const fn folder(title: &'a str, question: &'a str) -> Self {
        Self::new(QuestionKind::Folder, title, question)
    }
    /// Receive typing updates. Valid for text questions only.
    #[must_use]
    pub const fn live(mut self) -> Self {
        self.flags |= 1;
        self
    }
    /// Allow multiple lines. Valid for text questions only.
    #[must_use]
    pub const fn multiline(mut self) -> Self {
        self.flags |= 2;
        self
    }
    /// Mask text and keep its host storage off the card. Text questions only.
    #[must_use]
    pub const fn secret(mut self) -> Self {
        self.flags |= 4;
        self
    }
    /// Supply a keyboard mode. Valid for text questions only.
    #[must_use]
    pub const fn mode(mut self, mode: TextMode) -> Self {
        self.flags = (self.flags & !0xf0) | mode.bits() << 4;
        self
    }
    /// Use the remote keyboard. Valid for text questions only.
    #[must_use]
    pub const fn remote(mut self) -> Self {
        self.slot = "core.remote-keyboard";
        self
    }
    pub(super) fn record(self) -> Result<wire::Record<'a>, Error> {
        if self.kind != QuestionKind::Text && (self.flags != 0 || !self.slot.is_empty()) {
            return Err(Error::Invalid);
        }
        if self.kind == QuestionKind::Choice {
            if self.choices.is_empty() || self.choices.ends_with('\n') {
                return Err(Error::Invalid);
            }
            if self.choices.split('\n').any(str::is_empty) {
                return Err(Error::Invalid);
            }
            if self.choices.len() > wire::SERVICE_UI_ASK_CHOICES_MAX_BYTES
                || self.choices.split('\n').count() > wire::SERVICE_UI_ASK_CHOICES_MAX_ITEMS
                || self
                    .choices
                    .split('\n')
                    .any(|choice| choice.len() > wire::SERVICE_UI_ASK_CHOICES_ITEM_MAX_BYTES)
            {
                return Err(Error::NoSpace);
            }
        }
        Ok(wire::Record::ServiceUiAsk(wire::ServiceUiAsk {
            kind: self.kind.encode(),
            flags: self.flags,
            slot: self.slot,
            title: self.title,
            text: self.text,
            choices: self.choices,
        }))
    }
}

/// A dialog's answer. The host verifies the request kind and ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Answer {
    /// Commit the request's whole text or folder.
    Accept,
    /// Cancel without applying an edit.
    Cancel,
    /// Report committed live text and keep the question open.
    Typing,
    /// Select a zero-based choice index.
    Pick(u32),
    /// Confirm yes.
    Yes,
    /// Confirm no.
    No,
}
impl Answer {
    pub(super) const fn fields(self) -> (u32, u32) {
        match self {
            Self::Accept | Self::No => (0, 0),
            Self::Cancel => (1, 0),
            Self::Typing => (2, 0),
            Self::Pick(choice) => (0, choice),
            Self::Yes => (0, 1),
        }
    }
}

/// Whether a written host-text part commits the complete text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextWrite {
    /// More parts follow.
    Continue,
    /// Commit the complete text at the end of this part.
    Last,
}
impl TextWrite {
    pub(super) const fn flags(self) -> u32 {
        match self {
            Self::Continue => 0,
            Self::Last => 1,
        }
    }
}

/// One UTF-8 host-text part, borrowing the caller's reply buffer.
#[derive(Clone, Copy, Debug)]
pub struct TextPart<'a> {
    pub(super) text: u32,
    pub(super) offset: u32,
    pub(super) total: u32,
    pub(super) data: &'a str,
}
impl<'a> TextPart<'a> {
    /// Host text identifier; zero denotes the clipboard.
    #[must_use]
    pub const fn text(self) -> u32 {
        self.text
    }
    /// Byte offset of this part.
    #[must_use]
    pub const fn offset(self) -> u32 {
        self.offset
    }
    /// Complete text length in bytes.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// This whole UTF-8 part.
    #[must_use]
    pub const fn data(self) -> &'a str {
        self.data
    }
    /// Offset to request next, or None after the complete text.
    #[must_use]
    pub fn next_offset(self) -> Option<u32> {
        let end = self
            .offset
            .checked_add(u32::try_from(self.data.len()).ok()?)?;
        (end < self.total).then_some(end)
    }
}

/// A dialog's question, borrowed from its reply buffer.
#[derive(Clone, Copy, Debug)]
pub struct QuestionInfo<'a> {
    pub(super) kind: QuestionKind,
    pub(super) flags: u32,
    pub(super) max_bytes: u32,
    pub(super) title: &'a str,
    pub(super) text: &'a str,
    pub(super) choices: &'a str,
}
impl<'a> QuestionInfo<'a> {
    /// Which dialog was requested.
    #[must_use]
    pub const fn kind(self) -> QuestionKind {
        self.kind
    }
    /// Maximum complete answer text length, in bytes, on this host.
    #[must_use]
    pub const fn max_bytes(self) -> u32 {
        self.max_bytes
    }
    /// Formatted display title.
    #[must_use]
    pub const fn title(self) -> &'a str {
        self.title
    }
    /// Formatted question text.
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.text
    }
    /// Choice labels, in the order used by Answer::Pick.
    pub fn choices(self) -> impl Iterator<Item = &'a str> {
        self.choices.split('\n').filter(|value| !value.is_empty())
    }
    /// The asker requested typing updates.
    #[must_use]
    pub const fn is_live(self) -> bool {
        self.flags & 1 != 0
    }
    /// The dialog may accept multiple lines.
    #[must_use]
    pub const fn is_multiline(self) -> bool {
        self.flags & 2 != 0
    }
    /// The dialog must mask its text and keep it off the clipboard.
    #[must_use]
    pub const fn is_secret(self) -> bool {
        self.flags & 4 != 0
    }
    /// The requested keyboard mode.
    #[must_use]
    pub const fn mode(self) -> TextMode {
        match self.flags >> 4 & 15 {
            1 => TextMode::Number,
            2 => TextMode::Email,
            3 => TextMode::Url,
            4 => TextMode::FileName,
            _ => TextMode::Plain,
        }
    }
}
