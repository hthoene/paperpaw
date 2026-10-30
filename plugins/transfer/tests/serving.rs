//! Transfer by browser with the real kernel and its server: the screen
//! starts a server and shows the address with its key; requests come in
//! as a shell sends them (an upload's body in parts), and what the
//! plugin answers and writes is checked: an upload in many parts lands
//! whole, uploads out of reach are refused and leave nothing behind,
//! names taken get a number, a wrong key gets nothing, a lost
//! connection leaves no half file, and closing the screen stops the
//! server.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::{Effects, Io, Kernel, ServerCall, ServerEvent};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_transfer::Transfer;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{DirEntry, Id, MemoryStorage, Storage, StorageError};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-touch",
    "Test touch",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(false, true, false, &[]),
    MemoryClass::Small,
);

/// A storage the test looks into while the kernel has it, on a card that
/// can be made to fail.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>, Rc<Card>);

/// What the card does: how much it takes before it fails (none: never),
/// and the most it was given to append at once.
#[derive(Default)]
struct Card {
    fails_after: Cell<Option<u64>>,
    appended: Cell<u64>,
    largest_append: Cell<usize>,
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
        let card = &self.1;
        card.largest_append
            .set(card.largest_append.get().max(data.len()));
        let appended = card.appended.get() + data.len() as u64;
        if card.fails_after.get().is_some_and(|most| appended > most) {
            return Err(StorageError::Io);
        }
        card.appended.set(appended);
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

/// An answer the plugin gave: its status and body.
#[derive(Debug)]
struct Answer {
    status: u16,
    body: Vec<u8>,
    file: Option<String>,
}

/// A random source for the tests: bytes that differ, the same each run.
fn fill(buf: &mut [u8]) -> bool {
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte = (i as u8).wrapping_mul(97).wrapping_add(13);
    }
    true
}

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
    server: u32,
    key: String,
    next_request: u32,
}

