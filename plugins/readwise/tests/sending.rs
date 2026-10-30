//! Sending highlights to Readwise with the real kernel and canned
//! answers: the token is asked for once and kept veiled with the other
//! secrets, sent as a header, removed when Readwise refuses it; each
//! answer is told; and the JSON sent holds every highlight exactly,
//! whatever its text (quotes, line breaks, control characters, any
//! Unicode), as a property test checks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_readwise::{Highlight, Parser as Notes, Readwise, SECRET};
use paperpaw_sdk::builtin::events::NotesExported;
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::json::stream::{Event as Json, Parser};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, Priority, RegisterError, Registrar,
    Screen, ScreenSpec, Version,
};
use proptest::prelude::*;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(200, 300, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

const NOTES: &str = "Notes/Alice.md";
const MARKDOWN: &str = "# Alice\n\nLewis Carroll\n\n## Highlights\n\n\
    > Curiouser and curiouser!\n>\n> 12 %\n\n> Who are you?\n>\n> 40 %\n\nA note.\n";

/// Takes the place of the notice, to hear what the plugin tells.
struct Notices(Rc<RefCell<Vec<String>>>);

struct Silent;

impl Screen for Silent {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Notices {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("test-notices"),
            "Notices",
            Version::new(0, 1, 0),
        )
        .description("Keeps the notices for the test.")
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let screen = Id::new_static("test-notices.notice");
        let heard = self.0.clone();
        r.add_screen(ScreenSpec::new(
            screen.clone(),
            "notice",
            Box::new(move |_, p| {
                heard
                    .borrow_mut()
                    .push(p.get(params::TEXT).unwrap_or_default().to_string());
                Box::new(Silent)
            }),
        ))?;
        r.bind_slot(slots::NOTICE, screen, Priority::PREFERRED)
    }
}

struct Harness {
    kernel: Kernel,
    notices: Rc<RefCell<Vec<String>>>,
    now: u64,
}

impl Harness {
    fn new(storage: MemoryStorage, network: bool) -> Self {
        let notices = Rc::new(RefCell::new(Vec::new()));
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[core]\nlanguage = en\n")
            .network(network)
            .plugin(Readwise)
            .plugin(Notices(notices.clone()))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            notices,
            now: 0,
        }
    }

    /// Notes on the card and the token kept.
    fn with_token(token: &str) -> Self {
        let mut secrets = Secrets::default();
        secrets.apply(SecretChange::keep(SECRET, "", token));
        let storage = MemoryStorage::new()
            .with(NOTES, MARKDOWN)
            .with(FILE, secrets.encode().into_bytes());
        Self::new(storage, true)
    }

    fn send(&mut self, event: &'static str, data: impl EventData) {
        self.now += 100;
        self.kernel
            .send_event(Instant(self.now), Id::new_static(event), data.to_params());
    }

    fn export(&mut self, path: &str) -> Vec<HttpCall> {
        self.send(
            "core.notes-exported",
            NotesExported::new(String::from(path), String::from("Alice")),
        );
        self.requests()
    }

    fn requests(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn answer(&mut self, call: &HttpCall, status: u16) {
        self.now += 100;
        self.kernel.http_response(
            Instant(self.now),
            HttpResponse {
                id: call.id,
                status,
                ..HttpResponse::default()
            },
        );
    }

    fn told(&self) -> String {
        self.notices.borrow().last().cloned().unwrap_or_default()
    }
}

#[test]
fn the_token_is_asked_for_once_kept_veiled_and_sent() {
    let storage = MemoryStorage::new().with(NOTES, MARKDOWN);
    let mut h = Harness::new(storage, true);
    assert!(h.export(NOTES).is_empty());
    assert!(h.told().contains("enter your access token"), "{}", h.told());
    // The keyboard answers, with the notes' path passed along.
    h.send(
        "readwise.token-entered",
        Typed::new(
            String::from("  t0k-en  "),
            Params::new().with(params::PATH, NOTES),
        ),
    );
    let calls = h.requests();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].method, "POST");
    assert_eq!(calls[0].url, "https://readwise.io/api/v2/highlights/");
    assert!(
        calls[0]
            .headers
            .contains(&(String::from("Authorization"), String::from("Token t0k-en")))
    );
    assert_eq!(h.kernel.secrets().get(SECRET), Some("t0k-en"));
    let file = paperpaw_sdk::read_file(h.kernel.storage(), FILE, 1 << 16).unwrap();
    let file = String::from_utf8(file).unwrap();
    assert!(!file.contains("t0k-en"), "{file}");
    h.answer(&calls[0], 201);
    assert_eq!(h.told(), "2 highlights sent to Readwise.");
    // The next export needs no asking.
    let calls = h.export(NOTES);
    assert_eq!(calls.len(), 1);
}

