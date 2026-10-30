//! *About PaperPaw* with the real kernel: the page shows the version and
//! the address in rows; its last row writes the licences to the plugin's
//! folder and opens them in the reader, with the version, the address,
//! and the logo beside them (or the picture the logo setting names, where
//! it is there); when they cannot be written, nothing is opened.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_about::About;
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::{
    Canvas, Cx, DirEntry, Env, Handled, Id, Manifest, MemoryStorage, Plugin, Priority,
    RegisterError, Registrar, Screen, ScreenSpec, Storage, StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

const PAGE: &str = ".paperpaw/plugins/about/licences.md";
const LOGO: &str = ".paperpaw/plugins/about/logo.png";
const BUNDLED: &[u8] = include_bytes!("../logo.png");

/// The paths the reader was opened with.
type Opened = Rc<RefCell<Vec<String>>>;

/// A reader that keeps what it is asked to open.
struct Reader(Opened);

struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Reader {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("fake-reader"),
            "Reader",
            Version::new(0, 1, 0),
        )
        .description("Keeps what it opens.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // A line of its own on the page.
        r.contribute(Id::new_static("about.lines"), alloc_text())?;
        let opened = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("fake-reader.screen"),
            "Reader",
            Box::new(move |_, params| {
                opened
                    .borrow_mut()
                    .push(params.get(params::PATH).unwrap_or_default().to_string());
                Box::new(Blank)
            }),
        ))?;
        r.bind_slot(
            slots::READER,
            Id::new_static("fake-reader.screen"),
            Priority::DEFAULT,
        )
    }
}

struct Device {
    kernel: Kernel,
    now: u64,
    opened: Opened,
}

impl Device {
    fn new(storage: MemoryStorage, config: &str) -> Self {
        let opened = Opened::default();
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(About)
            .plugin(Reader(opened.clone()))
            .build()
            .unwrap();
        let mut device = Self {
            kernel,
            now: 0,
            opened,
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// The page, then its last row: the licences.
    fn open(&mut self) {
        self.run(Id::new_static("about.open"));
        self.run(paperpaw_sdk::builtin::actions::LEFT);
        self.run(paperpaw_sdk::builtin::actions::CONFIRM);
    }

    /// What the screen shows, as one text.
    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        paperpaw_sdk::read_file(self.kernel.storage(), path, 1 << 22).ok()
    }

    fn page(&self) -> String {
        String::from_utf8(self.read(PAGE).unwrap()).unwrap()
    }
}

#[test]
fn the_page_shows_rows_and_opens_the_licences_in_the_reader() {
    let mut device = Device::new(MemoryStorage::new(), "");
    device.run(Id::new_static("about.open"));
    let shown = device.shown();
    assert!(shown.contains("About PaperPaw"), "{shown}");
    let paperpaw = paperpaw_sdk::SDK_VERSION.to_string();
    assert!(shown.contains(&paperpaw), "{shown}");
    assert!(shown.contains("github.com/hthoene/paperpaw"), "{shown}");
    assert!(shown.contains("Licences"), "{shown}");
    // Nothing opened in the reader before a row is chosen.
    assert!(device.opened.borrow().is_empty());
    device.run(paperpaw_sdk::builtin::actions::LEFT);
    device.run(paperpaw_sdk::builtin::actions::CONFIRM);
    assert_eq!(*device.opened.borrow(), [PAGE]);
    let page = device.page();
    assert!(
        page.starts_with("![PaperPaw](logo.png)\n\n# PaperPaw "),
        "{page}"
    );
    // The version as shown: a beta as "0.5.0 Beta 1", so its release part.
    let release = paperpaw.split('-').next().unwrap();
    assert!(page.contains(release), "{page}");
    assert!(page.contains("https://github.com/hthoene/paperpaw"));
    // The licences of what it bundles.
    assert!(page.contains("Literata"));
    assert!(page.contains("SIL Open Font License"));
    assert_eq!(device.read(LOGO).unwrap(), BUNDLED);
    // Closed and opened again: the same page, not one added to the
    // other.
    device.now += 100;
    let effects = device
        .kernel
        .run_action(Instant(device.now), paperpaw_sdk::builtin::actions::BACK);
    device.settle(effects);
    device.run(paperpaw_sdk::builtin::actions::CONFIRM);
    assert_eq!(device.page(), page);
    assert_eq!(*device.opened.borrow(), [PAGE, PAGE]);
}

#[test]
fn a_damaged_logo_is_written_anew() {
    let storage = MemoryStorage::new().with(LOGO, b"broken".to_vec());
    let mut device = Device::new(storage, "");
    device.open();
    assert_eq!(device.read(LOGO).unwrap(), BUNDLED);
}

#[test]
fn the_logo_set_is_shown_where_it_is_there() {
    let storage = MemoryStorage::new().with("Pictures/me.png", BUNDLED);
    let mut device = Device::new(storage, "logo = /Pictures/me.png/\n");
    device.open();
    assert!(device.page().starts_with("![PaperPaw](/Pictures/me.png)"));
    // PaperPaw's own is not needed then.
    assert!(device.read(LOGO).is_none());

    let mut device = Device::new(MemoryStorage::new(), "logo = Pictures/gone.png\n");
    device.open();
    assert!(device.page().starts_with("![PaperPaw](logo.png)"));
    assert_eq!(device.read(LOGO).unwrap(), BUNDLED);
}

/// A card that is full for the plugin's folder.
struct Full(MemoryStorage);

impl Storage for Full {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.starts_with(".paperpaw/plugins/about") {
            return Err(StorageError::Full);
        }
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

#[test]
fn nothing_opens_when_the_page_cannot_be_written() {
    let opened = Opened::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(Full(MemoryStorage::new()))
        .plugin(About)
        .plugin(Reader(opened.clone()))
        .build()
        .unwrap();
    let _ = kernel.start(Instant(0));
    let _ = kernel.run_action(Instant(100), Id::new_static("about.open"));
    let _ = kernel.run_action(Instant(200), paperpaw_sdk::builtin::actions::LEFT);
    let _ = kernel.run_action(Instant(300), paperpaw_sdk::builtin::actions::CONFIRM);
    assert!(opened.borrow().is_empty());
}

/// What the fake reader adds to the page: a line, a fake heading and
/// picture, and far too much.
fn alloc_text() -> String {
    format!(
        "The fake reader is by A. Tester.\n# PaperPaw 99 ![x](y.png)\n{}",
        "z".repeat(2000)
    )
}

#[test]
fn lines_of_other_plugins_are_on_the_page() {
    let mut device = Device::new(MemoryStorage::new(), "");
    device.open();
    let page = device.page();
    // Named by its plugin, as plain text.
    let line = page
        .find("*Reader:* The fake reader is by A\\. Tester\\.")
        .unwrap_or_else(|| panic!("{page}"));
    assert!(page.find("https://github.com/hthoene/paperpaw").unwrap() < line);
    // No heading nor picture of its own.
    assert!(
        page.contains("*Reader:* \\# PaperPaw 99 \\!\\[x\\]\\(y\\.png\\)"),
        "{page}"
    );
    assert!(!page.contains("\n# PaperPaw 99"));
    // At most 512 bytes of it.
    let zs = page.matches('z').count();
    assert!(zs < 512, "{zs}");
}
