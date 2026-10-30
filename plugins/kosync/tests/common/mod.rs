//! A device with the real kernel and the kosync plugin, whose requests the
//! tests answer as a sync server would.

// Not every test file uses every helper.
#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every helper"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::{InputEvent, Point, PointerEvent, PointerPhase, PointerTool, Rotation};
use paperpaw_plugin_format_txt::TxtFormat;
use paperpaw_plugin_kosync::Kosync;
use paperpaw_sdk::builtin::events::{self, Goto, Reading};
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, Params, PixelFormat, Plugin, RegisterError, Registrar, Storage, StorageError,
    Version,
};

pub const fn device(touch: bool) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(!touch, touch, false, &[]),
        MemoryClass::Large,
    )
}

/// The MD5 of `secret`, the account's key.
pub const KEY: &str = "5ebe2294ecd0e0f08eab7690d2a6ee69";
/// Where the plugin keeps its files.
pub const DIR: &str = ".paperpaw/plugins/kosync";
/// The device id of [`Sync::with_id`].
pub const DEVICE_ID: &str = "0123456789ABCDEF0123456789ABCDEF";
pub const SIGNED_IN: &str = "[kosync]\nuser = alice\n";

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
pub struct Shared(pub Rc<MemoryStorage>);

impl Shared {
    pub fn text(&self, path: &str) -> Option<String> {
        self.0.get(path).map(|b| String::from_utf8(b).unwrap())
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

/// Hears where the reader is asked to go.
struct Listener(Rc<RefCell<Vec<Goto>>>);

impl Plugin for Listener {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("listener"),
            "Listener",
            Version::new(0, 1, 0),
        )
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let heard = self.0.clone();
        r.on(events::GOTO, move |_, goto, _| {
            heard.borrow_mut().push(goto.clone())
        })
    }
}

/// Secrets file content with the account's key, if any.
pub fn secrets_file(key: Option<&str>) -> Vec<u8> {
    let mut secrets = Secrets::default();
    if let Some(key) = key {
        secrets.apply(SecretChange::keep("kosync.key", "kosync.secret", key));
    }
    secrets.encode().into_bytes()
}

pub struct Options {
    pub config: &'static str,
    pub key: Option<&'static str>,
    pub network: bool,
    pub touch: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            config: SIGNED_IN,
            key: Some(KEY),
            network: true,
            touch: false,
        }
    }
}

pub struct Sync {
    pub kernel: Kernel,
    pub storage: Shared,
    pub gone: Rc<RefCell<Vec<Goto>>>,
    pub now: u64,
}

impl Sync {
    /// Signed in as `alice` with [`KEY`], the device id [`DEVICE_ID`].
    pub fn with_id() -> Self {
        let storage = Shared::default();
        storage
            .write(
                &format!("{DIR}/device.ini"),
                format!("[device]\nid = {DEVICE_ID}\n").as_bytes(),
            )
            .unwrap();
        Self::on(storage, Options::default())
    }

    /// A device on `storage`, the secrets as `options` say.
    pub fn on(storage: Shared, options: Options) -> Self {
        if !storage.exists(FILE) {
            storage.write(FILE, &secrets_file(options.key)).unwrap();
        }
        let gone = Rc::default();
        let mut kernel = Kernel::builder(device(options.touch))
            .storage(storage.clone())
            .user_config(options.config)
            .network(options.network)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(TxtFormat)
            .plugin(Kosync)
            .plugin(paperpaw_plugin_power::Power)
            .plugin(Listener(Rc::clone(&gone)))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut sync = Self {
            kernel,
            storage,
            gone,
            now: 0,
        };
        sync.calls();
        sync
    }

    pub fn later(&mut self, millis: u64) -> Instant {
        self.now += millis;
        Instant(self.now)
    }

    /// The requests the kernel asks the shell to make.
    pub fn calls(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    pub fn reading(&mut self, event: Event<Reading>, path: &str, percent: f32) -> Vec<HttpCall> {
        let reading = Reading::new(
            path.to_string(),
            String::new(),
            percent,
            0,
            // Each progress another place.
            (percent * 100.0) as u32,
            String::new(),
        );
        let now = self.later(1000);
        self.kernel
            .send_event(now, event.id().clone(), reading.to_params());
        self.calls()
    }

    pub fn open(&mut self, path: &str, percent: f32) -> Vec<HttpCall> {
        self.reading(events::BOOK_OPENED, path, percent)
    }

    pub fn turn(&mut self, path: &str, percent: f32) -> Vec<HttpCall> {
        self.reading(events::PAGE_TURNED, path, percent)
    }

    pub fn answer(&mut self, call: &HttpCall, status: u16, body: &str) -> Vec<HttpCall> {
        self.answer_bytes(call, status, body.as_bytes())
    }

    pub fn answer_bytes(&mut self, call: &HttpCall, status: u16, body: &[u8]) -> Vec<HttpCall> {
        let now = self.later(100);
        self.kernel.http_response(
            now,
            HttpResponse {
                id: call.id,
                status,
                body: body.to_vec(),
                ..HttpResponse::default()
            },
        );
        self.calls()
    }

    /// Sets a setting as the reader would, e.g. the power mode.
    pub fn set(&mut self, id: &str, value: &str) -> Vec<HttpCall> {
        let now = self.later(100);
        self.kernel.set_setting(now, id, value).unwrap();
        self.calls()
    }

    pub fn run(&mut self, action: Id) -> Vec<HttpCall> {
        let now = self.later(100);
        let effects = self.kernel.run_action(now, action);
        self.settle(effects);
        self.calls()
    }

    pub fn send(&mut self, event: &str, params: Params) -> Vec<HttpCall> {
        let now = self.later(100);
        self.kernel
            .send_event(now, Id::parse(event).unwrap(), params);
        self.calls()
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..1000 {
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

    /// What the screen shows, as one text.
    pub fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    pub fn screen(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }

    /// A tap on the part of the screen that says `label`.
    pub fn tap_on(&mut self, label: &str) -> Vec<HttpCall> {
        self.kernel.render(&mut |_, _| {});
        let parts = self.kernel.describe();
        let area = paperpaw_sdk::semantics::find(&parts, label)
            .unwrap_or_else(|| panic!("nothing says {label}"))
            .area;
        let at = Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        );
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let now = self.later(50);
            let effects = self.kernel.handle(
                now,
                InputEvent::Pointer(PointerEvent::new(0, phase, at, PointerTool::Finger, 1000)),
            );
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
        self.calls()
    }

    pub fn secret(&self) -> Option<String> {
        self.kernel.secrets().get("kosync.key").map(str::to_string)
    }

    pub fn user(&self) -> String {
        self.kernel.settings().text("kosync.user").to_string()
    }
}

/// The value of header `name` of `call`.
pub fn header<'a>(call: &'a HttpCall, name: &str) -> Option<&'a str> {
    call.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

pub fn body(call: &HttpCall) -> String {
    String::from_utf8(call.body.clone().unwrap_or_default()).unwrap()
}
