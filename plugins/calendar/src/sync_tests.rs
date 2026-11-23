//! The engine against a CalDAV/CardDAV server in memory, a subscription and a
//! file of the card: calendars are found from the server's address, events
//! with zones, repeats, exceptions and changed times are read, contacts give
//! birthdays, a source that fails keeps what the reader knew, a refused name
//! is left alone by runs that start by themselves, and no network ends the
//! run with nothing lost.
#![allow(clippy::unwrap_used, reason = "fixtures")]
use alloc::{
    collections::{BTreeMap, VecDeque},
    format,
    string::{String, ToString},
    vec::Vec,
};

use paperpaw_caldav::dav;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::files::Volume;

use crate::keep::{self, Loaded};
use crate::model::{Kind, Show, Store, state};
use crate::sync::{Call, Ended, Outcome, Report, Step, Sync, Wire};
use crate::test_volume::Fake;

const ROOT: &str = "https://cloud.example/dav/";
const HOME: &str = "/dav/calendars/anna/";
const PRINCIPAL: &str = "/dav/principals/anna/";
const NOW: i64 = 1_795_000_000;
/// The day of `NOW` (2026-11-18, a Wednesday).
const TODAY: i32 = 20_775;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

fn ical(events: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\n{events}END:VCALENDAR\r\n")
}

/// A server, a card and the device around the engine.
#[derive(Debug, Default)]
struct World {
    files: Fake,
    card: Fake,
    /// Collections of events by path: their resources' texts.
    calendars: BTreeMap<String, Vec<String>>,
    /// Address books by path.
    books: BTreeMap<String, Vec<String>>,
    /// A subscription's text by path.
    feeds: BTreeMap<String, String>,
    ended: VecDeque<(u32, Ended)>,
    /// Every request, as `METHOD path`.
    log: Vec<String>,
    /// The bodies of the REPORTs.
    bodies: Vec<String>,
    next: u32,
    offline: bool,
    denies: bool,
    /// A calendar whose `REPORT` fails with a server error.
    broken: Option<String>,
}

impl World {
    fn new() -> Self {
        let world = Self::default();
        world.card.put("calendars/work.ics", ical("").as_bytes());
        world
    }

    fn calendar_xml(path: &str, name: &str, comp: &str) -> String {
        format!(
            "<d:response><d:href>{path}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/>\
<c:calendar/></d:resourcetype><d:displayname>{name}</d:displayname>\
<c:supported-calendar-component-set><c:comp name=\"{comp}\"/></c:supported-calendar-component-set>\
</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
        )
    }

    fn multistatus(inner: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\" \
xmlns:card=\"urn:ietf:params:xml:ns:carddav\">{inner}</d:multistatus>"
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
            plain(&format!(
                "<c:calendar-home-set><d:href>{HOME}</d:href></c:calendar-home-set>"
            ))
        } else if path == HOME {
            let mut inner = plain("");
            inner.push_str(&Self::calendar_xml(
                &format!("{HOME}tasks/"),
                "Tasks",
                "VTODO",
            ));
            for path in self.calendars.keys() {
                inner.push_str(&Self::calendar_xml(path, "Personal", "VEVENT"));
            }
            inner
        } else if self.books.contains_key(path) {
            format!(
                "<d:response><d:href>{path}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/>\
<card:addressbook/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            )
        } else {
            return None;
        };
        Some(Self::multistatus(&inner))
    }

    fn report(&self, path: &str) -> Option<String> {
        let (items, tag) = if let Some(items) = self.calendars.get(path) {
            (items, "c:calendar-data")
        } else {
            (self.books.get(path)?, "card:address-data")
        };
        let mut inner = String::new();
        for (n, text) in items.iter().enumerate() {
            inner.push_str(&format!(
                "<d:response><d:href>{path}{n}.ics</d:href><d:propstat><d:prop><d:getetag>\"e{n}\"</d:getetag>\
<{tag}>{}</{tag}></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                escape(text)
            ));
        }
        Some(Self::multistatus(&inner))
    }

