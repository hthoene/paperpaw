//! Setting up Wi-Fi with a phone, with the real kernel: the device looks
//! for networks, opens its own, serves the page there, and joins the
//! network chosen on the phone, keeping its password; a network that
//! does not take the password opens the own network again. And networks
//! put on the card in `wifi.ini` are kept, the file removed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Io, Kernel, ServerCall, ServerEvent, WifiEvent};
use paperpaw_platform::Rotation;
use paperpaw_plugin_wifi::{PHONE, Wifi};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::secrets::{FILE, Secrets};
use paperpaw_sdk::wifi::{self, JoinFailure, WifiCommand, WifiNetwork, WifiState};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, Params, PixelFormat,
    Storage,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

fn random(buf: &mut [u8]) -> bool {
    buf.iter_mut()
        .enumerate()
        .for_each(|(i, b)| *b = (i as u8).wrapping_mul(37));
    true
}

struct Test {
    kernel: Kernel,
    at: u64,
}

impl Test {
    fn new(storage: impl Storage + 'static) -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .network(true)
            .wifi(true)
            .random(random)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Wifi)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let _ = kernel.take_io();
        Test { kernel, at: 0 }
    }

    fn now(&mut self, ms: u64) -> Instant {
        self.at += ms;
        Instant(self.at)
    }

    fn io(&mut self) -> Vec<Io> {
        self.kernel.take_io()
    }

    fn radio(io: &[Io]) -> Vec<WifiCommand> {
        io.iter()
            .filter_map(|io| match io {
                Io::Wifi(command) => Some(command.clone()),
                _ => None,
            })
            .collect()
    }

    fn wifi(&mut self, event: WifiEvent) -> Vec<Io> {
        let now = self.now(100);
        self.kernel.wifi_event(now, event);
        self.io()
    }

    fn server(&mut self, event: ServerEvent) -> Vec<Io> {
        let now = self.now(100);
        self.kernel.server_event(now, event);
        self.io()
    }

    fn tick(&mut self, ms: u64) -> Vec<Io> {
        let now = self.now(ms);
        self.kernel.tick(now);
        self.io()
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

fn network(ssid: &str) -> WifiNetwork {
    WifiNetwork::new(String::from(ssid), -50, true)
}

/// The page's answer to a request: its status and body.
fn answered(io: &[Io]) -> (u16, String) {
    io.iter()
        .find_map(|io| match io {
            Io::Server(ServerCall::Respond { status, body, .. }) => {
                Some((*status, String::from_utf8_lossy(body).into_owned()))
            }
            _ => None,
        })
        .unwrap()
}

/// First the step to join: the name and the password, in groups of
/// five, of letters not taken for one another; not the page yet. A phone
/// joined (it took an address): on to the page by itself.
fn on_to_the_page_once_a_phone_joined(t: &mut Test, ssid: &str, password: &str) {
    let shown = t.shown();
    assert!(shown.contains(ssid), "{shown}");
    assert!(
        shown.contains(&paperpaw_sdk::random::grouped(password)),
        "{shown}"
    );
    assert!(!shown.contains("192.168.4.1"), "{shown}");
    let readable = |text: &str| {
        text.chars()
            .all(|c| paperpaw_sdk::random::READABLE.contains(c))
    };
    assert!(readable(password) && password.len() == 15, "{password}");
    // A phone joined (it took an address): on to the page by itself.
    t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid: ssid.to_string(),
        address: String::from("192.168.4.1"),
        guests: 1,
    }));
    let shown = t.shown();
    assert!(
        shown.contains("http://192.168.4.1/") || shown.contains("192.168.4.1/"),
        "{shown}"
    );
    assert!(
        !shown.contains(&paperpaw_sdk::random::grouped(password)),
        "{shown}"
    );
}

