//! The native readers of mail give the same answers as the interface-6 ones
//! on what both take: encodings, headers, addresses, dates. The native ones
//! add bounds (headers, values, parameters, addresses) and are tested for the
//! differences that come of them, and for what hostile bytes do.
//!
//! The intended differences: a message has at most 256 headers of 8 KiB, a
//! Content-Type 32 parameters, a header 32 addresses; a date is read for the
//! years 1 to 9999 only.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_mail::native_model::{codec as new_codec, header as new_header, mem};
mod pin;

/// A small deterministic generator, so that failures repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn bytes(rng: &mut Rng, len: usize, alphabet: &[u8]) -> Vec<u8> {
    (0..len)
        .map(|_| {
            if rng.below(6) == 0 {
                rng.next() as u8
            } else {
                alphabet[rng.below(alphabet.len())]
            }
        })
        .collect()
}

/// Pieces of headers, put together at random.
fn header_text(rng: &mut Rng, pieces: usize) -> Vec<u8> {
    const PARTS: [&str; 30] = [
        "Subject: Hello\r\n",
        "From: Ada Lovelace <ada@example.org>\r\n",
        "To: \"Doe, John\" <john@example.org>, bob@example.org (Bob)\r\n",
        "Date: Tue, 3 Nov 2026 09:15:00 +0100\r\n",
        "Content-Type: multipart/mixed; boundary=\"b1\"\r\n",
        "Content-Type: text/plain; charset=iso-8859-1\r\n",
        "Content-Disposition: attachment; filename*=utf-8''%C3%A4.txt\r\n",
        " continued value\r\n",
        "\tmore\r\n",
        "Subject: =?utf-8?B?w6TDtsO8?=\r\n",
        "Subject: =?iso-8859-1?Q?caf=E9_au_lait?= =?utf-8?q?x?=\r\n",
        "Message-ID: <abc@example.org>\r\n",
        "References: <a@x> <b@x>\r\n",
        "X-Odd:\r\n",
        "no colon here\r\n",
        "bad name: x\r\n",
        "\r\n",
        "\n",
        "Subject: caf\u{e9}\r\n",
        ":\r\n",
        "From: <>\r\n",
        "To: a@b, ,c@d,\r\n",
        "Date: 99 Foo 1\r\n",
        "Content-Type: ;;;\r\n",
        "Content-Type: text/html; charset=\"utf-8\"; name=x; name=y\r\n",
        "From: \"unterminated <a@b>\r\n",
        "To: (nested (comment)) a@b\r\n",
        "Subject: Re: Re: Subject\r\n",
        "References: <1@x> <2@x> <3@x>\r\n",
        "Date: Thu, 1 Jan 1970 00:00:00 +0000\r\n",
    ];
    let mut out = Vec::new();
    for _ in 0..pieces {
        out.extend_from_slice(PARTS[rng.below(PARTS.len())].as_bytes());
    }
    out
}

#[test]
fn base64_is_the_same_both_ways_and_in_pieces() {
    let mut pin = pin::Pin::new("base64_is_the_same_both_ways_and_in_pieces");
    let mut rng = Rng(1);
    for len in [0, 1, 2, 3, 4, 5, 57, 58, 300, 1000] {
        for _ in 0..6 {
            let data = bytes(&mut rng, len, &[0, 1, 255, 65, 200]);
            pin.fold("", new_codec::encode_base64(&data).unwrap());
            pin.fold("", new_codec::base64_line(&data).unwrap());
            let text = new_codec::encode_base64(&data).unwrap();
            pin.fold("", new_codec::decode_base64(text.as_bytes()).unwrap());
            // Garbage in the middle, padding, and cut pieces.
            let noisy = bytes(&mut rng, len, b"ABCDabcd0123+/= \r\n-_!");
            pin.fold(
                format!("{noisy:?}"),
                new_codec::decode_base64(&noisy).unwrap(),
            );
            let mut pieces = new_codec::Base64::default();
            let mut out = Vec::new();
            for piece in noisy.chunks(1 + rng.below(7)) {
                pieces.push(piece, &mut out).unwrap();
            }
            pieces.finish(&mut out).unwrap();
            pin.fold("", &(out));
        }
    }
    pin.finish();
}

