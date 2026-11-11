//! The cable on interface 7 through the kernel's native host: the computer's lines
//! (`Kernel::cable_line`) reach the plugin's screen `transfer.cable` (slot
//! `core.cable`), which serves port 0 and answers with `respond`. These are the
//! behaviours the transfer has over the cable: a computer that
//! speaks opens the screen and learns what the device is; nothing is written
//! before the reader allowed what it offered, and only that; *Back* ends the
//! permission. They need the host's cable (Kernel #900), so they are ignored
//! until native plugins are activated; their assertions are kept.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

use paperpaw_core::Effects;
use paperpaw_platform::Instant;
use paperpaw_sdk::builtin::actions;
use std::collections::BTreeMap;

const SERVICES: &[&str] = &[
    "core.device",
    "core.document",
    "core.files",
    "core.library",
    "core.net",
    "core.registry",
    "core.secrets",
    "core.ui",
    "core.work",
];

fn sha(data: &[u8]) -> String {
    paperpaw_digest::sha256::hex(data)
}

/// A computer at the other end of the cable, and the device the plugin runs on.
struct Computer {
    device: hosted::Installed,
    now: u64,
    next: u32,
}

impl Computer {
    fn new() -> Self {
        let setup = hosted::Setup::x4()
            .config("[core]\nlanguage = en\n")
            .with_kernel(|builder| {
                builder
                    .network(true)
                    .wifi(true)
                    .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            });
        let mut device = hosted::install_in(
            &setup,
            paperpaw_plugin_transfer::NATIVE_ENTRY.entry(),
            "transfer",
            SERVICES,
        )
        .expect("the host admits the plugin");
        let effects = device.kernel().start(Instant(0));
        let mut computer = Self {
            device,
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
            effects = self.device.kernel().tick(Instant(self.now));
        }
    }

    fn line(&mut self, line: &str) {
        self.now += 10;
        let now = Instant(self.now);
        let effects = self.device.kernel().cable_line(now, line);
        self.settle(effects);
    }

    /// A request over the cable: the status and body of the answer.
    fn request(&mut self, method: &str, target: &str, body: &[u8]) -> (u16, String) {
        let n = self.next;
        self.next += 1;
        self.line(&format!("PPT/1 {n} {method} {target} {}", body.len()));
        for part in body.chunks(512) {
            let hex: String = part.iter().map(|b| format!("{b:02x}")).collect();
            self.line(&format!("PPT/1 {n} data {hex}"));
        }
        let lines = self.device.kernel().take_cable_lines();
        let answers = answers(&lines);
        answers
            .get(&n)
            .cloned()
            .unwrap_or_else(|| panic!("no answer to {method} {target}: {answers:?}"))
    }

    /// Whether the screen says `text` (lines joined: a long text wraps).
    fn says(&mut self, text: &str) -> bool {
        self.device.kernel().render(&mut |_, _| {});
        let labels: Vec<String> = self
            .device
            .kernel()
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        labels.join(" ").contains(text)
    }

    fn press(&mut self, action: paperpaw_sdk::Id) {
        self.now += 100;
        let now = Instant(self.now);
        let effects = self.device.kernel().run_action(now, action);
        self.settle(effects);
    }

    /// The reader answers the question shown: allow, or not.
    fn allow(&mut self, yes: bool) {
        if yes {
            self.press(actions::LEFT);
        }
        self.press(actions::CONFIRM);
    }

