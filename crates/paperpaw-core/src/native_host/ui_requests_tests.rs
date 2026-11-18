//! Requests: the form of a question, the phases, the answers, who may write and
//! read, cancellation, delivery and cleanup.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::native_host::ui_store::Limits;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_native::host_glue::ERR_IO;

fn who(owner: u64, epoch: u64) -> Stamp {
    Stamp::new(owner, epoch).unwrap()
}

fn store() -> UiStore {
    UiStore::new(Limits::ELSEWHERE)
}

fn q(kind: u32, flags: u32, choices: &'static str) -> Question<'static> {
    Question {
        kind,
        flags,
        remote: false,
        title: "Title",
        text: "Question?",
        choices,
    }
}

fn ask(s: &UiStore, owner: u64, epoch: u64, question: &Question<'_>) -> u32 {
    s.ask(
        who(owner, epoch),
        None,
        question,
        question.flags & SECRET != 0,
        None,
        false,
    )
    .unwrap()
    .0
}

/// The callback returns and the stack opens the dialog as `dialog`.
fn open(s: &UiStore, owner: u64, epoch: u64, id: u32, dialog: u32) {
    let mut started = Vec::new();
    s.callback_returned(who(owner, epoch), false, &mut |start| started.push(start))
        .unwrap();
    assert_eq!(started.iter().map(|s| s.request).collect::<Vec<_>>(), [id]);
    s.dialog_opened(id, dialog).unwrap();
}

fn text(s: &UiStore, who: u64, instance: Option<u32>, id: u32) -> Result<String, i32> {
    let mut all = Vec::new();
    let mut offset = 0;
    loop {
        let mut out = [0_u8; 5];
        let (total, n) = s.request_text_read(who, instance, id, offset, &mut out)?;
        all.extend_from_slice(&out[..n]);
        offset += n;
        if offset >= total {
            return Ok(String::from_utf8(all).unwrap());
        }
    }
}

#[test]
fn the_form_of_a_question_kinds_flags_and_choices() {
    let ok = |kind, flags, choices| q(kind, flags, choices).check();
    assert_eq!(ok(0, 0, ""), Ok(()));
    assert_eq!(ok(4, 0, "").err(), Some(ERR_INVALID));
    // Flags: bits 0-7 for text only (mode 0-4), 8-9 for confirm only.
    assert_eq!(ok(0, 0b111 | (4 << 4), ""), Ok(()));
    assert_eq!(ok(0, 5 << 4, "").err(), Some(ERR_INVALID));
    assert_eq!(ok(0, ONLY_YES, "").err(), Some(ERR_INVALID));
    assert_eq!(ok(2, LIVE, "").err(), Some(ERR_INVALID));
    assert_eq!(ok(1, 1, "a").err(), Some(ERR_INVALID));
    assert_eq!(ok(2, 0x400, "").err(), Some(ERR_INVALID));
    // Choice: non-empty lines, no trailing break, 32 of 128 bytes, 2,048 in all.
    assert_eq!(ok(1, 0, "").err(), Some(ERR_INVALID));
    assert_eq!(ok(1, 0, "a\nb"), Ok(()));
    assert_eq!(ok(1, 0, "a\n\nb").err(), Some(ERR_INVALID));
    assert_eq!(ok(1, 0, "a\nb\n").err(), Some(ERR_INVALID));
    let lines32 = std::vec!["x"; 32].join("\n");
    let lines33 = std::vec!["x"; 33].join("\n");
    assert_eq!(
        q(1, 0, std::boxed::Box::leak(lines32.into_boxed_str())).check(),
        Ok(())
    );
    assert_eq!(
        q(1, 0, std::boxed::Box::leak(lines33.into_boxed_str()))
            .check()
            .err(),
        Some(ERR_NO_SPACE)
    );
    let long = "l".repeat(129);
    assert_eq!(
        q(1, 0, std::boxed::Box::leak(long.into_boxed_str()))
            .check()
            .err(),
        Some(ERR_NO_SPACE)
    );
    let line128 = "l".repeat(128);
    assert_eq!(
        q(1, 0, std::boxed::Box::leak(line128.into_boxed_str())).check(),
        Ok(())
    );
    // Confirm: none, or exactly two lines (an empty one keeps the word).
    assert_eq!(ok(2, 0, ""), Ok(()));
    assert_eq!(ok(2, 0, "Delete\nKeep"), Ok(()));
    assert_eq!(ok(2, 0, "\nKeep"), Ok(()));
    assert_eq!(ok(2, 0, "Delete").err(), Some(ERR_INVALID));
    assert_eq!(ok(2, 0, "a\nb\nc").err(), Some(ERR_INVALID));
    let over = "y".repeat(129);
    assert_eq!(
        q(
            2,
            0,
            std::boxed::Box::leak(std::format!("{over}\nno").into_boxed_str())
        )
        .check()
        .err(),
        Some(ERR_NO_SPACE)
    );
    // Only yes: one button, its `no` line empty.
    assert_eq!(ok(2, ONLY_YES, "Read\n"), Ok(()));
    assert_eq!(ok(2, ONLY_YES, ""), Ok(()));
    assert_eq!(ok(2, ONLY_YES, "Read\nNo").err(), Some(ERR_INVALID));
    // Focus on yes (bit 9, for the dialog plugin to honour): with the
    // default words, with words, with only-yes; not on other kinds.
    let focus_yes = 1 << 9;
    assert_eq!(ok(2, focus_yes, ""), Ok(()));
    assert_eq!(ok(2, focus_yes, "Delete\nKeep"), Ok(()));
    assert_eq!(ok(2, focus_yes | ONLY_YES, "Read\n"), Ok(()));
    assert_eq!(ok(0, focus_yes, "").err(), Some(ERR_INVALID));
    assert_eq!(ok(1, focus_yes, "a").err(), Some(ERR_INVALID));
    // Kinds 0 and 3 take no choices.
    assert_eq!(ok(0, 0, "a").err(), Some(ERR_INVALID));
    assert_eq!(ok(3, 0, "a").err(), Some(ERR_INVALID));
    assert_eq!(ok(3, 0, ""), Ok(()));
}

#[test]
fn a_text_question_runs_asked_opened_answered_delivered_freed() {
    let s = store();
    let id = ask(&s, 1, 10, &q(0, 0, ""));
    // The asker writes the initial text inside the asking callback.
    s.request_text_write(who(1, 10), None, id, 0, b"start", true)
        .unwrap();
    assert_eq!(
        s.request_text_write(who(1, 11), None, id, 0, b"x", true),
        Err(ERR_STATE)
    );
    assert_eq!(
        s.request_text_write(who(2, 10), None, id, 0, b"x", true),
        Err(ERR_STATE)
    );
    // Nothing is read, nothing answered, before the dialog is up.
    assert_eq!(s.answer(7, id, 0, 0), Err(ERR_STATE));
    open(&s, 1, 10, id, 7);
    // Only the dialog reads the question.
    assert!(s.question(8, id, |_| ()).is_err());
    s.question(7, id, |shown| {
        assert_eq!(
            (shown.kind, shown.max_bytes, shown.title, shown.text),
            (0, 24_576, "Title", "Question?")
        );
    })
    .unwrap();
    assert_eq!(
        text(&s, 1, Some(30), id),
        Err(ERR_STATE),
        "the asker is not the dialog"
    );
    assert_eq!(
        text(&s, 9, Some(7), id).unwrap(),
        "start",
        "the dialog reads it"
    );
    // The dialog writes its answer (a start over) and commits with op 11.
    s.request_text_write(who(9, 3), Some(7), id, 0, b"hel", false)
        .unwrap();
    assert_eq!(s.answer(7, id, 0, 0), Err(ERR_STATE), "uncommitted");
    s.request_text_write(who(9, 3), Some(7), id, 3, b"lo", true)
        .unwrap();
    assert_eq!(
        s.request_text_write(who(1, 4), None, id, 0, b"x", true),
        Err(ERR_STATE)
    );
    s.answer(7, id, 0, 0).unwrap();
    assert_eq!(
        s.answer(7, id, 0, 0),
        Err(ERR_STATE),
        "the first final answer wins"
    );
    assert_eq!(
        s.request_text_write(who(9, 4), Some(7), id, 0, b"x", true),
        Err(ERR_STATE)
    );
    // Delivery: the event from the request, the text readable in that callback.
    assert_eq!(text(&s, 1, None, id), Err(ERR_STATE), "not delivering yet");
    let delivery = s.take_delivery(1).unwrap();
    assert_eq!(
        delivery,
        Delivery {
            request: id,
            kind: 0,
            outcome: 0,
            choice: 0,
            length: 5,
            flags: 0
        }
    );
    assert!(s.take_delivery(1).is_none());
    assert_eq!(text(&s, 1, None, id).unwrap(), "hello");
    assert_eq!(text(&s, 2, None, id), Err(ERR_STATE));
    s.delivered(id).unwrap();
    assert_eq!(s.request_count(), 0);
    assert_eq!(s.held().1, 0);
}

#[test]
fn a_failed_or_half_written_start_cancels_before_any_dialog() {
    let s = store();
    // The callback failed.
    let a = ask(&s, 1, 1, &q(0, 0, ""));
    let mut started = 0;
    s.callback_returned(who(1, 1), true, &mut |_| started += 1)
        .unwrap();
    // An initial text begun and not committed.
    let b = ask(&s, 1, 2, &q(0, 0, ""));
    s.request_text_write(who(1, 2), None, b, 0, b"half", false)
        .unwrap();
    s.callback_returned(who(1, 2), false, &mut |_| started += 1)
        .unwrap();
    assert_eq!(started, 0);
    let first = s.take_delivery(1).unwrap();
    assert_eq!(
        (first.request, first.outcome, first.flags),
        (a, 1, NOT_STARTED)
    );
    s.delivered(a).unwrap();
    let second = s.take_delivery(1).unwrap();
    assert_eq!(
        (second.request, second.outcome, second.flags),
        (b, 1, NOT_STARTED)
    );
    // A dialog that cannot open is the same.
    let c = ask(&s, 1, 3, &q(2, 0, ""));
    s.callback_returned(who(1, 3), false, &mut |_| ()).unwrap();
    s.dialog_failed(c).unwrap();
    s.delivered(b).unwrap();
    assert_eq!(s.take_delivery(1).unwrap().flags, NOT_STARTED);
}

#[test]
fn what_each_kind_answers() {
    let s = store();
    // Choice: the line's index, none past the list.
    let id = ask(&s, 1, 1, &q(1, 0, "a\nb\nc"));
    open(&s, 1, 1, id, 5);
    assert_eq!(s.answer(5, id, 0, 3), Err(ERR_INVALID));
    assert_eq!(s.answer(5, id, 3, 0), Err(ERR_INVALID), "unknown outcome");
    assert_eq!(
        s.answer(5, id, 2, 0),
        Err(ERR_INVALID),
        "choice cannot type"
    );
    s.answer(5, id, 0, 2).unwrap();
    assert_eq!(s.take_delivery(1).unwrap().choice, 2);
    s.delivered(id).unwrap();
    // Confirm: 1 yes, 0 no, nothing else.
    let id = ask(&s, 1, 2, &q(2, 0, ""));
    open(&s, 1, 2, id, 6);
    assert_eq!(s.answer(6, id, 0, 2), Err(ERR_INVALID));
    s.answer(6, id, 0, 1).unwrap();
    assert_eq!(s.take_delivery(1).unwrap().choice, 1);
    s.delivered(id).unwrap();
    // Folder: a committed path of at most 512 bytes.
    let id = ask(&s, 1, 3, &q(3, 0, ""));
    open(&s, 1, 3, id, 7);
    s.request_text_write(who(2, 1), Some(7), id, 0, "p".repeat(513).as_bytes(), true)
        .unwrap();
    assert_eq!(s.answer(7, id, 0, 0), Err(ERR_INVALID));
    s.request_text_write(who(2, 1), Some(7), id, 0, "p".repeat(512).as_bytes(), true)
        .unwrap();
    s.answer(7, id, 0, 0).unwrap();
    assert_eq!(s.take_delivery(1).unwrap().length, 512);
    s.delivered(id).unwrap();
    // Cancel needs no text and carries nothing.
    let id = ask(&s, 1, 4, &q(0, 0, ""));
    open(&s, 1, 4, id, 8);
    s.answer(8, id, 1, 9).unwrap();
    let d = s.take_delivery(1).unwrap();
    assert_eq!((d.outcome, d.choice, d.length), (1, 0, 0));
}

#[test]
fn live_typing_replaces_itself_and_a_final_answer_replaces_it() {
    let s = store();
    let id = ask(&s, 1, 1, &q(0, LIVE, ""));
    open(&s, 1, 1, id, 5);
    s.request_text_write(who(2, 1), Some(5), id, 0, b"a", true)
        .unwrap();
    s.answer(5, id, 2, 0).unwrap();
    s.request_text_write(who(2, 2), Some(5), id, 0, b"ab", true)
        .unwrap();
    // A write withdraws the pending update; the next answer makes a new one.
    assert!(s.take_delivery(1).is_none());
    s.answer(5, id, 2, 0).unwrap();
    s.answer(5, id, 2, 0).unwrap();
    let d = s.take_delivery(1).unwrap();
    assert_eq!((d.outcome, d.length), (2, 2));
    s.delivered(id).unwrap();
    assert_eq!(s.request_count(), 1, "typing keeps the request");
    s.answer(5, id, 0, 0).unwrap();
    assert_eq!(s.take_delivery(1).unwrap().outcome, 0);
    // Typing needs a live text question.
    let quiet = ask(&s, 3, 1, &q(0, 0, ""));
    open(&s, 3, 1, quiet, 6);
    s.request_text_write(who(2, 3), Some(6), quiet, 0, b"x", true)
        .unwrap();
    assert_eq!(s.answer(6, quiet, 2, 0), Err(ERR_INVALID));
}

#[test]
fn on_the_x4_live_typing_stops_past_512_bytes_but_the_final_answer_is_whole() {
    let s = UiStore::new(Limits::X4);
    let id = s
        .ask(who(1, 1), None, &q(0, LIVE, ""), false, None, false)
        .unwrap()
        .0;
    open(&s, 1, 1, id, 5);
    s.request_text_write(who(2, 1), Some(5), id, 0, &[b'a'; 512], true)
        .unwrap();
    s.answer(5, id, 2, 0).unwrap();
    s.request_text_write(who(2, 2), Some(5), id, 0, &[b'a'; 600], true)
        .unwrap();
    assert_eq!(s.answer(5, id, 2, 0), Err(ERR_NO_SPACE));
    let d = s.take_delivery(1).unwrap();
    assert_eq!((d.outcome, d.flags), (2, TYPING_STOPPED));
    s.delivered(id).unwrap();
    s.answer(5, id, 0, 0).unwrap();
    assert_eq!(s.take_delivery(1).unwrap().length, 600);
}

#[test]
fn closing_cancels_and_the_dialog_is_named_to_close() {
    let s = store();
    // A dialog that closes without an answer.
    let id = ask(&s, 1, 1, &q(2, 0, ""));
    open(&s, 1, 1, id, 5);
    s.dialog_closed(5).unwrap();
    let d = s.take_delivery(1).unwrap();
    assert_eq!((d.outcome, d.flags), (1, DIALOG_CLOSED));
    s.delivered(id).unwrap();
    // A screen that asked and closes: the request is cancelled, its dialog told.
    let s = store();
    let id = s
        .ask(who(1, 1), Some(30), &q(2, 0, ""), false, None, false)
        .unwrap()
        .0;
    open(&s, 1, 1, id, 5);
    let mut closed = Vec::new();
    s.asker_instance_closed(30, &mut |c| closed.push(c))
        .unwrap();
    assert_eq!(
        closed,
        [Cancelled {
            request: id,
            dialog: Some(5)
        }]
    );
    assert_eq!(s.take_delivery(1).unwrap().outcome, 1);
    // Asking again cancels the open one first.
    let s = store();
    let first = ask(&s, 1, 1, &q(2, 0, ""));
    open(&s, 1, 1, first, 5);
    let (second, cancelled) = s
        .ask(who(1, 2), None, &q(2, 0, ""), false, None, true)
        .unwrap();
    assert_eq!(
        cancelled,
        Some(Cancelled {
            request: first,
            dialog: Some(5)
        })
    );
    assert_ne!(first, second);
    assert_eq!(s.take_delivery(1).unwrap().request, first);
}

#[test]
fn a_nested_question_hangs_under_its_parent_and_ends_with_it() {
    let s = store();
    let parent = s
        .ask(who(1, 1), None, &q(0, SECRET, ""), true, None, false)
        .unwrap()
        .0;
    open(&s, 1, 1, parent, 5);
    assert_eq!(s.dialog_of(5), Some((parent, true, false, false)));
    let nested = s
        .ask(who(2, 1), Some(5), &q(0, 0, ""), true, Some(parent), false)
        .unwrap()
        .0;
    assert_eq!(
        s.dialog_of(5),
        Some((parent, true, false, true)),
        "its one nested is open"
    );
    // Not counted among the open requests.
    assert_eq!(s.open_requests(2), (1, false));
    // A second nested one under the same parent is full.
    assert_eq!(
        s.ask(who(2, 2), Some(5), &q(0, 0, ""), true, Some(parent), false)
            .err(),
        Some(ERR_FULL)
    );
    // The parent ends: the nested one is cancelled with it.
    s.answer(5, parent, 1, 0).unwrap();
    let d = s.take_delivery(2).unwrap();
    assert_eq!((d.request, d.outcome), (nested, 1));
}

#[test]
fn texts_are_capped_24576_and_8192_for_a_secret_one_and_freed_with_the_asker() {
    let s = store();
    let id = ask(&s, 1, 1, &q(0, 0, ""));
    let part = [b'a'; 3_072];
    for n in 0..8 {
        s.request_text_write(who(1, 1), None, id, n * 3_072, &part, false)
            .unwrap();
    }
    assert_eq!(
        s.request_text_write(who(1, 1), None, id, 24_576, b"x", true),
        Err(ERR_NO_SPACE)
    );
    let secret = s
        .ask(who(2, 1), None, &q(0, SECRET, ""), true, None, false)
        .unwrap()
        .0;
    s.request_text_write(who(2, 1), None, secret, 0, &[b'p'; 3_072], false)
        .unwrap();
    s.request_text_write(who(2, 1), None, secret, 3_072, &[b'p'; 3_072], false)
        .unwrap();
    assert_eq!(
        s.request_text_write(who(2, 1), None, secret, 6_144, &[b'p'; 3_072], true),
        Err(ERR_NO_SPACE)
    );
    s.question(1, secret, |_| ()).ok();
    // An open request keeps its asker from sleeping again (#248, D3).
    assert_eq!(s.owner_holds(2), Ok(true));
    s.owner_stopped(1).unwrap();
    s.owner_stopped(2).unwrap();
    assert_eq!(s.owner_holds(2), Ok(false));
    assert_eq!(s.request_count(), 0);
    assert_eq!(s.held().1, 0);
}

#[test]
fn nothing_shows_in_a_debug_print_and_failed_allocations_change_nothing() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let s = store();
    let id = ask(&s, 1, 1, &q(0, SECRET, ""));
    s.request_text_write(who(1, 1), None, id, 0, b"hunter2", true)
        .unwrap();
    assert!(!std::format!("{s:?}").contains("hunter2"));
    for nth in 1..=6 {
        FAIL_NTH.with(|n| n.set(nth));
        let asked = s.ask(who(3, 1), None, &q(0, 0, ""), false, None, false);
        FAIL_NTH.with(|n| n.set(0));
        match asked {
            Err(e) => assert_eq!(e, ERR_NO_MEMORY, "allocation {nth}"),
            Ok(_) => s.owner_stopped(3).unwrap(),
        }
    }
    assert_eq!(s.request_count(), 1);
    // Unknown ids.
    assert_eq!(s.answer(1, 999, 0, 0), Err(ERR_NOT_FOUND));
    assert!(s.question(1, 999, |_| ()).is_err());
}

