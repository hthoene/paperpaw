//! The session with Calibre, without a host: what it answers, what it asks the
//! plugin to do, and where the screen's state goes, for the messages Calibre
//! sends (hostile ones too).
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
mod fake;

use fake::Fake;
use paperpaw_plugin_calibre::native_model::{self as model, Framing, frame, op};
use paperpaw_plugin_calibre::native_session::{
    Coming, Ending, FIND_TRIES, LIST_PART, Placed, Session, State, Step,
};

const BOUND: usize = 64 * 1024;

fn session() -> Session {
    Session::new(BOUND, true)
}

/// What Calibre says: `op` with `args`, as it goes on the wire.
fn says(session: &mut Session, device: &mut Fake, op: u8, args: &str) -> Vec<Step> {
    let wire = String::from_utf8(frame(op, args).unwrap()).unwrap();
    // The session is given the bytes as the plugin does: what it wants next.
    let mut rest = wire.as_str();
    let mut last = Vec::new();
    while !rest.is_empty() {
        let n = (session.wanted() as usize).min(rest.len());
        let (part, tail) = rest.split_at(n);
        rest = tail;
        last = session.received(device, part).unwrap();
    }
    last
}

/// The frames among `steps`, as text.
fn sent(steps: &[Step]) -> Vec<String> {
    steps
        .iter()
        .filter_map(|step| match step {
            Step::Send(frame) => Some(String::from_utf8(frame.clone()).unwrap()),
            _ => None,
        })
        .collect()
}

/// The messages in `frames`, parsed.
fn messages(frames: &[String]) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    for frame in frames {
        let mut framing = Framing::default();
        let message = framing.feed(frame).unwrap().unwrap();
        out.push((message.op, message.args));
    }
    out
}

#[test]
fn it_starts_by_finding_calibre_or_connecting_to_the_address() {
    let mut s = session();
    assert_eq!(s.start(BOUND, "").unwrap(), [Step::Find, Step::Redraw]);
    assert_eq!(s.state, State::Finding);
    let steps = s.start(BOUND, " 192.168.1.5:9091 ").unwrap();
    assert_eq!(
        steps[0],
        Step::Connect {
            host: "192.168.1.5".into(),
            port: 9091
        }
    );
    assert_eq!(s.state, State::Connecting);
    // No port, or one that is not: the default.
    for address in ["pc.local", "pc.local:abc", "pc.local:70000"] {
        let steps = s.start(BOUND, address).unwrap();
        let Step::Connect { host, port } = &steps[0] else {
            panic!("{steps:?}")
        };
        assert_eq!(*port, 9090, "{address}");
        assert!(host.starts_with("pc.local"), "{host}");
    }
}

#[test]
fn a_computer_not_trusted_is_asked_about_and_trusted_once_chosen() {
    let mut device = Fake::default();
    let mut s = session();
    s.start(BOUND, "").unwrap();
    let answer = "calibre wireless device client (on pc);59678,9090";
    let steps = s.answer(&device, answer, "192.168.1.5").unwrap();
    assert_eq!(steps, [Step::Redraw]);
    assert_eq!(
        s.state,
        State::Found {
            host: "192.168.1.5".into(),
            port: 9090
        }
    );
    assert_eq!(s.name.as_deref(), Some("pc"));
    // Nothing is connected to until the reader chooses.
    let steps = s.connect_found(&mut device).unwrap();
    assert_eq!(
        steps[0],
        Step::Connect {
            host: "192.168.1.5".into(),
            port: 9090
        }
    );
    assert_eq!(device.trusted.as_deref(), Some("192.168.1.5"));
    // A computer trusted before is connected to at once.
    let mut s = session();
    s.start(BOUND, "").unwrap();
    let steps = s.answer(&device, answer, "192.168.1.5").unwrap();
    assert!(matches!(steps[0], Step::Connect { .. }), "{steps:?}");
    // Another one answering is not: it is shown.
    let mut s = session();
    s.start(BOUND, "").unwrap();
    s.answer(&device, answer, "10.9.9.9").unwrap();
    assert!(matches!(s.state, State::Found { .. }));
    // What is not Calibre's answer is left alone.
    let mut s = session();
    s.start(BOUND, "").unwrap();
    assert!(s.answer(&device, "other", "10.0.0.1").unwrap().is_empty());
    assert_eq!(s.state, State::Finding);
}

