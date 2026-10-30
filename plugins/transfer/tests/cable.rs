//! Transfer over the cable (USB) with the real kernel: lines as the
//! computer sends them (`Kernel::cable_line`) and the answers as the
//! device sends them back. A computer that speaks opens the screen; it
//! learns what the device is without asking the reader, but nothing is
//! written before the reader allowed what it offered, and only that, at
//! its size and digest; the plugin is installed in one step, books never
//! go over others; Back ends the permission; the radio stays off.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeMap;
use std::rc::Rc;

use paperpaw_core::{CodeFile, CodeLoader, Effects, Io, Kernel};
use paperpaw_native::{Chip, Header, LINK_BASE};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_transfer::Transfer;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::native::{Fingerprint, Native, installed_code};
use paperpaw_sdk::{
    DirEntry, MemoryStorage, Plugin, PluginProblem, Storage, StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Small,
};

const BUILD: Fingerprint = Fingerprint(0x0123_4567_89ab_cdef);

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
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
}

struct NoCode;

impl CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Damaged)
    }
}

fn sha(data: &[u8]) -> String {
    paperpaw_digest::sha256::hex(data)
}

fn ppn() -> Vec<u8> {
    let image = [0u8; 128];
    let mut header = Header {
        chip: Chip::Esp32C3,
        fingerprint: BUILD.0,
        base: LINK_BASE,
        image_len: image.len() as u32,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: paperpaw_sdk::native::INTERFACE,
        sha256: [0; 32],
    };
    let mut digest = header.digest();
    digest.update(&image);
    header.sha256 = digest.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    file
}

struct Computer {
    kernel: Kernel,
    storage: Shared,
    now: u64,
    next: u32,
}

impl Computer {
    fn new(storage: Shared) -> Self {
        let mut native = Native::new("code.ppn");
        native.fingerprint = BUILD;
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .user_config("[core]\nlanguage = en\n")
            .network(true)
            .wifi(true)
            .code_plugins(native, Box::new(NoCode))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Transfer)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut computer = Self {
            kernel,
            storage,
            now: 0,
            next: 1,
        };
        computer.settle(effects);
        computer
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

    fn line(&mut self, line: &str) {
        self.now += 10;
        let effects = self.kernel.cable_line(Instant(self.now), line);
        self.settle(effects);
    }

    /// A request over the cable; the status and body of the answer.
    fn request(&mut self, method: &str, target: &str, body: &[u8]) -> (u16, String) {
        let n = self.next;
        self.next += 1;
        self.request_numbered(n, method, target, body)
    }

    /// The same, numbered `n` by the computer.
    fn request_numbered(
        &mut self,
        n: u32,
        method: &str,
        target: &str,
        body: &[u8],
    ) -> (u16, String) {
        self.line(&format!("PPT/1 {n} {method} {target} {}", body.len()));
        for part in body.chunks(512) {
            let hex: String = part.iter().map(|b| format!("{b:02x}")).collect();
            self.line(&format!("PPT/1 {n} data {hex}"));
        }
        let answers = answers(&self.kernel.take_cable_lines());
        answers
            .get(&n)
            .cloned()
            .unwrap_or_else(|| panic!("no answer to {method} {target}: {answers:?}"))
    }

    fn put_plugin(&mut self, id: &str, path: &str, data: &[u8]) -> u16 {
        let target = format!("/api/plugin/{id}/{path}?sha256={}", sha(data));
        self.request("PUT", &target, data).0
    }

    /// Whether the screen says `text` (lines joined: a long text wraps).
    fn says(&mut self, text: &str) -> bool {
        self.kernel.render(&mut |_, _| {});
        let labels: Vec<String> = self
            .kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        labels.join(" ").contains(text)
    }

