//! The Wi-Fi page with the real kernel managing the radio, by touch and by
//! buttons only: networks with odd names (empty, Unicode, the longest of
//! 32 bytes, the same name twice), joining a secured network with a
//! password typed on the keyboard, a wrong password, an open network,
//! forgetting a network, and that the password is kept among the secrets
//! once joined, never in the configuration and never in clear.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{
    Effects, Io, JoinFailure, Kernel, WifiCommand, WifiEvent, WifiNetwork, WifiState,
};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat, Point,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_plugin_wifi::Wifi;
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::semantics::{self, Part, Role};
use paperpaw_sdk::{
    ActionSpec, Canvas, Command, Commands, Cx, DirEntry, Env, Handled, HttpRequest, Id, Manifest,
    MemoryStorage, Params, Plugin, Priority, RegisterError, Registrar, Screen, ScreenSpec, Storage,
    StorageError, Version,
};

/// An X4 as the emulator's profile has it: buttons, no touch.
const BUTTONS: DeviceInfo = DeviceInfo::new(
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

/// A touch screen as large as the X4's.
const TOUCH: DeviceInfo = BUTTONS
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

/// The longest name a network can have: 32 bytes.
const LONGEST: &str = "abcdefghijklmnopqrstuvwxyz012345";
/// As much of it as a row shows (the rest cut with an ellipsis).
const LONGEST_SHOWN: &str = "abcdefghijklmnop";
const UNICODE: &str = "Café ☕ Netz";

/// A storage the test looks into while kernels have it.
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

impl Shared {
    /// Whether `text` is in any file as it is.
    fn contains(&self, text: &str) -> bool {
        let mut dirs = vec![String::new()];
        while let Some(dir) = dirs.pop() {
            let mut entries = Vec::new();
            let _ = self
                .0
                .list(&dir, &mut |e| entries.push((e.name.to_string(), e.is_dir)));
            for (name, is_dir) in entries {
                let path = if dir.is_empty() {
                    name
                } else {
                    format!("{dir}/{name}")
                };
                if is_dir {
                    dirs.push(path);
                } else if let Some(data) = self.0.get(&path)
                    && data.windows(text.len()).any(|w| w == text.as_bytes())
                {
                    return true;
                }
            }
        }
        false
    }
}

/// Makes a request, which needs the network.
const FETCH: Id = Id::new_static("notices.fetch");

/// The notices shown, in order.
type Notices = Rc<RefCell<Vec<String>>>;

/// Shows notices by keeping their texts.
struct NoticeKeeper(Notices);

struct Notice;

impl Screen for Notice {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for NoticeKeeper {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("notices"), "Notices", Version::new(0, 1, 0))
            .description("Keeps the notices.")
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let notices = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("notices.screen"),
            "Notice",
            Box::new(move |_, params| {
                let text = params.get(params::TEXT).unwrap_or_default();
                notices.borrow_mut().push(text.to_string());
                Box::new(Notice)
            }),
        ))?;
        r.bind_slot(
            slots::NOTICE,
            Id::new_static("notices.screen"),
            Priority::DEFAULT,
        )?;
        // Wants the network: a request.
        r.add_action(ActionSpec::new(FETCH, "Fetch").handler(
            |_: &Env<'_>, _, out: &mut Commands| {
                out.push(Command::http(HttpRequest::new(
                    "GET",
                    "http://example.org/",
                    Id::new_static("notices.fetched"),
                )));
            },
        ))
    }
}

fn network(ssid: &str, signal: i8, secured: bool) -> WifiNetwork {
    WifiNetwork::new(ssid.to_string(), signal, secured)
}

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
    /// What the radio was told, in order.
    told: Vec<WifiCommand>,
    /// The requests that went out to the network.
    requests: Vec<u32>,
    notices: Notices,
}

impl Device {
    fn new(device: DeviceInfo) -> Self {
        Self::on(device, Shared::default())
    }

