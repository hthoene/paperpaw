//! The engine against a CalDAV server in memory: the lists are found from any
//! address of the account, tasks come and go both ways, what was changed
//! offline waits and is sent later, a task changed on both sides ends as the
//! server's with the reader's kept beside it, a power cut between a sending
//! and its note on the card makes no second task, and an answer cut short
//! removes nothing.
#![allow(clippy::unwrap_used, reason = "fixtures")]
use alloc::{
    collections::{BTreeMap, VecDeque},
    format,
    string::{String, ToString},
    vec::Vec,
};

use paperpaw_caldav::{dav, ical::read_todo};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::files::Volume;

use crate::keep::{self, Loaded};
use crate::model::{Seed, Store, View, flag};
use crate::sync::{Body, Call, Ended, Outcome, Report, Step, Sync, Wire};
use crate::test_volume::Fake;

const ROOT: &str = "https://cloud.example/dav/";
const HOME: &str = "/dav/calendars/anna/";
const PRINCIPAL: &str = "/dav/principals/anna/";
const NOW: i64 = 1_795_000_000;
/// The day of `NOW` (2026-11-18).
const TODAY: i32 = 20_775;

#[derive(Debug, Clone)]
struct Resource {
    etag: u32,
    text: String,
}

#[derive(Debug, Default)]
struct Collection {
    name: String,
    ctag: u32,
    events_only: bool,
    items: BTreeMap<String, Resource>,
}

/// A CalDAV server and the device around the engine.
#[derive(Debug, Default)]
struct Server {
    files: Fake,
    collections: BTreeMap<String, Collection>,
    /// The requests that ended and were not told yet.
    ended: VecDeque<(u32, Ended)>,
    etags: BTreeMap<u32, String>,
    /// Every request, as `METHOD path`.
    log: Vec<String>,
    next: u32,
    etag: u32,
    offline: bool,
    denies: bool,
    /// The next `REPORT` answer is cut short.
    cut_report: bool,
    /// Answers leave out the `ETag` header.
    no_etags: bool,
    /// Where the principal says the lists are, if not at [`HOME`].
    home_href: Option<String>,
    now: i64,
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

impl Server {
    fn new() -> Self {
        let mut server = Self {
            now: NOW,
            etag: 100,
            ..Self::default()
        };
        server.collection("tasks", "Tasks");
        server
    }

    fn collection(&mut self, folder: &str, name: &str) -> &mut Collection {
        self.collections
            .entry(format!("{HOME}{folder}/"))
            .or_insert_with(|| Collection {
                name: name.into(),
                ctag: 1,
                ..Collection::default()
            })
    }

    /// Puts a task on the server, as another program would.
    fn put(&mut self, folder: &str, uid: &str, lines: &str) {
        self.etag += 1;
        let etag = self.etag;
        let text = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\nBEGIN:VTODO\r\nUID:{uid}\r\n\
DTSTAMP:20261101T080000Z\r\n{lines}DESCRIPTION:kept by the server\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\n\
TRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
        );
        let collection = self.collection(folder, folder);
        collection.ctag += 1;
        collection
            .items
            .insert(format!("{uid}.ics"), Resource { etag, text });
    }

    fn remove(&mut self, folder: &str, uid: &str) {
        let collection = self.collection(folder, folder);
        collection.ctag += 1;
        collection.items.remove(&format!("{uid}.ics")).unwrap();
    }

    fn text(&mut self, folder: &str, uid: &str) -> String {
        self.collection(folder, folder).items[&format!("{uid}.ics")]
            .text
            .clone()
    }

    fn names(&mut self, folder: &str) -> Vec<String> {
        self.collection(folder, folder)
            .items
            .keys()
            .cloned()
            .collect()
    }

    fn calendar_xml(path: &str, collection: &Collection) -> String {
        let comp = if collection.events_only {
            "VEVENT"
        } else {
            "VTODO"
        };
        format!(
            "<d:response><d:href>{path}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/>\
<c:calendar/></d:resourcetype><d:displayname>{}</d:displayname><cs:getctag>\"c{}\"</cs:getctag>\
<c:supported-calendar-component-set><c:comp name=\"{comp}\"/></c:supported-calendar-component-set>\
</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
            escape(&collection.name),
            collection.ctag
        )
    }

