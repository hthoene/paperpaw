//! Read-aloud for native plugins: `command.speak`, `command.stop_speaking` and
//! `event.spoken` (89, `docs/abi/ui.md` section 7, "Speech").
//!
//! The kernel's speech (`io::speech`) is interface 6's and the shells' one
//! backend: a native `command.speak` becomes the same [`Command::Speak`] with
//! the host's own reply id ([`SPOKEN`]), which carries the submitter and the
//! plugin's `cookie` in its parameters. When the text ends, or is replaced or
//! stopped, `io::speech` notes how (`status`: 0 spoken, 1 replaced or stopped,
//! 2 no voice, 3 no speech on this device) in a list only the kernel holds, and
//! [`Kernel::native_speech_pump`] queues `event.spoken` from it for the
//! submitter alone. `command.stop_speaking` becomes a [`Command::Speak`] with
//! the reply id [`STOP`] the kernel's apply recognises: it stops the
//! submitter's own speech only. Neither id goes over the event bus, and an
//! interface-6 plugin's command with either is refused (`vet_reserved`), so
//! no plugin can forge an ending or stop another's speech.
//!
//! The only text copy beyond the plugin's own is the one the kernel keeps for
//! the shell (the sentence being sent); the bookkeeping per text is the
//! plugin's id and its cookie.
use super::registrations as wire;
use crate::Kernel;
use alloc::{boxed::Box, string::String};
use core::fmt::Write;
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_sdk::{Command, Id, Params, Speech};

/// The reply id of a native plugin's speech (the kernel's, no plugin's).
pub(crate) const SPOKEN: &str = "core.native-spoken";
/// The reply id of the request to stop the speech of one native plugin.
pub(crate) const STOP: &str = "core.native-stop-speaking";
/// The submitting plugin's id.
pub(crate) const OWNER: &str = "owner";
const COOKIE: &str = "cookie";
const STATUS: &str = "status";

/// `command.speak.rate`: 50 to 300 percent.
const RATE: core::ops::RangeInclusive<u32> = 50..=300;
/// `command.speak.flags` bit 0: after what is spoken, not in its place.
pub(crate) const QUEUE: u32 = 1;

/// The most texts of one native plugin, the one being spoken included.
pub(crate) const MAX_TEXTS: usize = 4;

/// What a native speech command does to the count of a plugin's texts:
/// `Some(queued)` for a speak, `None` for a stop.
pub(crate) fn count(texts: &mut usize, speak: Option<bool>) -> Result<(), i32> {
    match speak {
        // After what is spoken: beyond the cap nothing is queued.
        Some(true) if *texts >= MAX_TEXTS => return Err(ERR_FULL),
        Some(true) => *texts += 1,
        // In place of everything: the one text.
        Some(false) => *texts = 1,
        // The plugin's own texts end.
        None => *texts = 0,
    }
    Ok(())
}

/// Whether `event` is the reply event of a native plugin's speech.
pub(crate) fn is_reply(event: &Id) -> bool {
    event.as_str() == SPOKEN
}

/// Whether `event` is the reply id of a native plugin's stop request.
pub(crate) fn is_stop(event: &Id) -> bool {
    event.as_str() == STOP
}

/// `params` with the way the text ended (`event.spoken.status`).
pub(crate) fn with_status(params: Params, status: u32) -> Params {
    let mut text = String::new();
    let _ = write!(text, "{status}");
    params.with(STATUS, text)
}

/// What a `command.speak` asks, checked: a rate out of range or a flag that
/// is not defined is `ERR_INVALID`.
pub(crate) fn check(rate: u32, flags: u32) -> Result<(), i32> {
    if !RATE.contains(&rate) || flags & !QUEUE != 0 {
        return Err(ERR_INVALID);
    }
    Ok(())
}

/// The kernel command for a native `command.speak` of `owner`, its texts
/// copied with the allocation checked.
pub(crate) fn speak(
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
pub(crate) fn stop(owner: &Id) -> Result<Command, i32> {
    let mut params = Params::new();
    params
        .try_set(OWNER, owner.as_str())
        .map_err(|_| ERR_NO_MEMORY)?;
    let request = Speech::new(String::new(), Id::new_static(STOP)).with(params);
    Ok(Command::Speak(Box::new(request)))
}

impl Kernel {
    /// Queues `event.spoken` for the submitters of the texts that ended since
    /// the last pass (to each alone), from the kernel's own list. Returns
    /// `false` always: the event is queued here, not answered at once, and the
    /// event pump after this tells the queues.
    pub(crate) fn native_speech_pump(&mut self) -> bool {
        let ended = self.io.speech.take_ended();
        let now_ms = self.now.millis();
        for params in &ended {
            let number = |key| params.get(key).and_then(|text| text.parse::<u32>().ok());
            let (Some(owner), Some(cookie), Some(status)) =
                (params.get(OWNER), number(COOKIE), number(STATUS))
            else {
                continue;
            };
            self.native_queue_event(
                wire::EVENT_SPOKEN_TAG,
                &|plugin| plugin == owner,
                &|queue| {
                    let record = wire::Record::EventSpoken(wire::EventSpoken {
                        now_ms,
                        cookie,
                        status,
                    });
                    queue.push_record(wire::EVENT_SPOKEN_TAG, &record)
                },
            );
        }
        false
    }
}

#[cfg(test)]
#[path = "speech_tests.rs"]
mod tests;