#[test]
fn quoted_printable_and_encoded_words_are_the_same() {
    let mut pin = pin::Pin::new("quoted_printable_and_encoded_words_are_the_same");
    let mut rng = Rng(2);
    for len in [0, 1, 3, 10, 80, 400] {
        for _ in 0..10 {
            let qp = bytes(&mut rng, len, b"abc =\r\nA9F\t_?=E9=C3");
            for header in [false, true] {
                pin.fold(
                    format!("{qp:?} {header}"),
                    new_codec::decode_qp(&qp, header).unwrap(),
                );
            }
            let text = String::from_utf8_lossy(&qp).into_owned();
            pin.fold(format!("{text:?}"), new_codec::encode_qp(&text).unwrap());
            pin.fold(format!("{text:?}"), new_codec::encode_words(&text).unwrap());
        }
    }
    for text in [
        "plain",
        "=?utf-8?B?w6TDtsO8?=",
        "=?UTF-8?Q?a_b=C3=A4?= tail",
        "=?iso-8859-1?Q?caf=E9?= =?iso-8859-1?Q?_au?=",
        "=?koi8-r?B?8NLJ18XU?=",
        "=?windows-1251?Q?=CF=F0=E8=E2=E5=F2?=",
        "=?unknown?Q?x?=",
        "=?utf-8?B?",
        "=?utf-8?X?abc?=",
        "caf\u{e9} \u{4e16}\u{754c}",
        "=?utf-8*de?Q?x?=",
        "a =?utf-8?Q?b?= =?utf-8?Q?c?= d",
    ] {
        pin.fold(
            text.to_string(),
            new_codec::decode_words(text.as_bytes()).unwrap(),
        );
    }
    // Latin-1 bytes that are no UTF-8, whole and cut.
    let latin = [b'c', b'a', b'f', 0xE9, b' ', 0xFF, 0xC3];
    pin.fold("", new_codec::decode_words(&latin).unwrap());
    pin.finish();
}

#[test]
fn charsets_turn_bytes_into_the_same_text_whole_and_in_pieces() {
    let mut pin = pin::Pin::new("charsets_turn_bytes_into_the_same_text_whole_and_in_pieces");
    let names = [
        "utf-8",
        "us-ascii",
        "iso-8859-1",
        "iso-8859-15",
        "windows-1252",
        "iso-8859-2",
        "windows-1250",
        "windows-1251",
        "koi8-r",
        "UTF-8*de",
        "\"utf-8\"",
        "klingon",
        "",
    ];
    let mut rng = Rng(3);
    for name in names {
        let new = new_codec::Charset::of(name);
        pin.fold(name.to_string(), new.is_some());
        let Some(new) = new else {
            continue;
        };
        for len in [0, 1, 5, 64, 300] {
            for _ in 0..8 {
                let data: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
                let text = new_codec::text_of(new, &data).unwrap();
                // In pieces: a character cut in two waits for the next.
                let mut decoder = new_codec::Decoder::new(new);
                let mut out = String::new();
                for piece in data.chunks(1 + rng.below(5)) {
                    decoder.push(piece, &mut out).unwrap();
                }
                decoder.finish(&mut out).unwrap();
                // Pieces give what the whole gives.
                assert_eq!(out, text, "{name} {data:?}");
                pin.fold(name.to_string(), &(text));
            }
        }
    }
    pin.finish();
}

#[test]
fn headers_are_the_same() {
    let mut pin = pin::Pin::new("headers_are_the_same");
    let mut rng = Rng(4);
    for pieces in [0, 1, 2, 5, 12, 30] {
        for _ in 0..40 {
            let raw = header_text(&mut rng, pieces);
            let new = new_header::Headers::parse(&raw).unwrap();
            for name in [
                "subject",
                "from",
                "to",
                "date",
                "content-type",
                "content-disposition",
                "message-id",
                "references",
                "x-odd",
                "bad name",
                "",
            ] {
                pin.fold(format!("{name} in {raw:?}"), new.get(name));
            }
            pin.fold("", new.len());
            let new_type = new_header::ContentType::parse(new.get("content-type")).unwrap();
            pin.fold("", &(new_type.kind));
            pin.fold("", new_type.is_multipart());
            for param in ["boundary", "charset", "name", "filename"] {
                pin.fold(param.to_string(), new_type.param(param));
            }
        }
    }
    // Garbage as headers.
    for _ in 0..200 {
        let len = rng.below(300);
        let raw = bytes(&mut rng, len, b"abc: \r\n\t=?-");
        let new = new_header::Headers::parse(&raw).unwrap();
        pin.fold(format!("{raw:?}"), new.len());
        pin.fold("", new.get("a"));
    }
    pin.finish();
}

