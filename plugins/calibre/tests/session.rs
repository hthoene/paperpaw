//! A whole session with Calibre as its driver behaves: found by its
//! answer to the broadcast, then initialisation, a book sent, the books
//! listed, the book deleted, the end. Calibre's bytes come as the plugin
//! asks for them, never more. And what a peer posing as Calibre cannot
//! do: be connected to unasked, write anything but books, replace or
//! delete the reader's own files, send messages larger than the device
//! holds.

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
use paperpaw_plugin_format_epub::EpubFormat;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
    Storage, StorageError,
};

const DEVICE: DeviceInfo = device(MemoryClass::Large);

/// The test device with `memory`.
const fn device(memory: MemoryClass) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(300, 400, 100, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, false, false, &[]),
        memory,
    )
}

thread_local! {
    /// Renames fail on this test's thread, as on a failing card.
    static FAIL_RENAME: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The space the storage tells on this test's thread: free, in all.
    static SPACE: std::cell::Cell<Option<(u64, u64)>> = const { std::cell::Cell::new(None) };
}

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
        if FAIL_RENAME.get() {
            return Err(StorageError::Io);
        }
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
    fn free_space(&self) -> Option<u64> {
        SPACE.get().map(|(free, _)| free)
    }
    fn total_space(&self) -> Option<u64> {
        SPACE.get().map(|(_, total)| total)
    }
}

/// Calibre's side of the socket: what it still has to send, what came
/// from the device, and the device's read waiting for bytes.
#[derive(Default)]
struct Peer {
    wire: Vec<u8>,
    got: Vec<u8>,
    waiting: Option<(u32, u32)>,
    closed: bool,
    /// At most this many bytes a read, as a network may deliver them
    /// (`None`: as many as asked for).
    chunk: Option<usize>,
}

impl Peer {
    /// Serves the kernel's socket calls: sends are kept, a read is
    /// answered with as much as it asks of what is on the wire.
    fn pump(&mut self, kernel: &mut Kernel) {
        loop {
            for call in socket_calls(kernel) {
                match call {
                    SocketCall::Send { data, .. } => self.got.extend_from_slice(&data),
                    SocketCall::Close { .. } => self.closed = true,
                    SocketCall::Read { socket, max } => self.waiting = Some((socket, max)),
                    other => panic!("unexpected {other:?}"),
                }
            }
            match self.waiting {
                Some((socket, max)) if !self.wire.is_empty() => {
                    self.waiting = None;
                    let n = (max as usize)
                        .min(self.wire.len())
                        .min(self.chunk.unwrap_or(usize::MAX));
                    let data: Vec<u8> = self.wire.drain(..n).collect();
                    kernel.socket_event(Instant(5), SocketEvent::Data { socket, data });
                }
                _ => return,
            }
        }
    }

    /// Calibre closes the connection: the device's read gets nothing.
    fn close(&mut self, kernel: &mut Kernel) {
        let (socket, _) = self.waiting.take().expect("a read waiting");
        kernel.socket_event(
            Instant(6),
            SocketEvent::Data {
                socket,
                data: Vec::new(),
            },
        );
        self.pump(kernel);
    }

    fn say(&mut self, kernel: &mut Kernel, op: u8, args: &str) {
        self.wire.extend_from_slice(&frame(op, args));
        self.pump(kernel);
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
}

const CONFIG: &str = "[keymap.global]\nbutton.calibre = calibre.open\n";

fn press(kernel: &mut Kernel, name: &'static str) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        kernel.handle(Instant(1), InputEvent::Button { name, state });
    }
}

fn kernel(storage: &Shared, config: &str) -> Kernel {
    kernel_on(DEVICE, storage, config)
}