#[test]
fn a_silent_network_is_asked_again_and_then_calibre_is_not_found() {
    let mut s = session();
    s.start(BOUND, "").unwrap();
    for _ in 1..FIND_TRIES {
        assert_eq!(s.find_over(false).unwrap(), [Step::Find]);
    }
    s.find_over(false).unwrap();
    assert_eq!(s.state, State::Ended(Ending::NotFound));
    // No network: said at once, in the words for where Wi-Fi is managed or not.
    let mut s = session();
    s.start(BOUND, "").unwrap();
    s.find_over(true).unwrap();
    assert_eq!(s.state, State::Ended(Ending::NoNetwork { managed: true }));
    let mut s = Session::new(BOUND, false);
    s.start(BOUND, "").unwrap();
    s.no_network().unwrap();
    assert_eq!(s.state, State::Ended(Ending::NoNetwork { managed: false }));
}

#[test]
fn the_device_introduces_itself_with_its_extensions_and_the_password_hash() {
    let mut device = Fake {
        password: Some("pw".into()),
        ..Fake::default()
    };
    let mut s = session();
    s.connected().unwrap();
    let steps = says(
        &mut s,
        &mut device,
        op::GET_INITIALIZATION_INFO,
        "{\"passwordChallenge\": \"abc\"}",
    );
    let frames = messages(&sent(&steps));
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, op::OK);
    let answer = &frames[0].1;
    assert!(answer.contains("\"acceptedExtensions\": [\"epub\", \"fb2.zip\", \"txt\"]"));
    assert!(answer.contains("\"deviceName\": \"My Reader\""));
    // sha1 of the password and the challenge, as Calibre computes it.
    assert!(answer.contains("\"passwordHash\": \"cf2b7e4e50a003a1a04361027f6a339c2dda6bb1\""));
    // Reading on follows the answer.
    assert_eq!(steps.last(), Some(&Step::Read));
    // Without a password, no hash.
    device.password = None;
    let steps = says(
        &mut s,
        &mut device,
        op::GET_INITIALIZATION_INFO,
        "{\"passwordChallenge\": \"abc\"}",
    );
    assert!(
        messages(&sent(&steps))[0]
            .1
            .contains("\"passwordHash\": \"\"")
    );
}

#[test]
fn what_calibre_says_of_itself_and_of_the_device_is_kept_and_answered() {
    let mut device = Fake::default();
    let mut s = session();
    s.connected().unwrap();
    let steps = says(
        &mut s,
        &mut device,
        op::SET_CALIBRE_DEVICE_INFO,
        "{\"device_store_uuid\": \"x\"}",
    );
    assert_eq!(messages(&sent(&steps)), [(op::OK, "{}".to_string())]);
    assert!(
        device
            .info
            .as_deref()
            .unwrap()
            .contains("device_store_uuid")
    );
    let steps = says(&mut s, &mut device, op::GET_DEVICE_INFORMATION, "{}");
    let (_, info) = &messages(&sent(&steps))[0];
    assert!(
        info.contains("\"device_info\": {\"device_store_uuid\": \"x\"}"),
        "{info}"
    );
    // The library's name shows.
    says(
        &mut s,
        &mut device,
        op::SET_LIBRARY_INFO,
        "{\"libraryName\": \"Books\"}",
    );
    assert_eq!(s.library.as_deref(), Some("Books"));
    // Space: what the card says, else a large number so that Calibre sends.
    device.free = Some(1234);
    device.total = Some(5000);
    let steps = says(&mut s, &mut device, op::FREE_SPACE, "{}");
    assert_eq!(
        messages(&sent(&steps))[0].1,
        "{\"free_space_on_device\": 1234}"
    );
    let steps = says(&mut s, &mut device, op::TOTAL_SPACE, "{}");
    assert_eq!(
        messages(&sent(&steps))[0].1,
        "{\"total_space_on_device\": 5000}"
    );
    device.free = None;
    device.total = None;
    let steps = says(&mut s, &mut device, op::FREE_SPACE, "{}");
    assert_eq!(
        messages(&sent(&steps))[0].1,
        format!("{{\"free_space_on_device\": {}}}", 64u64 << 30)
    );
}

