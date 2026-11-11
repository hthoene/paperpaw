//! Text spoken aloud for plugins ([`Io::Speech`], each text's end
//! reported with `Kernel::speech_done`).
//!
//! [`Io::Speech`]: super::Io::Speech

use alloc::string::String;
use alloc::vec::Vec;

use crate::native_host::speech as native;
use paperpaw_sdk::{Command, Id, Params};

/// What a shell does for speech.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechCall {
    /// Speak `text` instead of anything spoken now, or after it with
    /// `queue`, and report with `Kernel::speech_done` and `id` when it is
    /// done. Texts replaced are not reported.
    Say {
        id: u32,
        text: String,
        /// E.g. `de` or `en-GB`; empty for the system's language.
        language: String,
        /// In percent of the normal speed.
        rate: u16,
        queue: bool,
    },
    /// Stop speaking.
    Stop,
}

/// A text being spoken.
#[derive(Debug)]
struct Speaking {
    /// Its number for the shell.
    id: u32,
    reply: Id,
    params: Params,
    /// Screens open when it started: it stops once fewer are.
    screens: usize,
}

/// What is spoken, and what the shell is to do for it.
#[derive(Debug, Default)]
pub(crate) struct Speech {
    /// The shell speaks.
    pub system: bool,
    /// A plugin's voice speaking, and the screens open when it started.
    pub voice: Option<(Id, usize)>,
    calls: Vec<SpeechCall>,
    speaking: Vec<Speaking>,
    next: u32,
    /// How native plugins' texts ended, for `native_speech_pump`: the
    /// plugin's own parameters (its id and cookie) and the status. Held here,
    /// never on the event bus, so that no plugin can forge one.
    ended: Vec<Params>,
    /// What was said last of the focus (`core_settings::SPEAK_FOCUS`).
    pub focus: Option<String>,
}

impl Speech {
    /// Speech where the shell speaks (`system`).
    pub fn new(system: bool) -> Self {
        Self {
            system,
            ..Self::default()
        }
    }

    /// Starts speaking for a plugin, or tells it that the device cannot.
    pub fn speak(
        &mut self,
        speech: paperpaw_sdk::Speech,
        available: bool,
        screens: usize,
        out: &mut Vec<Command>,
    ) {
        if !available {
            // Native plugins hear status 3 (no speech on this device) at once.
            if native::is_reply(speech.reply()) {
                self.ended.push(native::with_status(
                    speech.params().clone(),
                    Ended::NoSpeech as u32,
                ));
                return;
            }
            return reply(speech.reply().clone(), speech.params().clone(), false, out);
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        let queue = speech.is_queued();
        if !queue {
            self.drop_all();
        }
        self.speaking.push(Speaking {
            id,
            reply: speech.reply().clone(),
            params: speech.params().clone(),
            screens,
        });
        let (language, rate) = (String::from(speech.language()), speech.rate());
        self.calls.push(SpeechCall::Say {
            id,
            text: speech.into_text(),
            language,
            rate,
            queue,
        });
    }

    /// Forgets every text being spoken; those of native plugins are told
    /// they were replaced or stopped (`event.spoken` status 1), the
    /// interface-6 replies stay unsent (texts replaced are not reported).
    fn drop_all(&mut self) {
        for speaking in self.speaking.drain(..) {
            if native::is_reply(&speaking.reply) {
                self.ended
                    .push(native::with_status(speaking.params, Ended::Stopped as u32));
            }
        }
    }

    /// Stops speaking, if anything is spoken.
    pub fn stop(&mut self) {
        if !self.speaking.is_empty() {
            self.drop_all();
            self.calls.push(SpeechCall::Stop);
        }
    }

    /// Stops what the native plugin `owner` speaks (`command.stop_speaking`
    /// stops the submitter's own speech only). The shell has one voice: where
    /// the text spoken now is the owner's, it stops, and what waited behind it
    /// stops with it.
    pub fn stop_owned(&mut self, owner: &str) {
        let mine =
            |s: &Speaking| native::is_reply(&s.reply) && s.params.get(native::OWNER) == Some(owner);
        if self.speaking.first().is_some_and(mine) {
            self.stop();
            return;
        }
        // Behind another's text: the owner's waiting texts are dropped from
        // the line (the shell speaks them still; they end unreported).
        let mut at = 0;
        while at < self.speaking.len() {
            if mine(&self.speaking[at]) {
                let gone = self.speaking.remove(at);
                self.ended
                    .push(native::with_status(gone.params, Ended::Stopped as u32));
            } else {
                at += 1;
            }
        }
    }

    /// Stops speaking what a screen closed since started.
    pub fn stop_for_closed_screens(&mut self, screens: usize) {
        if self.speaking.iter().any(|s| s.screens > screens) {
            self.stop();
        }
    }

    /// Text `id` was spoken (`spoken`), or could not be: its plugin hears.
    pub fn done(&mut self, id: u32, spoken: bool, out: &mut Vec<Command>) {
        if let Some(at) = self.speaking.iter().position(|s| s.id == id) {
            let speaking = self.speaking.remove(at);
            if native::is_reply(&speaking.reply) {
                let ended = if spoken {
                    Ended::Spoken
                } else {
                    Ended::NoVoice
                };
                self.ended
                    .push(native::with_status(speaking.params, ended as u32));
            } else {
                reply(speaking.reply, speaking.params, spoken, out);
            }
        }
    }

    /// The native endings since the last call (`ui.md` section 7, `spoken`).
    pub fn take_ended(&mut self) -> Vec<Params> {
        core::mem::take(&mut self.ended)
    }

    /// What the shell is to do.
    pub fn take(&mut self) -> Vec<SpeechCall> {
        core::mem::take(&mut self.calls)
    }

    /// How many texts of the native plugin `owner` are being spoken or wait
    /// (`ui.md` section 7 caps them per plugin).
    pub fn owned_texts(&self, owner: &str) -> usize {
        self.speaking
            .iter()
            .filter(|s| native::is_reply(&s.reply) && s.params.get(native::OWNER) == Some(owner))
            .count()
    }

    /// Whether a text is being spoken.
    pub fn speaking(&self) -> bool {
        !self.speaking.is_empty()
    }
}

/// How a text ended (the `status` of `event.spoken`, `ui.md` section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Spoken = 0,
    Stopped = 1,
    NoVoice = 2,
    NoSpeech = 3,
}

/// The reply to an interface-6 [`Command::Speak`]: spoken or not, with the
/// parameters the plugin gave (a replaced text is not reported there).
fn reply(reply: Id, params: Params, spoken: bool, out: &mut Vec<Command>) {
    use paperpaw_sdk::event::EventData;
    let spoken = paperpaw_sdk::replies::Spoken::new(spoken, params);
    out.push(Command::Emit(reply, spoken.into_params()));
}
