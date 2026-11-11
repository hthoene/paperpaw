//! `firmware_dual_boot` as a job (`device.md` 5.3 "Dual boot"): the gates, the
//! always-asked mode 1, the write order, and the driver's restart, latch and
//! flags.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::dual_boot_ini::{INI_MAX, INI_PATH, card_choice};
use crate::native_host::firmware_driver::Driver;
use crate::native_host::firmware_install::{BuiltIns, Event, FirmwareInstall, Outcome};
use crate::native_host::plugin_job::PluginHost;
use alloc::{boxed::Box, rc::Rc};
use core::cell::RefCell;
use paperpaw_native::{
    header5::{Host as Loader, X4_PROFILE},
    metadata::Host as Offered,
};
use paperpaw_platform::{
    Storage,
    firmware::WRITE_BLOCK,
    firmware_fake::{FakeFirmware, paperpaw_facts},
};
use paperpaw_sdk::MemoryStorage;

extern crate std;

struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        0
    }
    fn index_of(&self, _: &str) -> Option<usize> {
        None
    }
}

fn host() -> PluginHost {
    PluginHost {
        loader: Loader::x4(1),
        offered: Offered {
            abi_major: 7,
            minor: 1,
            profile: X4_PROFILE,
            services: &[],
            features: &[],
        },
        signers: &[],
    }
}

/// What a run came to.
struct Run {
    done: Finished,
    asked: Option<PluginQuestion>,
}

fn drive(job: &mut DualBoot, card: &dyn Storage, answer: Answer) -> Run {
    let host = host();
    let mut asked = None;
    for _ in 0..50 {
        let env = JobEnv {
            storage: card,
            host: &host,
            built_ins: &Names,
            granted: true,
        };
        match job.step(&env) {
            JobStep::More => {}
            JobStep::Wait => panic!("waits for what nobody gives"),
            JobStep::Ask(question) => {
                asked = Some(question);
                job.answer(answer);
            }
            JobStep::Done(done) => return Run { done, asked },
            other => panic!("{other:?}"),
        }
    }
    panic!("never ended");
}

fn card_with(ini: &str) -> MemoryStorage {
    let card = MemoryStorage::new();
    card.write(INI_PATH, ini.as_bytes()).unwrap();
    card.write(NOTE_PATH, b"dual-boot source=ini slot=1\n")
        .unwrap();
    card
}

fn job(mode: Mode) -> DualBoot {
    DualBoot::new(7, 3, mode, true)
}

#[test]
fn a_device_without_a_second_partition_or_with_an_image_waiting_changes_nothing() {
    let card = card_with("");
    let mut job = DualBoot::new(7, 3, Mode::Off, false);
    let run = drive(&mut job, &card, Answer::Yes);
    assert_eq!((run.done.reason, run.asked), (reason::NOT_HERE, None));
    let mut job = DualBoot::new(7, 3, Mode::Off, true);
    job.image_waiting(true);
    let run = drive(&mut job, &card, Answer::Yes);
    assert_eq!((run.done.reason, run.asked), (21, None));
    // Reason 21 comes before 19.
    let mut job = DualBoot::new(7, 3, Mode::Off, false);
    job.image_waiting(true);
    assert_eq!(drive(&mut job, &card, Answer::Yes).done.reason, 21);
    assert_eq!(card_choice(&card), Ok(Choice::Auto));
    assert!(card.size(NOTE_PATH).is_ok());
}

#[test]
fn a_file_that_cannot_be_read_whole_ends_with_reason_16_and_nothing_is_written() {
    let card = MemoryStorage::new();
    card.write(INI_PATH, &std::vec![b'a'; INI_MAX + 1]).unwrap();
    for mode in [Mode::Off, Mode::Auto] {
        let mut job = job(mode);
        let run = drive(&mut job, &card, Answer::Yes);
        assert_eq!((run.done.reason, run.asked), (reason::CARD, None));
        assert_eq!(job.after(), After::Nothing);
    }
    assert_eq!(card.size(INI_PATH).unwrap(), INI_MAX as u64 + 1);
}