#[test]
fn content_types_are_the_same() {
    let mut pin = pin::Pin::new("content_types_are_the_same");
    for value in [
        None,
        Some(""),
        Some("text/plain"),
        Some("TEXT/HTML; Charset=\"UTF-8\""),
        Some("multipart/mixed; boundary=\"a;b\"; x=y"),
        Some("application/octet-stream; name*=utf-8''%C3%A4.bin"),
        Some("attachment; filename*0=\"long\"; filename*1=\"name\""),
        Some("a; b; c=; =d"),
        Some(";;;"),
        Some("text/plain; charset"),
        Some("text/plain; name*=%zz"),
    ] {
        let new = new_header::ContentType::parse(value).unwrap();
        pin.check(format!("{value:?}"), &(new.kind));
        for param in ["charset", "boundary", "name", "filename", "x", "c", "b"] {
            pin.check(format!("{value:?} {param}"), new.param(param));
        }
    }
    pin.finish();
}

#[test]
fn addresses_dates_and_subjects_are_the_same() {
    let mut pin = pin::Pin::new("addresses_dates_and_subjects_are_the_same");
    let lists = [
        "",
        "ada@example.org",
        "Ada Lovelace <ada@example.org>",
        "\"Doe, John\" <john@example.org>, bob@example.org (Bob)",
        "a@b, ,c@d,",
        "<>",
        "(nested (comment)) a@b",
        "\"unterminated <a@b>",
        "Group: a@b, c@d;",
        "no address, nobody",
        "=?utf-8?B?w6TDtsO8?= <x@y>",
    ];
    for list in lists {
        let new = new_header::addresses(list).unwrap();
        pin.check(list.to_string(), new.len());
        for b in &new {
            pin.check(list.to_string(), (&b.name, &b.address, b.shown()));
        }
    }
    for date in [
        "Tue, 3 Nov 2026 09:15:00 +0100",
        "3 Nov 2026 09:15 -0500",
        "Thu, 1 Jan 1970 00:00:00 +0000",
        "Mon, 29 Feb 2016 23:59:59 GMT",
        "Fri, 31 Dec 1999 23:59:60 +0000 (CET)",
        "Sat, 4 Jul 76 12:00:00 UT",
        "garbage",
        "",
        "99 Foo 1",
        "1 Jan 2026",
    ] {
        pin.check(date.to_string(), new_header::date_seconds(date));
    }
    for subject in [
        "",
        "Hello",
        "Re: Hello",
        "RE: Re: Hello",
        "AW: Hello",
        "re:hello",
        "Fwd: Re: x",
    ] {
        pin.check(
            subject.to_string(),
            new_header::reply_subject(subject).unwrap(),
        );
    }
    for days in [-1_000_000i64, -1, 0, 1, 18_000, 2_932_896] {
        let (y, m, d) = new_header::civil_from_days(days);
        pin.check(days, (y, m, d));
        // The native version holds the year to 1..=9999.
        if (1..=9999).contains(&y) {
            assert_eq!(new_header::days_from_civil(y, m, d), days);
        }
    }
    for text in ["", "short", "caf\u{e9} caf\u{e9}", &"x".repeat(1000)] {
        for max in [0, 1, 2, 3, 4, 5, 100, 2000] {
            pin.check("", new_header::cut(text, max));
            pin.check("", new_header::capped(text, max).unwrap());
        }
    }
    let ids = "<1@x> <2@x> <3@x> <4@x> <\u{e9}@x>";
    for max in [0, 3, 6, 12, 20, 40, 100] {
        pin.check(format!("{max}"), new_header::last_ids(ids, max).unwrap());
    }
    pin.finish();
}

#[test]
fn the_bounds_are_the_native_readers_own() {
    let mut pin = pin::Pin::new("the_bounds_are_the_native_readers_own");
    // More headers than fit: the first ones are read, the rest are not.
    let mut raw = Vec::new();
    for n in 0..(new_header::MAX_FIELDS + 50) {
        raw.extend_from_slice(format!("X-{n}: {n}\r\n").as_bytes());
    }
    let headers = new_header::Headers::parse(&raw).unwrap();
    assert_eq!(headers.len(), new_header::MAX_FIELDS);
    assert_eq!(headers.get("x-0"), Some("0"));
    assert_eq!(headers.get(&format!("x-{}", new_header::MAX_FIELDS)), None);
    // A value is cut, folded or not.
    let long = "y".repeat(3 * new_header::MAX_VALUE);
    let raw = format!("A: {long}\r\nB: x\r\n {long}\r\n");
    let headers = new_header::Headers::parse(raw.as_bytes()).unwrap();
    assert_eq!(headers.get("a").unwrap().len(), new_header::MAX_VALUE);
    assert!(headers.get("b").unwrap().len() <= new_header::MAX_VALUE);
    // Parameters and addresses are counted.
    let many: String = (0..100).map(|n| format!("; p{n}=v")).collect();
    let content = new_header::ContentType::parse(Some(&format!("text/plain{many}"))).unwrap();
    assert_eq!(content.param("p0"), Some("v"));
    assert_eq!(content.param("p99"), None);
    let list: String = (0..100).map(|n| format!("a{n}@x, ")).collect();
    assert_eq!(
        new_header::addresses(&list).unwrap().len(),
        new_header::MAX_ADDRESSES
    );
    // Dates are read for the years 1 to 9999; nothing else is a date.
    // A year below 100 follows RFC 5322's two-digit rule, as in interface 6.
    pin.check("", new_header::date_seconds("1 Jan 0000 00:00 +0000"));
    assert!(new_header::date_seconds("1 Jan 10000 00:00 +0000").is_none());
    assert!(new_header::date_seconds("1 Jan 9999 23:59 +0000").is_some());
    assert!(new_header::date_seconds("99999999999999999999 Jan 2000").is_none());
    pin.finish();
}

