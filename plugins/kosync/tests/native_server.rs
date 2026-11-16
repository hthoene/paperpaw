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

#[test]
fn an_unconfirmed_offer_never_writes_a_library_position() {
    for answer in [None, Some(Answer::Cancelled), Some(Answer::Confirm(false))] {
        let world = World::new(true);
        {
            let plugin = world.load();
            assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
            let remote = br#"{"percentage":0.8,"device_id":"another","device":"Test device","progress":"/known"}"#;
            assert_eq!(world.complete(&plugin, 0, 200, remote).status, 0);
            if let Some(answer) = answer {
                let request = world.dialogs.last().unwrap().request;
                let event = world.dialogs.answer(request, &answer).unwrap();
                assert_eq!(plugin.event(tokens::ACCOUNT, &event).unwrap().status, 0);
            }
            assert_eq!(
                world.closed(&plugin, "first.epub", 100_000, 70_000).status,
                0
            );
        }
        assert!(
            world.library_ops.borrow().contains(&25),
            "real plugin queried the fingerprint"
        );
        assert!(
            !world.library_ops.borrow().contains(&14),
            "unaccepted remote position must not replace the saved reader position"
        );
    }
}

#[test]
fn a_full_document_slot_neither_sends_empty_pointers_nor_dismisses_go_there() {
    let world = World::new(true);
    world.host.service("core.document", 1, 0, |_, _| Err(-11));
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    assert_eq!(world.complete(&plugin, 0, 200, b"{}").status, 0);
    assert_eq!(
        world.closed(&plugin, "first.epub", 200_000, 70_000).status,
        0
    );
    assert_eq!(world.server.borrow().requests.len(), 1);
    use paperpaw_core::native_host::registrations::{EventActionRun, Record};
    use paperpaw_scripted_host::event_batch;
    let sync = event_batch(&Record::EventActionRun(EventActionRun {
        now_ms: 71_000,
        action: "kosync.now",
        flags: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(tokens::SYNC_NOW, &sync).unwrap().status, 0);
    let remote = br#"{"percentage":0.8,"device":"Other","progress":"/known"}"#;
    assert_eq!(world.complete(&plugin, 1, 200, remote).status, 0);
    let question = world.dialogs.last().unwrap();
    let answer = world
        .dialogs
        .answer(question.request, &Answer::Confirm(true))
        .unwrap();
    assert_eq!(plugin.event(tokens::ACCOUNT, &answer).unwrap().status, 0);
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    assert_eq!(world.complete(&plugin, 2, 200, remote).status, 0);
    assert_ne!(world.dialogs.last().unwrap().request, question.request);
    assert!(
        world
            .server
            .borrow()
            .requests
            .iter()
            .all(|r| r.method == "GET")
    );
}

#[test]
fn a_long_device_name_still_shows_a_short_single_line_offer() {
    let world = World::new(true);
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    let body = format!(r#"{{"percentage":0.8,"device":"{}"}}"#, "a\\n".repeat(1000));
    assert_eq!(world.complete(&plugin, 0, 200, body.as_bytes()).status, 0);
    let question = world.dialogs.last().unwrap();
    assert!(question.text.contains(&format!("{}…", "a".repeat(40))));
    assert!(!question.text.contains('\n'));
    assert!(question.text.len() < 500);
}

#[test]
fn a_batch_setting_event_without_started_keeps_the_account() {
    use paperpaw_core::native_host::registrations::{EventSettingChanged, Record};
    use paperpaw_scripted_host::event_batch;
    for (key, removed) in [("", false), ("kosync.server", true)] {
        let world = World::new(true);
        let plugin = world.load();
        let event = event_batch(&Record::EventSettingChanged(EventSettingChanged {
            now_ms: 0,
            key,
            old: &[],
        }))
        .unwrap();
        assert_eq!(
            plugin
                .event(tokens::SETTING_CHANGED, &event)
                .unwrap()
                .status,
            0
        );
        assert_eq!(world.server.borrow().key.is_none(), removed);
    }
}

#[test]
fn page_turns_between_pushes_do_not_probe_wifi_or_query_work() {
    use paperpaw_core::native_host::registrations::{EventPageTurned, Record};
    use paperpaw_scripted_host::event_batch;
    use std::{cell::RefCell, rc::Rc};
    let world = World::new(true);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let log = calls.clone();
    world.host.service("core.device", 1, 0, move |op, _| {
        log.borrow_mut().push(("device", op));
        Err(-1)
    });
    let log = calls.clone();
    world.host.service("core.work", 1, 0, move |op, _| {
        log.borrow_mut().push(("work", op));
        Ok(Vec::new())
    });
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    assert_eq!(world.complete(&plugin, 0, 200, b"{}").status, 0);
    for now_ms in [1000, 2000, 3000] {
        let event = event_batch(&Record::EventPageTurned(EventPageTurned {
            now_ms,
            section: 0,
            offset: 20,
            progress_ppm: 200_000,
            path: "first.epub",
            title: "Test",
            chapter: "",
            page: None,
            pages: None,
            insert: None,
            chapter_left: None,
        }))
        .unwrap();
        calls.borrow_mut().clear();
        assert_eq!(plugin.event(tokens::PAGE_TURNED, &event).unwrap().status, 0);
        println!("turn {now_ms}: {:?}", calls.borrow());
        if now_ms == 3000 {
            assert!(calls.borrow().is_empty());
        }
    }
}

fn turn(now_ms: u64) -> Vec<u8> {
    use paperpaw_core::native_host::registrations::{EventPageTurned, Record};
    paperpaw_scripted_host::event_batch(&Record::EventPageTurned(EventPageTurned {
        now_ms, section: 0, offset: 20, progress_ppm: 200_000,
        path: "first.epub", title: "Test", chapter: "", page: None,
        pages: None, insert: None, chapter_left: None,
    })).unwrap()
}

#[test]
fn a_refused_push_is_declared_again_without_querying_work_state() {
    use paperpaw_core::native_host::registrations::{EventWorkRefused, Record};
    use paperpaw_scripted_host::event_batch;
    use std::{cell::RefCell, rc::Rc};
    let world = World::new(true);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let log = calls.clone();
    world.host.service("core.work", 1, 0, move |op, _| {
        log.borrow_mut().push(op);
        Ok(Vec::new())
    });
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    assert_eq!(world.complete(&plugin, 0, 200, b"{}").status, 0);
    for now in [1000, 2000] {
        assert_eq!(plugin.event(tokens::PAGE_TURNED, &turn(now)).unwrap().status, 0);
    }
    assert_eq!(*calls.borrow(), [1]);
    let refused = event_batch(&Record::EventWorkRefused(EventWorkRefused {
        now_ms: 2500, token: tokens::WORK_DUE, reason: 13, key: tokens::PUSH_WORK,
    })).unwrap();
    assert_eq!(plugin.event(13, &refused).unwrap().status, 0);
    assert_eq!(plugin.event(tokens::PAGE_TURNED, &turn(3000)).unwrap().status, 0);
    assert_eq!(*calls.borrow(), [1, 1]);
}
