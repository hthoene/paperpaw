//! The answer, the question, the kept offer, the counting and the dates.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use paperpaw_sdk::native::net::Reason;

const BODY: &str = r#"{"version": "0.5.0", "url": "/api/v1/updates/paperpaw-x4-0.5.0-update.bin",
    "size": 6194784,
    "plugins": ["paperpaw-plugin-stats 0.5.0"],
    "missing": ["paperpaw-plugin-chess not-ready"],
    "waiting": ["dict 0.3.1"],
    "code": [{"id": "stats", "name": "Statistics", "version": "0.5.0",
      "files": [{"path": "plugin.ini", "url": "https://x/p.ini", "size": 120,
                 "sha256": "0123456789012345678901234567890123456789012345678901234567890123"},
                {"path": "code.ppn", "url": "https://x/c.ppn", "size": 412345,
                 "sha1": "0123456789012345678901234567890123456789"}]},
     {"id": "bad id", "name": "X", "version": "1", "files": []}]}"#;

fn read(body: &str, running: &str, betas: bool, piece: usize) -> Option<Offer> {
    let mut reader = Reader::default();
    for part in body.as_bytes().chunks(piece) {
        reader.feed(part).unwrap();
    }
    reader.finish(running, betas).unwrap()
}

#[test]
fn an_answer_is_read_in_any_pieces() {
    for piece in [1, 7, 64, 4096] {
        let offer = read(BODY, "0.4.0", false, piece).unwrap();
        assert_eq!(offer.version, "0.5.0");
        assert_eq!(offer.size, 6_194_784);
        assert_eq!(offer.plugins, "paperpaw-plugin-stats 0.5.0\n");
        assert_eq!(offer.missing, "paperpaw-plugin-chess not-ready\n");
        assert_eq!(offer.waiting, "dict 0.3.1\n");
        // The build with an invalid id is left out.
        assert_eq!(offer.builds.len(), 1, "{piece}");
        let build = &offer.builds[0];
        assert_eq!(
            (build.package.plugin.as_str(), build.version.as_str()),
            ("stats", "0.5.0")
        );
        assert_eq!(build.package.files.len(), 2);
        assert_eq!(build.package.files[1].size, 412_345);
    }
}

#[test]
fn what_is_kept_is_what_is_read() {
    let offer = read(BODY, "0.4.0", false, 4096).unwrap();
    let text = offer.to_text().unwrap();
    assert!(text.starts_with("0.5.0\n/api/v1/updates/paperpaw-x4-0.5.0-update.bin\n6194784\n"));
    assert_eq!(Offer::from_text(&text).unwrap(), Some(offer));
    // A file that is no offer, and lines it does not know.
    assert_eq!(Offer::from_text("soon\n").unwrap(), None);
    assert_eq!(Offer::from_text("0.5.0\nu\nmany\n").unwrap(), None);
    assert_eq!(Offer::from_text("0.5.0/x\nu\n1\n").unwrap(), None);
    let odd = Offer::from_text("0.5.0\nu\n1\nnative 00 00\nwhat is this\n").unwrap();
    assert_eq!(odd.unwrap().version, "0.5.0");
}

/// The interface-6 file is read as it was written (`docs/data-files.md`).
#[test]
fn an_offer_of_interface_6_is_read() {
    let text = "0.5.0\n/api/v1/updates/u.bin\n6194784\nplugin paperpaw-plugin-stats 0.5.0\n\
        missing paperpaw-plugin-chess not-ready\nnative 0002aabbccddeeff 6a1f03c2\n\
        build stats 0.5.0 - Statistics\n\
        file 120 sha256:0123456789012345678901234567890123456789012345678901234567890123 plugin.ini https://x/p.ini\n";
    let offer = Offer::from_text(text).unwrap().unwrap();
    assert_eq!(offer.builds.len(), 1);
    assert_eq!(offer.builds[0].package.files[0].path, "plugin.ini");
    assert_eq!(offer.missing, "paperpaw-plugin-chess not-ready\n");
}

