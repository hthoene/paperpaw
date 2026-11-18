//! The scripted `core.ui`: asking, the host's request ids, the request text,
//! the answers and what a secret leaves behind.
#![allow(clippy::unwrap_used, reason = "fixture failures fail the test")]
mod common;
use common::{call, entry, run, run_calls};
use paperpaw_core::native_host::registrations::{
    RawRecord, Record, ServiceUiAsk, ServiceUiRequest, ServiceUiText, ServiceUiTextAt,
    records_encode,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_STATE};
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
    let done = run(&plugin, &ask(2, 0, "Reset?", ""), 4096);
    assert!(done.status > 0);
    let request = request_of(&done.out);
    assert_eq!(request, 1);
    let question = dialogs.last().unwrap();
    assert_eq!(
        (question.request, question.kind, question.title.as_str()),
        (1, 2, "Reset?")
    );
    // A second ask by the same asker cancels the open one; the cancel event is due.
    let second = request_of(&run(&plugin, &ask(1, 0, "Pick", "a\nb"), 4096).out);
    assert_eq!(second, 2);
    assert!(dialogs.answer(1, &Answer::Confirm(true)).is_err());
    assert!(dialogs.answer(1, &Answer::Cancelled).is_ok());
    // A kind the host does not know is invalid.
    assert_eq!(run(&plugin, &ask(4, 0, "x", ""), 4096).status, ERR_INVALID);
}

#[test]
fn answers_by_kind_and_the_event_the_asker_gets() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    let id = request_of(&run(&plugin, &ask(1, 0, "Pick", "a\nb"), 4096).out);
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
    let confirm = request_of(&run(&plugin, &ask(2, 0, "ok?", ""), 4096).out);
    let yes = dialogs.answer(confirm, &Answer::Confirm(true)).unwrap();
    assert!(!yes.is_empty());
}

#[test]
fn the_request_text_is_written_in_parts_and_read_back_after_the_answer() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    let id = request_of(&run(&plugin, &ask(0, 0, "Name", ""), 4096).out);
    assert_eq!(run(&plugin, &write(id, 0, "ab", false), 64).status, 0);
    // Appending at the wrong offset is invalid; the text is uncommitted until `last`.
    assert_eq!(
        run(&plugin, &write(id, 5, "c", true), 64).status,
        ERR_INVALID
    );
    assert_eq!(dialogs.initial_text(id), None);
    assert_eq!(run(&plugin, &write(id, 2, "cd", true), 64).status, 0);
    assert_eq!(dialogs.initial_text(id).as_deref(), Some("abcd"));
    // The asker reads only after the dialog answered.
    assert_eq!(run(&plugin, &read(id, 0), 4096).status, ERR_STATE);
    dialogs.answer(id, &Answer::Text("typed".into())).unwrap();
    let got = run(&plugin, &read(id, 0), 4096);
    let part = RawRecord::one(&got.out)
        .unwrap()
        .decode::<ServiceUiText<'_>>()
        .unwrap();
    assert_eq!((part.total, part.data), (5, "typed"));
    // After a final answer the dialog can write no more.
    assert_eq!(run(&plugin, &write(id, 0, "x", true), 64).status, ERR_STATE);
}

#[test]
fn texts_over_their_caps_are_no_space_and_a_secret_is_kept_out_of_the_commands() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    let id = request_of(&run(&plugin, &ask(0, 4, "Password", ""), 4096).out);
    assert!(dialogs.last().unwrap().secret());
    // A secret text holds 8,192 bytes: parts of 3,000 fit twice, not three times.
    let part = "x".repeat(3000);
    let status = |offset: u32, last: bool| run(&plugin, &write(id, offset, &part, last), 64).status;
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
    let after = run(&plugin, &read(id, 0), 4096);
    assert!(!after.batches_contain(b"hunter2"));
}

/// An `ask` with every field the test varies.
fn ask_with(kind: u32, flags: u32, slot: &str, choices: &str) -> Vec<u8> {
    call(
        "core.ui",
        8,
        &batch(&Record::ServiceUiAsk(ServiceUiAsk {
            kind,
            flags,
            slot,
            title: "t",
            text: "q",
            choices,
        })),
    )
}

fn answered_flags(event: &[u8]) -> (u32, u32) {
    let Record::EventAnswered(answered) =
        paperpaw_core::native_host::registrations::records_decode(event)
            .unwrap()
            .iter()
            .next()
            .unwrap()
    else {
        panic!("an Answered record")
    };
    (answered.outcome, answered.flags)
}