    fn multistatus(inner: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\" \
xmlns:c=\"urn:ietf:params:xml:ns:caldav\" xmlns:cs=\"http://calendarserver.org/ns/\">{inner}</d:multistatus>"
        )
    }

    fn propfind(&self, path: &str) -> Option<String> {
        let plain = |extra: &str| {
            format!(
                "<d:response><d:href>{path}</d:href><d:propstat><d:prop><d:resourcetype>\
<d:collection/></d:resourcetype>{extra}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            )
        };
        let inner = if path == "/dav/" {
            plain(&format!(
                "<d:current-user-principal><d:href>{PRINCIPAL}</d:href></d:current-user-principal>"
            ))
        } else if path == PRINCIPAL {
            let home = self.home_href.as_deref().unwrap_or(HOME);
            plain(&format!(
                "<c:calendar-home-set><d:href>{home}</d:href></c:calendar-home-set>"
            ))
        } else if path == HOME {
            let mut inner = plain("");
            for (path, collection) in &self.collections {
                inner.push_str(&Self::calendar_xml(path, collection));
            }
            inner
        } else {
            Self::calendar_xml(path, self.collections.get(path)?)
        };
        Some(Self::multistatus(&inner))
    }

    fn report(&mut self, path: &str) -> Option<String> {
        let collection = self.collections.get(path)?;
        let mut inner = String::new();
        for (name, item) in &collection.items {
            inner.push_str(&format!(
                "<d:response><d:href>{path}{name}</d:href><d:propstat><d:prop><d:getetag>\"e{}\"</d:getetag>\
<c:calendar-data>{}</c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                item.etag,
                escape(&item.text)
            ));
        }
        let mut answer = Self::multistatus(&inner);
        if core::mem::take(&mut self.cut_report) {
            // The connection broke after the first task.
            let end = answer.find("</d:response>").unwrap();
            answer.truncate(end + "</d:response>".len());
        }
        Some(answer)
    }

    fn split(path: &str) -> (String, String) {
        let at = path.rfind('/').unwrap() + 1;
        (path[..at].to_string(), path[at..].to_string())
    }

    /// A request for one task; the status and the new ETag.
    fn item(&mut self, call: &Call<'_>, path: &str) -> (u32, Option<u32>) {
        let (folder, name) = Self::split(path);
        self.etag += 1;
        let new_etag = self.etag;
        let body = match call.body {
            Body::None => String::new(),
            Body::Text(text) => text.to_string(),
            Body::File(name) => self.files.text(name),
        };
        let Some(collection) = self.collections.get_mut(&folder) else {
            return (404, None);
        };
        let current = collection.items.get(&name).map(|item| item.etag);
        let version = |etag: u32| format!("\"e{etag}\"");
        let matches = call
            .if_match
            .is_none_or(|wanted| current.is_some_and(|etag| version(etag) == wanted));
        match call.method {
            "GET" => match collection.items.get(&name) {
                Some(item) => {
                    let (file, _) = call.save_to.unwrap();
                    self.files.put(file, item.text.as_bytes());
                    (200, current)
                }
                None => (404, None),
            },
            "PUT" if call.only_new && current.is_some() => (412, None),
            "PUT" | "DELETE" if !matches => (412, None),
            "PUT" => {
                assert_eq!(call.content_type, dav::CALENDAR);
                let status = if current.is_some() { 204 } else { 201 };
                collection.ctag += 1;
                collection.items.insert(
                    name,
                    Resource {
                        etag: new_etag,
                        text: body,
                    },
                );
                (status, Some(new_etag))
            }
            "DELETE" => match collection.items.remove(&name) {
                Some(_) => {
                    collection.ctag += 1;
                    (204, None)
                }
                None => (404, None),
            },
            _ => (405, None),
        }
    }

    fn answer(&mut self, call: &Call<'_>) -> (u32, Option<u32>) {
        assert!(
            call.url.starts_with("https://cloud.example/"),
            "{}",
            call.url
        );
        let path = dav::path(call.url).to_string();
        if self.denies {
            return (401, None);
        }
        let listing = match call.method {
            "PROPFIND" => self.propfind(&path),
            "REPORT" => self.report(&path),
            _ => return self.item(call, &path),
        };
        assert!(call.deep, "a listing asks one level down");
        assert_eq!(call.content_type, dav::XML);
        match listing {
            Some(xml) => {
                let (file, most) = call.save_to.unwrap();
                assert!(xml.len() as u64 <= most);
                self.files.put(file, xml.as_bytes());
                (207, None)
            }
            None => (404, None),
        }
    }
}

impl Wire for Server {
    fn files(&self) -> &dyn Volume {
        &self.files
    }

    fn send(&mut self, call: &Call<'_>) -> Result<u32, Error> {
        self.next += 1;
        let id = self.next;
        self.log
            .push(format!("{} {}", call.method, dav::path(call.url)));
        if self.offline {
            self.ended.push_back((id, Ended::Offline));
            return Ok(id);
        }
        let (status, etag) = self.answer(call);
        if let Some(etag) = etag.filter(|_| !self.no_etags) {
            self.etags.insert(id, format!("\"e{etag}\""));
        }
        self.ended.push_back((id, Ended::Status(status)));
        Ok(id)
    }

