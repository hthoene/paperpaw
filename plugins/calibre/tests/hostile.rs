//! Sessions with the real kernel where the other side misbehaves or the
//! connection fails: unknown opcodes, truncated and oversized messages,
//! books cut short, paths out of the library (also on deleting), books
//! that are no longer there, free space, the password asked for, Calibre
//! not found, and no network.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{Io, Kernel, SocketCall, SocketEvent};
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_calibre::Calibre;
use paperpaw_plugin_calibre::protocol::{self, frame, op};
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

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

/// The library is `books` unless the configuration says otherwise.
const CONFIG: &str = "[keymap.global]\nbutton.calibre = calibre.open\n\
    [calibre]\naddress = 10.0.0.2\n[core]\nlibrary-folder = books\n";

/// A device with the Calibre screen open, and Calibre's side of the
/// connection.
struct Session {
    kernel: Kernel,
    storage: Shared,
    socket: u32,
    /// What Calibre still has to send.
    wire: Vec<u8>,
    /// What came from the device.
    got: Vec<u8>,
    /// The device's read waiting for bytes.
    waiting: Option<u32>,
    closed: bool,
    /// Bytes given at most per read, as a socket may bring less.
    chunk: usize,
    now: u64,
}

fn socket_calls(kernel: &mut Kernel) -> Vec<SocketCall> {
    kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Socket(call) => Some(call),
            _ => None,
        })
        .collect()
}

fn open(storage: &Shared, config: &str, network: bool) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .user_config(config)
        .network(network)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        // Calibre writes only books of formats PaperPaw reads.
        .plugin(paperpaw_plugin_format_epub::EpubFormat)
        .plugin(Calibre)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    for state in [ButtonState::Pressed, ButtonState::Released] {
        kernel.handle(
            Instant(1),
            InputEvent::Button {
                name: "calibre",
                state,
            },
        );
    }
    kernel
}

impl Session {
    /// Connected to Calibre at the address set.
    fn connected(storage: Shared, config: &str) -> Self {
        let mut kernel = open(&storage, config, true);
        let calls = socket_calls(&mut kernel);
        let [SocketCall::Connect { socket, .. }] = calls.as_slice() else {
            panic!("{calls:?}");
        };
        let socket = *socket;
        kernel.socket_event(Instant(2), SocketEvent::Connected { socket });
        let mut session = Self {
            kernel,
            storage,
            socket,
            wire: Vec::new(),
            got: Vec::new(),
            waiting: None,
            closed: false,
            chunk: usize::MAX,
            now: 10,
        };
        session.pump();
        assert!(session.waiting.is_some());
        session
    }

    fn new() -> Self {
        Self::connected(Shared::default(), CONFIG)
    }

    fn pump(&mut self) {
        loop {
            for call in socket_calls(&mut self.kernel) {
                match call {
                    SocketCall::Send { data, .. } => self.got.extend_from_slice(&data),
                    SocketCall::Close { .. } => self.closed = true,
                    SocketCall::Read { max, .. } => self.waiting = Some(max),
                    other => panic!("unexpected {other:?}"),
                }
            }
            match self.waiting {
                Some(max) if !self.wire.is_empty() && !self.closed => {
                    self.waiting = None;
                    let n = (max as usize).min(self.chunk).min(self.wire.len());
                    let data: Vec<u8> = self.wire.drain(..n).collect();
                    self.now += 1;
                    let socket = self.socket;
                    self.kernel
                        .socket_event(Instant(self.now), SocketEvent::Data { socket, data });
                }
                _ => return,
            }
        }
    }

    fn say(&mut self, op: u8, args: &str) {
        self.wire.extend_from_slice(&frame(op, args));
        self.pump();
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.wire.extend_from_slice(bytes);
        self.pump();
    }

    /// The connection ends from Calibre's side, or fails.
    fn event(&mut self, event: SocketEvent) {
        self.now += 1;
        self.kernel.socket_event(Instant(self.now), event);
        self.pump();
    }

