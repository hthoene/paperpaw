//! PaperPaw as one of two firmwares beside another's (#95, phase 1): where
//! the setup is declared a dual-boot one (the marker in the bootloader, or
//! `dual-boot = on` in `paperpaw.ini`), nothing is written that the other
//! firmware's slot or the OTA data holds: the start leaves the OTA data as it
//! finds it, no trial begins, and no update or other firmware installs.
//! Where it is not declared, everything is as before. On a flash in memory,
//! with each partition table PaperPaw ships and CrossPoint's, in either slot.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{FLASH, layouts, table};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::firmware::{CHIP_ESP32_C3, Firmware, InstallFailure};
use paperpaw_x4::dual_boot::{MARKER, REFUSED, Source, WINDOW, X4BOOT, scan_marker};
use paperpaw_x4::ota::{Flash, FlashError, OtaData, SLOTS, Start, State};
use paperpaw_x4::partitions::{Slots, TABLE_AT, TABLE_LEN};
use paperpaw_x4::updates::{TRIAL_MS, TrialEnd, Updates};

/// The whole flash, in the chip's addresses, as the firmware has it.
struct Chip(Vec<u8>);

impl Flash for Chip {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        buf.copy_from_slice(&self.0[at as usize..at as usize + buf.len()]);
        Ok(())
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        for (byte, new) in self.0[at as usize..].iter_mut().zip(data) {
            *byte &= new;
        }
        Ok(())
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        self.0[from as usize..to as usize].fill(0xFF);
        Ok(())
    }
}

/// A flash with the table and something of the other firmware in each slot
/// (CrossPoint's, or another's: a few bytes of its own).
fn flash(csv: &str) -> (Chip, Slots) {
    let mut flash = vec![0xFF; FLASH];
    flash[TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN].copy_from_slice(&table(csv));
    let slots = Slots::find(&flash[TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN]).unwrap();
    for (n, &(at, _)) in slots.apps.iter().enumerate() {
        flash[at as usize..at as usize + 256].fill(0xE9 ^ n as u8);
    }
    (Chip(flash), slots)
}

fn this() -> Firmware {
    Firmware::new("0.6.0-beta.1", "paperpaw-xteink", CHIP_ESP32_C3)
}

fn data(chip: &Chip, slots: &Slots) -> Vec<u8> {
    chip.0[slots.data as usize..slots.data as usize + 0x2000].to_vec()
}

/// The other firmware chose its own slot (`chosen`), and the bootloader of
/// the tool started `running` anyway: the case where the firmware, left to
/// itself, would write the OTA data to choose its own again.
fn disagreeing(chip: &mut Chip, slots: &Slots, chosen: u32, state: State) {
    let mut part = Part(chip, slots.data);
    let mut ota = OtaData::read(&mut part).unwrap();
    ota.choose(&mut part, chosen, state, 0).unwrap();
}

/// What a tool leaves where it chose a slot as `New` or `Undefined`: the
/// entries without the word this firmware puts in its own.
fn as_tool(chip: &mut Chip, slots: &Slots) {
    for entry in [0, 0x1000] {
        let at = slots.data as usize + entry + 16;
        chip.0[at..at + 4].fill(0xFF);
    }
}

/// The OTA data's partition, as `Updates` hands it to `OtaData`.
struct Part<'a>(&'a mut Chip, u32);

impl Flash for Part<'_> {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.0.read(self.1 + at, buf)
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        self.0.program(self.1 + at, data)
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        self.0.erase(self.1 + from, self.1 + to)
    }
}

/// The start as the firmware does it: looked at (`open`, nothing written),
/// then settled once the card has said what is declared. What it planned,
/// what it did, and the flash as `open` left it.
fn start(
    chip: Chip,
    slots: &Slots,
    running: u32,
    declared: Option<Source>,
) -> (Updates<Chip>, Start, Option<Start>) {
    let before = chip.0.clone();
    let (mut updates, plan) = Updates::open(chip, slots, running, this(), "x4").unwrap();
    // Until the setup is known, not one byte is written, except for what
    // must not wait: an update this firmware installed, or one that failed
    // (`OtaData::acts_early`, tested below).
    if updates.start_pending() || plan == Start::Kept {
        assert!(updates.flash().0 == before, "open wrote");
    }
    assert_eq!(
        updates.start_pending(),
        plan != Start::Kept && updates.flash().0 == before
    );
    let started = updates.settle(declared);
    assert!(!updates.start_pending());
    (updates, plan, started)
}

