//! The sync against a fake AnkiConnect: an in-process stand-in that answers
//! the classic API (version 6) from a small collection, as the add-on does
//! (answers in JSON, `error` null or a message, the key checked in the body),
//! and can be made to cut an answer, ask for another key or be unreachable.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use alloc::string::String;
use alloc::vec::Vec;
use std::format;
use std::string::ToString;

use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::files::Volume;

use crate::native_anki::Body;
use crate::native_cards::{self as store, ANSWERS, CARDS, NEW};
use crate::native_keep::Secret;
use crate::native_study::{self, Empty, Study};
use crate::native_sync::{Account, Done, Finish, Link, Mode, REQUEST_TOKEN, Step, Sync};
use crate::test_volume::Mem;

#[derive(Clone)]
struct FakeCard {
    id: u64,
    deck: &'static str,
    front: String,
    back: String,
    new: bool,
    due: bool,
}

/// The computer's side: AnkiConnect with its collection.
struct Fake {
    vol: Mem,
    key: Option<&'static str>,
    cards: Vec<FakeCard>,
    /// The answers it took: card and ease.
    taken: Vec<(u64, u8)>,
    /// Every request it got, as the body text.
    log: Vec<String>,
    next: u32,
    reply: Option<(Done, Vec<u8>)>,
    /// How the next requests end, instead of an answer.
    reason: Reason,
    /// Answers longer than this are cut (the host's buffer).
    cut_at: usize,
    /// The reply to every request, instead of AnkiConnect's.
    stranger: Option<&'static str>,
    /// Requests of `answerCards` that are answered; later ones fail.
    answers_allowed: usize,
}

impl Fake {
    fn new() -> Self {
        Self {
            vol: Mem::default(),
            key: None,
            cards: Vec::new(),
            taken: Vec::new(),
            log: Vec::new(),
            next: 0,
            reply: None,
            reason: Reason::Ok,
            cut_at: usize::MAX,
            stranger: None,
            answers_allowed: usize::MAX,
        }
    }

    fn card(&mut self, id: u64, deck: &'static str, front: &str, back: &str, new: bool, due: bool) {
        self.cards.push(FakeCard {
            id,
            deck,
            front: front.into(),
            back: back.into(),
            new,
            due,
        });
    }

