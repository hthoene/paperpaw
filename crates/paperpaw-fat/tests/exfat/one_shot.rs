//! A single failed I/O must not turn an unfinished move into shared aliases.

use super::*;

#[derive(Clone)]
struct OnceIo {
    bytes: Bytes,
    left: Rc<std::cell::Cell<usize>>,
}
impl OnceIo {
    fn pass(&self) -> Result<(), paperpaw_fat::DiskError> {
        match self.left.get() {
            usize::MAX => Ok(()),
            0 => {
                self.left.set(usize::MAX);
                Err(paperpaw_fat::DiskError)
            }
            n => {
                self.left.set(n - 1);
                Ok(())
            }
        }
    }
}
impl paperpaw_fat::Blocks for OnceIo {
    fn read_block(&mut self, i: u32, b: &mut [u8; 512]) -> Result<(), paperpaw_fat::DiskError> {
        self.pass()?;
        self.bytes.read_block(i, b)
    }
    fn write_block(&mut self, i: u32, b: &[u8; 512]) -> Result<(), paperpaw_fat::DiskError> {
        self.pass()?;
        self.bytes.write_block(i, b)
    }
    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        self.bytes.blocks()
    }
}
fn once_mount(image: &[u8]) -> (ExFatStorage<OnceIo>, OnceIo) {
    let io = OnceIo {
        bytes: card_of(image),
        left: Rc::new(std::cell::Cell::new(usize::MAX)),
    };
    let (disk, _) = BlockDisk::find(io.clone()).unwrap();
    (ExFatStorage::mount(SharedDisk::new(disk)).unwrap(), io)
}
#[test]
fn a_single_error_move_recovery_keeps_exactly_one_folder() {
    for from in ["shelf", "origin/shelf"] {
        let to = "elsewhere/deeper/shelf";
        let image = prepared(|s| {
            s.write(&format!("{from}/a.epub"), b"old book").unwrap();
            s.write("elsewhere/x.txt", b"sentinel").unwrap();
        });
        for (cut, image) in cut_images(image, |s| s.rename(from, to).unwrap())
            .iter()
            .enumerate()
        {
            for fault in 0..120 {
                let (card, io) = once_mount(image);
                io.left.set(fault);
                let first = card.size("elsewhere/x.txt");
                io.left.set(usize::MAX);
                let a = whole(&card, &format!("{from}/a.epub"));
                let b = whole(&card, &format!("{to}/a.epub"));
                let one = (a == Ok(b"old book".to_vec()) && b == Err(StorageError::NotFound))
                    || (b == Ok(b"old book".to_vec()) && a == Err(StorageError::NotFound));
                assert_eq!(whole(&card, "elsewhere/x.txt"), Ok(b"sentinel".to_vec()));
                assert!(
                    one,
                    "from={from},cut={cut},fault={fault},first={first:?},old={a:?},new={b:?}"
                );
            }
        }
    }
}
#[test]
fn a_single_error_remove_does_not_report_success_with_folder_left() {
    let image = prepared(|s| {
        s.write("books/shelf/a.epub", b"old book").unwrap();
        s.write("sentinel", b"safe").unwrap();
    });
    for fault in 0..200 {
        let (card, io) = once_mount(&image);
        io.left.set(fault);
        let result = card.remove_dir_all("books/shelf");
        io.left.set(usize::MAX);
        if result.is_ok() {
            let remains = folder(&card, "books/shelf");
            assert_eq!(remains, None, "fault={fault}: remove said success");
        }
        assert_eq!(whole(&card, "sentinel"), Ok(b"safe".to_vec()));
    }
}

#[test]
fn a_single_error_during_move_keeps_exactly_one_folder() {
    let image = prepared(|s| {
        s.write("origin/shelf/a.epub", b"old book").unwrap();
        s.write("elsewhere/x.txt", b"sentinel").unwrap();
    });
    for fault in 0..180 {
        let (card, io) = once_mount(&image);
        io.left.set(fault);
        let result = card.rename("origin/shelf", "elsewhere/shelf");
        io.left.set(usize::MAX);
        // Force normal recovery and then remount, as after card removal.
        let synced = card.sync();
        drop(card);
        let card = mount(&io.bytes);
        let a = whole(&card, "origin/shelf/a.epub");
        let b = whole(&card, "elsewhere/shelf/a.epub");
        let one = (a == Ok(b"old book".to_vec()) && b == Err(StorageError::NotFound))
            || (b == Ok(b"old book".to_vec()) && a == Err(StorageError::NotFound));
        assert!(
            one,
            "fault={fault},rename={result:?},sync={synced:?},old={a:?},new={b:?}"
        );
    }
}