#[test]
fn retained_request_rows_are_bounded_replacements_and_cancellations_included() {
    let s = UiStore::new(Limits::X4);
    let kept = Limits::X4.requests_kept;
    // Replacing at once and again: each cancelled row waits for its event, so
    // the rows kept stop at the cap and the refusal changes nothing.
    let mut asked = 0;
    let refused = loop {
        let got = s.ask(who(1, 1), None, &q(0, 0, ""), false, None, true);
        match got {
            Ok(_) => asked += 1,
            Err(e) => break e,
        }
        assert!(asked <= kept);
    };
    assert_eq!(refused, ERR_FULL);
    assert_eq!(asked, kept);
    assert_eq!(s.request_count(), kept);
    // Delivering the cancellations frees their rows and asking works again.
    while s.take_delivery(1).is_some() {
        // The answer is delivered and the callback returns.
    }
    for id in 1..=kept as u32 {
        s.delivered(id).unwrap();
    }
    assert!(s.request_count() < kept);
    s.ask(who(1, 2), None, &q(0, 0, ""), false, None, true)
        .unwrap();
}

#[test]
fn small_text_writes_cost_chunks_not_a_descriptor_each_and_are_charged_for_them() {
    let s = UiStore::new(Limits::X4);
    let id = ask(&s, 1, 1, &q(0, 0, ""));
    let mut written = 0;
    let stopped = loop {
        match s.request_text_write(who(1, 1), None, id, written, b"x", false) {
            Ok(()) => written += 1,
            Err(e) => break e,
        }
    };
    assert_eq!(stopped, ERR_IO, "the 2 KiB pool is charged for chunks");
    // 7 chunks of 256 bytes and their descriptors fit; an 8th does not.
    assert_eq!(written, 7 * 256);
    let inner = s.inner.borrow();
    let request = &inner.requests[0];
    assert_eq!(request.text.parts.len(), 7);
    assert_eq!(inner.text_memory, request.text.charged);
    assert!(inner.text_memory <= Limits::X4.text_memory);
}