    fn on(device: DeviceInfo, storage: Shared) -> Self {
        let notices = Notices::default();
        let kernel = Kernel::builder(device)
            .storage(storage.clone())
            .user_config("[core]\nlanguage = en\n")
            .wifi(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Keyboard)
            .plugin(Wifi)
            .plugin(NoticeKeeper(notices.clone()))
            .build()
            .unwrap();
        let mut device = Self {
            kernel,
            storage,
            now: 0,
            told: Vec::new(),
            requests: Vec::new(),
            notices,
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            self.collect();
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

    fn collect(&mut self) {
        for io in self.kernel.take_io() {
            match io {
                Io::Wifi(command) => self.told.push(command),
                Io::Http(call) => self.requests.push(call.id),
                _ => {}
            }
        }
    }

    /// What the radio was told since last asked.
    fn told(&mut self) -> Vec<WifiCommand> {
        self.collect();
        std::mem::take(&mut self.told)
    }

    /// The radio says what happened.
    fn radio(&mut self, event: WifiEvent) {
        self.now += 100;
        let effects = self.kernel.wifi_event(Instant(self.now), event);
        self.settle(effects);
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn on_top(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }

    fn open(&mut self) {
        self.now += 100;
        let effects = self.kernel.open_screen(
            Instant(self.now),
            Id::new_static("wifi.page"),
            Params::new(),
        );
        self.settle(effects);
        assert_eq!(self.on_top(), Some("wifi.page"));
    }

    fn parts(&mut self) -> Vec<Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    /// The rows of the page, as they say.
    fn rows(&mut self) -> Vec<String> {
        self.parts()
            .into_iter()
            .filter(|p| p.role == Role::Row)
            .map(|p| p.label)
            .collect()
    }

    fn row(&mut self, start: &str) -> Part {
        self.parts()
            .into_iter()
            .find(|p| matches!(p.role, Role::Row | Role::Button) && p.label.starts_with(start))
            .unwrap_or_else(|| panic!("no row {start:?}"))
    }

    fn finger(&mut self, phase: PointerPhase, point: Point) {
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

    fn center(part: &Part) -> Point {
        Point::new(
            i32::from(part.area.x) + i32::from(part.area.width) / 2,
            i32::from(part.area.y) + i32::from(part.area.height) / 2,
        )
    }

    fn tap(&mut self, start: &str) {
        let at = Self::center(&self.row(start));
        self.now += 1000;
        self.finger(PointerPhase::Down, at);
        self.finger(PointerPhase::Up, at);
    }

    fn hold(&mut self, start: &str) {
        let at = Self::center(&self.row(start));
        self.now += 1000;
        self.finger(PointerPhase::Down, at);
        self.now += 1500;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
        self.finger(PointerPhase::Up, at);
    }

    /// Moves the buttons' focus with *Next* to what begins with `start`.
    fn focus(&mut self, start: &str) {
        for _ in 0..80 {
            let parts = self.parts();
            if semantics::focused(&parts).is_some_and(|p| p.label.starts_with(start)) {
                return;
            }
            self.run(actions::NEXT);
        }
        panic!("nothing to focus begins with {start:?}");
    }

    /// Types `text` on the keyboard (lower case letters and digits) and
    /// finishes, by touch or by buttons.
    fn type_text(&mut self, text: &str) {
        assert_eq!(self.on_top(), Some("keyboard.screen"));
        let touch = self.kernel.device().input.touch;
        let keys: Vec<String> = text.chars().map(String::from).collect();
        let mut digits = false;
        for key in keys.iter().map(String::as_str).chain(["OK"]) {
            let digit = key.chars().all(|c| c.is_ascii_digit());
            if digit != digits && key != "OK" {
                let switch = if digit { "123" } else { "ABC" };
                self.press_key(switch, touch);
                digits = digit;
            }
            self.press_key(key, touch);
        }
    }

    fn press_key(&mut self, key: &str, touch: bool) {
        if touch {
            let part = self
                .parts()
                .into_iter()
                .find(|p| p.role == Role::Button && p.label == key)
                .unwrap_or_else(|| panic!("no key {key:?}"));
            let at = Self::center(&part);
            self.now += 1000;
            self.finger(PointerPhase::Down, at);
            self.finger(PointerPhase::Up, at);
        } else {
            self.focus(key);
            self.run(actions::CONFIRM);
        }
    }

    /// The networks kept among the secrets: names and passwords.
    fn saved(&self) -> Vec<(String, String)> {
        paperpaw_sdk::wifi::known(self.kernel.secrets().iter())
    }

    /// Switches the radio on from the page and lets it find `networks`.
    fn radio_on_with(&mut self, networks: Vec<WifiNetwork>) {
        if self.kernel.device().input.touch {
            self.tap("Wi-Fi");
        } else {
            self.focus("Wi-Fi");
            self.run(actions::CONFIRM);
        }
        assert_eq!(self.told(), [WifiCommand::On]);
        self.radio(WifiEvent::State(WifiState::On));
        // Switched on here: the page looks for networks.
        assert_eq!(self.told(), [WifiCommand::Scan]);
        self.radio(WifiEvent::Scanned(networks));
    }

    fn says(&mut self, text: &str) -> bool {
        self.parts().iter().any(|p| p.label.contains(text))
    }
}

fn odd_networks() -> Vec<WifiNetwork> {
    vec![
        network("", -40, true),
        network(UNICODE, -60, true),
        network(LONGEST, -70, false),
        network("Home", -80, true),
        network("Home", -50, true),
        network("  spaced  ", -90, true),
    ]
}

#[test]
fn odd_network_names_are_listed_strongest_first_each_once() {
    let mut device = Device::new(TOUCH);
    device.open();
    assert!(device.rows()[0].starts_with("Wi-Fi, Off"));
    device.radio_on_with(odd_networks());
    let rows = device.rows();
    let names: Vec<&str> = rows
        .iter()
        .skip(1)
        .take(4)
        .map(|r| r.split(", ").next().unwrap())
        .collect();
    // No network without a name; "Home" once, at its strongest.
    // The longest name smaller where the row needs it, shortened only
    // past the small size (how far depends on the look's margins).
    assert_eq!(names.len(), 4, "{rows:?}");
    assert_eq!(
        [names[0], names[1], names[3]],
        ["Home", UNICODE, "spaced"],
        "{rows:?}"
    );
    assert!(names[2].starts_with("abcdefghijklmnop"), "{rows:?}");
    assert!(rows[1].contains("Excellent signal"), "{rows:?}");
    assert!(rows[3].contains("Open, no password"), "{rows:?}");
}

#[test]
fn a_secured_network_joined_with_a_password_by_touch() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.radio_on_with(odd_networks());
    device.tap(UNICODE);
    // Asked for the password on the keyboard, as dots.
    assert_eq!(device.on_top(), Some("keyboard.screen"));
    assert!(device.says(&format!("Password for {UNICODE}")));
    device.type_text("hunter22");
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: UNICODE.into(),
            password: "hunter22".into()
        }]
    );
    // Not kept before it is joined.
    assert!(device.saved().is_empty());
    device.radio(WifiEvent::State(WifiState::Joining {
        ssid: UNICODE.into(),
    }));
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: UNICODE.into(),
        address: "192.168.1.20".into(),
    }));
    assert_eq!(device.saved(), [(UNICODE.into(), "hunter22".into())]);
    assert!(
        device.rows()[0].contains("Connected to"),
        "{:?}",
        device.rows()
    );
    // Among the secrets, veiled; not in the configuration nor anywhere in
    // clear.
    assert!(device.storage.exists(paperpaw_sdk::secrets::FILE));
    assert!(!device.storage.contains("hunter22"));
}