#[test]
fn a_network_chosen_on_the_phone_is_joined_and_kept() {
    let mut t = Test::new(MemoryStorage::new());
    let now = t.now(100);
    t.kernel.open_screen(now, PHONE, Params::new());
    let io = t.io();
    assert_eq!(Test::radio(&io), [WifiCommand::On, WifiCommand::Scan]);
    t.wifi(WifiEvent::State(WifiState::On));
    // Found: the own network opens.
    let io = t.wifi(WifiEvent::Scanned(vec![network("Home"), network("Cafe")]));
    let Some(WifiCommand::Host { ssid, password }) = Test::radio(&io).pop() else {
        panic!("{io:?}");
    };
    // Named as the device is (its model's, none set).
    assert_eq!(ssid, "Test device");
    assert!(password.len() >= 8);
    // Open: the page is served there (the kernel lets the server go to
    // the shell without a way out).
    let io = t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid: ssid.clone(),
        address: String::from("192.168.4.1"),
        guests: 0,
    }));
    let server = io
        .iter()
        .find_map(|io| match io {
            Io::Server(ServerCall::Listen { server, port: 80 }) => Some(*server),
            _ => None,
        })
        .unwrap();
    t.server(ServerEvent::Started {
        server,
        address: String::from("192.168.4.1:80"),
    });
    on_to_the_page_once_a_phone_joined(&mut t, &ssid, &password);
    // The page lists what was found.
    let io = t.server(ServerEvent::Request {
        server,
        request: 1,
        method: String::from("GET"),
        target: String::from("/"),
        body: false,
    });
    let (status, page) = answered(&io);
    assert_eq!(status, 200);
    assert!(
        page.contains("value=\"Home\"") && page.contains("value=\"Cafe\""),
        "{page}"
    );
    let token = token(&page);
    // A form from another page, without the token: refused.
    t.server(ServerEvent::Request {
        server,
        request: 9,
        method: String::from("POST"),
        target: String::from("/join"),
        body: true,
    });
    let io = t.server(ServerEvent::Body {
        request: 9,
        data: b"ssid=Evil&password=x".to_vec(),
        last: true,
    });
    assert_eq!(answered(&io).0, 403);
    assert!(Test::radio(&t.tick(2000)).is_empty());
    // Chosen: answered first, then the own network closes for that one.
    t.server(ServerEvent::Request {
        server,
        request: 2,
        method: String::from("POST"),
        target: String::from("/join"),
        body: true,
    });
    let io = t.server(ServerEvent::Body {
        request: 2,
        data: format!("t={token}&ssid=Home&other=&password=s3cret").into_bytes(),
        last: true,
    });
    let (status, page) = answered(&io);
    assert_eq!(status, 200);
    assert!(page.contains("Home"), "{page}");
    assert!(Test::radio(&io).is_empty());
    let io = t.tick(2000);
    assert_eq!(
        Test::radio(&io),
        [WifiCommand::Join {
            ssid: String::from("Home"),
            password: String::from("s3cret")
        }]
    );
    t.wifi(WifiEvent::State(WifiState::Joining {
        ssid: String::from("Home"),
    }));
    t.wifi(WifiEvent::State(WifiState::Online {
        ssid: String::from("Home"),
        address: String::from("10.0.0.5"),
    }));
    let secrets =
        Secrets::decode(&String::from_utf8(t.kernel.storage().read_to_vec(FILE)).unwrap());
    assert!(wifi::known(secrets.iter()).contains(&(String::from("Home"), String::from("s3cret"))));
    assert!(t.shown().contains("Connected to Home"));
}

/// Where the phone does not say it joined, Next (Select, the page
/// buttons) goes to the page and back; a phone joining after the reader
/// went back does not move it on again.
#[test]
fn next_goes_on_where_the_phone_does_not_say_it_joined() {
    let mut t = Test::new(MemoryStorage::new());
    let now = t.now(100);
    t.kernel.open_screen(now, PHONE, Params::new());
    t.wifi(WifiEvent::State(WifiState::On));
    let io = t.wifi(WifiEvent::Scanned(vec![network("Home")]));
    let Some(WifiCommand::Host { ssid, .. }) = Test::radio(&io).pop() else {
        panic!("{io:?}");
    };
    let io = t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid: ssid.clone(),
        address: String::from("192.168.4.1"),
        guests: 0,
    }));
    let server = io
        .iter()
        .find_map(|io| match io {
            Io::Server(ServerCall::Listen { server, port: 80 }) => Some(*server),
            _ => None,
        })
        .unwrap();
    t.server(ServerEvent::Started {
        server,
        address: String::from("192.168.4.1:80"),
    });
    let page = |t: &mut Test| t.shown().contains("192.168.4.1/");
    assert!(!page(&mut t));
    let now = t.now(200);
    let _ = t.kernel.run_action(now, actions::CONFIRM);
    assert!(page(&mut t));
    let now = t.now(300);
    let _ = t.kernel.run_action(now, actions::PREV_PAGE);
    assert!(!page(&mut t));
    let now = t.now(400);
    let _ = t.kernel.run_action(now, actions::NEXT_PAGE);
    assert!(page(&mut t));
    let now = t.now(500);
    let _ = t.kernel.run_action(now, actions::CONFIRM);
    assert!(!page(&mut t));
    // Back on the first step by hand: the first phone to join moves it
    // on, once; one going back after that stays.
    t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid: ssid.clone(),
        address: String::from("192.168.4.1"),
        guests: 1,
    }));
    assert!(page(&mut t), "the first time a phone joins, on");
    let now = t.now(600);
    let _ = t.kernel.run_action(now, actions::CONFIRM);
    assert!(!page(&mut t));
    t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid,
        address: String::from("192.168.4.1"),
        guests: 2,
    }));
    assert!(!page(&mut t), "once only");
}

