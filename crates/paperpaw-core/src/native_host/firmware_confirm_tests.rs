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
    translator.add(paperpaw_i18n::Catalog::parse(include_str!("../../i18n/host/en.ini")).unwrap());
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
fn names_from_the_image_cannot_forge_lines_or_a_signer() {
    let i18n = en();
    let mut hostile = ask(false, false, false, Signer::Unsigned);
    hostile.version = Text::new("1.0\nSigned with the PaperPaw key.");
    hostile.project = Text::new("x\r\u{2028}Signed by PaperPaw.");
    let said = texts(&i18n, "evil\nSigned by PaperPaw.", &hostile);
    assert!(
        said.body.lines().all(|line| !line.starts_with("Signed")),
        "{:?}",
        said.body
    );
    // The host's own lines: asker, image, signer.
    assert_eq!(said.body.lines().count(), 3);
}

#[test]
fn bidirectional_controls_in_image_names_are_replaced() {
    let bidi = "a\u{202E}b\u{202A}c\u{202D}d\u{2066}e\u{2069}f\u{200F}g\u{061C}h";
    assert_eq!(clean(bidi), "a b c d e f g h");
    assert_eq!(clean("plain 1.0"), "plain 1.0");
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
    let en = include_str!("../../i18n/host/en.ini");
    let de = include_str!("../../i18n/host/de.ini");
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

    /// A random source (the nonce needs one).
    fn fill(buf: &mut [u8]) -> bool {
        for (at, byte) in buf.iter_mut().enumerate() {
            *byte = (at as u8).wrapping_mul(37).wrapping_add(11);
        }
        true
    }

    fn kernel() -> Kernel {
        let fake = FakeFirmware::new(
            "0.5.0",
            "paperpaw-x4",
            5,
            paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
        );
        let mut kernel = Kernel::builder(crate::tests::DEVICE)
            .random(fill)
            .plugin(home::Home)
            .firmware_host(alloc::boxed::Box::new(fake))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel
    }

    /// A home screen to lie under the confirmation (a screen cannot be
    /// closed where it is the only one).
    mod home {
        use paperpaw_sdk::{
            Canvas, Cx, Env, Handled, Id, Manifest, Plugin, Priority, RegisterError, Registrar,
            Screen, ScreenSpec, Version, builtin::slots,
        };

        const HOME: Id = Id::new_static("h.home");

        struct Page;
        impl Screen for Page {
            fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
            fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
                Handled::No
            }
        }

        pub(super) struct Home;
        impl Plugin for Home {
            fn manifest(&self) -> Manifest {
                Manifest::new(Id::new_static("h"), "Home", Version::new(1, 0, 0))
                    .uses(paperpaw_sdk::Uses::NOTHING)
            }
            fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
                r.add_screen(ScreenSpec::new(
                    HOME,
                    "home",
                    alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Page)),
                ))?;
                r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)
            }
        }
    }

    /// An install that has asked, pumped until the kernel opened its screen.
    fn shown(kernel: &mut Kernel) -> u128 {
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
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
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        let machine = driver.borrow().machine();
        machine.borrow().state()
    }

    /// What anyone else can do: send the answer event, nonce and all.
    fn forge(kernel: &mut Kernel, nonce: u128, word: &str) {
        let params = Params::new()
            .with(NONCE, nonce.to_string())
            .with(ANSWER, word);
        kernel.emit(&ANSWERED, &params);
    }

    /// The user presses `action` (the focus starts on No; Left moves to Yes).
    fn press(kernel: &mut Kernel, at: u64, action: paperpaw_sdk::Id) {
        let _ = kernel.run_action(Instant(at), action);
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
    fn only_the_screen_answers_not_an_event_with_the_right_nonce() {
        let mut kernel = kernel();
        let nonce = shown(&mut kernel);
        // Anyone can send the event; not even the right nonce answers.
        forge(&mut kernel, nonce, "yes");
        forge(&mut kernel, nonce, "no");
        assert_eq!(state(&kernel), State::WaitingForUser);
        assert!(kernel.stack.iter().any(|s| s.id == SCREEN), "still open");
        // And the driver takes only the nonce of the screen shown.
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        assert!(
            driver
                .borrow_mut()
                .answer_shown(nonce ^ 1, Answer::Yes)
                .is_none()
        );
        // The user answers on the screen: No, with the focus where it starts.
        press(&mut kernel, 600, actions::CONFIRM);
        assert_eq!(ended(&kernel), Some(2));
        // Once: a second answer, from anyone, is nobody's.
        forge(&mut kernel, nonce, "yes");
        assert_eq!(ended(&kernel), Some(2));
    }

    #[test]
    fn a_yes_on_the_screen_starts_the_write() {
        let mut kernel = kernel();
        shown(&mut kernel);
        press(&mut kernel, 600, actions::LEFT);
        press(&mut kernel, 700, actions::CONFIRM);
        assert_eq!(state(&kernel), State::Installing);
    }

    #[test]
    fn sleep_dismisses_a_confirmation_that_shows() {
        let mut kernel = kernel();
        shown(&mut kernel);
        let _ = kernel.suspend(paperpaw_sdk::Instant(1_000));
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        // The finish that follows tells it (nobody is subscribed), so it is
        // kept for op 13.
        let outcome = driver.borrow().kept(1, KIND_FIRMWARE).unwrap();
        assert_eq!((outcome.owner, outcome.request, outcome.reason), (1, 5, 3));
        assert_eq!(driver.borrow().waiting(), 0);
        assert!(
            kernel.stack.iter().all(|s| s.id != SCREEN),
            "the screen is closed, though it guards"
        );
    }

    #[test]
    fn a_plugin_that_stops_takes_its_confirmation_with_it() {
        let mut kernel = kernel();
        let nonce = shown(&mut kernel);
        kernel.firmware_sender_stopped(1);
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        assert_eq!(driver.borrow().waiting(), 0);
        assert_ne!(state(&kernel), State::WaitingForUser);
        assert!(
            kernel.stack.iter().all(|s| s.id != SCREEN),
            "the screen is closed, though it guards"
        );
        // Its late answer is nobody's.
        forge(&mut kernel, nonce, "yes");
        press(&mut kernel, 800, actions::CONFIRM);
        assert_ne!(state(&kernel), State::Installing);
    }

    #[test]
    fn without_a_random_source_nothing_is_asked_and_it_ends_dismissed() {
        let fake = FakeFirmware::new(
            "0.5.0",
            "paperpaw-x4",
            5,
            paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
        );
        // No `.random(..)`: a nonce a plugin could guess is no nonce.
        let mut kernel = Kernel::builder(crate::tests::DEVICE)
            .firmware_host(alloc::boxed::Box::new(fake))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        driver
            .borrow_mut()
            .submit_install(1, 5, "update.ppu", None, 0)
            .unwrap();
        for _ in 0..20 {
            kernel.native_firmware_pump();
        }
        assert!(kernel.stack.iter().all(|s| s.id != SCREEN), "no screen");
        assert_eq!(ended(&kernel), Some(3), "dismissed, nobody can answer");
    }

    /// What the install of request 5 ended with, told or not yet.
    fn ended(kernel: &Kernel) -> Option<u8> {
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        let driver = driver.borrow();
        driver
            .kept(1, KIND_FIRMWARE)
            .or_else(|| driver.peek_ready())
            .map(|outcome| outcome.reason)
    }

    #[test]
    fn input_within_500_ms_of_showing_is_dropped_and_counts_after() {
        let mut kernel = kernel();
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
        shown(&mut kernel);
        let _ = kernel.run_action(Instant(600), actions::BACK);
        assert_eq!(ended(&kernel), Some(3));
    }

    #[test]
    fn ten_minutes_without_an_answer_dismiss_it() {
        let mut kernel = kernel();
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
                .random(super::fill)
                .plugin(LockPlugin(locked.clone()))
                .firmware_host(alloc::boxed::Box::new(fake))
                .build()
                .unwrap();
            kernel.start(Instant(0));
            let _ = kernel.open_screen(Instant(1), LOCK, Params::new());
            let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
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