/// The library at the storage's top, unless `config` says otherwise.
fn kernel_on(device: DeviceInfo, storage: &Shared, config: &str) -> Kernel {
    let config = format!("[core]\nlibrary-folder =\n{config}");
    let mut kernel = Kernel::builder(device)
        .storage(storage.clone())
        .user_config(config)
        .network(true)
        .plugin(Calibre)
        .plugin(EpubFormat)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, "calibre");
    kernel
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn a_session_with_calibre() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let mut kernel = kernel(&storage, CONFIG);

    // Looking for Calibre: a broadcast to its ports, and its answer.
    let calls = socket_calls(&mut kernel);
    let [
        SocketCall::Broadcast {
            broadcast, ports, ..
        },
    ] = calls.as_slice()
    else {
        panic!("{calls:?}");
    };
    assert_eq!(ports.as_slice(), protocol::BROADCAST_PORTS);
    kernel.socket_event(
        Instant(2),
        SocketEvent::Answer {
            broadcast: *broadcast,
            from: String::from("192.168.1.5"),
            data: b"calibre wireless device client (on pc);8080,9091".to_vec(),
        },
    );
    // Found, and connected once the reader says so.
    assert!(socket_calls(&mut kernel).is_empty());
    press(&mut kernel, "confirm");
    let calls = socket_calls(&mut kernel);
    let [SocketCall::Connect { socket, host, port }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!((host.as_str(), *port), ("192.168.1.5", 9091));
    kernel.socket_event(Instant(3), SocketEvent::Connected { socket: *socket });
    let mut calibre = Peer::default();
    calibre.pump(&mut kernel);
    assert!(calibre.waiting.is_some(), "reads once connected");

    // Initialisation: the device says what it can.
    calibre.say(
        &mut kernel,
        op::GET_INITIALIZATION_INFO,
        r#"{"serverProtocolVersion": 1, "validExtensions": ["epub", "mobi"],
            "passwordChallenge": "", "currentLibraryName": "Books"}"#,
    );
    let answers = calibre.answers();
    assert_eq!(answers.len(), 1);
    let info = &answers[0];
    assert_eq!(info.op, op::OK);
    for flag in [
        "versionOK",
        "canStreamBooks",
        "canStreamMetadata",
        "canReceiveBookBinary",
        "canDeleteMultipleBooks",
    ] {
        assert_eq!(
            protocol::text(&info.args, flag).as_deref(),
            Some("true"),
            "{flag}"
        );
    }
    assert!(protocol::number(&info.args, "maxBookContentPacketLen").unwrap() > 0);
    calibre.say(
        &mut kernel,
        op::SET_CALIBRE_DEVICE_INFO,
        r#"{"device_store_uuid": "store-1", "device_name": "PaperPaw"}"#,
    );
    calibre.say(&mut kernel, op::GET_DEVICE_INFORMATION, "{}");
    calibre.say(&mut kernel, op::FREE_SPACE, "{}");
    let answers = calibre.answers();
    assert_eq!(answers.len(), 3);
    let device_info = protocol::member(&answers[1].args, "device_info").unwrap();
    assert_eq!(
        protocol::text(device_info, "device_store_uuid").as_deref(),
        Some("store-1")
    );
    assert!(protocol::number(&answers[2].args, "free_space_on_device").unwrap() > 0);

    // A book: its message, then its bytes, of which the device takes
    // exactly as many as Calibre said.
    let book = b"PK\x03\x04 an epub, not text \xff\xfe".to_vec();
    let message = format!(
        r#"{{"lpath": "Ann Author/A Book.epub", "length": {}, "thisBook": 0, "totalBooks": 1,
            "metadata": {{"lpath": "Ann Author/A Book.epub", "uuid": "uuid-a", "title": "A Book"}},
            "willStreamBooks": true, "willStreamBinary": true, "wantsSendOkToSendbook": false}}"#,
        book.len()
    );
    calibre
        .wire
        .extend_from_slice(&frame(op::SEND_BOOK, &message));
    calibre.wire.extend_from_slice(&book);
    calibre.pump(&mut kernel);
    assert!(calibre.answers().is_empty(), "no answer to a book");
    assert!(calibre.wire.is_empty());
    let saved = paperpaw_sdk::read_file(&storage, "Ann Author/A Book.epub", 1 << 20).unwrap();
    assert_eq!(saved, book);
    assert!(!storage.exists("Ann Author/A Book.epub.part"));

    // The books on the device: the one sent, with its metadata.
    calibre.say(
        &mut kernel,
        op::GET_BOOK_COUNT,
        r#"{"canStream": true, "willUseCachedMetadata": false}"#,
    );
    // The count at once, the books' metadata in the next turn.
    kernel.tick(Instant(10));
    calibre.pump(&mut kernel);
    let answers = calibre.answers();
    assert_eq!(protocol::number(&answers[0].args, "count"), Some(1));
    assert_eq!(
        protocol::text(&answers[1].args, "uuid").as_deref(),
        Some("uuid-a")
    );

    // Deleted from Calibre: the file goes, Calibre hears its uuid.
    calibre.say(
        &mut kernel,
        op::DELETE_BOOK,
        r#"{"lpaths": ["Ann Author/A Book.epub"]}"#,
    );
    let answers = calibre.answers();
    assert_eq!(answers.len(), 2);
    assert_eq!(
        protocol::text(&answers[1].args, "uuid").as_deref(),
        Some("uuid-a")
    );
    assert!(!storage.exists("Ann Author/A Book.epub"));

    // A keep-alive is answered; ejecting is answered, and the device
    // lets Calibre close (closing first, its answer could be lost on the
    // way), then closes too.
    calibre.say(&mut kernel, op::NOOP, "{}");
    assert_eq!(calibre.answers().len(), 1);
    calibre.say(&mut kernel, op::NOOP, r#"{"ejecting": true}"#);
    assert_eq!(calibre.answers().len(), 1);
    assert!(!calibre.closed);
    calibre.close(&mut kernel);
    assert!(calibre.closed);
}