#[test]
fn a_secret_text_has_its_own_pool_and_does_not_take_room_from_ordinary_text() {
    let s = UiStore::new(Limits::X4);
    let secret = s
        .ask(who(1, 1), None, &q(0, SECRET, ""), true, None, false)
        .unwrap()
        .0;
    // 8 KiB of password, a byte at a time: a handful of chunks, the ordinary
    // pool untouched.
    for at in 0..SECRET_TEXT_MAX {
        s.request_text_write(who(1, 1), None, secret, at, b"p", false)
            .unwrap();
    }
    assert_eq!(s.held().1, 0, "nothing charged to the ordinary pool");
    assert!(s.inner.borrow().requests[0].text.parts.len() <= SECRET_TEXT_MAX / 256 + 1);
    // An ordinary text still gets its 2 KiB pool.
    let plain = ask(&s, 2, 1, &q(0, 0, ""));
    s.request_text_write(who(2, 1), None, plain, 0, &[b'a'; 1_800], true)
        .unwrap();
    assert!(s.held().1 >= 1_800);
    // Freeing the secret returns its pool.
    s.owner_stopped(1).unwrap();
    assert_eq!(s.inner.borrow().secret_memory, 0);
}

#[test]
fn text_writes_that_cannot_allocate_leave_the_text_as_it_was() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let s = UiStore::new(Limits::X4);
    let id = ask(&s, 1, 1, &q(0, 0, ""));
    s.request_text_write(who(1, 1), None, id, 0, b"abc", false)
        .unwrap();
    let before = s.held();
    for nth in 1..=3 {
        FAIL_NTH.with(|n| n.set(nth));
        // 300 bytes need a new chunk (an allocation); the failure changes nothing.
        let got = s.request_text_write(who(1, 1), None, id, 3, &[b'z'; 300], true);
        FAIL_NTH.with(|n| n.set(0));
        if got.is_ok() {
            // The nth allocation was not on this path (none happens): done.
            return;
        }
        assert_eq!(got, Err(ERR_NO_MEMORY));
        assert_eq!(s.held(), before);
        assert_eq!(s.inner.borrow().requests[0].text.bytes, 3);
    }
}