impl Device {
    /// The transfer screen open and its server started.
    fn serving() -> Self {
        let storage = Shared::default();
        storage.create_dir("Books").unwrap();
        storage.write("Books/a.epub", b"old").unwrap();
        let kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .user_config("[core]\nlanguage = en\n")
            .network(true)
            // The key is drawn from the platform's random source.
            .random(fill)
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
            next_request: 1,
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device.run(Id::new_static("transfer.open"));
        assert_eq!(
            device.kernel.active_screen().map(Id::as_str),
            Some("transfer.screen")
        );
        let calls = device.calls();
        let Some(ServerCall::Listen { server, port }) = calls.first() else {
            panic!("no server: {calls:?}");
        };
        assert_eq!(*port, 8080);
        device.server = *server;
        device.event(ServerEvent::Started {
            server: device.server,
            address: "192.168.1.20:8080".into(),
        });
        // The key, from the address on the screen.
        device.kernel.render(&mut |_, _| {});
        let shown: Vec<String> = device
            .kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        let address = shown
            .iter()
            .find(|l| l.contains("k="))
            .unwrap_or_else(|| panic!("no address: {shown:?}"));
        device.key = address.rsplit("k=").next().unwrap().to_string();
        // 16 letters of about 4.17 bits: 66 bits, none taken for another.
        assert_eq!(device.key.len(), 16, "{address}");
        assert!(
            device
                .key
                .chars()
                .all(|c| paperpaw_sdk::random::READABLE.contains(c)),
            "{address}"
        );
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

    /// Sends a request, with a body in `parts` if any, and returns the
    /// answer.
    fn request(&mut self, method: &str, target: &str, parts: Option<&[&[u8]]>) -> Answer {
        let request = self.next_request;
        self.next_request += 1;
        self.event(ServerEvent::Request {
            server: self.server,
            request,
            method: method.into(),
            target: target.into(),
            body: parts.is_some(),
        });
        if let Some(parts) = parts {
            for (i, part) in parts.iter().enumerate() {
                self.event(ServerEvent::Body {
                    request,
                    data: part.to_vec(),
                    last: i + 1 == parts.len(),
                });
            }
        }
        self.answer(request)
    }

    fn answer(&mut self, request: u32) -> Answer {
        let calls = self.calls();
        calls
            .into_iter()
            .find_map(|call| match call {
                ServerCall::Respond {
                    request: r,
                    status,
                    body,
                    file,
                    ..
                } if r == request => Some(Answer {
                    status,
                    body,
                    file: file.map(|f| f.path),
                }),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no answer to request {request}"))
    }

    /// Sends an upload of `parts` with `pause` milliseconds between them,
    /// the kernel's timers running meanwhile, as a slow network brings
    /// them; the calls the shell got before the last part, and after.
    fn upload_slowly(
        &mut self,
        path: &str,
        parts: &[Vec<u8>],
        pause: u64,
    ) -> (Vec<ServerCall>, Vec<ServerCall>) {
        let request = self.next_request;
        self.next_request += 1;
        let target = format!("/api/file?k={}&path={path}", self.key);
        self.event(ServerEvent::Request {
            server: self.server,
            request,
            method: "PUT".into(),
            target,
            body: true,
        });
        let mut before = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            self.now += pause;
            let effects = self.kernel.tick(Instant(self.now));
            self.settle(effects);
            let last = i + 1 == parts.len();
            if last {
                before = self.calls();
            }
            self.event(ServerEvent::Body {
                request,
                data: part.clone(),
                last,
            });
        }
        (before, self.calls())
    }

    fn upload(&mut self, path: &str, parts: &[&[u8]]) -> Answer {
        let target = format!("/api/file?k={}&path={path}", self.key);
        self.request("PUT", &target, Some(parts))
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.storage.0.get(path)
    }

    /// The files waiting in the kernel's folder for bodies.
    fn waiting_bodies(&self) -> usize {
        let mut count = 0;
        let _ = self.storage.list(".paperpaw/tmp", &mut |_| count += 1);
        count
    }

    /// What the screen says happened last.
    fn says(&mut self, text: &str) -> bool {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .iter()
            .any(|p| p.label.contains(text))
    }
}

#[test]
fn the_page_needs_the_key_shown() {
    let mut device = Device::serving();
    assert_eq!(device.request("GET", "/", None).status, 403);
    assert_eq!(device.request("GET", "/?k=00000000", None).status, 403);
    let target = format!("/?k={}", device.key);
    let page = device.request("GET", &target, None);
    assert_eq!(page.status, 200);
    let html = String::from_utf8(page.body).unwrap();
    // With its texts in the device's language.
    assert!(!html.contains("/*STRINGS*/{}"));
    assert!(
        html.contains("<html") || html.contains("<!doctype"),
        "{html}"
    );
}

#[test]
fn an_upload_in_many_parts_lands_whole() {
    let mut device = Device::serving();
    let data: Vec<u8> = (0..3 * 1024 * 1024u32).map(|i| (i % 253) as u8).collect();
    let parts: Vec<&[u8]> = data.chunks(64 * 1024).collect();
    let answer = device.upload("Books/big%20book.pdf", &parts);
    assert_eq!(
        answer.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&answer.body)
    );
    assert_eq!(device.read("Books/big book.pdf").unwrap(), data);
    assert_eq!(device.waiting_bodies(), 0);
    assert!(device.says("big book.pdf"));
}

#[test]
fn uploads_out_of_reach_are_refused_and_leave_nothing() {
    let mut device = Device::serving();
    for path in [
        "../x.epub",
        "%2E%2E%2Fx.epub",
        "Books\\..\\..\\x.epub",
        "C:\\x.epub",
        ".paperpaw/secrets.txt",
        "Books/.x.epub",
        "Books/%00.epub",
    ] {
        let answer = device.upload(path, &[b"evil"]);
        assert_eq!(answer.status, 400, "{path}");
        // The body saved for it is gone again.
        assert_eq!(device.waiting_bodies(), 0, "{path}");
    }
    assert!(!device.storage.exists("x.epub"));
    assert!(!device.storage.exists(".paperpaw/secrets.txt"));
    assert_eq!(device.read("Books/a.epub").unwrap(), b"old");
}

#[test]
fn a_name_taken_gets_a_number() {
    let mut device = Device::serving();
    assert_eq!(device.upload("Books/a.epub", &[b"new"]).status, 200);
    assert_eq!(device.upload("Books/a.epub", &[b"newer"]).status, 200);
    assert_eq!(device.read("Books/a.epub").unwrap(), b"old");
    assert_eq!(device.read("Books/a (2).epub").unwrap(), b"new");
    assert_eq!(device.read("Books/a (3).epub").unwrap(), b"newer");
    assert!(device.says("a (3).epub"));
}

#[test]
fn a_wrong_key_writes_nothing() {
    let mut device = Device::serving();
    let answer = device.request(
        "PUT",
        "/api/file?k=wrongkey&path=Books/x.epub",
        Some(&[b"x"]),
    );
    assert_eq!(answer.status, 403);
    assert!(!device.storage.exists("Books/x.epub"));
    assert_eq!(device.waiting_bodies(), 0);
}

#[test]
fn a_lost_connection_leaves_no_half_file() {
    let mut device = Device::serving();
    let request = device.next_request;
    device.next_request += 1;
    let target = format!("/api/file?k={}&path=Books/half.epub", device.key);
    device.event(ServerEvent::Request {
        server: device.server,
        request,
        method: "PUT".into(),
        target,
        body: true,
    });
    device.event(ServerEvent::Body {
        request,
        data: vec![1; 1000],
        last: false,
    });
    device.event(ServerEvent::Closed { request });
    assert!(!device.storage.exists("Books/half.epub"));
    assert_eq!(device.waiting_bodies(), 0);
    // Parts after the end are ignored.
    device.event(ServerEvent::Body {
        request,
        data: vec![2; 10],
        last: true,
    });
    assert!(device.calls().is_empty());
}

#[test]
fn files_are_sent_listed_and_deleted() {
    let mut device = Device::serving();
    let key = device.key.clone();
    let sent = device.request("GET", &format!("/api/file?k={key}&path=Books/a.epub"), None);
    assert_eq!(
        (sent.status, sent.file.as_deref()),
        (200, Some("Books/a.epub"))
    );
    let list = device.request("GET", &format!("/api/list?k={key}&path=Books"), None);
    assert_eq!(
        String::from_utf8(list.body).unwrap(),
        r#"{"entries":[{"name":"a.epub","dir":false,"size":3}]}"#
    );
    // The top lists no hidden folders.
    let top = device.request("GET", &format!("/api/list?k={key}"), None);
    assert!(!String::from_utf8(top.body).unwrap().contains(".paperpaw"));
    let made = device.request("POST", &format!("/api/mkdir?k={key}&path=Books/New"), None);
    assert_eq!(made.status, 200);
    let deleted = device.request(
        "POST",
        &format!("/api/delete?k={key}&path=Books/a.epub"),
        None,
    );
    assert_eq!(deleted.status, 200);
    assert!(!device.storage.exists("Books/a.epub"));
    assert!(device.says("a.epub"));
}

/// A folder deleted from the web page takes its books' files; what the
/// reader noted in them stays, as when a computer deletes them.
#[test]
fn a_folder_deleted_keeps_the_notes_of_its_books() {
    let mut device = Device::serving();
    let key = device.key.clone();
    let notes = paperpaw_sdk::annotations::file("Books/a.epub");
    let line = b"h\t1\t0\t1\t20\t5\t0\t0\tText\tNote\n";
    device.storage.write(&notes, line).unwrap();
    let deleted = device.request("POST", &format!("/api/delete?k={key}&path=Books"), None);
    assert_eq!(deleted.status, 200);
    assert!(!device.storage.exists("Books/a.epub"));
    assert_eq!(device.storage.0.get(&notes).as_deref(), Some(&line[..]));
    assert!(device.says("Books"));
}

#[test]
fn without_a_network_the_screen_says_so() {
    let mut device = Device::serving();
    device.event(ServerEvent::Failed {
        server: device.server,
        reason: "no network".into(),
    });
    device.kernel.render(&mut |_, _| {});
    let shown: Vec<String> = device
        .kernel
        .describe()
        .into_iter()
        .map(|p| p.label)
        .collect();
    assert!(!shown.iter().any(|l| l.contains("k=")), "{shown:?}");
}

#[test]
fn closing_the_screen_stops_the_server() {
    let mut device = Device::serving();
    device.run(actions::BACK);
    let calls = device.calls();
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, ServerCall::Stop { server } if *server == device.server)),
        "{calls:?}"
    );
    // Requests after it are not answered by the plugin.
    let request = device.next_request;
    device.event(ServerEvent::Request {
        server: device.server,
        request,
        method: "GET".into(),
        target: format!("/api/list?k={}", device.key),
        body: false,
    });
    assert!(device.calls().is_empty());
}

