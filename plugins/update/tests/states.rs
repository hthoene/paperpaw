//! The update page's states with the real kernel, by buttons and by
//! touch: installing with its bar (no way out half way), installed and
//! the restart, failed, an own build whose key the reader accepts at
//! each install, files for another device; what is said after the restart;
//! an update offered once at start (also as version 1 of `offered.txt`
//! has it); hostile and late answers of the update server; versions with
//! pre-releases.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::{InputEvent, Point, PointerEvent, PointerPhase, PointerTool, Rotation};
use paperpaw_plugin_update::{CHECK, OPEN, Update};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::firmware::{
    Firmware, FirmwareCommand, FirmwareState, InstallFailure, PLUGINS_AT, PluginCrate, Plugins,
};
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DirEntry, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, PixelFormat, Plugin, Priority, RegisterError, Registrar, Screen,
    ScreenSpec, Storage, StorageError, Version,
};

const fn device(touch: bool) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(!touch, touch, false, &[]),
        MemoryClass::Small,
    )
}

/// PaperPaw's key, built into the firmware.
const SEED: [u8; 32] = [3; 32];
/// Someone's own key.
const OWN: [u8; 32] = [7; 32];
const CHIP: u16 = paperpaw_sdk::firmware::CHIP_ESP32_C3;
const PROJECT: &str = "paperpaw-xteink";
const PAGE: &str = "update.page";
const CARD: &str = "updates/paperpaw-x4-0.5.0.bin";
const DIR: &str = ".paperpaw/plugins/update";

/// A signed update file of `version` for `project`, signed with `seed`.
fn update_file(project: &str, version: &str, seed: &[u8; 32]) -> Vec<u8> {
    let mut image = vec![0u8; 2000];
    image[0] = 0xE9;
    image[12..14].copy_from_slice(&CHIP.to_le_bytes());
    image[32..36].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
    image[48..48 + version.len()].copy_from_slice(version.as_bytes());
    image[80..80 + project.len()].copy_from_slice(project.as_bytes());
    let list = Plugins::record([PluginCrate {
        name: "paperpaw-plugin-stats",
        version,
    }])
    .unwrap();
    image[PLUGINS_AT..PLUGINS_AT + list.len()].copy_from_slice(&list);
    let trailer = paperpaw_update::sign(&image, seed).unwrap();
    image.extend_from_slice(&trailer.to_bytes());
    image
}

/// A storage the test looks into while the kernel has it.
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
}

/// Short messages, as a notice screen would show them.
struct Notices(Rc<RefCell<Vec<String>>>);

struct Notice;

impl Screen for Notice {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Notices {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("notices"), "Notices", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let said = self.0.clone();
        let id = Id::new_static("notices.notice");
        r.add_screen(ScreenSpec::new(
            id.clone(),
            "",
            Box::new(move |_, p| {
                said.borrow_mut()
                    .push(p.get(params::TEXT).unwrap_or_default().to_string());
                Box::new(Notice)
            }),
        ))?;
        r.bind_slot(slots::NOTICE, id, Priority::DEFAULT)
    }
}

struct Options {
    version: &'static str,
    touch: bool,
    undone: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            version: "0.4.0",
            touch: false,
            undone: false,
        }
    }
}

struct Device {
    kernel: Kernel,
    storage: Shared,
    said: Rc<RefCell<Vec<String>>>,
    now: u64,
}