    /// The messages the device sent since the last call.
    fn answers(&mut self) -> Vec<protocol::Message> {
        let mut framing = protocol::Framing::default();
        let mut out = Vec::new();
        let text = String::from_utf8(std::mem::take(&mut self.got)).unwrap();
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let n = (framing.wanted() as usize).min(rest.len());
            let (part, tail) = rest.split_at(n);
            rest = tail;
            if let Some(message) = framing.feed(part).unwrap() {
                out.push(message);
            }
        }
        out
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Sends a book of `bytes` with `lpath` and the metadata `title`.
    fn book(&mut self, lpath: &str, bytes: &[u8]) {
        let message = format!(
            r#"{{"lpath": {lpath}, "length": {}, "thisBook": 0, "totalBooks": 1,
                "metadata": {{"lpath": {lpath}, "uuid": "u-1", "title": "T"}}}}"#,
            bytes.len(),
            lpath = paperpaw_sdk::json::string(lpath),
        );
        self.wire.extend_from_slice(&frame(op::SEND_BOOK, &message));
        self.wire.extend_from_slice(bytes);
        self.pump();
    }

    /// Whether the session still answers: a keep-alive gets its OK.
    fn alive(&mut self) -> bool {
        self.answers();
        self.say(op::NOOP, "{}");
        !self.closed && self.answers().len() == 1
    }
}

