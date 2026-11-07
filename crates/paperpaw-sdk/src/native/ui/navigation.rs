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
    /// Open a screen by full id, including another plugin's screen.
    #[must_use]
    pub const fn screen(target: &'a str) -> Self {
        Self {
            target,
            slot: "",
            mode: 0,
            handover: None,
        }
    }
    /// Show the screen bound to this full slot id.
    #[must_use]
    pub const fn slot(target: &'a str) -> Self {
        Self {
            mode: 1,
            ..Self::screen(target)
        }
    }
    /// Replace the submitting plugin's own top instance.
    #[must_use]
    pub const fn replace(target: &'a str) -> Self {
        Self {
            mode: 2,
            ..Self::screen(target)
        }
    }
    /// Open a screen as the contents of a slot.
    #[must_use]
    pub const fn in_slot(target: &'a str, slot: &'a str) -> Self {
        Self {
            slot,
            mode: 3,
            ..Self::screen(target)
        }
    }
    /// Attach a handover created during this callback.
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
