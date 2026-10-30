//! What the statistics' kernel tests share: an X4-like device with
//! buttons and a touch device, a storage the test looks into while the
//! kernel has it, a log of reading written as the plugin writes it, and
//! a kernel with the plugin, its clock, buttons, taps and what the
//! screen shows.

// Not every test file uses every helper.
#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::rc::Rc;

use paperpaw_core::{Effects, HttpCall, Io, Kernel};
use paperpaw_platform::{
    ButtonState, InputEvent, PointerEvent, PointerPhase, PointerTool, Rotation, SystemStatus,
    WallClock,
};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Point, Storage, StorageError,
};

/// An X4: black and white, buttons only.
pub const X4: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// A touch screen without buttons, larger and in grays.
pub const TOUCH: DeviceInfo = DeviceInfo::new(
    "test-touch",
    "Test touch",
    DisplayInfo::new(1072, 1448, 300, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(false, true, false, &[]),
    MemoryClass::Large,
);

pub const DIR: &str = ".paperpaw/plugins/stats";
pub const DAY: i64 = 86_400;

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
pub struct Shared(pub Rc<MemoryStorage>);

impl Shared {
    pub fn new(storage: MemoryStorage) -> Self {
        Self(Rc::new(storage))
    }

    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path)
    }

    /// The files in the plugin's folder.
    pub fn files(&self) -> Vec<String> {
        let mut names = Vec::new();
        let _ = self
            .0
            .list(DIR, &mut |entry| names.push(entry.name.to_string()));
        names.sort();
        names
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

/// A stay on a page as `pages.bin` keeps it: 16 bytes.
pub fn record(book: u16, page: u32, start: i64, duration: u16, pages: u32) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..2].copy_from_slice(&book.to_le_bytes());
    out[2..6].copy_from_slice(&page.to_le_bytes());
    out[6..10].copy_from_slice(&(start as u32).to_le_bytes());
    out[10..12].copy_from_slice(&duration.to_le_bytes());
    out[12..16].copy_from_slice(&pages.to_le_bytes());
    out
}

/// The stays of `pages.bin`: book, page, start, duration, pages.
pub fn stays(storage: &dyn Storage) -> Vec<(u16, u32, u32, u16, u32)> {
    let data =
        paperpaw_sdk::read_file(storage, &format!("{DIR}/pages.bin"), 1 << 24).unwrap_or_default();
    data.as_chunks::<16>()
        .0
        .iter()
        .map(|r| {
            let u32_at = |i: usize| u32::from_le_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]);
            (
                u16::from_le_bytes([r[0], r[1]]),
                u32_at(2),
                u32_at(6),
                u16::from_le_bytes([r[10], r[11]]),
                u32_at(12),
            )
        })
        .collect()
}

/// A card with `books` (id, title, last opened) and a log of reading.
pub struct Card {
    pub storage: MemoryStorage,
    log: Vec<u8>,
    books: String,
}

impl Card {
    pub fn new() -> Self {
        Self {
            storage: MemoryStorage::new(),
            log: Vec::new(),
            books: String::new(),
        }
    }

    pub fn book(mut self, id: u16, title: &str, pages: u32, last_open: i64) -> Self {
        self.books.push_str(&format!(
            "[book.{id}]\ntitle = {title}\nauthors = Jane Doe\nmd5 = md5-{id}\npages = {pages}\n\
             last-open = {last_open}\npath = books/{id}.epub\n\n"
        ));
        self
    }

    /// `minutes` pages of a minute each of book `book`, from `start`.
    pub fn read(mut self, book: u16, start: i64, minutes: u32) -> Self {
        for m in 0..minutes {
            let at = start + i64::from(m) * 60;
            self.log
                .extend_from_slice(&record(book, m + 1, at, 60, 300));
        }
        self
    }

    pub fn stay(mut self, record: [u8; 16]) -> Self {
        self.log.extend_from_slice(&record);
        self
    }

    pub fn build(self) -> MemoryStorage {
        let storage = self.storage;
        if !self.log.is_empty() {
            storage
                .write(&format!("{DIR}/pages.bin"), &self.log)
                .unwrap();
        }
        if !self.books.is_empty() {
            storage
                .write(&format!("{DIR}/books.ini"), self.books.as_bytes())
                .unwrap();
        }
        storage
    }
}

