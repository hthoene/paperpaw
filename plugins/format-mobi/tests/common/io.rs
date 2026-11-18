//! The harness of the provider tests: the book and the cache as one storage,
//! and the calls.

use paperpaw_plugin_format_mobi::provider::MobiProvider;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

pub const SIDE: u32 = 800;
pub const COVER: u32 = 300;

/// Root 3 (the book, path `""`) and root 6 (any other path).
pub struct Roots(pub MemoryStorage);

impl Roots {
    pub fn new(book: Vec<u8>) -> Self {
        Self(MemoryStorage::new().with("book", book))
    }
    fn map(path: &str) -> String {
        if path.is_empty() {
            "book".into()
        } else {
            format!("cache/{path}")
        }
    }
}

impl Storage for Roots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(&Self::map(path), offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(&Self::map(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.write(&Self::map(path), data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::Unsupported);
        }
        self.0.append(&Self::map(path), data)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(&Self::map(path))
    }
}

pub fn call(op: u32, document: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
    let mut c = stage::FmtCall::new(op, document, 0, 0, 0, 0, 0, 0, 0, 0, 7, "", &[]);
    f(&mut c);
    let mut buf = vec![0u8; 4096];
    let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
    buf.truncate(n);
    buf
}

/// An open with the sides the tests decode to.
pub fn open_call(document: u32) -> Vec<u8> {
    call(2, document, |c| {
        c.image_side = SIDE;
        c.cover_side = COVER;
    })
}

pub fn run(p: &mut MobiProvider, io: &dyn Storage, input: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = vec![0u8; 3840];
    let n = p.call(io, input, &mut out)?;
    out.truncate(n);
    Ok(out)
}

pub fn rows(reply: &[u8]) -> Vec<stage::Record<'_>> {
    stage::reply_decode(reply)
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// (state, status, flags, progress) of an open or a prepare.
pub fn state(reply: &[u8]) -> (u32, i32, u32, u32) {
    let stage::Record::FmtState(s) = rows(reply)[0] else {
        panic!("state")
    };
    (s.state, s.status, s.flags, s.progress_ppm)
}

/// Opens document 1 and prepares it until it is ready or failed.
pub fn open_ready(p: &mut MobiProvider, io: &dyn Storage) -> (u32, i32, u32) {
    let (mut s, mut status, mut flags, _) = state(&run(p, io, &open_call(1)).unwrap());
    let mut guard = 0;
    while s == 1 {
        (s, status, flags, _) = state(&run(p, io, &call(3, 1, |_| {})).unwrap());
        guard += 1;
        assert!(guard < 1000);
    }
    (s, status, flags)
}