    fn etag(&mut self, id: u32) -> Result<String, Error> {
        Ok(self.etags.get(&id).cloned().unwrap_or_default())
    }

    fn now(&mut self) -> Option<i64> {
        Some(self.now)
    }

    fn zone(&mut self, _: i64) -> i32 {
        3600
    }

    fn seed(&mut self) -> Seed {
        Seed {
            unix: self.now,
            micros: u64::from(self.next),
        }
    }
}

/// The reader: its tasks and the engine.
#[derive(Debug, Default)]
struct Reader {
    store: Store,
    sync: Sync,
}

impl Reader {
    /// A whole run from `address`.
    fn run_from(&mut self, server: &mut Server, address: &str) -> Report {
        server.log.clear();
        let mut step = self.sync.start(&mut self.store, server, address, 7);
        loop {
            match step {
                Step::Ended(report) => {
                    assert!(server.ended.is_empty());
                    self.check_file(server);
                    return report;
                }
                Step::Waiting => {
                    let (id, ended) = server.ended.pop_front().expect("a request is under way");
                    step = self.sync.done(&mut self.store, server, id, ended);
                }
            }
        }
    }

    fn run(&mut self, server: &mut Server) -> Report {
        self.run_from(server, ROOT)
    }

    /// What is on the card is what is in memory, whenever a run has ended
    /// with something changed.
    fn check_file(&mut self, server: &Server) {
        if !self.sync.take_dirty() {
            return;
        }
        let Loaded::Store(kept) = keep::load(&server.files).unwrap() else {
            panic!("unreadable");
        };
        assert_eq!(kept.lists, self.store.lists);
        assert_eq!(self.titles_of(&kept), self.titles_of(&self.store));
    }

    fn titles_of(&self, store: &Store) -> Vec<String> {
        store
            .tasks
            .iter()
            .map(|task| task.title().to_string())
            .collect()
    }

    fn list(&self, name: &str) -> u8 {
        self.store
            .lists
            .iter()
            .find(|list| list.name == name)
            .unwrap()
            .id
    }

    fn at(&self, title: &str) -> usize {
        self.store
            .tasks
            .iter()
            .position(|task| task.title() == title && task.shows())
            .unwrap_or_else(|| panic!("no task {title}"))
    }

    /// The titles a list shows, in its order, the done ones marked.
    fn shown(&self, name: &str) -> Vec<String> {
        let list = self.list(name);
        self.store
            .order(View::List(list), TODAY)
            .unwrap()
            .iter()
            .map(|at| {
                let task = &self.store.tasks[usize::from(*at)];
                format!("{}{}", if task.done() { "x " } else { "" }, task.title())
            })
            .collect()
    }

    /// The reader starts again from what is on the card.
    fn restart(&mut self, server: &Server) {
        let Loaded::Store(kept) = keep::load(&server.files).unwrap() else {
            panic!("unreadable");
        };
        self.store = kept;
        self.sync = Sync::default();
    }
}

fn synced() -> (Server, Reader) {
    let mut server = Server::new();
    server.put(
        "tasks",
        "milk",
        "SUMMARY:Buy milk\r\nDUE;VALUE=DATE:20261120\r\nPRIORITY:1\r\n",
    );
    server.put(
        "tasks",
        "call",
        "SUMMARY:Call Anna\r\nSTATUS:NEEDS-ACTION\r\n",
    );
    let mut reader = Reader::default();
    let report = reader.run(&mut server);
    assert_eq!(report, Report::default());
    (server, reader)
}