    fn answer(&mut self, call: &Call<'_>) -> u32 {
        assert!(
            call.url.starts_with("https://cloud.example/"),
            "{}",
            call.url
        );
        let path = dav::path(call.url).to_string();
        if self.denies {
            return 401;
        }
        let xml = match call.method {
            "GET" => {
                return match self.feeds.get(&path) {
                    Some(text) => {
                        self.files.put(call.save_to.0, text.as_bytes());
                        200
                    }
                    None => 404,
                };
            }
            "PROPFIND" => self.propfind(&path),
            "REPORT" if self.broken.as_ref() == Some(&path) => return 500,
            "REPORT" => {
                self.bodies.push(call.body.unwrap_or("").to_string());
                self.report(&path)
            }
            _ => return 405,
        };
        assert!(call.deep && call.content_type == dav::XML);
        match xml {
            Some(xml) => {
                assert!(xml.len() as u64 <= call.save_to.1);
                self.files.put(call.save_to.0, xml.as_bytes());
                207
            }
            None => 404,
        }
    }
}

impl Wire for World {
    fn files(&self) -> &dyn Volume {
        &self.files
    }

    fn card(&self) -> &dyn Volume {
        &self.card
    }

    fn send(&mut self, call: &Call<'_>, store: &Store) -> Result<u32, Error> {
        assert!(store.source(call.source).is_some());
        self.next += 1;
        let id = self.next;
        self.log
            .push(format!("{} {}", call.method, dav::path(call.url)));
        if self.offline {
            self.ended.push_back((id, Ended::Offline));
            return Ok(id);
        }
        let status = self.answer(call);
        self.ended.push_back((id, Ended::Status(status)));
        Ok(id)
    }

    fn now(&mut self) -> Option<i64> {
        Some(NOW)
    }

    fn zone(&mut self, _: i64) -> i32 {
        3600
    }
}

/// The reader: its sources and events, and the engine.
#[derive(Debug, Default)]
struct Reader {
    store: Store,
    sync: Sync,
}

impl Reader {
    fn run(&mut self, world: &mut World, auto: bool) -> Report {
        world.log.clear();
        let mut step = self.sync.start(&mut self.store, world, None, auto);
        loop {
            match step {
                Step::Ended(report) => {
                    assert!(world.ended.is_empty());
                    self.check_file(world);
                    return report;
                }
                Step::Waiting => {
                    let (id, ended) = world.ended.pop_front().expect("a request is under way");
                    step = self.sync.done(&mut self.store, world, id, ended);
                }
            }
        }
    }

    /// What is on the card is what is in memory.
    fn check_file(&mut self, world: &World) {
        if !self.sync.take_dirty() {
            return;
        }
        let Loaded::Store(kept) = keep::load(&world.files).unwrap() else {
            panic!("unreadable");
        };
        assert_eq!(kept.sources, self.store.sources);
        assert_eq!(kept.entries, self.store.entries);
    }

    fn add(&mut self, kind: Kind, name: &str, url: &str) -> u8 {
        self.store.add_source(kind, name, url, "anna").unwrap()
    }

    /// The events from today for `days`, a line each: the day offset, the
    /// hour and the title.
    fn agenda(&self, days: i32) -> Vec<String> {
        self.store
            .occurrences(TODAY, TODAY + days - 1, Show::All)
            .unwrap()
            .iter()
            .map(|occ| {
                let entry = &self.store.entries[usize::from(occ.entry)];
                let at = if occ.whole_day() {
                    "all day".to_string()
                } else {
                    format!("{:02}:{:02}", occ.secs / 3600, occ.secs / 60 % 60)
                };
                format!("+{} {at} {}", occ.day - TODAY, entry.title)
            })
            .collect()
    }
}

const PERSONAL: &str = "/dav/calendars/anna/personal/";

