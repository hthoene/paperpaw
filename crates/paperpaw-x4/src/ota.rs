//! Updates over the air, the parts that can be tested on a computer: the
//! OTA data partition, whose two entries choose the app partition the
//! bootloader starts and keep the state of its image ([`OtaData`]), and an
//! update's image, written so that it can start only once it is whole and
//! its signature held ([`Image`]). [`updates`](crate::updates) checks the
//! signature and decides when; the firmware (`shells/xteink/src/ota.rs`)
//! finds the partitions.
//!
//! The entries are ESP-IDF's: a sequence number, a label the bootloader
//! does not read, the image's state and the CRC of the sequence number.
//! The one with the higher sequence number is in force; its number less
//! one, modulo the app partitions, is the partition it chooses. Choosing
//! writes the other entry whole (sequence number and state at once), so a
//! power cut keeps the choice before; the entry in force stays the way
//! back. The label keeps how long an update ran on trial, so that the
//! trial goes on across the X4's starts from sleep.

/// What these parts need of the flash, in addresses of their own
/// partition.
pub trait Flash {
    /// Reads `buf.len()` bytes at `at`.
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError>;
    /// Programs `data` at `at`, which was erased before (bits only go from
    /// 1 to 0); `at` and the length are multiples of 4.
    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError>;
    /// Erases `from..to` to `0xFF`; both are multiples of a sector.
    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError>;
}

/// The flash could not be read or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashError;

/// The app partitions the OTA data chooses between (`partitions.csv`).
pub const SLOTS: u32 = 2;
/// A flash sector: the least that is erased.
const SECTOR: u32 = 0x1000;
/// Where the two entries are in the OTA data partition: a sector each.
const ENTRIES: [u32; 2] = [0, SECTOR];
/// An entry's length.
const ENTRY_LEN: usize = 32;
/// What unwritten flash holds, as a word.
const UNWRITTEN: u32 = u32::MAX;

/// The state of an image, as ESP-IDF keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Chosen, not started yet (or started and ended cleanly before its
    /// trial was over).
    New,
    /// Started on trial: found so at a start, the trial failed.
    PendingVerify,
    /// It works.
    Valid,
    /// It failed: the bootloader passes over it.
    Invalid,
    /// Given up: passed over too.
    Aborted,
    /// None said, as by a tool that chose it.
    Undefined,
}

impl State {
    fn code(self) -> u32 {
        match self {
            State::New => 0,
            State::PendingVerify => 1,
            State::Valid => 2,
            State::Invalid => 3,
            State::Aborted => 4,
            State::Undefined => UNWRITTEN,
        }
    }

    fn from_code(code: u32) -> Self {
        match code {
            0 => State::New,
            1 => State::PendingVerify,
            2 => State::Valid,
            3 => State::Invalid,
            4 => State::Aborted,
            _ => State::Undefined,
        }
    }
}