#[test]
fn the_lists_are_found_from_the_servers_address_and_read() {
    let mut server = Server::new();
    let personal = server.collection("personal", "Personal");
    personal.events_only = true;
    server.collection("home", "Haus & Garten");
    server.put(
        "tasks",
        "milk",
        "SUMMARY:Buy milk\\, 2 l\r\nDUE;VALUE=DATE:20261120\r\nPRIORITY:1\r\n",
    );
    server.put(
        "tasks",
        "late",
        "SUMMARY:Late call\r\nDUE:20261120T233000Z\r\n",
    );
    server.put(
        "tasks",
        "old",
        "SUMMARY:Done long ago\r\nSTATUS:COMPLETED\r\nCOMPLETED:20261001T080000Z\r\n",
    );
    server.put(
        "tasks",
        "new",
        "SUMMARY:Done today\r\nSTATUS:COMPLETED\r\nCOMPLETED:20261117T080000Z\r\n",
    );
    server.put(
        "tasks",
        "daily",
        "SUMMARY:Water the plants\r\nRRULE:FREQ=DAILY\r\n",
    );
    server.put("home", "paint", "SUMMARY:Paint the fence\r\n");
    let mut reader = Reader::default();
    let report = reader.run(&mut server);
    assert_eq!(report, Report::default());
    // From the root to the account, to its calendars, then each list of tasks.
    assert_eq!(
        server.log,
        [
            "PROPFIND /dav/",
            "PROPFIND /dav/principals/anna/",
            "PROPFIND /dav/calendars/anna/",
            "REPORT /dav/calendars/anna/tasks/",
            "REPORT /dav/calendars/anna/home/",
        ]
    );
    assert_eq!(
        reader.store.home,
        "https://cloud.example/dav/calendars/anna/"
    );
    let names: Vec<_> = reader
        .store
        .lists
        .iter()
        .map(|list| list.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Haus & Garten", "Tasks"],
        "the calendar of events is no list"
    );
    assert_eq!(
        reader.shown("Tasks"),
        [
            "Buy milk, 2 l",
            "Late call",
            "Water the plants",
            "x Done today"
        ]
    );
    assert_eq!(reader.shown("Haus & Garten"), ["Paint the fence"]);
    let milk = &reader.store.tasks[reader.at("Buy milk, 2 l")];
    assert_eq!((milk.due, milk.priority), (Some(20_777), 1));
    assert_eq!(milk.href(), "/dav/calendars/anna/tasks/milk.ics");
    assert!(dav::etag_eq(milk.etag(), "\"e101\""));
    // Half past eleven at night in UTC is the next day on this reader.
    assert_eq!(reader.store.tasks[reader.at("Late call")].due, Some(20_778));
    assert!(reader.store.tasks[reader.at("Water the plants")].is(flag::FIXED));
    assert_eq!(reader.store.last_sync, NOW);
    assert_eq!(reader.store.waiting(), 0);
}

#[test]
fn a_run_with_nothing_changed_asks_once() {
    let (mut server, mut reader) = synced();
    assert_eq!(reader.run(&mut server), Report::default());
    assert_eq!(
        server.log,
        ["PROPFIND /dav/calendars/anna/"],
        "straight to the lists, none read"
    );
    // A change on the server: its list is read, and only it.
    server.collection("other", "Other");
    server.put("tasks", "bread", "SUMMARY:Buy bread\r\n");
    reader.run(&mut server);
    assert_eq!(
        server.log,
        [
            "PROPFIND /dav/calendars/anna/",
            "REPORT /dav/calendars/anna/tasks/",
            "REPORT /dav/calendars/anna/other/",
        ]
    );
    assert_eq!(
        reader.shown("Tasks"),
        ["Buy milk", "Buy bread", "Call Anna"]
    );
}

#[test]
fn what_is_changed_offline_waits_and_is_sent_with_everything_else_kept() {
    let (mut server, mut reader) = synced();
    server.offline = true;
    let milk = reader.at("Buy milk");
    reader.store.toggle(milk, NOW + 60).unwrap();
    reader.store.set_priority(milk, 5).unwrap();
    let call = reader.at("Call Anna");
    reader.store.set_title(call, "Call Anna; today").unwrap();
    reader.store.set_due(call, Some(TODAY)).unwrap();
    keep::save(&server.files, &reader.store).unwrap();
    assert_eq!(reader.store.waiting(), 2);
    assert_eq!(reader.run(&mut server).outcome, Outcome::Offline);
    // The reader is switched off and on: what waits is on the card.
    reader.restart(&server);
    assert_eq!(reader.store.waiting(), 2);
    server.offline = false;
    let report = reader.run(&mut server);
    assert_eq!(
        (report.outcome, report.sent, report.conflicts, report.failed),
        (Outcome::Synced, 2, 0, 0)
    );
    assert_eq!(
        server.log,
        [
            "PROPFIND /dav/calendars/anna/",
            "GET /dav/calendars/anna/tasks/call.ics",
            "PUT /dav/calendars/anna/tasks/call.ics",
            "GET /dav/calendars/anna/tasks/milk.ics",
            "PUT /dav/calendars/anna/tasks/milk.ics",
        ]
    );
    assert_eq!(reader.store.waiting(), 0);
    let milk = server.text("tasks", "milk");
    let todo = read_todo(milk.as_bytes(), 100).unwrap().unwrap();
    assert!(todo.done);
    assert_eq!((todo.completed, todo.priority), (Some(NOW + 60), 5));
    assert_eq!(todo.summary, "Buy milk");
    // What the reader does not show is as the other program wrote it.
    for kept in [
        "PRODID:-//Other//EN\r\n",
        "DUE;VALUE=DATE:20261120\r\n",
        "DESCRIPTION:kept by the server\r\n",
        "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\n",
    ] {
        assert!(milk.contains(kept), "{kept} in {milk}");
    }
    let call = read_todo(server.text("tasks", "call").as_bytes(), 100)
        .unwrap()
        .unwrap();
    assert_eq!(call.summary, "Call Anna; today");
    assert_eq!(call.due.map(|due| due.day), Some(TODAY));
    assert!(!call.done);
    // The reader knows the versions it wrote: the next run sends nothing.
    reader.run(&mut server);
    assert_eq!(
        server.log,
        [
            "PROPFIND /dav/calendars/anna/",
            "REPORT /dav/calendars/anna/tasks/"
        ]
    );
    assert_eq!(reader.shown("Tasks"), ["Call Anna; today", "x Buy milk"]);
    assert_eq!(reader.store.waiting(), 0);
}