#[test]
fn unknown_opcodes_and_quiet_ones_get_no_answer_and_the_session_goes_on() {
    let mut s = Session::new();
    for op in [
        10,
        21,
        99,
        255,
        op::OK,
        op::BOOK_DONE,
        op::CALIBRE_BUSY,
        op::SEND_BOOKLISTS,
    ] {
        s.say(op, r#"{"something": [1, 2, {"x": null}]}"#);
        assert!(s.answers().is_empty(), "{op}");
    }
    assert!(s.alive());
    // Metadata asked for: PaperPaw sends no books back.
    s.say(op::GET_BOOK_FILE_SEGMENT, "{}");
    let answers = s.answers();
    assert_eq!(answers[0].op, op::ERROR);
    assert!(s.alive());
}

#[test]
fn a_message_that_is_no_protocol_ends_the_connection() {
    for wire in [
        &b"GET / HTTP/1.1\r\n\r\n"[..],
        b"123456789[0, {}]",
        b"2000000[",
        b"0[]",
        b"6[a, b]",
        b"\xff\xfe\x00",
    ] {
        let mut s = Session::new();
        s.raw(wire);
        assert!(s.closed, "{}", String::from_utf8_lossy(wire));
        let shown = s.shown();
        assert!(
            shown.contains("does not speak Calibre's protocol"),
            "{shown}"
        );
    }
}

#[test]
fn a_connection_ending_mid_message_is_said() {
    let mut s = Session::new();
    s.raw(b"100[0, {\"a\":");
    assert!(!s.closed);
    // Calibre closes: the read brings nothing.
    let socket = s.socket;
    s.event(SocketEvent::Data {
        socket,
        data: Vec::new(),
    });
    let shown = s.shown();
    assert!(shown.contains("The connection has ended."), "{shown}");
}

#[test]
fn messages_coming_a_few_bytes_at_a_time_are_read() {
    let mut s = Session::new();
    s.chunk = 3;
    s.say(
        op::SET_LIBRARY_INFO,
        r#"{"libraryName": "Caf\u00e9 Books", "libraryUuid": "x"}"#,
    );
    s.say(op::FREE_SPACE, "{}");
    let answers = s.answers();
    assert_eq!(answers.len(), 2);
    assert!(s.shown().contains("Library: Café Books"));
    s.book("Ann/A.epub", b"0123456789abcdef");
    assert_eq!(
        s.storage.0.get("books/Ann/A.epub").unwrap(),
        b"0123456789abcdef"
    );
}

#[test]
fn free_and_total_space_are_large_and_agree() {
    let mut s = Session::new();
    s.say(op::TOTAL_SPACE, "{}");
    s.say(op::FREE_SPACE, "{}");
    let answers = s.answers();
    assert_eq!(answers.len(), 2);
    let total = protocol::number(&answers[0].args, "total_space_on_device").unwrap();
    let free = protocol::number(&answers[1].args, "free_space_on_device").unwrap();
    // Calibre refuses to send to a device it thinks full.
    assert!(free >= 1 << 30, "{free}");
    assert!(free <= total);
}

#[test]
fn a_book_cut_short_leaves_no_part_behind() {
    for event in ["closed", "failed"] {
        let mut s = Session::new();
        let message = r#"{"lpath": "Ann/A.epub", "length": 1000, "metadata": {"title": "A"}}"#;
        s.wire.extend_from_slice(&frame(op::SEND_BOOK, message));
        s.wire.extend_from_slice(&[7; 100]);
        s.pump();
        assert!(s.shown().contains("Receiving “A” (1 of 1)"));
        assert!(s.storage.exists("books/Ann/A.epub.part"));
        let socket = s.socket;
        if event == "closed" {
            s.event(SocketEvent::Data {
                socket,
                data: Vec::new(),
            });
        } else {
            s.event(SocketEvent::Failed {
                socket,
                reason: String::from("connection reset"),
            });
        }
        assert!(!s.storage.exists("books/Ann/A.epub.part"), "{event}");
        assert!(!s.storage.exists("books/Ann/A.epub"), "{event}");
        let shown = s.shown();
        assert!(shown.contains("The connection"), "{event}: {shown}");
    }
}

#[test]
fn a_book_sent_again_replaces_the_old_one() {
    let mut s = Session::new();
    s.book("Ann/A.epub", b"first version");
    s.book("Ann/A.epub", b"second");
    assert_eq!(s.storage.0.get("books/Ann/A.epub").unwrap(), b"second");
    assert!(s.alive());
    assert!(s.shown().contains("2 books received"));
}

#[test]
fn empty_and_negative_lengths_give_an_empty_book_and_go_on() {
    let mut s = Session::new();
    s.say(
        op::SEND_BOOK,
        r#"{"lpath": "Ann/Empty.epub", "length": -5, "metadata": {}}"#,
    );
    assert_eq!(s.storage.0.get("books/Ann/Empty.epub").unwrap(), b"");
    assert!(s.alive());
}

#[test]
fn books_with_paths_out_of_the_library_are_dropped() {
    let storage = Shared::default();
    storage.write("outside.epub", b"keep me").unwrap();
    storage
        .write("paperpaw.ini", b"[core]\nlibrary-folder = books\n")
        .unwrap();
    let mut s = Session::connected(storage, CONFIG);
    for lpath in [
        "../outside.epub",
        "..\\outside.epub",
        "a/../../paperpaw.ini",
        "C:\\x.epub",
        "",
    ] {
        s.book(lpath, b"evil");
        assert!(s.alive(), "{lpath}");
    }
    assert_eq!(s.storage.0.get("outside.epub").unwrap(), b"keep me");
    assert_eq!(
        s.storage.0.get("paperpaw.ini").unwrap(),
        b"[core]\nlibrary-folder = books\n"
    );
    assert!(!s.storage.exists(".paperpaw/plugins/calibre/dropped.part"));
    assert!(!s.shown().contains("received"));
    // An absolute path is taken as one below the library.
    s.book("/etc/x.epub", b"fine");
    assert_eq!(s.storage.0.get("books/etc/x.epub").unwrap(), b"fine");
}

/// With the library at the storage's top, a book path into `.paperpaw`
/// must not overwrite PaperPaw's own data.
#[test]
fn books_never_land_in_paperpaws_data_folder() {
    // The library folder may be the storage's top (`core.library-folder` empty);
    // whoever answers the broadcast can then name any hidden path.
    let storage = Shared::default();
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep("other.key", "", "kept"));
    storage.write(FILE, secrets.encode().as_bytes()).unwrap();
    let config = "[keymap.global]\nbutton.calibre = calibre.open\n\
        [calibre]\naddress = 10.0.0.2\n[core]\nlibrary-folder =\n";
    let mut s = Session::connected(storage, config);
    s.book(".paperpaw/secrets.txt", b"overwritten");
    assert_eq!(
        s.storage.0.get(FILE).unwrap(),
        secrets.encode().into_bytes(),
        "the secrets were overwritten"
    );
}