    fn json_string(text: &str) -> String {
        let mut out = String::from("\"");
        for c in text.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    fn numbers_after(body: &str, marker: &str) -> Vec<u64> {
        let from = body.find(marker).unwrap() + marker.len();
        let to = body[from..].find(']').unwrap() + from;
        body[from..to]
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect()
    }

    /// What AnkiConnect answers `body` with.
    fn answer(&mut self, body: &str) -> String {
        if let Some(key) = self.key
            && !body.contains(&format!("\"key\":\"{key}\""))
        {
            return "{\"result\":null,\"error\":\"valid api key must be provided\"}".into();
        }
        let action = body
            .split("\"action\":\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        match action {
            "deckNames" => {
                let mut names: Vec<&str> = self.cards.iter().map(|c| c.deck).collect();
                names.sort_unstable();
                names.dedup();
                let list: Vec<String> = names.iter().map(|n| Self::json_string(n)).collect();
                format!("{{\"result\":[{}],\"error\":null}}", list.join(","))
            }
            "findCards" => {
                let query = body.split("\"query\":\"").nth(1).unwrap();
                let (due, new) = (query.contains("is:due"), query.contains("is:new"));
                let deck = query
                    .split("deck:\\\"")
                    .nth(1)
                    .map(|rest| rest.split("\\\"").next().unwrap().replace("\\\\_", "_"));
                let ids: Vec<String> = self
                    .cards
                    .iter()
                    .filter(|c| {
                        // A deck's search takes its subdecks too, as Anki's does.
                        deck.as_deref()
                            .is_none_or(|d| c.deck == d || c.deck.starts_with(&format!("{d}::")))
                    })
                    .filter(|c| (due && c.due) || (new && c.new))
                    .map(|c| c.id.to_string())
                    .collect();
                format!("{{\"result\":[{}],\"error\":null}}", ids.join(","))
            }
            "cardsInfo" => {
                let ids = Self::numbers_after(body, "\"cards\":[");
                let infos: Vec<String> = ids
                    .iter()
                    .map(|id| match self.cards.iter().find(|c| c.id == *id) {
                        Some(c) => format!(
                            "{{\"cardId\":{},\"fields\":{{\"Front\":{{\"value\":{},\"order\":0}}}},\"question\":{},\"answer\":{},\"deckName\":{},\"css\":\".card{{}}\",\"interval\":3}}",
                            c.id,
                            Self::json_string(&c.front),
                            Self::json_string(&format!("<style>.card {{ color: black }}</style>{}", c.front)),
                            Self::json_string(&format!(
                                "<style>.card {{ color: black }}</style>{}<hr id=answer>{}",
                                c.front, c.back
                            )),
                            Self::json_string(c.deck),
                        ),
                        None => "{}".into(),
                    })
                    .collect();
                format!("{{\"result\":[{}],\"error\":null}}", infos.join(","))
            }
            "answerCards" => {
                if self.answers_allowed == 0 {
                    return "{\"result\":null,\"error\":\"collection is busy\"}".into();
                }
                self.answers_allowed -= 1;
                let mut results = Vec::new();
                for part in body.split("{\"cardId\":").skip(1) {
                    let id: u64 = part.split(',').next().unwrap().parse().unwrap();
                    let ease: u8 = part
                        .split("\"ease\":")
                        .nth(1)
                        .unwrap()
                        .split('}')
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap();
                    let known = self.cards.iter().any(|c| c.id == id);
                    if known {
                        self.taken.push((id, ease));
                        // Answered cards are not due any more.
                        for card in self.cards.iter_mut().filter(|c| c.id == id) {
                            card.due = false;
                            card.new = false;
                        }
                    }
                    results.push(known.to_string());
                }
                format!("{{\"result\":[{}],\"error\":null}}", results.join(","))
            }
            other => format!("{{\"result\":null,\"error\":\"unsupported action: {other}\"}}"),
        }
    }
}

impl Link for Fake {
    fn volume(&self) -> &dyn Volume {
        &self.vol
    }

    fn send(&mut self, body: &Body) -> Result<RequestId, Error> {
        let text = core::str::from_utf8(body.as_bytes()).unwrap().to_string();
        assert!(body.as_bytes().len() <= 2048, "a body is sent inline");
        self.log.push(text.clone());
        self.next += 1;
        let id = RequestId::new(self.next).unwrap();
        let mut done = Done {
            id,
            token: REQUEST_TOKEN,
            reason: self.reason,
            status: if self.reason == Reason::Ok { 200 } else { 0 },
            truncated: false,
            body_len: 0,
        };
        let mut bytes = match self.stranger {
            Some(page) => page.as_bytes().to_vec(),
            None if self.reason == Reason::Ok => self.answer(&text).into_bytes(),
            None => Vec::new(),
        };
        if bytes.len() > self.cut_at {
            bytes.truncate(self.cut_at);
            done.truncated = true;
        }
        done.body_len = bytes.len() as u32;
        self.reply = Some((done, bytes));
        Ok(id)
    }

    fn read(
        &mut self,
        done: &Done,
        feed: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let (_, bytes) = self.reply.as_ref().unwrap();
        assert_eq!(done.body_len as usize, bytes.len());
        // Pieces of a few bytes: the parser must not need whole answers.
        for piece in bytes.chunks(11) {
            feed(piece)?;
        }
        Ok(())
    }