#[test]
fn a_task_made_on_the_reader_is_made_on_the_server_once() {
    let (mut server, mut reader) = synced();
    let list = reader.list("Tasks");
    let seed = server.seed();
    let at = reader
        .store
        .add_task(list, "Write, the; report", Some(TODAY), seed)
        .unwrap();
    reader.store.set_priority(at, 9).unwrap();
    let uid = reader.store.tasks[at].uid().to_string();
    assert_eq!(reader.store.tasks[at].flags, flag::NEW);
    let report = reader.run(&mut server);
    assert_eq!((report.outcome, report.sent), (Outcome::Synced, 1));
    assert_eq!(
        server.log[1],
        format!("PUT /dav/calendars/anna/tasks/{uid}.ics")
    );
    let todo = read_todo(server.text("tasks", &uid).as_bytes(), 100)
        .unwrap()
        .unwrap();
    assert_eq!(
        (todo.uid.as_str(), todo.summary.as_str()),
        (uid.as_str(), "Write, the; report")
    );
    assert_eq!(
        (todo.due.map(|due| due.day), todo.priority, todo.done),
        (Some(TODAY), 9, false)
    );
    let task = &reader.store.tasks[reader.at("Write, the; report")];
    assert_eq!(task.flags, 0);
    assert_eq!(task.href(), format!("/dav/calendars/anna/tasks/{uid}.ics"));
    reader.run(&mut server);
    assert_eq!(server.names("tasks").len(), 3);
    assert_eq!(reader.store.tasks.len(), 3);
}

#[test]
fn a_task_removed_on_the_reader_is_removed_on_the_server() {
    let (mut server, mut reader) = synced();
    let milk = reader.at("Buy milk");
    reader.store.delete(milk).unwrap();
    assert_eq!(
        reader.shown("Tasks"),
        ["Call Anna"],
        "it shows no more at once"
    );
    assert_eq!(reader.store.tasks.len(), 2, "but waits for the server");
    let report = reader.run(&mut server);
    assert_eq!((report.outcome, report.sent), (Outcome::Synced, 1));
    assert_eq!(server.log[1], "DELETE /dav/calendars/anna/tasks/milk.ics");
    assert_eq!(server.names("tasks"), ["call.ics"]);
    assert_eq!(reader.store.tasks.len(), 1);
}

#[test]
fn what_the_server_removed_goes_from_the_reader() {
    let (mut server, mut reader) = synced();
    server.collection("home", "Home");
    server.put("home", "paint", "SUMMARY:Paint the fence\r\n");
    reader.run(&mut server);
    assert_eq!(reader.store.lists.len(), 2);
    server.remove("tasks", "milk");
    server.collections.remove("/dav/calendars/anna/home/");
    assert_eq!(reader.run(&mut server), Report::default());
    assert_eq!(reader.store.lists.len(), 1);
    assert_eq!(reader.titles_of(&reader.store), ["Call Anna"]);
}