fn personal(world: &mut World) {
    world.calendars.insert(
        PERSONAL.into(),
        alloc::vec![
            // Mondays at 10:00 in Berlin, one Monday left out, one moved.
            ical(
                "BEGIN:VEVENT\r\nUID:team@x\r\nSUMMARY:Team meeting\r\nLOCATION:Room 4\r\n\
DTSTART;TZID=Europe/Berlin:20261102T100000\r\nDTEND;TZID=Europe/Berlin:20261102T110000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO\r\nEXDATE;TZID=Europe/Berlin:20261130T100000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:team@x\r\nRECURRENCE-ID;TZID=Europe/Berlin:20261123T100000\r\n\
SUMMARY:Team meeting (moved)\r\nDTSTART;TZID=Europe/Berlin:20261124T150000\r\n\
DTEND;TZID=Europe/Berlin:20261124T160000\r\nEND:VEVENT\r\n"
            ),
            // Today, in UTC: 13:30 UTC is 14:30 in Berlin.
            ical(
                "BEGIN:VEVENT\r\nUID:call@x\r\nSUMMARY:Call\r\nDTSTART:20261118T133000Z\r\n\
DURATION:PT45M\r\nEND:VEVENT\r\n"
            ),
            // All day, three days.
            ical(
                "BEGIN:VEVENT\r\nUID:trip@x\r\nSUMMARY:Trip\r\nDTSTART;VALUE=DATE:20261120\r\n\
DTEND;VALUE=DATE:20261123\r\nEND:VEVENT\r\n"
            ),
            // Far outside the window, and cancelled.
            ical("BEGIN:VEVENT\r\nUID:old@x\r\nSUMMARY:Old\r\nDTSTART:20240101T080000Z\r\nEND:VEVENT\r\n"),
            ical(
                "BEGIN:VEVENT\r\nUID:no@x\r\nSUMMARY:Cancelled\r\nSTATUS:CANCELLED\r\n\
DTSTART:20261119T080000Z\r\nEND:VEVENT\r\n"
            ),
        ],
    );
}

#[test]
fn a_caldav_account_is_found_from_its_address_and_its_events_are_read() {
    let mut world = World::new();
    personal(&mut world);
    let mut reader = Reader::default();
    let id = reader.add(Kind::Caldav, "Work", ROOT);
    let report = reader.run(&mut world, false);
    assert_eq!(report.outcome, Outcome::Synced);
    assert_eq!((report.read, report.failed, report.denied), (1, 0, 0));
    // The server's own address led to the principal, the home and the
    // calendar of events; the calendar of tasks was left alone.
    assert_eq!(
        world.log,
        [
            "PROPFIND /dav/",
            format!("PROPFIND {PRINCIPAL}").as_str(),
            format!("PROPFIND {HOME}").as_str(),
            format!("REPORT {PERSONAL}").as_str()
        ]
    );
    // Only the window of days is asked for.
    assert!(
        world.bodies[0].contains("time-range start=\"20260919T000000Z\" end=\"20270117T000000Z\"")
    );
    assert_eq!(reader.store.source(id).unwrap().state, state::OK);
    assert_eq!(reader.store.source(id).unwrap().synced, NOW);
    assert_eq!(
        reader.agenda(14),
        [
            "+0 14:30 Call",
            "+2 all day Trip",
            "+3 all day Trip",
            "+4 all day Trip",
            // Monday the 23rd is the moved one on Tuesday; the 30th is left out.
            "+6 15:00 Team meeting (moved)",
        ]
    );
    // Mondays at 10:00 Berlin time: the 16th is past, the 23rd moved, the
    // 30th left out, December the 7th is back.
    let december = reader
        .store
        .occurrences(TODAY + 19, TODAY + 19, Show::All)
        .unwrap();
    assert_eq!(december.len(), 1);
    assert_eq!(december[0].secs, 10 * 3600);
    assert_eq!(
        reader
            .store
            .entries
            .iter()
            .find(|e| e.title == "Team meeting")
            .unwrap()
            .place,
        "Room 4"
    );
    // Nothing outside the window, nothing cancelled, no loose changed time.
    assert!(
        reader
            .store
            .entries
            .iter()
            .all(|e| e.title != "Old" && e.title != "Cancelled" && e.over.is_none())
    );
}

