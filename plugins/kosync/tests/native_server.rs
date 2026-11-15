//! The real native entry against an in-process fake sync service.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
mod common;
use common::{FINGERPRINT, World};
use paperpaw_plugin_kosync::declarations as tokens;
use paperpaw_scripted_host::Answer;

#[test]
fn a_book_fetches_by_fingerprint_and_close_pushes_after_the_server_is_known() {
    let world = World::new(true);
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    {
        let server = world.server.borrow();
        assert_eq!(server.requests.len(), 1);
        assert_eq!(server.requests[0].method, "GET");
        assert!(server.requests[0].url.ends_with(FINGERPRINT));
        assert!(server.requests[0].authenticated);
    }
    assert_eq!(world.complete(&plugin, 0, 200, b"{}").status, 0);
    assert_eq!(
        world.closed(&plugin, "first.epub", 200_000, 70_000).status,
        0
    );
    let server = world.server.borrow();
    assert_eq!(server.requests.len(), 2);
    let push = &server.requests[1];
    assert_eq!(push.method, "PUT");
    let body = std::str::from_utf8(&push.body).unwrap();
    assert!(body.contains(FINGERPRINT));
    assert!(body.contains("\"percentage\":0.2000"));
}

#[test]
fn a_further_remote_position_is_offered_and_held_until_stay_is_chosen() {
    let world = World::new(true);
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    let remote =
        br#"{"percentage":0.8,"device_id":"another","device":"Test device","progress":"/known"}"#;
    assert_eq!(world.complete(&plugin, 0, 200, remote).status, 0);
    let question = world.dialogs.last().unwrap();
    assert!(question.choices.contains("Go there"));
    assert_eq!(
        world.closed(&plugin, "first.epub", 200_000, 70_000).status,
        0
    );
    assert_eq!(
        world.server.borrow().requests.len(),
        1,
        "offer must prevent overwriting remote progress"
    );
    let answer = world
        .dialogs
        .answer(question.request, &Answer::Confirm(false))
        .unwrap();
    assert_eq!(plugin.event(tokens::ACCOUNT, &answer).unwrap().status, 0);
    assert_eq!(world.server.borrow().requests.last().unwrap().method, "PUT");
}

#[test]
fn bad_server_answers_do_not_panic_or_overwrite_a_remote_position() {
    for body in [
        &b"not JSON"[..],
        b"{",
        b"[]",
        br#"{"percentage":1e99999}"#,
        br#"{"percentage":"NaN"}"#,
        br#"{"percentage":-2}"#,
        br#"{"percentage":true}"#,
    ] {
        let world = World::new(true);
        let plugin = world.load();
        assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
        assert_eq!(world.complete(&plugin, 0, 200, body).status, 0);
        assert!(world.dialogs.questions().is_empty());
        assert_eq!(
            world.closed(&plugin, "first.epub", 200_000, 70_000).status,
            0
        );
        assert!(
            world
                .server
                .borrow()
                .requests
                .iter()
                .all(|r| r.method != "PUT")
        );
    }
}

#[test]
fn signed_out_accounts_never_send_book_requests() {
    let world = World::new(false);
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    assert_eq!(
        world.closed(&plugin, "first.epub", 200_000, 70_000).status,
        0
    );
    assert!(world.server.borrow().requests.is_empty());
}

#[test]
fn a_truncated_body_is_not_mistaken_for_a_complete_unknown_position() {
    let world = World::new(true);
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    world.server.borrow_mut().limit = Some(2);
    assert_eq!(world.complete(&plugin, 0, 200, b"{}missing").status, 0);
    assert_eq!(
        world.closed(&plugin, "first.epub", 200_000, 70_000).status,
        0
    );
    assert!(
        world
            .server
            .borrow()
            .requests
            .iter()
            .all(|r| r.method != "PUT")
    );
}
