//! The message written on the device, native against what interface 6 gave
//! (kept in `testdata/pins/`): the same headers and text for the same draft,
//! the same recipients and problems, the same reply, the same kept draft; and
//! what a reader makes of it again.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

mod pin;
use paperpaw_plugin_mail::native_model::compose as new;
use paperpaw_plugin_mail::native_model::compose::{Problem, Refused};
use paperpaw_plugin_mail::native_model::{render as new_render, store as new_store};
use paperpaw_sdk::MemoryStorage;

/// A small deterministic generator, so that failures repeat.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n as u64) as usize
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const NAMES: [&str; 6] = [
    "",
    "Ada Lovelace",
    "Jürgen Müller",
    "O'Neil",
    "Doe, Jane",
    "Eve\r\nBcc: x",
];
const LISTS: [&str; 8] = [
    "",
    "bob@example.org",
    "\"Doe, Jane\" <jane@example.org>; bob@example.org",
    "BOB@example.org, bob@example.org",
    "not an address",
    "a@b.org,c@d.org;e@f.org",
    "Jürgen <j@example.org>",
    "<x@y.org>, ,",
];
const SUBJECTS: [&str; 5] = ["", "Hi", "Grüße", "Hi\r\nBcc: spy@example.org", "Re: Lunch"];
const BODIES: [&str; 7] = [
    "",
    "Short.\n",
    "Hallo Jürgen,\n.punkt am Anfang\n",
    "a\r\nb\r\n",
    "no newline",
    "line\n\n\nend\n",
    "tab\there\n",
];

/// A draft of random parts.
#[allow(
    clippy::field_reassign_with_default,
    reason = "the drafts are built field by field"
)]
fn draft(rng: &mut Rng) -> new::Draft {
    let from_name = rng.pick(&NAMES);
    let from = rng.pick(&["ada@example.org", "", "bad", "x@y.org"]);
    let mut n = new::Draft::with_sender(from_name, from).unwrap();
    let (to, cc) = (rng.pick(&LISTS), rng.pick(&LISTS));
    n.to = to.into();
    n.cc = cc.into();
    let (subject, body) = (rng.pick(&SUBJECTS), rng.pick(&BODIES));
    n.subject = subject.into();
    n.body = body.into();
    let (reply, refs) = (rng.pick(&["", "<1@x>"]), rng.pick(&["", "<0@x>"]));
    n.in_reply_to = reply.into();
    n.references = refs.into();
    n
}

fn refusal(r: Result<Vec<String>, Refused>) -> Result<Vec<String>, Problem> {
    r.map_err(|e| match e {
        Refused::Problem(p) => p,
        other => panic!("{other:?}"),
    })
}

#[test]
fn a_kept_draft_is_what_interface_6_kept_and_is_read_again() {
    let mut pin = pin::Pin::new("a_kept_draft_is_what_interface_6_kept_and_is_read_again");
    let mut rng = Rng(3);
    for _ in 0..300 {
        let n = draft(&mut rng);
        let kept = n.to_text().unwrap();
        assert_eq!(
            new::Draft::from_text(&kept).unwrap().to_text().unwrap(),
            kept
        );
        pin.fold("", &(kept));
    }
    assert_eq!(new::Draft::from_text("").unwrap(), new::Draft::default());
    pin.finish();
}

#[test]
fn the_lists_split_the_same() {
    let mut pin = pin::Pin::new("the_lists_split_the_same");
    for list in LISTS {
        pin.check(format!("{list:?}"), new::split_list(list).unwrap());
    }
    pin.finish();
}

#[test]
fn drafts_are_sent_or_refused_for_the_reason_interface_6_gave() {
    let mut pin = pin::Pin::new("drafts_are_sent_or_refused_for_the_reason_interface_6_gave");
    let mut rng = Rng(7);
    for _ in 0..800 {
        let n = draft(&mut rng);
        pin.fold(
            format!("{:?} {:?} {:?}", n.from, n.to, n.cc),
            refusal(n.check()),
        );
    }
    pin.finish();
}

#[test]
fn the_message_made_is_the_one_interface_6_made() {
    let mut pin = pin::Pin::new("the_message_made_is_the_one_interface_6_made");
    let mut rng = Rng(11);
    for _ in 0..800 {
        let n = draft(&mut rng);
        let date = "Tue, 3 Nov 2026 09:15:00 +0000";
        pin.fold(format!("{n:?}"), n.message(date, "42").unwrap());
    }
    pin.finish();
}