#[test]
fn a_declared_setup_leaves_the_ota_data_as_it_finds_it() {
    for (name, csv) in layouts() {
        for running in 0..SLOTS {
            for declared in [Source::Marker, Source::Ini] {
                let (mut chip, slots) = flash(csv);
                // The other firmware's choice, which this start would undo.
                disagreeing(&mut chip, &slots, (running + 1) % SLOTS, State::Valid);
                let before = chip.0.clone();
                let (updates, plan, started) = start(chip, &slots, running, Some(declared));
                assert_eq!(plan, Start::Undone, "{name} {running}");
                assert_eq!(started, Some(Start::Kept), "{name} {running}");
                assert_eq!(updates.dual(), Some(declared), "{name}");
                assert!(!updates.on_trial(), "{name}");
                // Not one byte of the flash written.
                assert!(updates.into_flash().0 == before, "{name} {running}");
            }
        }
    }
}

#[test]
fn without_a_declaration_the_start_is_as_before() {
    for (name, csv) in layouts() {
        let (mut chip, slots) = flash(csv);
        disagreeing(&mut chip, &slots, 1, State::Valid);
        let before = data(&chip, &slots);
        let (updates, plan, started) = start(chip, &slots, 0, None);
        assert_eq!(plan, Start::Undone, "{name}");
        assert_eq!(started, Some(Start::Undone), "{name}");
        assert_eq!(updates.dual(), None, "{name}");
        assert_ne!(data(&updates.into_flash(), &slots), before, "{name}");
    }
}

#[test]
fn a_normal_start_writes_nothing_either_way() {
    for declared in [None, Some(Source::Marker)] {
        let (chip, slots) = flash(layouts()[0].1);
        let before = chip.0.clone();
        let (updates, plan, started) = start(chip, &slots, 0, declared);
        assert_eq!(plan, Start::Kept);
        assert_eq!(started, Some(Start::Kept));
        assert!(updates.into_flash().0 == before);
    }
}

#[test]
fn no_trial_begins_in_a_declared_setup() {
    // A tool chose this slot as `New`, as CrossPoint's installer does: the
    // firmware would mark it pending. The reader's ini says it is theirs
    // (no marker), after the start looked and before it did anything.
    for (name, csv) in layouts() {
        for declared in [Source::Ini, Source::Marker] {
            let (mut chip, slots) = flash(csv);
            disagreeing(&mut chip, &slots, 0, State::New);
            as_tool(&mut chip, &slots);
            let before = chip.0.clone();
            let (mut updates, plan, started) = start(chip, &slots, 0, Some(declared));
            assert_eq!(plan, Start::Trial { ran: 0 }, "{name}");
            assert_eq!(started, Some(Start::Kept), "{name}");
            assert!(!updates.on_trial(), "{name}");
            // And nothing is marked later either.
            assert!(!updates.check_trial(TRIAL_MS * 2).unwrap(), "{name}");
            updates.pause_trial(1000).unwrap();
            updates.resume_trial(2000).unwrap();
            assert!(updates.into_flash().0 == before, "{name}");
        }
    }
}