#[test]
fn where_nothing_would_change_it_ends_with_reason_0_and_asks_and_writes_nothing() {
    let card = card_with("[firmware]\ndual-boot = off\n");
    let mut off = job(Mode::Off);
    let run = drive(&mut off, &card, Answer::No);
    assert_eq!((run.done.reason, run.asked), (0, None));
    assert_eq!(off.after(), After::Nothing);
    let card = card_with("[firmware]\ndual-boot = on\n");
    let mut auto = job(Mode::Auto);
    assert_eq!(drive(&mut auto, &card, Answer::No).done.reason, 0);
    assert_eq!(auto.after(), After::Nothing);
    assert_eq!(card_choice(&card), Ok(Choice::On));
    assert!(card.size(NOTE_PATH).is_ok());
}

#[test]
fn mode_1_always_asks_and_a_no_or_a_dismissal_changes_nothing() {
    for (answer, why) in [(Answer::No, 2), (Answer::Dismissed, 3)] {
        let card = card_with("[firmware]\ndual-boot = on\n");
        let mut job = job(Mode::Off);
        let run = drive(&mut job, &card, answer);
        assert_eq!(run.done.reason, why);
        let question = run.asked.unwrap();
        assert_eq!((question.owner, question.request), (7, 3));
        assert_eq!(question.kind, 1);
        assert_eq!(job.after(), After::Nothing);
        assert_eq!(card_choice(&card), Ok(Choice::On));
        assert!(card.size(NOTE_PATH).is_ok());
    }
}

#[test]
fn a_yes_writes_off_removes_the_note_and_asks_for_the_restart() {
    let card = card_with("; mine\n[firmware]\ndual-boot = on\n");
    let mut job = job(Mode::Off);
    let run = drive(&mut job, &card, Answer::Yes);
    assert_eq!(run.done.reason, 0);
    assert!(run.asked.is_some());
    assert_eq!(job.after(), After::Restart);
    assert_eq!(card_choice(&card), Ok(Choice::Off));
    assert!(card.size(NOTE_PATH).is_err());
    let text = card.get(INI_PATH).unwrap();
    assert!(text.starts_with(b"; mine\n"));
}

#[test]
fn mode_0_does_not_ask_does_not_restart_and_latches() {
    let card = card_with("[firmware]\ndual-boot = off\n");
    let mut job = job(Mode::Auto);
    let run = drive(&mut job, &card, Answer::No);
    assert_eq!((run.done.reason, run.asked), (0, None));
    assert_eq!(job.after(), After::Declared);
    assert_eq!(card_choice(&card), Ok(Choice::Auto));
    // The note is not touched by mode 0.
    assert!(card.size(NOTE_PATH).is_ok());
}

#[test]
fn an_aborted_job_ends_with_the_given_reason_and_writes_nothing() {
    let card = card_with("[firmware]\ndual-boot = off\n");
    let mut job = job(Mode::Auto);
    job.abort(20);
    assert_eq!(drive(&mut job, &card, Answer::Yes).done.reason, 20);
    assert_eq!(card_choice(&card), Ok(Choice::Off));
}

fn driver(declared: bool) -> Driver {
    let mut fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    fake.dual_capable = true;
    fake.dual_declared = declared;
    fake.can_restart_running = true;
    let mut d = Driver::new(
        Box::new(fake),
        Rc::new(RefCell::new(FirmwareInstall::new())),
    );
    d.set_plugin_host(host());
    d.open_dual_boot();
    d
}

fn pump_to_end(d: &mut Driver, card: &dyn Storage) -> Outcome {
    for now in (100..20_000).step_by(100) {
        if let Event::Ended(outcome) = d.pump(card, &Names, now) {
            return outcome;
        }
    }
    panic!("never ended");
}