#[test]
fn a_path_out_of_the_library_is_dropped() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let config = format!("{CONFIG}[calibre]\naddress = 10.0.0.2\n");
    let mut kernel = kernel(&storage, &config);
    let calls = socket_calls(&mut kernel);
    let [SocketCall::Connect { socket, host, port }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!((host.as_str(), *port), ("10.0.0.2", protocol::DEFAULT_PORT));
    kernel.socket_event(Instant(3), SocketEvent::Connected { socket: *socket });
    let mut calibre = Peer::default();
    calibre.wire.extend_from_slice(&frame(
        op::SEND_BOOK,
        r#"{"lpath": "../outside.epub", "length": 3, "metadata": {}}"#,
    ));
    calibre.wire.extend_from_slice(b"abc");
    // The device goes on reading after the dropped book.
    calibre.wire.extend_from_slice(&frame(op::NOOP, "{}"));
    calibre.pump(&mut kernel);
    assert!(!storage.exists("../outside.epub") && !storage.exists("outside.epub"));
    assert!(!storage.exists(".paperpaw/plugins/calibre/dropped.part"));
    assert_eq!(calibre.answers().len(), 1, "the NOOP after it was answered");
}

#[test]
fn paperpaws_own_data_is_out_of_reach() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    storage
        .write(".paperpaw/firmware-keys.txt", b"own key")
        .unwrap();
    storage.write(".paperpaw/secrets.txt", b"password").unwrap();
    let config = format!("{CONFIG}[calibre]\naddress = 10.0.0.2\n");
    let mut kernel = kernel(&storage, &config);
    let calls = socket_calls(&mut kernel);
    let [SocketCall::Connect { socket, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    kernel.socket_event(Instant(3), SocketEvent::Connected { socket: *socket });
    let mut calibre = Peer::default();
    // A hostile peer sends a key to trust, and deletes the passwords.
    for lpath in [".paperpaw/firmware-keys.txt", ".PaperPaw/firmware-keys.txt"] {
        calibre.wire.extend_from_slice(&frame(
            op::SEND_BOOK,
            &format!(r#"{{"lpath": "{lpath}", "length": 3, "metadata": {{}}}}"#),
        ));
        calibre.wire.extend_from_slice(b"bad");
    }
    calibre.wire.extend_from_slice(&frame(
        op::DELETE_BOOK,
        r#"{"lpaths": [".paperpaw/secrets.txt"]}"#,
    ));
    calibre.pump(&mut kernel);
    assert_eq!(
        paperpaw_sdk::read_file(&storage, ".paperpaw/firmware-keys.txt", 64).unwrap(),
        b"own key"
    );
    assert!(!storage.exists(".paperpaw/firmware-keys.txt.part"));
    assert_eq!(
        paperpaw_sdk::read_file(&storage, ".paperpaw/secrets.txt", 64).unwrap(),
        b"password"
    );
}

/// The broadcast looking for Calibre.
fn broadcast(kernel: &mut Kernel) -> u32 {
    let calls = socket_calls(kernel);
    calls
        .iter()
        .find_map(|call| match call {
            SocketCall::Broadcast { broadcast, .. } => Some(*broadcast),
            _ => None,
        })
        .expect("a broadcast")
}

/// Calibre answers `broadcast` from `from`; the kernel's socket calls
/// then.
fn answer(kernel: &mut Kernel, broadcast: u32, from: &str) -> Vec<SocketCall> {
    kernel.socket_event(
        Instant(2),
        SocketEvent::Answer {
            broadcast,
            from: String::from(from),
            data: b"calibre wireless device client (on pc);8080,9090".to_vec(),
        },
    );
    socket_calls(kernel)
}

/// Calibre is told the space the card has free and in all, asked at
/// once (a card that cannot tell yet: room for books all the same).
#[test]
fn calibre_is_told_the_space_the_card_has() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let (mut kernel, mut calibre) = connected(DEVICE, &storage);
    SPACE.set(Some((3_000_000_000, 31_000_000_000)));
    calibre.say(&mut kernel, op::FREE_SPACE, "{}");
    calibre.say(&mut kernel, op::TOTAL_SPACE, "{}");
    let answers = calibre.answers();
    assert_eq!(
        protocol::number(&answers[0].args, "free_space_on_device"),
        Some(3_000_000_000)
    );
    assert_eq!(
        protocol::number(&answers[1].args, "total_space_on_device"),
        Some(31_000_000_000)
    );
    // Free space told, the size not: never less than what is free.
    SPACE.set(Some((900_000_000_000, 0)));
    calibre.say(&mut kernel, op::TOTAL_SPACE, "{}");
    let total = protocol::number(&calibre.answers()[0].args, "total_space_on_device");
    assert!(total >= Some(900_000_000_000), "{total:?}");
    SPACE.set(None);
    calibre.say(&mut kernel, op::FREE_SPACE, "{}");
    let free = protocol::number(&calibre.answers()[0].args, "free_space_on_device");
    assert!(free > Some(1 << 30), "{free:?}");
}

/// A broadcast that Calibre did not hear (a datagram lost on the way) is
/// sent again a few times before Calibre counts as not found (said, see
/// `hostile.rs`); an answer to a later one finds it.
#[test]
fn calibre_is_looked_for_again_when_it_does_not_answer() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let mut kernel = kernel(&storage, CONFIG);
    let mut rounds = 0;
    let mut last = broadcast(&mut kernel);
    loop {
        rounds += 1;
        kernel.socket_event(Instant(2), SocketEvent::Done { broadcast: last });
        let calls = socket_calls(&mut kernel);
        let Some(next) = calls.iter().find_map(|call| match call {
            SocketCall::Broadcast { broadcast, .. } => Some(*broadcast),
            _ => None,
        }) else {
            break;
        };
        last = next;
        assert!(rounds < 10, "looks for ever");
    }
    assert!(rounds >= 3, "{rounds} broadcasts");

    // Heard the second time.
    let mut kernel = kernel_on(DEVICE, &storage, CONFIG);
    let first = broadcast(&mut kernel);
    kernel.socket_event(Instant(2), SocketEvent::Done { broadcast: first });
    let second = broadcast(&mut kernel);
    answer(&mut kernel, second, "192.168.1.5");
    press(&mut kernel, "confirm");
    let calls = socket_calls(&mut kernel);
    assert!(
        matches!(calls.as_slice(), [SocketCall::Connect { .. }]),
        "{calls:?}"
    );
}

