//! Joplin's set-up screen with the real kernel, by buttons and by touch:
//! the rows each service has, what is typed kept in the settings or as a
//! secret, *Send to Joplin* from it, the status line; the books waiting
//! survive a restart; hostile and late answers of the server leave the
//! notes waiting without a panic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{Effects, HttpCall, HttpResponse, Io, Kernel, WifiEvent};
use paperpaw_platform::{
    InputEvent, Point, PointerEvent, PointerPhase, PointerTool, Rotation, SystemStatus, WallClock,
};
use paperpaw_plugin_joplin::{Joplin, OPEN, SEND};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::builtin::events::{self, NotesExported};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::wifi::WifiState;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

const fn device(touch: bool) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(!touch, touch, false, &[]),
        MemoryClass::Large,
    )
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

const WEBDAV: &str = "[joplin]\nservice = joplin.service.webdav\n\
    address = dav.example/Joplin\nuser = me\n";
const ALICE: &str = "Notes/Lewis Carroll - Alice.md";
const NOTES: &str = "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
    > Curiouser and curiouser!\n>\n> 12 %\n\n";
const WAITING: &str = ".paperpaw/plugins/joplin/waiting.txt";

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Device {
    /// With the password `pw` kept, the time known.
    fn new(config: &str, touch: bool, wifi: bool) -> Self {
        let storage = Shared::default();
        let mut secrets = Secrets::default();
        secrets.apply(SecretChange::keep("joplin.password", "", "pw"));
        storage.write(FILE, secrets.encode().as_bytes()).unwrap();
        storage.write(ALICE, NOTES.as_bytes()).unwrap();
        Self::on(storage, config, touch, wifi)
    }

    fn on(storage: Shared, config: &str, touch: bool, wifi: bool) -> Self {
        let mut kernel = Kernel::builder(device(touch))
            .storage(storage.clone())
            .user_config(config)
            .network(true)
            .wifi(wifi)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Joplin)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.set_status(
            Instant(0),
            SystemStatus::new(
                Some(WallClock::new(Instant(0), 1_792_483_200_000, None)),
                None,
            ),
        );
        let mut device = Self {
            kernel,
            storage,
            now: 0,
        };
        device.calls();
        device
    }

    fn later(&mut self) -> Instant {
        self.now += 100;
        Instant(self.now)
    }

    fn calls(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
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

    fn run(&mut self, action: Id) -> Vec<HttpCall> {
        let now = self.later();
        let effects = self.kernel.run_action(now, action);
        self.settle(effects);
        self.calls()
    }

    /// `text` typed for the field `field` (as the keyboard answers).
    fn typed(&mut self, field: &str, text: &str) -> Vec<HttpCall> {
        let now = self.later();
        let params = Params::new().with("text", text).with("field", field);
        self.kernel
            .send_event(now, Id::new_static("joplin.entered"), params);
        self.calls()
    }

    fn export(&mut self, path: &str) -> Vec<HttpCall> {
        let exported = NotesExported::new(path.to_string(), String::from("Alice"));
        let now = self.later();
        self.kernel.send_event(
            now,
            events::NOTES_EXPORTED.id().clone(),
            exported.to_params(),
        );
        self.calls()
    }

    fn answer(&mut self, call: &HttpCall, status: u16, body: &[u8]) -> Vec<HttpCall> {
        let now = self.later();
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

    fn wifi(&mut self, online: bool) -> Vec<HttpCall> {
        let state = if online {
            WifiState::Online {
                ssid: String::from("home"),
                address: String::from("192.168.1.20"),
            }
        } else {
            WifiState::On
        };
        let now = self.later();
        self.kernel.wifi_event(now, WifiEvent::State(state));
        self.calls()
    }

    /// The labels of what the screen shows.
    fn parts(&mut self) -> Vec<paperpaw_sdk::semantics::Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    fn shown(&mut self) -> String {
        let labels: Vec<String> = self.parts().into_iter().map(|p| p.label).collect();
        labels.join(" ")
    }

    fn focused(&mut self) -> String {
        let parts = self.parts();
        parts
            .into_iter()
            .find(|p| p.focused)
            .map(|p| p.label)
            .unwrap_or_default()
    }

    fn tap_on(&mut self, label: &str) -> Vec<HttpCall> {
        let parts = self.parts();
        let area = paperpaw_sdk::semantics::find(&parts, label)
            .unwrap_or_else(|| panic!("nothing says {label}: {parts:?}"))
            .area;
        let at = Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        );
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let now = self.later();
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

    fn setting(&self, id: &str) -> String {
        self.kernel.settings().text(id).to_string()
    }

    fn password(&self) -> Option<String> {
        self.kernel
            .secrets()
            .get("joplin.password")
            .map(str::to_string)
    }

    fn waiting(&self) -> String {
        String::from_utf8(self.storage.0.get(WAITING).unwrap_or_default()).unwrap()
    }
}

/// On a screen as wide as the X4's, the service row cuts both its title
/// and its value ("Synchronised …, Joplin Cl…") where the whole of either
/// would fit beside the other.
#[test]
fn the_service_row_shows_its_title_and_value_whole() {
    let mut d = Device::new(WEBDAV, false, false);
    d.run(OPEN);
    d.run(actions::CONFIRM);
    let shown = d.shown();
    assert!(shown.contains("Synchronised with, Joplin Cloud"), "{shown}");
}

#[test]
fn the_rows_follow_the_service_chosen_by_buttons() {
    let mut d = Device::new(WEBDAV, false, false);
    d.run(OPEN);
    assert_eq!(
        d.kernel.active_screen().map(Id::as_str),
        Some("joplin.setup")
    );
    let shown = d.shown();
    assert!(
        shown.contains("Synchronised") && shown.contains("WebDAV"),
        "{shown}"
    );
    assert!(shown.contains("Address, dav.example/Joplin"), "{shown}");
    assert!(shown.contains("User name, me"), "{shown}");
    assert!(shown.contains("Password"), "{shown}");
    assert!(shown.contains("Notebook, PaperPaw"), "{shown}");
    assert!(
        shown.contains("Exported notes are sent to Joplin at once."),
        "{shown}"
    );

    // Confirm on the first row goes round the services.
    d.run(actions::CONFIRM);
    assert_eq!(d.setting("joplin.service"), "joplin.service.cloud");
    let shown = d.shown();
    assert!(shown.contains("Joplin Cl"), "{shown}");
    // Joplin Cloud has its address; one signs in with an e-mail.
    assert!(!shown.contains("Address"), "{shown}");
    assert!(shown.contains("E-mail, me"), "{shown}");
    d.run(actions::CONFIRM);
    assert_eq!(d.setting("joplin.service"), "joplin.service.server");
    assert!(d.shown().contains("Address"));
    d.run(actions::CONFIRM);
    assert_eq!(d.setting("joplin.service"), "joplin.service.webdav");
}

#[test]
fn the_focus_stays_on_a_row_when_rows_go() {
    let mut d = Device::new(WEBDAV, false, false);
    d.run(OPEN);
    // Up from the first row: round to the last, "Send to Joplin".
    d.run(actions::UP);
    assert!(d.focused().starts_with("Send to Joplin"), "{}", d.focused());
    // The address row goes with Joplin Cloud: the focus stays on a row.
    let now = d.later();
    d.kernel
        .set_setting(now, "joplin.service", "joplin.service.cloud")
        .unwrap();
    assert!(d.focused().starts_with("Send to Joplin"), "{}", d.focused());
    // The help below follows the focus.
    assert!(d.shown().contains("Exported notes are sent at once."));
}

#[test]
fn what_is_typed_is_kept_trimmed_and_the_password_as_a_secret() {
    let mut d = Device::new(WEBDAV, false, false);
    d.run(OPEN);
    d.typed(
        "address",
        "  nextcloud.example/remote.php/dav/files/me/Joplin  ",
    );
    d.typed("user", " zoë@example.org ");
    d.typed("notebook", "Bücher ✓");
    assert_eq!(
        d.setting("joplin.address"),
        "nextcloud.example/remote.php/dav/files/me/Joplin"
    );
    assert_eq!(d.setting("joplin.user"), "zoë@example.org");
    assert_eq!(d.setting("joplin.notebook"), "Bücher ✓");
    let shown = d.shown();
    assert!(shown.contains("Notebook, Bücher ✓"), "{shown}");

    d.typed("password", " new secret ");
    assert_eq!(d.password().as_deref(), Some("new secret"));
    // The password is no setting.
    let config = String::from_utf8(d.storage.0.get("paperpaw.ini").unwrap_or_default()).unwrap();
    assert!(!config.contains("new secret"), "{config}");
    // Typed empty (confirmed by mistake): kept.
    d.typed("password", "   ");
    assert_eq!(d.password().as_deref(), Some("new secret"));
    assert!(d.shown().contains("Password"));
    // Fields that do not exist change nothing.
    d.typed("service", "joplin.service.cloud");
    d.typed("", "x");
    d.typed("send", "x");
    assert_eq!(d.setting("joplin.service"), "joplin.service.webdav");
}

#[test]
fn typing_moves_on_to_the_next_row() {
    let mut d = Device::new(WEBDAV, false, false);
    d.run(OPEN);
    d.typed("address", "dav.example/Joplin");
    assert!(d.focused().starts_with("User name"), "{}", d.focused());
    d.typed("notebook", "Books");
    assert!(d.focused().starts_with("Send to Joplin"), "{}", d.focused());
}

#[test]
fn by_touch_rows_ask_and_send_now_sends() {
    let mut d = Device::new(WEBDAV, true, false);
    d.storage
        .write(WAITING, format!("{ALICE}\n").as_bytes())
        .unwrap();
    d.run(OPEN);
    assert!(d.shown().contains("The notes of 1 book wait to be sent."));
    d.tap_on("Synchronised");
    assert_eq!(d.setting("joplin.service"), "joplin.service.cloud");
    d.tap_on("Synchronised");
    d.tap_on("Synchronised");
    assert_eq!(d.setting("joplin.service"), "joplin.service.webdav");
    let calls = d.tap_on("Send to Joplin");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].url, "https://dav.example/Joplin/info.json");
    // Unreachable: said on the status line, the book still waits.
    assert!(d.answer(&calls[0], 0, b"refused").is_empty());
    let shown = d.shown();
    assert!(shown.contains("Joplin cannot be reached."), "{shown}");
    assert_eq!(d.waiting(), format!("{ALICE}\n"));
}