    fn shown(&mut self) -> Option<String> {
        self.device
            .kernel()
            .active_screen()
            .map(|s| String::from(s.as_str()))
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
#[ignore = "needs native activation (#900 cable host)"]
fn a_computer_that_speaks_opens_the_screen_and_learns_what_the_device_is() {
    let mut computer = Computer::new();
    let (status, body) = computer.request("GET", "/api/hello", b"");
    assert_eq!(status, 200);
    assert!(body.contains(r#""protocol":1"#), "{body}");
    assert!(body.contains(r#""device":"xteink-x4""#), "{body}");
    assert_eq!(computer.shown().as_deref(), Some("transfer.cable"));
    assert!(computer.says("connected over USB"));
    let (status, body) = computer.request("GET", "/api/plugins", b"");
    assert_eq!(status, 200);
    assert!(body.contains(r#""id":"transfer""#), "{body}");
    // Lines of others (a log, a remote control) are passed over.
    computer.line("PP:OK");
    assert!(computer.device.kernel().take_cable_lines().is_empty());
}

#[test]
#[ignore = "needs native activation (#900 cable host)"]
fn nothing_is_written_before_the_reader_allows_it() {
    let mut computer = Computer::new();
    computer.request("GET", "/api/hello", b"");
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
    let (status, _) = computer.request("POST", "/api/offer", huge.as_bytes());
    assert_eq!(status, 507);
}

#[test]
#[ignore = "needs native activation (#900 cable host)"]
fn what_was_allowed_comes_in_whole_and_nothing_else() {
    let mut computer = Computer::new();
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
    let put = |computer: &mut Computer, id: &str, path: &str, data: &[u8]| {
        let target = format!("/api/plugin/{id}/{path}?sha256={}", sha(data));
        computer.request("PUT", &target, data).0
    };
    assert_eq!(put(&mut computer, "airy", "i18n/de.ini", b"[meta]"), 403);
    assert_eq!(
        put(&mut computer, "other", "plugin.ini", INI.as_bytes()),
        403
    );
    let target = format!("/api/plugin/airy/plugin.ini?sha256={}", sha(b"x"));
    let wrong = format!("{}{}", &INI[..INI.len() - 1], "x");
    assert_eq!(computer.request("PUT", &target, wrong.as_bytes()).0, 422);
    assert_eq!(
        put(&mut computer, "airy", "plugin.ini", INI.as_bytes()),
        200
    );
    // The host asks the reader once more (decision 132) and the answer to the
    // computer waits for it: installed in one step.
    computer.line(&format!("PPT/1 {} POST /api/plugin/airy 0", computer.next));
    computer.allow(true);
    let lines = computer.device.kernel().take_cable_lines();
    let answered = answers(&lines);
    let (status, body) = answered.get(&computer.next).cloned().unwrap();
    computer.next += 1;
    assert_eq!(status, 200, "{body}");
    // The book under a free name; offered once, taken once.
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    let (status, body) = computer.request("PUT", &target, book);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(".epub"), "{body}");
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    assert_eq!(computer.request("POST", "/api/plugin/airy", b"").0, 403);
}

#[test]
#[ignore = "needs native activation (#900 cable host)"]
fn back_ends_the_permission_and_a_pause_opens_the_screen_again() {
    let mut computer = Computer::new();
    let book = b"a book";
    let offer = format!("book a.epub {} {}\n", book.len(), sha(book));
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    computer.allow(true);
    // Back: the permission ends, and a computer that goes on asking does not
    // bring the screen back; a new start after a pause does.
    computer.press(actions::BACK);
    assert_ne!(computer.shown().as_deref(), Some("transfer.cable"));
    let (status, body) = computer.request("GET", "/api/offer", b"");
    assert_eq!((status, body.as_str()), (403, "ended on the device"));
    assert_ne!(computer.shown().as_deref(), Some("transfer.cable"));
    computer.now += 11_000;
    assert_eq!(computer.request("GET", "/api/hello", b"").0, 200);
    assert_eq!(computer.shown().as_deref(), Some("transfer.cable"));
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    assert_eq!(computer.request("PUT", &target, book).0, 403);
}

/// The reader's answer counts for the offer it was asked about only: no new
/// offer while one is asked about, and a question closed without an answer
/// allows nothing.
#[test]
#[ignore = "needs native activation (#900 cable host)"]
fn an_answer_counts_for_its_offer_only() {
    let mut computer = Computer::new();
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
    // Back closes the question without an answer: nothing is allowed.
    computer.press(actions::BACK);
    assert_eq!(
        computer.request("GET", "/api/offer", b"").1,
        r#"{"state":"refused"}"#
    );
}