/// A kernel connected to Calibre at the address set, and Calibre.
fn connected(device: DeviceInfo, storage: &Shared) -> (Kernel, Peer) {
    let config = format!("{CONFIG}[calibre]\naddress = 10.0.0.2\n");
    let mut kernel = kernel_on(device, storage, &config);
    let calls = socket_calls(&mut kernel);
    let [SocketCall::Connect { socket, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    kernel.socket_event(Instant(3), SocketEvent::Connected { socket: *socket });
    let mut calibre = Peer::default();
    calibre.pump(&mut kernel);
    (kernel, calibre)
}

/// Calibre sends a book of `bytes` at `lpath`.
fn send_book(calibre: &mut Peer, kernel: &mut Kernel, lpath: &str, bytes: &[u8]) {
    let message = format!(
        r#"{{"lpath": "{lpath}", "length": {}, "metadata": {{"lpath": "{lpath}", "title": "T"}}}}"#,
        bytes.len()
    );
    calibre
        .wire
        .extend_from_slice(&frame(op::SEND_BOOK, &message));
    calibre.wire.extend_from_slice(bytes);
    calibre.pump(kernel);
}

#[test]
fn only_a_computer_the_reader_chose_is_connected() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let mut device = kernel(&storage, CONFIG);
    // Found: not connected unasked.
    let found = broadcast(&mut device);
    assert!(answer(&mut device, found, "192.168.1.5").is_empty());
    press(&mut device, "confirm");
    let calls = socket_calls(&mut device);
    assert!(
        matches!(calls.as_slice(), [SocketCall::Connect { host, .. }] if host == "192.168.1.5"),
        "{calls:?}"
    );
    drop(device);
    // The next time, that computer is connected at once; another one
    // answering first is not.
    let mut device = kernel(&storage, CONFIG);
    let found = broadcast(&mut device);
    assert!(answer(&mut device, found, "192.168.1.66").is_empty());
    let calls = answer(&mut device, found, "192.168.1.5");
    assert!(
        matches!(calls.as_slice(), [SocketCall::Connect { host, .. }] if host == "192.168.1.5"),
        "{calls:?}"
    );
}

#[test]
fn calibre_touches_only_its_own_books() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    storage.write("Ann/Own.epub", b"own").unwrap();
    storage.write("paperpaw.ini", b"[core]\n").unwrap();
    let (mut kernel, mut calibre) = connected(DEVICE, &storage);
    let read = |path: &str| paperpaw_sdk::read_file(&storage, path, 64).unwrap();
    // The reader's own book is neither deleted nor replaced.
    calibre.say(
        &mut kernel,
        op::DELETE_BOOK,
        r#"{"lpaths": ["Ann/Own.epub"]}"#,
    );
    send_book(&mut calibre, &mut kernel, "Ann/Own.epub", b"new");
    assert_eq!(read("Ann/Own.epub"), b"own");
    // Nothing but books is written.
    for lpath in ["paperpaw.ini", "plugins/evil/plugin.ini", "updates/x.bin"] {
        send_book(&mut calibre, &mut kernel, lpath, b"bad");
    }
    assert_eq!(read("paperpaw.ini"), b"[core]\n");
    assert!(!storage.exists("plugins/evil/plugin.ini") && !storage.exists("updates/x.bin"));
    // A book Calibre sent is replaced and deleted as it asks.
    send_book(&mut calibre, &mut kernel, "Bo/New.epub", b"one");
    send_book(&mut calibre, &mut kernel, "Bo/New.epub", b"two");
    assert_eq!(read("Bo/New.epub"), b"two");
    calibre.answers();
    calibre.say(
        &mut kernel,
        op::DELETE_BOOK,
        r#"{"lpaths": ["Bo/New.epub"]}"#,
    );
    assert!(!storage.exists("Bo/New.epub"));
    // In step all along.
    calibre.say(&mut kernel, op::NOOP, "{}");
    assert_eq!(calibre.answers().len(), 3, "DELETE_BOOK twice, NOOP");
}

