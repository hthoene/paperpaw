//! The scripted `core.ui`: asking, the host's request ids, the request text,
//! the answers and what a secret leaves behind.
#![allow(clippy::unwrap_used, reason = "fixture failures fail the test")]
mod common;
use common::{call, entry};
use paperpaw_core::native_host::registrations::{
    RawRecord, Record, ServiceUiAsk, ServiceUiRequest, ServiceUiText, ServiceUiTextAt,
    records_encode,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE};
use paperpaw_scripted_host::{Answer, ScriptedHost};

fn batch(record: &Record<'_>) -> Vec<u8> {
    let mut bytes = vec![0_u8; 8192];
    let len = records_encode(core::slice::from_ref(record), &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}

fn ask(kind: u32, flags: u32, title: &str, choices: &str) -> Vec<u8> {
    call(
        "core.ui",
        8,
        &batch(&Record::ServiceUiAsk(ServiceUiAsk {
            kind,
            flags,
            slot: "",
            title,
            text: "q",
            choices,
        })),
    )
}

fn request_of(reply: &[u8]) -> u32 {
    RawRecord::one(reply)
        .unwrap()
        .decode::<ServiceUiRequest>()
        .unwrap()
        .request
}

fn write(request: u32, offset: u32, data: &str, last: bool) -> Vec<u8> {
    call(
        "core.ui",
        9,
        &batch(&Record::ServiceUiText(ServiceUiText {
            text: request,
            offset,
            total: 0,
            flags: u32::from(last),
            data,
        })),
    )
}

fn read(request: u32, offset: u32) -> Vec<u8> {
    call(
        "core.ui",
        10,
        &batch(&Record::ServiceUiTextAt(ServiceUiTextAt {
            text: request,
            offset,
        })),
    )
}

#[test]
fn asking_gives_the_asker_its_request_id_and_the_test_sees_the_question() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    let done = plugin.event(0, &ask(2, 0, "Reset?", ""), 64).unwrap();
    assert!(done.status > 0);
    let request = request_of(&done.out);
    assert_eq!(request, 1);
    let question = dialogs.last().unwrap();
    assert_eq!(
        (question.request, question.kind, question.title.as_str()),
        (1, 2, "Reset?")
    );
    // A second ask by the same asker cancels the open one; the cancel event is due.
    let second = request_of(&plugin.event(0, &ask(1, 0, "Pick", "a\nb"), 64).unwrap().out);
    assert_eq!(second, 2);
    assert!(dialogs.answer(1, &Answer::Confirm(true)).is_err());
    assert!(dialogs.answer(1, &Answer::Cancelled).is_ok());
    // A kind the host does not know is invalid.
    assert_eq!(
        plugin.event(0, &ask(4, 0, "x", ""), 64).unwrap().status,
        ERR_INVALID
    );
}

#[test]
fn answers_by_kind_and_the_event_the_asker_gets() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    let id = request_of(&plugin.event(0, &ask(1, 0, "Pick", "a\nb"), 64).unwrap().out);
    // A choice past the list, and text for a choice question, are refused.
    assert_eq!(dialogs.answer(id, &Answer::Choice(2)), Err(ERR_INVALID));
    assert_eq!(
        dialogs.answer(id, &Answer::Text("x".into())),
        Err(ERR_INVALID)
    );
    let event = dialogs.answer(id, &Answer::Choice(1)).unwrap();
    let Record::EventAnswered(answered) =
        paperpaw_core::native_host::registrations::records_decode(&event)
            .unwrap()
            .iter()
            .next()
            .unwrap()
    else {
        panic!("an Answered record")
    };
    assert_eq!(
        (
            answered.request,
            answered.kind,
            answered.outcome,
            answered.choice
        ),
        (id, 1, 0, 1)
    );
    // The first final answer wins.
    assert_eq!(dialogs.answer(id, &Answer::Choice(0)), Err(ERR_STATE));
    // Confirm: choice 1 is yes.
    let confirm = request_of(&plugin.event(0, &ask(2, 0, "ok?", ""), 64).unwrap().out);
    let yes = dialogs.answer(confirm, &Answer::Confirm(true)).unwrap();
    assert!(!yes.is_empty());
}

#[test]
fn the_request_text_is_written_in_parts_and_read_back_after_the_answer() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    let id = request_of(&plugin.event(0, &ask(0, 0, "Name", ""), 64).unwrap().out);
    assert_eq!(
        plugin
            .event(0, &write(id, 0, "ab", false), 8)
            .unwrap()
            .status,
        0
    );
    // Appending at the wrong offset is invalid; the text is uncommitted until `last`.
    assert_eq!(
        plugin.event(0, &write(id, 5, "c", true), 8).unwrap().status,
        ERR_INVALID
    );
    assert_eq!(dialogs.initial_text(id), None);
    assert_eq!(
        plugin
            .event(0, &write(id, 2, "cd", true), 8)
            .unwrap()
            .status,
        0
    );
    assert_eq!(dialogs.initial_text(id).as_deref(), Some("abcd"));
    // The asker reads only after the dialog answered.
    assert_eq!(plugin.event(0, &read(id, 0), 64).unwrap().status, ERR_STATE);
    dialogs.answer(id, &Answer::Text("typed".into())).unwrap();
    let got = plugin.event(0, &read(id, 0), 128).unwrap();
    let part = RawRecord::one(&got.out)
        .unwrap()
        .decode::<ServiceUiText<'_>>()
        .unwrap();
    assert_eq!((part.total, part.data), (5, "typed"));
    // After a final answer the dialog can write no more.
    assert_eq!(
        plugin.event(0, &write(id, 0, "x", true), 8).unwrap().status,
        ERR_STATE
    );
}

#[test]
fn texts_over_their_caps_are_no_space_and_a_secret_is_kept_out_of_the_commands() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    let id = request_of(&plugin.event(0, &ask(0, 4, "Password", ""), 64).unwrap().out);
    assert!(dialogs.last().unwrap().secret());
    // A secret text holds 8,192 bytes: parts of 3,000 fit twice, not three times.
    let part = "x".repeat(3000);
    let status = |offset: u32, last: bool| {
        plugin
            .event(0, &write(id, offset, &part, last), 8)
            .unwrap()
            .status
    };
    assert_eq!(status(0, false), 0);
    assert_eq!(status(3000, false), 0);
    assert_eq!(status(6000, true), ERR_NO_SPACE);
    assert_eq!(
        dialogs.answer(id, &Answer::Text("y".repeat(8193))),
        Err(ERR_NO_SPACE)
    );
    dialogs.answer(id, &Answer::Text("hunter2".into())).unwrap();
    assert_eq!(dialogs.secrets(), [b"hunter2".to_vec()]);
    // The proxy stages nothing, so no command batch holds the secret.
    let after = plugin.event(0, &read(id, 0), 128).unwrap();
    assert!(!after.batches_contain(b"hunter2"));
}
