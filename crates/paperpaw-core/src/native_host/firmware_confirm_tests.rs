//! The firmware confirmation's words, its answer event and the driver's side
//! of showing and answering it.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::firmware_install::{Answer, KIND_FIRMWARE};
use paperpaw_platform::firmware::{Signer, Text};

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
}
