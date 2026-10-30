//! The real kernel with the OPDS plugin, a storage the test looks into,
//! and the requests the kernel asks the shell to make, answered by hand.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    reason = "tests panic on purpose when something is wrong; not every test uses every helper"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_opds::Opds;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Env, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    Params, PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// Where the plugin keeps its files.
pub fn plugin_dir() -> String {
    Env::plugin_dir("opds")
}

/// The list of catalogs.
pub fn catalogs_path() -> String {
    format!("{}/catalogs.txt", plugin_dir())
}

/// A storage the test looks into while the kernel has it; files can be
/// made unreadable, as on a failing card.
#[derive(Clone, Default)]
pub struct Shared {
    pub files: Rc<MemoryStorage>,
    broken: Rc<RefCell<Vec<String>>>,
}

impl Shared {
    /// Reading `path` fails from now on.
    pub fn break_file(&self, path: &str) {
        self.broken.borrow_mut().push(path.to_string());
    }

    fn check(&self, path: &str) -> Result<(), StorageError> {
        match self.broken.borrow().iter().any(|p| p == path) {
            true => Err(StorageError::Io),
            false => Ok(()),
        }
    }

    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(path)
    }
}

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.check(path)?;
        self.files.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.check(path)?;
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.files.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.files.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.files.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove_dir_all(path)
    }
}

pub struct Device {
    pub kernel: Kernel,
    pub storage: Shared,
    now: u64,
}

impl Device {
    /// Started on `storage`, with the plugin and EPUB books.
    pub fn start(storage: Shared) -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            // The library at the storage's top.
            .user_config("[core]\nlibrary-folder =\n")
            .network(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_format_epub::EpubFormat)
            .plugin(Opds)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            storage,
            now: 0,
        }
    }

    pub fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    pub fn open(&mut self, screen: &'static str, params: Params) {
        self.now += 100;
        self.kernel
            .open_screen(Instant(self.now), Id::new_static(screen), params);
    }

    /// The requests the kernel asks the shell to make now.
    pub fn requests(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    /// The one request the kernel asks the shell to make now.
    pub fn request(&mut self) -> HttpCall {
        let mut calls = self.requests();
        assert_eq!(calls.len(), 1, "{calls:?}");
        calls.remove(0)
    }

    pub fn answer(&mut self, call: &HttpCall, status: u16, body: &str) {
        self.now += 100;
        let response = HttpResponse {
            id: call.id,
            status,
            body: body.as_bytes().to_vec(),
            ..HttpResponse::default()
        };
        self.kernel.http_response(Instant(self.now), response);
    }

    /// The labels of what the screen shows, in order.
    pub fn shown(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }
}

/// Whether `call` carries credentials.
pub fn authorized(call: &HttpCall) -> bool {
    call.headers.iter().any(|(name, _)| name == "Authorization")
}
