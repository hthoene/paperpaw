//! The card shown to a computer as a USB drive (#32, the ESP32-S3 devices):
//! what the firmware's `usb_drive` module does on plain numbers and bytes,
//! so that a computer tests it.
//!
//! - [`Msc`]: USB mass storage, Bulk-Only Transport with the SCSI
//!   transparent command set, the way every operating system's driver
//!   talks to a card reader: a command block comes (31 bytes), data moves
//!   one way, a status (13 bytes) goes back. It reads and writes the card
//!   through [`Disk`] and moves no packet itself: the firmware's USB
//!   driver hands it what came and sends what it gives.
//! - [`Session`]: how long the mode lasts (the computer ejects the drive,
//!   the reader presses Back, the cable goes, nobody ever came).

/// The size of a block, as the card and the computer count.
pub const BLOCK: usize = 512;
/// The command block's length.
pub const CBW_LEN: usize = 31;
/// The status block's length.
pub const CSW_LEN: usize = 13;

const CBW_SIGNATURE: u32 = 0x4342_5355;
const CSW_SIGNATURE: u32 = 0x5342_5355;

const PASSED: u8 = 0;
const FAILED: u8 = 1;
const PHASE_ERROR: u8 = 2;

/// What the drive needs of the card: its blocks, whole.
pub trait Disk {
    /// How many blocks the card has; `None` where it cannot be asked.
    fn blocks(&mut self) -> Option<u32>;
    /// Reads whole blocks from `lba` on into `data`; `false` where it failed.
    fn read(&mut self, lba: u32, data: &mut [u8]) -> bool;
    /// Writes `data` (whole blocks) from `lba` on; `false` where it failed.
    fn write(&mut self, lba: u32, data: &[u8]) -> bool;
}

/// What a command block asks for next ([`Msc::begin`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begin {
    /// Not a command block (wrong length or signature, a command block
    /// longer than 16 bytes, a unit that is not there): the endpoints are
    /// stalled until the computer resets the interface.
    Invalid,
    /// No data moves: the status goes back ([`Msc::csw`]), the data
    /// endpoint stalled first where the computer expected data ([`Msc::stall`]).
    Done,
    /// `len` bytes go to the computer ([`Msc::data_in`]), then the status.
    In(u32),
    /// `len` bytes come from the computer ([`Msc::data_out`]), then the
    /// status.
    Out(u32),
}

/// Which endpoint to stall before the status goes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stall {
    /// None.
    No,
    /// The one to the computer: it expected more data than there is.
    In,
    /// The one from the computer: it sends more data than is taken.
    Out,
}

/// Where the data stage of the command under way is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    /// The short answer in [`Msc::short`], from byte `at` on.
    Short(u8),
    Read,
    Write,
}

/// The longest short answer (INQUIRY's).
const SHORT: usize = 36;

/// A mass-storage unit on the card.
#[derive(Debug)]
pub struct Msc {
    tag: u32,
    /// What the computer expects to move, and which way.
    host_len: u32,
    host_in: bool,
    /// What did move.
    moved: u32,
    status: u8,
    stage: Stage,
    /// The sense of the last failure: key, additional code and qualifier;
    /// asked for by REQUEST SENSE, then gone.
    sense: [u8; 3],
    /// The next block to move, and how many are left.
    lba: u32,
    left: u32,
    short: [u8; SHORT],
    short_len: u8,
    /// The computer ejected the drive (START STOP UNIT).
    ejected: bool,
}

impl Default for Msc {
    fn default() -> Self {
        Self::new()
    }
}

impl Msc {
    pub const fn new() -> Self {
        Self {
            tag: 0,
            host_len: 0,
            host_in: false,
            moved: 0,
            status: PASSED,
            stage: Stage::Idle,
            sense: [0; 3],
            lba: 0,
            left: 0,
            short: [0; SHORT],
            short_len: 0,
            ejected: false,
        }
    }

    /// The computer ejected the drive: the mode ends once the status of
    /// that command went back.
    pub fn ejected(&self) -> bool {
        self.ejected
    }