/// The CRC of a sequence number, as ESP-IDF's bootloader checks it: CRC-32
/// (reflected, 0xEDB88320) over its four bytes, started from zero and
/// inverted at the end.
fn crc(sequence: u32) -> u32 {
    let mut crc = 0u32;
    for byte in sequence.to_le_bytes() {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// What a start found, and did, as [`OtaData::start`] says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Nothing to do: the firmware running is the one kept.
    Kept,
    /// The update chosen did not start (marked bad at the last start, or
    /// not startable): the firmware running is chosen again, as before.
    Undone,
    /// An update's start, or a start of one on trial whose last run ended
    /// cleanly: now on trial, after `ran` milliseconds run before.
    Trial { ran: u32 },
    /// An update on trial whose last run ended other than cleanly: marked
    /// bad, the device starts again into the firmware before it.
    Failed,
}

/// One of the two entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry([u8; ENTRY_LEN]);

impl Entry {
    fn new(sequence: u32, state: State, trial_ms: u32) -> Self {
        let mut bytes = [0xFF; ENTRY_LEN];
        bytes[..4].copy_from_slice(&sequence.to_le_bytes());
        bytes[28..].copy_from_slice(&crc(sequence).to_le_bytes());
        let mut entry = Self(bytes);
        entry.set(state, trial_ms);
        entry
    }

    fn word(&self, at: usize) -> u32 {
        u32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }

    /// Its sequence number, if it was written and its CRC holds.
    fn sequence(&self) -> Option<u32> {
        let sequence = self.word(0);
        (sequence != UNWRITTEN && self.word(28) == crc(sequence)).then_some(sequence)
    }

    /// The partition it chooses.
    fn slot(&self) -> Option<u32> {
        self.sequence().map(|s| s.wrapping_sub(1) % SLOTS)
    }

    fn state(&self) -> State {
        State::from_code(self.word(24))
    }

    /// How long its image ran on trial, in the label's first word.
    fn trial_ms(&self) -> u32 {
        match self.word(4) {
            UNWRITTEN => 0,
            ms => ms,
        }
    }

    fn set(&mut self, state: State, trial_ms: u32) {
        self.0[24..28].copy_from_slice(&state.code().to_le_bytes());
        let ms = if trial_ms == 0 { UNWRITTEN } else { trial_ms };
        self.0[4..8].copy_from_slice(&ms.to_le_bytes());
    }
}

/// The OTA data partition's two entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaData {
    entries: [Entry; 2],
}

impl OtaData {
    /// Reads the entries of the OTA data partition `flash` is.
    pub fn read(flash: &mut impl Flash) -> Result<Self, FlashError> {
        let mut entries = [Entry([0xFF; ENTRY_LEN]); 2];
        for (entry, at) in entries.iter_mut().zip(ENTRIES) {
            flash.read(at, &mut entry.0)?;
        }
        Ok(Self { entries })
    }

    /// The entry in force: the one with the higher sequence number.
    fn newest(&self) -> Option<usize> {
        (0..2)
            .filter_map(|i| self.entries[i].sequence().map(|s| (s, i)))
            .max()
            .map(|(_, i)| i)
    }

    /// The newest entry choosing `slot`.
    fn newest_of(&self, slot: u32) -> Option<usize> {
        (0..2)
            .filter(|&i| self.entries[i].slot() == Some(slot))
            .filter_map(|i| self.entries[i].sequence().map(|s| (s, i)))
            .max()
            .map(|(_, i)| i)
    }

    /// The partition chosen to start; `None` if none was ever chosen (the
    /// bootloader starts the first).
    pub fn chosen(&self) -> Option<u32> {
        self.newest().and_then(|i| self.entries[i].slot())
    }

    /// The state of `slot`'s image and how long it ran on trial, as its
    /// newest entry says; `None` without one.
    pub fn state_of(&self, slot: u32) -> Option<(State, u32)> {
        self.newest_of(slot)
            .map(|i| (self.entries[i].state(), self.entries[i].trial_ms()))
    }

    /// Chooses `slot` to start, its image in `state` after `trial_ms` on
    /// trial: the entry not in force is written whole, with the next
    /// sequence number that chooses `slot`.
    pub fn choose(
        &mut self,
        flash: &mut impl Flash,
        slot: u32,
        state: State,
        trial_ms: u32,
    ) -> Result<(), FlashError> {
        let newest = self.newest();
        let last = newest.and_then(|i| self.entries[i].sequence()).unwrap_or(0);
        let step = (slot % SLOTS + SLOTS - last % SLOTS) % SLOTS;
        let sequence = last
            .checked_add(1 + step)
            .filter(|&s| s != UNWRITTEN)
            .ok_or(FlashError)?;
        let index = newest.map_or(0, |i| 1 - i);
        self.write(flash, index, Entry::new(sequence, state, trial_ms))
    }

