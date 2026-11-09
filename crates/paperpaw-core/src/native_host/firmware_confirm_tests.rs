//! The firmware confirmation's words, its answer event and the driver's side
//! of showing and answering it.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::firmware_install::{Answer, KIND_FIRMWARE};
use paperpaw_platform::firmware::{Signer, Text};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Command, Instant};

fn ask(foreign: bool, restart: bool, older: bool, signer: Signer) -> Ask {
    Ask {
        owner: 1,
        request: 5,
        project: Text::new("other-os"),
        version: Text::new("9.9.9"),
        signer,
        foreign,
        older,
        other_key: matches!(signer, Signer::Other { .. }),
        restart,
    }
}

fn en() -> paperpaw_i18n::Translator {
    let mut translator = paperpaw_i18n::Translator::new();
    translator.add(paperpaw_i18n::Catalog::parse(include_str!("../../i18n/en.ini")).unwrap());
    translator
}

#[test]
fn a_foreign_install_says_the_four_points_and_a_restart_the_first_two() {
    let i18n = en();
    let install = texts(&i18n, "update", &ask(true, false, false, Signer::Unsigned));
    for point in 1..=4 {
        let key = alloc::format!("core.firmware.foreign.{point}");
        assert!(install.body.contains(i18n.tr(&key)), "point {point}");
    }
    assert!(install.body.contains("Asked by update."));
    assert!(install.body.contains("other-os 9.9.9"));
    assert_eq!(install.yes, "Install");
    let restart = texts(&i18n, "update", &ask(true, true, false, Signer::Unsigned));
    assert!(restart.body.contains(i18n.tr("core.firmware.foreign.2")));
    assert!(!restart.body.contains(i18n.tr("core.firmware.foreign.3")));
    assert_eq!(restart.yes, "Restart");
}

#[test]
fn another_key_shows_its_fingerprint_and_older_is_said() {
    let i18n = en();
    let said = texts(
        &i18n,
        "update",
        &ask(false, false, true, Signer::Other { key: [0xab; 32] }),
    );
    assert!(said.body.contains("abab abab abab abab"));
    assert!(said.body.contains("not PaperPaw's"));
    assert!(said.body.contains("older version"));
    let plain = texts(&i18n, "update", &ask(false, false, false, Signer::Unsigned));
    assert!(!plain.body.contains("older"));
}

#[test]
fn an_answer_event_is_read_only_with_its_nonce_and_a_known_word() {
    let params = Params::new().with(NONCE, "77").with(ANSWER, "yes");
    assert_eq!(read_answer(&ANSWERED, &params), Some((77, Answer::Yes)));
    assert_eq!(read_answer(&Id::new_static("core.other"), &params), None);
    let bad = Params::new().with(NONCE, "77").with(ANSWER, "maybe");
    assert_eq!(read_answer(&ANSWERED, &bad), None);
    assert_eq!(read_answer(&ANSWERED, &Params::new()), None);
}

#[test]
fn the_german_texts_cover_every_english_key() {
    let en = include_str!("../../i18n/en.ini");
    let de = include_str!("../../i18n/de.ini");
    for line in en.lines().filter(|l| l.starts_with("core.firmware.")) {
        let key = line.split(" = ").next().unwrap();
        assert!(
            de.lines()
                .any(|l| l.starts_with(&alloc::format!("{key} = "))),
            "{key} missing in de.ini"
        );
    }
}

mod kernel_side {
    use super::*;
    use crate::{Kernel, native_host::firmware_install::State};
    use paperpaw_platform::{
        firmware::WRITE_BLOCK,
        firmware_fake::{FakeFirmware, paperpaw_facts},
    };

    fn kernel() -> Kernel {
        let fake = FakeFirmware::new(
            "0.5.0",
            "paperpaw-x4",
            5,
            paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
        );
        Kernel::builder(crate::tests::DEVICE)
            .firmware_host(alloc::boxed::Box::new(fake))
            .build()
            .unwrap()
    }