#[test]
fn a_subscription_a_file_and_contacts() {
    let mut world = World::new();
    world.feeds.insert(
        "/holidays.ics".into(),
        ical("BEGIN:VEVENT\nUID:h1\nSUMMARY:Day off\nDTSTART;VALUE=DATE:20261119\nEND:VEVENT\n"),
    );
    world.card.put(
        "calendars/work.ics",
        ical("BEGIN:VEVENT\nUID:w1\nSUMMARY:From the card\nDTSTART:20261118T080000\nEND:VEVENT\n")
            .as_bytes(),
    );
    world.books.insert(
        "/card/anna/".into(),
        alloc::vec![
            "BEGIN:VCARD\r\nFN:Jane Doe\r\nBDAY:19851120\r\nEND:VCARD\r\n".into(),
            "BEGIN:VCARD\r\nFN:Max\r\nBDAY:--1203\r\nANNIVERSARY:20100601\r\nEND:VCARD\r\n".into(),
        ],
    );
    let mut reader = Reader::default();
    reader.add(Kind::Ics, "Holidays", "webcal://cloud.example/holidays.ics");
    reader.add(Kind::File, "Work", "calendars/work.ics");
    reader.add(
        Kind::Contacts,
        "Contacts",
        "https://cloud.example/card/anna/",
    );
    let report = reader.run(&mut world, false);
    assert_eq!((report.read, report.failed), (3, 0));
    assert_eq!(
        world.log,
        [
            "GET /holidays.ics",
            "PROPFIND /card/anna/",
            "REPORT /card/anna/"
        ]
    );
    assert_eq!(
        reader.agenda(20),
        [
            "+0 08:00 From the card",
            "+1 all day Day off",
            "+2 all day Jane Doe",
            "+15 all day Max",
        ]
    );
    let jane = reader
        .store
        .entries
        .iter()
        .find(|e| e.title == "Jane Doe")
        .unwrap();
    assert!(jane.known);
    let occs = reader
        .store
        .occurrences(TODAY + 2, TODAY + 2, Show::Birthdays)
        .unwrap();
    assert_eq!(occs[0].age, 41);
    // Max's birthday has no year; his anniversary is a year known: June.
    assert_eq!(reader.store.entries.len(), 5);
}

#[test]
fn a_source_that_fails_keeps_what_the_reader_knew() {
    let mut world = World::new();
    personal(&mut world);
    let mut reader = Reader::default();
    let id = reader.add(Kind::Caldav, "Work", ROOT);
    reader.run(&mut world, false);
    let known = reader.agenda(14);
    assert!(!known.is_empty());
    // The calendar is gone from the server: a run that finds none fails and
    // leaves the events.
    world.calendars.clear();
    let report = reader.run(&mut world, false);
    assert_eq!((report.read, report.failed), (0, 1));
    assert_eq!(reader.store.source(id).unwrap().state, state::FAILED);
    assert_eq!(reader.agenda(14), known);
    // No network is no failure of the source.
    world.offline = true;
    personal(&mut world);
    let report = reader.run(&mut world, false);
    assert_eq!(report.outcome, Outcome::Offline);
    assert_eq!(reader.agenda(14), known);
}

#[test]
fn a_refused_name_is_left_alone_by_runs_that_start_by_themselves() {
    let mut world = World::new();
    personal(&mut world);
    world.denies = true;
    let mut reader = Reader::default();
    let id = reader.add(Kind::Caldav, "Work", ROOT);
    let report = reader.run(&mut world, true);
    assert_eq!(report.denied, 1);
    assert_eq!(reader.store.source(id).unwrap().state, state::DENIED);
    // The next automatic run does not even ask.
    let report = reader.run(&mut world, true);
    assert_eq!(report.outcome, Outcome::NoSource);
    assert!(world.log.is_empty());
    // One the reader asks for does, and works once the password is right.
    world.denies = false;
    let report = reader.run(&mut world, false);
    assert_eq!(report.read, 1);
    assert_eq!(reader.store.source(id).unwrap().state, state::OK);
}