#[test]
fn the_book_list_goes_in_parts_and_reading_on_comes_last() {
    let big = format!(
        "{{\"lpath\": \"a.epub\", \"title\": \"{}\"}}",
        "x".repeat(LIST_PART / 2)
    );
    let mut device = Fake::default();
    for n in 0..5 {
        device = device.with_book(&format!("b{n}.epub"), &big);
    }
    let mut s = session();
    s.connected().unwrap();
    let steps = says(&mut s, &mut device, op::GET_BOOK_COUNT, "{}");
    assert_eq!(
        messages(&sent(&steps))[0].1,
        "{\"count\": 5, \"willStream\": true, \"willScan\": true}"
    );
    // The list is sent after the count, not read on yet.
    assert!(!steps.contains(&Step::Read));
    assert!(s.listing());
    let mut parts = 0;
    let mut books = 0;
    while s.listing() {
        let steps = s.list_part(&device).unwrap();
        books += sent(&steps).len();
        parts += 1;
        assert!(parts < 10);
    }
    assert_eq!(books, 5);
    // About LIST_PART bytes a part: three books of half of it, then two.
    assert!(parts >= 2, "{parts}");
    // The last part reads on.
    let mut device = Fake::default().with_book("a.epub", "{\"lpath\": \"a.epub\"}");
    let mut s = session();
    says(&mut s, &mut device, op::GET_BOOK_COUNT, "{}");
    let steps = s.list_part(&device).unwrap();
    assert_eq!(steps.last(), Some(&Step::Read));
    // No books: no list, reading on at once.
    let mut device = Fake::default();
    let mut s = session();
    let steps = says(&mut s, &mut device, op::GET_BOOK_COUNT, "{}");
    assert_eq!(steps.last(), Some(&Step::Read));
    assert!(!s.listing());
}

fn book_args(lpath: &str, extra: &str) -> String {
    format!(
        "{{\"lpath\": {}, \"length\": 1234, \"thisBook\": 0, \"totalBooks\": 3, \"metadata\": {{\"title\": \"A Title\", \"uuid\": \"u-1\", \"lpath\": {}}}{extra}}}",
        model::json_string(lpath).unwrap(),
        model::json_string(lpath).unwrap(),
    )
}

#[test]
fn a_book_is_received_then_put_in_the_library_and_remembered() {
    let mut device = Fake::default();
    let mut s = session();
    s.connected().unwrap();
    let steps = says(
        &mut s,
        &mut device,
        op::SEND_BOOK,
        &book_args("Ann/A Title.epub", ""),
    );
    assert_eq!(
        steps,
        [
            Step::ReadFile {
                len: 1234,
                dropped: false
            },
            Step::Redraw
        ]
    );
    assert_eq!(
        s.state,
        State::Receiving {
            title: "A Title".into(),
            number: 1,
            total: 3
        }
    );
    let steps = s.saved().unwrap();
    let [Step::Import(coming)] = &steps[..] else {
        panic!("{steps:?}")
    };
    assert_eq!(coming.path, "Ann/A Title.epub");
    assert_eq!(coming.lpath, "Ann/A Title.epub");
    let steps = s.imported(&mut device, coming, Ok(())).unwrap();
    assert_eq!(steps, [Step::Read, Step::Redraw]);
    assert_eq!(s.received, 1);
    assert_eq!(s.state, State::Connected);
    assert!(device.sent.contains_key("Ann/A Title.epub"));
}