#[test]
fn the_asker_reads_the_text_while_a_typing_update_is_delivered_and_only_then() {
    let s = store();
    let id = ask(&s, 1, 1, &q(0, LIVE, ""));
    open(&s, 1, 1, id, 5);
    s.request_text_write(who(2, 1), Some(5), id, 0, b"hel", true)
        .unwrap();
    s.answer(5, id, 2, 0).unwrap();
    // Before the delivery: the asker may not read (the dialog still may).
    assert_eq!(text(&s, 1, None, id), Err(ERR_STATE));
    assert_eq!(text(&s, 2, Some(5), id).unwrap(), "hel");
    // During the delivery of the typing update the request is still open.
    let d = s.take_delivery(1).unwrap();
    assert_eq!(d.outcome, 2);
    assert_eq!(text(&s, 1, None, id).unwrap(), "hel");
    assert_eq!(text(&s, 3, None, id), Err(ERR_STATE), "another plugin");
    // After the callback returned: refused again.
    s.delivered(id).unwrap();
    assert_eq!(text(&s, 1, None, id), Err(ERR_STATE));
}

#[test]
fn a_final_answer_notes_when_its_dialog_closed_after_the_answer() {
    let s = store();
    let id = ask(&s, 1, 1, &q(2, 0, ""));
    open(&s, 1, 1, id, 5);
    s.answer(5, id, 0, 1).unwrap();
    // The dialog closes before the answer is delivered.
    s.dialog_closed(5).unwrap();
    let d = s.take_delivery(1).unwrap();
    assert_eq!((d.outcome, d.choice), (0, 1));
    assert_eq!(d.flags & DIALOG_CLOSED, DIALOG_CLOSED);
    s.delivered(id).unwrap();
    // A dialog that stays up until delivery does not set the bit.
    let again = ask(&s, 1, 2, &q(2, 0, ""));
    open(&s, 1, 2, again, 6);
    s.answer(6, again, 0, 0).unwrap();
    s.dialog_closed(99).unwrap();
    assert_eq!(s.take_delivery(1).unwrap().flags & DIALOG_CLOSED, 0);
}

