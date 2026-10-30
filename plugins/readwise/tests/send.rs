//! Sendings against answers as Readwise gives them, with the real
//! kernel: the highlights sent from a file, notes of any length, what
//! fails waiting and going later, the token asked for and let go when
//! refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel, WifiEvent};
use paperpaw_platform::Rotation;
use paperpaw_plugin_readwise::Readwise;
use paperpaw_sdk::builtin::events::{self, NotesExported};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::wifi::WifiState;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(200, 300, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// A storage the test looks into while the kernel has it.
#[derive(Clone)]
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

const ALICE: &str = "Notes/Lewis Carroll - Alice.md";
const NOTES: &str = "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
    > Curiouser and curiouser!\n>\n> 12 %\n\n\
    > Who are you?\n>\n> 40 %\n\nThe caterpillar asks.\n\n\
    ## Bookmarks\n\n- 50 %: Down the rabbit hole\n";
const BOB: &str = "Notes/Bob.md";

struct Test {
    kernel: Kernel,
    storage: Shared,
    at: u64,
}

impl Test {
    /// A device with the token `token` kept, and Wi-Fi PaperPaw manages
    /// if `wifi`.
    fn new(token: Option<&str>, wifi: bool) -> Self {
        let mut secrets = Secrets::default();
        if let Some(token) = token {
            secrets.apply(SecretChange::keep("readwise.token", "", token));
        }
        let storage = Shared(Rc::new(
            MemoryStorage::new()
                .with(FILE, secrets.encode().into_bytes())
                .with(ALICE, NOTES)
                .with(BOB, "# Bob's Book\n\n## Highlights\n\n> Hello.\n>\n> 3 %\n"),
        ));
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .network(true)
            .wifi(wifi)
            .plugin(Readwise)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut test = Test {
            kernel,
            storage,
            at: 1,
        };
        if wifi {
            test.wifi(true);
        }
        test.requests();
        test
    }

    fn now(&mut self) -> Instant {
        self.at += 1000;
        Instant(self.at)
    }

    /// The requests the kernel asks the shell to make.
    fn requests(&mut self) -> Vec<HttpCall> {
        let io = self.kernel.take_io().into_iter();
        io.filter_map(|io| match io {
            Io::Http(call) => Some(call),
            _ => None,
        })
        .collect()
    }

    /// The reader exported the notes in `path`.
    fn export(&mut self, path: &str) -> Vec<HttpCall> {
        let exported = NotesExported::new(path.to_string(), String::new());
        let now = self.now();
        let id = events::NOTES_EXPORTED.id().clone();
        self.kernel.send_event(now, id, exported.to_params());
        self.requests()
    }

    /// The token typed when asked.
    fn typed(&mut self, token: &str) -> Vec<HttpCall> {
        let now = self.now();
        let typed = Params::new().with("text", token);
        self.kernel
            .send_event(now, Id::new_static("readwise.token-entered"), typed);
        self.requests()
    }

    fn answer(&mut self, call: &HttpCall, status: u16, body: &str) -> Vec<HttpCall> {
        let now = self.now();
        self.kernel.http_response(
            now,
            HttpResponse {
                id: call.id,
                status,
                body: body.as_bytes().to_vec(),
                ..HttpResponse::default()
            },
        );
        self.requests()
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
        let now = self.now();
        self.kernel.wifi_event(now, WifiEvent::State(state));
        self.requests()
    }

    /// The body a request sends, from its file.
    fn body(&self, call: &HttpCall) -> String {
        let path = call.body_from.as_ref().expect("sent from a file");
        String::from_utf8(self.storage.0.get(path).unwrap()).unwrap()
    }

    fn file(&self, name: &str) -> String {
        let path = format!(".paperpaw/plugins/readwise/{name}");
        String::from_utf8(self.storage.0.get(&path).unwrap_or_default()).unwrap()
    }
}

fn token(call: &HttpCall) -> &str {
    let (_, value) = call
        .headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .unwrap();
    value
}

#[test]
fn highlights_are_sent_from_a_file() {
    let mut t = Test::new(Some("t0k"), false);
    let calls = t.export(ALICE);
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.method, "POST");
    assert_eq!(call.url, "https://readwise.io/api/v2/highlights/");
    assert_eq!(token(call), "Token t0k");
    assert!(call.body.is_none());
    let body = t.body(call);
    assert!(body.starts_with("{\"highlights\":[{\"text\":\"Curiouser and curiouser!\""));
    assert!(body.contains(",\"note\":\"The caterpillar asks.\"}]}"));
    // Readwise's answer, a list of every highlight, is not kept in memory.
    assert!(call.save_to.is_some());
    // It waits until Readwise took it.
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    assert!(t.answer(call, 200, "[]").is_empty());
    assert_eq!(t.file("waiting.txt"), "");
    for name in ["body.json", "answer.json"] {
        assert!(t.file(name).is_empty(), "{name}");
    }
}