/// Where PaperPaw runs Wi-Fi, the transfer goes without a router too:
/// the reader opens its own network, serves the page there and closes
/// the network with the screen (or after ten minutes left alone).
#[test]
fn a_transfer_without_a_router_opens_the_readers_own_network() {
    use paperpaw_core::WifiEvent;
    use paperpaw_sdk::wifi::{WifiCommand, WifiState};
    let mut kernel = Kernel::builder(DEVICE)
        .storage(Shared::default())
        .user_config("[core]\nlanguage = en\n")
        .network(true)
        .wifi(true)
        .random(fill)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Transfer)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(100), Id::new_static("transfer.open"));
    let _ = kernel.take_io();
    kernel.render(&mut |_, _| {});
    let parts = kernel.describe();
    let button = parts
        .iter()
        .find(|p| p.label == "Without a router")
        .unwrap_or_else(|| panic!("{parts:?}"));
    let at = paperpaw_platform::Point::new(
        i32::from(button.area.x + button.area.width / 2),
        i32::from(button.area.y + button.area.height / 2),
    );
    for (t, phase) in [
        (200, paperpaw_platform::PointerPhase::Down),
        (260, paperpaw_platform::PointerPhase::Up),
    ] {
        kernel.handle(
            Instant(t),
            paperpaw_platform::InputEvent::Pointer(paperpaw_platform::PointerEvent::new(
                0,
                phase,
                at,
                paperpaw_platform::PointerTool::Finger,
                1000,
            )),
        );
    }
    kernel.tick(Instant(1000));
    let radio: Vec<WifiCommand> = kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Wifi(command) => Some(command),
            _ => None,
        })
        .collect();
    let Some(WifiCommand::Host { ssid, password }) = radio.last().cloned() else {
        panic!("{radio:?}");
    };
    kernel.wifi_event(
        Instant(1200),
        WifiEvent::State(WifiState::Hosting {
            ssid: ssid.clone(),
            address: "192.168.4.1".into(),
            guests: 0,
        }),
    );
    // The server waiting for a network goes; the one in the own network
    // listens.
    let io = kernel.take_io();
    let Some(server) = io.iter().rev().find_map(|io| match io {
        Io::Server(ServerCall::Listen { server, .. }) => Some(*server),
        _ => None,
    }) else {
        panic!("{io:?}");
    };
    kernel.server_event(
        Instant(1300),
        ServerEvent::Started {
            server,
            address: "192.168.4.1:8080".into(),
        },
    );
    kernel.render(&mut |_, _| {});
    let shown: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(shown.iter().any(|l| l.contains(&password)), "{shown:?}");
    assert!(shown.iter().any(|l| l.contains("192.168.4.1")), "{shown:?}");
    // Left alone ten minutes: the own network closes.
    kernel.tick(Instant(1300 + 11 * 60 * 1000));
    let radio: Vec<WifiCommand> = kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Wifi(command) => Some(command),
            _ => None,
        })
        .collect();
    assert!(radio.contains(&WifiCommand::Leave), "{radio:?}");
}

