//! Requests, servers, files, the clipboard and secrets between plugins and the shell.

use super::*;

#[test]
fn http_requests_go_to_the_shell_and_answers_come_back() {
    let request = |id: &str| {
        paperpaw_sdk::HttpRequest::get("https://example.org/x", Id::new_static("ov.reply"))
            .header("accept", "text/plain")
            .with(paperpaw_sdk::Params::new().with("id", String::from(id)))
    };
    // With network: the shell gets it and answers.
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .network(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.request(request("a"));
    let calls = kernel.take_requests();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].method.as_str(), calls[0].url.as_str()),
        ("GET", "https://example.org/x")
    );
    assert!(kernel.take_requests().is_empty());
    kernel.http_response(
        Instant(5),
        crate::HttpResponse {
            id: calls[0].id,
            status: 200,
            body: b"ok".to_vec(),
            etag: None,
            ..Default::default()
        },
    );
    assert!(log.borrow().contains(&String::from("event ov.reply a")));
    // Without: it fails at once.
    let log = self::log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.request(request("b"));
    kernel.run_commands();
    assert!(kernel.take_requests().is_empty());
    assert!(log.borrow().contains(&String::from("event ov.reply b")));
}

#[test]
fn plugins_serve_through_the_shell() {
    use paperpaw_sdk::server_params as p;
    let log = log();
    let mut kernel = serving_kernel(&log);
    let events = Id::new_static("ov.reply");
    kernel.output.commands.push(Ask::serve(
        Id::new_static("ov.server"),
        8080,
        events.clone(),
    ));
    kernel.run_commands();
    assert!(kernel.serving());
    let calls = kernel.take_server_calls();
    let crate::ServerCall::Listen { server, port } = calls[0] else {
        panic!("{calls:?}");
    };
    assert_eq!(port, 8080);
    let path = a_put_request_comes_in(&mut kernel, &log, server);
    responses_go_out(&mut kernel, &path);
    kernel
        .output
        .commands
        .push(Ask::StopServing(Id::new_static("ov.server")));
    kernel.run_commands();
    assert!(!kernel.serving());
    assert!(matches!(
        kernel.take_server_calls()[0],
        crate::ServerCall::Stop { .. }
    ));
    // A server stops with the screen it was started on.
    kernel.output.commands.push(Ask::OpenScreen(
        Id::new_static("ov.base"),
        paperpaw_sdk::Params::new().with("n", "2"),
    ));
    kernel.run_commands();
    kernel.output.commands.push(Ask::serve(
        Id::new_static("ov.server"),
        8080,
        events.clone(),
    ));
    kernel.run_commands();
    assert!(kernel.serving());
    kernel.take_server_calls();
    kernel.output.commands.push(Ask::Back);
    kernel.run_commands();
    assert!(!kernel.serving());
    assert!(matches!(
        kernel.take_server_calls()[0],
        crate::ServerCall::Stop { .. }
    ));
    let _ = p::KIND;
}