    /// Sets the state of `slot`'s image and how long it ran on trial, in
    /// the newest entry choosing it, where it is (in force or not).
    pub fn set_state(
        &mut self,
        flash: &mut impl Flash,
        slot: u32,
        state: State,
        trial_ms: u32,
    ) -> Result<(), FlashError> {
        let index = self.newest_of(slot).ok_or(FlashError)?;
        let mut entry = self.entries[index];
        entry.set(state, trial_ms);
        self.write(flash, index, entry)
    }

    /// Readies an install beside `running`: an update chosen and not
    /// started yet is given up (`running` chosen again, as it was), since
    /// the install writes over it. The partition to write the update to.
    pub fn prepare_install(
        &mut self,
        flash: &mut impl Flash,
        running: u32,
    ) -> Result<u32, FlashError> {
        let target = (running + 1) % SLOTS;
        if self.chosen() == Some(target) {
            let (state, trial_ms) = self.state_of(running).unwrap_or((State::Valid, 0));
            self.choose(flash, running, state, trial_ms)?;
        }
        Ok(target)
    }

    /// What to do at a start of the firmware in `running`, done: an update
    /// put on trial, a failed one marked bad, or the firmware before chosen
    /// again where the update did not start (see [`Start`]). `None` where
    /// writing failed.
    pub fn start(&mut self, flash: &mut impl Flash, running: u32) -> Option<Start> {
        match (self.chosen(), self.state_of(running)) {
            (Some(chosen), _) if chosen != running => {
                self.choose(flash, running, State::Valid, 0).ok()?;
                Some(Start::Undone)
            }
            // Undefined: chosen by a tool.
            (Some(_), Some((State::New | State::Undefined, ran))) => {
                self.set_state(flash, running, State::PendingVerify, ran)
                    .ok()?;
                Some(Start::Trial { ran })
            }
            (Some(_), Some((State::PendingVerify, _))) => {
                self.set_state(flash, running, State::Invalid, 0).ok()?;
                Some(Start::Failed)
            }
            _ => Some(Start::Kept),
        }
    }

    /// The slot ESP-IDF's bootloader starts: the one the newest entry not
    /// marked bad chooses; the first where none does.
    pub fn booted(&self) -> u32 {
        (0..2)
            .filter(|&i| !matches!(self.entries[i].state(), State::Invalid | State::Aborted))
            .filter_map(|i| self.entries[i].sequence().map(|s| (s, i)))
            .max()
            .and_then(|(_, i)| self.entries[i].slot())
            .unwrap_or(0)
    }

    fn write(
        &mut self,
        flash: &mut impl Flash,
        index: usize,
        entry: Entry,
    ) -> Result<(), FlashError> {
        let at = ENTRIES[index];
        flash.erase(at, at + SECTOR)?;
        flash.program(at, &entry.0)?;
        self.entries[index] = entry;
        Ok(())
    }
}

/// The flash's erase block, as an image is written.
pub const BLOCK: u32 = 64 * 1024;
/// An image's first word: its magic byte, which makes it an image to the
/// bootloader, is in it.
pub const HEAD: usize = 4;

/// An update's image, written into an app partition a piece at a time.
/// Its first word is kept back (the flash left erased there) until
/// [`Image::finish`]: a partition holding an image cut short, or one
/// whose signature failed, holds nothing the bootloader would start.
#[derive(Debug, Clone)]
pub struct Image {
    total: u32,
    done: u32,
    head: [u8; HEAD],
}

impl Image {
    /// An image of `total` bytes, none written yet.
    pub fn new(total: u32) -> Self {
        Self {
            total,
            done: 0,
            head: [0xFF; HEAD],
        }
    }

    /// The image's length.
    pub fn total(&self) -> u32 {
        self.total
    }

    /// Bytes written.
    pub fn done(&self) -> u32 {
        self.done
    }

