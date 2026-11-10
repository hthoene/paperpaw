//! TEMPORARY
#![allow(clippy::all, clippy::pedantic, missing_docs, dead_code, unused)]
use paperpaw_plugin_mail::{compose as old, store as old_store};
use paperpaw_sdk::{MemoryStorage, Storage};
use std::fmt::Write as _;

struct Out(String);
impl Out {
    fn push(&mut self, tag: &str, content: String) {
        assert!(!content.contains('\n'), "{content}");
        writeln!(self.0, "{tag}\t{content}").unwrap();
    }
}
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
fn letters() -> Vec<String> {
    vec![
        "From: Jane Doe <jane@example.org>\nTo: Ada <ada@example.org>, bob@example.org\nCc: Jane Doe <JANE@example.org>, carol@example.org\nSubject: Lunch\nMessage-ID: <m1@x>\nReferences: <m0@x>\n\nNoon?\n".into(),
        "From: Jane <jane@example.org>\nReply-To: list@example.org\nSubject: RE: Lunch\nMessage-ID: <m2@x>\n\nOk\n".into(),
        "From: x@y.org\n\n".into(),
    ]
}

fn draft(rng: &mut Rng) -> old::Draft {
    let mut o = old::Draft::default();
    o.from_name = rng.pick(&NAMES).into();
    o.from = rng.pick(&["ada@example.org", "", "bad", "x@y.org"]).into();
    o.to = rng.pick(&LISTS).into();
    o.cc = rng.pick(&LISTS).into();
    o.subject = rng.pick(&SUBJECTS).into();
    o.body = rng.pick(&BODIES).into();
    o.in_reply_to = rng.pick(&["", "<1@x>"]).into();
    o.references = rng.pick(&["", "<0@x>"]).into();
    o
}

#[test]
fn dump_compose() {
    let mut out = Out(String::new());
    let mut rng = Rng(11);
    for n in 0..60 {
        let d = draft(&mut rng);
        let date = "Tue, 3 Nov 2026 09:15:00 +0000";
        out.push(
            "draft",
            format!(
                "{n}\t{:?}\t{:?}\t{:?}\t{:?}",
                d.to_text(),
                d.check(),
                d.message(date, "42"),
                old::Draft::from_text(&d.to_text()).to_text() == d.to_text()
            ),
        );
    }
    for list in LISTS {
        out.push("split", format!("{list:?}\t{:?}", old::split_list(list)));
    }
    for (n, text) in letters().iter().enumerate() {
        let storage = MemoryStorage::new().with("l", text.as_bytes());
        let letter = old_store::read_letter(&storage, "l").unwrap();
        for all in [false, true] {
            let r = old::reply_to(&letter, "ada@example.org", all, "Jane wrote:");
            out.push(
                "reply",
                format!("{n}\t{all}\t{:?}\t{:?}", r.to_text(), r.from),
            );
        }
    }
    for seconds in [
        0i64,
        1_793_693_700,
        -86_400,
        4_102_444_800,
        i64::MAX / 4,
        i64::MIN / 4,
    ] {
        out.push(
            "date_header",
            format!("{seconds}\t{:?}", old::date_header(seconds)),
        );
    }
    for (text, intro, max) in [
        ("a\nb\n", "Ada wrote:", 10),
        ("a\nb\nc", "", 2),
        ("", "x", 3),
    ] {
        out.push(
            "quote",
            format!(
                "{text:?}\t{intro:?}\t{max}\t{:?}",
                old::quote(text, intro, max)
            ),
        );
    }
    std::fs::write(
        format!("{}/testdata/compose.golden", env!("CARGO_MANIFEST_DIR")),
        out.0,
    )
    .unwrap();
}
