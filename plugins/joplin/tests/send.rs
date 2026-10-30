//! Sendings against answers as Joplin's sync targets give them, with the
//! real kernel: new notes and updates of them, a note changed or moved in
//! Joplin, Joplin Cloud's session, and what fails: no connection (the
//! notes wait and go when Wi-Fi comes), a wrong password, encryption,
//! errors of single notes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel, WifiEvent};
use paperpaw_platform::{Rotation, SystemStatus, WallClock};
use paperpaw_plugin_joplin::Joplin;
use paperpaw_sdk::builtin::events::{self, NotesExported};
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::wifi::WifiState;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, Params, PixelFormat, Plugin, RegisterError, Registrar, Storage, StorageError,
    Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(200, 300, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
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

paperpaw_sdk::event_data! {
    /// How a sending ended, as the plugin says it.
    struct Finished {
        text: String,
    }
}
const DONE: Event<Finished> = Event::new("joplin.done");

/// Hears how sendings end.
struct Listener(Rc<RefCell<Vec<String>>>);

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
        let said = self.0.clone();
        r.on(DONE, move |_, finished, _| {
            said.borrow_mut().push(finished.text.clone())
        })
    }
}

const WEBDAV: &str = "[joplin]\nservice = joplin.service.webdav\n\
    address = dav.example/Joplin\nuser = me\n";
const CLOUD: &str = "[joplin]\nservice = joplin.service.cloud\nuser = me@example.org\n";
const ALICE: &str = "Notes/Lewis Carroll - Alice.md";
const NOTES: &str = "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
    > Curiouser and curiouser!\n>\n> 12 %\n\n";
const INFO: &str = r#"{"version": 3, "e2ee": {"value": false, "updatedTime": 0},
    "masterKeys": [], "ppk": {"value": null, "updatedTime": 0}}"#;
/// 2026-10-20 08:00 UTC.
const NOW: i64 = 1_792_483_200_000;

struct Test {
    kernel: Kernel,
    storage: Shared,
    said: Rc<RefCell<Vec<String>>>,
    at: u64,
}