/// A book Calibre deletes leaves the reader's notes and place in it: sent
/// again, it has them.
#[test]
fn books_calibre_deletes_keep_their_notes() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let (mut kernel, mut calibre) = connected(DEVICE, &storage);
    send_book(&mut calibre, &mut kernel, "Bo/New.epub", b"one");
    let notes = paperpaw_sdk::annotations::file("Bo/New.epub");
    let line = b"h\t1\t0\t1\t20\t5\t0\t0\tSome text\tMy note\n";
    storage.write(&notes, line).unwrap();
    calibre.say(
        &mut kernel,
        op::DELETE_BOOK,
        r#"{"lpaths": ["Bo/New.epub"]}"#,
    );
    kernel.tick(Instant(20));
    assert!(!storage.exists("Bo/New.epub"));
    assert_eq!(storage.0.get(&notes).as_deref(), Some(&line[..]));
    send_book(&mut calibre, &mut kernel, "Bo/New.epub", b"one");
    assert!(storage.exists("Bo/New.epub"));
    assert_eq!(storage.0.get(&notes).as_deref(), Some(&line[..]));
}

/// A book sent again that cannot be put in place leaves the one there
/// as it was, and no part behind.
#[test]
fn a_failed_replacement_keeps_the_old_book() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let (mut kernel, mut calibre) = connected(DEVICE, &storage);
    send_book(&mut calibre, &mut kernel, "Bo/New.epub", b"one");
    FAIL_RENAME.set(true);
    send_book(&mut calibre, &mut kernel, "Bo/New.epub", b"two");
    FAIL_RENAME.set(false);
    assert_eq!(storage.0.get("Bo/New.epub").as_deref(), Some(&b"one"[..]));
    assert!(!storage.exists("Bo/New.epub.part"));
}

