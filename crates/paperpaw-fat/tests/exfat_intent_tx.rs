//! Publication/replay ordering on a flush-aware model, not real-card qualification.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "transaction assertions"
)]
use hadris_fat::exfat::intent::{self, Change, Identity, Operation, RecordError};
use hadris_fat::exfat::intent_tx::{self, IntentIo, TransactionError};

#[derive(Clone)]
struct Image {
    slots: Vec<Vec<u8>>,
    entries: [[u8; 32]; 2],
}
#[derive(Clone)]
enum Write {
    Slot(usize, usize, Vec<u8>),
    Entry(usize, [u8; 32]),
}
#[derive(Clone)]
struct Disk {
    stable: Image,
    live: Image,
    pending: Vec<Write>,
    calls: usize,
    cut: usize,
}
impl Disk {
    fn new() -> Self {
        let stable = Image {
            slots: vec![vec![0; intent::RECORD_BYTES]; 2],
            entries: [[1; 32], [2; 32]],
        };
        Self {
            live: stable.clone(),
            stable,
            pending: Vec::new(),
            calls: 0,
            cut: usize::MAX,
        }
    }
    fn step(&mut self) -> Result<(), ()> {
        let index = self.calls;
        self.calls += 1;
        if index == self.cut { Err(()) } else { Ok(()) }
    }
    fn crash(&mut self) {
        self.live = self.stable.clone();
        self.pending.clear();
        self.calls = 0;
        self.cut = usize::MAX;
    }
}
fn identity() -> Identity {
    Identity {
        serial: 1,
        volume_bytes: 8 << 20,
        heap_start: 1 << 20,
        heap_end: 8 << 20,
        journal_start: 2 << 20,
        journal_len: 8192,
        epoch: 3,
    }
}
fn entry_index(offset: u64) -> Result<usize, ()> {
    match offset {
        1_048_576 => Ok(0),
        1_048_608 => Ok(1),
        _ => Err(()),
    }
}
impl IntentIo for Disk {
    type Error = ();
    fn read_slot(&mut self, slot: usize, out: &mut [u8]) -> Result<(), ()> {
        self.step()?;
        out.copy_from_slice(&self.live.slots[slot]);
        Ok(())
    }
    fn write_slot(&mut self, slot: usize, offset: usize, data: &[u8]) -> Result<(), ()> {
        self.step()?;
        self.live.slots[slot][offset..offset + data.len()].copy_from_slice(data);
        for (i, block) in data.chunks(512).enumerate() {
            self.pending
                .push(Write::Slot(slot, offset + i * 512, block.to_vec()));
        }
        Ok(())
    }
    fn validate_target(&mut self, change: &Change<'_>) -> Result<(), ()> {
        self.step()?;
        entry_index(change.offset).map(|_| ())
    }
    fn read_entry(&mut self, offset: u64, out: &mut [u8; 32]) -> Result<(), ()> {
        self.step()?;
        *out = self.live.entries[entry_index(offset)?];
        Ok(())
    }
    fn write_entry(&mut self, offset: u64, data: &[u8; 32]) -> Result<(), ()> {
        self.step()?;
        let index = entry_index(offset)?;
        self.live.entries[index] = *data;
        self.pending.push(Write::Entry(index, *data));
        Ok(())
    }
    fn flush(&mut self) -> Result<(), ()> {
        self.step()?;
        // A cut may land between any two buffered media writes.
        for write in self.pending.clone() {
            self.step()?;
            match write {
                Write::Slot(slot, offset, data) => {
                    self.stable.slots[slot][offset..offset + data.len()].copy_from_slice(&data)
                }
                Write::Entry(index, data) => self.stable.entries[index] = data,
            }
        }
        self.pending.clear();
        Ok(())
    }
}
fn update(disk: &mut Disk, values: [[u8; 32]; 2]) -> Result<(), TransactionError<()>> {
    let old = disk.live.entries;
    let mut work = vec![0; intent::RECORD_BYTES];
    intent_tx::execute(
        disk,
        identity(),
        Operation::Update,
        &[
            Change {
                offset: 1_048_576,
                before: &old[0],
                after: &values[0],
            },
            Change {
                offset: 1_048_608,
                before: &old[1],
                after: &values[1],
            },
        ],
        &mut work,
    )
}
fn recover(disk: &mut Disk) -> Result<(), TransactionError<()>> {
    intent_tx::recover(disk, identity(), &mut vec![0; intent::RECORD_BYTES])
}

#[test]
fn every_cut_in_publication_replay_retirement_and_reuse_is_recoverable() {
    let mut baseline = Disk::new();
    for round in 0..4 {
        let before = baseline.live.entries;
        let after = [[10 + round; 32], [20 + round; 32]];
        let mut complete = baseline.clone();
        complete.calls = 0;
        update(&mut complete, after).unwrap();
        let calls = complete.calls;
        for cut in 0..calls {
            let mut interrupted = baseline.clone();
            interrupted.calls = 0;
            interrupted.cut = cut;
            assert!(
                update(&mut interrupted, after).is_err(),
                "round {round}, cut {cut}"
            );
            interrupted.crash();
            let mut counted = interrupted.clone();
            recover(&mut counted).unwrap();
            for replay_cut in 0..counted.calls {
                let mut replay = interrupted.clone();
                replay.cut = replay_cut;
                assert!(recover(&mut replay).is_err());
                replay.crash();
                recover(&mut replay).unwrap();
                assert!(replay.live.entries == before || replay.live.entries == after);
                let saved = replay.live.entries;
                recover(&mut replay).unwrap();
                assert_eq!(replay.live.entries, saved, "repeated replay");
            }
            recover(&mut interrupted).unwrap();
            assert!(interrupted.live.entries == before || interrupted.live.entries == after);
        }
        baseline = complete;
        baseline.crash();
    }
}

#[test]
fn stale_target_and_damaged_new_slot_do_not_trigger_older_replay() {
    let mut disk = Disk::new();
    update(&mut disk, [[10; 32], [20; 32]]).unwrap();
    // Publish a second intent directly, leaving its targets unchanged.
    let before = disk.live.entries;
    let mut prepared = vec![0; intent::RECORD_BYTES];
    intent::encode(
        &mut prepared,
        identity(),
        2,
        Operation::Update,
        &[
            Change {
                offset: 1_048_576,
                before: &before[0],
                after: &[30; 32],
            },
            Change {
                offset: 1_048_608,
                before: &before[1],
                after: &[40; 32],
            },
        ],
    )
    .unwrap();
    disk.live.slots[1] = prepared;
    disk.live.entries[1] = [99; 32];
    assert!(matches!(
        recover(&mut disk),
        Err(TransactionError::Record(RecordError::Stale))
    ));
    assert_eq!(
        disk.live.entries[0], before[0],
        "no writes before ALL targets validate"
    );
    disk.live.slots[1][600] ^= 1;
    assert!(matches!(
        recover(&mut disk),
        Err(TransactionError::Record(RecordError::Malformed))
    ));
    assert_eq!(disk.live.entries[1], [99; 32]);
}
