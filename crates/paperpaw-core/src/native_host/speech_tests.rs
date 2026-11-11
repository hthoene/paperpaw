//! Read-aloud for native plugins over the kernel's own speech: the end of a
//! text reaches its submitter alone as `event.spoken`, a replaced or stopped
//! text says status 1, a device without speech says status 3 at once, and a
//! stop is the submitter's own.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::speak;
use crate::io::speech::SpeechCall;
use crate::native_host::{
    codec,
    commands::CommandStage,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
};
use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, OK};
use paperpaw_sdk::{Command, Id, Instant, Params, Speech};

/// What one plugin was told: `cookie`, `status`.
type Told = Rc<RefCell<Vec<(u32, u32)>>>;

struct Probe {
    told: Told,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        tag == wire::EVENT_SPOKEN_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let wire::Record::EventSpoken(event) = codec::decode_any_single(bytes).unwrap() else {
            return Err(ERR_INVALID);
        };
        self.told.borrow_mut().push((event.cookie, event.status));
        Ok(OK)
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        _: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.service_event(bytes, env)
    }
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

struct World {
    kernel: crate::Kernel,
    a: (Id, Told),
    b: (Id, Told),
}

/// Two plugins `a` and `b` that subscribed to `event.spoken`.
fn world(speech: bool) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .speech(speech)
        .build()
        .unwrap();
    let mut add = |name: &'static str| {
        let told = Told::default();
        kernel
            .shared
            .native_instances
            .test_insert(
                Id::try_parse(name).unwrap(),
                Some(NativeLease::try_new(Probe { told: told.clone() }).unwrap()),
                true,
            )
            .unwrap();
        (Id::try_parse(name).unwrap(), told)
    };
    let a = add("a");
    let b = add("b");
    World { kernel, a, b }
}

impl World {
    /// `command.speak` of `owner` as the stage turns it, applied and run.
    fn speak(&mut self, owner: &Id, cookie: u32, flags: u32) {
        let command = speak(owner, (cookie, "Hello", "de"), (100, flags)).unwrap();
        self.kernel.output.commands.push(command);
        self.kernel.run_commands();
    }

    fn stop(&mut self, owner: &Id) {
        let command = super::stop(owner).unwrap();
        self.kernel.output.commands.push(command);
        self.kernel.run_commands();
    }

    /// The shell's calls since the last look.
    fn calls(&mut self) -> Vec<SpeechCall> {
        self.kernel.io.speech.take()
    }

    /// One pass of the service pump, as the kernel runs it after a call.
    fn pump(&mut self) {
        self.kernel.native_service_pump();
    }
}

#[test]
fn a_spoken_text_tells_its_submitter_alone_with_its_cookie() {
    let mut w = world(true);
    let a = w.a.0.clone();
    w.speak(&a, 7, 0);
    let calls = w.calls();
    let [
        SpeechCall::Say {
            id,
            text,
            language,
            rate,
            queue,
        },
    ] = calls.as_slice()
    else {
        panic!("{calls:?}");
    };
    assert_eq!(
        (text.as_str(), language.as_str(), *rate, *queue),
        ("Hello", "de", 100, false)
    );
    w.pump();
    assert!(w.a.1.borrow().is_empty(), "not before the shell is done");
    w.kernel.speech_done(Instant(5), *id, true);
    w.pump();
    assert_eq!(*w.a.1.borrow(), [(7, 0)]);
    assert!(w.b.1.borrow().is_empty(), "another plugin hears nothing");
    // Told once.
    w.pump();
    assert_eq!(w.a.1.borrow().len(), 1);
}