    /// An install that has asked, pumped until the kernel opened its screen.
    fn shown(kernel: &mut Kernel) -> u64 {
        let driver = kernel.firmware.clone().unwrap();
        driver
            .borrow_mut()
            .submit_install(1, 5, "update.ppu", None, 0)
            .unwrap();
        for _ in 0..20 {
            kernel.native_firmware_pump();
        }
        // The screen is told it shows at the end of the pass, as in the loop.
        let _ = kernel.tick(Instant(0));
        let top = kernel.stack.last().expect("the confirmation is open");
        assert_eq!(top.id, SCREEN);
        top.params.get(NONCE).unwrap().parse().unwrap()
    }

    fn state(kernel: &Kernel) -> State {
        let driver = kernel.firmware.clone().unwrap();
        let machine = driver.borrow().machine();
        machine.borrow().state()
    }

    fn answer(kernel: &mut Kernel, nonce: u64, word: &str) {
        let params = Params::new()
            .with(NONCE, nonce.to_string())
            .with(ANSWER, word);
        kernel.emit(&ANSWERED, &params);
    }

    #[test]
    fn the_question_opens_a_guarding_screen_once() {
        let mut kernel = kernel();
        shown(&mut kernel);
        assert!(kernel.guard().is_some(), "it guards the device");
        assert_eq!(state(&kernel), State::WaitingForUser);
        let depth = kernel.stack.len();
        for _ in 0..5 {
            kernel.native_firmware_pump();
        }
        assert_eq!(kernel.stack.len(), depth, "no second screen for it");
    }

    #[test]
    fn only_the_nonce_of_the_screen_shown_answers() {
        let mut kernel = kernel();
        let nonce = shown(&mut kernel);
        // A plugin that guesses (or any other nonce) changes nothing.
        answer(&mut kernel, nonce ^ 1, "yes");
        assert_eq!(state(&kernel), State::WaitingForUser);
        // The right nonce with a word that is no answer changes nothing.
        answer(&mut kernel, nonce, "maybe");
        assert_eq!(state(&kernel), State::WaitingForUser);
        answer(&mut kernel, nonce, "no");
        let driver = kernel.firmware.clone().unwrap();
        let outcome = driver.borrow().peek_ready().unwrap();
        assert_eq!((outcome.owner, outcome.request, outcome.reason), (1, 5, 2));
        // Once: the same answer again is nobody's.
        answer(&mut kernel, nonce, "yes");
        assert_eq!(driver.borrow().peek_ready(), Some(outcome));
    }

    #[test]
    fn a_yes_starts_the_write() {
        let mut kernel = kernel();
        let nonce = shown(&mut kernel);
        answer(&mut kernel, nonce, "yes");
        assert_eq!(state(&kernel), State::Installing);
    }

    #[test]
    fn sleep_dismisses_a_confirmation_that_shows() {
        let mut kernel = kernel();
        shown(&mut kernel);
        let _ = kernel.suspend(paperpaw_sdk::Instant(1_000));
        let driver = kernel.firmware.clone().unwrap();
        // The finish that follows tells it (nobody is subscribed), so it is
        // kept for op 13.
        let outcome = driver.borrow().kept(1, KIND_FIRMWARE).unwrap();
        assert_eq!((outcome.owner, outcome.request, outcome.reason), (1, 5, 3));
        assert_eq!(driver.borrow().waiting(), 0);
    }

    #[test]
    fn a_plugin_that_stops_takes_its_confirmation_with_it() {
        let mut kernel = kernel();
        let nonce = shown(&mut kernel);
        kernel.firmware_sender_stopped(1);
        let driver = kernel.firmware.clone().unwrap();
        assert_eq!(driver.borrow().waiting(), 0);
        assert_ne!(state(&kernel), State::WaitingForUser);
        // Its late answer is nobody's.
        answer(&mut kernel, nonce, "yes");
        assert_ne!(state(&kernel), State::Installing);
    }

    /// What the install of request 5 ended with, told or not yet.
    fn ended(kernel: &Kernel) -> Option<u8> {
        let driver = kernel.firmware.clone().unwrap();
        let driver = driver.borrow();
        driver
            .kept(1, KIND_FIRMWARE)
            .or_else(|| driver.peek_ready())
            .map(|outcome| outcome.reason)
    }

