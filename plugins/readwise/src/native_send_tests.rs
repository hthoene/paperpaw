//! The sending (`native_send`) over a world in memory: the data folder and the
//! card as volumes, a network that records what is sent, a token kept or not.
//! The cases are those interface 6 was tested on (`tests/send.rs` and
//! `tests/sending.rs` of its kernel runs), now without a kernel.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::fail_alloc::sweep;
use crate::native_send::{
    BODY, Done, REQUEST_TOKEN, Secret, Sender, Start, Step, Tell, WAITING, Wire, add_waiting,
    for_each_line, waiting,
};
use crate::test_volume::Mem;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::files::Volume;

const NOTES: &str = "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
    > Curiouser and curiouser!\n>\n> 12 %\n\n\
    > Who are you?\n>\n> 40 %\n\nThe caterpillar asks.\n\n## Bookmarks\n\n- 50 %: x\n";
const NO_HIGHLIGHTS: &str = "# Title\n\nAuthor\n\n## Bookmarks\n\n- 5 %: x\n";

/// What one `post` sent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Post {
    token: String,
    body: String,
}

struct World {
    data: Mem,
    card: Mem,
    kept: Option<String>,
    network: bool,
    posts: Vec<Post>,
    cancelled: usize,
    next: u32,
}

impl World {
    fn new() -> Self {
        Self {
            data: Mem::default(),
            card: Mem::default(),
            kept: Some("kept-token".to_string()),
            network: true,
            posts: Vec::new(),
            cancelled: 0,
            next: 0,
        }
    }

    fn notes(&self, path: &str, text: &str) {
        self.card.write(path, text.as_bytes()).unwrap();
    }

    fn id(&self) -> RequestId {
        RequestId::new(self.next).unwrap()
    }

    fn done(&self, status: u32) -> Done {
        Done::new(self.id(), REQUEST_TOKEN, Reason::Ok, status)
    }

    fn waiting(&self) -> Vec<String> {
        waiting(&self.data).unwrap()
    }
}

impl Wire for World {
    fn data(&self) -> &dyn Volume {
        &self.data
    }

    fn notes(&self) -> &dyn Volume {
        &self.card
    }

    fn token(&mut self) -> Result<Option<Secret>, Error> {
        self.kept.as_deref().map(Secret::new).transpose()
    }

    fn network(&mut self) -> Result<(), Error> {
        if self.network {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }

    fn post(&mut self, token: &Secret) -> Result<RequestId, Error> {
        let body = String::from_utf8(self.data.get(BODY).unwrap()).unwrap();
        self.posts.push(Post {
            token: token.as_str().to_string(),
            body,
        });
        self.next += 1;
        Ok(self.id())
    }

    fn cancel(&mut self) {
        self.cancelled += 1;
    }
}

fn export(w: &mut World, sender: &mut Sender, path: &str) -> Step {
    sender.exported(w, path).unwrap()
}

#[test]
fn highlights_are_sent_from_a_file_and_leave_the_list_when_taken() {
    let mut w = World::new();
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), Step::Quiet);
    assert!(s.busy());
    assert_eq!(w.posts.len(), 1);
    assert_eq!(w.posts[0].token, "kept-token");
    assert!(
        w.posts[0]
            .body
            .starts_with("{\"highlights\":[{\"text\":\"Curiouser and curiouser!\"")
    );
    assert!(w.posts[0].body.contains("},{\"text\":\"Who are you?\""));
    assert!(
        w.posts[0]
            .body
            .contains("\"note\":\"The caterpillar asks.\"")
    );
    assert!(w.posts[0].body.ends_with("}]}"));
    // It waits until Readwise has taken it.
    assert_eq!(w.waiting(), ["Notes/a.md"]);
    let done = w.done(200);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Told(Tell::Sent(2)));
    assert!(w.waiting().is_empty());
    assert!(!s.busy());
    assert!(w.data.get(BODY).is_none(), "the body is cleaned away");
}

#[test]
fn notes_of_any_length_are_sent_in_small_writes() {
    let mut w = World::new();
    let mut text = String::from("# T\n\nA\n\n## Highlights\n\n");
    for i in 0..300 {
        text.push_str(&alloc::format!(
            "> highlight number {i} \"quoted\"\n>\n> {} %\n\n",
            i % 100
        ));
    }
    w.notes("Notes/long.md", &text);
    let mut s = Sender::new();
    export(&mut w, &mut s, "Notes/long.md");
    assert_eq!(w.posts[0].body.matches("\"text\"").count(), 300);
    assert!(*w.data.longest.borrow() <= paperpaw_sdk::native::storage::MAX_DATA);
    let done = w.done(201);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Told(Tell::Sent(300)));
}