#[test]
fn the_driver_restarts_after_a_yes_and_tells_the_flags() {
    let card = card_with("[firmware]\ndual-boot = on\n");
    let mut d = driver(true);
    d.submit_dual_boot(7, 3, Mode::Off).unwrap();
    // The first idle pass read the file: `shared` and no `ini_off`.
    d.pump(&card, &Names, 50);
    assert_eq!(d.machine().borrow().dual_flags(), 0b01);
    // Mode 1 asks: the pump stops on the question.
    for now in (100..2_000).step_by(100) {
        d.pump(&card, &Names, now);
    }
    let question = *d.plugin_question().unwrap();
    assert_eq!((question.owner, question.request, question.kind), (7, 3, 1));
    assert!(d.answer_plugin(7, 3, Answer::Yes));
    let outcome = pump_to_end(&mut d, &card);
    assert_eq!((outcome.kind, outcome.reason), (1, 0));
    assert!(d.restart_wanted());
    assert_eq!(d.machine().borrow().dual_flags(), 0b11);
    // The restart is of the running image.
    assert!(!d.restart_now());
    assert!(!d.restart_wanted());
}

#[test]
fn mode_0_latches_the_setup_declared_and_an_install_ends_with_reason_19() {
    let card = card_with("[firmware]\ndual-boot = off\n");
    let mut d = driver(false);
    d.submit_dual_boot(7, 3, Mode::Auto).unwrap();
    d.pump(&card, &Names, 50);
    assert_eq!(d.machine().borrow().dual_flags(), 0b10);
    let outcome = pump_to_end(&mut d, &card);
    assert_eq!(outcome.reason, 0);
    // No restart: a plugin must not restart the device by it.
    assert!(!d.restart_wanted());
    assert_eq!(d.machine().borrow().dual_flags(), 0b01);
    d.submit_install(7, 4, "u.ppu", None, 0).unwrap();
    let outcome = pump_to_end(&mut d, &card);
    assert_eq!((outcome.kind, outcome.reason), (1, 19));
}

#[test]
fn the_command_is_unsupported_until_the_host_opens_it() {
    let mut fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    fake.dual_capable = true;
    let d = Driver::new(
        Box::new(fake),
        Rc::new(RefCell::new(FirmwareInstall::new())),
    );
    assert!(!d.dual_boot_open());
}

#[test]
fn a_latch_that_found_the_machine_borrowed_is_kept_and_applied_next() {
    let card = card_with("[firmware]\ndual-boot = off\n");
    let mut d = driver(false);
    let machine = d.machine();
    {
        // A host fault: the machine is held when the job ends.
        let _held = machine.borrow_mut();
        d.dual_after(After::Declared);
        assert!(d.has_work(0));
    }
    // Free again: the next pass applies it, and nothing is lost.
    d.pump(&card, &Names, 100);
    assert_eq!(machine.borrow().dual_flags() & 1, 1);
    assert!(!d.has_work(0));
    d.submit_install(7, 4, "u.ppu", None, 0).unwrap();
    let outcome = pump_to_end(&mut d, &card);
    assert_eq!((outcome.kind, outcome.reason), (1, 19));
}

/// A card whose removals fail: the power went between the file and the note.
struct NoRemove<'a>(&'a MemoryStorage);
impl Storage for NoRemove<'_> {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, _: &str) -> Result<(), paperpaw_platform::StorageError> {
        Err(paperpaw_platform::StorageError::Io)
    }
}

#[test]
fn a_cut_between_the_file_and_the_note_keeps_the_users_yes_and_the_note() {
    let card = card_with("[firmware]\ndual-boot = on\n");
    let mut job = job(Mode::Off);
    let run = drive(&mut job, &NoRemove(&card), Answer::Yes);
    // The file is written first and whole; the note that stays is harmless
    // (`off` wins over it), and the restart still comes.
    assert_eq!(run.done.reason, 0);
    assert_eq!(job.after(), After::Restart);
    assert_eq!(card_choice(&card), Ok(Choice::Off));
    assert!(card.size(NOTE_PATH).is_ok());
}

#[test]
fn a_plugin_that_holds_the_install_grant_is_still_asked_for_mode_1() {
    // `drive` steps with `granted: true`, as a trusted installer's job runs.
    let card = card_with("[firmware]\ndual-boot = on\n");
    let mut job = job(Mode::Off);
    let run = drive(&mut job, &card, Answer::No);
    assert!(run.asked.is_some());
    assert_eq!(run.done.reason, 2);
    assert_eq!(card_choice(&card), Ok(Choice::On));
}