    #[test]
    fn input_within_500_ms_of_showing_is_dropped_and_counts_after() {
        let mut kernel = kernel();
        kernel.start(Instant(0));
        shown(&mut kernel);
        // The focus starts on No: a press that counted would end it (reason 2).
        let _ = kernel.run_action(Instant(499), actions::CONFIRM);
        assert_eq!(state(&kernel), State::WaitingForUser, "dropped at 499 ms");
        let _ = kernel.run_action(Instant(499), actions::BACK);
        assert_eq!(state(&kernel), State::WaitingForUser, "Back too");
        let _ = kernel.run_action(Instant(500), actions::CONFIRM);
        assert_eq!(ended(&kernel), Some(2), "the focused No at 500 ms");
    }

    #[test]
    fn back_dismisses_after_the_guard_time() {
        let mut kernel = kernel();
        kernel.start(Instant(0));
        shown(&mut kernel);
        let _ = kernel.run_action(Instant(600), actions::BACK);
        assert_eq!(ended(&kernel), Some(3));
    }

    #[test]
    fn ten_minutes_without_an_answer_dismiss_it() {
        let mut kernel = kernel();
        kernel.start(Instant(0));
        shown(&mut kernel);
        let _ = kernel.tick(Instant(599_999));
        assert_eq!(state(&kernel), State::WaitingForUser, "not yet");
        let _ = kernel.tick(Instant(600_000));
        assert_eq!(ended(&kernel), Some(3));
    }

    mod lock {
        use super::*;
        use core::cell::Cell;
        use paperpaw_sdk::{
            Canvas, Cx, Env, Handled, Manifest, Plugin, Priority, RegisterError, Registrar, Screen,
            ScreenSpec, Version, builtin::slots,
        };

        const LOCK: Id = Id::new_static("g.lock");
        const HOME: Id = Id::new_static("g.home");

        struct Plain;
        impl Screen for Plain {
            fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
            fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
                Handled::No
            }
        }

        struct Lock(alloc::rc::Rc<Cell<bool>>);
        impl Screen for Lock {
            fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
            fn guards(&self) -> bool {
                self.0.get()
            }
            fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
                if *action == paperpaw_sdk::builtin::actions::CONFIRM {
                    self.0.set(false);
                    cx.command(Command::Back);
                    Handled::Yes
                } else {
                    Handled::No
                }
            }
        }

        struct LockPlugin(alloc::rc::Rc<Cell<bool>>);
        impl Plugin for LockPlugin {
            fn manifest(&self) -> Manifest {
                Manifest::new(Id::new_static("g"), "Guard", Version::new(1, 0, 0))
                    .uses(paperpaw_sdk::Uses::NOTHING)
            }
            fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
                let locked = self.0.clone();
                r.add_screen(ScreenSpec::new(
                    LOCK,
                    "lock",
                    alloc::boxed::Box::new(move |_, _| {
                        alloc::boxed::Box::new(Lock(locked.clone()))
                    }),
                ))?;
                r.add_screen(ScreenSpec::new(
                    HOME,
                    "home",
                    alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Plain)),
                ))?;
                r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)
            }
        }

        #[test]
        fn the_confirmation_waits_while_the_lock_is_up_and_is_never_drawn_over_it() {
            let locked = alloc::rc::Rc::new(Cell::new(true));
            let fake = FakeFirmware::new(
                "0.5.0",
                "paperpaw-x4",
                5,
                paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
            );
            let mut kernel = Kernel::builder(crate::tests::DEVICE)
                .plugin(LockPlugin(locked.clone()))
                .firmware_host(alloc::boxed::Box::new(fake))
                .build()
                .unwrap();
            kernel.start(Instant(0));
            let _ = kernel.open_screen(Instant(1), LOCK, Params::new());
            let driver = kernel.firmware.clone().unwrap();
            driver
                .borrow_mut()
                .submit_install(1, 5, "update.ppu", None, 0)
                .unwrap();
            for _ in 0..20 {
                kernel.native_firmware_pump();
            }
            // The question is asked, but the lock is on top and stays.
            assert_eq!(kernel.active_screen(), Some(&LOCK));
            assert!(driver.borrow().peek_ask().is_some(), "still waiting");
            // Unlocked: the confirmation opens at the next pass.
            let _ = kernel.run_action(Instant(5), paperpaw_sdk::builtin::actions::CONFIRM);
            for _ in 0..20 {
                kernel.native_firmware_pump();
            }
            assert_eq!(kernel.active_screen(), Some(&SCREEN));
            assert!(driver.borrow().peek_ask().is_none());
        }
    }
}