#[test]
fn what_fails_waits_and_goes_later() {
    let mut w = World::new();
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    export(&mut w, &mut s, "Notes/a.md");
    let done = w.done(503);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Told(Tell::Failed));
    assert_eq!(w.waiting(), ["Notes/a.md"]);
    // By itself later, it tells only what went well.
    assert_eq!(s.start(&mut w, Start::Retry, None).unwrap(), Step::Quiet);
    assert_eq!(w.posts.len(), 2);
    let done = w.done(500);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Quiet);
    assert_eq!(s.start(&mut w, Start::Retry, None).unwrap(), Step::Quiet);
    let done = w.done(200);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Told(Tell::Sent(2)));
    assert!(w.waiting().is_empty());
}

#[test]
fn no_answer_ends_the_sending_and_the_files_wait() {
    for reason in [
        Reason::NoNetwork,
        Reason::Dns,
        Reason::Tls,
        Reason::Timeout,
        Reason::Refused,
        Reason::Offline,
        Reason::Saving,
        Reason::NetworkOff,
        Reason::Other(99),
    ] {
        let mut w = World::new();
        w.notes("a.md", NOTES);
        w.notes("b.md", NOTES);
        let mut s = Sender::new();
        export(&mut w, &mut s, "a.md");
        export(&mut w, &mut s, "b.md");
        let done = Done::new(w.id(), REQUEST_TOKEN, reason, 0);
        assert_eq!(
            s.done(&mut w, &done).unwrap(),
            Step::Told(Tell::Unreachable),
            "{reason:?}"
        );
        assert_eq!(w.posts.len(), 1, "no second file after a failure to reach");
        assert_eq!(w.waiting(), ["a.md", "b.md"]);
        assert!(!s.busy());
    }
}

#[test]
fn the_end_of_the_run_stops_it_quietly_when_it_ran_by_itself() {
    let mut w = World::new();
    w.notes("a.md", NOTES);
    add_waiting(&w.data, "a.md").unwrap();
    let mut s = Sender::new();
    s.start(&mut w, Start::Retry, None).unwrap();
    let done = Done::new(w.id(), REQUEST_TOKEN, Reason::WorkEnded, 0);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Quiet);
    assert!(!s.busy());
    assert_eq!(w.waiting(), ["a.md"]);
    assert_eq!(w.posts.len(), 1);
}

#[test]
fn the_token_is_asked_for_when_there_is_none_and_only_by_an_export() {
    let mut w = World::new();
    w.kept = None;
    w.notes("a.md", NOTES);
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "a.md"), Step::NeedsToken);
    assert!(w.posts.is_empty());
    assert_eq!(s.start(&mut w, Start::Retry, None).unwrap(), Step::Quiet);
    // Typed: sent with it directly, though it is not kept (yet).
    let typed = Secret::new("typed").unwrap();
    assert_eq!(
        s.start(&mut w, Start::Export, Some(typed)).unwrap(),
        Step::Quiet
    );
    assert_eq!(w.posts[0].token, "typed");
}

#[test]
fn a_refused_token_is_let_go_until_another_is_kept_or_entered() {
    let mut w = World::new();
    w.notes("a.md", NOTES);
    let mut s = Sender::new();
    export(&mut w, &mut s, "a.md");
    let done = w.done(401);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Told(Tell::BadToken));
    assert_eq!(w.waiting(), ["a.md"]);
    // The refused one is not tried again by itself, and an export asks anew.
    assert_eq!(s.start(&mut w, Start::Retry, None).unwrap(), Step::Quiet);
    assert_eq!(
        s.start(&mut w, Start::Export, None).unwrap(),
        Step::NeedsToken
    );
    assert_eq!(w.posts.len(), 1);
    // Another token kept is sent by itself.
    w.kept = Some("new".to_string());
    s.start(&mut w, Start::Retry, None).unwrap();
    assert_eq!(w.posts[1].token, "new");
    let done = w.done(403);
    // A refusal is told only when the reader started the sending.
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Quiet);
    // The same token entered anew is sent again.
    let typed = Secret::new("new").unwrap();
    s.start(&mut w, Start::Export, Some(typed)).unwrap();
    assert_eq!(w.posts.len(), 3);
    let done = w.done(200);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Told(Tell::Sent(2)));
}

#[test]
fn deleted_notes_and_notes_without_highlights_wait_no_more() {
    let mut w = World::new();
    w.notes("empty.md", NO_HIGHLIGHTS);
    add_waiting(&w.data, "gone.md").unwrap();
    let mut s = Sender::new();
    assert_eq!(
        export(&mut w, &mut s, "empty.md"),
        Step::Told(Tell::Nothing)
    );
    assert!(w.waiting().is_empty());
    assert!(w.posts.is_empty());
}