/// A mode whose level is Offline, worn at the start.
static OFFLINE: [paperpaw_sdk::presets::Preset; 1] = [paperpaw_sdk::presets::Preset::fixed(
    "unplugged.mode",
    "core.power-mode",
    "Unplugged",
    "",
    "power.level = offline",
)];

struct Unplugged;

impl paperpaw_sdk::Plugin for Unplugged {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("unplugged"),
            "Unplugged",
            paperpaw_sdk::Version::new(0, 1, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_presets(&OFFLINE)?;
        r.subscribe(
            paperpaw_sdk::builtin::events::STARTED.id().clone(),
            Box::new(|_, _, out| {
                out.push(paperpaw_sdk::Command::Wear(paperpaw_sdk::power::Wear::new(
                    Id::new_static("unplugged.mode"),
                )));
            }),
        )
    }
}

/// In Offline mode the screen offers to go online first, inline; going
/// online starts the server, not now says there is no network.
#[test]
fn offline_mode_offers_to_go_online_first() {
    for go in [true, false] {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(Shared::default())
            .user_config("[core]\nlanguage = en\n")
            .network(true)
            .random(fill)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Unplugged)
            .plugin(Transfer)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.run_action(Instant(100), Id::new_static("transfer.open"));
        let listens = |kernel: &mut Kernel| {
            kernel
                .take_io()
                .iter()
                .any(|io| matches!(io, Io::Server(ServerCall::Listen { .. })))
        };
        assert!(!listens(&mut kernel), "not before asking");
        let shown = |kernel: &mut Kernel| {
            kernel.render(&mut |_, _| {});
            kernel
                .describe()
                .into_iter()
                .map(|p| (p.label, p.area))
                .collect::<Vec<_>>()
        };
        let parts = shown(&mut kernel);
        let label = if go { "Go online" } else { "Not now" };
        let (_, area) = parts
            .iter()
            .find(|(l, _)| l == label)
            .unwrap_or_else(|| panic!("{parts:?}"))
            .clone();
        let at = paperpaw_platform::Point::new(
            i32::from(area.x + area.width / 2),
            i32::from(area.y + area.height / 2),
        );
        for (t, phase) in [
            (200, paperpaw_platform::PointerPhase::Down),
            (260, paperpaw_platform::PointerPhase::Up),
        ] {
            kernel.handle(
                Instant(t),
                paperpaw_platform::InputEvent::Pointer(paperpaw_platform::PointerEvent::new(
                    0,
                    phase,
                    at,
                    paperpaw_platform::PointerTool::Finger,
                    1000,
                )),
            );
        }
        kernel.tick(Instant(1000));
        assert_eq!(listens(&mut kernel), go);
        if !go {
            let parts = shown(&mut kernel);
            assert!(
                parts.iter().any(|(l, _)| l.contains("No Wi-Fi")),
                "{parts:?}"
            );
        }
    }
}

