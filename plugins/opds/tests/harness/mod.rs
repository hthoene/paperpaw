//! What the OPDS tests share: a kernel with the plugin, a storage the test
//! looks into, and the requests it asks for, answered as a server would.

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_opds::Opds;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

pub const fn device(memory: MemoryClass) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, false, false, &[]),
        memory,
    )
}

/// The plugin's folder.
pub const DIR: &str = ".paperpaw/plugins/opds";
pub const CATALOGS: &str = ".paperpaw/plugins/opds/catalogs.txt";

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
pub struct Shared(pub Rc<MemoryStorage>);

impl Shared {
    pub fn with(files: &[(&str, &[u8])]) -> Self {
        let mut storage = MemoryStorage::new();
        for (path, data) in files {
            storage = storage.with(path, data.to_vec());
        }
        Self(Rc::new(storage))
    }

    /// Every file, by its path, below `dir` ("" for all).
    pub fn files(&self, dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut entries = Vec::new();
        let _ = self
            .0
            .list(dir, &mut |e| entries.push((e.name.to_string(), e.is_dir)));
        for (name, is_dir) in entries {
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                found.extend(self.files(&path));
            } else {
                found.push(path);
            }
        }
        found.sort();
        found
    }

    pub fn text(&self, path: &str) -> Option<String> {
        self.0
            .get(path)
            .map(|d| String::from_utf8_lossy(&d).into_owned())
    }
}

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

pub struct Harness {
    pub kernel: Kernel,
    pub storage: Shared,
    pub now: u64,
}

impl Harness {
    /// A started kernel with the OPDS plugin, the EPUB format and fonts.
    pub fn new(storage: Shared) -> Self {
        Self::build(storage, MemoryClass::Small, "")
    }

    pub fn build(storage: Shared, memory: MemoryClass, config: &str) -> Self {
        let mut kernel = Kernel::builder(device(memory))
            .storage(storage.clone())
            // The library at the storage's top.
            .user_config(format!("[core]\nlibrary-folder =\n{config}"))
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

    fn tick(&mut self) -> Instant {
        self.now += 100;
        Instant(self.now)
    }

    pub fn run(&mut self, action: Id) {
        let now = self.tick();
        self.kernel.run_action(now, action);
    }

    /// Sends a plugin's event as the keyboard or a choice would.
    pub fn send(&mut self, event: &'static str, data: impl EventData) {
        let now = self.tick();
        self.kernel
            .send_event(now, Id::new_static(event), data.to_params());
    }

    pub fn send_params(&mut self, event: &'static str, params: Params) {
        let now = self.tick();
        self.kernel.send_event(now, Id::new_static(event), params);
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

    /// The one request asked for now.
    pub fn request(&mut self) -> HttpCall {
        let mut calls = self.requests();
        assert_eq!(calls.len(), 1, "{calls:?}");
        calls.remove(0)
    }

    pub fn answer(&mut self, call: &HttpCall, status: u16, body: &[u8]) {
        self.reply(HttpResponse {
            id: call.id,
            status,
            body: body.to_vec(),
            ..HttpResponse::default()
        });
    }

    pub fn reply(&mut self, response: HttpResponse) {
        let now = self.tick();
        self.kernel.http_response(now, response);
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

    pub fn shows(&mut self, text: &str) -> bool {
        self.shown().iter().any(|l| l.contains(text))
    }

    /// All the text shown as one line, so text wrapped over lines is
    /// found too.
    pub fn said(&mut self) -> String {
        self.shown()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A catalogs list as version 1 writes it: name, address, user,
/// (password), folder, `sync`, `webdav`.
pub fn catalogs(lines: &[[&str; 7]]) -> Vec<u8> {
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.join("\t"));
        text.push('\n');
    }
    text.into_bytes()
}

/// An acquisition feed with `entries` of (id, title, href).
pub fn books_feed(title: &str, next: Option<&str>, entries: &[(&str, &str, &str)]) -> String {
    let mut feed = format!(
        "<?xml version=\"1.0\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\"><title>{title}</title>"
    );
    if let Some(next) = next {
        feed.push_str(&format!(
            "<link rel=\"next\" type=\"application/atom+xml;profile=opds-catalog\" href=\"{next}\"/>"
        ));
    }
    for (id, title, href) in entries {
        feed.push_str(&format!(
            "<entry><id>{id}</id><title>{title}</title>\
             <link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"{href}\"/></entry>"
        ));
    }
    feed.push_str("</feed>");
    feed
}