#[test]
fn a_file_that_cannot_be_read_counts_as_failed_and_the_others_go_on() {
    let mut w = World::new();
    w.notes("good.md", NOTES);
    // A file whose notes are not on the card to read (a size, then a refusal).
    struct Broken(Mem);
    impl Volume for Broken {
        fn size(&self, name: &str) -> Result<u64, Error> {
            self.0.size(name)
        }
        fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
            if name == "bad.md" {
                return Err(Error::Io);
            }
            self.0.read_at(name, at, out)
        }
        fn write(&self, n: &str, d: &[u8]) -> Result<(), Error> {
            self.0.write(n, d)
        }
        fn append(&self, n: &str, d: &[u8]) -> Result<(), Error> {
            self.0.append(n, d)
        }
        fn rename(&self, a: &str, b: &str) -> Result<(), Error> {
            self.0.rename(a, b)
        }
        fn remove(&self, n: &str) -> Result<(), Error> {
            self.0.remove(n)
        }
        fn sync(&self) -> Result<(), Error> {
            Ok(())
        }
    }
    w.notes("bad.md", NOTES);
    let card = core::mem::take(&mut w.card);
    let broken = Broken(card);
    struct Split<'a>(&'a mut World, &'a Broken);
    impl Wire for Split<'_> {
        fn data(&self) -> &dyn Volume {
            self.0.data()
        }
        fn notes(&self) -> &dyn Volume {
            self.1
        }
        fn token(&mut self) -> Result<Option<Secret>, Error> {
            self.0.token()
        }
        fn network(&mut self) -> Result<(), Error> {
            self.0.network()
        }
        fn post(&mut self, token: &Secret) -> Result<RequestId, Error> {
            self.0.post(token)
        }
        fn cancel(&mut self) {
            self.0.cancel();
        }
    }
    add_waiting(&w.data, "bad.md").unwrap();
    add_waiting(&w.data, "good.md").unwrap();
    let mut s = Sender::new();
    let mut wire = Split(&mut w, &broken);
    s.start(&mut wire, Start::Export, None).unwrap();
    assert_eq!(wire.0.posts.len(), 1, "the good file goes on");
    let done = wire.0.done(200);
    assert_eq!(s.done(&mut wire, &done).unwrap(), Step::Told(Tell::Failed));
    assert_eq!(wire.0.waiting(), ["bad.md"]);
}

#[test]
fn nothing_is_sent_without_a_network_and_the_files_wait() {
    let mut w = World::new();
    w.network = false;
    w.notes("a.md", NOTES);
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "a.md"), Step::Told(Tell::NoNetwork));
    assert!(w.posts.is_empty());
    assert_eq!(w.waiting(), ["a.md"]);
    assert!(!s.busy());
    assert_eq!(s.start(&mut w, Start::Retry, None).unwrap(), Step::Quiet);
}

#[test]
fn an_export_while_sending_comes_along() {
    let mut w = World::new();
    w.notes("a.md", NOTES);
    w.notes("b.md", NOTES);
    let mut s = Sender::new();
    export(&mut w, &mut s, "a.md");
    export(&mut w, &mut s, "b.md");
    assert_eq!(w.posts.len(), 1);
    let done = w.done(200);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Quiet);
    assert_eq!(w.posts.len(), 2);
    let done = w.done(200);
    assert_eq!(s.done(&mut w, &done).unwrap(), Step::Told(Tell::Sent(4)));
    assert!(w.waiting().is_empty());
}

#[test]
fn other_requests_and_other_ends_are_ignored_and_a_cancel_ends_it() {
    let mut w = World::new();
    w.notes("a.md", NOTES);
    let mut s = Sender::new();
    export(&mut w, &mut s, "a.md");
    let other = Done::new(RequestId::new(77).unwrap(), REQUEST_TOKEN, Reason::Ok, 200);
    assert_eq!(s.done(&mut w, &other).unwrap(), Step::Quiet);
    let token = Done::new(w.id(), REQUEST_TOKEN + 1, Reason::Ok, 200);
    assert_eq!(s.done(&mut w, &token).unwrap(), Step::Quiet);
    assert!(s.busy());
    s.cancel(&mut w);
    assert!(!s.busy());
    assert_eq!(w.cancelled, 1);
    assert!(w.data.get(BODY).is_none());
    // Its end, when it comes, is ignored.
    let cancelled = Done::new(w.id(), REQUEST_TOKEN, Reason::Cancelled, 0);
    assert_eq!(s.done(&mut w, &cancelled).unwrap(), Step::Quiet);
    assert_eq!(w.waiting(), ["a.md"]);
}