#[test]
fn a_nested_question_goes_when_the_plugin_that_asked_its_parent_stops() {
    let s = store();
    let parent = ask(&s, 1, 1, &q(0, 0, ""));
    open(&s, 1, 1, parent, 5);
    // The parent's dialog (instance 5) asks a nested question; the remote
    // keyboard (owner 9) is its asker.
    let (nested, _) = s
        .ask(who(9, 1), None, &q(0, 0, ""), false, Some(parent), false)
        .unwrap();
    assert!(s.is_request(nested));
    s.owner_stopped(1).unwrap();
    assert!(!s.is_request(parent));
    assert!(!s.is_request(nested), "no nested row is left behind");
    assert_eq!(s.request_count(), 0);
}

#[test]
fn stopping_an_unrelated_plugin_keeps_a_nested_cancellation_whose_event_is_owed() {
    let s = store();
    let parent = ask(&s, 1, 1, &q(0, 0, ""));
    open(&s, 1, 1, parent, 5);
    let (nested, _) = s
        .ask(who(9, 1), None, &q(0, 0, ""), false, Some(parent), false)
        .unwrap();
    // The parent is answered (this cancels the nested question), delivered and
    // freed; the nested one's cancellation is still owed to its asker.
    s.request_text_write(who(2, 1), Some(5), parent, 0, b"x", true)
        .unwrap();
    s.answer(5, parent, 0, 0).unwrap();
    assert!(s.take_delivery(1).is_some());
    s.delivered(parent).unwrap();
    assert!(!s.is_request(parent));
    assert!(s.is_request(nested));
    // An unrelated plugin stops: the owed event stays.
    s.owner_stopped(77).unwrap();
    assert!(s.is_request(nested), "its cancellation is still owed");
    assert_eq!(s.take_delivery(9).unwrap().outcome, 1);
}