    /// The computer reset the interface (the class request): what was
    /// under way is dropped.
    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.status = PASSED;
        self.sense = [0; 3];
    }

    /// The command block `cbw` came: what moves next. The card is asked
    /// only where a command needs it.
    pub fn begin(&mut self, disk: &mut impl Disk, cbw: &[u8]) -> Begin {
        let cb_len = usize::from(cbw.get(14).copied().unwrap_or(0) & 0x1f);
        let signature = cbw
            .get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        if cbw.len() != CBW_LEN
            || signature != Some(CBW_SIGNATURE)
            || cbw[13] & 0x0f != 0
            || !(1..=16).contains(&cb_len)
        {
            return Begin::Invalid;
        }
        self.tag = u32::from_le_bytes([cbw[4], cbw[5], cbw[6], cbw[7]]);
        self.host_len = u32::from_le_bytes([cbw[8], cbw[9], cbw[10], cbw[11]]);
        self.host_in = cbw[12] & 0x80 != 0;
        self.moved = 0;
        self.status = PASSED;
        self.stage = Stage::Idle;
        let mut cb = [0u8; 16];
        cb[..cb_len].copy_from_slice(&cbw[15..15 + cb_len]);
        let wants = self.command(disk, &cb);
        self.agree(wants)
    }

    /// What the command moves (`Some((to the computer, bytes))`) set up;
    /// `None` where it moves nothing, or failed.
    fn command(&mut self, disk: &mut impl Disk, cb: &[u8; 16]) -> Option<(bool, u32)> {
        match cb[0] {
            // TEST UNIT READY
            0x00 => {
                self.media(disk);
                None
            }
            // REQUEST SENSE
            0x03 => {
                let length = usize::from(cb[4]);
                let sense = core::mem::take(&mut self.sense);
                let mut data = [0u8; 18];
                data[0] = 0x70;
                data[2] = sense[0];
                data[7] = 10;
                data[12] = sense[1];
                data[13] = sense[2];
                self.say(&data[..length.min(18)])
            }
            // INQUIRY
            0x12 => {
                if cb[1] & 1 != 0 {
                    return self.fail(5, 0x24, 0);
                }
                let mut data = [0u8; SHORT];
                // A direct-access block device, removable, SPC-2, a
                // response format of 2, 31 more bytes.
                data[1] = 0x80;
                data[2] = 4;
                data[3] = 2;
                data[4] = 31;
                data[8..16].copy_from_slice(b"PaperPaw");
                data[16..32].copy_from_slice(b"SD card         ");
                data[32..36].copy_from_slice(b"1.00");
                let length = usize::from(u16::from_be_bytes([cb[3], cb[4]]));
                self.say(&data[..length.min(SHORT)])
            }
            // MODE SENSE (6): not write protected, no pages.
            0x1a => self.say(&[3, 0, 0, 0]),
            // MODE SENSE (10)
            0x5a => self.say(&[0, 6, 0, 0, 0, 0, 0, 0]),
            // READ FORMAT CAPACITIES: one formatted medium.
            0x23 => {
                let blocks = self.media(disk)?;
                let mut data = [0u8; 12];
                data[3] = 8;
                data[4..8].copy_from_slice(&blocks.to_be_bytes());
                data[8] = 2;
                data[10..12].copy_from_slice(&(BLOCK as u16).to_be_bytes());
                self.say(&data)
            }
            // READ CAPACITY (10): the last block, the block's size.
            0x25 => {
                let blocks = self.media(disk)?;
                let mut data = [0u8; 8];
                data[..4].copy_from_slice(&blocks.saturating_sub(1).to_be_bytes());
                data[4..].copy_from_slice(&(BLOCK as u32).to_be_bytes());
                self.say(&data)
            }
            // PREVENT ALLOW MEDIUM REMOVAL, SYNCHRONIZE CACHE (10): the card
            // is written through, there is nothing to hold or flush.
            0x1e | 0x35 => None,
            // START STOP UNIT: load/eject without start is the computer's
            // "eject".
            0x1b => {
                if cb[4] & 0b10 != 0 && cb[4] & 1 == 0 {
                    self.ejected = true;
                }
                None
            }
            // READ (10), WRITE (10)
            op @ (0x28 | 0x2a) => {
                let blocks = self.media(disk)?;
                let lba = u32::from_be_bytes([cb[2], cb[3], cb[4], cb[5]]);
                let count = u32::from(u16::from_be_bytes([cb[7], cb[8]]));
                if u64::from(lba) + u64::from(count) > u64::from(blocks) {
                    return self.fail(5, 0x21, 0);
                }
                self.lba = lba;
                self.left = count;
                if count == 0 {
                    return None;
                }
                let read = op == 0x28;
                self.stage = if read { Stage::Read } else { Stage::Write };
                Some((read, count * BLOCK as u32))
            }
            _ => self.fail(5, 0x20, 0),
        }
    }

    /// The card's size where the medium is there; a failure said else.
    fn media(&mut self, disk: &mut impl Disk) -> Option<u32> {
        if self.ejected {
            self.fail::<()>(2, 0x3a, 0);
            return None;
        }
        let blocks = disk.blocks().filter(|blocks| *blocks > 0);
        if blocks.is_none() {
            self.fail::<()>(2, 0x3a, 0);
        }
        blocks
    }

    /// Fails the command with a sense: nothing moves.
    fn fail<T>(&mut self, key: u8, code: u8, qualifier: u8) -> Option<T> {
        self.status = FAILED;
        self.sense = [key, code, qualifier];
        self.stage = Stage::Idle;
        None
    }

    /// A short answer to the computer, at most the bytes asked for.
    fn say(&mut self, data: &[u8]) -> Option<(bool, u32)> {
        self.short[..data.len()].copy_from_slice(data);
        self.short_len = data.len() as u8;
        self.stage = Stage::Short(0);
        Some((true, data.len() as u32))
    }

    /// What the computer expects against what the command moves (BOT's
    /// thirteen cases): the data stage, or the status alone.
    fn agree(&mut self, wants: Option<(bool, u32)>) -> Begin {
        match wants {
            // Data the computer expected is stalled for ([`Msc::stall`]).
            None => Begin::Done,
            Some((to_host, len)) => {
                if self.host_len == 0 || to_host != self.host_in || self.host_len < len {
                    self.status = PHASE_ERROR;
                    self.stage = Stage::Idle;
                    return Begin::Done;
                }
                if to_host {
                    Begin::In(len)
                } else {
                    Begin::Out(len)
                }
            }
        }
    }

    /// Which endpoint to stall before the status: where the computer
    /// expected more data than moved ([`Begin::Done`] or after the data).
    /// (embassy's classes cannot stall: the firmware ends the stage a stall
    /// would, with a short packet, or reads and drops [`Msc::unread`].)
    pub fn stall(&self) -> Stall {
        match (self.host_len > self.moved, self.host_in) {
            (false, _) => Stall::No,
            (true, true) => Stall::In,
            (true, false) => Stall::Out,
        }
    }

    /// How many bytes the computer still sends that nothing took (it
    /// expected to move more than the command does).
    pub fn unread(&self) -> u32 {
        self.host_len.saturating_sub(self.moved)
    }

    /// The next bytes to the computer, into `out` (room for at least a
    /// block); empty once all moved. `None`: the card failed to read, stall
    /// the endpoint and send the status.
    pub fn data_in(&mut self, disk: &mut impl Disk, out: &mut [u8]) -> Option<usize> {
        match self.stage {
            Stage::Short(at) => {
                let rest = &self.short[usize::from(at)..usize::from(self.short_len)];
                let n = rest.len().min(out.len());
                out[..n].copy_from_slice(&rest[..n]);
                self.moved += n as u32;
                self.stage = if n == rest.len() {
                    Stage::Idle
                } else {
                    Stage::Short(at + n as u8)
                };
                Some(n)
            }
            Stage::Read => {
                let n = self.left.min((out.len() / BLOCK) as u32);
                if n == 0 || !disk.read(self.lba, &mut out[..n as usize * BLOCK]) {
                    self.fail::<()>(3, 0x11, 0);
                    return None;
                }
                self.lba += n;
                self.left -= n;
                self.moved += n * BLOCK as u32;
                if self.left == 0 {
                    self.stage = Stage::Idle;
                }
                Some(n as usize * BLOCK)
            }
            _ => Some(0),
        }
    }

    /// Whole blocks from the computer: written to the card. A write that
    /// fails fails the command; what still comes is taken and dropped.
    pub fn data_out(&mut self, disk: &mut impl Disk, chunk: &[u8]) {
        if self.stage != Stage::Write {
            return;
        }
        let n = self.left.min((chunk.len() / BLOCK) as u32);
        if self.status == PASSED && !disk.write(self.lba, &chunk[..n as usize * BLOCK]) {
            self.status = FAILED;
            self.sense = [3, 0x0c, 0];
        }
        self.lba += n;
        self.left -= n;
        self.moved += n * BLOCK as u32;
        if self.left == 0 {
            self.stage = Stage::Idle;
        }
    }

    /// The status block to send once the data stage is over.
    pub fn csw(&self) -> [u8; CSW_LEN] {
        let mut csw = [0u8; CSW_LEN];
        csw[..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&self.tag.to_le_bytes());
        csw[8..12].copy_from_slice(&self.host_len.saturating_sub(self.moved).to_le_bytes());
        csw[12] = self.status;
        csw
    }
}