#[test]
fn an_update_this_firmware_installed_is_marked_at_once_and_goes_back_if_it_dies() {
    // Its own entry (`New`, with its word): marked before the card is read,
    // so that a hang or a panic in the board's start does not leave it
    // unmarked for good. The start that dies before the setup is settled
    // finds the trial failed the next time and goes back.
    for (name, csv) in layouts() {
        let (mut chip, slots) = flash(csv);
        disagreeing(&mut chip, &slots, 0, State::New);
        let before = data(&chip, &slots);
        let (updates, plan) = Updates::open(chip, &slots, 0, this(), "x4").unwrap();
        assert_eq!(plan, Start::Trial { ran: 0 }, "{name}");
        assert!(!updates.start_pending(), "{name}: done at once");
        assert!(updates.on_trial(), "{name}");
        let chip = updates.into_flash();
        assert_ne!(data(&chip, &slots), before, "{name}: marked");
        // It died here. The next start:
        let (updates, plan) = Updates::open(chip, &slots, 0, this(), "x4").unwrap();
        assert!(matches!(plan, Start::Failed { .. }), "{name}: {plan:?}");
        assert!(!updates.start_pending(), "{name}: marked bad at once");
        let mut chip = updates.into_flash();
        let mut part = Part(&mut chip, slots.data);
        let ota = OtaData::read(&mut part).unwrap();
        assert_eq!(
            ota.state_of(0).map(|(state, _)| state),
            Some(State::Invalid)
        );
    }
}

/// The flash already carries the bootloader's marker (#222): the start asks
/// before it writes anything at once.
fn open_declared(chip: Chip, slots: &Slots) -> (Updates<Chip>, Start) {
    Updates::open_with(chip, slots, 0, this(), "x4", || Some(Source::X4Boot)).unwrap()
}

#[test]
fn a_vouched_trial_in_a_marked_bootloader_writes_nothing_at_any_step() {
    // The same start as above, but the bootloader says dual boot: no mark
    // before the card, none after it, none at the trial's end, none at a
    // sleep (OTA data, other slot and all).
    for (name, csv) in layouts() {
        let (mut chip, slots) = flash(csv);
        disagreeing(&mut chip, &slots, 0, State::New);
        let before = chip.0.clone();
        let (mut updates, plan) = open_declared(chip, &slots);
        assert_eq!(plan, Start::Trial { ran: 0 }, "{name}");
        assert!(updates.start_pending(), "{name}: waits for the setup");
        assert!(!updates.on_trial(), "{name}");
        assert_eq!(updates.settle(Some(Source::X4Boot)), Some(Start::Kept));
        assert!(!updates.on_trial(), "{name}");
        // Time passes: the trial's end, a nap, a wake-up, a give-up.
        assert_eq!(updates.check_trial(TRIAL_MS * 2), Ok(false), "{name}");
        assert_eq!(updates.pause_trial(1000), Ok(()), "{name}");
        assert_eq!(updates.resume_trial(2000), Ok(()), "{name}");
        assert_eq!(updates.give_up(), Ok(false), "{name}");
        assert!(updates.into_flash().0 == before, "{name}: wrote");
    }
}

#[test]
fn a_failed_vouched_trial_in_a_marked_bootloader_does_not_go_back() {
    // The update died before (marked pending by this firmware): in a plain
    // setup this start marks it bad and chooses the firmware before it. Here
    // the other slot is another firmware, chosen by the tool: not touched.
    for (name, csv) in layouts() {
        let (mut chip, slots) = flash(csv);
        disagreeing(&mut chip, &slots, 0, State::New);
        let (updates, _) = Updates::open(chip, &slots, 0, this(), "x4").unwrap();
        let chip = updates.into_flash(); // died after the early mark
        let before = chip.0.clone();
        let (mut updates, plan) = open_declared(chip, &slots);
        assert!(matches!(plan, Start::Failed { .. }), "{name}: {plan:?}");
        assert!(updates.start_pending(), "{name}");
        assert_eq!(updates.settle(Some(Source::X4Boot)), Some(Start::Kept));
        assert_eq!(updates.check_trial(TRIAL_MS * 2), Ok(false), "{name}");
        assert!(updates.into_flash().0 == before, "{name}: wrote");
    }
}

#[test]
fn the_bootloader_is_asked_only_where_the_start_would_write() {
    for (name, csv) in layouts() {
        // Nothing to write (a start kept): not asked.
        let (chip, slots) = flash(csv);
        let mut asked = 0;
        let (_, plan) = Updates::open_with(chip, &slots, 0, this(), "x4", || {
            asked += 1;
            None
        })
        .unwrap();
        assert_eq!((plan, asked), (Start::Kept, 0), "{name}");
        // A vouched trial: asked once, and unmarked it is done at once.
        let (mut chip, slots) = flash(csv);
        disagreeing(&mut chip, &slots, 0, State::New);
        let (updates, _) = Updates::open_with(chip, &slots, 0, this(), "x4", || {
            asked += 1;
            None
        })
        .unwrap();
        assert_eq!(asked, 1, "{name}");
        assert!(updates.on_trial() && !updates.start_pending(), "{name}");
    }
}