#[test]
fn only_a_newer_update_is_an_offer() {
    let body = |version: &str| alloc::format!(r#"{{"version": "{version}", "url": "u"}}"#);
    for (answer, running, betas, named) in [
        ("0.5.0", "0.4.0", false, true),
        ("0.4.0", "0.4.0", false, false),
        ("0.3.0", "0.4.0", false, false),
        ("", "0.4.0", false, false),
        // Names no version: it would name the file downloaded to.
        ("0.5.0/../../x", "0.4.0", false, false),
        ("9", "0.4.0", false, false),
        // Betas where the reader asked, or the device runs one; newer only.
        ("0.5.0-beta.1", "0.4.0", false, false),
        ("0.5.0-beta.1", "0.4.0", true, true),
        ("0.5.0-beta.2", "0.5.0-beta.1", false, true),
        ("0.5.0-beta.10", "0.5.0-beta.2", false, true),
        ("0.5.0", "0.5.0-beta.10", false, true),
        ("0.5.0-beta.1", "0.5.0-beta.2", true, false),
        ("0.5.0-beta.3", "0.5.0", true, false),
    ] {
        let offer = read(&body(answer), running, betas, 4096);
        assert_eq!(
            offer.is_some(),
            named,
            "{answer} on {running} (betas {betas})"
        );
    }
    for bad in [r#"{"version": "0.5.0"}"#, "not json", ""] {
        assert_eq!(read(bad, "0.4.0", false, 4096), None, "{bad}");
    }
}

#[test]
fn the_question_says_what_runs() {
    let firmware = Firmware {
        project: "paperpaw-xteink",
        chip: 5,
        version: "0.4.0",
    };
    let mut question = Question::new(&firmware).unwrap();
    question.built_in("stats", "0.4.0").unwrap();
    question.built_in("chess", "1.0.0").unwrap();
    question.code_begin().unwrap();
    question.code("hello", "0.1.0").unwrap();
    let (counted, _) = counting(false, None, None, None);
    let body = question.finish(Some((7, 3)), false, &counted).unwrap();
    assert_eq!(
        body,
        "{\"project\":\"paperpaw-xteink\",\"chip\":5,\"version\":\"0.4.0\",\
         \"plugins\":[\"paperpaw-plugin-stats 0.4.0\",\"paperpaw-plugin-chess 1.0.0\"],\
         \"code\":[\"hello 0.1.0\"],\"abi_major\":7,\"abi_draft\":3,\"trigger\":\"manual\"}"
    );
    // Betas where asked, the counting where the time is known; a name that is
    // no plain text is quoted.
    let mut question = Question::new(&Firmware {
        project: "a\"b",
        ..firmware
    })
    .unwrap();
    question.code_begin().unwrap();
    let day = days_from_civil(2026, 10, 25);
    let (counted, _) = counting(true, Some(day), None, None);
    let body = question.finish(None, true, &counted).unwrap();
    assert!(body.starts_with("{\"project\":\"a\\\"b\""), "{body}");
    assert!(
        body.ends_with(
            "\"code\":[],\"betas\":true,\"trigger\":\"auto\",\
             \"first\":[\"day\",\"week\",\"month\"],\"age\":\"new\"}"
        ),
        "{body}"
    );
    assert!(body.contains("\"plugins\":[]"), "{body}");
}

/// The counting as interface 6 kept it (`count.rs`): the first check is the
/// first of everything, a second one the same day of nothing, the dates are
/// kept, and a clock set back changes nothing it should not.
#[test]
fn checks_are_counted_without_knowing_the_device() {
    let day = days_from_civil(2026, 10, 25);
    let (counted, keep) = counting(true, Some(day), None, None);
    assert_eq!(
        counted.periods.unwrap().names().collect::<Vec<_>>(),
        ["day", "week", "month"]
    );
    assert_eq!(counted.age, Some("new"));
    assert_eq!(
        keep,
        Keep {
            last: Some(day),
            first: Some(day)
        }
    );
    // The same day again: of nothing, nothing to keep.
    let (counted, keep) = counting(false, Some(day), Some(day), Some(day));
    assert_eq!(counted.periods.unwrap().names().count(), 0);
    assert_eq!(counted.trigger, "manual");
    assert_eq!(
        keep,
        Keep {
            last: None,
            first: None
        }
    );
    // A device that counted before the first day was kept: its last check.
    let (counted, keep) = counting(false, Some(day + 40), Some(day), None);
    assert_eq!(counted.age, Some("month"));
    assert_eq!(keep.first, Some(day));
    // The clock set back: the first after today is today, the last stays.
    let (counted, keep) = counting(false, Some(day - 3), Some(day), Some(day));
    assert_eq!(counted.age, Some("new"));
    assert_eq!(
        keep,
        Keep {
            last: None,
            first: Some(day - 3)
        }
    );
    // No time: nothing counted and nothing kept.
    let (counted, keep) = counting(true, None, Some(day), Some(day));
    assert_eq!((counted.periods, counted.age), (None, None));
    assert_eq!(
        keep,
        Keep {
            last: None,
            first: None
        }
    );
}

#[test]
fn dates_are_kept_as_interface_6_wrote_them() {
    let day = days_from_civil(2026, 1, 9);
    assert_eq!(date_text(day).unwrap(), "2026-01-09\n");
    assert_eq!(parse_date(b"2026-01-09\n"), Some(day));
    for bad in ["", "2026-13-01", "2026-01-32", "x-y-z", "2026-01", "-1-1-1"] {
        assert_eq!(parse_date(bad.as_bytes()), None, "{bad}");
    }
}

#[test]
fn the_server_is_named_as_the_reader_knows_it() {
    assert_eq!(server(""), DEFAULT_SERVER);
    assert_eq!(server("  https://example.org/ "), "https://example.org");
    assert_eq!(server_name("https://example.org/api"), "example.org");
    assert_eq!(server_name("example.org"), "example.org");
    assert_eq!(
        address("https://s", "/api/v1/u.bin").unwrap(),
        "https://s/api/v1/u.bin"
    );
    assert_eq!(
        address("https://s", "https://o/u.bin").unwrap(),
        "https://o/u.bin"
    );
}

#[test]
fn a_failed_request_says_why() {
    assert_eq!(Trouble::of(Reason::Ok, 503), Trouble::Status(503));
    assert_eq!(
        Trouble::of(Reason::NoNetwork, 0),
        Trouble::Reason("update.net.no-network")
    );
    assert_eq!(Trouble::of(Reason::Dns, 0), Trouble::of(Reason::Refused, 0));
    assert_eq!(
        Trouble::of(Reason::Offline, 0),
        Trouble::of(Reason::NetworkOff, 0)
    );
    assert_eq!(
        Trouble::of(Reason::Other(99), 0),
        Trouble::Reason("update.net.other")
    );
}

/// Whatever the server sends, the answer is no offer or a sane one: nothing
/// panics, a name that is no version, an address that is blank and a body that
/// is no JSON offer nothing.
#[test]
fn hostile_answers_of_the_server_offer_nothing_or_something_sane() {
    let deep = alloc::format!("{}{}", "[".repeat(500), "]".repeat(500));
    let bodies: [&[u8]; 7] = [
        b"",
        b"not json",
        br#"{"version": 5, "url": "u"}"#,
        br#"{"version": "v9.9.9", "url": "u"}"#,
        br#"{"version": "0.5.0", "url": "  "}"#,
        deep.as_bytes(),
        &[0xff, 0xfe, b'{', 0x80],
    ];
    for body in bodies {
        let mut reader = Reader::default();
        // A body the parser refuses is no offer either.
        let fed = reader.feed(body);
        let offer = fed.and_then(|()| reader.finish("0.4.0", false));
        assert!(matches!(offer, Ok(None) | Err(_)), "{body:?}: {offer:?}");
    }
    // The streaming parser reads a number as the text it is: an address that is
    // a number is kept as one (the download of it fails and says so).
    let odd = read(r#"{"version": "0.5.0", "url": 7}"#, "0.4.0", false, 4096).unwrap();
    assert_eq!(odd.url, "7");
    // Numbers out of range, lists of the wrong kind: offered, sane.
    for body in [
        r#"{"version": "0.5.0", "url": "https://x/u.bin", "size": -12,
            "plugins": [1, null, {"a": 1}], "missing": "not a list"}"#,
        r#"{"version": "0.5.0", "url": "u", "size": 1e30,
            "plugins": ["paperpaw-plugin-stats 0.5.0"]}"#,
    ] {
        let offer = read(body, "0.4.0", false, 4096).unwrap();
        assert_eq!(offer.version, "0.5.0");
        assert_eq!(offer.size, 0, "no size known");
        assert!(offer.missing.is_empty());
    }
}