/// A started kernel with the overlay plugin and the network on.
fn serving_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .storage(paperpaw_sdk::MemoryStorage::new())
        .network(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

/// A PUT request with its body in two parts reaches the plugin once the
/// body is complete. Returns the path of the kept body.
fn a_put_request_comes_in(kernel: &mut Kernel, log: &Log, server: u32) -> String {
    kernel.server_event(
        Instant(1),
        crate::ServerEvent::Request {
            server,
            request: 7,
            method: String::from("PUT"),
            target: String::from("/up?name=a"),
            body: true,
        },
    );
    // The plugin hears of it once the body is complete.
    let told = |log: &Log| log.borrow().iter().any(|l| l.starts_with("event ov.reply"));
    assert!(!told(log));
    for (data, last) in [(&b"da"[..], false), (&b"ta"[..], true)] {
        kernel.server_event(
            Instant(1),
            crate::ServerEvent::Body {
                request: 7,
                data: data.to_vec(),
                last,
            },
        );
    }
    assert!(told(log));
    let path = String::from(".paperpaw/tmp/request-7");
    let storage = kernel.shared.storage.as_ref();
    assert_eq!(
        paperpaw_sdk::read_file(storage, &path, 64).unwrap(),
        b"data"
    );
    path
}

/// A response with its body in memory, and one with a file the shell
/// reads in parts. The body of the request is gone after its response.
fn responses_go_out(kernel: &mut Kernel, path: &str) {
    kernel
        .output
        .commands
        .push(Ask::respond(paperpaw_sdk::ServerResponse::new(
            7,
            200,
            "text/plain",
            b"ok".to_vec(),
        )));
    // A file goes in parts, read by the shell.
    let mut response = paperpaw_sdk::ServerResponse::new(8, 200, "text/plain", Vec::new());
    response = response.body_from(String::from("served.txt"));
    kernel
        .shared
        .storage
        .write("served.txt", b"0123456789")
        .unwrap();
    kernel.output.commands.push(Ask::respond(response));
    kernel.run_commands();
    let calls = kernel.take_server_calls();
    assert!(
        matches!(&calls[0], crate::ServerCall::Respond { request: 7, status: 200, body, .. } if body == b"ok")
    );
    let crate::ServerCall::Respond {
        file: Some(file), ..
    } = &calls[1]
    else {
        panic!("{calls:?}");
    };
    assert_eq!(file.size, 10);
    let mut part = [0u8; 4];
    assert_eq!(kernel.read_served(&file.path, 8, &mut part).unwrap(), 2);
    assert_eq!(&part[..2], b"89");
    assert!(
        !kernel.shared.storage.exists(path),
        "the body is cleaned up"
    );
}

#[test]
fn files_go_up_and_come_down() {
    let storage = paperpaw_sdk::MemoryStorage::new().with("up.bin", b"\0\x01data".to_vec());
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .network(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let reply = Id::new_static("core.test-reply");
    kernel.request(
        paperpaw_sdk::HttpRequest::new("PUT", "https://example.org/f", reply.clone())
            .body_from("up.bin")
            .save_to("down.bin"),
    );
    let calls = kernel.take_requests();
    // The shell reads the file to send, so it never needs to fit in memory.
    assert_eq!(calls[0].body, None);
    assert_eq!(calls[0].body_from.as_deref(), Some("up.bin"));
    assert_eq!(calls[0].max_response, crate::MAX_FILE);
    assert_eq!(calls[0].save_to.as_deref(), Some("down.bin"));
    kernel.http_response(
        Instant(1),
        crate::HttpResponse {
            id: calls[0].id,
            status: 200,
            body: b"\xffbinary".to_vec(),
            etag: Some(String::from("\"abc\"")),
            content_type: Some(String::from("application/epub+zip")),
            url: Some(String::from("https://example.org/g")),
            saved: false,
            ..crate::HttpResponse::default()
        },
    );
    let storage = kernel.shared.storage.as_ref();
    assert_eq!(
        paperpaw_sdk::read_file(storage, "down.bin", 64).unwrap(),
        b"\xffbinary"
    );
    // Written by the shell itself as it came: left as it is.
    kernel.request(
        paperpaw_sdk::HttpRequest::get("http://example.org/k", reply.clone()).save_to("kept.bin"),
    );
    let calls = kernel.take_requests();
    kernel.storage().write("kept.bin", b"streamed").unwrap();
    kernel.http_response(
        Instant(2),
        crate::HttpResponse {
            id: calls[0].id,
            status: 200,
            saved: true,
            ..Default::default()
        },
    );
    assert_eq!(
        paperpaw_sdk::read_file(kernel.storage(), "kept.bin", 64).unwrap(),
        b"streamed"
    );
    // A file that is not there fails at once.
    kernel.request(
        paperpaw_sdk::HttpRequest::new("PUT", "https://example.org/f", reply)
            .body_from("missing.bin"),
    );
    assert!(kernel.take_requests().is_empty());
}

#[test]
fn copied_text_reaches_plugins_and_the_shell() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(!kernel.shared.place(Instant(0)).clipboard());
    let read = |id: &str| {
        Ask::ReadClipboard(
            Id::new_static("ov.reply"),
            paperpaw_sdk::Params::new().with("id", String::from(id)),
        )
    };
    kernel
        .output
        .commands
        .extend([Ask::SetClipboard(String::from("Alice")), read("a")]);
    kernel.run_commands();
    assert!(kernel.shared.place(Instant(0)).clipboard());
    assert_eq!(kernel.take_io(), [Io::Copy(String::from("Alice"))]);
    assert_eq!(kernel.take_io(), []);
    // Without the system's clipboard, the answer comes at once.
    assert!(log.borrow().contains(&String::from("event ov.reply a")));
    // Long text is cut at a character boundary.
    let long = "ä".repeat(paperpaw_sdk::MAX_CLIPBOARD);
    assert_eq!(
        crate::commands::clipped(&long).len(),
        paperpaw_sdk::MAX_CLIPBOARD
    );

    // With it: the shell reads it when asked.
    let log = self::log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .system_clipboard(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.set_clipboard_available(true);
    assert!(kernel.shared.place(Instant(0)).clipboard());
    kernel.output.commands.push(read("b"));
    kernel.run_commands();
    // Asked once, however many wait.
    assert_eq!(kernel.take_io(), [Io::ReadClipboard]);
    kernel.output.commands.push(read("c"));
    kernel.run_commands();
    assert_eq!(kernel.take_io(), []);
    assert!(!log.borrow().contains(&String::from("event ov.reply b")));
    kernel.clipboard_text(Instant(1), Some(String::from("from another app")));
    assert_eq!(kernel.take_io(), []);
    assert!(log.borrow().contains(&String::from("event ov.reply c")));
    assert!(log.borrow().contains(&String::from("event ov.reply b")));
}

#[test]
fn secrets_are_kept_apart_and_read_at_start() {
    use paperpaw_sdk::secrets::{FILE, SecretChange};
    let mut kernel = Kernel::builder(DEVICE)
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
        .output
        .commands
        .push(Ask::set_secret(SecretChange::keep(
            "a.key", "a.label", "hunter2",
        )));
    kernel.run_commands();
    // Its plugin sees it; another, or the kernel's own code, not.
    assert_eq!(kernel.shared.place(Instant(0)).secrets().get("a.key"), None);
    {
        let _other = kernel.shared.meter.enter("b");
        assert_eq!(kernel.shared.place(Instant(0)).secrets().get("a.key"), None);
        assert_eq!(
            kernel.shared.place(Instant(0)).secrets().listed().count(),
            1
        );
    }
    {
        let _own = kernel.shared.meter.enter("a");
        assert_eq!(
            kernel.shared.place(Instant(0)).secrets().get("a.key"),
            Some("hunter2")
        );
    }
    let data = paperpaw_sdk::read_file(kernel.shared.storage.as_ref(), FILE, 4096).unwrap();
    assert!(!String::from_utf8_lossy(&data).contains("hunter2"));
    // Read again at the next start.
    let mut kernel = Kernel::builder(DEVICE)
        .storage(paperpaw_sdk::MemoryStorage::new().with(FILE, data))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.shared.secrets.get("a.key"), Some("hunter2"));
    // The last one removed, the file goes.
    kernel
        .output
        .commands
        .push(Ask::set_secret(SecretChange::remove("a.key")));
    kernel.run_commands();
    assert!(kernel.shared.secrets.is_empty());
    assert!(!kernel.shared.storage.exists(FILE));
}
