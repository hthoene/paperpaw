#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The cable's link on its own: one server, its start told once, answers
//! framed as lines, files refused, holding and its time.
extern crate std;
use super::*;
use std::vec::Vec;

fn lines(link: &CableLink) -> Vec<String> {
    core::iter::from_fn(|| link.next_line()).collect()
}

/// A request the service accepted as `request`, open on the cable as the
/// computer's `n`.
fn opened(link: &CableLink, n: u32, request: u32) -> u32 {
    let (number, _, _) = link.proto(|c| c.begin(n, 0)).unwrap();
    link.keep(Open {
        link: number,
        request,
        path: String::new(),
        received: 0,
    })
    .unwrap();
    number
}

#[test]
fn one_server_listens_and_its_start_is_told_once() {
    let link = CableLink::new();
    assert!(link.listen(7));
    assert!(!link.listen(8), "the cable has one server");
    assert_eq!(link.started(), None, "not before the kernel told it");
    assert_eq!(link.take_start(), Some(7));
    assert_eq!(link.take_start(), None);
    assert_eq!(link.started(), Some(7));
    // A start the service did not take leaves no server.
    let link = CableLink::new();
    link.listen(3);
    link.take_start();
    link.start_refused(3);
    assert_eq!(link.started(), None);
    assert!(link.listen(4), "the cable is free again");
}

#[test]
fn an_answer_goes_out_as_lines_and_its_request_is_gone() {
    let link = CableLink::new();
    link.listen(1);
    opened(&link, 5, 40);
    assert!(link.carries(40));
    assert!(!link.carries(41));
    link.answer(40, 200, "application/json", Some(b"{}"), false)
        .unwrap();
    assert!(!link.carries(40));
    assert_eq!(
        lines(&link),
        [
            "PPT/1 5 200 application/json",
            "PPT/1 5 data 7b7d",
            "PPT/1 5 end"
        ]
    );
    assert_eq!(link.pop_gone(), Some(40));
    assert_eq!(link.pop_gone(), None);
}

#[test]
fn a_file_is_not_sent_over_the_cable() {
    let link = CableLink::new();
    link.listen(1);
    opened(&link, 2, 9);
    assert_eq!(
        link.answer(9, 200, "text/html", None, true),
        Err(ERR_UNSUPPORTED)
    );
    let sent = lines(&link);
    assert_eq!(sent[0], "PPT/1 2 500 text/plain;charset=utf-8");
    assert_eq!(link.pop_gone(), Some(9));
}

#[test]
fn a_close_answers_with_the_hosts_status_or_503() {
    let link = CableLink::new();
    link.listen(1);
    opened(&link, 1, 10);
    opened(&link, 2, 11);
    link.close(10, 503);
    link.close(11, 0);
    let sent = lines(&link);
    assert!(sent.contains(&String::from("PPT/1 1 503 text/plain;charset=utf-8")));
    assert!(sent.contains(&String::from("PPT/1 2 503 text/plain;charset=utf-8")));
    let mut gone = [link.pop_gone(), link.pop_gone()];
    gone.sort_unstable();
    assert_eq!(gone, [Some(10), Some(11)]);
}

#[test]
fn stopping_the_server_answers_what_is_open_and_marks_it_ended() {
    let link = CableLink::new();
    link.listen(1);
    link.take_start();
    opened(&link, 3, 12);
    link.hold(Held {
        n: 4,
        method: String::from("GET"),
        target: String::from("/api/hello"),
        until: 9_000,
    });
    assert!(!link.unlisten(2), "not the cable's server");
    assert!(!link.ended());
    assert!(link.unlisten(1));
    assert!(link.ended());
    assert_eq!(link.started(), None);
    let sent = lines(&link);
    assert!(sent.contains(&String::from("PPT/1 3 503 text/plain;charset=utf-8")));
    assert!(sent.contains(&String::from("PPT/1 4 503 text/plain;charset=utf-8")));
    assert!(!link.carries(12));
    // The stop forgets the server's requests itself: nothing to report.
    assert_eq!(link.pop_gone(), None);
    // A new server clears the mark.
    link.listen(5);
    assert!(!link.ended());
}

#[test]
fn a_held_request_waits_for_the_start_or_its_time() {
    let held = |n, until| Held {
        n,
        method: String::from("GET"),
        target: String::from("/api/hello"),
        until,
    };
    let link = CableLink::new();
    link.hold(held(1, 5_000));
    assert_eq!(link.held_until(), Some(5_000));
    assert!(link.take_held(4_999).is_none(), "still waiting");
    // A newer one replaces it: the older is answered 503.
    link.hold(held(2, 6_000));
    assert_eq!(lines(&link)[0], "PPT/1 1 503 text/plain;charset=utf-8");
    // Started: the held one is handed over.
    link.listen(1);
    link.take_start();
    let taken = link.take_held(5_500).unwrap();
    assert_eq!((taken.n, taken.target.as_str()), (2, "/api/hello"));
    assert_eq!(link.held_until(), None);
    // No server and the time ran out: 503.
    let link = CableLink::new();
    link.hold(held(3, 5_000));
    assert!(link.take_held(5_000).is_none());
    assert_eq!(lines(&link)[0], "PPT/1 3 503 text/plain;charset=utf-8");
    assert_eq!(link.held_until(), None);
}

/// While the answers waiting for the computer leave no room, `respond` is
/// `ERR_BUSY` and the request stays open: the plugin answers again once the
/// computer read.
#[test]
fn a_full_queue_is_busy_and_the_request_stays_open() {
    use crate::io::cable::OUT_MAX;
    let link = CableLink::new();
    link.listen(1);
    for n in 0..OUT_MAX as u32 {
        link.proto(|c| c.refuse(100 + n, 400, "bad")).unwrap();
    }
    opened(&link, 5, 40);
    assert_eq!(
        link.answer(40, 200, "text/plain", Some(b"ok"), false),
        Err(ERR_BUSY)
    );
    assert!(link.carries(40), "still open");
    assert_eq!(link.pop_gone(), None);
    assert_eq!(lines(&link).len(), OUT_MAX * 3);
    link.answer(40, 200, "text/plain", Some(b"ok"), false)
        .unwrap();
    assert_eq!(lines(&link)[1], "PPT/1 5 data 6f6b");
    assert_eq!(link.pop_gone(), Some(40));
}
