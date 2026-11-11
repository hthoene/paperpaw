//! Read-aloud for native plugins: `command.speak`, `command.stop_speaking` and
//! `event.spoken` (89, `docs/abi/ui.md` section 7, "Speech").
//!
//! The kernel's speech (`io::speech`) is interface 6's and the shells' one
//! backend: a native `command.speak` becomes the same [`Command::Speak`] with
//! the host's own reply event ([`SPOKEN`]), which carries the submitter and
//! the plugin's `cookie` in its parameters. When the text ends, or is replaced
//! or stopped, the kernel emits that reply with a `status` (0 spoken, 1
//! replaced or stopped, 2 no voice, 3 no speech on this device) and
//! [`Kernel::native_speech_event`] queues `event.spoken` for the submitter
//! alone. `command.stop_speaking` becomes a [`STOP`] request the same way: it
//! stops the submitter's own speech only.
//!
//! The only text copy beyond the plugin's own is the one the kernel keeps for
//! the shell (the sentence being sent); the bookkeeping per text is the
//! plugin's id and its cookie.
use super::registrations as wire;
use crate::Kernel;
use alloc::{boxed::Box, string::String};
use core::fmt::Write;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_sdk::{Command, Id, Params, Speech};

/// The reply event of a native plugin's speech (the kernel's, no plugin's).
pub(crate) const SPOKEN: &str = "core.native-spoken";
/// The request to stop the speech of one native plugin.
pub(crate) const STOP: &str = "core.native-stop-speaking";
/// The submitting plugin's id.
pub(crate) const OWNER: &str = "owner";
const COOKIE: &str = "cookie";
const STATUS: &str = "status";

/// `command.speak.rate`: 50 to 300 percent.
const RATE: core::ops::RangeInclusive<u32> = 50..=300;
/// `command.speak.flags` bit 0: after what is spoken, not in its place.
const QUEUE: u32 = 1;

/// Whether `event` is the reply event of a native plugin's speech.
pub(crate) fn is_reply(event: &Id) -> bool {
    event.as_str() == SPOKEN
}

/// `params` with the way the text ended (`event.spoken.status`).
pub(crate) fn with_status(params: Params, status: u32) -> Params {
    let mut text = String::new();
    let _ = write!(text, "{status}");
    params.with(STATUS, text)
}

/// What a `command.speak` asks, checked: a rate out of range or a flag that
/// is not defined is `ERR_INVALID`.
pub(super) fn check(rate: u32, flags: u32) -> Result<(), i32> {
    if !RATE.contains(&rate) || flags & !QUEUE != 0 {
        return Err(ERR_INVALID);
    }
    Ok(())
}

/// The kernel command for a native `command.speak` of `owner`, its texts
/// copied with the allocation checked.
pub(super) fn speak(
    owner: &Id,
    (cookie, text, language): (u32, &str, &str),
    (rate, flags): (u32, u32),
) -> Result<Command, i32> {
    check(rate, flags)?;
    let own = |text: &str| -> Result<String, i32> {
        let mut copy = String::new();
        copy.try_reserve_exact(text.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        copy.push_str(text);
        Ok(copy)
    };
    let mut number = String::new();
    let _ = write!(number, "{cookie}");
    let mut params = Params::new();
    params
        .try_set(OWNER, owner.as_str())
        .and_then(|()| params.try_set(COOKIE, &number))
        .map_err(|_| ERR_NO_MEMORY)?;
    let rate = u16::try_from(rate).map_err(|_| ERR_INVALID)?;
    let speech = Speech::new(own(text)?, Id::new_static(SPOKEN))
        .in_language(own(language)?)
        .at_rate(rate)
        .queued(flags & QUEUE != 0)
        .with(params);
    Ok(Command::Speak(Box::new(speech)))
}

/// The kernel command for a native `command.stop_speaking` of `owner`.
pub(super) fn stop(owner: &Id) -> Result<Command, i32> {
    let mut params = Params::new();
    params
        .try_set(OWNER, owner.as_str())
        .map_err(|_| ERR_NO_MEMORY)?;
    Ok(Command::Emit(Id::new_static(STOP), params))
}

impl Kernel {
    /// The kernel's own events of a native plugin's speech: the end of a text
    /// is queued as `event.spoken` for its submitter alone, and a stop request
    /// stops the submitter's speech. `true` for those two events (nothing else
    /// hears them), `false` for any other.
    pub(crate) fn native_speech_event(&mut self, event: &Id, params: &Params) -> bool {
        let owner = params.get(OWNER).unwrap_or("");
        match event.as_str() {
            STOP => {
                self.io.speech.stop_owned(owner, &mut self.output.commands);
            }
            SPOKEN => {
                let number = |key| params.get(key).and_then(|text| text.parse::<u32>().ok());
                let (Some(cookie), Some(status)) = (number(COOKIE), number(STATUS)) else {
                    return true;
                };
                let now_ms = self.now.millis();
                self.native_queue_event(wire::EVENT_SPOKEN_TAG, &|plugin| plugin == owner, &|queue| {
                    let record = wire::Record::EventSpoken(wire::EventSpoken {
                        now_ms,
                        cookie,
                        status,
                    });
                    queue.push_record(wire::EVENT_SPOKEN_TAG, &record)
                });
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
#[path = "speech_tests.rs"]
mod tests;