impl Device {
    fn new(storage: Shared, options: Options) -> Self {
        let firmware = Firmware::new(options.version, PROJECT, CHIP)
            .keys(vec![paperpaw_update::public_key(&SEED)])
            .room(1_000_000)
            .undone(options.undone)
            .plugins(Plugins::new("paperpaw-plugin-stats 0.4.0\n"));
        let said = Rc::default();
        let mut kernel = Kernel::builder(device(options.touch))
            .storage(storage.clone())
            .network(true)
            .firmware(firmware)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Update)
            .plugin(Notices(Rc::clone(&said)))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            storage,
            said,
            now: 0,
        }
    }

    /// Running 0.4.0, with `file` on the card at [`CARD`] (offered at
    /// a start before, so the page is not opened by itself).
    fn with_card(file: &[u8], touch: bool) -> Self {
        let storage = Shared::default();
        storage.write(CARD, file).unwrap();
        storage
            .write(
                &format!("{DIR}/offered.txt"),
                format!("{CARD}\n0.5.0\n").as_bytes(),
            )
            .unwrap();
        Self::new(
            storage,
            Options {
                touch,
                ..Options::default()
            },
        )
    }

    fn later(&mut self, millis: u64) -> Instant {
        self.now += millis;
        Instant(self.now)
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..100 {
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
        let now = self.later(100);
        let effects = self.kernel.run_action(now, action);
        self.settle(effects);
    }

    fn io(&mut self) -> Vec<Io> {
        self.kernel.take_io()
    }

    fn calls(&mut self) -> Vec<HttpCall> {
        self.io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn firmware_commands(&mut self) -> Vec<FirmwareCommand> {
        self.io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Firmware(command) => Some(command),
                _ => None,
            })
            .collect()
    }

    fn answer(&mut self, call: &HttpCall, status: u16, body: &[u8]) {
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
    }

    fn state(&mut self, state: FirmwareState) {
        let now = self.later(100);
        let effects = self.kernel.firmware_state(now, state);
        self.settle(effects);
    }

    fn parts(&mut self) -> Vec<paperpaw_sdk::semantics::Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    fn shown(&mut self) -> String {
        let labels: Vec<String> = self.parts().into_iter().map(|p| p.label).collect();
        labels.join(" | ")
    }

    fn focused(&mut self) -> Option<String> {
        self.parts()
            .into_iter()
            .find(|p| p.focused)
            .map(|p| p.label)
    }

    fn screen(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }

    fn tap_on(&mut self, label: &str) {
        let parts = self.parts();
        let area = paperpaw_sdk::semantics::find(&parts, label)
            .unwrap_or_else(|| panic!("nothing says {label}: {parts:?}"))
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
    }

    fn file(&self, name: &str) -> Option<String> {
        self.storage
            .0
            .get(&format!("{DIR}/{name}"))
            .map(|b| String::from_utf8(b).unwrap())
    }
}

#[test]
fn installing_shows_its_bar_and_cannot_be_left() {
    let mut d = Device::with_card(&update_file(PROJECT, "0.5.0", &SEED), false);
    d.run(OPEN);
    d.io();
    let shown = d.shown();
    assert!(shown.contains("PaperPaw 0.5.0"), "{shown}");
    assert!(shown.contains("Installed now: PaperPaw 0.4.0"), "{shown}");
    assert_eq!(d.focused().as_deref(), Some("Install"));
    d.run(actions::CONFIRM);
    assert_eq!(
        d.firmware_commands(),
        [FirmwareCommand::install(CARD.to_string(), None, false)]
    );
    // Kept through the restart, to say how it went.
    assert_eq!(
        d.file("installing.txt").as_deref(),
        Some(&*format!("{CARD}\n0.5.0\n"))
    );
    let shown = d.shown();
    assert!(shown.contains("Installing the update"), "{shown}");
    assert!(shown.contains("0 %"), "{shown}");

    d.state(FirmwareState::Installing {
        done: 750,
        total: 1000,
    });
    assert!(d.shown().contains("75 %"));
    // No way out half way, by any button.
    for action in [actions::BACK, actions::DOWN, actions::CONFIRM] {
        d.run(action);
        assert_eq!(d.screen(), Some(PAGE));
    }
    assert!(d.firmware_commands().is_empty());

    d.state(FirmwareState::Installed);
    let shown = d.shown();
    assert!(shown.contains("Update installed"), "{shown}");
    assert!(shown.contains("Restarting"), "{shown}");
    // The restart, a moment later.
    let now = d.later(2000);
    d.kernel.tick(now);
    assert_eq!(d.firmware_commands(), [FirmwareCommand::Restart]);
}

