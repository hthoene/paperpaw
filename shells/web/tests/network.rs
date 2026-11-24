//! Plugins' requests in the browser's emulator: answered from the page's
//! recorded answers, the same each time and without a network; those none
//! answers fail as without one, or go to the page to fetch where it allows
//! that; each told to the page.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_web::emulator::Emulator;
use paperpaw_web::network::Fixture;

/// The plugin library's list for Android, with one theme.
const LIST: &str = r#"{"format": 2, "plugins": [{
  "id": "airy-theme", "name": "Airy Theme", "summary": "Light and calm.",
  "group": "look", "author": "Someone", "licence": "MIT", "kind": "data",
  "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 1234}}]}"#;

/// A Boox: its network is the system's (on an X4 profile PaperPaw's own
/// radio is joined first, as on the device).
fn started() -> Emulator {
    let mut boox = Emulator::new("boox-go-7", "en", false).unwrap();
    boox.start().unwrap();
    boox.advance(5_000);
    boox
}

#[test]
fn requests_get_recorded_answers_or_fail_or_go_to_the_page() {
    // Recorded: the store's list answered, the request told.
    let mut boox = started();
    boox.set_fixtures(vec![Fixture {
        method: Some(String::from("GET")),
        url: String::from("https://paperpaw.org/api/v1/plugins.json*"),
        status: 200,
        headers: vec![(
            String::from("content-type"),
            String::from("application/json"),
        )],
        body: std::rc::Rc::from(LIST.as_bytes()),
        delay_ms: 300,
    }]);
    boox.command("run store.open", 6_000).unwrap();
    boox.advance(6_100);
    let told = boox.requests();
    let list_url = format!(
        r#""url":"https://paperpaw.org/api/v1/plugins.json?device=android&lang=en&abi=7.{}&order=fair""#,
        paperpaw_native::host_glue::MINOR
    );
    assert!(told.contains(&list_url), "{told}");
    assert!(
        told.contains(r#""source":"fixture","status":200"#),
        "{told}"
    );
    // The answer due in its delay is something to wake for: the page
    // comes back then, nothing else needed.
    let next = boox.advance(6_150).unwrap();
    assert!(next <= 150, "{next} ms");
    // A snapshot meanwhile has its own network: it takes no answer of
    // this device's.
    boox.snapshot(None, Some(true), false).unwrap();
    boox.advance(6_150 + next);
    let shown = boox.command("describe", 6_400).unwrap();
    assert!(shown.contains("Look"), "{shown}");
    assert!(boox.requests() == "[]" || !boox.requests().contains("plugins.json?"));
    drop(boox);

    // None recorded: failed as without a network, told as such.
    let mut boox = started();
    boox.command("run store.open", 6_000).unwrap();
    boox.advance(6_100);
    let told = boox.requests();
    assert!(told.contains(r#""source":"none","status":0"#), "{told}");
    drop(boox);

    // Real fetches allowed: the page fetches and hands the answer back.
    let mut boox = started();
    boox.set_real_network(true);
    boox.command("run store.open", 6_000).unwrap();
    boox.advance(6_100);
    assert!(
        boox.requests()
            .contains(r#""source":"fetch","status":null"#)
    );
    let outbox = boox.outbox();
    assert!(outbox.contains(r#""method":"GET""#), "{outbox}");
    let id: u32 = outbox
        .split(r#""id":"#)
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|id| id.parse().ok())
        .unwrap();
    assert_eq!(boox.outbox(), "[]", "handed over once");
    boox.fetched(
        id,
        200,
        "content-type: application/json",
        LIST.as_bytes().to_vec(),
        "",
        6_500,
    )
    .unwrap();
    assert!(
        boox.fetched(id, 200, "", Vec::new(), "", 6_600).is_err(),
        "answered once"
    );
    boox.advance(8_000);
    let shown = boox.command("describe", 9_000).unwrap();
    assert!(shown.contains("Look"), "{shown}");
}