impl Test {
    fn new(config: &str, wifi: bool) -> Self {
        let mut secrets = Secrets::default();
        secrets.apply(SecretChange::keep("joplin.password", "", "pw"));
        let storage = Shared(Rc::new(
            MemoryStorage::new()
                .with(FILE, secrets.encode().into_bytes())
                .with(ALICE, NOTES),
        ));
        let said = Rc::default();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .user_config(config)
            .network(true)
            .wifi(wifi)
            .plugin(Joplin)
            .plugin(Listener(Rc::clone(&said)))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let clock = WallClock::new(Instant(0), NOW, None);
        kernel.set_status(Instant(0), SystemStatus::new(Some(clock), None));
        let mut test = Test {
            kernel,
            storage,
            said,
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
        let exported = NotesExported::new(path.to_string(), String::from("Alice's Adventures"));
        let now = self.now();
        let id = events::NOTES_EXPORTED.id().clone();
        self.kernel.send_event(now, id, exported.to_params());
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

    /// Answers `info.json` (version 3, not encrypted) and the list of
    /// locks that follows (none held).
    fn checked(&mut self, call: &HttpCall) -> Vec<HttpCall> {
        let calls = self.answer(call, 200, INFO);
        assert_eq!(calls.len(), 1);
        let empty = if calls[0].method == "PROPFIND" {
            assert!(calls[0].url.ends_with("/locks/"));
            assert!(
                calls[0]
                    .headers
                    .contains(&(String::from("Depth"), String::from("1")))
            );
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\"></d:multistatus>"
        } else {
            assert_eq!(calls[0].method, "GET");
            assert!(calls[0].url.ends_with("/api/locks"));
            r#"{"items": [], "has_more": false}"#
        };
        self.answer(&calls[0], 207, empty)
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

    /// The body a request sends, from its file where it has one.
    fn body(&self, call: &HttpCall) -> String {
        let bytes = match &call.body_from {
            Some(path) => self.storage.0.get(path).unwrap(),
            None => call.body.clone().unwrap(),
        };
        String::from_utf8(bytes).unwrap()
    }

    fn file(&self, name: &str) -> String {
        let path = format!(".paperpaw/plugins/joplin/{name}");
        String::from_utf8(self.storage.0.get(&path).unwrap_or_default()).unwrap()
    }

    fn last_said(&self) -> String {
        self.said.borrow().last().cloned().unwrap_or_default()
    }

    /// A first sending to the WebDAV folder: `info.json`, the notebook,
    /// the note; the notebook's and the note's text as sent.
    fn send_first(&mut self) -> (String, String) {
        let calls = self.export(ALICE);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "GET");
        assert_eq!(calls[0].url, "https://dav.example/Joplin/info.json");
        let calls = self.checked(&calls[0]);
        assert_eq!(calls[0].method, "PUT");
        let notebook = self.body(&calls[0]);
        let calls = self.answer(&calls[0], 201, "");
        assert_eq!(calls[0].method, "PUT");
        let note = self.body(&calls[0]);
        assert!(
            calls[0]
                .url
                .ends_with(&format!("/{}.md", prop(&note, "id")))
        );
        assert!(self.answer(&calls[0], 201, "").is_empty());
        (notebook, note)
    }
}

/// Property `key` of an item's text.
fn prop<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .rev()
        .find_map(|l| l.strip_prefix(&format!("{key}: ")))
        .unwrap_or_default()
}

#[test]
fn sends_a_new_note_then_updates_it() {
    let mut t = Test::new(WEBDAV, false);
    let (notebook, note) = t.send_first();
    // The notebook, as Joplin writes one.
    assert!(notebook.starts_with("PaperPaw\n\nid: "));
    assert!(notebook.ends_with("\ntype_: 2"));
    let notebook_id = prop(&notebook, "id").to_string();
    // The note: the title, the notes but their title, the properties.
    assert!(note.starts_with(
        "Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
         > Curiouser and curiouser!\n>\n> 12 %\n\nid: "
    ));
    assert_eq!(prop(&note, "parent_id"), notebook_id);
    assert_eq!(prop(&note, "author"), "Lewis Carroll");
    assert_eq!(prop(&note, "created_time"), "2026-10-20T08:00:04.001Z");
    assert_eq!(prop(&note, "type_"), "1");
    let id = prop(&note, "id").to_string();
    assert_eq!(t.last_said(), "The notes of 1 book are in Joplin.");
    assert_eq!(t.file("waiting.txt"), "");
    assert!(t.file("sent.txt").starts_with(&format!("{ALICE}\t{id}\t")));
    // Nothing of the sending is left behind.
    for name in ["info.json", "found.md", "note.md"] {
        assert!(t.file(name).is_empty(), "{name}");
    }

    // A highlight more, exported again: the same note written over, in
    // the notebook it was moved to in Joplin meanwhile.
    t.storage
        .0
        .write(
            ALICE,
            format!("{NOTES}> Who are you?\n>\n> 40 %\n\n").as_bytes(),
        )
        .unwrap();
    let calls = t.export(ALICE);
    let calls = t.checked(&calls[0]);
    assert!(calls[0].url.ends_with(&format!("/{notebook_id}.md")));
    let calls = t.answer(&calls[0], 200, &notebook);
    assert_eq!(calls[0].method, "GET");
    assert!(calls[0].url.ends_with(&format!("/{id}.md")));
    // Moved by a device whose clock is ahead: written over later still,
    // or Joplin would keep what it has.
    let moved = note
        .replace(&notebook_id, "0123456789abcdef0123456789abcdef")
        .replace(
            &format!("\nupdated_time: {}", prop(&note, "updated_time")),
            "\nupdated_time: 2026-10-20T09:00:00.000Z",
        );
    let calls = t.answer(&calls[0], 200, &moved);
    assert_eq!(calls[0].method, "PUT");
    assert!(calls[0].url.ends_with(&format!("/{id}.md")));
    let update = t.body(&calls[0]);
    assert!(update.contains("> Who are you?\n>\n> 40 %\n\nid: "));
    assert_eq!(prop(&update, "id"), id);
    assert_eq!(
        prop(&update, "parent_id"),
        "0123456789abcdef0123456789abcdef"
    );
    for kept in ["created_time", "user_created_time", "order"] {
        assert_eq!(prop(&update, kept), prop(&note, kept), "{kept}");
    }
    assert_eq!(prop(&update, "updated_time"), "2026-10-20T09:00:00.001Z");
    assert_eq!(
        prop(&update, "user_updated_time"),
        "2026-10-20T09:00:00.001Z"
    );
    assert!(t.answer(&calls[0], 204, "").is_empty());
    // What was sent is known by its text.
    let text = &update[..update.find("\n\nid: ").unwrap()];
    let md5 = paperpaw_sdk::digest::md5::of(text);
    assert_eq!(t.file("sent.txt"), format!("{ALICE}\t{id}\t{md5}\n"));
}

#[test]
fn a_note_changed_in_joplin_stays_and_a_new_one_is_made() {
    let mut t = Test::new(WEBDAV, false);
    let (notebook, note) = t.send_first();
    let calls = t.export(ALICE);
    let calls = t.checked(&calls[0]);
    let calls = t.answer(&calls[0], 200, &notebook);
    let edited = note.replace("Curiouser and curiouser!", "My own words.");
    let calls = t.answer(&calls[0], 200, &edited);
    let fresh = t.body(&calls[0]);
    assert_ne!(prop(&fresh, "id"), prop(&note, "id"));
    assert_eq!(prop(&fresh, "parent_id"), prop(&notebook, "id"));
    assert!(t.answer(&calls[0], 201, "").is_empty());
    assert!(t.file("sent.txt").contains(prop(&fresh, "id")));

    // Deleted in Joplin: made anew too, the notebook as well.
    let calls = t.export(ALICE);
    let calls = t.checked(&calls[0]);
    let calls = t.answer(&calls[0], 404, "");
    assert_eq!(calls[0].method, "PUT");
    let new_notebook = t.body(&calls[0]);
    assert_ne!(prop(&new_notebook, "id"), prop(&notebook, "id"));
    let calls = t.answer(&calls[0], 201, "");
    let calls = t.answer(&calls[0], 404, "");
    let again = t.body(&calls[0]);
    assert_ne!(prop(&again, "id"), prop(&fresh, "id"));
    assert_eq!(prop(&again, "parent_id"), prop(&new_notebook, "id"));
}

#[test]
fn a_lost_answer_makes_no_second_note() {
    // A new note Joplin wrote, its answer lost on the way back.
    let mut t = Test::new(WEBDAV, false);
    let calls = t.export(ALICE);
    let calls = t.checked(&calls[0]);
    let notebook = t.body(&calls[0]);
    let calls = t.answer(&calls[0], 201, "");
    let mut joplin = t.body(&calls[0]);
    let id = prop(&joplin, "id").to_string();
    assert!(t.answer(&calls[0], 0, "connection reset").is_empty());
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    // Sent again: the note Joplin has is found and written over.
    let calls = t.export(ALICE);
    let calls = t.checked(&calls[0]);
    let calls = t.answer(&calls[0], 200, &notebook);
    assert_eq!(calls[0].method, "GET");
    assert!(calls[0].url.ends_with(&format!("/{id}.md")));
    let calls = t.answer(&calls[0], 200, &joplin);
    assert_eq!(calls[0].method, "PUT");
    assert!(calls[0].url.ends_with(&format!("/{id}.md")));
    joplin = t.body(&calls[0]);
    assert!(t.answer(&calls[0], 201, "").is_empty());
    assert_eq!(t.last_said(), "The notes of 1 book are in Joplin.");
    assert!(t.file("sent.txt").starts_with(&format!("{ALICE}\t{id}\t")));
    assert_eq!(t.file("sending.txt"), "");

    // An update whose answer was lost: written over again, whether it
    // reached Joplin or Joplin still has the note before it.
    for reached in [true, false] {
        let notes = format!("{NOTES}> Who are you? ({reached})\n>\n> 40 %\n\n");
        t.storage.0.write(ALICE, notes.as_bytes()).unwrap();
        let calls = t.export(ALICE);
        let calls = t.checked(&calls[0]);
        let calls = t.answer(&calls[0], 200, &notebook);
        let calls = t.answer(&calls[0], 200, &joplin);
        assert!(calls[0].url.ends_with(&format!("/{id}.md")));
        if reached {
            joplin = t.body(&calls[0]);
        }
        assert!(t.answer(&calls[0], 0, "connection reset").is_empty());

        let calls = t.export(ALICE);
        let calls = t.checked(&calls[0]);
        let calls = t.answer(&calls[0], 200, &notebook);
        let calls = t.answer(&calls[0], 200, &joplin);
        assert_eq!(calls[0].method, "PUT", "{reached}");
        assert!(calls[0].url.ends_with(&format!("/{id}.md")), "{reached}");
        joplin = t.body(&calls[0]);
        assert!(t.answer(&calls[0], 204, "").is_empty());
        assert_eq!(t.last_said(), "The notes of 1 book are in Joplin.");
    }
}

#[test]
fn joplin_cloud_signs_in_and_again_when_the_session_ends() {
    let mut t = Test::new(CLOUD, false);
    let calls = t.export(ALICE);
    assert_eq!(calls[0].method, "POST");
    assert_eq!(calls[0].url, "https://api.joplincloud.com/api/sessions");
    assert_eq!(
        t.body(&calls[0]),
        "{\"email\":\"me@example.org\",\"password\":\"pw\"}"
    );
    let calls = t.answer(&calls[0], 200, r#"{"id": "S1", "user_id": "u"}"#);
    assert_eq!(
        calls[0].url,
        "https://api.joplincloud.com/api/items/root:/info.json:/content"
    );
    let session = (String::from("X-API-AUTH"), String::from("S1"));
    assert!(calls[0].headers.contains(&session));
    let calls = t.checked(&calls[0]);
    let calls = t.answer(&calls[0], 200, "{}");
    assert!(calls[0].url.ends_with(".md:/content"));
    assert!(t.answer(&calls[0], 200, "{}").is_empty());

    // The session is kept; once it ended, signed in anew.
    let calls = t.export(ALICE);
    assert!(calls[0].headers.contains(&session));
    let calls = t.answer(&calls[0], 403, "");
    assert_eq!(calls[0].method, "POST");
    let calls = t.answer(&calls[0], 200, r#"{"id": "S2"}"#);
    assert!(calls[0].url.ends_with("/info.json:/content"));
    assert!(
        calls[0]
            .headers
            .contains(&(String::from("X-API-AUTH"), String::from("S2")))
    );
    // Not let in again: the password is wrong.
    let calls = t.answer(&calls[0], 403, "");
    assert!(calls.is_empty());
    assert!(t.last_said().starts_with("Joplin did not accept"));
}

#[test]
fn notes_wait_without_a_connection_and_go_when_wifi_comes() {
    let mut t = Test::new(WEBDAV, true);
    let calls = t.export(ALICE);
    let calls = t.answer(&calls[0], 0, "connection refused");
    assert!(calls.is_empty());
    assert!(t.last_said().starts_with("Joplin cannot be reached."));
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    // Wi-Fi comes back: sent by itself.
    t.wifi(false);
    let calls = t.wifi(true);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].url.ends_with("/info.json"));
    let calls = t.checked(&calls[0]);
    let calls = t.answer(&calls[0], 201, "");
    assert!(t.answer(&calls[0], 201, "").is_empty());
    assert_eq!(t.file("waiting.txt"), "");
    // Nothing waits: Wi-Fi coming sends nothing.
    t.wifi(false);
    assert!(t.wifi(true).is_empty());
}

#[test]
fn a_wrong_password_stops_tries_until_it_is_changed() {
    let mut t = Test::new(WEBDAV, true);
    let calls = t.export(ALICE);
    assert!(t.answer(&calls[0], 401, "").is_empty());
    assert!(t.last_said().starts_with("Joplin did not accept"));
    t.wifi(false);
    assert!(t.wifi(true).is_empty());
    // A new password typed on the set-up: tried again by itself.
    let typed = Params::new()
        .with("text", "right")
        .with("field", "password");
    let now = t.now();
    t.kernel
        .send_event(now, Id::new_static("joplin.entered"), typed);
    t.requests();
    t.wifi(false);
    let calls = t.wifi(true);
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].headers[0],
        (
            String::from("Authorization"),
            paperpaw_sdk::web::basic_auth("me", "right")
        )
    );
}

