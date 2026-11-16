//! A plugin's voice: chosen with `core.voice`, it speaks the texts of its
//! languages (the system's voice the rest), answers as the system would,
//! is stopped when asked or when the screen that spoke closes, and gives a
//! device without a voice of its own speech.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{DEVICE, Harness, REPLY};
use paperpaw_core::Items;
use paperpaw_core::sdk::voices::VoiceSpec;
use paperpaw_core::sdk::{Command, Id, Manifest, Params, RegisterError, Speech};
use paperpaw_core::{Io, Kernel, SpeechCall};
use paperpaw_platform::Instant;

/// What the voice was given: `say <text>` and `stop`.
type Heard = Rc<RefCell<Vec<String>>>;

/// A voice for English that answers at once.
fn echo(kept: Heard) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let (said, stopped) = (kept.clone(), kept.clone());
        r.add_voice(
            VoiceSpec::new(
                Id::new_static("echo.voice"),
                move |_, speech, out| {
                    said.borrow_mut().push(format!("say {}", speech.text()));
                    out.push(speech.answer(true));
                },
                move |_, _| stopped.borrow_mut().push(String::from("stop")),
            )
            .languages(&["en"]),
        )
    }
}

/// What [`echo`] registers under.
fn echo_manifest() -> Manifest {
    common::manifest("echo")
}

/// A voice for German, telling `heard` too.
fn deutsch(kept: Heard) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let (said, stopped) = (kept.clone(), kept.clone());
        r.add_voice(
            VoiceSpec::new(
                Id::new_static("deutsch.voice"),
                move |_, speech, _| said.borrow_mut().push(format!("sage {}", speech.text())),
                move |_, _| stopped.borrow_mut().push(String::from("halt")),
            )
            .languages(&["de"]),
        )
    }
}

/// What [`deutsch`] registers under.
fn deutsch_manifest() -> Manifest {
    common::manifest("deutsch")
}

fn kernel(harness: &Harness, heard: &Heard, config: &str, system: bool) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .items(echo_manifest(), echo(heard.clone()))
        .items(deutsch_manifest(), deutsch(heard.clone()))
        .speech(system)
        .user_config(config.to_string())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    kernel
}

fn say(text: &str, language: &str) -> Command {
    let speech = Speech::new(text, REPLY)
        .in_language(language)
        .with(Params::new().with("n", String::from(text)));
    Command::speak(speech)
}

fn system_said(kernel: &mut Kernel) -> Vec<String> {
    kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Speech(SpeechCall::Say { text, .. }) => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn the_voice_chosen_speaks_its_languages() {
    let (harness, heard) = (Harness::new(), Heard::default());
    let mut kernel = kernel(&harness, &heard, "[core]\nvoice = echo.voice\n", true);
    harness.run(
        &mut kernel,
        10,
        [say("Hello", "en-GB"), say("Bonjour", "fr")],
    );
    // The French in its place: the voice stopped.
    assert_eq!(*heard.borrow(), ["say Hello", "stop"]);
    // The system's voice the rest.
    assert_eq!(system_said(&mut kernel), ["Bonjour"]);
    // The voice answered as the system does.
    let replies = harness.lines("event drv.reply");
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(replies[0].contains("n=Hello") && !replies[0].contains("failed"));
    // Stopped when asked.
    harness.run(&mut kernel, 20, [Command::StopSpeaking]);
    assert_eq!(heard.borrow().last().map(String::as_str), Some("stop"));
}

#[test]
fn the_systems_voice_speaks_until_another_is_chosen() {
    let (harness, heard) = (Harness::new(), Heard::default());
    let mut kernel = kernel(&harness, &heard, "", true);
    harness.run(&mut kernel, 10, [say("Hello", "en")]);
    assert!(heard.borrow().is_empty());
    assert_eq!(system_said(&mut kernel), ["Hello"]);
}

#[test]
fn a_plugins_voice_gives_a_device_speech() {
    let (harness, heard) = (Harness::new(), Heard::default());
    let mut kernel = kernel(&harness, &heard, "[core]\nvoice = echo.voice\n", false);
    harness.run(&mut kernel, 10, [say("Hello", "en"), say("Hallo", "fr")]);
    assert_eq!(*heard.borrow(), ["say Hello", "stop"]);
    // French has no voice here: it fails at once, as without speech.
    let replies = harness.lines("event drv.reply");
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(
        replies
            .iter()
            .any(|r| r.contains("n=Hallo") && r.contains("failed"))
    );
    assert!(system_said(&mut kernel).is_empty());
}

#[test]
fn without_the_systems_voice_a_plugins_speaks_unchosen() {
    let (harness, heard) = (Harness::new(), Heard::default());
    let mut kernel = kernel(&harness, &heard, "", false);
    harness.run(&mut kernel, 10, [say("Hello", "en")]);
    assert_eq!(*heard.borrow(), ["say Hello"]);
}

#[test]
fn the_systems_voice_speaking_in_its_place_stops_the_plugins() {
    let (harness, heard) = (Harness::new(), Heard::default());
    let mut kernel = kernel(&harness, &heard, "[core]\nvoice = echo.voice\n", true);
    harness.run(&mut kernel, 10, [say("Hello", "en")]);
    // French is the system's: the voice stops, not both talking.
    harness.run(&mut kernel, 20, [say("Bonjour", "fr")]);
    assert_eq!(*heard.borrow(), ["say Hello", "stop"]);
    assert_eq!(system_said(&mut kernel), ["Bonjour"]);
}

#[test]
fn one_voice_speaking_in_place_of_another_stops_it() {
    let (harness, heard) = (Harness::new(), Heard::default());
    // No system voice: each language its plugin's.
    let mut kernel = kernel(&harness, &heard, "", false);
    harness.run(&mut kernel, 10, [say("Hello", "en")]);
    harness.run(&mut kernel, 20, [say("Hallo", "de")]);
    assert_eq!(*heard.borrow(), ["say Hello", "stop", "sage Hallo"]);
    // Stopping reaches the one speaking now.
    harness.run(&mut kernel, 30, [Command::StopSpeaking]);
    assert_eq!(heard.borrow().last().map(String::as_str), Some("halt"));
}