#[test]
fn a_password_not_taken_opens_the_own_network_again() {
    let mut t = Test::new(MemoryStorage::new());
    let now = t.now(100);
    t.kernel.open_screen(now, PHONE, Params::new());
    t.io();
    t.wifi(WifiEvent::State(WifiState::On));
    let io = t.wifi(WifiEvent::Scanned(vec![network("Home")]));
    let host = Test::radio(&io);
    let Some(WifiCommand::Host { ssid, .. }) = host.first() else {
        panic!("{io:?}");
    };
    let io = t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid: ssid.clone(),
        address: String::from("192.168.4.1"),
        guests: 0,
    }));
    let Some(Io::Server(ServerCall::Listen { server, .. })) = io.first().cloned() else {
        panic!("{io:?}");
    };
    let io = t.server(ServerEvent::Request {
        server,
        request: 7,
        method: String::from("GET"),
        target: String::from("/"),
        body: false,
    });
    let token = token(&answered(&io).1);
    t.server(ServerEvent::Request {
        server,
        request: 1,
        method: String::from("POST"),
        target: String::from("/join"),
        body: true,
    });
    t.server(ServerEvent::Body {
        request: 1,
        data: format!("t={token}&ssid=Home&password=wrong").into_bytes(),
        last: true,
    });
    t.tick(2000);
    let io = t.wifi(WifiEvent::Failed {
        ssid: String::from("Home"),
        reason: JoinFailure::Password,
    });
    assert_eq!(Test::radio(&io), host);
    assert!(t.shown().contains("Wrong password for Home"));
}

#[test]
fn the_own_network_left_alone_closes_after_ten_minutes() {
    let mut t = Test::new(MemoryStorage::new());
    let now = t.now(100);
    t.kernel.open_screen(now, PHONE, Params::new());
    t.io();
    t.wifi(WifiEvent::State(WifiState::On));
    let io = t.wifi(WifiEvent::Scanned(vec![network("Home")]));
    let Some(WifiCommand::Host { ssid, .. }) = Test::radio(&io).pop() else {
        panic!("{io:?}");
    };
    t.wifi(WifiEvent::State(WifiState::Hosting {
        ssid,
        address: String::from("192.168.4.1"),
        guests: 0,
    }));
    assert!(Test::radio(&t.tick(9 * 60 * 1000)).is_empty());
    let io = t.tick(2 * 60 * 1000);
    assert_eq!(Test::radio(&io), [WifiCommand::Leave]);
}

#[test]
fn networks_on_the_card_are_kept_and_the_file_goes() {
    // Known before with a password: a line without one keeps it.
    let mut secrets = Secrets::default();
    secrets.apply(wifi::keep("Office", "kept"));
    let storage = MemoryStorage::new()
        .with(FILE, secrets.encode().into_bytes())
        .with(
            "wifi.ini",
            "# from the flasher\nssid = Home Net\npassword = pw 1\n\nssid = Cafe\nssid = Office\n",
        );
    let t = Test::new(storage);
    let storage = t.kernel.storage();
    assert!(!storage.exists("wifi.ini"));
    let secrets = Secrets::decode(&String::from_utf8(storage.read_to_vec(FILE)).unwrap());
    let known = wifi::known(secrets.iter());
    assert!(known.contains(&(String::from("Home Net"), String::from("pw 1"))));
    assert!(known.contains(&(String::from("Cafe"), String::new())));
    assert!(known.contains(&(String::from("Office"), String::from("kept"))));
}

#[test]
fn a_wifi_file_too_large_to_read_goes_unread() {
    let storage = MemoryStorage::new().with("wifi.ini", "x".repeat(20_000));
    let t = Test::new(storage);
    assert!(!t.kernel.storage().exists("wifi.ini"));
}

/// Keeps everything but the passwords and keys file, which it refuses to
/// write (a full card, say).
struct NoSecrets(MemoryStorage);

impl Storage for NoSecrets {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_sdk::StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, paperpaw_sdk::StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
        if path == FILE {
            return Err(paperpaw_sdk::StorageError::Full);
        }
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
    ) -> Result<(), paperpaw_sdk::StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), paperpaw_sdk::StorageError> {
        self.0.remove(path)
    }
    fn create_dir(&self, path: &str) -> Result<(), paperpaw_sdk::StorageError> {
        self.0.create_dir(path)
    }
}

/// Networks on the card whose passwords cannot be saved with the others
/// stay in the file: removed, they would be gone at the next restart.
#[test]
fn networks_on_the_card_stay_while_their_passwords_cannot_be_saved() {
    let wifi_ini = "ssid = Home Net\npassword = pw 1\n";
    let t = Test::new(NoSecrets(MemoryStorage::new().with("wifi.ini", wifi_ini)));
    let storage = t.kernel.storage();
    assert_eq!(storage.read_to_vec("wifi.ini"), wifi_ini.as_bytes());
    assert!(!storage.exists(FILE));
}

/// The token in the page's form.
fn token(page: &str) -> String {
    let mark = "name=\"t\" value=\"";
    let at = page.find(mark).unwrap() + mark.len();
    page[at..].split('"').next().unwrap().to_string()
}

trait ReadAll {
    fn read_to_vec(&self, path: &str) -> Vec<u8>;
}

impl ReadAll for dyn Storage + '_ {
    fn read_to_vec(&self, path: &str) -> Vec<u8> {
        paperpaw_sdk::read_file(self, path, 1 << 20).unwrap()
    }
}