#[test]
fn an_empty_token_is_not_kept() {
    let storage = MemoryStorage::new().with(NOTES, MARKDOWN);
    let mut h = Harness::new(storage, true);
    h.send(
        "readwise.token-entered",
        Typed::new(String::from("   "), Params::new().with(params::PATH, NOTES)),
    );
    assert!(h.requests().is_empty());
    assert_eq!(h.kernel.secrets().get(SECRET), None);
}

/// A refused token is kept (Readwise may refuse it for a while, or the
/// reader may have typed the right one wrong): nothing is sent by itself
/// until the next export, which asks for it anew.
#[test]
fn a_refused_token_is_kept_and_asked_for_again() {
    for status in [401, 403] {
        let mut h = Harness::with_token("old");
        let calls = h.export(NOTES);
        h.answer(&calls[0], status);
        assert!(
            h.told().contains("did not accept the token"),
            "{}",
            h.told()
        );
        assert_eq!(h.kernel.secrets().get(SECRET), Some("old"));
        h.send("readwise.retry", ());
        assert!(h.requests().is_empty(), "no retries");
        assert!(h.export(NOTES).is_empty());
        assert!(h.told().contains("enter your access token"));
        assert_eq!(h.kernel.secrets().get(SECRET), Some("old"));
        h.send(
            "readwise.token-entered",
            Typed::new(String::from("new"), Params::new()),
        );
        let calls = h.requests();
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert!(
            calls[0]
                .headers
                .contains(&(String::from("Authorization"), String::from("Token new")))
        );
    }
}

/// A new token kept after one was refused (entered on the passwords and
/// keys page, say) is sent by itself again; so is the refused one once
/// entered anew, also when nothing waits then.
#[test]
fn a_new_token_after_a_refused_one_is_sent_by_itself() {
    let mut h = Harness::with_token("old");
    let calls = h.export(NOTES);
    h.answer(&calls[0], 401);
    h.kernel
        .set_secret(SecretChange::keep(SECRET, "readwise.token", "new"));
    h.send("readwise.retry", ());
    let calls = h.requests();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(
        calls[0]
            .headers
            .contains(&(String::from("Authorization"), String::from("Token new")))
    );
    h.answer(&calls[0], 401);
    // Nothing waits when the same token is entered anew.
    h.kernel
        .storage()
        .remove(".paperpaw/plugins/readwise/waiting.txt")
        .unwrap();
    h.send(
        "readwise.token-entered",
        Typed::new(String::from("new"), Params::new()),
    );
    assert!(h.requests().is_empty());
    let calls = h.export(NOTES);
    assert_eq!(calls.len(), 1, "{calls:?}");
}

#[test]
fn every_answer_is_told() {
    for (status, said) in [
        (200, "2 highlights sent"),
        (0, "cannot be reached"),
        (500, "Not all highlights could be sent"),
        (429, "Not all highlights could be sent"),
    ] {
        let mut h = Harness::with_token("t");
        let calls = h.export(NOTES);
        h.answer(&calls[0], status);
        assert!(h.told().contains(said), "{status}: {}", h.told());
        assert_eq!(h.kernel.secrets().get(SECRET), Some("t"), "{status}");
    }
}

/// Notes that are gone or hold no highlight send nothing and wait no
/// more; notes of any length are read a line at a time and sent from a
/// file, a highlight longer than Readwise takes cut.
#[test]
fn notes_missing_or_empty_send_nothing_and_long_ones_are_sent() {
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep(SECRET, "", "t"));
    let storage = MemoryStorage::new()
        .with(FILE, secrets.encode().into_bytes())
        .with("Notes/empty.md", "# Empty\n\n## Bookmarks\n\n- 5 %: x\n")
        .with("Notes/binary.md", vec![0xff_u8, 0xfe, 0, 1, 2])
        .with(
            "Notes/huge.md",
            format!("# Long\n\n## Highlights\n\n{}", "> x\n".repeat(100_000)),
        );
    let mut h = Harness::new(storage, true);
    for path in ["Notes/missing.md", "Notes/empty.md", "Notes/binary.md"] {
        assert!(h.export(path).is_empty(), "{path}");
        assert!(h.told().contains("no highlights"), "{path}: {}", h.told());
    }
    let calls = h.export("Notes/huge.md");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].body.is_none());
    let file = calls[0].body_from.as_deref().expect("sent from a file");
    let size = h.kernel.storage().size(file).unwrap();
    // 8191 bytes of text at most, each line break escaped in two.
    assert!(size < 2 * 8191 + 1000, "{size}");
    h.answer(&calls[0], 200);
    assert_eq!(h.told(), "1 highlight sent to Readwise.");
}