#[test]
fn the_longest_name_joined_by_buttons_only() {
    let mut device = Device::new(BUTTONS);
    device.open();
    device.radio_on_with(vec![network(LONGEST, -50, true)]);
    device.focus(LONGEST_SHOWN);
    device.run(actions::CONFIRM);
    device.type_text("pw2024x");
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: LONGEST.into(),
            password: "pw2024x".into()
        }]
    );
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: LONGEST.into(),
        address: "10.0.0.2".into(),
    }));
    assert_eq!(device.saved(), [(LONGEST.into(), "pw2024x".into())]);
}

#[test]
fn a_wrong_password_is_said_and_not_kept() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.radio_on_with(vec![network("Home", -50, true)]);
    device.tap("Home");
    device.type_text("wrong");
    device.told();
    device.radio(WifiEvent::State(WifiState::Joining {
        ssid: "Home".into(),
    }));
    device.radio(WifiEvent::Failed {
        ssid: "Home".into(),
        reason: JoinFailure::Password,
    });
    device.radio(WifiEvent::State(WifiState::On));
    assert_eq!(
        device.notices.borrow().last().map(String::as_str),
        Some("Wrong password for Home: enter it again.")
    );
    assert!(device.saved().is_empty());
    assert!(!device.storage.contains("wrong"));
    // Again, the right one.
    device.now += 10_000;
    device.tap("Home");
    device.type_text("right");
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: "Home".into(),
            password: "right".into()
        }]
    );
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: "Home".into(),
        address: "10.0.0.2".into(),
    }));
    assert_eq!(device.saved(), [("Home".into(), "right".into())]);
}

#[test]
fn a_password_typed_and_cancelled_joins_nothing() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.radio_on_with(vec![network("Home", -50, true)]);
    device.tap("Home");
    device.run(actions::BACK);
    assert_eq!(device.on_top(), Some("wifi.page"));
    // Back on the page it may look for networks again; it joins nothing.
    let told = device.told();
    assert!(told.iter().all(|c| *c == WifiCommand::Scan), "{told:?}");
    assert!(device.saved().is_empty());
}

