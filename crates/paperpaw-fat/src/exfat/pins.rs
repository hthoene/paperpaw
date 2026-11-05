//! Pin lifetimes on failed recovery comparisons, without public test hooks.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "regression assertions"
)]
use super::*;
use crate::BlockDisk;
use alloc::rc::Rc;
use alloc::vec;

#[derive(Clone)]
struct Bytes(Rc<RefCell<Vec<u8>>>);
impl Blocks for Bytes {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.0.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        Ok(())
    }
    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.0
            .borrow_mut()
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        Ok(())
    }
    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.0.borrow().len() / BLOCK) as u32)
    }
}

fn storage() -> ExFatStorage<Bytes> {
    let sparse = include_bytes!("../../tests/images/small.sparse");
    let size = u64::from_le_bytes(sparse[8..16].try_into().unwrap()) as usize;
    let mut bytes = vec![0; size];
    for chunk in sparse[16..].chunks(BLOCK + 4) {
        let at = u32::from_le_bytes(chunk[..4].try_into().unwrap()) as usize * BLOCK;
        bytes[at..at + BLOCK].copy_from_slice(&chunk[4..]);
    }
    let bytes = Bytes(Rc::new(RefCell::new(bytes)));
    let (disk, _) = BlockDisk::find(bytes).unwrap();
    ExFatStorage::mount(SharedDisk::new(disk)).unwrap()
}

#[test]
fn failed_comparisons_release_all_pins() {
    let card = storage();
    card.write("a", b"a").unwrap();
    card.write("b", b"b").unwrap();
    card.sync().unwrap();
    let root = card.fs.borrow().root();
    let baseline = card.fs.borrow().open_nodes();
    assert_eq!(
        baseline, 1,
        "only the root is pinned before the comparisons"
    );
    for _ in 0..64 {
        assert_eq!(card.share(root, "a", "missing"), Ok(false));
        assert_eq!(
            card.fs.borrow().open_nodes(),
            baseline,
            "missing second name"
        );
        assert_eq!(
            card.share(root, "a", "bad/name"),
            Err(StorageError::InvalidPath)
        );
        assert_eq!(
            card.fs.borrow().open_nodes(),
            baseline,
            "failed second lookup"
        );
        let result = card.with_pair(root, "a", "b", |_, _| Err(StorageError::Io));
        assert_eq!(result, Err(StorageError::Io));
        assert_eq!(
            card.fs.borrow().open_nodes(),
            baseline,
            "metadata error after both lookups"
        );
    }
    assert_eq!(card.share(root, "a", "a"), Ok(true));
    assert_eq!(
        card.fs.borrow().open_nodes(),
        baseline,
        "two pins of the same node"
    );
}
