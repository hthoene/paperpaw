//! Plugins installed from the computer over the transfer page, with the
//! real kernel: files sent one by one, each checked by its SHA-256, then
//! installed in one step and loaded at once; an update takes the old
//! version's place and leaves the plugin's own data; a file changed on
//! the way installs nothing; a plugin with code is installed only once
//! the reader said yes on the device; what came this way is local, never
//! the store's to update.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{CodeFile, CodeLoader, Effects, Io, Kernel, ServerCall, ServerEvent};
use paperpaw_native::{Chip, Header, LINK_BASE};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_transfer::Transfer;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::install::{self, Origin};
use paperpaw_sdk::native::{Fingerprint, Native, installed_code};
use paperpaw_sdk::{
    DirEntry, Id, MemoryStorage, Plugin, PluginProblem, Storage, StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// The build of this PaperPaw, for plugins with code.
const BUILD: Fingerprint = Fingerprint(0x0123_4567_89ab_cdef);

#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

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
}

/// Loads no code: plugins with code start after a restart the tests never
/// make.
struct NoCode;

impl CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Damaged)
    }
}

fn fill(buf: &mut [u8]) -> bool {
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte = (i as u8).wrapping_mul(97).wrapping_add(13);
    }
    true
}

fn sha(data: &[u8]) -> String {
    paperpaw_digest::sha256::hex(data)
}

fn manifest(id: &str, version: &str) -> String {
    format!("[plugin]\nid = {id}\nname = Airy\ndescription = Roomy.\nversion = {version}\n")
}

/// A plugin file built for this PaperPaw, not signed (one's own build).
fn ppn() -> Vec<u8> {
    ppn_of(0)
}

/// The same with an image of bytes `byte`: other code.
fn ppn_of(byte: u8) -> Vec<u8> {
    let image = [byte; 128];
    let mut header = Header {
        chip: Chip::Esp32C3,
        fingerprint: BUILD.0,
        base: LINK_BASE,
        image_len: image.len() as u32,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: paperpaw_sdk::native::INTERFACE,
        sha256: [0; 32],
    };
    let mut digest = header.digest();
    digest.update(&image);
    header.sha256 = digest.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    file
}

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
    server: u32,
    key: String,
    next: u32,
}