    fn cancel(&mut self) {
        self.reply = None;
    }
}

fn account(key: Option<&str>, deck: &str, new: usize) -> Account {
    Account {
        key: key.map(|k| Secret::copy(k).unwrap()),
        deck: deck.into(),
        new,
    }
}

/// Runs a sync to its end, the requests answered one after the other.
fn run(fake: &mut Fake, account: Account, mode: Mode) -> Finish {
    let mut sync = Sync::default();
    let mut step = sync.start(fake, account, mode).unwrap();
    loop {
        match step {
            Step::Ended(finish) => {
                assert!(!sync.running());
                return finish;
            }
            Step::Quiet => {
                let (done, _) = fake.reply.clone().expect("a request is in flight");
                step = sync.done(fake, &done).unwrap();
            }
        }
    }
}

fn lines(fake: &Fake, name: &str) -> Vec<String> {
    fake.vol
        .get(name)
        .map(|b| {
            String::from_utf8(b)
                .unwrap()
                .lines()
                .map(Into::into)
                .collect()
        })
        .unwrap_or_default()
}

fn collection() -> Fake {
    let mut fake = Fake::new();
    fake.card(11, "Spanish", "perro", "dog", false, true);
    fake.card(12, "Spanish", "gato", "cat", false, true);
    fake.card(13, "Spanish::Verbs", "ser", "to be", true, false);
    fake.card(14, "Spanish", "casa", "house", true, false);
    fake.card(21, "Other_deck", "x", "y", false, true);
    fake
}

#[test]
fn a_sync_fetches_the_due_cards_then_the_new_ones() {
    let mut fake = collection();
    let finish = run(&mut fake, account(None, "", 10), Mode::Sync);
    assert_eq!(finish.key, "anki.synced");
    assert_eq!((finish.sent, finish.fetched), (0, 5));
    // The due cards come first, the new ones after.
    let cards = lines(&fake, CARDS);
    let ids: Vec<&str> = cards
        .iter()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    assert_eq!(ids, ["11", "12", "21", "13", "14"]);
    assert_eq!(cards[0], "11\tperro\tdog");
    assert!(fake.vol.get(NEW).is_none());
    // Only what was asked for: one search for each kind, cards three at a time.
    let actions: Vec<&str> = fake
        .log
        .iter()
        .map(|b| {
            b.split("\"action\":\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
        })
        .collect();
    assert_eq!(
        actions,
        ["findCards", "findCards", "cardsInfo", "cardsInfo"]
    );
}

#[test]
fn the_deck_and_the_number_of_new_cards_limit_the_search() {
    let mut fake = collection();
    let finish = run(&mut fake, account(None, "Spanish", 1), Mode::Sync);
    assert_eq!(finish.fetched, 3);
    let ids: Vec<String> = lines(&fake, CARDS)
        .iter()
        .map(|l| l.split('\t').next().unwrap().into())
        .collect();
    // Two due, one new card of the deck (the other new one is over the limit).
    assert_eq!(ids, ["11", "12", "13"]);

    // Without new cards, no search for them.
    let mut fake = collection();
    let finish = run(&mut fake, account(None, "Other_deck", 0), Mode::Sync);
    assert_eq!(finish.fetched, 1);
    assert_eq!(fake.log.len(), 2);
    assert!(fake.log[0].contains("deck:\\\"Other\\\\_deck\\\" is:due"));
}

#[test]
fn nothing_due_leaves_no_cards() {
    let mut fake = Fake::new();
    store::record(&fake.vol, 1, 3).unwrap();
    fake.vol.write(CARDS, b"1\ta\tb\n").unwrap();
    let finish = run(&mut fake, account(None, "", 0), Mode::Sync);
    // The answer for a card Anki does not know is let go, as it cannot be taken.
    assert_eq!(
        (finish.key, finish.sent, finish.fetched),
        ("anki.nothing-due", 1, 0)
    );
    assert!(fake.vol.get(CARDS).is_none());
    assert!(fake.vol.get(ANSWERS).is_none());
}

#[test]
fn answers_wait_offline_and_go_with_the_next_sync() {
    let mut fake = collection();
    run(&mut fake, account(None, "Spanish", 0), Mode::Sync);

    // Study on the reader: show, answer.
    let mut study = Study::open(&fake.vol).unwrap();
    assert_eq!(study.current.as_ref().unwrap().front, "perro");
    assert_eq!(study.left(), 2);
    assert!(study.reveal());
    study.answer(&fake.vol, 3).unwrap();
    assert_eq!(study.current.as_ref().unwrap().front, "gato");
    study.answer(&fake.vol, 1).unwrap();
    assert!(study.current.is_none());
    assert_eq!(study.empty, Empty::AllDone);
    assert_eq!(lines(&fake, ANSWERS), ["11\t3", "12\t1"]);
    assert_eq!(native_study::ready(&fake.vol).unwrap(), 0);
    // Opening again resumes after the last answer.
    assert!(Study::open(&fake.vol).unwrap().current.is_none());

    // No network: nothing is lost.
    fake.reason = Reason::NoNetwork;
    let finish = run(&mut fake, account(None, "Spanish", 0), Mode::Sync);
    assert_eq!(finish.key, "anki.no-network");
    assert_eq!(lines(&fake, ANSWERS), ["11\t3", "12\t1"]);
    assert!(fake.vol.get(CARDS).is_some());
    assert!(fake.taken.is_empty());

    // Back online: the answers go first, then the cards come.
    fake.reason = Reason::Ok;
    fake.cards.push(FakeCard {
        id: 15,
        deck: "Spanish",
        front: "mesa".into(),
        back: "table".into(),
        new: false,
        due: true,
    });
    let finish = run(&mut fake, account(None, "Spanish", 0), Mode::Sync);
    assert_eq!(
        (finish.key, finish.sent, finish.fetched),
        ("anki.synced", 2, 1)
    );
    assert_eq!(fake.taken, [(11, 3), (12, 1)]);
    assert!(fake.vol.get(ANSWERS).is_none());
    assert_eq!(lines(&fake, CARDS), ["15\tmesa\ttable"]);
}

#[test]
fn a_wrong_key_is_told_and_nothing_is_lost() {
    let mut fake = collection();
    fake.key = Some("secret");
    store::record(&fake.vol, 11, 4).unwrap();
    let finish = run(&mut fake, account(Some("wrong"), "", 0), Mode::Sync);
    assert_eq!(finish.key, "anki.bad-key");
    assert_eq!(lines(&fake, ANSWERS), ["11\t4"]);
    // The key is in the body of every request, and nowhere else.
    assert!(fake.log[0].contains("\"key\":\"wrong\""));

    let finish = run(&mut fake, account(Some("secret"), "", 0), Mode::Sync);
    assert_eq!(finish.key, "anki.synced");
    assert_eq!(fake.taken, [(11, 4)]);
}

#[test]
fn what_is_not_anki_connect_is_told() {
    let mut fake = collection();
    fake.stranger = Some("<html>router login</html>");
    let finish = run(&mut fake, account(None, "", 0), Mode::Sync);
    assert_eq!(finish.key, "anki.bad-reply");

    fake.stranger = Some("{\"result\": null, \"error\": \"unsupported action: findCards\"}");
    let finish = run(&mut fake, account(None, "", 0), Mode::Sync);
    assert_eq!(finish.key, "anki.said");
    assert_eq!(
        finish.said.as_deref(),
        Some("unsupported action: findCards")
    );

    // A computer that cannot be reached.
    fake.stranger = None;
    fake.reason = Reason::Refused;
    assert_eq!(
        run(&mut fake, account(None, "", 0), Mode::Sync).key,
        "anki.unreachable"
    );
    fake.reason = Reason::NetworkOff;
    assert_eq!(
        run(&mut fake, account(None, "", 0), Mode::Sync).key,
        "anki.denied"
    );
}

#[test]
fn a_large_answer_is_fetched_in_smaller_batches() {
    let mut fake = Fake::new();
    let big = "x".repeat(300);
    for id in 1..=5 {
        fake.card(id, "D", &format!("front {id}"), &big, false, true);
    }
    // One card is too large for any answer.
    fake.card(6, "D", "huge", &"y".repeat(3000), false, true);
    fake.cut_at = 1500;
    let finish = run(&mut fake, account(None, "", 0), Mode::Sync);
    assert_eq!(finish.fetched, 5);
    let ids: Vec<String> = lines(&fake, CARDS)
        .iter()
        .map(|l| l.split('\t').next().unwrap().into())
        .collect();
    assert_eq!(ids, ["1", "2", "3", "4", "5"]);
    // The cards are cut to what a screen shows.
    assert!(lines(&fake, CARDS)[0].len() < 400);
}

#[test]
fn a_long_queue_goes_in_requests_and_a_failure_keeps_the_rest() {
    let mut fake = Fake::new();
    for id in 1..=70u64 {
        fake.card(id, "D", "q", "a", false, true);
        store::record(&fake.vol, id, 2).unwrap();
    }
    fake.vol.write(CARDS, b"1\tq\ta\n").unwrap();
    // The first request of 40 is taken, the second is not.
    fake.answers_allowed = 1;
    let finish = run(&mut fake, account(None, "", 0), Mode::Sync);
    assert_eq!(finish.key, "anki.said");
    assert_eq!(finish.sent, 40);
    assert_eq!(fake.taken.len(), 40);
    assert_eq!(lines(&fake, ANSWERS).len(), 30);
    assert_eq!(lines(&fake, ANSWERS)[0], "41\t2");
    // The cards were made stale by the answers taken.
    assert!(fake.vol.get(CARDS).is_none());
    assert!(fake.log.iter().all(|b| b.len() <= 2048));

    fake.answers_allowed = usize::MAX;
    let finish = run(&mut fake, account(None, "", 0), Mode::Sync);
    assert_eq!(finish.sent, 30);
    assert!(fake.vol.get(ANSWERS).is_none());
    assert_eq!(fake.taken.len(), 70);
}

#[test]
fn the_decks_are_listed() {
    let mut fake = collection();
    let finish = run(&mut fake, account(None, "", 0), Mode::Decks);
    assert_eq!(finish.key, "anki.decks-listed");
    assert_eq!(
        finish.names.unwrap(),
        ["Other_deck", "Spanish", "Spanish::Verbs"]
    );
    // Listing the decks touches no card.
    assert!(fake.vol.get(CARDS).is_none());
}

#[test]
fn a_full_queue_refuses_an_answer_and_keeps_the_card() {
    let fake = collection();
    fake.vol.write(CARDS, b"1\tq\ta\n2\tq\ta\n").unwrap();
    let filler = [b'1'; 30];
    let mut line = filler.to_vec();
    line.extend_from_slice(b"\t3\n");
    while fake.vol.size(ANSWERS).unwrap_or(0) < store::ANSWERS_MAX {
        fake.vol.append(ANSWERS, &line).unwrap();
    }
    let mut study = Study::open(&fake.vol).unwrap();
    assert_eq!(study.answer(&fake.vol, 3), Err(Error::NoSpace));
    assert_eq!(study.current.as_ref().unwrap().front, "q");
    assert_eq!(study.left(), 2);
}

/// The same fake behind a real socket on this computer: HTTP/1.1 requests as
/// the host sends them (`POST /`, `Content-Type`, `Content-Length`, the body
/// inline) and the answers as the add-on sends them, with a status line.
mod socket {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::thread::{self, JoinHandle};

    /// Serves `fake` until a request whose body is `STOP`; gives it back.
    fn serve(listener: TcpListener, mut fake: Fake) -> JoinHandle<Fake> {
        thread::spawn(move || {
            loop {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 512];
                let (head_end, length) = loop {
                    let n = stream.read(&mut chunk).unwrap();
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&bytes[..at]).to_lowercase();
                        let length: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .unwrap()
                            .trim()
                            .parse()
                            .unwrap();
                        assert!(head.starts_with("post / http/1.1"), "{head}");
                        assert!(head.contains("content-type: application/json"));
                        break (at + 4, length);
                    }
                };
                while bytes.len() < head_end + length {
                    let n = stream.read(&mut chunk).unwrap();
                    bytes.extend_from_slice(&chunk[..n]);
                }
                let body = String::from_utf8(bytes[head_end..head_end + length].to_vec()).unwrap();
                if body == "STOP" {
                    return fake;
                }
                let reply = fake.answer(&body);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
            }
        })
    }

    /// The reader's side over a socket: its data folder in memory.
    struct TcpLink {
        vol: Mem,
        addr: SocketAddr,
        next: u32,
        reply: Option<(Done, Vec<u8>)>,
    }

    impl TcpLink {
        fn post(&self, body: &[u8]) -> (u32, Vec<u8>) {
            let mut stream = TcpStream::connect(self.addr).unwrap();
            write!(
                stream,
                "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
            let mut all = Vec::new();
            stream.read_to_end(&mut all).unwrap();
            let at = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let status = String::from_utf8_lossy(&all[..at])
                .split(' ')
                .nth(1)
                .unwrap()
                .parse()
                .unwrap();
            (status, all[at + 4..].to_vec())
        }
    }

    impl Link for TcpLink {
        fn volume(&self) -> &dyn Volume {
            &self.vol
        }
        fn send(&mut self, body: &Body) -> Result<RequestId, Error> {
            let (status, bytes) = self.post(body.as_bytes());
            self.next += 1;
            let id = RequestId::new(self.next).unwrap();
            let done = Done {
                id,
                token: REQUEST_TOKEN,
                reason: Reason::Ok,
                status,
                truncated: false,
                body_len: bytes.len() as u32,
            };
            self.reply = Some((done, bytes));
            Ok(id)
        }
        fn read(
            &mut self,
            _: &Done,
            feed: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
        ) -> Result<(), Error> {
            for piece in self.reply.as_ref().unwrap().1.chunks(64) {
                feed(piece)?;
            }
            Ok(())
        }
        fn cancel(&mut self) {}
    }

    fn sync(link: &mut TcpLink, account: Account) -> Finish {
        let mut sync = Sync::default();
        let mut step = sync.start(link, account, Mode::Sync).unwrap();
        loop {
            match step {
                Step::Ended(finish) => return finish,
                Step::Quiet => {
                    let (done, _) = link.reply.clone().unwrap();
                    step = sync.done(link, &done).unwrap();
                }
            }
        }
    }

    #[test]
    fn a_study_and_a_sync_work_over_a_real_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut fake = collection();
        fake.key = Some("k");
        let server = serve(listener, fake);
        let mut link = TcpLink {
            vol: Mem::default(),
            addr,
            next: 0,
            reply: None,
        };

        let finish = sync(&mut link, account(Some("k"), "Spanish", 1));
        assert_eq!((finish.key, finish.fetched), ("anki.synced", 3));
        let mut study = Study::open(&link.vol).unwrap();
        study.answer(&link.vol, 3).unwrap();
        study.answer(&link.vol, 4).unwrap();

        let finish = sync(&mut link, account(Some("k"), "Spanish", 0));
        assert_eq!((finish.sent, finish.fetched), (2, 0));
        assert_eq!(
            sync(&mut link, account(Some("bad"), "", 0)).key,
            "anki.bad-key"
        );

        let mut stop = TcpStream::connect(addr).unwrap();
        write!(
            stop,
            "POST / HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 4\r\n\r\nSTOP"
        )
        .unwrap();
        let fake = server.join().unwrap();
        assert_eq!(fake.taken, [(11, 3), (12, 4)]);
    }
}