#[test]
fn a_progress_beyond_its_total_shows_at_most_all() {
    let mut d = Device::with_card(&update_file(PROJECT, "0.5.0", &SEED), false);
    d.run(OPEN);
    d.run(actions::CONFIRM);
    d.state(FirmwareState::Installing {
        done: 5000,
        total: 1000,
    });
    assert!(d.shown().contains("100 %"));
    d.state(FirmwareState::Installing { done: 10, total: 0 });
    assert!(d.shown().contains("0 %"));
}

#[test]
fn a_failed_install_says_why_and_closes() {
    let mut d = Device::with_card(&update_file(PROJECT, "0.5.0", &SEED), false);
    d.run(OPEN);
    d.run(actions::CONFIRM);
    d.state(FirmwareState::Failed(InstallFailure::Damaged));
    let shown = d.shown();
    assert!(shown.contains("Update not installed"), "{shown}");
    assert!(shown.contains("changed or damaged"), "{shown}");
    assert!(shown.contains("PaperPaw 0.4.0 stays"), "{shown}");
    d.run(actions::CONFIRM);
    assert_ne!(d.screen(), Some(PAGE));
}

/// The battery's charge, as the shell tells it.
fn battery(d: &mut Device, percent: u8, charging: bool) {
    let now = d.later(10);
    let status = paperpaw_sdk::SystemStatus::default()
        .with_battery(Some(paperpaw_sdk::Battery::new(percent, charging, None)));
    let effects = d.kernel.set_status(now, status);
    d.settle(effects);
}

#[test]
fn below_20_percent_and_not_charging_nothing_is_installed() {
    let mut d = Device::with_card(&update_file(PROJECT, "0.5.0", &SEED), false);
    battery(&mut d, 19, false);
    d.run(OPEN);
    d.run(actions::CONFIRM);
    assert!(d.firmware_commands().is_empty(), "the kernel refused it");
    let shown = d.shown();
    assert!(
        shown.contains("Charge to 20 % or plug in to install"),
        "{shown}"
    );
    assert!(shown.contains("PaperPaw 0.4.0 stays"), "{shown}");
    // Plugged in: it installs.
    d.run(actions::CONFIRM);
    battery(&mut d, 19, true);
    d.run(OPEN);
    d.run(actions::CONFIRM);
    assert_eq!(
        d.firmware_commands(),
        [FirmwareCommand::install(CARD.to_string(), None, false)]
    );
}

#[test]
fn from_20_percent_it_installs_on_battery() {
    let mut d = Device::with_card(&update_file(PROJECT, "0.5.0", &SEED), false);
    battery(&mut d, 20, false);
    d.run(OPEN);
    d.run(actions::CONFIRM);
    assert_eq!(d.firmware_commands().len(), 1);
}

#[test]
fn an_own_build_installs_only_with_its_key_accepted() {
    let mut d = Device::with_card(&update_file(PROJECT, "0.5.0", &OWN), false);
    d.run(OPEN);
    let shown = d.shown();
    assert!(shown.contains("PaperPaw 0.5.0, own build"), "{shown}");
    assert!(shown.contains("Install with this key"), "{shown}");
    // "Not now": nothing installed.
    d.run(actions::DOWN);
    assert_eq!(d.focused().as_deref(), Some("Not now"));
    d.run(actions::CONFIRM);
    assert_ne!(d.screen(), Some(PAGE));
    assert!(d.firmware_commands().is_empty());

    // Accepted: the install names the key shown.
    d.run(OPEN);
    d.run(actions::CONFIRM);
    assert_eq!(
        d.firmware_commands(),
        [FirmwareCommand::install(
            CARD.to_string(),
            Some(paperpaw_update::public_key(&OWN)),
            false
        )]
    );
    // Asked again at the next install: nothing on the card trusts a key.
    d.state(FirmwareState::Failed(InstallFailure::Write));
    d.run(actions::CONFIRM);
    d.run(OPEN);
    let shown = d.shown();
    assert!(shown.contains("own build"), "{shown}");
    assert!(shown.contains("Install with this key"), "{shown}");
}