#[test]
fn a_long_line_is_quoted_printable_and_no_line_is_too_long() {
    let mut n = draft(&mut Rng(1));
    n.from = "ada@example.org".into();
    n.to = "a@b.org".into();
    n.body = format!("short\n{}\nend\n", "x".repeat(2000));
    let message = n.message("Tue, 3 Nov 2026 09:15:00 +0100", "1").unwrap();
    assert!(message.contains("quoted-printable"));
    assert!(message.split("\r\n").all(|l| l.len() <= 998));
    n.body = "x".repeat(900);
    assert!(n.message("d", "1").unwrap().contains("7bit"));
    n.body = "x".repeat(new::MAX_BODY + 1);
    assert_eq!(refusal(n.check()), Err(Problem::TooLong));
}

#[test]
fn headers_cannot_be_broken_out_of() {
    let mut n = new::Draft::with_sender("Eve\r\nBcc: spy@example.org", "e@x.org").unwrap();
    n.subject = "Hi\r\nBcc: spy@example.org".into();
    n.to = "a@b.org".into();
    assert!(!n.message("x", "1").unwrap().contains("\r\nBcc:"));
}

#[test]
fn what_a_reader_makes_of_a_message_is_what_was_written() {
    let mut n = new::Draft::with_sender("Ada Lovelace", "ada@example.org").unwrap();
    n.to = "\"Doe, Jane\" <jane@example.org>; bob@example.org".into();
    n.subject = "Grüße".into();
    n.body = "Hallo Jürgen,\n.punkt am Anfang\n".into();
    let message = n.message("Tue, 3 Nov 2026 09:15:00 +0000", "42").unwrap();
    assert!(message.is_ascii());
    let storage = MemoryStorage::new().with("m.eml", message.as_bytes());
    let info = new_render::render(&storage, "m.eml", "m.txt", 4096).unwrap();
    assert_eq!(info.subject, "Grüße");
    assert_eq!(info.to.len(), 2);
    assert_eq!(
        storage.get("m.txt").unwrap(),
        b"Hallo J\xc3\xbcrgen,\n.punkt am Anfang\n"
    );
}

fn letters() -> Vec<String> {
    vec![
        "From: Jane Doe <jane@example.org>\nTo: Ada <ada@example.org>, bob@example.org\nCc: Jane Doe <JANE@example.org>, carol@example.org\nSubject: Lunch\nMessage-ID: <m1@x>\nReferences: <m0@x>\n\nNoon?\n".into(),
        "From: Jane <jane@example.org>\nReply-To: list@example.org\nSubject: RE: Lunch\nMessage-ID: <m2@x>\n\nOk\n".into(),
        "From: x@y.org\n\n".into(),
    ]
}

#[test]
fn a_reply_is_the_one_interface_6_made() {
    let mut pin = pin::Pin::new("a_reply_is_the_one_interface_6_made");
    for text in letters() {
        let new_storage = MemoryStorage::new().with("l", text.as_bytes());
        let new_letter = new_store::read_letter(&new_storage, "l").unwrap().unwrap();
        for all in [false, true] {
            let n = new::reply_to(&new_letter, "ada@example.org", all, "Jane wrote:").unwrap();
            pin.check(format!("{text:?} all={all}"), n.to_text().unwrap());
            pin.check("from", &n.from);
        }
    }
    pin.finish();
    let letter = letter_of(&letters()[0]);
    let one = new::reply_to(&letter, "ada@example.org", false, "Jane wrote:").unwrap();
    assert_eq!(
        (one.to.as_str(), one.cc.as_str()),
        ("Jane Doe <jane@example.org>", "")
    );
    assert_eq!(one.subject, "Re: Lunch");
    assert_eq!(one.references, "<m0@x> <m1@x>");
    assert_eq!(one.body, "\n\nJane wrote:\n> Noon?\n");
    let all = new::reply_to(&letter, "ada@example.org", true, "").unwrap();
    assert_eq!(all.cc, "bob@example.org, carol@example.org");
}

fn letter_of(text: &str) -> new_store::Letter {
    let storage = MemoryStorage::new().with("l", text.as_bytes());
    new_store::read_letter(&storage, "l").unwrap().unwrap()
}

#[test]
fn dates_and_quotes_are_the_same() {
    let mut pin = pin::Pin::new("dates_and_quotes_are_the_same");
    for seconds in [
        0,
        1_793_693_700,
        -86_400,
        4_102_444_800,
        i64::MAX / 4,
        i64::MIN / 4,
    ] {
        pin.check(format!("{seconds}"), new::date_header(seconds).unwrap());
    }
    assert_eq!(
        new::date_header(1_793_693_700).unwrap(),
        "Tue, 3 Nov 2026 08:15:00 +0000"
    );
    for (text, intro, max) in [
        ("a\nb\n", "Ada wrote:", 10),
        ("a\nb\nc", "", 2),
        ("", "x", 3),
    ] {
        pin.check("", new::quote(text, intro, max).unwrap());
    }
    pin.finish();
}