#[test]
fn request_cleanup_on_a_busy_store_is_busy_and_retried() {
    use paperpaw_native::host_glue::ERR_BUSY;
    let s = store();
    let id = ask(&s, 1, 1, &q(0, 0, ""));
    open(&s, 1, 1, id, 5);
    let other = s
        .ask(who(2, 1), Some(7), &q(0, 0, ""), false, None, false)
        .unwrap()
        .0;
    {
        let _busy = s.inner.borrow_mut();
        assert_eq!(s.owner_stopped(1), Err(ERR_BUSY));
        assert_eq!(s.dialog_closed(5), Err(ERR_BUSY));
        assert_eq!(s.asker_instance_closed(7, &mut |_| ()), Err(ERR_BUSY));
        assert_eq!(s.dialog_failed(other), Err(ERR_BUSY));
        assert_eq!(s.delivered(id), Err(ERR_BUSY));
        assert_eq!(
            s.callback_returned(who(2, 1), false, &mut |_| ()),
            Err(ERR_BUSY)
        );
    }
    assert_eq!(s.request_count(), 2, "nothing changed");
    s.dialog_closed(5).unwrap();
    let mut cancelled = Vec::new();
    s.asker_instance_closed(7, &mut |c| cancelled.push(c.request))
        .unwrap();
    assert_eq!(cancelled, [other]);
    s.owner_stopped(1).unwrap();
    assert!(!s.is_request(id));
}

