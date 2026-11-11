//! Every allocation of the download's and the install's code may be refused:
//! the job, the staging names, the proof, the record of the installs and the
//! queue's turn end in `NoMemory` or succeed, never abort. The plugin's host
//! calls are stood in for by a scripted wire.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_download::{self as dl, Done, Download, FILE_REQUEST};
use paperpaw_plugin_store::native_install::{
    self as inst, End, Have, Installer, Installs, Next, Proof,
};
use paperpaw_plugin_store::native_item::{Digest, File, More, Rotation};
use paperpaw_plugin_store::native_list::Page;
use paperpaw_plugin_store::native_model::{Entry, Revoked};
use paperpaw_plugin_store::native_queue::{Mark, Queue};
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

/// A card whose own bookkeeping is not what the sweep refuses.
struct Card(MemoryStorage);

impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        fail_alloc::exempt(|| self.0.read_at(p, o, b))
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        fail_alloc::exempt(|| self.0.size(p))
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.write(p, d))
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.append(p, d))
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.list(d, v))
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.remove(p))
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.rename(a, b))
    }
}

struct Wire {
    card: Card,
    last: Option<String>,
    asked: u32,
}

impl dl::Wire for Wire {
    fn storage(&self) -> &dyn Storage {
        &self.card
    }
    fn get(&mut self, _url: &str, file: &str, _token: u32, _max: u64) -> Result<RequestId, Error> {
        self.last = Some(file.to_string());
        self.asked += 1;
        Ok(RequestId::new(self.asked).unwrap())
    }
    fn cancel(&mut self, _token: u32) {}
}

impl inst::Wire for Wire {
    fn storage(&self) -> &dyn Storage {
        &self.card
    }
    fn install(&mut self, _r: u32, _f: &str, _u: bool, _o: bool, _p: &Proof) -> Result<(), Error> {
        Ok(())
    }
    fn revoke(&mut self, _revoked: &[Revoked]) -> Result<(), Error> {
        Ok(())
    }
}

const INI: &[u8] = b"id = airy-theme\nname = Airy\n";

fn page() -> Page {
    let file = |path: &str, body: &[u8]| File {
        path: path.into(),
        url: format!("https://lib.example/p/{path}"),
        size: body.len() as u64,
        digest: Digest::Sha256(paperpaw_sdk::digest::sha256::hex(body)),
    };
    Page {
        entry: Entry {
            id: "airy-theme".into(),
            name: "Airy".into(),
            ..Entry::default()
        },
        files: vec![file("plugin.ini", INI)],
        code: true,
        author_key: Some([7; 32]),
        recovery_key: Some([8; 32]),
        rotations: vec![Rotation {
            to: [1; 32],
            signature: [2; 64],
        }],
        more: More::default(),
    }
}

#[test]
fn a_download_and_an_install_survive_refused_allocations() {
    let page = page();
    let revoked = vec![Revoked {
        plugin: "airy-theme".into(),
        key: [7; 32],
        signature: [9; 64],
    }];
    fail_alloc::sweep(|| {
        let mut wire = fail_alloc::exempt(|| Wire {
            card: Card(MemoryStorage::new()),
            last: None,
            asked: 0,
        });
        let mut download = Download::new();
        let mut installer = Installer::new();
        download.start(&mut wire, &page)?;
        let Some(file) = wire.last.clone() else {
            // The job could not be kept: nothing went out.
            return Ok(());
        };
        fail_alloc::exempt(|| wire.card.write(&file, INI)).unwrap();
        let done = Done::new(
            RequestId::new(wire.asked).unwrap(),
            FILE_REQUEST,
            Reason::Ok,
            true,
        );
        download.ended(&mut wire, &done)?;
        if let Some(staged) = download.take_ready(&wire.card)? {
            let have = Have::default();
            installer.finish(&mut wire, &mut download, staged, &page, have)?;
            installer.done(&mut wire, &mut download, &End::new(1, 0, 2), "1.0.0", false)?;
        }
        inst::hand_over(&mut wire, &revoked)?;
        Download::progress(&wire.card)?;
        Ok::<(), Error>(())
    });
}

#[test]
fn the_record_of_installs_and_the_queue_survive_refused_allocations() {
    let card = Card(MemoryStorage::new());
    fail_alloc::sweep(|| {
        fail_alloc::exempt(|| {
            for name in [
                paperpaw_plugin_store::native_install::INSTALLS_FILE,
                "queue.txt",
                "queue-quiet.txt",
            ] {
                let _ = card.remove(name);
            }
        });
        for n in 0..5 {
            Installs::remember(&card, "airy-theme", &format!("1.{n}.0"))?;
        }
        let installs = Installs::read(&card)?;
        assert!(installs.of("airy-theme", "1.4.0"));
        Installs::forget(&card, "airy-theme")?;
        Installs::adopt(&card, [("other", "2.0.0")])?;
        let mut queue = Queue::default();
        queue.add(["alpha", "beta"], Mark::Quiet)?;
        assert!(matches!(inst::next(&mut queue, &card)?, Next::FetchPage(_)));
        Ok::<(), Error>(())
    });
}