#[test]
fn ask_validates_the_form_as_the_host_does() {
    let host = ScriptedHost::new();
    host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    let status = |kind, flags, slot: &str, choices: &str| {
        let status = run(&plugin, &ask_with(kind, flags, slot, choices), 4096).status;
        i32::from(status < 0) * status
    };
    let long = "a".repeat(129);
    let many = vec!["a"; 33].join("\n");
    // The forms the host takes.
    for (kind, flags, slot, choices) in [
        (0, 0, "", ""),
        (0, 0x4_5, "", ""),
        (0, 4 << 4, "", ""),
        (0, 0, "core.remote-keyboard", ""),
        (1, 0, "", "a\nb"),
        (2, 0, "", ""),
        (2, 1 << 9, "", "Yes\nNo"),
        (2, 1 << 8, "", "OK\n"),
        (3, 0, "", ""),
    ] {
        assert_eq!(
            status(kind, flags, slot, choices),
            0,
            "{kind} {flags:#x} {choices:?}"
        );
    }
    // Unknown flag bits (bits 0-7 are kind 0's, 8-9 kind 2's) and text modes.
    for (kind, flags) in [
        (0, 1 << 10),
        (0, 1 << 8),
        (0, 5 << 4),
        (1, 1),
        (2, 1),
        (2, 1 << 10),
        (3, 1 << 8),
    ] {
        let choices = if kind == 1 { "a" } else { "" };
        assert_eq!(
            status(kind, flags, "", choices),
            ERR_INVALID,
            "{kind} {flags:#x}"
        );
    }
    // Slots: only the keyboard's, only for text.
    for (kind, slot) in [
        (0, "x"),
        (1, "core.remote-keyboard"),
        (2, "core.remote-keyboard"),
        (3, "core.remote-keyboard"),
    ] {
        let choices = if kind == 1 { "a" } else { "" };
        assert_eq!(status(kind, 0, slot, choices), ERR_INVALID, "{kind} {slot}");
    }
    // Choices per kind: choice questions need lines; text and folder take none.
    assert_eq!(status(1, 0, "", ""), ERR_INVALID);
    assert_eq!(status(1, 0, "", "a\n\nb"), ERR_INVALID);
    assert_eq!(status(1, 0, "", &long), ERR_NO_SPACE);
    assert_eq!(status(1, 0, "", &many), ERR_NO_SPACE);
    // (The 2,048 byte total is the wire field's own bound: it cannot be sent over.)
    assert_eq!(status(0, 0, "", "a"), ERR_INVALID);
    assert_eq!(status(3, 0, "", "a"), ERR_INVALID);
    // Confirm words: empty, or exactly two lines of at most 128 bytes.
    assert_eq!(status(2, 0, "", "Yes"), ERR_INVALID);
    assert_eq!(status(2, 0, "", "a\nb\nc"), ERR_INVALID);
    assert_eq!(status(2, 0, "", &format!("{long}\nno")), ERR_NO_SPACE);
    // `only_yes` shows one button: its `no` line must be empty.
    assert_eq!(status(2, 1 << 8, "", "OK\nNo"), ERR_INVALID);
}

#[test]
fn a_failed_asking_callback_cancels_the_request_before_any_dialog_opens() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    // The callback asks, then fails.
    let (end, done) = run_calls(&plugin, &[(ask(0, 0, "Name", ""), 4096)], ERR_IO);
    assert_eq!((end, done[0].status > 0), (ERR_IO, true));
    assert!(!dialogs.opened(1));
    assert_eq!(dialogs.due(), [1]);
    // No partial answer reaches a dialog; the asker gets outcome 1 with
    // `not_started` (bit 1), once.
    assert_eq!(dialogs.answer(1, &Answer::Text("x".into())), Err(ERR_STATE));
    let event = dialogs.answer(1, &Answer::Cancelled).unwrap();
    assert_eq!(answered_flags(&event), (1, 2));
    assert!(dialogs.due().is_empty());
    assert_eq!(dialogs.answer(1, &Answer::Cancelled), Err(ERR_STATE));
}

#[test]
fn an_initial_text_begun_and_not_committed_cancels_the_request_at_the_callbacks_end() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    // The callback returns well, but its initial text has no `last` part.
    let (end, _) = run_calls(
        &plugin,
        &[
            (ask(0, 0, "Name", ""), 4096),
            (write(1, 0, "ab", false), 64),
        ],
        0,
    );
    assert_eq!(end, 0);
    assert!(!dialogs.opened(1));
    let event = dialogs.answer(1, &Answer::Cancelled).unwrap();
    assert_eq!(answered_flags(&event), (1, 2));
    // With `last` the text is committed and the dialog opens.
    let (_, done) = run_calls(
        &plugin,
        &[(ask(0, 0, "Name", ""), 4096), (write(2, 0, "ab", true), 64)],
        0,
    );
    assert!(done.iter().all(|done| done.status >= 0));
    assert!(dialogs.opened(2));
    assert!(dialogs.due().is_empty());
    assert_eq!(dialogs.initial_text(2).as_deref(), Some("ab"));
    // Asking again is the other cancel: not flagged `not_started`.
    run(&plugin, &ask(0, 0, "Again", ""), 4096);
    assert_eq!(dialogs.due(), [2]);
    assert_eq!(
        answered_flags(&dialogs.answer(2, &Answer::Cancelled).unwrap()),
        (1, 0)
    );
}

#[test]
fn a_secret_typing_update_is_kept_for_the_commands_check_and_a_folder_answer_is_capped() {
    let host = ScriptedHost::new();
    let dialogs = host.dialogs();
    let plugin = host.load(&entry()).unwrap();
    // Secret and live (bits 2 and 0): the typing text is as secret as the answer.
    let secret = request_of(&run(&plugin, &ask(0, 5, "PIN", ""), 4096).out);
    dialogs
        .answer(secret, &Answer::Typing("12".into()))
        .unwrap();
    assert_eq!(dialogs.secrets(), [b"12".to_vec()]);
    dialogs
        .answer(secret, &Answer::Text("1234".into()))
        .unwrap();
    assert_eq!(dialogs.secrets(), [b"12".to_vec(), b"1234".to_vec()]);
    // A cancel leaves nothing to look for.
    let again = request_of(&run(&plugin, &ask(0, 4, "PIN", ""), 4096).out);
    dialogs.answer(again, &Answer::Cancelled).unwrap();
    assert_eq!(dialogs.secrets().len(), 2);
    // Folder answers are paths on root 1 of at most 512 bytes.
    let folder = request_of(&run(&plugin, &ask(3, 0, "Folder", ""), 4096).out);
    assert_eq!(
        dialogs.answer(folder, &Answer::Text("a".repeat(513))),
        Err(ERR_INVALID)
    );
    assert!(
        dialogs
            .answer(folder, &Answer::Text("a".repeat(512)))
            .is_ok()
    );
}
