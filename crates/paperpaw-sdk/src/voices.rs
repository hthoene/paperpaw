//! Voices: what speaks text aloud ([`Command::Speak`](crate::Command::Speak)) besides the
//! system's.
//!
//! A plugin's voice ([`VoiceSpec`], `Items::add_voice`) gets each
//! text to speak while the setting
//! [`VOICE`](crate::builtin::settings::VOICE) chooses it and it knows the
//! text's language; the system's voice speaks the rest. It answers as the
//! system does: once a text is spoken (or could not be), it emits the
//! text's reply ([`Speech::answer`]). Stopping
//! ([`Command::StopSpeaking`](crate::Command::StopSpeaking), or the screen that spoke closing) calls
//! its `stop`.
//!
//! A device has speech ([`Capabilities::speech`](crate::Capabilities))
//! where its system speaks or a plugin brings a voice.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::{Env, Id, Speech};

/// How a voice speaks a text: the commands that start it.
pub type SpeakFn = Box<dyn Fn(&Env<'_>, &Speech, &mut crate::Commands)>;
/// How a voice stops.
pub type StopFn = Box<dyn Fn(&Env<'_>, &mut crate::Commands)>;

/// A voice contribution (see the [module documentation](self)): its name
/// in the setting is the translation of its id.
#[non_exhaustive]
pub struct VoiceSpec {
    pub id: Id,
    /// The languages it speaks (`de`, `en` …); any where empty.
    pub languages: Vec<Cow<'static, str>>,
    pub speak: SpeakFn,
    pub stop: StopFn,
}

impl fmt::Debug for VoiceSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceSpec")
            .field("id", &self.id)
            .field("languages", &self.languages)
            .finish_non_exhaustive()
    }
}

impl VoiceSpec {
    /// Voice `id`, speaking with `speak` and stopping with `stop`, in any
    /// language.
    pub fn new(
        id: Id,
        speak: impl Fn(&Env<'_>, &Speech, &mut crate::Commands) + 'static,
        stop: impl Fn(&Env<'_>, &mut crate::Commands) + 'static,
    ) -> Self {
        Self {
            id,
            languages: Vec::new(),
            speak: Box::new(speak),
            stop: Box::new(stop),
        }
    }

    /// Speaks these languages only.
    pub fn languages(mut self, languages: &[&'static str]) -> Self {
        self.languages = languages.iter().map(|l| Cow::Borrowed(*l)).collect();
        self
    }

    /// Whether it speaks `language` (`de-AT` is German; an unknown
    /// language, empty, any voice speaks).
    pub fn speaks(&self, language: &str) -> bool {
        let base = language.split(['-', '_']).next().unwrap_or(language);
        language.is_empty()
            || self.languages.is_empty()
            || self
                .languages
                .iter()
                .any(|l| l.eq_ignore_ascii_case(language) || l.eq_ignore_ascii_case(base))
    }
}

/// The registered voices, in the order of the plugins.
#[derive(Debug, Default)]
pub struct Voices {
    list: Vec<VoiceSpec>,
}

impl Voices {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, voice: VoiceSpec) {
        self.list.push(voice);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.list.shrink_to_fit();
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&VoiceSpec> {
        self.list.iter().find(|v| v.id.as_str() == id)
    }

    /// The voices' ids, in the order of the plugins.
    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.list.iter().map(|v| &v.id)
    }

    /// The plugin's voice that speaks `language` now: the one
    /// [`VOICE`](crate::builtin::settings::VOICE) chooses if it speaks it;
    /// where the system cannot speak (`system` false), else the first that
    /// does. `None`: the system's.
    pub fn chosen(&self, env: &Env<'_>, language: &str, system: bool) -> Option<&VoiceSpec> {
        let chosen = env.settings().read(&crate::builtin::settings::VOICE);
        let fitting = |v: &&VoiceSpec| v.speaks(language);
        self.get(chosen)
            .filter(fitting)
            .or_else(|| (!system).then(|| self.list.iter().find(fitting)).flatten())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_voice_speaks_its_languages_and_their_regions() {
        let voice =
            VoiceSpec::new(Id::new_static("v.de"), |_, _, _| {}, |_, _| {}).languages(&["de"]);
        assert!(voice.speaks("de-AT"));
        assert!(voice.speaks(""));
        assert!(!voice.speaks("en"));
        let any = VoiceSpec::new(Id::new_static("v.any"), |_, _, _| {}, |_, _| {});
        assert!(any.speaks("fr"));
    }
}