#[test]
fn changed_on_both_sides_the_servers_wins_and_the_readers_is_kept_beside_it() {
    let (mut server, mut reader) = synced();
    // On the reader: ticked off. On the phone meanwhile: another title.
    let milk = reader.at("Buy milk");
    reader.store.toggle(milk, NOW).unwrap();
    server.put(
        "tasks",
        "milk",
        "SUMMARY:Buy oat milk\r\nDUE;VALUE=DATE:20261121\r\n",
    );
    let report = reader.run(&mut server);
    assert_eq!(
        (report.outcome, report.conflicts, report.failed),
        (Outcome::Synced, 1, 0)
    );
    assert_eq!(report.sent, 1, "the reader's own version, as a new task");
    assert_eq!(
        reader.shown("Tasks"),
        ["Buy oat milk", "Call Anna", "x Buy milk"]
    );
    assert_eq!(reader.store.waiting(), 0);
    // The server has both: its own version untouched, the reader's beside it.
    let theirs = read_todo(server.text("tasks", "milk").as_bytes(), 100)
        .unwrap()
        .unwrap();
    assert_eq!(
        (theirs.summary.as_str(), theirs.done),
        ("Buy oat milk", false)
    );
    assert_eq!(server.names("tasks").len(), 3);
    let copy = &reader.store.tasks[reader.at("Buy milk")];
    let ours = read_todo(server.text("tasks", copy.uid()).as_bytes(), 100)
        .unwrap()
        .unwrap();
    assert_eq!((ours.summary.as_str(), ours.done), ("Buy milk", true));
    assert_eq!(ours.due.map(|due| due.day), Some(20_777));
    // And it stays so.
    assert_eq!(reader.run(&mut server), Report::default());
    assert_eq!(
        reader.shown("Tasks"),
        ["Buy oat milk", "Call Anna", "x Buy milk"]
    );
}

#[test]
fn a_change_that_races_the_reading_is_caught_by_the_version() {
    let (mut server, mut reader) = synced();
    let milk = reader.at("Buy milk");
    reader.store.set_title(milk, "Buy two milks").unwrap();
    // The reader believes its list is fresh (the change tag is the same), but
    // the task has another version: only `If-Match` and the fetch can tell.
    server.put("tasks", "milk", "SUMMARY:Buy oat milk\r\n");
    let tasks = server.collection("tasks", "Tasks");
    tasks.ctag -= 1;
    let report = reader.run(&mut server);
    assert_eq!((report.conflicts, report.failed), (1, 0));
    assert_eq!(
        server.log[..3],
        [
            "PROPFIND /dav/calendars/anna/",
            "GET /dav/calendars/anna/tasks/milk.ics",
            // No PUT of the changed task: its copy is made instead.
            &format!(
                "PUT /dav/calendars/anna/tasks/{}.ics",
                reader.store.tasks[reader.at("Buy two milks")].uid()
            ),
        ]
    );
    assert_eq!(
        server.log[3], "REPORT /dav/calendars/anna/tasks/",
        "then the server's version is read"
    );
    // The reader's version has the day and the priority it had.
    assert_eq!(
        reader.shown("Tasks"),
        ["Buy two milks", "Buy oat milk", "Call Anna"]
    );
    let theirs = read_todo(server.text("tasks", "milk").as_bytes(), 100)
        .unwrap()
        .unwrap();
    assert_eq!(theirs.summary, "Buy oat milk");
}

#[test]
fn removed_there_and_changed_here_is_kept_as_a_new_task() {
    let (mut server, mut reader) = synced();
    let milk = reader.at("Buy milk");
    reader.store.set_priority(milk, 5).unwrap();
    server.remove("tasks", "milk");
    let report = reader.run(&mut server);
    assert_eq!(
        (report.outcome, report.conflicts, report.sent),
        (Outcome::Synced, 1, 1)
    );
    assert_eq!(reader.shown("Tasks"), ["Buy milk", "Call Anna"]);
    assert_eq!(server.names("tasks").len(), 2);
    let task = &reader.store.tasks[reader.at("Buy milk")];
    assert_eq!(task.flags, 0);
    // Under an id of the reader's own: the old one is the name of what the
    // server removed, and may be another program's.
    assert!(task.uid().starts_with("pp-"));
    assert_eq!(
        task.href(),
        format!("/dav/calendars/anna/tasks/{}.ics", task.uid())
    );
    let uid = task.uid().to_string();
    let again = read_todo(server.text("tasks", &uid).as_bytes(), 100)
        .unwrap()
        .unwrap();
    assert_eq!((again.summary.as_str(), again.priority), ("Buy milk", 5));
    // Removed on the reader and changed there: the server's version is back.
    let call = reader.at("Call Anna");
    reader.store.delete(call).unwrap();
    server.put("tasks", "call", "SUMMARY:Call Anna tomorrow\r\n");
    let report = reader.run(&mut server);
    assert_eq!(report.failed, 0);
    assert_eq!(reader.shown("Tasks"), ["Buy milk", "Call Anna tomorrow"]);
    assert_eq!(server.names("tasks").len(), 2);
}

