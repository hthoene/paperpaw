//! Full-id navigation with an optional host-owned handover.
use super::{Error, HandoverId};
use crate::native::wire::commands_wire::Command;

/// One navigation command. The host resolves its target and applies it after
/// the callback. A handover must have been made in that same callback.
#[derive(Clone, Copy, Debug)]
pub struct Open<'a> {
    target: &'a str,
    slot: &'a str,
    mode: u32,
    handover: Option<HandoverId>,
}
impl<'a> Open<'a> {
    fn checked(target: &'a str, slot: &'a str, mode: u32) -> Result<Self, Error> {
        full_id(target)?;
        if mode == 3 {
            full_id(slot)?;
        }
        Ok(Self {
            target,
            slot,
            mode,
            handover: None,
        })
    }
    /// Open a full screen id, refusing invalid ids or caps without truncation.
    ///
    /// # Errors
    /// Invalid for malformed ids; NoSpace above the contract cap.
    pub fn screen(target: &'a str) -> Result<Self, Error> {
        Self::checked(target, "", 0)
    }
    /// Open a full slot id.
    ///
    /// # Errors
    /// Invalid for malformed ids; NoSpace above the contract cap.
    pub fn slot(target: &'a str) -> Result<Self, Error> {
        Self::checked(target, "", 1)
    }
    /// Replace the submitter's top instance with this full screen id.
    ///
    /// # Errors
    /// Invalid for malformed ids; NoSpace above the contract cap.
    pub fn replace(target: &'a str) -> Result<Self, Error> {
        Self::checked(target, "", 2)
    }
    /// Open a screen as the contents of a full slot id.
    ///
    /// # Errors
    /// Invalid for malformed ids; NoSpace above the contract caps.
    pub fn in_slot(target: &'a str, slot: &'a str) -> Result<Self, Error> {
        Self::checked(target, slot, 3)
    }
    /// Attach a handover decoded from the current callback's host.
    #[must_use]
    pub const fn handover(mut self, handover: HandoverId) -> Self {
        self.handover = Some(handover);
        self
    }
    pub(crate) fn command(self) -> Result<Command<'a>, Error> {
        full_id(self.target)?;
        if self.mode == 3 {
            full_id(self.slot)?;
        }
        Ok(Command::Open {
            target: self.target,
            slot: self.slot,
            mode: self.mode,
            handover: self.handover.map_or(0, HandoverId::get),
        })
    }
}

pub(crate) fn full_id(id: &str) -> Result<(), Error> {
    if id.len() > 128 {
        return Err(Error::NoSpace);
    }
    if !id.contains('.')
        || id.split('.').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// How the row of button hints labels the button bound to one action while the
/// plugin's own top screen shows (`command.set_hint`, `docs/abi/ui.md`). It
/// goes with the screen instance and ends when that closes; a second hint for
/// an action replaces the first; an instance keeps at most eight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonHint<'a> {
    action: &'a str,
    kind: u32,
    label: &'a str,
}
impl<'a> ButtonHint<'a> {
    /// The action's usual name again: removes an earlier hint for `action`
    /// (a full action id, as key maps name it: `core.back`, the plugin's own).
    ///
    /// # Errors
    /// Invalid for an id that is not a full id, NoSpace above 128 bytes.
    pub fn usual(action: &'a str) -> Result<Self, Error> {
        Self::checked(action, 0, "")
    }
    /// The plugin's own words, already translated: not empty, at most 64 bytes.
    /// The host draws them by its own rules (smaller, two lines, then shortened).
    ///
    /// # Errors
    /// As [`Self::usual`]; Invalid for empty words, NoSpace above 64 bytes.
    pub fn words(action: &'a str, words: &'a str) -> Result<Self, Error> {
        Self::labelled(action, 1, words)
    }
    /// An icon by its name (an `icon.spec` name or a host icon); the host
    /// answers NotFound for a name it does not know.
    ///
    /// # Errors
    /// As [`Self::usual`]; Invalid for an empty name, NoSpace above 64 bytes.
    pub fn icon(action: &'a str, icon: &'a str) -> Result<Self, Error> {
        Self::labelled(action, 2, icon)
    }
    /// No hint: the button does nothing here. The host answers Denied for the
    /// user's ways out (`core.back`, the pinned actions, the Power key's escape).
    ///
    /// # Errors
    /// As [`Self::usual`].
    pub fn hidden(action: &'a str) -> Result<Self, Error> {
        Self::checked(action, 3, "")
    }
    fn labelled(action: &'a str, kind: u32, label: &'a str) -> Result<Self, Error> {
        if label.is_empty() {
            return Err(Error::Invalid);
        }
        if label.len() > crate::native::wire::commands_wire::COMMAND_SET_HINT_LABEL_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        Self::checked(action, kind, label)
    }
    fn checked(action: &'a str, kind: u32, label: &'a str) -> Result<Self, Error> {
        full_id(action)?;
        Ok(Self {
            action,
            kind,
            label,
        })
    }
    pub(crate) const fn command(self) -> Command<'a> {
        Command::SetHint {
            action: self.action,
            kind: self.kind,
            label: self.label,
        }
    }
}