#[test]
fn nothing_is_sent_without_network() {
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep(SECRET, "", "t"));
    let storage = MemoryStorage::new()
        .with(NOTES, MARKDOWN)
        .with(FILE, secrets.encode().into_bytes());
    let mut h = Harness::new(storage, false);
    assert!(h.export(NOTES).is_empty());
    assert!(h.told().contains("needs a network"), "{}", h.told());
}

/// The title and highlights the reader's Markdown `text` holds, read a
/// line at a time as the plugin reads a notes file.
fn parse(text: &str) -> (String, Vec<Highlight>) {
    let mut notes = Notes::default();
    let mut highlights: Vec<Highlight> = text.lines().filter_map(|l| notes.line(l)).collect();
    highlights.extend(notes.finish());
    (notes.title().to_string(), highlights)
}

#[test]
fn odd_markdown_is_read_as_well_as_it_can() {
    let (title, highlights) = parse(
        "no title line\n## Highlights\n> 99999999999999999999 %\n>   \n> -5 %\n> 50%\n\
         \n> second\n> lines\n> 7 %\n## Highlights again\n> not read\n",
    );
    assert_eq!(title, "");
    assert_eq!(highlights.len(), 2, "{highlights:?}");
    // Places that are no numbers are text.
    assert_eq!(highlights[0].text, "99999999999999999999 %\n-5 %");
    assert_eq!(highlights[0].percent, 50);
    assert_eq!(highlights[1].text, "second\nlines");
    assert_eq!(highlights[1].percent, 7);
    assert_eq!(parse(""), (String::new(), Vec::new()));
}

/// The texts, notes and places of the highlights in a body sent.
fn sent(body: &str) -> Vec<(String, String, String)> {
    let mut found: Vec<(String, String, String)> = Vec::new();
    let mut parser = Parser::default();
    parser.feed(body.as_bytes(), &mut |event| match event {
        Json::Open(path) if path == ["highlights", "[]"] => {
            found.push(Default::default());
        }
        Json::Value(path, value) if path.len() == 3 && path[..2] == ["highlights", "[]"] => {
            if let Some(last) = found.last_mut() {
                match path[2].as_str() {
                    "text" => last.0 = value.to_string(),
                    "note" => last.1 = value.to_string(),
                    "location" => last.2 = value.to_string(),
                    _ => {}
                }
            }
        }
        _ => {}
    });
    found
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        ".{1,40}",
        "[\"\\\\\n\r\t\u{0}-\u{1f}\u{7f}\u{2028}\u{2029}a]{1,10}",
        "[\u{10000}-\u{10ffff}]{1,4}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(200)))]

    #[test]
    fn the_json_sent_holds_every_highlight_exactly(
        title in text(),
        author in prop_oneof![Just(String::new()), text()],
        highlights in prop::collection::vec((text(), prop_oneof![Just(String::new()), text()], 0u32..=100), 1..5),
    ) {
        let mut notes = Notes::default();
        notes.line(&format!("# {}", title.replace(['\n', '\r'], " ")));
        notes.line(&author.replace(['\n', '\r'], " "));
        let objects: Vec<String> = highlights
            .iter()
            .map(|(text, note, percent)| {
                notes.json(&Highlight {
                    text: text.clone(),
                    note: note.clone(),
                    percent: *percent,
                })
            })
            .collect();
        let body = format!("{{\"highlights\":[{}]}}", objects.join(","));
        let expected: Vec<(String, String, String)> = highlights
            .iter()
            .map(|(t, n, p)| (t.clone(), n.clone(), p.to_string()))
            .collect();
        prop_assert_eq!(sent(&body), expected);
    }

    #[test]
    fn any_markdown_parses_without_panic(text in ".{0,400}") {
        let (_, highlights) = parse(&text);
        prop_assert!(highlights.iter().all(|h| !h.text.is_empty()));
    }
}