#[test]
fn a_marked_bootloader_the_reader_switched_off_still_marks_the_trial() {
    // `dual-boot = off` in the ini wins over the marker: the start does what
    // it would have done at once, once the card has said so.
    for (name, csv) in layouts() {
        let (mut chip, slots) = flash(csv);
        disagreeing(&mut chip, &slots, 0, State::New);
        let before = chip.0.clone();
        let (mut updates, _) = open_declared(chip, &slots);
        assert_eq!(
            updates.settle(None),
            Some(Start::Trial { ran: 0 }),
            "{name}"
        );
        assert!(updates.on_trial(), "{name}");
        assert!(updates.into_flash().0 != before, "{name}: marked");
    }
}

#[test]
fn what_a_tool_chose_waits_for_the_setup_and_a_hang_leaves_it_as_it_was() {
    for (name, csv) in layouts() {
        let (mut chip, slots) = flash(csv);
        disagreeing(&mut chip, &slots, 0, State::New);
        as_tool(&mut chip, &slots);
        let before = chip.0.clone();
        let (updates, plan) = Updates::open(chip, &slots, 0, this(), "x4").unwrap();
        assert_eq!(plan, Start::Trial { ran: 0 }, "{name}");
        assert!(updates.start_pending(), "{name}");
        // Dying before the ini is read writes nothing: the next start is as
        // this one (the cost: no rollback of a tool's own install).
        assert!(updates.into_flash().0 == before, "{name}");
    }
}

#[test]
fn the_choice_the_update_page_writes_is_the_one_the_firmware_reads() {
    // #224: the page's button writes `off` (updates work again), its way
    // back `auto` (the bootloader's tag counts again); the firmware reads
    // both as the file has them, where the bootloader carries the tag.
    use paperpaw_sdk::config;
    use paperpaw_x4::dual_boot::{Choice, Evidence, IniScan, decide};
    let read = |text: &str| {
        let mut scan = IniScan::default();
        scan.feed(text.as_bytes());
        scan.finish()
    };
    let mut ini = String::from("[core]\nui-size = 12\n[firmware]\ndual-boot = on\n");
    for (value, choice, declared) in [
        ("off", Choice::Off, None),
        ("auto", Choice::Auto, Some(Source::X4Boot)),
        ("off", Choice::Off, None),
    ] {
        ini = config::set(&ini, "firmware", "dual-boot", value);
        assert_eq!(read(&ini), choice, "{value}");
        let marker = Some(Source::X4Boot);
        assert_eq!(decide(Evidence { choice, marker }), declared, "{value}");
        for (name, csv) in layouts() {
            let (chip, slots) = flash(csv);
            let (mut updates, _, _) = start(chip, &slots, 0, declared);
            let card = MemoryStorage::new();
            let refused = Some(InstallFailure::Other(REFUSED));
            let begun = updates
                .begin(&card, "updates/paperpaw.bin", None, true)
                .err();
            assert_eq!(
                begun == refused,
                declared.is_some(),
                "{name} {value}: {begun:?}"
            );
            assert_eq!(
                updates.firmware().room == 0,
                declared.is_some(),
                "{name} {value}"
            );
        }
    }
    assert!(ini.contains("ui-size = 12"));
}

