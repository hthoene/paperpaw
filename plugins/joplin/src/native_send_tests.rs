//! The sending (`native_send`) over a world in memory: the data folder and the
//! card as volumes, and a Joplin target that answers the requests the way the
//! sync targets do (sign-in, `info.json`, locks, items). The cases are those
//! interface 6 was tested on (`tests/send.rs` and `tests/setup.rs` of its
//! kernel runs), now without a kernel.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::fail_alloc::{exempt, sweep};
use crate::native_joplin::{self as joplin, Service};
use crate::native_keep::Secret;
use crate::native_kept as kept;
use crate::native_send::{
    Account, Auth, Body, Call, Done, FOUND, INFO, NOTE_FILE, Outcome, Sender, Start, Step, Tell,
    Wire, sign_in_body, target_info,
};
use crate::test_volume::Mem;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::files::Volume;

const NOW: i64 = 1_792_483_200_000;
const INFO_OK: &str = "{\"version\":3,\"e2ee\":{\"value\":false,\"updatedTime\":1}}";
const NOTES: &str = "# Alice\n\nLewis Carroll\n\n## Highlights\n\n> Curiouser!\n>\n> 12 %\n\n";
const NOTES_MORE: &str =
    "# Alice\n\nLewis Carroll\n\n## Highlights\n\n> Curiouser!\n>\n> 12 %\n\n> Who?\n>\n> 40 %\n\n";

/// What one request was.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    step: Step,
    method: String,
    name: String,
    auth: String,
}

struct World {
    data: Mem,
    card: Mem,
    service: Service,
    /// The address, where the test sets one.
    base: Option<String>,
    notebook: String,
    password: Option<String>,
    network: bool,
    now: Option<i64>,
    /// The target's files by name.
    items: BTreeMap<String, Vec<u8>>,
    locks: Option<String>,
    sign_in: u32,
    /// How many requests with the session are refused (it ended).
    expire: u32,
    /// Notes answered with this status (500, ...) by name.
    fail_put: Vec<String>,
    /// The answer to the next request of this step is lost (a put is stored).
    lose: Option<Step>,
    seen: Vec<Seen>,
    next: u32,
    answer: Option<Done>,
    /// The memory body of the last answer, by request.
    bodies: BTreeMap<u32, Vec<u8>>,
}

impl World {
    fn new(service: Service) -> Self {
        let mut items = BTreeMap::new();
        items.insert(INFO.to_string(), INFO_OK.as_bytes().to_vec());
        Self {
            data: Mem::default(),
            card: Mem::default(),
            service,
            base: None,
            notebook: "PaperPaw".to_string(),
            password: Some("pw".to_string()),
            network: true,
            now: Some(NOW),
            items,
            locks: None,
            sign_in: 200,
            expire: 0,
            fail_put: Vec::new(),
            lose: None,
            seen: Vec::new(),
            next: 0,
            answer: None,
            bodies: BTreeMap::new(),
        }
    }

    fn notes(&self, path: &str, text: &str) {
        self.card.write(path, text.as_bytes()).unwrap();
    }

    fn waiting(&self) -> Vec<String> {
        kept::waiting(&self.data).unwrap()
    }

    /// The name of the file a request is for.
    fn name_of(&self, url: &str) -> String {
        let tail = url.rsplit_once("/api/items/root:/").map(|(_, t)| t);
        match (self.service, tail) {
            (Service::WebDav, _) => url.rsplit('/').next().unwrap().to_string(),
            (_, Some(tail)) => tail.strip_suffix(":/content").unwrap().to_string(),
            _ => url.rsplit('/').next().unwrap().to_string(),
        }
    }