#[test]
fn encrypted_targets_get_nothing() {
    let mut t = Test::new(WEBDAV, false);
    let calls = t.export(ALICE);
    let calls = t.answer(&calls[0], 200, &INFO.replace("false", "true"));
    assert!(calls.is_empty());
    assert!(
        t.last_said()
            .starts_with("Joplin's end-to-end encryption is on.")
    );
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
}

#[test]
fn nothing_is_written_while_a_client_upgrades_the_target() {
    // A WebDAV folder with an exclusive lock renewed a minute ago (the
    // clock says 08:00:02 by then), and a shared one.
    let mut t = Test::new(WEBDAV, true);
    let listing = |modified: &str| {
        format!(
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">\
             <d:response><d:href>/Joplin/locks/1_1_desktop.json</d:href><d:propstat><d:prop>\
             <d:getlastmodified>{modified}</d:getlastmodified></d:prop></d:propstat></d:response>\
             <d:response><d:href>/Joplin/locks/2_3_cli.json</d:href><d:propstat><d:prop>\
             <d:getlastmodified>{modified}</d:getlastmodified></d:prop></d:propstat></d:response>\
             </d:multistatus>"
        )
    };
    let calls = t.export(ALICE);
    let calls = t.answer(&calls[0], 200, INFO);
    assert_eq!(calls[0].method, "PROPFIND");
    let calls = t.answer(&calls[0], 207, &listing("Tue, 20 Oct 2026 07:59:02 GMT"));
    assert!(calls.is_empty());
    assert!(
        t.last_said()
            .starts_with("Joplin is upgrading the notes there")
    );
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    assert!(t.file("locks.txt").is_empty());
    // Tried again by itself; the lock has run out since (or was left by
    // a client that stopped): the notes go.
    t.wifi(false);
    let calls = t.wifi(true);
    let calls = t.answer(&calls[0], 200, INFO);
    let calls = t.answer(&calls[0], 207, &listing("Tue, 20 Oct 2026 07:50:00 GMT"));
    assert_eq!(calls[0].method, "PUT");

    // Joplin Server lists its locks with the time each was renewed.
    let mut t = Test::new(CLOUD, false);
    let calls = t.export(ALICE);
    let calls = t.answer(&calls[0], 200, r#"{"id": "S1"}"#);
    let calls = t.answer(&calls[0], 200, INFO);
    assert_eq!(calls[0].url, "https://api.joplincloud.com/api/locks");
    assert!(
        calls[0]
            .headers
            .contains(&(String::from("X-API-AUTH"), String::from("S1")))
    );
    let lock = format!(
        r#"{{"items": [{{"id": "l", "type": 2, "clientType": 1, "clientId": "d",
            "updatedTime": {}}}], "has_more": false}}"#,
        NOW - 30_000
    );
    assert!(t.answer(&calls[0], 200, &lock).is_empty());
    assert!(
        t.last_said()
            .starts_with("Joplin is upgrading the notes there")
    );
}