#[test]
fn nothing_installs_in_a_declared_setup() {
    for (name, csv) in layouts() {
        let (chip, slots) = flash(csv);
        let other = (
            slots.apps[1].0 as usize,
            chip.0[slots.apps[1].0 as usize..][..4096].to_vec(),
        );
        let (mut updates, _, _) = start(chip, &slots, 0, Some(Source::Marker));
        let card = MemoryStorage::new();
        // Refused before the file is even read, whatever it is, and said as
        // no size or file problem.
        let refused = Some(InstallFailure::Other(REFUSED));
        assert_eq!(
            updates
                .begin(&card, "updates/paperpaw.bin", None, true)
                .err(),
            refused,
            "{name}"
        );
        assert_eq!(
            updates.begin_foreign(&card, "updates/other.bin").err(),
            refused,
            "{name}"
        );
        assert!(!updates.installing(), "{name}");
        // The page's own check finds no room for any update either.
        assert_eq!(updates.firmware().room, 0, "{name}");
        let chip = updates.into_flash();
        assert!(
            chip.0[other.0..][..4096] == other.1[..],
            "{name}: the other slot"
        );
    }
}

#[test]
fn without_a_declaration_installs_are_as_before() {
    let (chip, slots) = flash(layouts()[0].1);
    let (mut updates, _, _) = start(chip, &slots, 0, None);
    let card = MemoryStorage::new();
    // Not refused for being in a dual-boot setup: the file is not there.
    assert_eq!(
        updates
            .begin(&card, "updates/paperpaw.bin", None, true)
            .err(),
        Some(InstallFailure::Unreadable)
    );
    assert!(updates.firmware().room > 0);
}

#[test]
fn what_is_declared_guards_from_the_settling_on() {
    let (chip, slots) = flash(layouts()[0].1);
    let (mut updates, _, _) = start(chip, &slots, 0, Some(Source::Ini));
    let card = MemoryStorage::new();
    assert_eq!(updates.dual(), Some(Source::Ini));
    assert_eq!(
        updates
            .begin(&card, "updates/paperpaw.bin", None, true)
            .err(),
        Some(InstallFailure::Other(REFUSED))
    );
    assert_eq!(updates.firmware().room, 0);
    // Declared off again (`dual-boot = off` beats the marker): as before.
    updates.settle(None);
    assert!(updates.firmware().room > 0);
}

#[test]
fn the_marker_is_looked_for_in_the_bootloaders_window_only() {
    let (mut chip, _) = flash(layouts()[0].1);
    assert_eq!(scan_marker(&mut chip), Ok(None));
    // Across two pieces, at the very end of the window, and just beyond it.
    for (tag, source) in [(MARKER, Source::Marker), (X4BOOT, Source::X4Boot)] {
        for (at, found) in [
            (0x1000 - 3, true),
            (WINDOW as usize - tag.len(), true),
            (WINDOW as usize - tag.len() + 1, false),
            (WINDOW as usize + 0x100, false),
        ] {
            let (mut chip, _) = flash(layouts()[0].1);
            chip.0[at..at + tag.len()].copy_from_slice(tag);
            let want = found.then_some(source);
            assert_eq!(scan_marker(&mut chip), Ok(want), "{source:?} at {at:#x}");
        }
    }
    // Both: ours is the one said.
    let (mut chip, _) = flash(layouts()[0].1);
    chip.0[0x100..0x100 + X4BOOT.len()].copy_from_slice(X4BOOT);
    chip.0[0x7000..0x7000 + MARKER.len()].copy_from_slice(MARKER);
    assert_eq!(scan_marker(&mut chip), Ok(Some(Source::Marker)));
}

/// 86 bytes of the dualbooter's own hosted bootloader (`bins/bootloader.bin`
/// of lowio.xyz/tools/dualboot-x4, 21,712 bytes, SHA-256 9855f7b2…db3dfd),
/// read on 2026-11-04 as bytes only: its tag, with the text around it, where
/// the file has them (offset 0xb8; the tool's own check is the same six bytes
/// anywhere in the image).
const LOWIO_EXCERPT: [u8; 86] = [
    0x30, 0x38, 0x78, 0x0d, 0x0a, 0x00, 0x00, 0x00, 0x64, 0x6f, 0x77, 0x6e, 0x00, 0x00, 0x00, 0x00,
    0x75, 0x70, 0x00, 0x00, 0x6f, 0x6b, 0x00, 0x00, 0x56, 0x45, 0x52, 0x49, 0x46, 0x59, 0x20, 0x46,
    0x41, 0x49, 0x4c, 0x45, 0x44, 0x00, 0x00, 0x00, 0x78, 0x34, 0x62, 0x6f, 0x6f, 0x74, 0x00, 0x00,
    0x45, 0x20, 0x28, 0x25, 0x6c, 0x75, 0x29, 0x20, 0x25, 0x73, 0x3a, 0x20, 0x6c, 0x6f, 0x61, 0x64,
    0x20, 0x70, 0x61, 0x72, 0x74, 0x69, 0x74, 0x69, 0x6f, 0x6e, 0x20, 0x74, 0x61, 0x62, 0x6c, 0x65,
    0x20, 0x65, 0x72, 0x72, 0x6f, 0x72,
];
const LOWIO_EXCERPT_AT: usize = 0xb8;