    /// What the target answers: a status, and what it saves to the file.
    fn serve(&mut self, call: &Call<'_>) -> (u32, Option<Vec<u8>>) {
        let name = self.name_of(call.url);
        let session = matches!(call.auth, Auth::Session(_));
        if session && self.expire > 0 && call.step != Step::SignIn {
            self.expire -= 1;
            return (401, None);
        }
        match (call.step, call.method) {
            (Step::SignIn, _) => match self.sign_in {
                200 => (200, Some(b"{\"id\":\"S1\",\"user_id\":\"u\"}".to_vec())),
                status => (status, None),
            },
            (Step::Locks, _) => match &self.locks {
                Some(text) => (200, Some(text.as_bytes().to_vec())),
                None => (404, None),
            },
            (_, "PUT") => {
                let Body::File(path) = call.body else {
                    panic!("a put sends a file");
                };
                if self.fail_put.contains(&name) {
                    return (500, None);
                }
                self.items.insert(name, self.data.get(path).unwrap());
                (200, None)
            }
            _ => match self.items.get(&name) {
                Some(body) => (200, Some(body.clone())),
                None => (404, None),
            },
        }
    }
}

impl Wire for World {
    fn data(&self) -> &dyn Volume {
        &self.data
    }

    fn notes(&self) -> &dyn Volume {
        &self.card
    }

    fn account(&mut self) -> Result<Account, Error> {
        exempt(|| {
            Ok(Account {
                service: self.service,
                base: match (&self.base, self.service) {
                    (Some(base), _) => base.clone(),
                    (_, Service::Cloud) => joplin::CLOUD.to_string(),
                    _ => "https://j.example".to_string(),
                },
                user: "me@example.org".to_string(),
                notebook: self.notebook.clone(),
            })
        })
    }

    fn password(&mut self) -> Result<Option<Secret>, Error> {
        exempt(|| self.password.as_deref().map(Secret::copy).transpose())
    }

    fn network(&mut self) -> Result<(), Error> {
        if self.network {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }

    fn now_ms(&mut self) -> Option<i64> {
        self.now
    }

    fn answer(&mut self, id: RequestId) -> Result<Secret, Error> {
        exempt(|| {
            let body = self.bodies.get(&id.get()).ok_or(Error::NotFound)?;
            Secret::copy(core::str::from_utf8(body).unwrap())
        })
    }

    fn send(&mut self, call: &Call<'_>) -> Result<RequestId, Error> {
        if call.url.len() > paperpaw_sdk::native::net::MAX_URL {
            return Err(Error::Invalid);
        }
        exempt(|| {
            let (status, saved) = self.serve(call);
            match (saved, call.save_to) {
                (Some(body), Some((path, _))) => self.data.write(path, &body).unwrap(),
                (Some(body), None) => {
                    self.bodies.insert(self.next + 1, body);
                }
                _ => {}
            }
            let auth = match call.auth {
                Auth::Session(id) => format!("session {id}"),
                Auth::Basic { user, password } => format!("basic {user}:{password}"),
                Auth::None => String::new(),
            };
            let name = self.name_of(call.url);
            self.seen.push(Seen {
                step: call.step,
                method: call.method.to_string(),
                name,
                auth,
            });
            self.next += 1;
            let id = RequestId::new(self.next).unwrap();
            let lost = self.lose == Some(call.step);
            if lost {
                self.lose = None;
            }
            let reason = if lost { Reason::Timeout } else { Reason::Ok };
            self.answer = Some(Done::new(id, call.step.token(), reason, status));
            Ok(id)
        })
    }
}

/// Answers every request in flight until the sending ends or is quiet.
fn pump(w: &mut World, s: &mut Sender, mut outcome: Outcome) -> Outcome {
    while let Some(done) = w.answer.take() {
        outcome = s.done(w, &done).unwrap();
        if matches!(outcome, Outcome::Ended { .. }) {
            break;
        }
    }
    outcome
}

fn export(w: &mut World, s: &mut Sender, path: &str) -> Outcome {
    let first = s.exported(w, path).unwrap();
    pump(w, s, first)
}

fn ended(tell: Tell) -> Outcome {
    Outcome::Ended {
        tell,
        quiet: false,
        apart: 0,
    }
}

fn puts(w: &World) -> Vec<&Seen> {
    w.seen.iter().filter(|r| r.method == "PUT").collect()
}

fn text(w: &World, name: &str) -> String {
    String::from_utf8(w.items.get(name).unwrap().clone()).unwrap()
}

fn prop<'a>(item: &'a str, key: &str) -> &'a str {
    item.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}: ")))
        .unwrap()
}