#[test]
fn a_book_that_cannot_be_put_there_is_told_or_not_counted() {
    let mut device = Fake::default();
    let mut s = session();
    s.connected().unwrap();
    says(&mut s, &mut device, op::SEND_BOOK, &book_args("a.epub", ""));
    let steps = s.saved().unwrap();
    let [Step::Import(coming)] = &steps[..] else {
        panic!("{steps:?}")
    };
    let steps = s
        .imported(&mut device, coming, Err(Placed::NotReplaced))
        .unwrap();
    assert_eq!(
        steps[0],
        Step::NotReplaced {
            title: "A Title".into()
        }
    );
    assert_eq!(s.received, 0);
    assert!(device.sent.is_empty(), "nothing was kept");
    // A failure of the library is not counted and not told.
    says(&mut s, &mut device, op::SEND_BOOK, &book_args("b.epub", ""));
    let steps = s.saved().unwrap();
    let [Step::Import(coming)] = &steps[..] else {
        panic!("{steps:?}")
    };
    let steps = s
        .imported(&mut device, coming, Err(Placed::Failed))
        .unwrap();
    assert_eq!(steps, [Step::Read, Step::Redraw]);
    assert_eq!(s.received, 0);
}

#[test]
fn bytes_for_a_path_that_leads_out_or_a_format_not_read_are_taken_and_dropped() {
    for lpath in ["../evil.epub", ".paperpaw/x.epub", "a.zip", "x.exe", ""] {
        let mut device = Fake::default();
        let mut s = session();
        s.connected().unwrap();
        let steps = says(&mut s, &mut device, op::SEND_BOOK, &book_args(lpath, ""));
        assert_eq!(
            steps[0],
            Step::ReadFile {
                len: 1234,
                dropped: true
            },
            "{lpath:?}"
        );
        assert_eq!(s.saved().unwrap(), [Step::RemoveDropped], "{lpath:?}");
        assert_eq!(s.dropped().unwrap(), [Step::Read, Step::Redraw]);
        assert!(device.sent.is_empty());
        assert_eq!(s.received, 0);
    }
}

#[test]
fn a_book_without_its_length_ends_the_connection() {
    let mut device = Fake::default();
    let mut s = session();
    s.connected().unwrap();
    let steps = says(
        &mut s,
        &mut device,
        op::SEND_BOOK,
        "{\"lpath\": \"a.epub\", \"thisBook\": 0}",
    );
    assert_eq!(steps[0], Step::Disconnect);
    assert_eq!(s.state, State::Ended(Ending::NotCalibre));
}

#[test]
fn only_books_calibre_sent_are_deleted() {
    let mut device = Fake::default().with_book("Ann/A.epub", "{\"uuid\": \"u-a\"}");
    let mut s = session();
    s.connected().unwrap();
    let steps = says(
        &mut s,
        &mut device,
        op::DELETE_BOOK,
        "{\"lpaths\": [\"Ann/A.epub\", \"Mine/Own.epub\", \"../x.epub\"]}",
    );
    let deletes: Vec<_> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Delete { path } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deletes, ["Ann/A.epub"], "{steps:?}");
    // OK first, then one answer for each path, with the uuid where it was kept.
    let replies = messages(&sent(&steps));
    let texts: Vec<_> = replies.iter().map(|(_, args)| args.as_str()).collect();
    assert_eq!(
        texts,
        [
            "{}",
            "{\"uuid\": \"u-a\"}",
            "{\"uuid\": \"\"}",
            "{\"uuid\": \"\"}"
        ]
    );
    assert!(device.sent.is_empty());
    assert_eq!(steps.last(), Some(&Step::Read));
}