#[test]
fn targets_in_another_format_get_nothing() {
    let cases = [
        (
            r#""version": 4"#,
            "Joplin keeps the notes there in a newer format",
        ),
        (
            r#""version": 2"#,
            "Joplin keeps the notes there in an older format",
        ),
        (
            r#""versions": 3"#,
            "Joplin keeps the notes there in an older format",
        ),
    ];
    for (version, said) in cases {
        let mut t = Test::new(WEBDAV, false);
        let calls = t.export(ALICE);
        let info = INFO.replace(r#""version": 3"#, version);
        assert!(t.answer(&calls[0], 200, &info).is_empty(), "{version}");
        assert!(t.last_said().starts_with(said), "{version}");
        assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
    }
}

#[test]
fn a_note_that_fails_waits_and_the_others_go() {
    let mut t = Test::new(WEBDAV, false);
    let bob = "Notes/Bob.md";
    t.storage.0.write(bob, b"# Bob\n\nSomeone\n").unwrap();
    // Two books wait; the action sends both.
    t.storage
        .0
        .write(
            ".paperpaw/plugins/joplin/waiting.txt",
            format!("{ALICE}\n{bob}\nNotes/Deleted.md\n").as_bytes(),
        )
        .unwrap();
    let now = t.now();
    t.kernel.run_action(now, Id::new_static("joplin.send"));
    let calls = t.requests();
    let calls = t.checked(&calls[0]);
    let calls = t.answer(&calls[0], 201, "");
    let calls = t.answer(&calls[0], 507, "Insufficient Storage");
    assert!(t.body(&calls[0]).starts_with("Bob\n\nSomeone\n\nid: "));
    assert!(t.answer(&calls[0], 201, "").is_empty());
    assert_eq!(
        t.last_said(),
        "Sent to Joplin: 1. Not sent: 1; they are sent again later."
    );
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
}

#[test]
fn without_an_account_the_set_up_opens() {
    let mut t = Test::new("[joplin]\nservice = joplin.service.server\n", false);
    assert!(t.export(ALICE).is_empty());
    assert_eq!(
        t.kernel.active_screen().map(Id::as_str),
        Some("joplin.setup")
    );
    assert!(t.last_said().starts_with("Set up Joplin first"));
    assert_eq!(t.file("waiting.txt"), format!("{ALICE}\n"));
}
