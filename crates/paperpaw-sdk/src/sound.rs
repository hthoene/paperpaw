//! Sound played (reserved: no shell plays any yet). A plugin asks with
//! `Ask::PlaySound` and hears
//! [`Played`](crate::replies::Played) as the sound's reply. Where the device has no sound
//! ([`Capability::AUDIO`](crate::Capability::AUDIO) missing, which is
//! everywhere today) the reply comes at once, not played: a plugin built
//! now plays where a later shell can, without a new plugin interface.

use alloc::string::String;

use crate::event::EventData;
use crate::id::Id;
use crate::params::Params;

/// Parameters of the reply to `Ask::PlaySound`.
pub mod params {
    /// Present (`1`) when the sound could not be played, e.g. on a device
    /// without sound.
    pub const FAILED: &str = "failed";
}

/// A sound to play: a file on the card (see
/// `Ask::PlaySound`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Sound {
    /// The file, e.g. `plugins/words/audio/haus.ogg`.
    path: String,
    /// How loud, in percent of the device's volume.
    volume: u8,
    /// Emitted once it has played, or could not: a
    /// [`Played`](crate::replies::Played) with `params`.
    reply: Id,
    params: Params,
}

impl Sound {
    /// The file at `path`, at the device's volume.
    pub fn new(path: impl Into<String>, reply: impl Into<Id>) -> Self {
        Self {
            path: path.into(),
            volume: 100,
            reply: reply.into(),
            params: Params::new(),
        }
    }

    /// At `volume` percent of the device's volume (at most 100).
    pub fn at_volume(mut self, volume: u8) -> Self {
        self.volume = volume.min(100);
        self
    }

    /// With `params` passed back in the reply.
    pub fn with(mut self, params: Params) -> Self {
        self.params = params;
        self
    }

    /// The file to play.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// How loud, in percent of the device's volume.
    pub fn volume(&self) -> u8 {
        self.volume
    }

    /// The event its reply comes as.
    pub fn reply(&self) -> &Id {
        &self.reply
    }

    /// The parameters passed back with its reply.
    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Its reply, the event and its params: it was `played`, or could
    /// not be.
    pub fn answer(&self, played: bool) -> (Id, Params) {
        let reply = crate::replies::Played::new(played, self.params.clone());
        (self.reply.clone(), reply.into_params())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]
mod tests {
    use super::*;
    use crate::replies::Played;

    #[test]
    fn a_reply_says_whether_it_played_and_brings_its_params_back() {
        let sound = Sound::new("a.ogg", Id::new_static("w.played"))
            .at_volume(250)
            .with(Params::new().with("word", "Haus"));
        assert_eq!((sound.path(), sound.volume()), ("a.ogg", 100));
        let (event, params) = sound.answer(false);
        assert_eq!(event, *sound.reply());
        let played = Played::from_params(&params).unwrap();
        assert!(!played.played);
        assert_eq!(played.context.get("word"), Some("Haus"));
        let again = Played::from_params(&Played::new(true, played.context).to_params()).unwrap();
        assert!(again.played);
    }
}
