//! The native sync engine's decisions, on the scenarios interface 6's
//! `tests/sync.rs` runs through a kernel (the old tests stay the reference
//! until activation): what is fetched and sent, what is held back, offered,
//! declined and dismissed, and what an answer for a book left does.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_plugin_kosync::native_model::Remote;
use paperpaw_plugin_kosync::native_sync::{
    Action, Answer, Answered, Ctx, Engine, MOST_DISMISSED, Notice, PULL_EVERY_MS, PUSH_EVERY_MS,
    Position,
};

const ME: &str = "me";

fn ctx(now_ms: u64) -> Ctx {
    Ctx::new(now_ms)
}

fn offline(now_ms: u64) -> Ctx {
    let mut c = ctx(now_ms);
    c.online = false;
    c
}

fn at(path: &str, progress: f64) -> Position {
    Position::new(path, (1, 0), progress).unwrap()
}

fn remote(progress: f64, device_id: &str) -> Remote {
    Remote {
        progress,
        device_id: Some(device_id.into()),
        device: Some("Other".into()),
        xpointer: Some("/body/x".into()),
    }
}

fn answer<'a>(path: &'a str, local: f64, manual: bool, r: Option<&'a Remote>) -> Answer<'a> {
    Answer::new(path, manual, local, if r.is_some() { 200 } else { 500 }, r)
}

fn fetches(out: &[Action]) -> Vec<(&str, bool)> {
    out.iter()
        .filter_map(|a| match a {
            Action::Fetch { path, manual, .. } => Some((path.as_str(), *manual)),
            _ => None,
        })
        .collect()
}