#[test]
fn a_secret_hides_in_debug_and_comes_out_as_asked() {
    let creds = new_codec::plain_credentials("ada", "hunter2").unwrap();
    assert_eq!(creds.as_slice(), b"\0ada\0hunter2");
    assert!(!format!("{creds:?}").contains("hunter2"));
    let mut secret = mem::Secret::with_capacity(8).unwrap();
    secret.push_str("hunter2").unwrap();
    assert_eq!(secret.as_str(), "hunter2");
    assert!(!format!("{secret:?}").contains("hunter2"));
    // `wipe` leaves zeros where the bytes were.
    let mut bytes = *b"hunter2";
    mem::wipe(&mut bytes);
    assert_eq!(bytes, [0; 7]);
    // Handed out, the text is the caller's to wipe.
    let text = secret.into_string();
    assert_eq!(text, "hunter2");
    mem::wipe_text(text);
}

#[test]
fn a_secret_never_grows_past_its_capacity() {
    // A grown buffer would leave its old copy in memory, unwiped.
    let mut secret = mem::Secret::with_capacity(4).unwrap();
    secret.push_str("abcd").unwrap();
    let before = secret.as_str().as_ptr();
    assert!(secret.push_str("e").is_err());
    assert_eq!(secret.as_str(), "abcd");
    assert_eq!(secret.as_str().as_ptr(), before);
    let mut bytes = mem::SecretBytes::with_capacity(2).unwrap();
    bytes.push_bytes(b"a").unwrap();
    bytes.push_byte(b'b').unwrap();
    assert!(bytes.push_byte(b'c').is_err());
    assert!(bytes.push_bytes(b"cd").is_err());
    assert_eq!(bytes.as_slice(), b"ab");
}

#[test]
fn days_from_civil_does_not_panic_on_any_year() {
    for year in [i64::MIN, -1, 0, 10_000, i64::MAX / 2, i64::MAX] {
        for (month, day) in [(0, 0), (1, 1), (2, i64::MAX), (13, i64::MIN), (12, 31)] {
            let _ = new_header::days_from_civil(year, month, day);
        }
    }
    // The year is held to 1..=9999.
    assert_eq!(
        new_header::days_from_civil(i64::MAX, 6, 1),
        new_header::days_from_civil(9999, 6, 1)
    );
    assert_eq!(
        new_header::days_from_civil(-5, 6, 1),
        new_header::days_from_civil(1, 6, 1)
    );
}

#[test]
fn hostile_bytes_are_read_without_a_panic() {
    let mut rng = Rng(7);
    for len in [0, 1, 2, 7, 100, 1000, 5000] {
        for _ in 0..20 {
            let data: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            let _ = new_codec::decode_base64(&data).unwrap();
            let _ = new_codec::decode_qp(&data, true).unwrap();
            let _ = new_codec::decode_qp(&data, false).unwrap();
            let _ = new_codec::decode_words(&data).unwrap();
            let _ = new_header::Headers::parse(&data).unwrap();
            let text = String::from_utf8_lossy(&data);
            let _ = new_header::ContentType::parse(Some(&text)).unwrap();
            let _ = new_header::addresses(&text).unwrap();
            let _ = new_header::date_seconds(&text);
            let _ = new_header::reply_subject(&text).unwrap();
            let _ = new_codec::encode_words(&text).unwrap();
            let _ = new_codec::encode_qp(&text).unwrap();
            let _ = new_header::last_ids(&text, rng.below(300)).unwrap();
        }
    }
    // Dates around every edge.
    for date in [
        "1 Jan 1 00:00 +0000",
        "31 Dec 9999 23:59:59 +9999",
        "1 Jan 2000 24:61:61 -9999",
        "0 Jan 2000",
        "32 Jan 2000",
        "29 Feb 2001 00:00 +0000",
        "-1 Jan 2000",
    ] {
        let _ = new_header::date_seconds(date);
    }
}