#[test]
fn kept_questions_are_charged_to_a_pool_and_freed_with_their_request() {
    let s = UiStore::new(Limits::X4);
    let big = Question {
        kind: 1,
        flags: 0,
        remote: false,
        title: &"t".repeat(256),
        text: &"x".repeat(1_024),
        choices: &std::vec!["c".repeat(63); 32].join("\n"),
    };
    let size = 256 + 1_024 + 32 * 63 + 31;
    assert!(size <= 3_328);
    s.ask(who(1, 1), None, &big, false, None, false).unwrap();
    assert_eq!(s.inner.borrow().question_memory, size);
    // A second one of that size does not fit the 4 KiB pool; nothing changed.
    assert_eq!(
        s.ask(who(2, 1), None, &big, false, None, false).err(),
        Some(ERR_FULL)
    );
    assert_eq!(s.request_count(), 1);
    assert_eq!(s.inner.borrow().question_memory, size);
    // A small one fits, and freeing everything returns the pool.
    s.ask(who(2, 1), None, &q(0, 0, ""), false, None, false)
        .unwrap();
    s.owner_stopped(1).unwrap();
    s.owner_stopped(2).unwrap();
    assert_eq!(s.inner.borrow().question_memory, 0);
}

#[test]
fn the_x4_secret_pool_is_8192_bytes_and_their_chunks_no_more() {
    let s = UiStore::new(Limits::X4);
    let id = s
        .ask(who(1, 1), None, &q(0, SECRET, ""), true, None, false)
        .unwrap()
        .0;
    for at in 0..SECRET_TEXT_MAX {
        s.request_text_write(who(1, 1), None, id, at, b"p", false)
            .unwrap();
    }
    assert_eq!(s.inner.borrow().secret_memory, 8_192 + 32 * 24);
    assert_eq!(s.inner.borrow().secret_memory, Limits::X4.secret_memory);
    // A second secret text finds the pool full (`ERR_IO`), changing nothing.
    let second = s
        .ask(who(2, 1), None, &q(0, SECRET, ""), true, None, false)
        .unwrap()
        .0;
    assert_eq!(
        s.request_text_write(who(2, 1), None, second, 0, b"x", false),
        Err(ERR_IO)
    );
}

