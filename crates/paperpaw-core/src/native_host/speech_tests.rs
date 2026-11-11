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
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_sdk::{Command, Id, Instant};

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