#[test]
fn nothing_waiting_is_said_when_sending() {
    let mut d = Device::new(WEBDAV, false, false);
    d.run(OPEN);
    assert!(d.run(SEND).is_empty());
    assert!(d.shown().contains("No notes wait to be sent to Joplin."));
}

#[test]
fn books_waiting_survive_a_restart_and_go_when_wifi_comes() {
    let mut d = Device::new(WEBDAV, false, true);
    d.wifi(true);
    let calls = d.export(ALICE);
    assert!(d.answer(&calls[0], 503, b"").is_empty());
    assert_eq!(d.waiting(), format!("{ALICE}\n"));
    let storage = d.storage.clone();
    drop(d);

    // Started again on the same storage: Wi-Fi coming sends what waits.
    let mut d = Device::on(storage, WEBDAV, false, true);
    assert!(d.wifi(false).is_empty());
    let calls = d.wifi(true);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].url, "https://dav.example/Joplin/info.json");
}

#[test]
fn a_book_exported_again_waits_once() {
    let mut d = Device::new(WEBDAV, false, false);
    for _ in 0..3 {
        let calls = d.export(ALICE);
        for call in calls {
            d.answer(&call, 0, b"");
        }
    }
    assert_eq!(d.waiting(), format!("{ALICE}\n"));
}

