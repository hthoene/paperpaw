//! Updates over the air with every partition table PaperPaw ships, and
//! CrossPoint's (which PaperPaw runs in when installed from its menu): the
//! table read as the firmware reads it, then whole updates on a flash in
//! memory, as the firmware does them (`shells/xteink/src/ota.rs`) and as
//! ESP-IDF's bootloader chooses: after a first flash, after one update
//! and after two, kept or undone. An update always finds a slot, never
//! the one running, and nothing else on the flash is touched.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_x4::ota::{Flash, FlashError, Image, OtaData, Start, State};
use paperpaw_x4::partitions::{Slots, TABLE_AT, TABLE_LEN, entries};

mod common;

use common::{FLASH, layouts, table};

/// A part of the flash, as the firmware hands one to `paperpaw_x4::ota`:
/// nothing outside it may be read or written.
struct Part<'a> {
    flash: &'a mut [u8],
    at: u32,
    len: u32,
}

impl Part<'_> {
    fn range(&self, at: u32, len: usize) -> core::ops::Range<usize> {
        assert!(
            at as usize + len <= self.len as usize,
            "outside its part: {at:#x}+{len:#x} of {:#x}",
            self.len
        );
        let from = (self.at + at) as usize;
        from..from + len
    }
}

impl Flash for Part<'_> {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let range = self.range(at, buf.len());
        buf.copy_from_slice(&self.flash[range]);
        Ok(())
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        let range = self.range(at, data.len());
        for (byte, new) in self.flash[range].iter_mut().zip(data) {
            *byte &= new;
        }
        Ok(())
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        assert!(from.is_multiple_of(0x1000) && to.is_multiple_of(0x1000));
        let range = self.range(from, (to - from) as usize);
        self.flash[range].fill(0xFF);
        Ok(())
    }
}

/// A device's flash with one table on it, flashed as the web flasher does:
/// the table, the firmware in the first slot, the OTA data erased.
struct Device {
    flash: Vec<u8>,
    slots: Slots,
}

impl Device {
    fn flashed(csv: &str) -> Self {
        let mut flash = vec![0xFF; FLASH];
        let table = table(csv);
        flash[TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN].copy_from_slice(&table);
        // Something in every partition, to see that it stays.
        for (n, partition) in entries(&table).enumerate() {
            let from = partition.offset as usize;
            flash[from..from + 64].fill(n as u8);
        }
        let slots = read_slots(&flash);
        let (first, size) = slots.apps[0];
        let image = image(1, size);
        flash[first as usize..first as usize + image.len()].copy_from_slice(&image);
        Self { flash, slots }
    }

    fn data(&mut self) -> Part<'_> {
        let at = self.slots.data;
        Part {
            flash: &mut self.flash,
            at,
            len: 0x2000,
        }
    }

    fn ota_data(&mut self) -> OtaData {
        OtaData::read(&mut self.data()).unwrap()
    }

    /// The slot the bootloader starts now.
    fn booted(&mut self) -> u32 {
        self.ota_data().booted()
    }

    /// A start, as the firmware's: the slot running and what it did.
    fn start(&mut self) -> (u32, Start) {
        let running = self.booted();
        let mut data = self.ota_data();
        let start = data.start(&mut self.data(), running).unwrap();
        (running, start)
    }

    /// The trial is over: the update is kept.
    fn keep(&mut self, running: u32) {
        let mut data = self.ota_data();
        data.set_state(&mut self.data(), running, State::Valid, 0)
            .unwrap();
    }

    /// Installs update number `n` beside the firmware running, as the
    /// firmware does: the slot readied, the image written a piece at a
    /// time with its first word last, then chosen. The slot written.
    fn install(&mut self, running: u32, n: u8) -> u32 {
        let mut data = self.ota_data();
        let target = data.prepare_install(&mut self.data(), running).unwrap();
        assert_ne!(target, running, "an update never goes where it runs");
        assert_eq!(target, self.slots.target(running));
        let (at, len) = self.slots.apps[target as usize];
        let bytes = image(n, len);
        let mut part = Part {
            flash: &mut self.flash,
            at,
            len,
        };
        let mut writing = Image::new(bytes.len() as u32);
        let mut buf = vec![0u8; 4096];
        for piece in bytes.chunks(4096) {
            buf[..piece.len()].copy_from_slice(piece);
            writing.write(&mut part, &mut buf, piece.len()).unwrap();
        }
        writing.finish(&mut part, &mut buf).unwrap();
        let mut data = self.ota_data();
        data.choose(&mut self.data(), target, State::New, 0)
            .unwrap();
        target
    }

    /// The image in `slot`, as long as image number `n` is.
    fn holds(&self, slot: u32, n: u8) -> bool {
        let (at, len) = self.slots.apps[slot as usize];
        let bytes = image(n, len);
        self.flash[at as usize..at as usize + bytes.len()] == bytes[..]
    }

    /// Everything on the flash outside the app partitions and the OTA
    /// data: the table, the NVS, CrossPoint's files.
    fn rest(&self) -> Vec<u8> {
        let mut rest = self.flash.clone();
        for &(at, len) in &self.slots.apps {
            rest[at as usize..(at + len) as usize].fill(0);
        }
        let data = self.slots.data as usize;
        rest[data..data + 0x2000].fill(0);
        rest
    }
}