fn sends(out: &[Action]) -> Vec<&Position> {
    out.iter()
        .filter_map(|a| match a {
            Action::Send(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn opened(engine: &mut Engine, now: u64, path: &str, progress: f64) -> Vec<Action> {
    let mut out = Vec::new();
    engine
        .book_opened(ctx(now), at(path, progress), &mut out)
        .unwrap();
    out
}

#[test]
fn opening_a_book_fetches_and_sends_nothing_yet() {
    let mut e = Engine::new(ME).unwrap();
    let out = opened(&mut e, 0, "a.epub", 0.1);
    assert_eq!(fetches(&out), [("a.epub", false)]);
    assert!(sends(&out).is_empty());
}

#[test]
fn a_position_goes_after_the_server_says_nothing_is_further() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let mut out = Vec::new();
    // The page turned while the fetch is out: held back.
    e.page_turned(ctx(1_000), at("a.epub", 0.2), &mut out)
        .unwrap();
    assert!(sends(&out).is_empty());
    let r = remote(0.1, "other");
    e.pulled(
        ctx(2_000),
        &answer("a.epub", 0.1, false, Some(&r)),
        &mut out,
    )
    .unwrap();
    assert_eq!(sends(&out), [&at("a.epub", 0.2)]);
}

#[test]
fn a_further_position_is_offered_and_nothing_is_sent_until_the_user_chose() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let mut out = Vec::new();
    e.page_turned(ctx(1_000), at("a.epub", 0.1), &mut out)
        .unwrap();
    let r = remote(0.6, "other");
    e.pulled(
        ctx(2_000),
        &answer("a.epub", 0.1, false, Some(&r)),
        &mut out,
    )
    .unwrap();
    assert!(sends(&out).is_empty());
    let offer = out
        .iter()
        .find_map(|a| match a {
            Action::Offer(o) => Some(o),
            _ => None,
        })
        .unwrap();
    assert_eq!(offer.path, "a.epub");
    assert_eq!(offer.percentage, "0.6");
    assert_eq!(offer.xpointer, "/body/x");
    assert_eq!(offer.device, "Other");
    // Staying keeps this device's position and sends it, forced.
    let mut out = Vec::new();
    e.answered(
        ctx(3_000),
        &Answered {
            path: "a.epub",
            percentage: "0.6",
            went: false,
            stay: true,
        },
        &mut out,
    )
    .unwrap();
    assert_eq!(sends(&out), [&at("a.epub", 0.1)]);
    // The same position is not offered again.
    let mut out = Vec::new();
    e.pulled(
        ctx(70_000),
        &answer("a.epub", 0.1, false, Some(&r)),
        &mut out,
    )
    .unwrap();
    assert!(!out.iter().any(|a| matches!(a, Action::Offer(_))));
}

#[test]
fn leaving_the_offer_without_a_choice_sends_nothing_and_asks_no_more() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let mut out = Vec::new();
    let r = remote(0.6, "other");
    e.pulled(
        ctx(1_000),
        &answer("a.epub", 0.1, false, Some(&r)),
        &mut out,
    )
    .unwrap();
    out.clear();
    e.answered(
        ctx(2_000),
        &Answered {
            path: "a.epub",
            percentage: "0.6",
            went: false,
            stay: false,
        },
        &mut out,
    )
    .unwrap();
    assert!(out.is_empty());
    // A page turns and the next fetch says the same: held, not asked.
    e.page_turned(ctx(100_000), at("a.epub", 0.15), &mut out)
        .unwrap();
    assert!(sends(&out).is_empty());
    out.clear();
    e.pulled(
        ctx(100_500),
        &answer("a.epub", 0.15, false, Some(&r)),
        &mut out,
    )
    .unwrap();
    assert!(out.is_empty());
    // A manual sync offers it again.
    e.pulled(
        ctx(101_000),
        &answer("a.epub", 0.15, true, Some(&r)),
        &mut out,
    )
    .unwrap();
    assert!(out.iter().any(|a| matches!(a, Action::Offer(_))));
}

#[test]
fn a_position_of_this_device_or_not_further_is_not_offered() {
    for (remote_progress, device, local, offered) in [
        (0.5, "other", 0.5, false),
        (0.501, "other", 0.5, false),
        (0.6, ME, 0.3, false),
        (0.6, "other", 0.3, true),
        // The book starts here anew: this device's own position is offered.
        (0.6, ME, 0.0, true),
    ] {
        let mut e = Engine::new(ME).unwrap();
        opened(&mut e, 0, "a.epub", local);
        let mut out = Vec::new();
        let r = remote(remote_progress, device);
        e.pulled(ctx(1), &answer("a.epub", local, false, Some(&r)), &mut out)
            .unwrap();
        assert_eq!(
            out.iter().any(|a| matches!(a, Action::Offer(_))),
            offered,
            "{remote_progress} {device} {local}"
        );
    }
}

#[test]
fn nothing_is_sent_after_a_failed_fetch_until_one_succeeds() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let mut out = Vec::new();
    e.pulled(ctx(1_000), &answer("a.epub", 0.1, false, None), &mut out)
        .unwrap();
    assert!(out.is_empty(), "no notice for a sync by itself");
    // The progress goes at most once a minute, and not before the server's is read.
    e.page_turned(ctx(PUSH_EVERY_MS + 1), at("a.epub", 0.2), &mut out)
        .unwrap();
    assert_eq!(fetches(&out), [("a.epub", false)]);
    assert!(sends(&out).is_empty());
    out.clear();
    let r = remote(0.1, "other");
    e.pulled(
        ctx(PUSH_EVERY_MS + 2),
        &answer("a.epub", 0.2, false, Some(&r)),
        &mut out,
    )
    .unwrap();
    assert_eq!(sends(&out), [&at("a.epub", 0.2)]);
}

#[test]
fn a_sync_asked_for_says_what_went_wrong() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let mut out = Vec::new();
    e.pulled(ctx(1), &answer("a.epub", 0.1, true, None), &mut out)
        .unwrap();
    assert_eq!(out, [Action::Notice(Notice::Failed(500))]);
    out.clear();
    let unreadable = Answer::new("a.epub", true, 0.1, 200, None);
    e.pulled(ctx(2), &unreadable, &mut out).unwrap();
    assert_eq!(out, [Action::Notice(Notice::Unreadable(200))]);
    out.clear();
    let r = remote(0.1, "other");
    e.pulled(ctx(3), &answer("a.epub", 0.1, true, Some(&r)), &mut out)
        .unwrap();
    assert_eq!(out, [Action::Notice(Notice::UpToDate(200))]);
}