#[test]
fn deleting_never_reaches_out_of_the_library_nor_the_readers_own_books() {
    let storage = Shared::default();
    storage.write("outside.epub", b"keep me").unwrap();
    // A book of the reader's own, not sent by Calibre.
    storage.write("books/Ann/Own.epub", b"mine").unwrap();
    let mut s = Session::connected(storage, CONFIG);
    s.book("Ann/A.epub", b"a");
    s.answers();
    s.say(
        op::DELETE_BOOK,
        r#"{"lpaths": ["../outside.epub", "..\\outside.epub", "/../outside.epub", "Ann/A.epub", "Ann/Own.epub", "Never/Sent.epub"]}"#,
    );
    let answers = s.answers();
    // An OK, then one for each book, with the uuid if it was known.
    assert_eq!(answers.len(), 7);
    let uuids: Vec<String> = answers[1..]
        .iter()
        .map(|a| protocol::text(&a.args, "uuid").unwrap())
        .collect();
    assert_eq!(uuids, ["", "", "", "u-1", "", ""]);
    assert!(s.storage.exists("outside.epub"));
    assert!(!s.storage.exists("books/Ann/A.epub"));
    assert_eq!(s.storage.0.get("books/Ann/Own.epub").unwrap(), b"mine");
    assert!(s.alive());
}