#[test]
fn notes_of_any_length_are_sent() {
    // Some 400 KB of notes, more than a small device holds at once.
    let mut notes = String::from("# A Long Book\n\nSomeone\n\n## Highlights\n\n");
    for n in 0..2000 {
        let words = "and so on ".repeat(18);
        notes.push_str(&format!("> {n}: {words}\n>\n> {} %\n\nA note.\n\n", n / 20));
    }
    assert!(notes.len() > 400 * 1024);
    let mut t = Test::new(Some("t0k"), false);
    t.storage.0.write(BOB, notes.as_bytes()).unwrap();
    let calls = t.export(BOB);
    let body = t.body(&calls[0]);
    assert_eq!(body.matches("{\"text\":").count(), 2000);
    assert!(body.contains("{\"text\":\"1999: and so on"));
    assert!(t.answer(&calls[0], 200, "[]").is_empty());
    assert_eq!(t.file("waiting.txt"), "");
}

#[test]
fn what_fails_waits_and_goes_later() {
    let mut t = Test::new(Some("t0k"), true);
    let calls = t.export(ALICE);
    assert!(t.answer(&calls[0], 0, "connection refused").is_empty());
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    // Wi-Fi comes back: sent by itself.
    t.wifi(false);
    let calls = t.wifi(true);
    assert_eq!(calls.len(), 1);
    // Readwise fails: it still waits, and goes with the next export.
    assert!(t.answer(&calls[0], 503, "").is_empty());
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    let calls = t.export(BOB);
    assert!(t.body(&calls[0]).contains("Curiouser and curiouser!"));
    let calls = t.answer(&calls[0], 200, "[]");
    assert!(t.body(&calls[0]).contains("\"title\":\"Bob's Book\""));
    assert!(t.answer(&calls[0], 201, "[]").is_empty());
    assert_eq!(t.file("waiting.txt"), "");
    // Nothing waits: Wi-Fi coming sends nothing.
    t.wifi(false);
    assert!(t.wifi(true).is_empty());
}

#[test]
fn the_token_is_asked_for_and_let_go_when_refused() {
    let mut t = Test::new(None, true);
    assert!(t.export(ALICE).is_empty());
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    // Not asked for by itself.
    t.wifi(false);
    assert!(t.wifi(true).is_empty());
    let calls = t.typed(" t0k ");
    assert_eq!(token(&calls[0]), "Token t0k");
    // Refused: let go, so nothing is tried by itself; the notes wait.
    assert!(t.answer(&calls[0], 401, "").is_empty());
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    t.wifi(false);
    assert!(t.wifi(true).is_empty());
    // A new token, entered with the next export.
    assert!(t.export(ALICE).is_empty());
    let calls = t.typed("good");
    assert_eq!(token(&calls[0]), "Token good");
    assert!(t.answer(&calls[0], 200, "[]").is_empty());
    assert_eq!(t.file("waiting.txt"), "");
}

#[test]
fn deleted_notes_and_notes_without_highlights_wait_no_more() {
    let mut t = Test::new(Some("t0k"), false);
    t.storage
        .0
        .write(BOB, b"# Bob's Book\n\n## Bookmarks\n\n- 5 %: x\n")
        .unwrap();
    t.storage
        .0
        .write(
            ".paperpaw/plugins/readwise/waiting.txt",
            format!("Notes/Deleted.md\n{BOB}\n").as_bytes(),
        )
        .unwrap();
    let calls = t.export(ALICE);
    assert_eq!(calls.len(), 1);
    assert!(t.body(&calls[0]).contains("Curiouser"));
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    assert!(t.answer(&calls[0], 200, "[]").is_empty());
    assert_eq!(t.file("waiting.txt"), "");
}
