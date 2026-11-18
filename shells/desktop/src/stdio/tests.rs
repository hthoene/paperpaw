use super::*;
use paperpaw_core::sdk::json;
use paperpaw_platform::PointerPhase;

#[test]
fn requests_are_read_from_their_lines() {
    assert_eq!(
        parse(r#"{"op": "button", "name": "confirm", "pressed": true}"#),
        Ok(Request::Button {
            name: String::from("confirm"),
            pressed: true
        })
    );
    assert_eq!(
        parse(r#"{"op":"pointer","phase":"down","x":10,"y":20}"#),
        Ok(Request::Pointer {
            phase: PointerPhase::Down,
            x: 10,
            y: 20
        })
    );
    assert_eq!(
        parse(r#"{"op": "open", "screen": "wifi.phone", "params": {"a": "b", "n": 2}}"#),
        Ok(Request::Open {
            screen: String::from("wifi.phone"),
            params: vec![
                (String::from("a"), String::from("b")),
                (String::from("n"), String::from("2"))
            ]
        })
    );
    assert!(parse("not json").is_err());
    assert!(parse(r#"{"op": "fly"}"#).is_err());
    assert!(parse(r#"{"op": "button"}"#).is_err());
}

/// Lines are read up to their end, a long one said and passed over,
/// the next one read as it is.
#[test]
fn long_lines_are_passed_over_to_their_end() {
    let mut text = format!("{{\"op\": \"quit\"}}\r\n{}\nshort\n", "x".repeat(40)).into_bytes();
    // A byte that is no UTF-8.
    text.extend_from_slice(b"\xff\ntail");
    let mut input = std::io::BufReader::with_capacity(8, text.as_slice());
    let mut lines = Vec::new();
    while let Some(line) = read_line(&mut input, 20).unwrap() {
        lines.push(line);
    }
    assert_eq!(lines[0], Ok(String::from(r#"{"op": "quit"}"#)));
    assert!(
        lines[1]
            .as_ref()
            .is_err_and(|e| e.contains("longer than 20"))
    );
    assert_eq!(lines[2], Ok(String::from("short")));
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[4], Ok(String::from("tail")));
}

#[test]
fn base64_is_rfc_4648() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

/// A device started over the protocol says what it is, shows its home
/// screen in a frame, answers what it shows, and errors are said, not
/// fatal.
#[test]
fn a_device_is_driven_over_lines() {
    let _one = crate::one_device_at_a_time();
    let folder = std::env::temp_dir().join(format!("paperpaw-stdio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let (lines, received) = channel();
    let library = string(&folder.display().to_string());
    for line in [
        String::from(r#"{"op": "describe"}"#),
        format!(r#"{{"op": "start", "profile": "xteink-x4", "library": {library}}}"#),
        String::from(r#"{"op": "button", "name": "confirm", "pressed": true}"#),
        String::from(r#"{"op": "button", "name": "confirm", "pressed": false}"#),
        String::from(r#"{"op": "describe"}"#),
        String::from(r#"{"op": "pointer", "phase": "down", "x": 1, "y": 1}"#),
        String::from(r#"{"op": "shot"}"#),
        String::from(r#"{"op": "quit"}"#),
    ] {
        lines.send(Ok(line)).unwrap();
    }
    let mut events = Vec::new();
    let mut session = Session::new();
    serve(&mut session, &received, &mut |event| {
        events.push(event.to_string());
        Ok(())
    })
    .unwrap();
    let kinds: Vec<String> = events
        .iter()
        .map(|e| {
            json::text(&json::object(e), "event")
                .unwrap_or("?")
                .to_string()
        })
        .collect();
    assert_eq!(kinds[0], "error", "{events:?}");
    assert!(events[0].contains("no device started"));
    assert_eq!(kinds[1], "started");
    assert!(events[1].contains("\"width\": 480"));
    assert!(kinds.contains(&String::from("frame")));
    assert!(kinds.contains(&String::from("described")));
    // The X4 has no touch screen: said, and on it goes.
    assert!(events.iter().any(|e| e.contains("no touch screen")));
    let shot = events
        .iter()
        .find(|e| e.contains("\"event\": \"shot\""))
        .unwrap();
    assert!(shot.contains("\"png\": \"iVBOR"), "{shot}");
    assert!(folder.join(".paperpaw/plugins/setup/done").exists());
    let _ = std::fs::remove_dir_all(&folder);
}

/// Runs `lines` through the protocol on a device of the X4, its events
/// in order.
fn through(lines: &[String]) -> Vec<String> {
    let (sender, received) = channel();
    for line in lines {
        sender.send(Ok(line.clone())).unwrap();
    }
    sender.send(Ok(String::from(r#"{"op": "quit"}"#))).unwrap();
    let mut events = Vec::new();
    let mut session = Session::new();
    serve(&mut session, &received, &mut |event| {
        events.push(event.to_string());
        Ok(())
    })
    .unwrap();
    events
}

/// The event named `name`, the first.
fn event<'a>(events: &'a [String], name: &str) -> &'a str {
    let key = format!("\"event\": \"{name}\"");
    events
        .iter()
        .find(|e| e.contains(&key))
        .unwrap_or_else(|| panic!("no {name}: {events:?}"))
}

/// A wait lasts its time, at most ten minutes, while the requests that
/// come meanwhile are read: done after it, but `quit` or the end of
/// the input (the program gone) end it at once, and the process.
#[test]
fn waiting_ends_with_the_input() {
    let parsed = |line: &str| parse(line).unwrap();
    assert_eq!(
        parsed(r#"{"op": "wait", "ms": 1e12}"#),
        Request::Wait { ms: MAX_WAIT_MS }
    );
    assert_eq!(
        parsed(r#"{"op": "until", "text": "x", "timeout": 1e12}"#),
        Request::Until {
            text: Some("x".into()),
            screen: None,
            timeout: MAX_WAIT_MS
        }
    );
    // `then` sent `after` the lines, while they are waited for.
    let served = |lines: &[&str], after: u64, then: Option<&str>| {
        let (sender, received) = channel();
        for line in lines {
            sender.send(Ok((*line).to_string())).unwrap();
        }
        let then = then.map(str::to_string);
        let late = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(after));
            if let Some(line) = then {
                let _ = sender.send(Ok(line));
            }
            // Dropped here: the end of the input.
        });
        let start = std::time::Instant::now();
        let mut events = Vec::new();
        let mut session = Session::new();
        serve(&mut session, &received, &mut |event| {
            events.push(event.to_string());
            Ok(())
        })
        .unwrap();
        late.join().unwrap();
        (events, start.elapsed())
    };
    let ms = |event: &str| json::number(&json::object(event), "ms").unwrap();

    // Waited for in full, and what came meanwhile done after it.
    let (events, took) = served(
        &[r#"{"op": "wait", "ms": 300}"#, r#"{"op": "screens"}"#],
        600,
        Some(r#"{"op": "quit"}"#),
    );
    assert!(
        ms(&events[0]) >= 300.0 && took >= Duration::from_millis(300),
        "{events:?}"
    );
    assert!(events[1].contains("\"op\": \"screens\""), "{events:?}");
    // `quit`, or the program gone, ends a long wait at once.
    for then in [Some(r#"{"op": "quit"}"#), None] {
        let (events, took) = served(
            &[r#"{"op": "until", "text": "never shown", "timeout": 600000}"#],
            200,
            then,
        );
        assert!(took < Duration::from_secs(10), "{took:?}");
        assert!(events[0].contains("\"found\": false"), "{events:?}");
    }
}

/// A recorded answer's file is a file in the device's folder, of at
/// most 16 MB; never one outside it, through a link either, nor a
/// folder, pipe or device.
#[test]
fn answers_come_from_files_in_the_device_s_folder() {
    let folder = crate::private::dir("paperpaw-fixture").unwrap();
    let outside = crate::private::dir("paperpaw-fixture-outside").unwrap();
    std::fs::create_dir_all(folder.join("answers")).unwrap();
    std::fs::write(folder.join("answers/feed.xml"), b"<feed/>").unwrap();
    std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
    let big = std::fs::File::create(folder.join("big.bin")).unwrap();
    big.set_len(MAX_FIXTURE + 1).unwrap();
    drop(big);
    let read = |file: &Path| fixture_file(&folder, file);
    assert_eq!(read(Path::new("answers/feed.xml")).unwrap(), b"<feed/>");
    assert_eq!(read(&folder.join("answers/feed.xml")).unwrap(), b"<feed/>");
    assert!(read(Path::new("../secret.txt")).is_err());
    assert!(
        read(&outside.join("secret.txt"))
            .unwrap_err()
            .contains("not in the folder")
    );
    assert!(
        read(Path::new("answers"))
            .unwrap_err()
            .contains("not a file")
    );
    assert!(
        read(Path::new("big.bin"))
            .unwrap_err()
            .contains("larger than 16 MB")
    );
    assert!(read(Path::new("missing.txt")).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("secret.txt"), folder.join("link.txt")).unwrap();
        assert!(
            read(Path::new("link.txt"))
                .unwrap_err()
                .contains("not in the folder")
        );
        assert!(read(Path::new("/dev/zero")).is_err());
    }

    // Over the lines: read where the device was started, none before.
    let _one = crate::one_device_at_a_time();
    let library = string(&folder.display().to_string());
    let events = through(&[
        String::from(
            r#"{"op": "fixture", "url": "https://example.org/", "file": "answers/feed.xml"}"#,
        ),
        format!(r#"{{"op": "start", "profile": "xteink-x4", "library": {library}}}"#),
        String::from(
            r#"{"op": "fixture", "url": "https://example.org/", "file": "answers/feed.xml"}"#,
        ),
        String::from(r#"{"op": "fixture", "url": "https://example.org/", "file": "big.bin"}"#),
    ]);
    let fixture: Vec<&String> = events.iter().filter(|e| e.contains("fixture")).collect();
    assert_eq!(fixture.len(), 3, "{events:?}");
    assert!(fixture[0].contains("start one first"), "{fixture:?}");
    assert!(fixture[1].contains("\"fixtures\": 1"), "{fixture:?}");
    assert!(fixture[2].contains("larger than 16 MB"), "{fixture:?}");
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&outside);
}

/// Version 2's requests, as x4-remote has them: slots, settings,
/// events, the screens and actions there are, waiting for what shows.
#[test]
#[cfg_attr(
    all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"),
    ignore = "the native settings plugin is not registered on the desktop yet (it needs core.secrets and event 75/76, #916); this test opens its page: enable it with that wiring"
)]
fn the_device_is_asked_and_waited_for_over_lines() {
    let _one = crate::one_device_at_a_time();
    let folder = crate::private::dir("paperpaw-stdio2").unwrap();
    let library = string(&folder.display().to_string());
    let events = through(&[
        format!(r#"{{"op": "start", "profile": "xteink-x4", "library": {library}}}"#),
        String::from(r#"{"op": "screens"}"#),
        String::from(r#"{"op": "actions", "prefix": "core."}"#),
        String::from(r#"{"op": "slot", "slot": "core.settings"}"#),
        String::from(r#"{"op": "until", "text": "Display", "timeout": 5000}"#),
        String::from(r#"{"op": "expect", "text": "Display"}"#),
        String::from(r#"{"op": "expect", "text": "nothing says this", "not": true}"#),
        String::from(r#"{"op": "describe"}"#),
        String::from(r#"{"op": "set", "id": "core.night-mode", "value": "on"}"#),
        String::from(r#"{"op": "setting", "id": "core.night-mode"}"#),
        String::from(r#"{"op": "set", "id": "core.no-such-thing", "value": "on"}"#),
        String::from(r#"{"op": "emit", "event": "core.library-changed"}"#),
        String::from(r#"{"op": "wait", "ms": 100}"#),
        String::from(r#"{"op": "until", "text": "nothing says this", "timeout": 200}"#),
        String::from(r#"{"op": "network", "mode": "offline"}"#),
        String::from(r#"{"op": "fixture", "url": "https://example.org/*", "body": "hi"}"#),
        String::from(r#"{"op": "network", "mode": "real"}"#),
    ]);
    let screens = event(&events, "screens");
    assert!(
        screens.contains("\"known\": [\"") && screens.contains("\"shown\": \""),
        "{screens}"
    );
    assert!(event(&events, "actions").contains("\"id\": \"core."));
    assert!(
        event(&events, "until").contains("\"found\": true"),
        "{events:?}"
    );
    let expects: Vec<&String> = events
        .iter()
        .filter(|e| e.contains("\"event\": \"expect\""))
        .collect();
    assert!(
        expects.len() == 2 && expects.iter().all(|e| e.contains("\"ok\": true")),
        "{expects:?}"
    );
    let described = event(&events, "described");
    assert!(described.contains("\"screen\": \""), "{described}");
    assert!(event(&events, "set").contains("core.night-mode"));
    assert!(
        event(&events, "setting").contains("\"value\": \"on\""),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| e.contains("core.no-such-thing") && e.contains("\"op\": \"set\""))
    );
    // The quit that came meanwhile ended the wait (see
    // `waiting_ends_with_the_input`).
    assert!(event(&events, "waited").contains("\"ms\": "));
    let untils: Vec<&String> = events
        .iter()
        .filter(|e| e.contains("\"event\": \"until\""))
        .collect();
    assert!(
        untils.last().unwrap().contains("\"found\": false"),
        "{untils:?}"
    );
    assert!(event(&events, "network").contains("\"mode\": \"offline\""));
    assert!(event(&events, "fixture").contains("\"fixtures\": 1"));
    let _ = std::fs::remove_dir_all(&folder);
}

/// Recorded answers answer what they were recorded for (the last one
/// recorded first, `*` a start), others fail, as all do offline.
#[test]
fn recorded_answers_answer_their_requests() {
    let call = |method: &str, url: &str| paperpaw_core::HttpCall {
        id: 7,
        method: String::from(method),
        url: String::from(url),
        headers: Vec::new(),
        body: None,
        body_from: None,
        max_response: 1 << 20,
        timeout_ms: 1000,
        want_headers: Vec::new(),
        save_to: None,
        progress: false,
    };
    let fixture = |url: &str, method: Option<&str>, body: &str| Fixture {
        url: String::from(url),
        method: method.map(String::from),
        status: 200,
        body: body.as_bytes().to_vec(),
        content_type: Some(String::from("text/plain")),
    };
    let state = std::rc::Rc::new(std::cell::RefCell::new((
        Some(Network::Recorded),
        vec![
            fixture("https://example.org/*", None, "any"),
            fixture("https://example.org/feed", Some("GET"), "feed"),
        ],
    )));
    let mut answers = Answers(state.clone());
    use crate::http::Route as _;
    let answer = |answers: &mut Answers, method, url| answers.start(call(method, url)).unwrap();
    let feed = answer(&mut answers, "GET", "https://example.org/feed");
    assert_eq!(
        (feed.id, feed.status, feed.body.as_slice()),
        (7, 200, &b"feed"[..])
    );
    assert_eq!(
        answer(&mut answers, "POST", "https://example.org/feed").body,
        b"any"
    );
    assert_eq!(answer(&mut answers, "GET", "https://other.org/").status, 0);
    state.borrow_mut().0 = Some(Network::Offline);
    assert_eq!(
        answer(&mut answers, "GET", "https://example.org/feed").status,
        0
    );
}

#[test]
fn ready_says_the_protocol_and_profiles() {
    let ready = Session::ready();
    let members = json::object(&ready);
    assert_eq!(json::text(&members, "event"), Some("ready"));
    assert_eq!(
        json::number(&members, "protocol"),
        Some(f64::from(PROTOCOL))
    );
    assert!(ready.contains("\"xteink-x4-pro\""));
    // Built in a checkout: its commit, as Git writes it.
    let commit = json::text(&members, "commit").unwrap_or_default();
    assert!(
        commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "{ready}"
    );
    assert!(ready.contains("\"release\": true") || ready.contains("\"release\": false"));
}

/// `"wifi": "online"` starts the device in the workshop's network, as
/// `--wifi online` does; anything else but `off` is an error.
#[test]
fn a_device_starts_online_where_asked() {
    assert!(parse(r#"{"op": "start", "profile": "p", "library": "l", "wifi": "maybe"}"#).is_err());
    let _one = crate::one_device_at_a_time();
    let folder = crate::private::dir("paperpaw-stdio-online").unwrap();
    let library = string(&folder.display().to_string());
    let events = through(&[
        format!(
            r#"{{"op": "start", "profile": "xteink-x4", "library": {library}, "wifi": "online"}}"#
        ),
        String::from(r#"{"op": "open", "screen": "wifi.page"}"#),
        String::from(r#"{"op": "describe"}"#),
    ]);
    let described = event(&events, "described");
    assert!(
        described.contains("Connected to Workshop Wi-Fi"),
        "{described}"
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// What Battery's log on the device's card says at the end of a session
/// that `through` ran in `folder`: its lines' kinds, in order.
fn battery_log_kinds(folder: &std::path::Path) -> Vec<String> {
    let log = std::fs::read_to_string(folder.join(".paperpaw/plugins/battery/log.csv"))
        .unwrap_or_default();
    log.lines()
        .filter_map(|line| line.split(',').next())
        .map(String::from)
        .collect()
}

/// Gate V2 on the emulator: closing it (`quit`, the window's close) gives
/// plugins their moment to save (`core.suspend`) first, and what they wrote
/// is on the card when it returns; a device that is asleep already has
/// saved at its sleep and is not told again.
#[test]
#[ignore = "re-enable at Battery activation (#228): the gate reads Battery's log, which the app does not carry until the native plugin runs"]
fn quitting_gives_plugins_their_moment_to_save_first() {
    let _one = crate::one_device_at_a_time();
    for (name, asleep) in [
        ("paperpaw-stdio-quit", false),
        ("paperpaw-stdio-asleep", true),
    ] {
        let folder = crate::private::dir(name).unwrap();
        let library = string(&folder.display().to_string());
        let mut lines = vec![format!(
            r#"{{"op": "start", "profile": "xteink-x4", "library": {library}}}"#
        )];
        if asleep {
            for pressed in ["true", "false"] {
                lines.push(format!(
                    r#"{{"op": "button", "name": "power", "pressed": {pressed}}}"#
                ));
            }
        }
        through(&lines);
        let kinds = battery_log_kinds(&folder);
        assert_eq!(
            kinds.iter().filter(|kind| *kind == "end").count(),
            1,
            "one end of the time awake, written before quit returned (asleep: {asleep}): {kinds:?}"
        );
        assert_eq!(kinds.last().map(String::as_str), Some("end"), "{kinds:?}");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