    fn press(&mut self, action: paperpaw_sdk::Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// The reader answers the question shown: allow, or not.
    fn allow(&mut self, yes: bool) {
        if yes {
            self.press(actions::LEFT);
        }
        self.press(actions::CONFIRM);
    }

    fn files_below(&self, dir: &str) -> usize {
        let mut count = 0;
        let _ = self.storage.list(dir, &mut |_| count += 1);
        count
    }
}

/// The answers among `lines`, by the computer's numbers: status and body.
fn answers(lines: &[String]) -> BTreeMap<u32, (u16, String)> {
    let mut answers: BTreeMap<u32, (u16, Vec<u8>)> = BTreeMap::new();
    for line in lines {
        let rest = line.strip_prefix("PPT/1 ").expect(line);
        let mut words = rest.split(' ');
        let n: u32 = words.next().unwrap().parse().unwrap();
        match words.next().unwrap() {
            "data" => {
                let hex = words.next().unwrap();
                let bytes = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap());
                answers.get_mut(&n).unwrap().1.extend(bytes);
            }
            "end" => {}
            status => {
                answers.insert(n, (status.parse().unwrap(), Vec::new()));
            }
        }
    }
    answers
        .into_iter()
        .map(|(n, (status, body))| (n, (status, String::from_utf8(body).unwrap())))
        .collect()
}

const INI: &str = "[plugin]\nid = airy\nname = Airy\nversion = 1.0.0\n";