/// How long the bus may be down (no computer talking: the cable pulled, the
/// computer off) after it was up before the mode ends (ms).
pub const BUS_DOWN_MS: u64 = 4_000;
/// How long the mode waits for a computer that never came (ms).
pub const NO_COMPUTER_MS: u64 = 60_000;

/// Why the mode ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The computer ejected the drive.
    Ejected,
    /// The reader pressed Back (or the power button).
    Back,
    /// The computer's bus went away: the cable pulled, the computer off.
    BusGone,
    /// No computer came.
    NoComputer,
    /// The USB driver failed to start or stopped.
    Failed,
}

/// The mode's length, decided by what is seen of the bus and the reader.
#[derive(Debug, Clone, Copy)]
pub struct Session {
    started_ms: u64,
    up: bool,
    down_since: Option<u64>,
}

impl Session {
    pub const fn new(now_ms: u64) -> Self {
        Self {
            started_ms: now_ms,
            up: false,
            down_since: None,
        }
    }

    /// Whether to end the mode now, and why. `bus`: a computer has the
    /// device set up and talks to it (not suspended: a pulled cable ends
    /// the bus's frames within milliseconds); `back`: the reader pressed;
    /// `ejected`: [`Msc::ejected`], once its status went back.
    pub fn step(&mut self, now_ms: u64, bus: bool, back: bool, ejected: bool) -> Option<Why> {
        if ejected {
            return Some(Why::Ejected);
        }
        if back {
            return Some(Why::Back);
        }
        if bus {
            self.up = true;
            self.down_since = None;
            return None;
        }
        if !self.up {
            let waited = now_ms.saturating_sub(self.started_ms);
            return (waited >= NO_COMPUTER_MS).then_some(Why::NoComputer);
        }
        let since = *self.down_since.get_or_insert(now_ms);
        (now_ms.saturating_sub(since) >= BUS_DOWN_MS).then_some(Why::BusGone)
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    /// A card in memory.
    struct Ram {
        data: Vec<u8>,
        fail_read: bool,
        fail_write: bool,
        gone: bool,
    }

    impl Ram {
        fn new(blocks: usize) -> Self {
            let mut data = vec![0u8; blocks * BLOCK];
            for (i, byte) in data.iter_mut().enumerate() {
                *byte = (i / BLOCK) as u8 ^ (i % 251) as u8;
            }
            Self {
                data,
                fail_read: false,
                fail_write: false,
                gone: false,
            }
        }
    }

    impl Disk for Ram {
        fn blocks(&mut self) -> Option<u32> {
            (!self.gone).then_some((self.data.len() / BLOCK) as u32)
        }
        fn read(&mut self, lba: u32, data: &mut [u8]) -> bool {
            let at = lba as usize * BLOCK;
            if self.fail_read || at + data.len() > self.data.len() {
                return false;
            }
            data.copy_from_slice(&self.data[at..at + data.len()]);
            true
        }
        fn write(&mut self, lba: u32, data: &[u8]) -> bool {
            let at = lba as usize * BLOCK;
            if self.fail_write || at + data.len() > self.data.len() {
                return false;
            }
            self.data[at..at + data.len()].copy_from_slice(data);
            true
        }
    }

    fn cbw(tag: u32, host_len: u32, to_host: bool, cb: &[u8]) -> [u8; CBW_LEN] {
        let mut block = [0u8; CBW_LEN];
        block[..4].copy_from_slice(&CBW_SIGNATURE.to_le_bytes());
        block[4..8].copy_from_slice(&tag.to_le_bytes());
        block[8..12].copy_from_slice(&host_len.to_le_bytes());
        block[12] = if to_host { 0x80 } else { 0 };
        block[14] = cb.len() as u8;
        block[15..15 + cb.len()].copy_from_slice(cb);
        block
    }

    fn rw(op: u8, lba: u32, count: u16) -> [u8; 10] {
        let mut cb = [0u8; 10];
        cb[0] = op;
        cb[2..6].copy_from_slice(&lba.to_be_bytes());
        cb[7..9].copy_from_slice(&count.to_be_bytes());
        cb
    }

    fn status(msc: &Msc) -> (u32, u32, u8) {
        let csw = msc.csw();
        assert_eq!(&csw[..4], b"USBS");
        (
            u32::from_le_bytes([csw[4], csw[5], csw[6], csw[7]]),
            u32::from_le_bytes([csw[8], csw[9], csw[10], csw[11]]),
            csw[12],
        )
    }

    /// Reads all of a data-in stage in `chunk`-byte pieces.
    fn take(msc: &mut Msc, disk: &mut Ram, chunk: usize) -> Vec<u8> {
        let mut all = Vec::new();
        let mut buf = vec![0u8; chunk];
        loop {
            match msc.data_in(disk, &mut buf) {
                Some(0) => return all,
                Some(n) => all.extend_from_slice(&buf[..n]),
                None => return all,
            }
        }
    }

    #[test]
    fn inquiry_says_a_removable_disk() {
        let mut disk = Ram::new(8);
        let mut msc = Msc::new();
        let begin = msc.begin(&mut disk, &cbw(7, 36, true, &[0x12, 0, 0, 0, 36, 0]));
        assert_eq!(begin, Begin::In(36));
        let data = take(&mut msc, &mut disk, 64);
        assert_eq!((data[0], data[1] & 0x80), (0, 0x80));
        assert_eq!(&data[8..16], b"PaperPaw");
        assert_eq!(status(&msc), (7, 0, PASSED));
    }

    #[test]
    fn the_capacity_is_the_last_block_and_its_size() {
        let mut disk = Ram::new(1000);
        let mut msc = Msc::new();
        let begin = msc.begin(
            &mut disk,
            &cbw(1, 8, true, &[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        );
        assert_eq!(begin, Begin::In(8));
        let data = take(&mut msc, &mut disk, 64);
        assert_eq!(data, [0, 0, 3, 231, 0, 0, 2, 0]);
    }

    #[test]
    fn a_read_moves_the_blocks_in_pieces_of_any_size() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        let begin = msc.begin(&mut disk, &cbw(2, 3 * 512, true, &rw(0x28, 4, 3)));
        assert_eq!(begin, Begin::In(1536));
        // Pieces of two blocks, then one.
        let data = take(&mut msc, &mut disk, 1024);
        assert_eq!(data, disk.data[4 * 512..7 * 512]);
        assert_eq!(status(&msc), (2, 0, PASSED));
        assert_eq!(msc.stall(), Stall::No);
    }

    #[test]
    fn a_write_goes_to_the_card_block_by_block() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        let begin = msc.begin(&mut disk, &cbw(3, 2 * 512, false, &rw(0x2a, 2, 2)));
        assert_eq!(begin, Begin::Out(1024));
        msc.data_out(&mut disk, &[0xAB; 512]);
        msc.data_out(&mut disk, &[0xCD; 512]);
        assert!(disk.data[2 * 512..3 * 512].iter().all(|b| *b == 0xAB));
        assert!(disk.data[3 * 512..4 * 512].iter().all(|b| *b == 0xCD));
        assert_eq!(status(&msc), (3, 0, PASSED));
    }

    #[test]
    fn a_failed_write_fails_the_command_and_takes_the_rest() {
        let mut disk = Ram::new(16);
        disk.fail_write = true;
        let mut msc = Msc::new();
        msc.begin(&mut disk, &cbw(4, 1024, false, &rw(0x2a, 0, 2)));
        msc.data_out(&mut disk, &[1; 512]);
        msc.data_out(&mut disk, &[2; 512]);
        assert_eq!(status(&msc), (4, 0, FAILED));
        // The sense says a write error.
        disk.fail_write = false;
        msc.begin(&mut disk, &cbw(5, 18, true, &[0x03, 0, 0, 0, 18, 0]));
        let sense = take(&mut msc, &mut disk, 64);
        assert_eq!((sense[2], sense[12]), (3, 0x0c));
        // Told once: then it is gone.
        msc.begin(&mut disk, &cbw(6, 18, true, &[0x03, 0, 0, 0, 18, 0]));
        assert_eq!(take(&mut msc, &mut disk, 64)[2], 0);
    }

    #[test]
    fn a_failed_read_stalls_and_leaves_what_did_not_move_as_the_residue() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        msc.begin(&mut disk, &cbw(8, 2 * 512, true, &rw(0x28, 0, 2)));
        disk.fail_read = true;
        let mut buf = [0u8; 1024];
        assert_eq!(msc.data_in(&mut disk, &mut buf), None);
        assert_eq!(status(&msc), (8, 1024, FAILED));
        assert_eq!(msc.stall(), Stall::In);
    }