#[test]
fn files_that_do_not_fit_say_why() {
    let cases: [(&[u8], &str); 3] = [
        (&update_file("other", "0.5.0", &SEED), "for another device"),
        (&[0xE9; 4096], "no update"),
        (b"", "no update"),
    ];
    for (file, said) in cases {
        let mut d = Device::with_card(file, false);
        d.run(OPEN);
        let shown = d.shown();
        assert!(shown.contains("This update cannot be installed"), "{shown}");
        assert!(shown.contains(said), "{said}: {shown}");
        assert_eq!(d.focused().as_deref(), Some("OK"));
    }
}

#[test]
fn by_touch_the_buttons_are_tapped() {
    let mut d = Device::new(
        Shared::default(),
        Options {
            touch: true,
            ..Options::default()
        },
    );
    d.run(OPEN);
    // No focus shows on a touch screen.
    assert_eq!(d.focused(), None);
    d.tap_on("Search online");
    let calls = d.calls();
    assert_eq!(calls.len(), 1);
    let file = update_file(PROJECT, "0.5.0", &SEED);
    let offer = format!(
        r#"{{"version": "0.5.0", "url": "/api/v1/updates/u.bin", "size": {},
            "plugins": ["paperpaw-plugin-stats 0.5.0"]}}"#,
        file.len()
    );
    d.answer(&calls[0], 200, offer.as_bytes());
    d.tap_on("Download");
    let calls = d.calls();
    assert_eq!(calls[0].url, "https://paperpaw.org/api/v1/updates/u.bin");
    d.answer(&calls[0], 200, &file);
    assert!(d.shown().contains("Install"));
    d.tap_on("Not now");
    assert_ne!(d.screen(), Some(PAGE));
}

#[test]
fn after_the_restart_the_update_is_said_and_its_file_removed() {
    let storage = Shared::default();
    storage
        .write(CARD, &update_file(PROJECT, "0.5.0", &SEED))
        .unwrap();
    storage
        .write(
            &format!("{DIR}/installing.txt"),
            format!("{CARD}\n0.5.0\n").as_bytes(),
        )
        .unwrap();
    let d = Device::new(
        storage.clone(),
        Options {
            version: "0.5.0",
            ..Options::default()
        },
    );
    assert_eq!(d.said.borrow().as_slice(), ["Updated to PaperPaw 0.5.0"]);
    assert!(!storage.exists(CARD));
    assert!(d.file("installing.txt").is_none());
}

#[test]
fn an_update_that_did_not_start_is_said_and_kept() {
    let storage = Shared::default();
    storage
        .write(CARD, &update_file(PROJECT, "0.5.0", &SEED))
        .unwrap();
    storage
        .write(
            &format!("{DIR}/installing.txt"),
            format!("{CARD}\n0.5.0\n").as_bytes(),
        )
        .unwrap();
    let d = Device::new(
        storage.clone(),
        Options {
            undone: true,
            ..Options::default()
        },
    );
    assert_eq!(
        d.said.borrow().as_slice(),
        ["The update did not start: PaperPaw 0.4.0 is back."]
    );
    assert!(storage.exists(CARD));
    assert!(d.file("installing.txt").is_none());
    // Not offered at once again.
    assert_ne!(d.screen(), Some(PAGE));
}