#[test]
fn a_computer_that_speaks_opens_the_screen_and_learns_what_the_device_is() {
    let mut computer = Computer::new(Shared::default());
    let (status, body) = computer.request("GET", "/api/hello", b"");
    assert_eq!(status, 200);
    assert!(body.contains(r#""protocol":1"#), "{body}");
    assert!(body.contains(r#""device":"test-x4""#), "{body}");
    assert!(
        body.contains(r#""fingerprint":"0123456789abcdef""#),
        "{body}"
    );
    assert_eq!(
        computer.kernel.active_screen().map(|s| s.as_str()),
        Some("transfer.cable")
    );
    assert!(computer.says("connected over USB"));
    let (status, body) = computer.request("GET", "/api/plugins", b"");
    assert_eq!(status, 200);
    assert!(body.contains(r#""id":"transfer""#), "{body}");
    // The radio is not asked for: the cable needs no network.
    let radio = computer
        .kernel
        .take_io()
        .into_iter()
        .any(|io| matches!(io, Io::Wifi(_)));
    assert!(!radio);
    // Lines of others (a log, a remote control) are passed over.
    computer.line("PP:OK");
    assert!(computer.kernel.take_cable_lines().is_empty());
}

#[test]
fn nothing_is_written_before_the_reader_allows_it() {
    let mut computer = Computer::new(Shared::default());
    let book = b"a book";
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    let offer = format!("book a.epub {} {}\n", book.len(), sha(book));
    let (status, body) = computer.request("POST", "/api/offer", offer.as_bytes());
    assert_eq!((status, body.as_str()), (202, r#"{"state":"asking"}"#));
    assert!(computer.says("Allow this computer to add books?"));
    assert!(computer.says("a.epub ("));
    // Asking: still nothing.
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    computer.allow(false);
    assert_eq!(
        computer.request("GET", "/api/offer", b"").1,
        r#"{"state":"refused"}"#
    );
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    assert_eq!(computer.files_below("books"), 0);
    // Offers that name what may not be written are no offers.
    let z = "0".repeat(64);
    for bad in [
        format!("book ../a.epub 3 {z}\n"),
        format!("book .hidden 3 {z}\n"),
        format!("plugin ../x\nplugin.ini 3 {z}\n"),
        format!("plugin airy\n../paperpaw.ini 3 {z}\n"),
        String::from("plugin airy\nplugin.ini 3\n"),
        format!("settings 3 {z}\n"),
    ] {
        let (status, _) = computer.request("POST", "/api/offer", bad.as_bytes());
        assert_eq!(status, 400, "{bad}");
    }
    // Sizes past what can be counted are no room.
    let huge = format!("book a 18446744073709551615 {z}\nbook b 2 {z}\n");
    assert_eq!(
        computer.request("POST", "/api/offer", huge.as_bytes()).0,
        507
    );
}

#[test]
fn what_was_allowed_comes_in_whole_and_nothing_else() {
    let storage = Shared::default();
    storage.write("books/a.epub", b"the reader's own").unwrap();
    let mut computer = Computer::new(storage);
    computer.request("GET", "/api/hello", b"");
    let book = b"a new book";
    let offer = format!(
        "plugin airy\nplugin.ini {} {}\nbook a.epub {} {}\n",
        INI.len(),
        sha(INI.as_bytes()),
        book.len(),
        sha(book)
    );
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    assert!(computer.says("install a plugin and add books"));
    computer.allow(true);
    assert!(computer.says("Allowed"));
    // Only what was offered, at its size and digest.
    assert_eq!(computer.put_plugin("airy", "i18n/de.ini", b"[meta]"), 403);
    assert_eq!(
        computer.put_plugin("other", "plugin.ini", INI.as_bytes()),
        403
    );
    let target = format!("/api/plugin/airy/plugin.ini?sha256={}", sha(b"x"));
    let wrong = format!("{}{}", &INI[..INI.len() - 1], "x");
    assert_eq!(computer.request("PUT", &target, wrong.as_bytes()).0, 422);
    assert_eq!(
        computer.put_plugin("airy", "plugin.ini", INI.as_bytes()),
        200
    );
    let (status, body) = computer.request("POST", "/api/plugin/airy", b"");
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        computer
            .kernel
            .plugins()
            .get("airy")
            .map(|p| p.manifest.version),
        Some(Version::new(1, 0, 0))
    );
    // The book under a free name: the reader's own stays.
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    let (status, body) = computer.request("PUT", &target, book);
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        computer.storage.0.get("books/a.epub").unwrap(),
        b"the reader's own"
    );
    assert!(body.contains("books/a"), "{body}");
    // Offered once, taken once.
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    assert_eq!(computer.request("POST", "/api/plugin/airy", b"").0, 403);
    assert!(!body.contains(r#""books/a.epub""#), "{body}");
    assert!(body.contains(".epub"), "{body}");
    let mut left = 0;
    let _ = computer.storage.list(".paperpaw/tmp", &mut |_| left += 1);
    assert_eq!(left, 0, "a body left behind");
}

#[test]
fn code_runs_as_the_reader_allowed_and_back_ends_the_permission() {
    let mut computer = Computer::new(Shared::default());
    let code = ppn();
    let ini = "[plugin]\nid = clock\nname = Clock\nversion = 1.0.0\n";
    let offer = format!(
        "plugin clock\nplugin.ini {} {}\ncode.ppn {} {}\n",
        ini.len(),
        sha(ini.as_bytes()),
        code.len(),
        sha(&code)
    );
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    assert!(computer.says("program code from this computer"));
    computer.allow(true);
    assert_eq!(
        computer.put_plugin("clock", "plugin.ini", ini.as_bytes()),
        200
    );
    assert_eq!(computer.put_plugin("clock", "code.ppn", &code), 200);
    let (status, body) = computer.request("POST", "/api/plugin/clock", b"");
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(r#""restart":true"#), "{body}");
    assert!(installed_code(&computer.storage, "clock").is_some());
    // Back: the permission ends, and a computer that goes on asking does
    // not bring the screen back; a new start after a pause does.
    computer.press(actions::BACK);
    let shown = |c: &Computer| c.kernel.active_screen().map(|s| String::from(s.as_str()));
    assert_ne!(shown(&computer).as_deref(), Some("transfer.cable"));
    let (status, body) = computer.request("GET", "/api/offer", b"");
    assert_eq!((status, body.as_str()), (403, "ended on the device"));
    assert_ne!(shown(&computer).as_deref(), Some("transfer.cable"));
    computer.now += 11_000;
    assert_eq!(computer.request("GET", "/api/hello", b"").0, 200);
    assert_eq!(shown(&computer).as_deref(), Some("transfer.cable"));
    assert_eq!(
        computer.put_plugin("clock", "plugin.ini", ini.as_bytes()),
        403
    );
}

/// The reader's answer counts for the offer it was asked about only: no
/// new offer while one is asked about, an answer naming another changes
/// nothing, and a question closed without an answer allows nothing.
#[test]
fn an_answer_counts_for_its_offer_only() {
    let mut computer = Computer::new(Shared::default());
    let book = b"a book";
    let first = format!("book a.epub {} {}\n", book.len(), sha(book));
    let second = format!("book b.epub {} {}\n", book.len(), sha(book));
    assert_eq!(
        computer.request("POST", "/api/offer", first.as_bytes()).0,
        202
    );
    assert_eq!(
        computer.request("POST", "/api/offer", second.as_bytes()).0,
        409
    );
    // An answer about another offer.
    let other = paperpaw_sdk::replies::Confirmed::new(
        true,
        paperpaw_sdk::Params::new().with("offer", "99"),
    );
    computer.now += 100;
    computer.kernel.send_event(
        Instant(computer.now),
        paperpaw_sdk::Id::new_static("transfer.cable-allowed"),
        paperpaw_sdk::event::EventData::to_params(&other),
    );
    assert_eq!(
        computer.request("GET", "/api/offer", b"").1,
        r#"{"state":"asking"}"#
    );
    // Closed without an answer: not allowed; a new offer is asked about.
    computer.press(actions::BACK);
    assert_eq!(
        computer.request("GET", "/api/offer", b"").1,
        r#"{"state":"refused"}"#
    );
    assert_eq!(
        computer.request("POST", "/api/offer", second.as_bytes()).0,
        202
    );
    computer.allow(true);
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    assert_eq!(computer.request("PUT", &target, book).0, 403);
}

/// What was sent for a plugin before an offer does not count: its files
/// are taken anew, only those the reader was shown.
#[test]
fn files_sent_before_an_offer_are_let_go() {
    let storage = Shared::default();
    storage
        .write("plugins/.airy.cable/code.ppn", b"not offered")
        .unwrap();
    let mut computer = Computer::new(storage);
    let offer = format!(
        "plugin airy\nplugin.ini {} {}\n",
        INI.len(),
        sha(INI.as_bytes())
    );
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    assert!(!computer.storage.exists("plugins/.airy.cable/code.ppn"));
}

/// What is installed is what was offered, checked when it is installed:
/// a file changed in the plugin's folder after it came (another way of
/// sending, say), or one put beside it, is not installed.
#[test]
fn only_files_as_offered_are_installed() {
    for (name, change) in [
        ("plugin.ini", INI.replace("Airy", "Other")),
        (
            "extra.ini",
            String::from(
                "[x]
",
            ),
        ),
    ] {
        let mut computer = Computer::new(Shared::default());
        let offer = format!(
            "plugin airy
plugin.ini {} {}
",
            INI.len(),
            sha(INI.as_bytes())
        );
        assert_eq!(
            computer.request("POST", "/api/offer", offer.as_bytes()).0,
            202
        );
        computer.allow(true);
        assert_eq!(
            computer.put_plugin("airy", "plugin.ini", INI.as_bytes()),
            200
        );
        computer
            .storage
            .write(&format!("plugins/.airy.cable/{name}"), change.as_bytes())
            .unwrap();
        let (status, body) = computer.request("POST", "/api/plugin/airy", b"");
        assert_eq!(
            (status, body.as_str()),
            (422, r#"{"why":"not as offered"}"#),
            "{name}"
        );
        assert!(computer.kernel.plugins().get("airy").is_none(), "{name}");
    }
}

/// The cable and the Wi-Fi transfer never send the same plugin at once:
/// an offer of a plugin the open Wi-Fi transfer is being sent waits.
#[test]
fn a_plugin_sent_over_wifi_is_not_offered_over_the_cable_meanwhile() {
    let storage = Shared::default();
    storage
        .write("plugins/.airy.local/plugin.ini", INI.as_bytes())
        .unwrap();
    let mut computer = Computer::new(storage);
    computer.now += 100;
    let effects = computer.kernel.open_screen(
        Instant(computer.now),
        paperpaw_sdk::Id::new_static("transfer.screen"),
        paperpaw_sdk::Params::new(),
    );
    computer.settle(effects);
    let offer = format!(
        "plugin airy\nplugin.ini {} {}\n",
        INI.len(),
        sha(INI.as_bytes())
    );
    let (status, body) = computer.request("POST", "/api/offer", offer.as_bytes());
    assert_eq!(
        (status, body.as_str()),
        (409, r#"{"why":"being sent over Wi-Fi"}"#)
    );
    // Its files there are left as they are.
    assert!(computer.storage.exists("plugins/.airy.local/plugin.ini"));
}

/// A request given the number of one whose body was still coming takes
/// its place: the new one is answered, and the cut one's body goes.
#[test]
fn a_number_given_again_ends_the_request_before_only() {
    let mut computer = Computer::new(Shared::default());
    computer.line("PPT/1 50 PUT /api/book/a.epub 10");
    computer.line("PPT/1 50 data 6162636465");
    let (status, body) = computer.request_numbered(50, "GET", "/api/hello", b"");
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"protocol\":1"), "{body}");
    assert_eq!(computer.files_below(".paperpaw/tmp"), 0);
}

/// Long offers are read a line at a time: a plugin and many books are
/// asked about at once, but not more books than an offer may hold (the
/// others go in the offers after), nor more files than a plugin may have.
#[test]
fn long_offers_are_read_in_parts() {
    let mut computer = Computer::new(Shared::default());
    let z = "0".repeat(64);
    let files: String = (0..64).map(|n| format!("f{n}.ini 10 {z}\n")).collect();
    let books: String = (0..64)
        .map(|n| format!("book A book with a long name, number {n}.epub 10 {z}\n"))
        .collect();
    let offer = format!("plugin airy\n{files}{books}");
    assert!(offer.len() > 8 * 1024);
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    // Offered here: the Wi-Fi transfer leaves it alone meanwhile.
    let offered = ".paperpaw/plugins/transfer/cable-offer.txt";
    assert!(computer.storage.exists(offered));
    computer.allow(false);
    assert!(!computer.storage.exists(offered));

    let too_many: String = (0..65)
        .map(|n| format!("book b{n}.epub 10 {z}\n"))
        .collect();
    assert_eq!(
        computer
            .request("POST", "/api/offer", too_many.as_bytes())
            .0,
        400
    );
    let files: String = (0..65).map(|n| format!("f{n}.ini 10 {z}\n")).collect();
    let plugin = format!("plugin airy\n{files}");
    assert_eq!(
        computer.request("POST", "/api/offer", plugin.as_bytes()).0,
        400
    );
}

/// Only closing the question refuses: a screen shown over it (another
/// dialog, a menu) does not.
#[test]
fn a_screen_over_the_question_does_not_refuse() {
    let mut computer = Computer::new(Shared::default());
    let book = b"a book";
    let offer = format!("book a.epub {} {}\n", book.len(), sha(book));
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    computer.now += 100;
    const CHOSEN: paperpaw_sdk::event::Event<paperpaw_sdk::ui::choice::Picked> =
        paperpaw_sdk::event::Event::new("test.chosen");
    let ask = paperpaw_sdk::ask::choice("Which?", &CHOSEN)
        .options("test.", &["a", "b"])
        .command();
    let paperpaw_sdk::Command::OpenSlot(slot, params) = ask else {
        panic!("{ask:?}");
    };
    let effects = computer
        .kernel
        .open_slot(Instant(computer.now), slot, params);
    computer.settle(effects);
    let shown: Vec<String> = computer.kernel.screens().map(|s| s.to_string()).collect();
    assert_eq!(shown.last().map(String::as_str), Some("core.choice-dialog"));
    let state = |c: &mut Computer| c.request("GET", "/api/offer", b"").1;
    assert_eq!(state(&mut computer), r#"{"state":"asking"}"#);
    // The one over it closes: still asking.
    computer.press(actions::BACK);
    assert_eq!(state(&mut computer), r#"{"state":"asking"}"#);
    // The question closes without an answer: refused.
    computer.press(actions::BACK);
    assert_eq!(state(&mut computer), r#"{"state":"refused"}"#);
}