#[test]
fn sends_a_new_note_then_updates_it() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::Sent(1)));
    // The notebook, then the note in it.
    let made = puts(&w);
    assert_eq!(made.len(), 2);
    let notebook = made[0].name.clone();
    let note = made[1].name.clone();
    let item = text(&w, &note);
    assert!(item.starts_with("Alice\n\n"));
    assert_eq!(prop(&item, "parent_id"), notebook.trim_end_matches(".md"));
    assert_eq!(prop(&item, "type_"), "1");
    assert_eq!(prop(&item, "author"), "Lewis Carroll");
    assert!(w.waiting().is_empty());
    assert_eq!(w.seen[0].auth, "basic me@example.org:pw");
    // Exported again with more: the same note is written over, no new one.
    w.notes("Notes/a.md", NOTES_MORE);
    w.seen.clear();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::Sent(1)));
    assert_eq!(puts(&w).len(), 1);
    assert_eq!(puts(&w)[0].name, note);
    assert!(text(&w, &note).contains("Who?"));
    assert_eq!(w.items.len(), 3, "info, notebook, note");
    // The notebook is found, not made again.
    assert_eq!(w.seen[2].step, Step::NotebookFound);
    for name in [FOUND, NOTE_FILE, INFO] {
        assert!(w.data.get(name).is_none(), "{name} is cleaned away");
    }
}

#[test]
fn a_note_changed_in_joplin_stays_and_a_new_one_is_made() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    export(&mut w, &mut s, "Notes/a.md");
    let note = puts(&w)[1].name.clone();
    let edited = text(&w, &note).replacen("Curiouser", "Edited", 1);
    w.items.insert(note.clone(), edited.clone().into_bytes());
    w.seen.clear();
    let done = export(&mut w, &mut s, "Notes/a.md");
    assert_eq!(
        done,
        Outcome::Ended {
            tell: Tell::Sent(1),
            quiet: false,
            apart: 1
        }
    );
    assert_eq!(text(&w, &note), edited, "what was edited is left as it is");
    assert_ne!(puts(&w)[0].name, note);
}

#[test]
fn a_lost_answer_makes_no_second_note() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    // The notebook goes, the note's answer is lost.
    w.lose = Some(Step::NoteSent);
    let out = export(&mut w, &mut s, "Notes/a.md");
    assert_eq!(out, ended(Tell::Unreachable));
    assert_eq!(w.waiting(), ["Notes/a.md"]);
    let sending = kept::sending(&w.data, "Notes/a.md").unwrap().unwrap();
    let note = format!("{}.md", sending.id);
    assert!(w.items.contains_key(&note), "Joplin has it all the same");
    // The next sending looks for it and writes it over.
    w.seen.clear();
    let again = s.start(&mut w, Start::Action).unwrap();
    assert_eq!(pump(&mut w, &mut s, again), ended(Tell::Sent(1)));
    assert_eq!(puts(&w).len(), 1);
    assert_eq!(puts(&w)[0].name, note);
    assert!(kept::sending(&w.data, "Notes/a.md").unwrap().is_none());
    assert_eq!(
        kept::sent(&w.data, "Notes/a.md").unwrap().unwrap().id,
        sending.id
    );
}

