//! What the store's tests share: an X4, a storage the test looks into
//! while the kernel has it, and the kernel with the store, driven as by
//! the buttons and answered as by the shell.

// Not every test uses every part.
#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::rc::Rc;

use paperpaw_core::{CodeLoader, HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_store::{OPEN, Store};
use paperpaw_sdk::native::Native;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    PixelFormat, Plugin, PluginProblem, Storage, StorageError,
};

pub const DEVICE: DeviceInfo = DeviceInfo::new(
    "xteink-x4",
    "Xteink X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// The list for the X4.
pub const LIST: &str = "https://paperpaw.org/api/v1/plugins.json?device=x4&lang=en&order=fair";

/// A storage the test looks into while the kernel has it (in memory, or
/// on the disk), with as much free space as the test says (none said: not
/// known).
#[derive(Clone)]
pub struct Shared(pub Rc<dyn Storage>, pub Rc<std::cell::Cell<Option<u64>>>);

impl Default for Shared {
    fn default() -> Self {
        Self(Rc::new(MemoryStorage::new()), Rc::default())
    }
}

impl Shared {
    /// A storage in a folder of its own on the disk: what it holds takes
    /// no memory.
    pub fn on_disk(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("paperpaw-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let storage = paperpaw_fs::DirStorage::new(&root).unwrap();
        Self(Rc::new(storage), Rc::default())
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
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
    fn free_space(&self) -> Option<u64> {
        self.1.get()
    }
}

/// Loads no code: code plugins installed in a test start after a restart
/// it never makes.
pub struct NoCode;

impl CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &paperpaw_core::CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Damaged)
    }
}

/// The kernel with the store, as the reader uses it.
pub struct Shopping {
    pub kernel: Kernel,
    pub storage: Shared,
    pub now: u64,
}

impl Shopping {
    /// The store opened on the X4 and its list answered with `list`.
    pub fn new(list: &str) -> Self {
        Self::on(Some("x4"), None, list)
    }

    /// The store opened on a device libraries call `device`, loading code
    /// plugins as `native` says, its list answered with `list`.
    pub fn on(device: Option<&'static str>, native: Option<Native>, list: &str) -> Self {
        Self::with(device, native, list, Shared::default())
    }

    /// As [`Shopping::on`], on `storage` (plugins already in it are
    /// installed).
    pub fn with(
        device: Option<&'static str>,
        native: Option<Native>,
        list: &str,
        storage: Shared,
    ) -> Self {
        Self::of(DEVICE, device, native, list, storage, None)
    }

    /// The store on the X4 running firmware `version`.
    pub fn on_firmware(version: &str, list: &str, storage: Shared) -> Self {
        let firmware = paperpaw_sdk::firmware::Firmware::new(version, "", 0);
        Self::of(DEVICE, Some("x4"), None, list, storage, Some(firmware))
    }

    /// The store on the X4 Pro (touch and buttons), its list answered with
    /// `list`.
    pub fn touch(list: &str) -> Self {
        Self::touch_with(None, list)
    }

    /// As [`Shopping::touch`], loading code plugins as `native` says.
    pub fn touch_with(native: Option<Native>, list: &str) -> Self {
        let mut pro = DEVICE;
        pro.input.touch = true;
        Self::of(pro, Some("x4-pro"), native, list, Shared::default(), None)
    }

    fn of(
        info: DeviceInfo,
        device: Option<&'static str>,
        native: Option<Native>,
        list: &str,
        storage: Shared,
        firmware: Option<paperpaw_sdk::firmware::Firmware>,
    ) -> Self {
        let mut builder = Kernel::builder(info)
            .storage(storage.clone())
            .network(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_fonts_browser::FontsBrowser)
            // Shows the notices.
            .plugin(paperpaw_plugin_menu::Menu)
            // Asks for text and numbers.
            .plugin(paperpaw_plugin_keyboard::Keyboard)
            .plugin(Store);
        if let Some(device) = device {
            builder = builder.catalog_device(device);
        }
        if let Some(native) = native {
            builder = builder.code_plugins(native, Box::new(NoCode));
        }
        if let Some(firmware) = firmware {
            builder = builder.firmware(firmware);
        }
        let mut kernel = builder.build().unwrap();
        kernel.start(Instant(0));
        let mut shopping = Self {
            kernel,
            storage,
            now: 0,
        };
        shopping.run(OPEN);
        if let Some(device) = device {
            let list_url = LIST.replace("device=x4&", &format!("device={device}&"));
            let call = shopping.request(&list_url);
            shopping.answer(&call, 200, list.as_bytes());
        }
        shopping
    }

    pub fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
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

    /// The one request for `url` asked for now.
    pub fn request(&mut self, url: &str) -> HttpCall {
        let all = self.requests();
        let mut calls: Vec<HttpCall> = all.iter().filter(|c| c.url == url).cloned().collect();
        assert_eq!(calls.len(), 1, "{url}: {all:?}");
        calls.remove(0)
    }

    pub fn answer(&mut self, call: &HttpCall, status: u16, body: &[u8]) {
        self.now += 100;
        let response = HttpResponse {
            id: call.id,
            status,
            body: body.to_vec(),
            ..HttpResponse::default()
        };
        self.kernel.http_response(Instant(self.now), response);
    }

    /// The one request for `url` answered with `body`.
    pub fn serve(&mut self, url: &str, body: &[u8]) {
        let call = self.request(url);
        self.answer(&call, 200, body);
    }

    /// Moves the focus down to the entry whose label begins with `label`
    /// and chooses it, as with the buttons.
    pub fn choose(&mut self, label: &str) {
        for _ in 0..200 {
            self.kernel.render(&mut |_, _| {});
            let focused = self
                .kernel
                .describe()
                .into_iter()
                .any(|p| p.focused && p.label.starts_with(label));
            if focused {
                return self.run(paperpaw_sdk::builtin::actions::CONFIRM);
            }
            self.run(paperpaw_sdk::builtin::actions::DOWN);
        }
        panic!("{label}: {}", self.shown());
    }

    /// A finger on the screen at `x`, `y`: a tap, or held for a second.
    pub fn touch_at(&mut self, x: i32, y: i32, held: bool) {
        use paperpaw_sdk::{InputEvent, Point, PointerEvent, PointerPhase, PointerTool};
        let event = |phase| {
            InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                Point::new(x, y),
                PointerTool::Finger,
                1000,
            ))
        };
        self.now += 200;
        self.kernel
            .handle(Instant(self.now), event(PointerPhase::Down));
        self.now += if held { 1000 } else { 60 };
        self.kernel.tick(Instant(self.now));
        self.kernel
            .handle(Instant(self.now), event(PointerPhase::Up));
    }

    /// The labels of what the screen shows, in order.
    pub fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Compares what the screen shows, part by part, with
    /// `tests/snapshots/{name}.txt`; written anew with
    /// `PAPERPAW_SNAPSHOTS=update`.
    pub fn check(&mut self, name: &str) {
        self.kernel.render(&mut |_, _| {});
        let mut shown = String::new();
        for part in self.kernel.describe() {
            let focused = if part.focused { " (focused)" } else { "" };
            let label = part.label.replace('\n', " / ");
            shown.push_str(&format!("{:?} \"{label}\"{focused}\n", part.role));
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(format!("{name}.txt"));
        if std::env::var("PAPERPAW_SNAPSHOTS").as_deref() == Ok("update") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &shown).unwrap();
            return;
        }
        let kept = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} missing: PAPERPAW_SNAPSHOTS=update", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(
            shown, kept,
            "{name} shows something else now (PAPERPAW_SNAPSHOTS=update if meant)"
        );
    }
}