    /// Writes the image's next `len` bytes, at the start of `buf` (which
    /// has room for them rounded up to a word), erasing each block as it
    /// is reached. `buf` is changed: the first word kept back, the end
    /// filled up as erased flash.
    pub fn write(
        &mut self,
        flash: &mut impl Flash,
        buf: &mut [u8],
        len: usize,
    ) -> Result<(), FlashError> {
        let words = len.div_ceil(4) * 4;
        if words > buf.len() || self.done.saturating_add(len as u32) > self.total {
            return Err(FlashError);
        }
        let at = self.done;
        let end = at + len as u32;
        // Every block this piece reaches into, erased as it is first met.
        let mut block = at.next_multiple_of(BLOCK);
        while block < end {
            flash.erase(block, block + BLOCK)?;
            block += BLOCK;
        }
        if at == 0 {
            let head = HEAD.min(len);
            self.head[..head].copy_from_slice(&buf[..head]);
            buf[..head].fill(0xFF);
        }
        buf[len..words].fill(0xFF);
        flash.program(at, &buf[..words])?;
        self.done = end;
        Ok(())
    }

    /// Writes the first word, kept back: the image is whole. From `buf`,
    /// which the flash writes from (aligned to words).
    pub fn finish(&self, flash: &mut impl Flash, buf: &mut [u8]) -> Result<(), FlashError> {
        if self.done != self.total || buf.len() < HEAD {
            return Err(FlashError);
        }
        buf[..HEAD].copy_from_slice(&self.head);
        flash.program(0, &buf[..HEAD])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// NOR flash in memory: programming only clears bits; it can be made
    /// to fail from some operation on, as a power cut would.
    struct Nor {
        bytes: Vec<u8>,
        /// Operations left before it fails.
        left: usize,
    }

    impl Nor {
        fn new(len: usize) -> Self {
            Self {
                bytes: vec![0xFF; len],
                left: usize::MAX,
            }
        }

        fn tick(&mut self) -> Result<(), FlashError> {
            self.left = self.left.checked_sub(1).ok_or(FlashError)?;
            Ok(())
        }
    }

    impl Flash for Nor {
        fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
            let at = at as usize;
            buf.copy_from_slice(&self.bytes[at..at + buf.len()]);
            Ok(())
        }

        fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
            self.tick()?;
            assert!(
                at.is_multiple_of(4) && data.len().is_multiple_of(4),
                "words only"
            );
            for (byte, new) in self.bytes[at as usize..].iter_mut().zip(data) {
                *byte &= new;
            }
            Ok(())
        }

        fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
            self.tick()?;
            assert!(
                from.is_multiple_of(SECTOR) && to.is_multiple_of(SECTOR),
                "sectors only"
            );
            self.bytes[from as usize..to as usize].fill(0xFF);
            Ok(())
        }
    }

    fn ota_data() -> Nor {
        Nor::new(2 * SECTOR as usize)
    }

    #[test]
    fn the_crc_is_the_bootloaders() {
        // As an X4's OTA data holds them.
        assert_eq!(crc(1), 0x4743_989A);
        assert_eq!(crc(2), 0x55F6_3774);
    }

    #[test]
    fn choosing_writes_one_entry_whole_and_keeps_the_way_back() {
        let mut flash = ota_data();
        let mut data = OtaData::read(&mut flash).unwrap();
        assert_eq!(data.chosen(), None);
        data.choose(&mut flash, 0, State::Valid, 0).unwrap();
        data.choose(&mut flash, 1, State::New, 0).unwrap();
        let read = OtaData::read(&mut flash).unwrap();
        assert_eq!(read, data);
        assert_eq!(read.chosen(), Some(1));
        assert_eq!(read.state_of(1), Some((State::New, 0)));
        // The firmware before stays in the other entry.
        assert_eq!(read.state_of(0), Some((State::Valid, 0)));
        // Again for the other: the sequence goes on.
        data.choose(&mut flash, 0, State::New, 0).unwrap();
        assert_eq!(OtaData::read(&mut flash).unwrap().chosen(), Some(0));
    }

    #[test]
    fn a_power_cut_while_choosing_keeps_the_choice_before() {
        for left in 0..2 {
            let mut flash = ota_data();
            let mut data = OtaData::read(&mut flash).unwrap();
            data.choose(&mut flash, 0, State::Valid, 0).unwrap();
            flash.left = left;
            assert!(data.choose(&mut flash, 1, State::New, 0).is_err());
            let read = OtaData::read(&mut flash).unwrap();
            assert_eq!(read.chosen(), Some(0), "cut after {left} steps");
            assert_eq!(read.state_of(0), Some((State::Valid, 0)));
        }
    }

    /// CrossPoint's menu installs into its other slot and writes the entry
    /// not in force as ESP-IDF's bootloader reads it: the next sequence
    /// number for that slot, its label unwritten, state New. PaperPaw
    /// started so finds itself on trial, and the way back is CrossPoint's
    /// entry, untouched.
    #[test]
    fn an_install_by_crosspoints_menu_is_on_trial() {
        let mut flash = ota_data();
        // CrossPoint runs from its first slot.
        let mut data = OtaData::read(&mut flash).unwrap();
        data.choose(&mut flash, 0, State::Valid, 0).unwrap();
        // Its `ota_boot::switchTo`, in the second entry.
        let mut entry = [0xFF; ENTRY_LEN];
        entry[..4].copy_from_slice(&2u32.to_le_bytes());
        entry[24..28].copy_from_slice(&0u32.to_le_bytes());
        entry[28..].copy_from_slice(&crc(2).to_le_bytes());
        flash.bytes[SECTOR as usize..][..ENTRY_LEN].copy_from_slice(&entry);
        let mut read = OtaData::read(&mut flash).unwrap();
        assert_eq!(read.chosen(), Some(1));
        assert_eq!(read.state_of(1), Some((State::New, 0)));
        // As the firmware's start puts it on trial, and gives up.
        read.set_state(&mut flash, 1, State::PendingVerify, 0)
            .unwrap();
        read.set_state(&mut flash, 1, State::Invalid, 0).unwrap();
        let read = OtaData::read(&mut flash).unwrap();
        assert_eq!(read.state_of(1), Some((State::Invalid, 0)));
        assert_eq!(read.state_of(0), Some((State::Valid, 0)));
    }

    #[test]
    fn a_pending_update_is_given_up_before_an_install() {
        let mut flash = ota_data();
        let mut data = OtaData::read(&mut flash).unwrap();
        data.choose(&mut flash, 0, State::Valid, 0).unwrap();
        // Nothing pending: the other partition, nothing written.
        let before = flash.bytes.clone();
        assert_eq!(data.prepare_install(&mut flash, 0), Ok(1));
        assert_eq!(flash.bytes, before);
        // An update installed into partition 1, not started yet.
        data.choose(&mut flash, 1, State::New, 0).unwrap();
        assert_eq!(data.prepare_install(&mut flash, 0), Ok(1));
        let read = OtaData::read(&mut flash).unwrap();
        assert_eq!(read.chosen(), Some(0));
        assert_eq!(read.state_of(0), Some((State::Valid, 0)));
    }

    #[test]
    fn the_running_partition_is_confirmed_even_with_an_update_chosen() {
        let mut flash = ota_data();
        let mut data = OtaData::read(&mut flash).unwrap();
        // Partition 0 on trial, then an update into 1 chosen meanwhile.
        data.choose(&mut flash, 0, State::PendingVerify, 0).unwrap();
        data.choose(&mut flash, 1, State::New, 0).unwrap();
        data.set_state(&mut flash, 0, State::Valid, 0).unwrap();
        let read = OtaData::read(&mut flash).unwrap();
        assert_eq!(read.state_of(0), Some((State::Valid, 0)));
        assert_eq!(read.chosen(), Some(1));
        assert_eq!(read.state_of(1), Some((State::New, 0)));
    }

    #[test]
    fn the_time_on_trial_is_kept_across_starts() {
        let mut flash = ota_data();
        let mut data = OtaData::read(&mut flash).unwrap();
        data.choose(&mut flash, 0, State::Valid, 0).unwrap();
        data.choose(&mut flash, 1, State::New, 0).unwrap();
        // Started, then asleep after 12 s: new again, with its time.
        data.set_state(&mut flash, 1, State::PendingVerify, 0)
            .unwrap();
        data.set_state(&mut flash, 1, State::New, 12_000).unwrap();
        let read = OtaData::read(&mut flash).unwrap();
        assert_eq!(read.state_of(1), Some((State::New, 12_000)));
        assert_eq!(read.chosen(), Some(1));
        // Confirmed: the time goes.
        let mut data = read;
        data.set_state(&mut flash, 1, State::Valid, 0).unwrap();
        assert_eq!(
            OtaData::read(&mut flash).unwrap().state_of(1),
            Some((State::Valid, 0))
        );
    }

    #[test]
    fn entries_not_written_or_damaged_choose_nothing() {
        let mut flash = ota_data();
        let mut data = OtaData::read(&mut flash).unwrap();
        data.choose(&mut flash, 1, State::Valid, 0).unwrap();
        // The CRC broken: as if never written.
        flash.bytes[28] ^= 1;
        let read = OtaData::read(&mut flash).unwrap();
        assert_eq!(read.chosen(), None);
        assert_eq!(read.state_of(1), None);
    }

    /// An image of `len` bytes that is not erased flash anywhere.
    fn image(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    /// Writes `bytes` as an image in pieces of `piece`, into flash that
    /// held something before; finishes it if asked.
    fn written(bytes: &[u8], piece: usize, finish: bool) -> Nor {
        let mut flash = Nor::new(3 * BLOCK as usize);
        flash.bytes.fill(0x5A);
        let mut image = Image::new(bytes.len() as u32);
        let mut buf = vec![0u8; piece];
        for chunk in bytes.chunks(piece) {
            buf[..chunk.len()].copy_from_slice(chunk);
            image.write(&mut flash, &mut buf, chunk.len()).unwrap();
        }
        assert_eq!(image.done(), image.total());
        if finish {
            image.finish(&mut flash, &mut buf).unwrap();
        }
        flash
    }

    #[test]
    fn the_first_word_is_written_last() {
        let bytes = image(BLOCK as usize + 5000);
        let flash = written(&bytes, 4096, false);
        // Whole but for its first word: no image to the bootloader.
        assert_eq!(flash.bytes[..HEAD], [0xFF; HEAD]);
        assert_eq!(flash.bytes[HEAD..bytes.len()], bytes[HEAD..]);
        let flash = written(&bytes, 4096, true);
        assert_eq!(flash.bytes[..bytes.len()], bytes[..]);
        // Pieces that do not meet the blocks' bounds erase them too.
        let flash = written(&bytes, 3000, true);
        assert_eq!(flash.bytes[..bytes.len()], bytes[..]);
    }

    #[test]
    fn an_image_not_whole_cannot_be_finished() {
        let mut flash = Nor::new(BLOCK as usize);
        let mut image = Image::new(8000);
        let mut buf = image_buf();
        image.write(&mut flash, &mut buf, 4096).unwrap();
        assert_eq!(image.finish(&mut flash, &mut buf), Err(FlashError));
        assert_eq!(flash.bytes[..HEAD], [0xFF; HEAD]);
        // Nor past its end.
        let mut image = Image::new(100);
        assert_eq!(image.write(&mut flash, &mut buf, 200), Err(FlashError));
    }

    fn image_buf() -> Vec<u8> {
        image(4096)
    }
}