#[test]
fn a_look_for_nothing_finds_nothing() {
    use paperpaw_x4::dual_boot::MarkerScan;
    assert!(!MarkerScan::of(b"").feed(b"anything"));
}

#[test]
fn the_real_dualbooter_bootloader_is_recognised() {
    let (mut chip, _) = flash(layouts()[0].1);
    chip.0[LOWIO_EXCERPT_AT..LOWIO_EXCERPT_AT + LOWIO_EXCERPT.len()]
        .copy_from_slice(&LOWIO_EXCERPT);
    assert_eq!(scan_marker(&mut chip), Ok(Some(Source::X4Boot)));
}

#[test]
fn the_note_the_firmware_writes_is_a_note_by_its_own_reading() {
    use paperpaw_sdk::Storage;
    use paperpaw_x4::dual_boot::{NOTE, note, noted};
    let card = MemoryStorage::new();
    assert!(!noted(""));
    for source in [Source::Ini, Source::Marker, Source::X4Boot] {
        for slot in 0..SLOTS {
            let text = note(source, Some(slot));
            assert!(noted(&text));
            card.write(NOTE, text.as_bytes()).unwrap();
            let mut kept = vec![0; text.len()];
            assert_eq!(card.read_at(NOTE, 0, &mut kept).unwrap(), text.len());
            assert!(noted(core::str::from_utf8(&kept).unwrap()));
            card.write(NOTE, b"").unwrap();
            assert_eq!(card.size(NOTE).unwrap(), 0);
        }
    }
}

/// A flash whose writes can be made to fail.
struct Failing(Chip, std::rc::Rc<std::cell::Cell<bool>>);

impl Flash for Failing {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.0.read(at, buf)
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        if self.1.get() {
            return Err(FlashError);
        }
        self.0.program(at, data)
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        if self.1.get() {
            return Err(FlashError);
        }
        self.0.erase(from, to)
    }
}

#[test]
fn a_trial_is_kept_only_where_its_mark_was_written() {
    // What holds back code plugins until the update is kept (#164) must not
    // end where the OTA data still says on trial: the update may go back.
    for fail in [false, true] {
        let (chip, slots) = flash(layouts()[0].1);
        let mut chip = chip;
        disagreeing(&mut chip, &slots, 0, State::New);
        as_tool(&mut chip, &slots);
        let broken = std::rc::Rc::new(std::cell::Cell::new(false));
        let (mut updates, _) =
            Updates::open(Failing(chip, broken.clone()), &slots, 0, this(), "x4").unwrap();
        assert_eq!(updates.settle(None), Some(Start::Trial { ran: 0 }));
        assert!(updates.on_trial());
        assert_eq!(updates.end_of_trial(TRIAL_MS / 2), TrialEnd::Running);
        broken.set(fail);
        let end = updates.end_of_trial(TRIAL_MS + 1);
        assert_eq!(
            end,
            if fail {
                TrialEnd::Unmarked
            } else {
                TrialEnd::Kept
            }
        );
        // What the flash says now: kept, or still to be proven.
        broken.set(false);
        let mut chip = updates.into_flash().0;
        let data = OtaData::read(&mut Part(&mut chip, slots.data)).unwrap();
        let state = data.state_of(0).map(|(state, _)| state);
        assert_eq!(
            state,
            Some(if fail {
                State::PendingVerify
            } else {
                State::Valid
            })
        );
    }
}