/// An open network is joined when chosen, without a password; it is not
/// saved, as anyone can set one up under any name, so it is not joined by
/// itself later.
#[test]
fn an_open_network_is_joined_without_asking_and_not_saved() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.radio_on_with(vec![network("Library", -50, false)]);
    device.tap("Library");
    assert_eq!(device.on_top(), Some("wifi.page"));
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: "Library".into(),
            password: String::new()
        }]
    );
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: "Library".into(),
        address: "10.0.0.2".into(),
    }));
    assert!(device.saved().is_empty());
}

#[test]
fn a_saved_network_is_joined_again_without_asking_and_forgotten() {
    let storage = Shared::default();
    let mut device = Device::on(TOUCH, storage.clone());
    device.open();
    device.radio_on_with(vec![network("Home", -50, true)]);
    device.tap("Home");
    device.type_text("secret1");
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: "Home".into(),
        address: "10.0.0.2".into(),
    }));
    device.told();

    // Another start: the network is known.
    let mut device = Device::on(TOUCH, storage);
    device.open();
    device.radio_on_with(vec![
        network("Home", -50, true),
        network("Other", -60, true),
    ]);
    assert!(device.row("Home").label.contains("Saved"));
    device.tap("Home");
    assert_eq!(device.on_top(), Some("wifi.page"));
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: "Home".into(),
            password: "secret1".into()
        }]
    );
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: "Home".into(),
        address: "10.0.0.2".into(),
    }));
    // Held: forgotten, and left as it is joined.
    device.hold("Home");
    assert!(device.saved().is_empty());
    assert_eq!(device.told(), [WifiCommand::Leave]);
    // A network not saved is not forgotten: holding it does nothing.
    device.hold("Other");
    assert!(device.told().is_empty());
}

#[test]
fn forgetting_by_buttons_with_the_menu_button() {
    let mut device = Device::new(BUTTONS);
    device.open();
    // Secured: only those are saved.
    device.radio_on_with(vec![network("Home", -50, true)]);
    device.focus("Home");
    device.run(actions::CONFIRM);
    device.type_text("pw1");
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: "Home".into(),
        address: "10.0.0.2".into(),
    }));
    assert_eq!(device.saved(), [("Home".into(), "pw1".into())]);
    device.told();
    device.focus("Home");
    device.run(actions::MENU);
    assert!(device.saved().is_empty());
    assert_eq!(device.told(), [WifiCommand::Leave]);
}

#[test]
fn no_networks_found_says_so() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.radio_on_with(Vec::new());
    assert!(device.says("No networks found"));
    device.tap("Look for networks again");
    assert_eq!(device.told(), [WifiCommand::Scan]);
    // Only nameless networks: none.
    device.radio(WifiEvent::Scanned(vec![network("", -40, true)]));
    assert!(device.says("No networks found"));
}

/// Names of networks may hold any bytes, tabs and line breaks too; the
/// secrets' file keeps them as they are, so after a restart the network
/// kept is the one joined: known, and can be forgotten.
#[test]
fn a_name_with_a_tab_stays_known_after_a_restart() {
    let name = "Cafe\tGuest";
    let storage = Shared::default();
    let mut device = Device::on(TOUCH, storage.clone());
    device.open();
    device.radio_on_with(vec![network(name, -50, true)]);
    device.tap("Cafe");
    device.type_text("pw1");
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: name.into(),
        address: "10.0.0.2".into(),
    }));
    assert_eq!(device.saved(), [(name.into(), "pw1".into())]);
    let device = Device::on(TOUCH, storage);
    assert_eq!(device.saved(), [(name.into(), "pw1".into())]);
}

/// Without a saved network, what wants one fails at once and its screen
/// says so: no notice over that screen besides.
#[test]
fn without_a_saved_network_wanting_it_fails_at_once_without_a_notice() {
    let mut device = Device::new(TOUCH);
    device.run(FETCH);
    assert!(device.told().is_empty());
    assert!(device.notices.borrow().is_empty());
    assert!(device.requests.is_empty());
}