#[test]
fn ejecting_a_wrong_password_and_errors_end_or_change_the_screen() {
    let mut device = Fake::default();
    let mut s = session();
    s.connected().unwrap();
    // A plain NOOP keeps the connection alive; one that counts or asks for
    // cached metadata is not answered.
    let steps = says(&mut s, &mut device, op::NOOP, "{}");
    assert_eq!(messages(&sent(&steps)).len(), 1);
    let steps = says(&mut s, &mut device, op::NOOP, "{\"count\": 3}");
    assert!(sent(&steps).is_empty());
    let steps = says(&mut s, &mut device, op::NOOP, "{\"ejecting\": true}");
    assert_eq!(messages(&sent(&steps)).len(), 1);
    assert_eq!(s.state, State::Ended(Ending::Ejected));
    // Calibre closing after it is not an error: the state stays.
    s.closed_by_calibre().unwrap();
    assert_eq!(s.state, State::Ended(Ending::Ejected));
    // The password is wrong or missing.
    let mut s = session();
    s.connected().unwrap();
    let steps = says(
        &mut s,
        &mut device,
        op::DISPLAY_MESSAGE,
        "{\"messageKind\": 1}",
    );
    assert_eq!(steps[0], Step::Disconnect);
    assert_eq!(s.state, State::Password);
    assert!(s.needs_password());
    // Calibre's own error words are shown.
    let mut s = session();
    s.connected().unwrap();
    let steps = says(&mut s, &mut device, op::ERROR, "{\"message\": \"Boom\"}");
    assert_eq!(steps[0], Step::Disconnect);
    assert_eq!(s.state, State::Ended(Ending::Error("Boom".into())));
    // PaperPaw sends no books.
    let mut s = session();
    s.connected().unwrap();
    let steps = says(&mut s, &mut device, op::GET_BOOK_METADATA, "{}");
    let (code, text) = &messages(&sent(&steps))[0];
    assert_eq!(*code, op::ERROR);
    assert!(text.contains("does not send books"));
}

#[test]
fn what_is_not_calibres_protocol_ends_the_connection() {
    let mut device = Fake::default();
    for hostile in ["x", "[", "0[", "99999999999[", "7[a, {}]", "9999999[0, {}]"] {
        let mut s = session();
        s.connected().unwrap();
        let steps = s.received(&mut device, hostile).unwrap();
        if matches!(s.state, State::Ended(Ending::NotCalibre)) {
            assert_eq!(steps[0], Step::Disconnect, "{hostile:?}");
        } else {
            // Waited for: a message not whole yet.
            assert_eq!(steps, [Step::Read], "{hostile:?}");
        }
    }
    // A message above the bound is refused whole.
    let mut s = Session::new(100, true);
    s.connected().unwrap();
    s.received(&mut device, "101[").unwrap();
    assert_eq!(s.state, State::Ended(Ending::NotCalibre));
    // Bytes that are not text.
    let mut s = session();
    s.not_text().unwrap();
    assert_eq!(s.state, State::Ended(Ending::NotCalibre));
}

#[test]
fn a_connection_that_ends_says_why() {
    let mut s = session();
    s.connected().unwrap();
    s.closed(Some("timed out")).unwrap();
    assert_eq!(s.state, State::Ended(Ending::Failed("timed out".into())));
    let mut s = session();
    s.connected().unwrap();
    s.closed(None).unwrap();
    assert_eq!(s.state, State::Ended(Ending::Closed));
    // An end already said is not said again, nor is a password asked for.
    s.closed(Some("late")).unwrap();
    assert_eq!(s.state, State::Ended(Ending::Closed));
    let mut s = session();
    s.state = State::Password;
    s.closed(None).unwrap();
    assert_eq!(s.state, State::Password);
    // The coming book is forgotten with the connection.
    let mut device = Fake::default();
    let mut s = session();
    says(&mut s, &mut device, op::SEND_BOOK, &book_args("a.epub", ""));
    s.closed(None).unwrap();
    assert_eq!(s.saved().unwrap(), [Step::RemoveDropped]);
    let _ = Coming {
        lpath: String::new(),
        path: String::new(),
        metadata: String::new(),
    };
}