#[test]
fn keeps_what_waits() {
    let w = World::new();
    assert!(w.waiting().is_empty());
    add_waiting(&w.data, "Notes/a.md").unwrap();
    add_waiting(&w.data, "Notes/b.md").unwrap();
    add_waiting(&w.data, "Notes/a.md").unwrap();
    assert_eq!(w.waiting(), ["Notes/b.md", "Notes/a.md"]);
    assert!(w.data.get("waiting.txt.new").is_none());
}

#[test]
fn a_long_list_is_taken_a_few_at_a_time_and_none_is_lost() {
    let mut w = World::new();
    for i in 0..40 {
        w.notes(&alloc::format!("n{i}.md"), NOTES);
        add_waiting(&w.data, &alloc::format!("n{i}.md")).unwrap();
    }
    assert_eq!(w.waiting().len(), crate::native_send::MOST_WAITING);
    let mut s = Sender::new();
    let mut rounds = 0;
    while !w.waiting().is_empty() {
        rounds += 1;
        assert!(rounds <= 3, "every sending takes its share");
        s.start(&mut w, Start::Retry, None).unwrap();
        while s.busy() {
            let done = w.done(200);
            s.done(&mut w, &done).unwrap();
        }
    }
    assert_eq!(rounds, 2);
    assert!(w.waiting().is_empty());
    assert_eq!(w.posts.len(), 40);
}

#[test]
fn a_list_that_cannot_be_written_is_left_as_it_was() {
    let w = World::new();
    add_waiting(&w.data, "a.md").unwrap();
    // A path longer than a write is written in pieces and read whole.
    let long = "x".repeat(3000);
    add_waiting(&w.data, &long).unwrap();
    assert_eq!(w.waiting(), ["a.md", long.as_str()]);
}

#[test]
fn lines_are_read_as_the_storage_reader_read_them() {
    let mem = Mem::with(WAITING, b"\xef\xbb\xbfone\r\ntwo\n\nbad\xffbyte\nlast");
    let mut got = Vec::new();
    for_each_line(&mem, WAITING, &mut |line| {
        got.push(line.to_string());
        Ok(ControlFlow::Continue(()))
    })
    .unwrap();
    assert_eq!(got, ["one", "two", "", "bad\u{fffd}byte", "last"]);
    // A long line is cut, and the reading can be stopped.
    let long = alloc::vec![b'a'; 10_000];
    let mem = Mem::with("l", &long);
    let mut lens = Vec::new();
    for_each_line(&mem, "l", &mut |line| {
        lens.push(line.len());
        Ok(ControlFlow::Break(()))
    })
    .unwrap();
    assert_eq!(lens, [4096]);
    assert_eq!(
        for_each_line(&mem, "missing", &mut |_| Ok(ControlFlow::Continue(()))),
        Err(Error::NotFound)
    );
}

/// A world that allocates nothing of its own: a fixed list and notes, writes
/// counted and dropped, so a sweep refuses only the sending's allocations.
struct Sink;

const SINK_LIST: &[u8] = b"a.md\n";

impl Volume for Sink {
    fn size(&self, _: &str) -> Result<u64, Error> {
        Ok(NOTES.len() as u64)
    }

    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        let source = if name == WAITING {
            SINK_LIST
        } else {
            NOTES.as_bytes()
        };
        let from = usize::try_from(at)
            .map_err(|_| Error::Invalid)?
            .min(source.len());
        let n = out.len().min(source.len() - from);
        out[..n].copy_from_slice(&source[from..from + n]);
        Ok(n)
    }

    fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn append(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn rename(&self, _: &str, _: &str) -> Result<(), Error> {
        Ok(())
    }

    fn remove(&self, _: &str) -> Result<(), Error> {
        Ok(())
    }

    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }
}

impl Wire for Sink {
    fn data(&self) -> &dyn Volume {
        self
    }

    fn notes(&self) -> &dyn Volume {
        self
    }

    fn token(&mut self) -> Result<Option<Secret>, Error> {
        Secret::new("token").map(Some)
    }

    fn network(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn post(&mut self, _: &Secret) -> Result<RequestId, Error> {
        Ok(RequestId::new(1).unwrap())
    }

    fn cancel(&mut self) {}
}

#[test]
fn refused_memory_ends_in_no_memory_never_an_abort() {
    sweep(|| {
        let mut sink = Sink;
        let mut sender = Sender::new();
        sender.start(&mut sink, Start::Export, None)?;
        let done = Done::new(RequestId::new(1).unwrap(), REQUEST_TOKEN, Reason::Ok, 200);
        sender.done(&mut sink, &done)
    });
}

#[test]
fn the_token_is_wiped_and_never_shown() {
    let secret = Secret::new("abc").unwrap();
    assert_eq!(alloc::format!("{secret:?}"), "Secret(..)");
    let header = crate::native_send::authorization(&secret).unwrap();
    assert_eq!(header.as_str(), "Token abc");
}