#[test]
fn the_same_book_is_fetched_again_only_after_a_minute() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let out = opened(&mut e, PULL_EVERY_MS - 1, "a.epub", 0.1);
    assert!(fetches(&out).is_empty());
    let out = opened(&mut e, PULL_EVERY_MS + 1, "a.epub", 0.1);
    assert_eq!(fetches(&out), [("a.epub", false)]);
}

#[test]
fn offline_keeps_the_progress_and_sends_it_once_allowed() {
    let mut e = Engine::new(ME).unwrap();
    let mut out = Vec::new();
    e.book_opened(offline(0), at("a.epub", 0.4), &mut out)
        .unwrap();
    e.book_opened(offline(10), at("b.epub", 0.1), &mut out)
        .unwrap();
    assert!(out.is_empty());
    // Online again: the book left is fetched first (its position goes only if
    // no other device is further), and the open one's own fetch.
    e.allowed(ctx(1_000), &mut out).unwrap();
    assert_eq!(fetches(&out), [("a.epub", false), ("b.epub", false)]);
    assert!(sends(&out).is_empty());
    out.clear();
    let further = remote(0.9, "other");
    e.pulled(
        ctx(1_100),
        &answer("a.epub", 0.4, false, Some(&further)),
        &mut out,
    )
    .unwrap();
    assert!(
        out.is_empty(),
        "a further position elsewhere is not overwritten"
    );
    let same = remote(0.4, "other");
    let mut e2 = Engine::new(ME).unwrap();
    e2.book_opened(offline(0), at("a.epub", 0.4), &mut out)
        .unwrap();
    e2.book_opened(offline(10), at("b.epub", 0.1), &mut out)
        .unwrap();
    e2.allowed(ctx(1_000), &mut out).unwrap();
    out.clear();
    e2.pulled(
        ctx(1_100),
        &answer("a.epub", 0.4, false, Some(&same)),
        &mut out,
    )
    .unwrap();
    assert_eq!(sends(&out), [&at("a.epub", 0.4)]);
}

#[test]
fn a_position_held_for_a_fetch_still_goes_after_another_book_is_opened() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let mut out = Vec::new();
    e.page_turned(ctx(500), at("a.epub", 0.3), &mut out)
        .unwrap();
    let out = opened(&mut e, 1_000, "b.epub", 0.0);
    assert!(sends(&out).is_empty());
    let mut out = Vec::new();
    let r = remote(0.2, "other");
    e.pulled(
        ctx(1_500),
        &answer("a.epub", 0.1, false, Some(&r)),
        &mut out,
    )
    .unwrap();
    assert_eq!(sends(&out), [&at("a.epub", 0.3)]);
}

#[test]
fn closing_and_sleeping_send_what_was_not_sent() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let mut out = Vec::new();
    let r = remote(0.1, "other");
    e.pulled(ctx(10), &answer("a.epub", 0.1, false, Some(&r)), &mut out)
        .unwrap();
    out.clear();
    e.page_turned(ctx(20), at("a.epub", 0.2), &mut out).unwrap();
    // Not due (sent a moment ago): the work is declared instead.
    out.clear();
    e.page_turned(ctx(30), at("a.epub", 0.25), &mut out)
        .unwrap();
    assert!(out.contains(&Action::PushLater));
    out.clear();
    e.send_now(ctx(40), &mut out).unwrap();
    assert_eq!(sends(&out), [&at("a.epub", 0.25)]);
    out.clear();
    e.book_closed(ctx(50), at("a.epub", 0.25), &mut out)
        .unwrap();
    assert!(out.is_empty(), "the same position is not sent twice");
}