#[test]
fn a_text_that_could_not_be_spoken_says_no_voice() {
    let mut w = world(true);
    let a = w.a.0.clone();
    w.speak(&a, 3, 0);
    let calls = w.calls();
    let [SpeechCall::Say { id, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    w.kernel.speech_done(Instant(5), *id, false);
    w.pump();
    assert_eq!(*w.a.1.borrow(), [(3, 2)]);
}

#[test]
fn a_replaced_text_says_replaced_and_a_queued_one_waits() {
    let mut w = world(true);
    let a = w.a.0.clone();
    w.speak(&a, 1, 0);
    w.speak(&a, 2, 1);
    w.pump();
    assert!(
        w.a.1.borrow().is_empty(),
        "queued after the first, nothing ended"
    );
    w.speak(&a, 3, 0);
    w.pump();
    // The two texts in the line were replaced.
    assert_eq!(*w.a.1.borrow(), [(1, 1), (2, 1)]);
}

#[test]
fn a_stop_ends_the_submitters_speech_and_nothing_follows() {
    let mut w = world(true);
    let a = w.a.0.clone();
    w.speak(&a, 9, 0);
    let calls = w.calls();
    let [SpeechCall::Say { id, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    w.stop(&a);
    assert_eq!(w.calls(), [SpeechCall::Stop]);
    w.pump();
    assert_eq!(*w.a.1.borrow(), [(9, 1)]);
    // The shell's late report of the stopped text tells nothing more.
    w.kernel.speech_done(Instant(6), *id, true);
    w.pump();
    assert_eq!(*w.a.1.borrow(), [(9, 1)]);
    assert!(!w.kernel.speaking());
}

#[test]
fn a_stop_of_another_plugin_leaves_the_speech_alone() {
    let mut w = world(true);
    let (a, b) = (w.a.0.clone(), w.b.0.clone());
    w.speak(&a, 5, 0);
    w.calls();
    w.stop(&b);
    assert!(w.calls().is_empty(), "the shell is not stopped");
    w.pump();
    assert!(w.a.1.borrow().is_empty() && w.b.1.borrow().is_empty());
    assert!(w.kernel.speaking());
}

#[test]
fn a_device_without_speech_says_status_3_at_once() {
    let mut w = world(false);
    let a = w.a.0.clone();
    w.speak(&a, 4, 0);
    assert!(w.calls().is_empty());
    w.pump();
    assert_eq!(*w.a.1.borrow(), [(4, 3)]);
    assert!(w.b.1.borrow().is_empty());
}

#[test]
fn a_rate_out_of_range_and_an_undefined_flag_are_refused_and_the_texts_are_copied() {
    let owner = Id::try_parse("a").unwrap();
    for (rate, flags) in [(49, 0), (301, 0), (100, 2), (100, 0x8000_0000)] {
        assert_eq!(
            speak(&owner, (1, "x", ""), (rate, flags)).unwrap_err(),
            ERR_INVALID
        );
    }
    let Command::Speak(speech) = speak(&owner, (1, "Hallo", "de-DE"), (50, 1)).unwrap() else {
        panic!("a speech");
    };
    assert_eq!(
        (speech.text(), speech.language(), speech.rate()),
        ("Hallo", "de-DE", 50)
    );
    assert!(speech.is_queued());
    speak(&owner, (1, "x", ""), (300, 0)).unwrap();
}

/// What an interface-6 plugin would send to pass for another plugin's speech.
fn forged(reply: &'static str) -> Command {
    let params = Params::new()
        .with(super::OWNER, "a")
        .with("cookie", "1")
        .with("status", "0");
    Command::Speak(alloc::boxed::Box::new(
        Speech::new("x", Id::new_static(reply)).with(params),
    ))
}

#[test]
fn an_interface_6_plugin_cannot_forge_an_ending_or_stop_anothers_speech() {
    let mut w = world(true);
    let a = w.a.0.clone();
    w.speak(&a, 5, 0);
    w.calls();
    // Its commands with the host's ids are refused by the vetting...
    let reserved =
        crate::commands::Reserved::of(&w.kernel.shared.settings, &w.kernel.shared.registry);
    let mut out = alloc::vec![forged(super::SPOKEN), forged(super::STOP)];
    assert!(crate::commands::vet_reserved(
        &mut out, 0, "evil", &reserved
    ));
    assert!(out.is_empty());
    // ... also those of a plugin exempt from the guarded-settings checks.
    let mut out = alloc::vec![forged(super::SPOKEN), forged(super::STOP)];
    assert!(crate::commands::vet_reserved(
        &mut out,
        0,
        crate::meter::CORE,
        &reserved
    ));
    assert!(out.is_empty());
    // ... and on the event bus they are no events of the speech at all.
    for id in [super::SPOKEN, super::STOP] {
        let params = Params::new()
            .with(super::OWNER, "a")
            .with("cookie", "5")
            .with("status", "0");
        w.kernel.emit(&Id::new_static(id), &params);
    }
    w.pump();
    assert!(w.a.1.borrow().is_empty(), "no forged event.spoken");
    assert!(w.calls().is_empty(), "no forged stop");
    assert!(w.kernel.speaking());
}

#[test]
fn a_stop_while_another_texts_is_first_drops_only_the_submitters_waiting_text() {
    let mut w = world(true);
    let (a, b) = (w.a.0.clone(), w.b.0.clone());
    w.speak(&a, 1, 0);
    w.speak(&b, 2, 1);
    w.calls();
    // `b`'s text waits behind `a`'s: its stop drops it from the line (status 1)
    // and does not stop the shell, which is speaking `a`'s text.
    w.stop(&b);
    assert!(w.calls().is_empty());
    w.pump();
    assert_eq!(*w.b.1.borrow(), [(2, 1)]);
    assert!(w.a.1.borrow().is_empty());
    // `a`'s stop, with its text first, stops the shell.
    w.stop(&a);
    assert_eq!(w.calls(), [SpeechCall::Stop]);
    w.pump();
    assert_eq!(*w.a.1.borrow(), [(1, 1)]);
}

/// A batch of `command.speak` (and stops) through a stage that is told the
/// plugin holds `held` texts.
fn stage_speech(held: usize, batches: &[&[(u32, bool)]]) -> Vec<i32> {
    use crate::native_host::commands::{WireCommand, commands_decode, commands_encode};
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .speech(true)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage =
        CommandStage::new(&owner, env.settings(), None, 16, 8192).with_ui(&env, None, 0, 100);
    stage.set_speech_held(held);
    let mut statuses = Vec::new();
    for batch in batches {
        // (flags, true) is a speak; (_, false) a stop.
        let commands: Vec<WireCommand<'_>> = batch
            .iter()
            .map(|&(flags, speak)| {
                if speak {
                    WireCommand::Speak {
                        cookie: 1,
                        text: "x",
                        language: "",
                        rate: 100,
                        flags,
                    }
                } else {
                    WireCommand::StopSpeaking {}
                }
            })
            .collect();
        let mut bytes = [0; 1024];
        let len = commands_encode(&commands, &mut bytes).unwrap();
        statuses.push(stage.stage(&commands_decode(&bytes[..len]).unwrap()));
    }
    statuses
}

#[test]
fn a_plugin_holds_at_most_four_texts_a_queued_fifth_is_err_full() {
    // One being spoken and two waiting: one more fits, the next does not.
    assert_eq!(
        stage_speech(3, &[&[(1, true)], &[(1, true)]]),
        [OK, ERR_FULL]
    );
    // Five in one batch: the whole batch is refused.
    assert_eq!(
        stage_speech(
            0,
            &[&[(1, true), (1, true), (1, true), (1, true), (1, true)]]
        ),
        [ERR_FULL]
    );
    // A speak that replaces never meets the cap, and starts the count again.
    assert_eq!(
        stage_speech(
            4,
            &[
                &[(0, true)],
                &[(1, true), (1, true), (1, true)],
                &[(1, true)]
            ]
        ),
        [OK, OK, ERR_FULL]
    );
    // A stop frees the plugin's texts.
    assert_eq!(stage_speech(4, &[&[(0, false)], &[(1, true)]]), [OK, OK]);
}

#[test]
fn the_kernels_speech_counts_a_plugins_texts() {
    let mut w = world(true);
    let (a, b) = (w.a.0.clone(), w.b.0.clone());
    w.speak(&a, 1, 0);
    w.speak(&a, 2, 1);
    w.speak(&b, 3, 1);
    assert_eq!(w.kernel.io.speech.owned_texts("a"), 2);
    assert_eq!(w.kernel.io.speech.owned_texts("b"), 1);
    // A replacing text of `b` ends all.
    w.speak(&b, 4, 0);
    assert_eq!(w.kernel.io.speech.owned_texts("a"), 0);
    assert_eq!(w.kernel.io.speech.owned_texts("b"), 1);
}
