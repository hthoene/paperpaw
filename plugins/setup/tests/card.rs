//! The first-run setup where the card cannot keep that it was done: it
//! does not come at every start (an X4 with an exFAT card brought it back
//! at each start in beta.7, its choices lost; exFAT is read since
//! beta.9, NTFS is not), and the reader is told
//! why: by the kernel where the shell found no card it can use, by the
//! guide where its mark could not be written.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::rc::Rc;

use common::*;
use paperpaw_core::CardTrouble;
use paperpaw_platform::NoStorage;

const GUIDE: &str = "setup.guide";

/// A storage that keeps what it has but writes nothing to paths that
/// begin with `refused` (all paths where it is empty).
struct Refusing {
    files: MemoryStorage,
    refused: &'static str,
}

impl Refusing {
    fn shared(refused: &'static str) -> Shared {
        let storage = Shared::bare("", &[]);
        let files = MemoryStorage::new().with(
            paperpaw_sdk::builtin::CONFIG_FILE,
            storage.config().into_bytes(),
        );
        Shared(Rc::new(Self { files, refused }))
    }

    fn check(&self, path: &str) -> Result<(), StorageError> {
        if path.starts_with(self.refused) {
            Err(StorageError::Io)
        } else {
            Ok(())
        }
    }
}

impl Storage for Refusing {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check(path)?;
        self.files.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check(path)?;
        self.files.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.check(path)?;
        self.files.remove(path)
    }
}

#[test]
fn a_card_of_ntfs_brings_no_setup_and_says_why_at_each_start() {
    for device in DEVICES {
        let mut app = App::with_shell(device, Shared(Rc::new(NoStorage)), |builder| {
            builder.card_trouble(CardTrouble::Ntfs)
        });
        for _ in 0..2 {
            assert_ne!(app.screen(), GUIDE);
            for line in ["The memory card is NTFS", "on a computer."] {
                assert!(app.shows(line), "{}", app.described());
            }
            app = app.restart();
        }
    }
}

#[test]
fn a_card_that_cannot_be_written_brings_no_setup() {
    for device in DEVICES {
        let mut app = App::new(device, Refusing::shared(""));
        for _ in 0..2 {
            assert_ne!(app.screen(), GUIDE, "{}", app.described());
            app = app.restart();
        }
    }
}

#[test]
fn a_done_mark_that_cannot_be_written_is_said() {
    let mut app = App::new(X4, Refusing::shared(SETUP_DONE));
    assert_eq!(app.screen(), GUIDE);
    // Skipped: *Back*, then the second answer.
    app.run(actions::BACK);
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "home.dashboard", "{}", app.described());
    assert!(
        app.shows("The memory card could not be"),
        "{}",
        app.described()
    );
}