#[test]
fn joplin_cloud_signs_in_and_again_when_the_session_ends() {
    let mut w = World::new(Service::Cloud);
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::Sent(1)));
    assert_eq!(w.seen[0].step, Step::SignIn);
    assert_eq!(w.seen[1].auth, "session S1");
    // The session is kept: the next sending does not sign in.
    w.notes("Notes/b.md", NOTES);
    w.seen.clear();
    export(&mut w, &mut s, "Notes/b.md");
    assert_eq!(w.seen[0].step, Step::Info);
    // It ends: signed in anew, once, and the book still goes.
    w.notes("Notes/c.md", NOTES);
    w.seen.clear();
    w.expire = 1;
    assert_eq!(export(&mut w, &mut s, "Notes/c.md"), ended(Tell::Sent(1)));
    let steps: Vec<Step> = w.seen.iter().map(|r| r.step).collect();
    assert_eq!(&steps[..3], [Step::Info, Step::SignIn, Step::Info]);
}

#[test]
fn notes_wait_without_a_connection_and_go_when_wifi_comes() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    w.network = false;
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::NoNetwork));
    assert_eq!(w.waiting(), ["Notes/a.md"]);
    assert!(w.seen.is_empty());
    w.network = true;
    let first = s.start(&mut w, Start::Retry).unwrap();
    let out = pump(&mut w, &mut s, first);
    assert_eq!(
        out,
        Outcome::Ended {
            tell: Tell::Sent(1),
            quiet: false,
            apart: 0
        },
        "a sending by itself tells what went well"
    );
    assert!(w.waiting().is_empty());
}

#[test]
fn a_sending_by_itself_is_quiet_about_what_went_wrong() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    w.network = false;
    let mut s = Sender::new();
    kept::add_waiting(&w.data, "Notes/a.md").unwrap();
    let out = s.start(&mut w, Start::Retry).unwrap();
    assert_eq!(
        out,
        Outcome::Ended {
            tell: Tell::NoNetwork,
            quiet: true,
            apart: 0
        }
    );
}

#[test]
fn a_wrong_password_stops_tries_until_it_is_changed() {
    let mut w = World::new(Service::Server);
    w.notes("Notes/a.md", NOTES);
    w.sign_in = 403;
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::BadLogin));
    let calls = w.seen.len();
    // Not tried by itself.
    assert_eq!(s.start(&mut w, Start::Retry).unwrap(), Outcome::Quiet);
    assert_eq!(w.seen.len(), calls);
    // The account changes: tried again.
    w.sign_in = 200;
    s.account_changed();
    let first = s.start(&mut w, Start::Retry).unwrap();
    assert!(matches!(
        pump(&mut w, &mut s, first),
        Outcome::Ended {
            tell: Tell::Sent(1),
            ..
        }
    ));
}

#[test]
fn encrypted_targets_get_nothing() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    w.items.insert(
        INFO.to_string(),
        b"{\"version\":3,\"e2ee\":{\"value\":true}}".to_vec(),
    );
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::Encrypted));
    assert!(puts(&w).is_empty());
    assert_eq!(w.waiting(), ["Notes/a.md"]);
}

#[test]
fn targets_in_another_format_get_nothing() {
    for (info, tell) in [
        ("{\"version\":2}", Tell::OlderTarget),
        ("{\"version\":4}", Tell::NewerTarget),
        ("{}", Tell::OlderTarget),
        ("not json", Tell::OlderTarget),
    ] {
        let mut w = World::new(Service::WebDav);
        w.notes("Notes/a.md", NOTES);
        w.items.insert(INFO.to_string(), info.as_bytes().to_vec());
        let mut s = Sender::new();
        assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(tell), "{info}");
        assert!(puts(&w).is_empty());
    }
    // No Joplin data there at all.
    let mut w = World::new(Service::WebDav);
    w.items.clear();
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::NoData));
}

