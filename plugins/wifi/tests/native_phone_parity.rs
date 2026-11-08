//! The pages of the phone setup, made by the native module, against those
//! the interface-6 screen serves on the real kernel (all four: the choice of
//! networks, the same asked again, the network being joined, nothing there).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Io, Kernel, ServerCall, ServerEvent, WifiEvent};
use paperpaw_platform::Rotation;
use paperpaw_plugin_wifi::native_phone::page::{
    Found, Texts, choose_page, line_page, not_found_page,
};
use paperpaw_plugin_wifi::{PHONE, Wifi};
use paperpaw_sdk::wifi::{WifiNetwork, WifiState};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, Params, PixelFormat,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);
const EN: &str = include_str!("../i18n/en.ini");

fn random(buf: &mut [u8]) -> bool {
    buf.iter_mut()
        .enumerate()
        .for_each(|(i, b)| *b = (i as u8).wrapping_mul(37));
    true
}

/// A word of the English catalog.
fn word(key: &str) -> &'static str {
    EN.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(" = "))
        .unwrap_or_else(|| panic!("{key}"))
}

struct Test {
    kernel: Kernel,
    at: u64,
    server: u32,
}

impl Test {
    fn new() -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .network(true)
            .wifi(true)
            .random(random)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Wifi)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let _ = kernel.take_io();
        let mut t = Self {
            kernel,
            at: 0,
            server: 0,
        };
        let now = t.now(100);
        t.kernel.open_screen(now, PHONE, Params::new());
        t.kernel.take_io();
        t.wifi(WifiEvent::State(WifiState::On));
        let found = ["Home", "Caf\u{e9} & <Bar>", "Open \"Net\""]
            .iter()
            .enumerate()
            .map(|(n, ssid)| WifiNetwork::new((*ssid).to_string(), -50 - n as i8, n != 2))
            .collect();
        let io = t.wifi(WifiEvent::Scanned(found));
        let ssid = io
            .iter()
            .find_map(|io| match io {
                Io::Wifi(paperpaw_sdk::wifi::WifiCommand::Host { ssid, .. }) => Some(ssid.clone()),
                _ => None,
            })
            .unwrap();
        let io = t.wifi(WifiEvent::State(WifiState::Hosting {
            ssid,
            address: "192.168.4.1".to_string(),
            guests: 0,
        }));
        t.server = io
            .iter()
            .find_map(|io| match io {
                Io::Server(ServerCall::Listen { server, .. }) => Some(*server),
                _ => None,
            })
            .unwrap();
        t
    }

    fn now(&mut self, ms: u64) -> Instant {
        self.at += ms;
        Instant(self.at)
    }

    fn wifi(&mut self, event: WifiEvent) -> Vec<Io> {
        let now = self.now(100);
        self.kernel.wifi_event(now, event);
        self.kernel.take_io()
    }

    /// The status and body of the answer to a request.
    fn ask(&mut self, method: &str, target: &str, form: Option<&str>) -> (u16, String) {
        let now = self.now(100);
        let request = self.at as u32;
        self.kernel.server_event(
            now,
            ServerEvent::Request {
                server: self.server,
                request,
                method: method.to_string(),
                target: target.to_string(),
                body: form.is_some(),
            },
        );
        let mut io = self.kernel.take_io();
        if let Some(form) = form {
            let now = self.now(100);
            self.kernel.server_event(
                now,
                ServerEvent::Body {
                    request,
                    data: form.as_bytes().to_vec(),
                    last: true,
                },
            );
            io = self.kernel.take_io();
        }
        io.iter()
            .find_map(|io| match io {
                Io::Server(ServerCall::Respond { status, body, .. }) => {
                    Some((*status, String::from_utf8(body.clone()).unwrap()))
                }
                _ => None,
            })
            .unwrap()
    }
}

fn token(page: &str) -> String {
    let mark = "name=\"t\" value=\"";
    let at = page.find(mark).unwrap() + mark.len();
    page[at..].split('"').next().unwrap().to_string()
}

#[test]
fn the_native_pages_are_the_pages_the_interface_6_screen_serves() {
    let mut t = Test::new();
    let texts = Texts {
        language: "en",
        title: word("wifi.phone.title"),
        choose_one: word("wifi.phone.choose-one"),
        other: word("wifi.phone.other"),
        password: word("wifi.phone.password"),
        join: word("wifi.phone.join"),
        note: word("wifi.phone.page-note"),
        open: word("wifi.network.open"),
    };
    let found = [
        Found {
            ssid: "Home",
            secured: true,
        },
        Found {
            ssid: "Caf\u{e9} & <Bar>",
            secured: true,
        },
        Found {
            ssid: "Open \"Net\"",
            secured: false,
        },
    ];
    let (status, page) = t.ask("GET", "/", None);
    assert_eq!(status, 200);
    let token = token(&page);
    assert_eq!(page, choose_page(&texts, &found, false, &token).unwrap());
    // A form from another page: refused, the choice again.
    let (status, again) = t.ask("POST", "/join", Some("ssid=Evil&password=x"));
    assert_eq!(status, 403);
    assert_eq!(again, choose_page(&texts, &found, true, &token).unwrap());
    // A form of the page without a network.
    let (status, again) = t.ask(
        "POST",
        "/join",
        Some(&format!("t={token}&other=&password=x")),
    );
    assert_eq!(status, 400);
    assert_eq!(again, choose_page(&texts, &found, true, &token).unwrap());
    // Nothing else is served.
    let (status, missing) = t.ask("GET", "/elsewhere?x=1", None);
    assert_eq!(status, 404);
    assert_eq!(missing, not_found_page(&texts).unwrap());
    // A network chosen: told what happens.
    let (status, joining) = t.ask(
        "POST",
        "/join",
        Some(&format!(
            "t={token}&ssid=Caf%C3%A9+%26+%3CBar%3E&other=&password=pw"
        )),
    );
    assert_eq!(status, 200);
    let line = word("wifi.phone.joining-page").replace("{ssid}", "Caf\u{e9} & <Bar>");
    assert_eq!(joining, line_page(&texts, &line).unwrap());
}