#[test]
fn hostile_info_files_leave_the_notes_waiting() {
    // Nested, but not so deep that reading it takes long (see the test
    // below).
    let deep = format!("{}{}", "[".repeat(500), "]".repeat(500));
    let bodies: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"not json".to_vec(),
        b"{\"version\":".to_vec(),
        br#"{"version": "3"}"#.to_vec(),
        br#"{"version": 3.5, "e2ee": {"value": false}}"#.to_vec(),
        br#"{"version": 99999999999999999999, "e2ee": {"value": false}}"#.to_vec(),
        br#"{"version": -3, "e2ee": {"value": false}}"#.to_vec(),
        br#"{"version": 3, "e2ee": {"value": "true"}}"#.to_vec(),
        br#"{"version": 3, "e2ee": true}"#.to_vec(),
        deep.into_bytes(),
        vec![0xff, 0xfe, b'{', 0x80, b'}'],
    ];
    for body in bodies {
        let label = String::from_utf8_lossy(&body[..body.len().min(40)]).into_owned();
        let mut d = Device::new(WEBDAV, false, false);
        d.run(OPEN);
        let calls = d.export(ALICE);
        let after = d.answer(&calls[0], 200, &body);
        // Either stopped with a message, or on to the locks; never a
        // note written to a target it does not understand.
        assert!(
            after.iter().all(|c| c.method != "PUT"),
            "{label}: {after:?}"
        );
        if after.is_empty() {
            assert_eq!(d.waiting(), format!("{ALICE}\n"), "{label}");
            assert!(d.shown().contains("Joplin"), "{label}");
            assert!(
                !d.storage.exists(".paperpaw/plugins/joplin/info.json"),
                "{label}: the answer is not left behind"
            );
        }
    }
}

#[test]
fn a_deeply_nested_info_file_is_read_quickly() {
    // 100 KB: a server, or anyone between, can answer with this.
    let deep = format!("{}{}", "[".repeat(50_000), "]".repeat(50_000));
    let mut d = Device::new(WEBDAV, false, false);
    let calls = d.export(ALICE);
    let started = std::time::Instant::now();
    d.answer(&calls[0], 200, deep.as_bytes());
    let took = started.elapsed();
    assert!(took.as_secs() < 5, "took {took:?}");
    assert_eq!(d.waiting(), format!("{ALICE}\n"));
}

#[test]
fn an_answer_twice_or_after_the_end_changes_nothing() {
    let mut d = Device::new(WEBDAV, false, false);
    let calls = d.export(ALICE);
    let first = calls[0].clone();
    assert!(d.answer(&first, 0, b"").is_empty());
    // The same answer again: the sending ended already.
    assert!(d.answer(&first, 200, b"{\"version\": 3}").is_empty());
    assert_eq!(d.waiting(), format!("{ALICE}\n"));
    // A reply for a request never made.
    let made_up = HttpCall { id: 9999, ..first };
    assert!(d.answer(&made_up, 200, b"{}").is_empty());
}