#[test]
fn nothing_is_written_while_a_client_upgrades_the_target() {
    let mut w = World::new(Service::Server);
    w.notes("Notes/a.md", NOTES);
    let lock = |ago: i64| {
        format!(
            "{{\"items\":[{{\"id\":\"x\",\"type\":2,\"clientType\":1,\"clientId\":\"c\",\
             \"updatedTime\":{}}}],\"has_more\":false}}",
            NOW - ago
        )
    };
    w.locks = Some(lock(30_000));
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::Upgrading));
    assert!(puts(&w).is_empty());
    // An old lock holds nothing, and a target without a lock list has none.
    w.locks = Some(lock(crate::native_locks::TTL + 1));
    let first = s.start(&mut w, Start::Action).unwrap();
    assert!(matches!(
        pump(&mut w, &mut s, first),
        Outcome::Ended {
            tell: Tell::Sent(1),
            ..
        }
    ));
}

#[test]
fn a_note_that_fails_waits_and_the_others_go() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    w.notes("Notes/b.md", NOTES);
    let mut s = Sender::new();
    kept::add_waiting(&w.data, "Notes/a.md").unwrap();
    let first = s.exported(&mut w, "Notes/b.md").unwrap();
    // The first book's note goes wrong: its name is not known before it is
    // made, so every note put fails once, then the target is well again.
    let mut out = first;
    let mut failed_one = false;
    while let Some(done) = w.answer.take() {
        let step = Step::of_token(done.token).unwrap();
        let done = if step == Step::NoteSent && !failed_one {
            failed_one = true;
            Done::new(done.id, done.token, Reason::Ok, 500)
        } else {
            done
        };
        out = s.done(&mut w, &done).unwrap();
        if matches!(out, Outcome::Ended { .. }) {
            break;
        }
    }
    assert_eq!(out, ended(Tell::SomeFailed { sent: 1, failed: 1 }));
    assert_eq!(w.waiting(), ["Notes/a.md"]);
}

#[test]
fn without_an_account_nothing_is_sent_and_it_is_said() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    w.password = None;
    let mut s = Sender::new();
    assert_eq!(
        export(&mut w, &mut s, "Notes/a.md"),
        ended(Tell::SetupNeeded)
    );
    assert!(w.seen.is_empty());
    assert_eq!(w.waiting(), ["Notes/a.md"]);
}

#[test]
fn an_address_the_network_refuses_is_said_so_and_not_tried_again() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    // Over the longest address a request has: not "not enough memory".
    let long = format!("https://j.example/{}", "x".repeat(2000));
    w.base = Some(long);
    assert_eq!(
        export(&mut w, &mut s, "Notes/a.md"),
        ended(Tell::BadAddress)
    );
    assert!(s.blocked());
    assert_eq!(s.start(&mut w, Start::Retry).unwrap(), Outcome::Quiet);
    assert_eq!(w.waiting(), ["Notes/a.md"]);
}

#[test]
fn the_time_must_be_known() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    w.now = None;
    let mut s = Sender::new();
    assert_eq!(export(&mut w, &mut s, "Notes/a.md"), ended(Tell::NoTime));
}

#[test]
fn a_deleted_notes_file_leaves_the_list() {
    let mut w = World::new(Service::WebDav);
    let mut s = Sender::new();
    kept::add_waiting(&w.data, "Notes/gone.md").unwrap();
    let first = s.start(&mut w, Start::Action).unwrap();
    assert_eq!(pump(&mut w, &mut s, first), ended(Tell::Nothing));
    assert!(w.waiting().is_empty());
}

#[test]
fn an_answer_twice_or_after_the_end_changes_nothing() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    let first = s.exported(&mut w, "Notes/a.md").unwrap();
    let answer = w.answer.unwrap();
    assert_eq!(first, Outcome::Quiet);
    let other = Done::new(RequestId::new(99).unwrap(), answer.token, Reason::Ok, 200);
    assert_eq!(s.done(&mut w, &other).unwrap(), Outcome::Quiet);
    assert_eq!(
        s.done(&mut w, &Done::new(answer.id, 42, Reason::Ok, 200)),
        Ok(Outcome::Quiet)
    );
    let out = pump(&mut w, &mut s, first);
    assert_eq!(out, ended(Tell::Sent(1)));
    assert_eq!(s.done(&mut w, &answer).unwrap(), Outcome::Quiet);
    assert!(!s.busy());
}