/// The kernel with the statistics on `device`.
pub struct App {
    pub kernel: Kernel,
    pub storage: Shared,
    pub now: u64,
}

impl App {
    /// With the time `unix_millis` (none: the device does not know it),
    /// `utc_offset` minutes east of UTC and `config` (the user's
    /// configuration after `[core] language = en`).
    pub fn new(
        device: DeviceInfo,
        storage: MemoryStorage,
        unix_millis: Option<i64>,
        config: &str,
        network: bool,
    ) -> Self {
        let storage = Shared::new(storage);
        let user = format!("[core]\nlanguage = en\n[help]\nshow = false\n{config}\n");
        let mut kernel = Kernel::builder(device)
            .storage(storage.clone())
            .user_config(user)
            .network(network)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(paperpaw_plugin_stats::Stats)
            .build()
            .unwrap();
        if let Some(unix_millis) = unix_millis {
            let clock = WallClock::new(Instant(0), unix_millis, Some(120));
            kernel.set_status(Instant(0), SystemStatus::new(Some(clock), None));
        }
        let effects = kernel.start(Instant(0));
        let mut app = Self {
            kernel,
            storage,
            now: 0,
        };
        app.settle(effects);
        app
    }

    /// Lets timers run until nothing is due within a second.
    pub fn settle(&mut self, mut effects: Effects) {
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

    pub fn open(&mut self, screen: &'static str, params: Params) {
        self.now += 100;
        let effects = self
            .kernel
            .open_screen(Instant(self.now), Id::new_static(screen), params);
        self.settle(effects);
    }

    pub fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// A button pressed and let go.
    pub fn press(&mut self, name: &'static str) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.now += 50;
            let effects = self
                .kernel
                .handle(Instant(self.now), InputEvent::Button { name, state });
            if state == ButtonState::Released {
                self.settle(effects);
            }
        }
    }

    /// A finger at `point`, coming down, moving or lifted.
    pub fn finger(&mut self, phase: PointerPhase, point: Point) {
        self.now += 50;
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            point,
            PointerTool::Finger,
            1000,
        ));
        let effects = self.kernel.handle(Instant(self.now), event);
        if phase == PointerPhase::Up {
            self.settle(effects);
        }
    }

    pub fn tap(&mut self, point: Point) {
        self.finger(PointerPhase::Down, point);
        self.finger(PointerPhase::Up, point);
    }

    /// A swipe from `from` to `to`.
    pub fn swipe(&mut self, from: Point, to: Point) {
        self.finger(PointerPhase::Down, from);
        for step in 1..=4 {
            let at = Point::new(
                from.x + (to.x - from.x) * step / 4,
                from.y + (to.y - from.y) * step / 4,
            );
            self.finger(PointerPhase::Move, at);
        }
        self.finger(PointerPhase::Up, to);
    }

    /// The middle of the part of the screen that says `label`.
    pub fn where_is(&mut self, label: &str) -> Option<Point> {
        self.kernel.render(&mut |_, _| {});
        let parts = self.kernel.describe();
        let area = paperpaw_sdk::semantics::find(&parts, label)?.area;
        Some(Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        ))
    }

    /// A tap on the part of the screen that says `label`.
    pub fn tap_on(&mut self, label: &str) {
        let at = self
            .where_is(label)
            .unwrap_or_else(|| panic!("nothing says {label}:\n{}", self.shown()));
        self.tap(at);
    }

    /// What the screen shows: a line for each part, `>` before the
    /// focused one.
    pub fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        let mut text = String::new();
        for part in self.kernel.describe() {
            if part.focused {
                text.push('>');
            }
            text.push_str(&part.label.replace('\n', " / "));
            text.push('\n');
        }
        text
    }

    pub fn screen(&self) -> String {
        self.kernel
            .active_screen()
            .map_or_else(String::new, |id| id.as_str().to_string())
    }

    /// The requests the kernel asks the shell to make.
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

    /// How many pixels are black.
    pub fn black(&mut self) -> usize {
        let mut black = 0;
        self.kernel.render(&mut |_, view| {
            for y in 0..view.height {
                for x in 0..view.width {
                    black += usize::from(view.pixel(x, y).luma() < 128);
                }
            }
        });
        black
    }
}