#[test]
fn a_power_cut_after_a_sending_makes_no_second_task_and_no_conflict() {
    let (mut server, mut reader) = synced();
    let list = reader.list("Tasks");
    let seed = server.seed();
    reader.store.add_task(list, "New here", None, seed).unwrap();
    let milk = reader.at("Buy milk");
    reader.store.toggle(milk, NOW).unwrap();
    keep::save(&server.files, &reader.store).unwrap();
    let before = server.files.text(keep::FILE);
    assert_eq!(reader.run(&mut server).sent, 2);
    // The power went before the card knew what was sent: the reader starts
    // again with the file as it was before the run.
    server.files.put(keep::FILE, before.as_bytes());
    reader.restart(&server);
    assert_eq!(reader.store.waiting(), 2);
    let report = reader.run(&mut server);
    assert_eq!(
        (report.outcome, report.conflicts, report.failed),
        (Outcome::Synced, 0, 0)
    );
    assert_eq!(
        reader.shown("Tasks"),
        ["Call Anna", "New here", "x Buy milk"]
    );
    assert_eq!(server.names("tasks").len(), 3);
    assert_eq!(reader.store.waiting(), 0);
    assert_eq!(reader.run(&mut server), Report::default());
}

#[test]
fn an_answer_cut_short_removes_nothing() {
    let (mut server, mut reader) = synced();
    server.put("tasks", "bread", "SUMMARY:Buy bread\r\n");
    server.cut_report = true;
    let report = reader.run(&mut server);
    assert_eq!((report.outcome, report.failed), (Outcome::Synced, 1));
    // What the answer still had is taken; what it did not reach stays.
    assert_eq!(
        reader.shown("Tasks"),
        ["Buy milk", "Buy bread", "Call Anna"]
    );
    server.log.clear();
    // The next run reads the list again, whole.
    assert_eq!(reader.run(&mut server), Report::default());
    assert_eq!(
        reader.shown("Tasks"),
        ["Buy milk", "Buy bread", "Call Anna"]
    );
}

#[test]
fn a_server_that_tells_no_versions_still_works() {
    let (mut server, mut reader) = synced();
    server.no_etags = true;
    let milk = reader.at("Buy milk");
    reader.store.toggle(milk, NOW).unwrap();
    assert_eq!(reader.run(&mut server).sent, 1);
    // The list is read once more, which tells the version.
    assert_eq!(
        server.log.last().map(String::as_str),
        Some("REPORT /dav/calendars/anna/tasks/")
    );
    assert!(!reader.store.tasks[reader.at("Buy milk")].etag().is_empty());
    // The next run has nothing to do; nothing is a conflict.
    let report = reader.run(&mut server);
    assert_eq!((report.conflicts, report.sent), (0, 0));
    let milk = reader.at("Buy milk");
    assert!(!reader.store.tasks[milk].etag().is_empty());
    reader.store.toggle(milk, 0).unwrap();
    assert_eq!(reader.run(&mut server).sent, 1);
    assert!(
        !read_todo(server.text("tasks", "milk").as_bytes(), 100)
            .unwrap()
            .unwrap()
            .done
    );
}

#[test]
fn a_change_after_a_sending_without_a_version_is_still_caught_as_a_conflict() {
    let (mut server, mut reader) = synced();
    server.no_etags = true;
    let milk = reader.at("Buy milk");
    reader.store.set_title(milk, "Buy two milks").unwrap();
    assert_eq!(reader.run(&mut server).sent, 1);
    // Changed on the phone and on the reader again: the server's version
    // stays, the reader's is kept beside it.
    server.put("tasks", "milk", "SUMMARY:Buy oat milk\r\n");
    let milk = reader.at("Buy two milks");
    reader.store.set_title(milk, "Buy three milks").unwrap();
    let report = reader.run(&mut server);
    assert_eq!((report.conflicts, report.failed), (1, 0));
    let theirs = read_todo(server.text("tasks", "milk").as_bytes(), 100)
        .unwrap()
        .unwrap();
    assert_eq!(theirs.summary, "Buy oat milk");
    assert_eq!(
        reader.shown("Tasks"),
        ["Buy three milks", "Buy oat milk", "Call Anna"]
    );
}

#[test]
fn a_refused_password_stops_the_runs_and_nothing_is_lost() {
    let (mut server, mut reader) = synced();
    let milk = reader.at("Buy milk");
    reader.store.toggle(milk, NOW).unwrap();
    server.denies = true;
    assert_eq!(reader.run(&mut server).outcome, Outcome::Denied);
    assert!(reader.sync.blocked());
    assert_eq!(reader.store.waiting(), 1);
    reader.sync.account_changed();
    server.denies = false;
    assert_eq!(reader.run(&mut server).sent, 1);
}

