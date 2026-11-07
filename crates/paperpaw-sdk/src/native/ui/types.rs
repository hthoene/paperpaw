//! Borrowed questions and host-issued identifiers.
use super::{Error, wire};

macro_rules! identifier {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(core::num::NonZeroU32);
        impl $name {
            /// Validate a nonzero identifier received from the host.
            #[must_use]
            pub(crate) const fn new(value: u32) -> Option<Self> {
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
/// A request or handover text obtained from a decoded host identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextId(core::num::NonZeroU32);
impl TextId {
    /// The host-issued identifier.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}
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
    pub(crate) fn decode(value: u32) -> Result<Self, Error> {
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
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn text(title: &'a str, question: &'a str) -> Result<Self, Error> {
        Self::new(QuestionKind::Text, title, question).checked()
    }
    /// Ask for one of the newline-separated choices. Empty lines are refused.
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn choice(title: &'a str, question: &'a str, choices: &'a str) -> Result<Self, Error> {
        Self {
            choices,
            ..Self::new(QuestionKind::Choice, title, question)
        }
        .checked()
    }
    /// Ask a yes-or-no question.
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn confirm(title: &'a str, question: &'a str) -> Result<Self, Error> {
        Self::new(QuestionKind::Confirm, title, question).checked()
    }
    /// Set confirmation button labels as exactly two lines, `yes\nno`.
    /// An empty label retains the dialog's default word; empty text resets both.
    ///
    /// # Errors
    /// Invalid for another kind or malformed lines; NoSpace above 128 bytes per label.
    pub fn confirm_labels(mut self, labels: &'a str) -> Result<Self, Error> {
        if self.kind != QuestionKind::Confirm {
            return Err(Error::Invalid);
        }
        self.choices = labels;
        self.checked()
    }
    /// Show only the confirmation's yes button, whose default word is OK.
    ///
    /// # Errors
    /// Invalid for another question kind or a nonempty no label.
    pub fn only_yes(mut self) -> Result<Self, Error> {
        self.flags |= 1 << 8;
        self.checked()
    }
    /// Start confirmation focus on the yes button.
    ///
    /// # Errors
    /// Invalid for another question kind.
    pub fn focus_yes(mut self) -> Result<Self, Error> {
        self.flags |= 1 << 9;
        self.checked()
    }
    /// Ask for a root-1 folder. Write the initial folder to the request text.
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn folder(title: &'a str, question: &'a str) -> Result<Self, Error> {
        Self::new(QuestionKind::Folder, title, question).checked()
    }
    /// Receive typing updates. Valid for text questions only.
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn live(mut self) -> Result<Self, Error> {
        self.flags |= 1;
        self.checked()
    }
    /// Allow multiple lines. Valid for text questions only.
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn multiline(mut self) -> Result<Self, Error> {
        self.flags |= 2;
        self.checked()
    }
    /// Mask text and keep its host storage off the card. Text questions only.
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn secret(mut self) -> Result<Self, Error> {
        self.flags |= 4;
        self.checked()
    }
    /// Supply a keyboard mode. Valid for text questions only.
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn mode(mut self, mode: TextMode) -> Result<Self, Error> {
        self.flags = (self.flags & !0xf0) | mode.bits() << 4;
        self.checked()
    }
    /// Use the remote keyboard. Valid for text questions only.
    ///
    /// # Errors
    /// Invalid for an incompatible question kind; NoSpace above a field cap.
    pub fn remote(mut self) -> Result<Self, Error> {
        self.slot = "core.remote-keyboard";
        self.checked()
    }
    fn checked(self) -> Result<Self, Error> {
        self.record()?;
        Ok(self)
    }
    pub(super) fn record(self) -> Result<wire::Record<'a>, Error> {
        if self.kind != QuestionKind::Text && !self.slot.is_empty() {
            return Err(Error::Invalid);
        }
        validate_question(self.kind, self.flags, self.choices)?;
        let record = wire::Record::ServiceUiAsk(wire::ServiceUiAsk {
            kind: self.kind.encode(),
            flags: self.flags,
            slot: self.slot,
            title: self.title,
            text: self.text,
            choices: self.choices,
        });
        wire::record_framed_len(&record).map_err(Error::from_status)?;
        Ok(record)
    }
}

pub(super) fn validate_question(
    kind: QuestionKind,
    flags: u32,
    choices: &str,
) -> Result<(), Error> {
    let allowed = match kind {
        QuestionKind::Text => 0xf7,
        QuestionKind::Confirm => 0x300,
        _ => 0,
    };
    if flags & !allowed != 0 || kind == QuestionKind::Text && flags >> 4 & 15 > 4 {
        return Err(Error::Invalid);
    }
    match kind {
        QuestionKind::Confirm => {
            if !choices.is_empty() {
                let (yes, no) = choices.split_once('\n').ok_or(Error::Invalid)?;
                if no.contains('\n') || flags & 0x100 != 0 && !no.is_empty() {
                    return Err(Error::Invalid);
                }
                if yes.len() > 128 || no.len() > 128 {
                    return Err(Error::NoSpace);
                }
            }
        }
        QuestionKind::Choice => {
            if choices.is_empty() || choices.split('\n').any(str::is_empty) {
                return Err(Error::Invalid);
            }
            if choices.len() > wire::SERVICE_UI_ASK_CHOICES_MAX_BYTES
                || choices.split('\n').count() > wire::SERVICE_UI_ASK_CHOICES_MAX_ITEMS
                || choices
                    .split('\n')
                    .any(|label| label.len() > wire::SERVICE_UI_ASK_CHOICES_ITEM_MAX_BYTES)
            {
                return Err(Error::NoSpace);
            }
        }
        _ if !choices.is_empty() => return Err(Error::Invalid),
        _ => {}
    }
    Ok(())
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
    /// Custom yes label, or None to retain the dialog's default word.
    #[must_use]
    pub fn yes_label(self) -> Option<&'a str> {
        (self.kind == QuestionKind::Confirm)
            .then(|| self.choices.split_once('\n').map(|(yes, _)| yes))
            .flatten()
            .filter(|label| !label.is_empty())
    }
    /// Custom no label, or None to retain the dialog's default word.
    #[must_use]
    pub fn no_label(self) -> Option<&'a str> {
        (self.kind == QuestionKind::Confirm)
            .then(|| self.choices.split_once('\n').map(|(_, no)| no))
            .flatten()
            .filter(|label| !label.is_empty())
    }
    /// The confirmation shows only its yes button.
    #[must_use]
    pub const fn only_yes(self) -> bool {
        self.flags & 0x100 != 0
    }
    /// The confirmation starts with focus on yes.
    #[must_use]
    pub const fn focus_yes(self) -> bool {
        self.flags & 0x200 != 0
    }

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