/// A book without its length is refused: it would be taken as empty and
/// replace the one sent before.
#[test]
fn a_book_without_its_length_is_refused() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let (mut kernel, mut calibre) = connected(DEVICE, &storage);
    send_book(&mut calibre, &mut kernel, "Bo/New.epub", b"one");
    calibre.say(
        &mut kernel,
        op::SEND_BOOK,
        r#"{"lpath": "Bo/New.epub", "metadata": {"lpath": "Bo/New.epub", "title": "T"}}"#,
    );
    assert_eq!(storage.0.get("Bo/New.epub").as_deref(), Some(&b"one"[..]));
    assert!(calibre.closed);
}

#[test]
fn the_book_list_goes_in_parts() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    // 300 books Calibre sent, each with a long description.
    let description = "A long description. ".repeat(100);
    for i in 0..300 {
        let lpath = format!("Ann/Book {i}.epub");
        storage.write(&lpath, b"x").unwrap();
        let metadata =
            format!(r#"{{"lpath": "{lpath}", "uuid": "u{i}", "comments": "{description}"}}"#);
        let record = format!(
            ".paperpaw/plugins/calibre/books/{}.json",
            paperpaw_sdk::digest::md5::of(&lpath)
        );
        storage.write(&record, metadata.as_bytes()).unwrap();
    }
    let (mut kernel, mut calibre) = connected(DEVICE, &storage);
    calibre.say(&mut kernel, op::GET_BOOK_COUNT, r#"{"canStream": true}"#);
    // Not all at once (about 600 KB here), but a part at a time.
    assert!(calibre.got.len() < 40 * 1024, "{}", calibre.got.len());
    let mut answers = calibre.answers();
    assert_eq!(protocol::number(&answers[0].args, "count"), Some(300));
    for at in 10..1000 {
        kernel.tick(Instant(at));
        calibre.pump(&mut kernel);
        assert!(calibre.got.len() < 40 * 1024, "{}", calibre.got.len());
        answers.extend(calibre.answers());
    }
    assert_eq!(answers.len(), 301);
    let mut uuids: Vec<String> = answers[1..]
        .iter()
        .map(|a| protocol::text(&a.args, "uuid").unwrap())
        .collect();
    uuids.sort();
    uuids.dedup();
    assert_eq!(uuids.len(), 300);
    // Then the device reads on.
    calibre.say(&mut kernel, op::NOOP, "{}");
    assert_eq!(calibre.answers().len(), 1);
}

#[test]
fn a_message_larger_than_the_device_holds_ends_the_connection() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let (mut kernel, mut calibre) = connected(device(MemoryClass::Small), &storage);
    calibre.wire.extend_from_slice(b"1000000[");
    calibre.pump(&mut kernel);
    assert!(calibre.closed);
}

/// A character split between socket reads stays whole (H-8), so
/// Calibre's framing by bytes keeps in step.
#[test]
fn a_character_split_between_reads_keeps_messages_in_step() {
    let storage = Shared(Rc::new(MemoryStorage::new()));
    let (mut kernel, mut calibre) = connected(DEVICE, &storage);
    // Three bytes a read: `ö` and `ß` come in two reads each.
    calibre.chunk = Some(3);
    let book = b"abc";
    let message = format!(
        r#"{{"lpath": "Ann/Größe.epub", "length": {}, "metadata": {{"title": "Größe"}}}}"#,
        book.len()
    );
    calibre
        .wire
        .extend_from_slice(&frame(op::SEND_BOOK, &message));
    calibre.wire.extend_from_slice(book);
    calibre.say(&mut kernel, op::NOOP, "{}");
    assert!(!calibre.closed);
    assert_eq!(calibre.answers().len(), 1, "the NOOP after the book");
    assert_eq!(
        paperpaw_sdk::read_file(&storage, "Ann/Größe.epub", 64).unwrap(),
        book
    );
}

/// What the kernel asks the shell to do with sockets.
fn socket_calls(kernel: &mut Kernel) -> Vec<SocketCall> {
    let io = kernel.take_io().into_iter();
    io.filter_map(|io| match io {
        Io::Socket(call) => Some(call),
        _ => None,
    })
    .collect()
}