#[test]
fn an_over_cap_ask_is_full_and_allocates_nothing() {
    use crate::native_host::shell::tests::FAIL_NTH;
    // Request rows: at the cap, the first allocation is armed to fail; the
    // answer is still `ERR_FULL` because nothing is copied before the check.
    let s = UiStore::new(Limits::X4);
    for owner in 1..=Limits::X4.requests_kept as u64 {
        ask(&s, owner, 1, &q(0, 0, ""));
    }
    FAIL_NTH.with(|n| n.set(1));
    let got = s.ask(who(99, 1), None, &q(0, 0, ""), false, None, false);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(got.err(), Some(ERR_FULL));
    assert_eq!(s.request_count(), Limits::X4.requests_kept);

    // Question pool: a question that does not fit is `ERR_FULL`, not
    // `ERR_NO_MEMORY`, with the allocator refusing.
    let s = UiStore::new(Limits::X4);
    let big = Question {
        kind: 1,
        flags: 0,
        remote: false,
        title: &"t".repeat(256),
        text: &"x".repeat(1_024),
        choices: &std::vec!["c".repeat(63); 32].join("\n"),
    };
    s.ask(who(1, 1), None, &big, false, None, false).unwrap();
    let before = s.inner.borrow().question_memory;
    FAIL_NTH.with(|n| n.set(1));
    let got = s.ask(who(2, 1), None, &big, false, None, false);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(got.err(), Some(ERR_FULL));
    assert_eq!(s.inner.borrow().question_memory, before);
    assert_eq!(s.request_count(), 1);
}

#[test]
fn a_restart_that_does_not_fit_beside_the_old_text_frees_the_old_one_first() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let s = UiStore::new(Limits::X4);
    let id = ask(&s, 1, 1, &q(0, 0, ""));
    s.request_text_write(who(1, 1), None, id, 0, &[b'a'; 1_000], false)
        .unwrap();
    let old = s.inner.borrow().text_memory;
    assert!(old >= 1_000);
    // 1,000 + 1,000 would peak over the 2 KiB pool: the old text goes first,
    // the restart fits, and the pool never held both.
    s.request_text_write(who(1, 1), None, id, 0, &[b'b'; 1_100], true)
        .unwrap();
    {
        let inner = s.inner.borrow();
        assert_eq!(inner.requests[0].text.bytes, 1_100);
        assert!(inner.text_memory <= Limits::X4.text_memory);
    }
    // A refused allocation after that leaves the text started over and empty,
    // and the pool charge is gone with it.
    FAIL_NTH.with(|n| n.set(1));
    let got = s.request_text_write(who(1, 1), None, id, 0, &[b'c'; 1_100], true);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(got, Err(ERR_NO_MEMORY));
    let inner = s.inner.borrow();
    assert_eq!(inner.requests[0].text.bytes, 0);
    assert_eq!(inner.text_memory, 0);
}

#[test]
fn a_dialog_reads_its_question_while_it_opens_and_a_failed_open_cancels_it() {
    let s = store();
    let id = ask(&s, 1, 1, &q(0, 0, ""));
    let mut started = Vec::new();
    s.callback_returned(who(1, 1), false, &mut |start| started.push(start.request))
        .unwrap();
    assert_eq!(started, [id]);
    // Before the open ran, the dialog has no question to read.
    assert_eq!(s.question(7, id, |_| ()), Err(ERR_STATE));
    // Named open for the instance about to be made: its `screen_opened`
    // reads the question, inside the open.
    s.dialog_opening(id, 7).unwrap();
    assert_eq!(s.question(7, id, |shown| shown.kind), Ok(0));
    // Another instance does not (the request is the dialog's alone).
    assert_eq!(s.question(8, id, |_| ()), Err(ERR_STATE));
    // The open made it: told the same instance, nothing changes; another
    // instance (the stack made a screen before it) takes it over.
    s.dialog_opened(id, 7).unwrap();
    s.dialog_opened(id, 9).unwrap();
    assert_eq!(s.question(9, id, |shown| shown.kind), Ok(0));
    // An open that did not come cancels what was opened for it.
    let second = ask(&s, 2, 1, &q(0, 0, ""));
    s.callback_returned(who(2, 1), false, &mut |_| {}).unwrap();
    s.dialog_opening(second, 11).unwrap();
    s.dialog_failed(second).unwrap();
    assert_eq!(s.question(11, second, |_| ()), Err(ERR_STATE));
}