    #[test]
    fn blocks_past_the_end_are_refused_without_data() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        let begin = msc.begin(&mut disk, &cbw(9, 512, true, &rw(0x28, 16, 1)));
        assert_eq!(begin, Begin::Done);
        assert_eq!(status(&msc), (9, 512, FAILED));
        assert_eq!(msc.stall(), Stall::In);
        let begin = msc.begin(&mut disk, &cbw(10, 512, true, &rw(0x28, 15, 1)));
        assert_eq!(begin, Begin::In(512));
    }

    #[test]
    fn a_card_that_is_gone_is_a_medium_not_present() {
        let mut disk = Ram::new(16);
        disk.gone = true;
        let mut msc = Msc::new();
        let begin = msc.begin(&mut disk, &cbw(1, 0, false, &[0; 6]));
        assert_eq!(begin, Begin::Done);
        assert_eq!(status(&msc).2, FAILED);
        msc.begin(&mut disk, &cbw(2, 18, true, &[0x03, 0, 0, 0, 18, 0]));
        let sense = take(&mut msc, &mut disk, 64);
        assert_eq!((sense[2], sense[12]), (2, 0x3a));
    }

    #[test]
    fn a_command_that_is_not_known_fails_with_an_illegal_request() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        assert_eq!(
            msc.begin(&mut disk, &cbw(1, 0, false, &[0xEE, 0, 0, 0, 0, 0])),
            Begin::Done
        );
        assert_eq!(status(&msc).2, FAILED);
        msc.begin(&mut disk, &cbw(2, 18, true, &[0x03, 0, 0, 0, 18, 0]));
        assert_eq!(take(&mut msc, &mut disk, 64)[2], 5);
    }

    #[test]
    fn an_eject_is_seen_and_the_unit_is_then_empty() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        // Load/eject alone; start with it is a load, not an eject.
        msc.begin(&mut disk, &cbw(1, 0, false, &[0x1b, 0, 0, 0, 0b11, 0]));
        assert!(!msc.ejected());
        msc.begin(&mut disk, &cbw(2, 0, false, &[0x1b, 0, 0, 0, 0b10, 0]));
        assert!(msc.ejected());
        assert_eq!(status(&msc), (2, 0, PASSED));
        msc.begin(&mut disk, &cbw(3, 0, false, &[0; 6]));
        assert_eq!(status(&msc).2, FAILED);
    }

    #[test]
    fn what_is_not_a_command_block_is_invalid() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        let mut block = cbw(1, 0, false, &[0; 6]);
        assert_eq!(msc.begin(&mut disk, &block[..30]), Begin::Invalid);
        block[0] = 0;
        assert_eq!(msc.begin(&mut disk, &block), Begin::Invalid);
        let mut block = cbw(1, 0, false, &[0; 6]);
        block[13] = 1;
        assert_eq!(msc.begin(&mut disk, &block), Begin::Invalid);
        let mut block = cbw(1, 0, false, &[0; 6]);
        block[14] = 17;
        assert_eq!(msc.begin(&mut disk, &block), Begin::Invalid);
    }

    #[test]
    fn a_computer_that_expects_another_amount_or_way_gets_a_phase_error() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        // Less than the command moves.
        assert_eq!(
            msc.begin(&mut disk, &cbw(1, 512, true, &rw(0x28, 0, 2))),
            Begin::Done
        );
        assert_eq!(status(&msc).2, PHASE_ERROR);
        // The other way.
        assert_eq!(
            msc.begin(&mut disk, &cbw(2, 512, false, &rw(0x28, 0, 1))),
            Begin::Done
        );
        assert_eq!(status(&msc).2, PHASE_ERROR);
        // More than a short answer: the rest is the residue, the endpoint stalled.
        assert_eq!(
            msc.begin(&mut disk, &cbw(3, 255, true, &[0x1a, 0, 0, 0, 255, 0])),
            Begin::In(4)
        );
        take(&mut msc, &mut disk, 64);
        assert_eq!(status(&msc), (3, 251, PASSED));
        assert_eq!(msc.stall(), Stall::In);
    }

    #[test]
    fn a_reset_drops_what_was_under_way() {
        let mut disk = Ram::new(16);
        let mut msc = Msc::new();
        msc.begin(&mut disk, &cbw(1, 1024, true, &rw(0x28, 0, 2)));
        msc.reset();
        let mut buf = [0u8; 1024];
        assert_eq!(msc.data_in(&mut disk, &mut buf), Some(0));
    }

    #[test]
    fn the_session_ends_by_what_comes_first() {
        // Ejected, and Back, win over the bus.
        let mut s = Session::new(0);
        assert_eq!(s.step(10, true, false, true), Some(Why::Ejected));
        let mut s = Session::new(0);
        assert_eq!(s.step(10, true, true, false), Some(Why::Back));
        // A bus that was up and is down for BUS_DOWN_MS ends it; one that
        // comes back in time does not.
        let mut s = Session::new(0);
        assert_eq!(s.step(100, true, false, false), None);
        assert_eq!(s.step(200, false, false, false), None);
        assert_eq!(s.step(200 + BUS_DOWN_MS - 1, false, false, false), None);
        assert_eq!(s.step(300 + BUS_DOWN_MS, true, false, false), None);
        assert_eq!(s.step(400 + BUS_DOWN_MS, false, false, false), None);
        assert_eq!(
            s.step(400 + 2 * BUS_DOWN_MS, false, false, false),
            Some(Why::BusGone)
        );
    }

    #[test]
    fn a_computer_that_never_comes_is_waited_for_a_minute() {
        let mut s = Session::new(1_000);
        assert_eq!(
            s.step(1_000 + NO_COMPUTER_MS - 1, false, false, false),
            None
        );
        assert_eq!(
            s.step(1_000 + NO_COMPUTER_MS, false, false, false),
            Some(Why::NoComputer)
        );
    }
}