/// A large upload over a slow network lands whole: the kernel writes each
/// part as it comes, never more than a part at a time, however long the
/// parts take.
#[test]
fn a_large_slow_upload_lands_whole_a_part_at_a_time() {
    let mut device = Device::serving();
    const PART: usize = 16 * 1024;
    let mut data = vec![0u8; 6 * 1024 * 1024 + 123];
    fill(&mut data);
    let parts: Vec<Vec<u8>> = data.chunks(PART).map(<[u8]>::to_vec).collect();
    let (_, calls) = device.upload_slowly("Books/large.epub", &parts, 1_500);
    let answered = calls.iter().find_map(|call| match call {
        ServerCall::Respond { status, .. } => Some(*status),
        _ => None,
    });
    assert_eq!(answered, Some(200), "{calls:?}");
    assert_eq!(device.read("Books/large.epub").as_deref(), Some(&data[..]));
    assert!(
        device.storage.1.largest_append.get() <= PART,
        "a part at a time"
    );
    assert_eq!(device.waiting_bodies(), 0, "nothing left behind");
}

/// A card that fails in the middle of an upload: the client is answered at
/// once, while it is still sending, and nothing is left behind.
#[test]
fn a_card_failing_mid_upload_is_answered_at_once() {
    let mut device = Device::serving();
    device.storage.1.fails_after.set(Some(100 * 1024));
    let parts: Vec<Vec<u8>> = (0..40).map(|_| vec![7u8; 16 * 1024]).collect();
    let (before, _) = device.upload_slowly("Books/large.epub", &parts, 200);
    let refused = before.iter().find_map(|call| match call {
        ServerCall::Respond { status, body, .. } => {
            Some((*status, String::from_utf8_lossy(body).into_owned()))
        }
        _ => None,
    });
    let (status, said) =
        refused.unwrap_or_else(|| panic!("no answer before the body ended: {before:?}"));
    assert_eq!(status, 500, "{said}");
    assert!(said.contains("card"), "{said}");
    assert_eq!(device.read("Books/large.epub"), None);
    assert_eq!(device.waiting_bodies(), 0, "nothing left behind");
}