#[test]
fn a_failed_push_is_sent_again_with_the_next_page() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.1);
    let mut out = Vec::new();
    let r = remote(0.1, "other");
    e.pulled(ctx(10), &answer("a.epub", 0.1, false, Some(&r)), &mut out)
        .unwrap();
    out.clear();
    e.send_now(ctx(20), &mut out).unwrap();
    assert_eq!(sends(&out).len(), 1);
    e.not_sent();
    out.clear();
    e.send_now(ctx(30), &mut out).unwrap();
    assert_eq!(sends(&out).len(), 1);
}

#[test]
fn an_answer_for_a_book_left_meanwhile_sends_its_position_unless_further() {
    for (remote_progress, device, sent) in
        [(0.2, "other", true), (0.9, "other", false), (0.9, ME, true)]
    {
        let mut e = Engine::new(ME).unwrap();
        opened(&mut e, 0, "a.epub", 0.1);
        let mut out = Vec::new();
        e.page_turned(ctx(500), at("a.epub", 0.3), &mut out)
            .unwrap();
        opened(&mut e, 1_000, "b.epub", 0.0);
        let mut out = Vec::new();
        let r = remote(remote_progress, device);
        e.pulled(
            ctx(1_500),
            &answer("a.epub", 0.1, false, Some(&r)),
            &mut out,
        )
        .unwrap();
        assert_eq!(!sends(&out).is_empty(), sent, "{remote_progress} {device}");
        assert!(
            !out.iter().any(|a| matches!(a, Action::Offer(_))),
            "no offer for a closed book"
        );
    }
}

#[test]
fn signed_out_or_switched_off_nothing_goes() {
    let mut e = Engine::new(ME).unwrap();
    let mut out = Vec::new();
    for mutate in [
        |c: &mut Ctx| c.signed_in = false,
        |c: &mut Ctx| c.auto = false,
        |c: &mut Ctx| c.network = false,
    ] {
        let mut c = ctx(0);
        mutate(&mut c);
        e.book_opened(c, at("a.epub", 0.1), &mut out).unwrap();
        e.page_turned(c, at("a.epub", 0.2), &mut out).unwrap();
        e.book_closed(c, at("a.epub", 0.3), &mut out).unwrap();
        e.resumed(c, &mut out).unwrap();
        e.allowed(c, &mut out).unwrap();
        assert!(out.is_empty());
    }
    // *Sync progress* signed out opens the account instead.
    let mut c = ctx(0);
    c.signed_in = false;
    assert!(!e.sync_now(c, &mut out).unwrap());
    assert!(out.is_empty());
}

#[test]
fn a_manual_sync_fetches_and_sends_after_the_answer() {
    let mut e = Engine::new(ME).unwrap();
    opened(&mut e, 0, "a.epub", 0.2);
    let mut out = Vec::new();
    e.pulled(
        ctx(5),
        &answer("a.epub", 0.2, false, Some(&remote(0.2, "other"))),
        &mut out,
    )
    .unwrap();
    out.clear();
    assert!(e.sync_now(ctx(10), &mut out).unwrap());
    assert_eq!(fetches(&out), [("a.epub", true)]);
    out.clear();
    e.pulled(
        ctx(20),
        &answer("a.epub", 0.2, true, Some(&remote(0.2, "other"))),
        &mut out,
    )
    .unwrap();
    assert!(out.contains(&Action::Notice(Notice::UpToDate(200))));
    assert_eq!(sends(&out), [&at("a.epub", 0.2)]);
}

#[test]
fn few_offers_are_remembered() {
    let mut e = Engine::new(ME).unwrap();
    for i in 0..=(MOST_DISMISSED + 2) {
        let path = format!("{i}.epub");
        opened(&mut e, u64::try_from(i).unwrap() * 100_000, &path, 0.1);
        let mut out = Vec::new();
        let r = remote(0.8, "other");
        e.pulled(ctx(1), &answer(&path, 0.1, false, Some(&r)), &mut out)
            .unwrap();
        e.answered(
            ctx(2),
            &Answered {
                path: &path,
                percentage: "0.8",
                went: false,
                stay: false,
            },
            &mut out,
        )
        .unwrap();
    }
}