fn read_slots(flash: &[u8]) -> Slots {
    Slots::find(&flash[TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN]).unwrap()
}

/// Image number `n`: a magic byte, then bytes of its own; a few hundred
/// KB, well within any slot of `room`.
fn image(n: u8, room: u32) -> Vec<u8> {
    let len = (300 * 1024 + usize::from(n) * 4096).min(room as usize);
    let mut bytes: Vec<u8> = (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(n))
        .collect();
    bytes[0] = 0xE9;
    bytes
}

#[test]
fn every_table_shipped_is_tested() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../shells/xteink");
    let known: Vec<&str> = layouts().iter().map(|(name, _)| *name).collect();
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.starts_with("partitions") && name.ends_with(".csv") {
            assert!(known.contains(&name.as_str()), "{name} is not tested here");
        }
    }
}

#[test]
fn every_table_has_two_slots_for_updates() {
    for (name, csv) in layouts() {
        let slots = Slots::find(&table(csv)).unwrap_or_else(|why| panic!("{name}: {why:?}"));
        for running in 0..2 {
            assert_ne!(slots.target(running), running, "{name}");
            // Room for a firmware as large as PaperPaw's grows: 6 MB.
            assert!(slots.room(running) >= 6 * 1024 * 1024, "{name}: room");
        }
    }
}

#[test]
fn updates_go_to_the_other_slot_and_are_kept_after_their_trial() {
    for (name, csv) in layouts() {
        let mut device = Device::flashed(csv);
        let rest = device.rest();
        // After the first flash: the first slot, nothing on trial.
        assert_eq!(device.start(), (0, Start::Kept), "{name}");
        // One update, then a second, each kept after its trial.
        let mut running = 0;
        for n in 2..=3 {
            let target = device.install(running, n);
            assert!(
                device.holds(running, n - 1),
                "{name}: the running one stays"
            );
            assert_eq!(device.start(), (target, Start::Trial { ran: 0 }), "{name}");
            device.keep(target);
            assert_eq!(device.start(), (target, Start::Kept), "{name}");
            assert!(device.holds(target, n), "{name}: update {n} in place");
            running = target;
        }
        assert_eq!(running, 0, "{name}: two updates go back to the first slot");
        assert_eq!(device.rest(), rest, "{name}: nothing else touched");
    }
}

#[test]
fn an_update_that_fails_its_trial_is_undone() {
    for (name, csv) in layouts() {
        let mut device = Device::flashed(csv);
        let rest = device.rest();
        device.start();
        for running in [0, 1] {
            // One kept first, for the second round to start in slot 1.
            if running == 1 {
                let target = device.install(0, 2);
                device.start();
                device.keep(target);
            }
            let before = if running == 0 { 1 } else { 2 };
            let target = device.install(running, 9);
            assert_eq!(device.start(), (target, Start::Trial { ran: 0 }), "{name}");
            // It hangs, the watchdog ends it: found on trial, marked bad.
            assert_eq!(device.start(), (target, Start::Failed), "{name}");
            // The bootloader passes over it: the one before is back.
            assert_eq!(device.start(), (running, Start::Undone), "{name}");
            assert_eq!(device.start(), (running, Start::Kept), "{name}");
            assert!(
                device.holds(running, before),
                "{name}: the one before intact"
            );
            // And the next update still finds the other slot.
            assert_eq!(device.slots.target(running), target, "{name}");
        }
        assert_eq!(device.rest(), rest, "{name}: nothing else touched");
    }
}

#[test]
fn an_update_cut_short_is_never_started() {
    for (name, csv) in layouts() {
        let mut device = Device::flashed(csv);
        device.start();
        let mut data = device.ota_data();
        let target = data.prepare_install(&mut device.data(), 0).unwrap();
        let (at, len) = device.slots.apps[target as usize];
        let mut part = Part {
            flash: &mut device.flash,
            at,
            len,
        };
        // Half an image written, then the power goes.
        let mut writing = Image::new(8192);
        let mut buf = vec![0xE9u8; 4096];
        writing.write(&mut part, &mut buf, 4096).unwrap();
        assert_eq!(device.flash[at as usize], 0xFF, "{name}: no magic byte yet");
        assert_eq!(device.start(), (0, Start::Kept), "{name}");
    }
}