#[test]
fn an_update_on_the_card_is_offered_once_at_start() {
    let storage = Shared::default();
    storage
        .write(CARD, &update_file(PROJECT, "0.5.0", &SEED))
        .unwrap();
    let d = Device::new(storage.clone(), Options::default());
    assert_eq!(d.screen(), Some(PAGE));
    assert_eq!(
        d.file("offered.txt").as_deref(),
        Some(&*format!("{CARD}\n0.5.0\n"))
    );
    // Started again: not offered again.
    let d = Device::new(storage, Options::default());
    assert_ne!(d.screen(), Some(PAGE));
}

/// What version 1 wrote stays readable (see `docs/data-files.md`): the
/// update offered before is not offered again.
#[test]
fn offered_of_version_1_is_read() {
    let storage = Shared::default();
    storage
        .write(CARD, &update_file(PROJECT, "0.5.0", &SEED))
        .unwrap();
    storage
        .write(
            &format!("{DIR}/offered.txt"),
            include_bytes!("../testdata/offered.txt.v1"),
        )
        .unwrap();
    let d = Device::new(storage, Options::default());
    assert_ne!(d.screen(), Some(PAGE));
    // A newer one than offered is.
    let storage = Shared::default();
    storage
        .write(
            "updates/paperpaw-x4-0.6.0.bin",
            &update_file(PROJECT, "0.6.0", &SEED),
        )
        .unwrap();
    storage
        .write(
            &format!("{DIR}/offered.txt"),
            include_bytes!("../testdata/offered.txt.v1"),
        )
        .unwrap();
    let d = Device::new(storage, Options::default());
    assert_eq!(d.screen(), Some(PAGE));
}

#[test]
fn hostile_answers_of_the_server_offer_nothing_or_something_sane() {
    let deep = format!("{}{}", "[".repeat(500), "]".repeat(500));
    let bodies: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"not json".to_vec(),
        br#"{"version": 5, "url": "u"}"#.to_vec(),
        br#"{"version": "0.5.0", "url": 7}"#.to_vec(),
        br#"{"version": "v9.9.9", "url": "u"}"#.to_vec(),
        br#"{"version": "0.5.0", "url": "  "}"#.to_vec(),
        deep.into_bytes(),
        vec![0xff, 0xfe, b'{', 0x80],
    ];
    for body in bodies {
        let label = String::from_utf8_lossy(&body[..body.len().min(40)]).into_owned();
        let mut d = Device::new(Shared::default(), Options::default());
        d.run(OPEN);
        d.run(actions::CONFIRM);
        let calls = d.calls();
        d.answer(&calls[0], 200, &body);
        let shown = d.shown();
        assert!(shown.contains("PaperPaw is up to date"), "{label}: {shown}");
        assert!(d.file("offer.txt").is_none(), "{label}");
    }
    // Numbers out of range, plugins of the wrong kind: offered, sane.
    let mut d = Device::new(Shared::default(), Options::default());
    d.run(OPEN);
    d.run(actions::CONFIRM);
    let calls = d.calls();
    let body = br#"{"version": "0.5.0", "url": "https://x/u.bin", "size": -12,
        "plugins": [1, null, {"a": 1}], "missing": "not a list"}"#;
    d.answer(&calls[0], 200, body);
    let shown = d.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    // No size known: none said.
    assert!(shown.contains("Downloading the update takes"), "{shown}");
    // What it lacks of what runs here is said.
    assert!(shown.contains("cannot come along"), "{shown}");
    let mut d = Device::new(Shared::default(), Options::default());
    d.run(OPEN);
    d.run(actions::CONFIRM);
    let calls = d.calls();
    d.answer(
        &calls[0],
        200,
        br#"{"version": "0.5.0", "url": "u", "size": 1e30, "plugins": ["paperpaw-plugin-stats 0.5.0"]}"#,
    );
    assert!(d.shown().contains("PaperPaw 0.5.0 is available"));
}