#[test]
fn the_end_of_the_work_run_ends_the_sending() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    let mut s = Sender::new();
    s.exported(&mut w, "Notes/a.md").unwrap();
    let answer = w.answer.take().unwrap();
    let ends = Done::new(answer.id, answer.token, Reason::WorkEnded, 0);
    assert_eq!(s.done(&mut w, &ends).unwrap(), ended(Tell::Unreachable));
    assert_eq!(w.waiting(), ["Notes/a.md"]);
    assert!(!s.busy());
}

#[test]
fn books_exported_meanwhile_come_along_and_wait_once() {
    let mut w = World::new(Service::WebDav);
    w.notes("Notes/a.md", NOTES);
    w.notes("Notes/b.md", NOTES);
    let mut s = Sender::new();
    let first = s.exported(&mut w, "Notes/a.md").unwrap();
    assert_eq!(s.exported(&mut w, "Notes/b.md").unwrap(), Outcome::Quiet);
    assert_eq!(s.exported(&mut w, "Notes/b.md").unwrap(), Outcome::Quiet);
    assert_eq!(w.waiting(), ["Notes/a.md", "Notes/b.md"]);
    assert_eq!(pump(&mut w, &mut s, first), ended(Tell::Sent(2)));
    assert!(w.waiting().is_empty());
}

#[test]
fn the_sign_in_is_json_whatever_the_account_says() {
    let body = sign_in_body("me@example.org", "p\"w\\\n\u{1}é").unwrap();
    assert_eq!(
        body.as_str(),
        "{\"email\":\"me@example.org\",\"password\":\"p\\\"w\\\\\\n\\u0001é\"}"
    );
    assert_eq!(
        sign_in_body("a", &"x".repeat(4000)).err(),
        Some(Error::NoSpace)
    );
}

#[test]
fn the_target_info_is_read_from_a_file_of_any_shape() {
    let read = |text: &str| {
        let data = Mem::with(INFO, text.as_bytes());
        target_info(&data).unwrap()
    };
    assert_eq!(read(INFO_OK), (3, false));
    assert_eq!(read("{\"version\":3,\"e2ee\":{\"value\":true}}"), (3, true));
    assert_eq!(read("{\"e2ee\":{\"value\":true},\"version\":2}"), (2, true));
    assert_eq!(read(""), (0, false));
    assert_eq!(read("[[[[[[[[[[[[[[[[[[[["), (0, false));
    assert_eq!(read("{\"version\":\"x\"}"), (0, false));
    let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    assert_eq!(read(&deep), (0, false));
    assert_eq!(target_info(&Mem::default()).unwrap(), (0, false));
}

#[test]
fn refused_memory_ends_the_sending_never_an_abort() {
    for service in [Service::WebDav, Service::Cloud] {
        sweep(|| {
            let (mut w, mut s) = exempt(|| {
                let w = World::new(service);
                w.notes("Notes/a.md", NOTES);
                (w, Sender::new())
            });
            let first = s.exported(&mut w, "Notes/a.md")?;
            let _ = pump_fallible(&mut w, &mut s, first)?;
            Ok(())
        });
    }
}

/// [`pump`] for a sweep: a refusal the sender hands back is the result.
fn pump_fallible(w: &mut World, s: &mut Sender, mut outcome: Outcome) -> Result<Outcome, Error> {
    while let Some(done) = w.answer.take() {
        outcome = s.done(w, &done)?;
        if matches!(outcome, Outcome::Ended { .. }) {
            break;
        }
    }
    Ok(outcome)
}