#[test]
fn books_gone_from_the_device_or_with_bad_records_are_not_listed() {
    let mut s = Session::new();
    s.book("Ann/A.epub", b"a");
    s.book("Bo/B.epub", b"b");
    // B deleted on the device; a broken record and one that points out,
    // named as records are (an id of 32 hex digits), and a file that is
    // no record. Through the kernel's storage, as a file manager would.
    let storage = s.kernel.storage();
    storage.remove("books/Bo/B.epub").unwrap();
    let records = ".paperpaw/plugins/calibre/books";
    storage
        .write(
            &format!("{records}/0123456789abcdef0123456789abcdef.json"),
            &[0xff, 0x00],
        )
        .unwrap();
    storage
        .write(
            &format!("{records}/fedcba9876543210fedcba9876543210.json"),
            br#"{"lpath": "../../outside.epub"}"#,
        )
        .unwrap();
    storage
        .write(
            &format!("{records}/out.json"),
            br#"{"lpath": "Ann/A.epub"}"#,
        )
        .unwrap();
    storage.write("outside.epub", b"x").unwrap();
    s.answers();
    s.say(op::GET_BOOK_COUNT, r#"{"canStream": true}"#);
    // The count at once, the books' metadata in the next turns.
    for _ in 0..10 {
        s.now += 100;
        s.kernel.tick(Instant(s.now));
        s.pump();
    }
    let answers = s.answers();
    assert_eq!(
        protocol::number(&answers[0].args, "count"),
        Some(1),
        "{answers:?}"
    );
    assert_eq!(answers.len(), 2);
    assert_eq!(
        protocol::text(&answers[1].args, "lpath").as_deref(),
        Some("Ann/A.epub")
    );
}

#[test]
fn the_password_is_asked_for_kept_and_answers_the_challenge() {
    let mut s = Session::new();
    s.say(
        op::DISPLAY_MESSAGE,
        r#"{"messageKind": 1, "message": "Invalid password"}"#,
    );
    assert!(s.closed);
    assert!(s.shown().contains("Calibre asks for a password."));
    // Typed on the keyboard: kept, and connected again.
    s.now += 1;
    s.kernel.send_event(
        Instant(s.now),
        Id::new_static("calibre.password-typed"),
        Params::new().with("text", "pässword"),
    );
    assert_eq!(s.kernel.secrets().get("calibre.password"), Some("pässword"));
    let calls = socket_calls(&mut s.kernel);
    let [SocketCall::Connect { socket, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    s.socket = *socket;
    s.closed = false;
    s.waiting = None;
    s.event(SocketEvent::Connected { socket: *socket });
    s.say(
        op::GET_INITIALIZATION_INFO,
        r#"{"passwordChallenge": "1234567890", "validExtensions": []}"#,
    );
    let answers = s.answers();
    let expected = paperpaw_sdk::digest::sha1::hex("pässword1234567890".as_bytes());
    assert_eq!(
        protocol::text(&answers[0].args, "passwordHash"),
        Some(expected)
    );
}

#[test]
fn an_empty_password_typed_changes_nothing() {
    let mut s = Session::new();
    s.say(op::DISPLAY_MESSAGE, r#"{"messageKind": 1}"#);
    s.now += 1;
    s.kernel.send_event(
        Instant(s.now),
        Id::new_static("calibre.password-typed"),
        Params::new().with("text", ""),
    );
    assert_eq!(s.kernel.secrets().get("calibre.password"), None);
    assert!(socket_calls(&mut s.kernel).is_empty());
    assert!(s.shown().contains("Calibre asks for a password."));
}

#[test]
fn an_error_of_calibre_ends_with_its_message() {
    let mut s = Session::new();
    s.say(op::ERROR, r#"{"message": "Ünexpected ✗"}"#);
    assert!(s.closed);
    let shown = s.shown();
    assert!(
        shown.contains("The connection failed: Ünexpected ✗"),
        "{shown}"
    );
}

#[test]
fn calibre_not_found_or_no_network_is_said_and_tried_again() {
    let storage = Shared::default();
    let config = "[keymap.global]\nbutton.calibre = calibre.open\n";
    let mut kernel = open(&storage, config, true);
    let calls = socket_calls(&mut kernel);
    let [SocketCall::Broadcast { broadcast, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    let broadcast = *broadcast;
    // Answers of others are no Calibre.
    for data in [
        &b"hello"[..],
        b"calibre wireless device client (on pc);x",
        b"\xff\xfe",
    ] {
        kernel.socket_event(
            Instant(2),
            SocketEvent::Answer {
                broadcast,
                from: String::from("10.0.0.9"),
                data: data.to_vec(),
            },
        );
        assert!(socket_calls(&mut kernel).is_empty());
    }
    // Heard by no Calibre, each broadcast sent again, until it counts as
    // not found.
    let mut last = broadcast;
    loop {
        kernel.socket_event(Instant(3), SocketEvent::Done { broadcast: last });
        match socket_calls(&mut kernel).as_slice() {
            [SocketCall::Broadcast { broadcast, .. }] => last = *broadcast,
            [] => break,
            other => panic!("{other:?}"),
        }
    }
    kernel.render(&mut |_, _| {});
    let shown: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(
        shown.iter().any(|l| l.contains("Calibre was not found")),
        "{shown:?}"
    );
    // A late answer after the search ended connects nowhere.
    kernel.socket_event(
        Instant(4),
        SocketEvent::Answer {
            broadcast,
            from: String::from("10.0.0.9"),
            data: b"calibre wireless device client (on pc);9090".to_vec(),
        },
    );
    assert!(socket_calls(&mut kernel).is_empty());
    // Confirm searches again.
    kernel.run_action(Instant(5), paperpaw_sdk::builtin::actions::CONFIRM);
    let calls = socket_calls(&mut kernel);
    assert!(matches!(calls.as_slice(), [SocketCall::Broadcast { .. }]));

    let mut kernel = open(&storage, config, false);
    assert!(socket_calls(&mut kernel).is_empty());
    kernel.render(&mut |_, _| {});
    let shown: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(shown.iter().any(|l| l.contains("No Wi-Fi")), "{shown:?}");
}