#[test]
fn an_answer_after_the_page_closed_is_kept_for_it() {
    let mut d = Device::new(Shared::default(), Options::default());
    d.run(OPEN);
    d.run(actions::CONFIRM);
    let calls = d.calls();
    d.run(actions::BACK);
    assert_ne!(d.screen(), Some(PAGE));
    d.answer(
        &calls[0],
        200,
        br#"{"version": "0.5.0", "url": "u", "plugins": ["paperpaw-plugin-stats 0.5.0"]}"#,
    );
    // Asked on the page, not in the background: not opened by itself.
    assert_ne!(d.screen(), Some(PAGE));
    d.run(OPEN);
    assert!(d.shown().contains("PaperPaw 0.5.0 is available"));
    // The same answer again changes nothing.
    d.answer(&calls[0], 200, b"{}");
    assert!(d.shown().contains("PaperPaw 0.5.0 is available"));
}

#[test]
fn a_download_ending_with_the_page_closed_is_said() {
    let mut d = Device::new(Shared::default(), Options::default());
    d.run(CHECK);
    let calls = d.calls();
    let file = update_file(PROJECT, "0.5.0", &SEED);
    let offer = format!(
        r#"{{"version": "0.5.0", "url": "https://x/u.bin", "size": {},
            "plugins": ["paperpaw-plugin-stats 0.5.0"]}}"#,
        file.len()
    );
    d.answer(&calls[0], 200, offer.as_bytes());
    // Offered by opening the page.
    assert_eq!(d.screen(), Some(PAGE));
    d.run(actions::CONFIRM);
    let calls = d.calls();
    d.run(actions::BACK);
    d.answer(&calls[0], 200, &file);
    assert_eq!(
        d.said.borrow().last().map(String::as_str),
        Some("Update downloaded: PaperPaw 0.5.0, ready to install under Update")
    );
    assert!(d.storage.exists("updates/paperpaw-xteink-0.5.0.bin"));
    assert!(d.file("offer.txt").is_none());
    // A failed one too, and nothing is left of it.
    let mut d = Device::new(Shared::default(), Options::default());
    d.run(CHECK);
    let calls = d.calls();
    d.answer(&calls[0], 200, offer.as_bytes());
    d.run(actions::CONFIRM);
    let calls = d.calls();
    d.run(actions::BACK);
    d.answer(&calls[0], 503, b"busy");
    assert_eq!(
        d.said.borrow().last().map(String::as_str),
        Some("The update could not be downloaded")
    );
    assert!(!d.storage.exists("updates/.download"));
}

/// The server's answer naming `version`.
fn offering(version: &str) -> String {
    format!(
        r#"{{"version": "{version}", "url": "u", "plugins": ["paperpaw-plugin-stats {version}"]}}"#
    )
}

/// Whether a device running `running` is offered `offered`.
fn offered(running: &'static str, offered: &str) -> bool {
    let mut d = Device::new(
        Shared::default(),
        Options {
            version: running,
            ..Options::default()
        },
    );
    d.run(OPEN);
    d.run(actions::CONFIRM);
    let calls = d.calls();
    d.answer(&calls[0], 200, offering(offered).as_bytes());
    d.shown().contains("is available")
}

#[test]
fn releases_come_after_their_pre_releases() {
    assert!(offered("0.5.0-dev", "0.5.0"));
    assert!(offered("0.5.0-rc.1", "0.5.0"));
    assert!(offered("0.5.0-alpha", "0.5.0-beta"));
    assert!(offered("0.9.0", "0.10.0"));
    assert!(offered("0.5.0+build.7", "0.5.1"));
    assert!(!offered("0.5.0", "0.5.0-rc.1"));
    assert!(!offered("0.5.0", "0.5.0+build.8"));
    assert!(!offered("0.10.0", "0.9.9"));
}

#[test]
fn pre_release_numbers_compare_as_numbers() {
    assert!(offered("0.5.0-rc.9", "0.5.0-rc.10"));
    assert!(offered("0.5.0-beta.2", "0.5.0-beta.11"));
}