#[test]
fn wanted_the_radio_joins_the_saved_network_and_goes_off_after() {
    let mut device = Device::new(BUTTONS);
    device
        .kernel
        .set_secret(paperpaw_sdk::wifi::keep("Home", "pw"));
    device.run(FETCH);
    assert_eq!(device.told(), [WifiCommand::On]);
    device.radio(WifiEvent::State(WifiState::On));
    assert_eq!(device.told(), [WifiCommand::Scan]);
    device.radio(WifiEvent::Scanned(vec![
        network("Stranger", -40, true),
        network("Home", -70, true),
    ]));
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: "Home".into(),
            password: "pw".into()
        }]
    );
    // No request before there is a network.
    assert!(device.requests.is_empty());
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: "Home".into(),
        address: "10.0.0.2".into(),
    }));
    assert_eq!(device.requests.len(), 1);
    device.now += 100;
    let effects = device.kernel.http_response(
        Instant(device.now),
        paperpaw_core::HttpResponse {
            id: device.requests[0],
            status: 200,
            body: b"ok".to_vec(),
            ..paperpaw_core::HttpResponse::default()
        },
    );
    device.settle(effects);
    assert!(device.told().is_empty());
    // A minute later, nothing using it: off.
    device.now += 61_000;
    let effects = device.kernel.tick(Instant(device.now));
    device.settle(effects);
    assert_eq!(device.told(), [WifiCommand::Off]);
}

/// The network picked last on the page is joined first after a restart,
/// though another saved one is stronger (*Join first*: Last picked).
#[test]
fn the_network_picked_last_is_joined_first_after_a_restart() {
    let storage = Shared::default();
    let mut device = Device::on(TOUCH, storage.clone());
    device
        .kernel
        .set_secret(paperpaw_sdk::wifi::keep("Strong", "pw1"));
    device
        .kernel
        .set_secret(paperpaw_sdk::wifi::keep("Weak", "pw2"));
    device.open();
    device.radio_on_with(vec![
        network("Strong", -45, true),
        network("Weak", -70, true),
    ]);
    device.told();
    device.tap("Weak");
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: "Weak".into(),
            password: "pw2".into()
        }]
    );
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: "Weak".into(),
        address: "10.0.0.2".into(),
    }));

    // Another start: wanted, both in reach, the one picked joins.
    let mut device = Device::on(BUTTONS, storage);
    device.run(FETCH);
    assert_eq!(device.told(), [WifiCommand::On]);
    device.radio(WifiEvent::State(WifiState::On));
    assert_eq!(device.told(), [WifiCommand::Scan]);
    device.radio(WifiEvent::Scanned(vec![
        network("Strong", -45, true),
        network("Weak", -70, true),
    ]));
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: "Weak".into(),
            password: "pw2".into()
        }]
    );
}

#[test]
fn a_saved_network_not_in_reach_is_said_when_wanted() {
    let mut device = Device::new(TOUCH);
    device
        .kernel
        .set_secret(paperpaw_sdk::wifi::keep("Home", "pw"));
    device.run(FETCH);
    device.radio(WifiEvent::State(WifiState::On));
    // Looked for a few times first.
    for _ in 0..3 {
        assert!(device.notices.borrow().is_empty());
        device.radio(WifiEvent::Scanned(vec![network("Other", -40, true)]));
    }
    assert_eq!(
        device.notices.borrow().last().map(String::as_str),
        Some("No saved Wi-Fi network in reach.")
    );
    assert!(device.requests.is_empty());
}

/// A network that did not answer (or turned the device away) keeps the
/// password typed: chosen again, the keyboard offers it, and OK joins
/// with it, not typed anew.
#[test]
fn a_password_that_got_no_answer_is_offered_again() {
    let mut device = Device::new(TOUCH);
    device.open();
    device.radio_on_with(vec![network("Hannes", -45, true)]);
    device.tap("Hannes");
    device.type_text("secret1");
    device.told();
    device.radio(WifiEvent::State(WifiState::Joining {
        ssid: "Hannes".into(),
    }));
    device.radio(WifiEvent::Failed {
        ssid: "Hannes".into(),
        reason: JoinFailure::Timeout,
    });
    device.radio(WifiEvent::State(WifiState::On));
    assert_eq!(
        device.notices.borrow().last().map(String::as_str),
        Some("Hannes did not answer.")
    );
    // Not kept among the secrets before it is joined.
    assert!(device.saved().is_empty());
    device.now += 10_000;
    device.tap("Hannes");
    device.type_text("");
    assert_eq!(
        device.told(),
        [WifiCommand::Join {
            ssid: "Hannes".into(),
            password: "secret1".into()
        }]
    );
    device.radio(WifiEvent::State(WifiState::Online {
        ssid: "Hannes".into(),
        address: "10.0.0.7".into(),
    }));
    assert_eq!(device.saved(), [("Hannes".into(), "secret1".into())]);
}