#[test]
fn an_answer_pointing_to_another_server_or_to_http_is_not_followed() {
    // The server's `answer` fails the test on any address that is not on
    // `https://cloud.example/`: nothing, nor the password, goes there.
    for home in [
        "http://cloud.example/dav/calendars/anna/",
        "https://evil.example/dav/calendars/anna/",
        "https://cloud.example:8443/dav/calendars/anna/",
    ] {
        let mut server = Server::new();
        server.home_href = Some(home.into());
        let mut reader = Reader::default();
        assert_eq!(reader.run(&mut server).outcome, Outcome::Foreign, "{home}");
        assert!(reader.store.home.is_empty());
        assert_eq!(
            server.log,
            ["PROPFIND /dav/", "PROPFIND /dav/principals/anna/"]
        );
    }
    // A place kept from before on another server is not used either.
    let (mut server, mut reader) = synced();
    reader.store.home = "http://cloud.example/dav/calendars/anna/".into();
    assert_eq!(reader.run(&mut server).outcome, Outcome::Synced);
    assert_eq!(reader.store.home, format!("https://cloud.example{HOME}"));
}

#[test]
fn an_address_without_lists_says_so_and_keeps_what_is_there() {
    let (mut server, mut reader) = synced();
    reader.store.forget_server();
    let report = reader.run_from(&mut server, "https://cloud.example/nothing/");
    assert_eq!(report.outcome, Outcome::NotFound);
    assert_eq!(reader.store.tasks.len(), 2);
    assert_eq!(reader.run_from(&mut server, "").outcome, Outcome::NoAccount);
    // One list's own address is enough.
    reader.store.forget_server();
    server.collection("home", "Home");
    let report = reader.run_from(
        &mut server,
        "https://cloud.example/dav/calendars/anna/tasks/",
    );
    assert_eq!(report.outcome, Outcome::Synced);
    assert_eq!(reader.store.lists.len(), 1);
    assert_eq!(reader.store.tasks.len(), 2);
}

#[test]
fn a_local_list_is_never_sent() {
    let (mut server, mut reader) = synced();
    let own = reader.store.add_list("Private", "").unwrap();
    let seed = server.seed();
    let at = reader
        .store
        .add_task(own, "Secret plan", None, seed)
        .unwrap();
    reader.store.toggle(at, NOW).unwrap();
    assert_eq!(reader.store.waiting(), 0);
    assert_eq!(reader.run(&mut server), Report::default());
    assert_eq!(server.log, ["PROPFIND /dav/calendars/anna/"]);
    assert_eq!(reader.shown("Private"), ["x Secret plan"]);
}

#[test]
fn lists_that_moved_on_the_server_are_found_again() {
    let (mut server, mut reader) = synced();
    // The address kept from the first run is no more; the account's is.
    reader.store.home = "https://cloud.example/dav/calendars/old/".into();
    assert_eq!(reader.run(&mut server), Report::default());
    assert_eq!(
        server.log[..3],
        [
            "PROPFIND /dav/calendars/old/",
            "PROPFIND /dav/",
            "PROPFIND /dav/principals/anna/",
        ]
    );
    assert_eq!(
        reader.store.home,
        "https://cloud.example/dav/calendars/anna/"
    );
    assert_eq!(reader.shown("Tasks"), ["Buy milk", "Call Anna"]);
}

#[test]
fn the_readers_own_first_list_goes_when_the_accounts_lists_come_unless_it_was_used() {
    for used in [false, true] {
        let mut server = Server::new();
        server.put("tasks", "milk", "SUMMARY:Buy milk\r\n");
        let mut reader = Reader::default();
        let first = reader.store.add_list("My tasks", "").unwrap();
        reader.store.list_mut(first).unwrap().ctag = crate::model::FIRST.into();
        let own = reader.store.add_list("Private", "").unwrap();
        if used {
            let seed = server.seed();
            reader.store.add_task(first, "Mine", None, seed).unwrap();
        }
        assert_eq!(reader.run(&mut server), Report::default());
        let names: Vec<_> = reader
            .store
            .lists
            .iter()
            .map(|list| list.name.as_str())
            .collect();
        if used {
            assert_eq!(names, ["My tasks", "Private", "Tasks"]);
            assert_eq!(reader.shown("My tasks"), ["Mine"]);
        } else {
            assert_eq!(
                names,
                ["Private", "Tasks"],
                "one made by hand stays, empty or not"
            );
        }
        assert!(reader.store.list(own).is_some());
        assert_eq!(
            server
                .log
                .iter()
                .filter(|line| line.starts_with("PUT"))
                .count(),
            0
        );
    }
}