/// One formatted display message, checked without truncation.
#[derive(Clone, Copy, Debug)]
pub struct Notify<'a> {
    text: &'a str,
    flags: u32,
}
impl<'a> Notify<'a> {
    /// A message shown while the plugin has a screen open.
    ///
    /// # Errors
    /// NoSpace above the contract's display-text cap.
    pub fn new(text: &'a str) -> Result<Self, Error> {
        if text.len() > crate::native::wire::commands_wire::COMMAND_NOTIFY_TEXT_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        Ok(Self { text, flags: 0 })
    }
    /// Also show reader-requested or device messages from background work.
    #[must_use]
    pub const fn announce(mut self) -> Self {
        self.flags |= 1;
        self
    }
    pub(crate) const fn command(self) -> Command<'a> {
        Command::Notify {
            text: self.text,
            flags: self.flags,
        }
    }
}

/// Runtime policies for an own screen instance. A fresh policy clears all
/// four runtime flags (guards, secret, held edges, grays) and keeps the status
/// bar and the row of hints as declared; declaration-only overlay/readiness bits are excluded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenPolicy {
    flags: u32,
}
impl ScreenPolicy {
    /// Clear guarding, secret content and held edge gestures.
    #[must_use]
    pub const fn new() -> Self {
        Self { flags: 0 }
    }
    /// Guard the device; the host permits enabling only on the own top screen.
    #[must_use]
    pub const fn guards(mut self) -> Self {
        self.flags |= 2;
        self
    }
    /// Mark the screen's content secret.
    #[must_use]
    pub const fn secret(mut self) -> Self {
        self.flags |= 16;
        self
    }
    /// Hold edge gestures.
    #[must_use]
    pub const fn holds_edges(mut self) -> Self {
        self.flags |= 32;
        self
    }
    /// The content is best in grays (see
    /// [`ScreenDecl::grays`](crate::native::declare::ScreenDecl::grays)).
    #[must_use]
    pub const fn grays(mut self) -> Self {
        self.flags |= 512;
        self
    }
    /// Whether the host's status bar shows above the screen (see
    /// [`ScreenDecl::status_bar`](crate::native::declare::ScreenDecl::status_bar));
    /// [`StatusBar::Default`](crate::native::declare::StatusBar::Default)
    /// keeps what the screen declared.
    #[must_use]
    pub const fn status_bar(mut self, bar: crate::native::declare::StatusBar) -> Self {
        self.flags = (self.flags & !(3 << 11)) | bar.bits();
        self
    }
    /// When the row of button hints shows with the screen (see
    /// [`ScreenDecl::hints`](crate::native::declare::ScreenDecl::hints));
    /// not said: as the screen declared. Draft 38.
    #[must_use]
    pub const fn hints(mut self, hints: crate::native::declare::HintRow) -> Self {
        self.flags = (self.flags & !(3 << 13)) | hints.bits();
        self
    }
    pub(crate) const fn flags(self) -> u32 {
        self.flags
    }
}

/// A speech request with its caller-chosen completion cookie.
#[derive(Clone, Copy, Debug)]
pub struct SpeechRequest<'a> {
    cookie: u32,
    text: &'a str,
    language: &'a str,
    rate: u32,
    flags: u32,
}
impl<'a> SpeechRequest<'a> {
    /// Replace the caller's current speech with complete display text.
    ///
    /// # Errors
    /// NoSpace above the generated speech-text cap.
    pub fn new(cookie: u32, text: &'a str) -> Result<Self, Error> {
        if text.len() > crate::native::wire::commands_wire::COMMAND_SPEAK_TEXT_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        Ok(Self {
            cookie,
            text,
            language: "",
            rate: 100,
            flags: 0,
        })
    }
    /// Select a BCP47 language tag, or empty for the system language.
    ///
    /// # Errors
    /// Invalid for tag syntax; NoSpace above the generated language cap.
    pub fn language(mut self, language: &'a str) -> Result<Self, Error> {
        if language.len() > crate::native::wire::commands_wire::COMMAND_SPEAK_LANGUAGE_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        if !language.is_empty()
            && language
                .split('-')
                .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return Err(Error::Invalid);
        }
        self.language = language;
        Ok(self)
    }
    /// Speaking rate, in percent.
    ///
    /// # Errors
    /// Invalid outside the contract's 50..=300 range.
    pub fn rate(mut self, percent: u16) -> Result<Self, Error> {
        if !(50..=300).contains(&percent) {
            return Err(Error::Invalid);
        }
        self.rate = u32::from(percent);
        Ok(self)
    }
    /// Queue after the caller's current speech instead of replacing it.
    #[must_use]
    pub const fn queue(mut self) -> Self {
        self.flags |= 1;
        self
    }
    pub(crate) const fn command(self) -> Command<'a> {
        Command::Speak {
            cookie: self.cookie,
            text: self.text,
            language: self.language,
            rate: self.rate,
            flags: self.flags,
        }
    }
}