#[test]
fn more_events_than_the_reader_holds_are_counted_not_lost_silently() {
    let mut world = World::new();
    let mut events = String::new();
    for n in 0..260 {
        events.push_str(&format!(
            "BEGIN:VEVENT\nUID:e{n}\nSUMMARY:Event {n}\nDTSTART:20261120T{:02}0000Z\nEND:VEVENT\n",
            n % 24
        ));
    }
    world.feeds.insert("/big.ics".into(), ical(&events));
    let mut reader = Reader::default();
    let id = reader.add(Kind::Ics, "Big", "https://cloud.example/big.ics");
    let report = reader.run(&mut world, false);
    assert_eq!(reader.store.entries.len(), crate::model::MAX_ENTRIES);
    assert_eq!(report.skipped, 20);
    assert_eq!(reader.store.source(id).unwrap().state, state::PARTLY);
}

const FAMILY: &str = "/dav/calendars/anna/family/";

fn family(world: &mut World, title: &str) {
    world.calendars.insert(
        FAMILY.into(),
        alloc::vec![ical(&format!(
            "BEGIN:VEVENT\r\nUID:zoo@x\r\nSUMMARY:{title}\r\nDTSTART;VALUE=DATE:20261119\r\nEND:VEVENT\r\n"
        ))],
    );
}

#[test]
fn a_calendar_that_fails_keeps_its_events_while_the_others_are_read() {
    let mut world = World::new();
    personal(&mut world);
    family(&mut world, "Zoo");
    let mut reader = Reader::default();
    let id = reader.add(Kind::Caldav, "Work", ROOT);
    reader.run(&mut world, false);
    let known = reader.agenda(14);
    assert!(known.contains(&"+1 all day Zoo".to_string()), "{known:?}");
    assert!(known.contains(&"+0 14:30 Call".to_string()), "{known:?}");
    // The family calendar (read first) changed; the personal one fails.
    family(&mut world, "Zoo trip");
    world.broken = Some(PERSONAL.into());
    let report = reader.run(&mut world, false);
    assert_eq!(report.read, 1);
    assert_eq!(reader.store.source(id).unwrap().state, state::PARTLY);
    let now = reader.agenda(14);
    let want: Vec<String> = known
        .iter()
        .map(|line| line.replace("Zoo", "Zoo trip"))
        .collect();
    assert_eq!(now, want);
    // Read again whole, and a calendar gone from the account takes its events along.
    world.broken = None;
    world.calendars.remove(FAMILY);
    reader.run(&mut world, false);
    assert_eq!(reader.store.source(id).unwrap().state, state::OK);
    let without: Vec<String> = known
        .into_iter()
        .filter(|line| !line.contains("Zoo"))
        .collect();
    assert_eq!(reader.agenda(14), without);
}

#[test]
fn a_page_that_is_no_calendar_replaces_nothing() {
    let mut world = World::new();
    world.feeds.insert(
        "/holidays.ics".into(),
        ical("BEGIN:VEVENT\nUID:h1\nSUMMARY:Day off\nDTSTART;VALUE=DATE:20261119\nEND:VEVENT\n"),
    );
    let mut reader = Reader::default();
    let id = reader.add(Kind::Ics, "Holidays", "https://cloud.example/holidays.ics");
    reader.run(&mut world, false);
    assert_eq!(reader.agenda(3), ["+1 all day Day off"]);
    // A captive portal answers in its place, then a text cut short.
    for answer in [
        "<html><body>Sign in to the Wi-Fi</body></html>".to_string(),
        ical("BEGIN:VEVENT\nUID:h2\nSUMMARY:Other\nDTSTART;VALUE=DATE:20261119\nEND:VEVENT\n")
            .replace("END:VCALENDAR\r\n", ""),
    ] {
        world.feeds.insert("/holidays.ics".into(), answer);
        let report = reader.run(&mut world, false);
        assert_eq!(report.failed, 1);
        assert_eq!(reader.store.source(id).unwrap().state, state::FAILED);
        assert_eq!(reader.agenda(3), ["+1 all day Day off"]);
    }
}