impl Device {
    /// The transfer screen open and serving, on `storage`.
    fn serving(storage: Shared) -> Self {
        let mut native = Native::new("code.ppn");
        native.fingerprint = BUILD;
        let kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .user_config("[core]\nlanguage = en\n")
            .network(true)
            .random(fill)
            .code_plugins(native, Box::new(NoCode))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Transfer)
            .build()
            .unwrap();
        let mut device = Self {
            kernel,
            storage,
            now: 0,
            server: 0,
            key: String::new(),
            next: 1,
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device.run(Id::new_static("transfer.open"));
        let calls = device.calls();
        let Some(ServerCall::Listen { server, .. }) = calls.first() else {
            panic!("no server: {calls:?}");
        };
        device.server = *server;
        device.event(ServerEvent::Started {
            server: device.server,
            address: "192.168.1.20:8080".into(),
        });
        device.kernel.render(&mut |_, _| {});
        let shown: Vec<String> = device
            .kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        let address = shown.iter().find(|l| l.contains("k=")).unwrap();
        device.key = address.rsplit("k=").next().unwrap().to_string();
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

    fn calls(&mut self) -> Vec<ServerCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Server(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn event(&mut self, event: ServerEvent) {
        self.now += 10;
        let effects = self.kernel.server_event(Instant(self.now), event);
        self.settle(effects);
    }

    /// A request with the key, its body if any; the status and body of
    /// the answer.
    fn request(&mut self, method: &str, path: &str, body: Option<&[u8]>) -> (u16, String) {
        let request = self.next;
        self.next += 1;
        let joined = if path.contains('?') { '&' } else { '?' };
        self.event(ServerEvent::Request {
            server: self.server,
            request,
            method: method.into(),
            target: format!("{path}{joined}k={}", self.key),
            body: body.is_some(),
        });
        if let Some(body) = body {
            self.event(ServerEvent::Body {
                request,
                data: body.to_vec(),
                last: true,
            });
        }
        self.calls()
            .into_iter()
            .find_map(|call| match call {
                ServerCall::Respond {
                    request: r,
                    status,
                    body,
                    ..
                } if r == request => Some((status, String::from_utf8_lossy(&body).into_owned())),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no answer to {method} {path}"))
    }

    /// Sends a file of plugin `id` with the SHA-256 of `said`.
    fn put(&mut self, id: &str, path: &str, data: &[u8], said: &[u8]) -> u16 {
        let target = format!("/api/plugin/{id}/{path}?sha256={}", sha(said));
        self.request("PUT", &target, Some(data)).0
    }

    fn version(&self, id: &str) -> Option<Version> {
        self.kernel.plugins().get(id).map(|p| p.manifest.version)
    }

    fn says(&mut self, text: &str) -> bool {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .iter()
            .any(|p| p.label.contains(text))
    }
}

#[test]
fn a_plugin_sent_from_the_computer_is_installed_at_once_and_local() {
    let mut device = Device::serving(Shared::default());
    let ini = manifest("airy", "1.0.0");
    assert_eq!(
        device.put("airy", "plugin.ini", ini.as_bytes(), ini.as_bytes()),
        200
    );
    let de = b"[meta]\nlanguage = de\n";
    assert_eq!(device.put("airy", "i18n/de.ini", de, de), 200);
    // Nothing installed before the commit.
    assert_eq!(device.version("airy"), None);
    let (status, body) = device.request("POST", "/api/plugin/airy", None);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"installed\""), "{body}");
    assert_eq!(device.version("airy"), Some(Version::new(1, 0, 0)));
    assert!(device.storage.exists("plugins/airy/i18n/de.ini"));
    assert!(!device.storage.exists("plugins/.airy.local/plugin.ini"));
    assert!(device.says("Installed: airy"));
    let (_, list) = device.request("GET", "/api/plugins", None);
    assert!(
        list.contains(r#"{"id":"airy","version":"1.0.0","on":true,"origin":"local"}"#),
        "{list}"
    );
    let (_, state) = device.request("GET", "/api/plugin/airy", None);
    assert!(state.contains("\"installed\""), "{state}");
}

#[test]
fn an_update_takes_the_old_ones_place_and_leaves_its_data_and_the_store() {
    let storage = Shared::default();
    storage
        .write(
            "plugins/airy/plugin.ini",
            manifest("airy", "1.0.0").as_bytes(),
        )
        .unwrap();
    storage
        .write(".paperpaw/plugins/airy/notes.txt", b"mine")
        .unwrap();
    install::remember_store_install(&storage, "airy", Version::new(1, 0, 0));
    let mut device = Device::serving(storage);
    let ini = manifest("airy", "1.1.0");
    assert_eq!(
        device.put("airy", "plugin.ini", ini.as_bytes(), ini.as_bytes()),
        200
    );
    assert_eq!(device.request("POST", "/api/plugin/airy", None).0, 200);
    assert_eq!(device.version("airy"), Some(Version::new(1, 1, 0)));
    assert_eq!(
        device.storage.0.get("plugins/airy/plugin.ini").unwrap(),
        ini.as_bytes()
    );
    assert_eq!(
        device
            .storage
            .0
            .get(".paperpaw/plugins/airy/notes.txt")
            .unwrap(),
        b"mine"
    );
    // Not the store's any more: it offers no update of it.
    let storage = &device.storage;
    assert_eq!(
        install::origin(storage, "airy", Version::new(1, 0, 0)),
        Origin::Local
    );
}

#[test]
fn a_file_changed_on_the_way_installs_nothing() {
    let storage = Shared::default();
    storage
        .write(
            "plugins/airy/plugin.ini",
            manifest("airy", "1.0.0").as_bytes(),
        )
        .unwrap();
    let mut device = Device::serving(storage);
    let ini = manifest("airy", "2.0.0");
    assert_eq!(
        device.put("airy", "plugin.ini", b"cut sh", ini.as_bytes()),
        422
    );
    let (status, body) = device.request("POST", "/api/plugin/airy", None);
    assert_eq!(status, 422, "{body}");
    assert!(body.contains("not the plugin"), "{body}");
    assert_eq!(device.version("airy"), Some(Version::new(1, 0, 0)));
    // Another plugin's files under this id, a wrong key, a bad name.
    let other = manifest("other", "1.0.0");
    device.put("airy", "plugin.ini", other.as_bytes(), other.as_bytes());
    assert_eq!(device.request("POST", "/api/plugin/airy", None).0, 422);
    assert_eq!(device.version("airy"), Some(Version::new(1, 0, 0)));
    assert!(!device.storage.exists("plugins/.airy.local/plugin.ini"));
    let target = format!("/api/plugin/airy/plugin.ini?sha256={}&k=wrong", sha(b"x"));
    device.key = String::from("wrong");
    assert_eq!(device.request("PUT", &target, Some(b"x")).0, 403);
    let mut left = 0;
    let _ = device.storage.list(".paperpaw/tmp", &mut |_| left += 1);
    assert_eq!(left, 0, "a body left");
}

#[test]
fn code_from_the_computer_runs_only_once_the_reader_said_yes() {
    let mut device = Device::serving(Shared::default());
    let code = ppn();
    let ini = manifest("clock", "1.0.0");
    assert_eq!(
        device.put("clock", "plugin.ini", ini.as_bytes(), ini.as_bytes()),
        200
    );
    assert_eq!(device.put("clock", "code.ppn", &code, &code), 200);
    let (status, body) = device.request("POST", "/api/plugin/clock", None);
    assert_eq!((status, body.as_str()), (202, r#"{"state":"asking"}"#));
    assert!(device.says("Install and run this"));
    assert_eq!(installed_code(&device.storage, "clock"), None);
    // No (where the focus starts): nothing installed, the files gone.
    device.run(actions::CONFIRM);
    let (_, state) = device.request("GET", "/api/plugin/clock", None);
    assert!(state.contains("not approved"), "{state}");
    assert!(!device.storage.exists("plugins/.clock.local/code.ppn"));
    assert!(device.kernel.plugins().get("clock").is_none());
    // Sent again, and yes: installed, its code named as installed.
    device.put("clock", "plugin.ini", ini.as_bytes(), ini.as_bytes());
    device.put("clock", "code.ppn", &code, &code);
    assert_eq!(device.request("POST", "/api/plugin/clock", None).0, 202);
    device.run(actions::LEFT);
    device.run(actions::CONFIRM);
    let (_, state) = device.request("GET", "/api/plugin/clock", None);
    assert!(state.contains(r#""installed","restart":true"#), "{state}");
    assert!(installed_code(&device.storage, "clock").is_some());
    assert!(device.storage.exists("plugins/clock/code.ppn"));
}

/// Code put in place of the code asked about, while the question shows,
/// is not installed: the answer was for the code asked about.
#[test]
fn code_changed_while_asked_about_is_not_installed() {
    let mut device = Device::serving(Shared::default());
    let code = ppn();
    let ini = manifest("clock", "1.0.0");
    device.put("clock", "plugin.ini", ini.as_bytes(), ini.as_bytes());
    device.put("clock", "code.ppn", &code, &code);
    assert_eq!(device.request("POST", "/api/plugin/clock", None).0, 202);
    let other = ppn_of(1);
    assert_eq!(device.put("clock", "code.ppn", &other, &other), 200);
    device.run(actions::LEFT);
    device.run(actions::CONFIRM);
    let (_, state) = device.request("GET", "/api/plugin/clock", None);
    assert!(state.contains("refused"), "{state}");
    assert_eq!(installed_code(&device.storage, "clock"), None);
    assert!(device.kernel.plugins().get("clock").is_none());
}

/// The SHA-256 a code file's header holds, as installers name its code.
fn code_name(file: &[u8]) -> [u8; 32] {
    Header::read(file).unwrap().sha256
}

/// An update that the kernel refuses (one of its files does not load)
/// changes nothing kept for the version before: its code stays named as
/// installed, and it stays the store's.
#[test]
fn a_refused_update_keeps_what_was_kept_for_the_version_before() {
    let storage = Shared::default();
    let old = ppn_of(2);
    storage
        .write(
            "plugins/clock/plugin.ini",
            manifest("clock", "1.0.0").as_bytes(),
        )
        .unwrap();
    storage.write("plugins/clock/code.ppn", &old).unwrap();
    paperpaw_sdk::native::remember_code(&storage, "clock", &code_name(&old));
    install::remember_store_install(&storage, "clock", Version::new(1, 0, 0));
    let mut device = Device::serving(storage);
    let code = ppn();
    let ini = manifest("clock", "2.0.0");
    device.put("clock", "plugin.ini", ini.as_bytes(), ini.as_bytes());
    device.put("clock", "code.ppn", &code, &code);
    // A catalog that does not load: the kernel refuses the folder.
    device.put("clock", "i18n/de.ini", b"[meta]\n", b"[meta]\n");
    assert_eq!(device.request("POST", "/api/plugin/clock", None).0, 202);
    device.run(actions::LEFT);
    device.run(actions::CONFIRM);
    let storage = &device.storage;
    assert_eq!(installed_code(storage, "clock"), Some(code_name(&old)));
    assert_eq!(
        install::origin(storage, "clock", Version::new(1, 0, 0)),
        Origin::Store
    );
    assert_eq!(storage.0.get("plugins/clock/code.ppn").unwrap(), old);
}

/// An install cut short after the plugin was in place (the power gone
/// before its marks were kept): kept at the next start.
#[test]
fn marks_left_in_a_plugins_folder_are_kept_at_the_next_start() {
    let storage = Shared::default();
    let code = ppn();
    storage
        .write(
            "plugins/clock/plugin.ini",
            manifest("clock", "1.0.0").as_bytes(),
        )
        .unwrap();
    storage.write("plugins/clock/code.ppn", &code).unwrap();
    let marks = install::marks::Marks::new(install::marks::From::Local).code(code_name(&code));
    assert!(install::marks::write(&storage, "plugins/clock", &marks).is_ok());
    let device = Device::serving(storage);
    assert_eq!(
        installed_code(&device.storage, "clock"),
        Some(code_name(&code))
    );
    assert!(!device.storage.exists("plugins/clock/.install-marks"));
}

/// A plugin offered over the cable, its screen open, is not sent over
/// Wi-Fi meanwhile: the two never put the same plugin together.
#[test]
fn a_plugin_offered_over_the_cable_is_not_sent_over_wifi_meanwhile() {
    let mut device = Device::serving(Shared::default());
    let ini = manifest("clock", "1.0.0");
    device.now += 100;
    let effects = device.kernel.open_screen(
        Instant(device.now),
        Id::new_static("transfer.cable"),
        paperpaw_sdk::Params::new(),
    );
    device.settle(effects);
    // As the cable's offer names it.
    let offered = ".paperpaw/plugins/transfer/cable-offer.txt";
    device.storage.write(offered, b"clock").unwrap();
    assert_eq!(
        device.put("clock", "plugin.ini", ini.as_bytes(), ini.as_bytes()),
        409
    );
    assert_eq!(device.request("POST", "/api/plugin/clock", None).0, 409);
    assert!(!device.storage.exists("plugins/.clock.local/plugin.ini"));
    // The cable's screen closed, it may.
    device.run(actions::BACK);
    assert_eq!(
        device.put("clock", "plugin.ini", ini.as_bytes(), ini.as_bytes()),
        200
    );
}
