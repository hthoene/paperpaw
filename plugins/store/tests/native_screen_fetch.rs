//! The fetch screen's rules (slice 7, host-free): what is counted, how a fetch
//! ends, what a row does, where the parts go.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_ask::{Say, size_text};
use paperpaw_plugin_store::native_model::Entry;
use paperpaw_plugin_store::native_screen_fetch::{
    BY, Core, IDS, Kind, Layout, Metrics, Phase, Row, Tally, Why, activate, open_next, progress,
    ready_key, request_ids, rows, settle, step_focus, tally,
};
use paperpaw_plugin_store::native_screens::{Installs, Next, Step, Trouble};
use paperpaw_sdk::Handled;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use std::cell::RefCell;
use std::collections::BTreeMap;

/// What is going on, as the test sets it.
#[derive(Default)]
struct Going {
    steps: RefCell<BTreeMap<String, Step>>,
    queued: RefCell<bool>,
    restart: RefCell<Vec<String>>,
}

impl Going {
    fn set(&self, id: &str, step: Step) {
        self.steps.borrow_mut().insert(id.to_string(), step);
    }
    fn queue(&self, queued: bool) {
        *self.queued.borrow_mut() = queued;
    }
}

impl Installs for Going {
    fn step(&self, id: &str) -> Step {
        self.steps.borrow().get(id).cloned().unwrap_or(Step::Idle)
    }
    fn queued(&self) -> bool {
        *self.queued.borrow()
    }
    fn restart_needed(&self, id: &str) -> bool {
        self.restart.borrow().iter().any(|r| r == id)
    }
}

/// A tally with some fields set.
fn counted(set: impl FnOnce(&mut Tally)) -> Tally {
    let mut tally = Tally::default();
    set(&mut tally);
    tally
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

fn entry(id: &str, name: &str, size: u64) -> (String, Option<Entry>) {
    (
        id.to_string(),
        Some(Entry {
            id: id.into(),
            name: name.into(),
            size,
            ..Entry::default()
        }),
    )
}

fn request(by: &str) -> Core {
    let listed = [
        entry("pack-a", "Letters A", 1000),
        entry("pack-b", "Letters B", 3000),
    ];
    Core::new(Kind::Request, by, strings(&["pack-a", "pack-b"]), &listed).unwrap()
}

#[test]
fn the_count_is_the_interface_6_count() {
    let going = Going::default();
    let (ids, sizes) = (strings(&["a", "b", "c"]), [1000, 3000, 0]);
    let t = tally(&ids, &sizes, &going);
    assert_eq!(t, counted(|t| t.missing = true));
    // Installed counts whole, the one downloading by its bytes, never more
    // than its size.
    going.set("a", Step::Installed);
    going.set(
        "b",
        Step::Downloading {
            done: 9_000,
            total: 3000,
        },
    );
    let t = tally(&ids, &sizes, &going);
    assert_eq!((t.done, t.whole, t.busy), (4000, 1, true));
    assert_eq!(
        progress(&t, 4000, 3),
        (990, 4000),
        "never whole before the end"
    );
    going.set(
        "b",
        Step::Downloading {
            done: 1_000,
            total: 3000,
        },
    );
    let t = tally(&ids, &sizes, &going);
    assert_eq!(progress(&t, 4000, 3), (500, 2000));
    // No sizes known: the plugins installed are the share.
    let t = tally(&ids, &[0, 0, 0], &going);
    assert_eq!(progress(&t, 0, 3), (333, 0));
    assert_eq!(progress(&Tally::default(), 0, 0), (0, 0));
}

#[test]
fn a_fetch_of_plugins_ends_as_the_installs_say() {
    let ok = counted(|t| t.whole = 2);
    for kind in [Kind::Request, Kind::All] {
        assert_eq!(settle(kind, &ok, false, Trouble::None), Phase::Done);
        // The queue still holds plugins, or one is going on: not over.
        assert_eq!(settle(kind, &ok, true, Trouble::None), Phase::Working);
        let busy = counted(|t| (t.whole, t.busy) = (2, true));
        assert_eq!(settle(kind, &busy, false, Trouble::None), Phase::Working);
        let failed = counted(|t| (t.whole, t.failed) = (2, true));
        assert_eq!(
            settle(kind, &failed, false, Trouble::None),
            Phase::Failed(Why::Missing)
        );
        let wait = counted(|t| (t.whole, t.restart) = (2, true));
        assert_eq!(settle(kind, &wait, false, Trouble::None), Phase::Restart);
    }
    // A plugin of a request that is not installed is not here; for Update all
    // a plugin set aside is the queue's to say.
    let missing = counted(|t| (t.whole, t.missing) = (2, true));
    assert_eq!(
        settle(Kind::Request, &missing, false, Trouble::None),
        Phase::Failed(Why::Missing)
    );
    assert_eq!(
        settle(Kind::All, &missing, false, Trouble::None),
        Phase::Done
    );
}

#[test]
fn a_fetch_of_the_list_ends_as_the_loader_says() {
    let none = Tally::default();
    for (list, phase) in [
        (Trouble::Loading, Phase::Working),
        (Trouble::None, Phase::Done),
        (Trouble::Offline, Phase::Failed(Why::Offline)),
        (Trouble::Failed, Phase::Failed(Why::Unreachable)),
    ] {
        assert_eq!(settle(Kind::List, &none, true, list), phase);
    }
}

#[test]
fn the_rows_of_the_end_do_what_they_say() {
    assert!(rows(Phase::Working).is_empty() && rows(Phase::Done).is_empty());
    assert_eq!(rows(Phase::Failed(Why::Missing)), [Row::Retry, Row::Close]);
    assert_eq!(rows(Phase::Restart), [Row::RestartNow, Row::Later]);
    let ids = strings(&["pack-a"]);
    assert_eq!(
        activate(Kind::Request, "reader", &ids, Row::Retry).unwrap(),
        Next::Again {
            by: "reader".into(),
            ids: ids.clone()
        }
    );
    assert_eq!(
        activate(Kind::All, "store", &ids, Row::Retry).unwrap(),
        Next::UpdateAll
    );
    assert_eq!(
        activate(Kind::List, "store", &ids, Row::Retry).unwrap(),
        Next::FetchList
    );
    assert_eq!(
        activate(Kind::Request, "x", &ids, Row::RestartNow).unwrap(),
        Next::Restart
    );
    for row in [Row::Close, Row::Later] {
        assert_eq!(
            activate(Kind::Request, "x", &ids, row).unwrap(),
            Next::Close
        );
    }
}

#[test]
fn a_request_works_to_its_end_and_says_ready() {
    let going = Going::default();
    going.queue(true);
    let mut core = request("reader");
    assert_eq!(core.refresh(&going, Trouble::None), Next::None);
    going.set(
        "pack-a",
        Step::Downloading {
            done: 500,
            total: 1000,
        },
    );
    assert_eq!(core.refresh(&going, Trouble::None), Next::Redraw);
    assert_eq!((core.permille, core.done), (125, 500));
    assert_eq!(
        core.refresh(&going, Trouble::None),
        Next::None,
        "nothing moved"
    );
    going.set("pack-a", Step::Installed);
    going.set("pack-b", Step::Installed);
    going.queue(false);
    // The reader's own book has its words.
    assert_eq!(
        core.refresh(&going, Trouble::None),
        Next::CloseNotify("store.fetch.ready.reader")
    );
    assert_eq!(core.phase, Phase::Done);
    assert_eq!(ready_key("calibre"), "store.fetch.ready");
}

#[test]
fn an_end_while_covered_is_said_at_once_and_closed_on_return() {
    let going = Going::default();
    going.queue(true);
    let mut core = request("other");
    core.hide();
    going.set("pack-a", Step::Installed);
    going.set("pack-b", Step::Installed);
    going.queue(false);
    assert_eq!(
        core.refresh(&going, Trouble::None),
        Next::Notify("store.fetch.ready")
    );
    // Nothing more while covered; closing when it is on top again.
    assert_eq!(core.refresh(&going, Trouble::None), Next::None);
    assert_eq!(core.resume(&going, Trouble::None), Next::Close);
    // Update all and the list close without a notice.
    let mut all = Core::new(Kind::All, "store", strings(&["a"]), &[]).unwrap();
    all.hide();
    going.set("a", Step::Installed);
    assert_eq!(all.refresh(&going, Trouble::None), Next::None);
    assert_eq!(all.resume(&going, Trouble::None), Next::Close);
}

#[test]
fn what_did_not_come_is_named_and_can_be_tried_again() {
    let going = Going::default();
    going.queue(true);
    let mut core = request("reader");
    going.set("pack-a", Step::Installed);
    going.set("pack-b", Step::Failed(13));
    going.queue(false);
    assert_eq!(core.refresh(&going, Trouble::None), Next::Redraw);
    assert_eq!(core.phase, Phase::Failed(Why::Missing));
    // Moving between the rows, round the ends; Select chooses.
    assert_eq!(
        core.action("core.down").unwrap(),
        (Handled::Yes, Next::Redraw)
    );
    assert_eq!(core.focus, 1);
    core.action("core.down").unwrap();
    assert_eq!(core.focus, 0);
    core.action("core.up").unwrap();
    assert_eq!(core.focus, 1);
    assert_eq!(core.action("core.back").unwrap(), (Handled::No, Next::None));
    core.focus = 0;
    let (handled, next) = core.action("core.confirm").unwrap();
    assert_eq!(handled, Handled::Yes);
    assert_eq!(
        next,
        Next::Again {
            by: "reader".into(),
            ids: strings(&["pack-a", "pack-b"])
        }
    );
    // Trying again starts the count anew.
    assert_eq!(
        (core.phase, core.permille, core.done),
        (Phase::Working, 0, 0)
    );
    // The other row closes and does not start anything.
    let mut closed = request("reader");
    going.queue(false);
    closed.refresh(&going, Trouble::None);
    assert_eq!(closed.choose(1).unwrap(), Next::Close);
    assert_eq!(closed.choose(7).unwrap(), Next::None);
}

#[test]
fn a_pack_that_works_after_a_restart_asks_for_one() {
    let going = Going::default();
    going.set("pack-a", Step::Installed);
    going.set("pack-b", Step::Installed);
    going.restart.borrow_mut().push("pack-b".into());
    let mut core = request("reader");
    assert_eq!(core.refresh(&going, Trouble::None), Next::Redraw);
    assert_eq!((core.phase, core.permille), (Phase::Restart, 1000));
    let (_, next) = core.action("core.confirm").unwrap();
    assert_eq!(next, Next::Restart);
}

#[test]
fn a_fetch_of_the_list_without_a_bar_closes_when_the_list_is_here() {
    let going = Going::default();
    let mut core = Core::new(Kind::List, "store", Vec::new(), &[]).unwrap();
    assert!(!core.bar());
    assert_eq!(core.refresh(&going, Trouble::Loading), Next::None);
    assert_eq!(core.refresh(&going, Trouble::Offline), Next::Redraw);
    assert_eq!(core.phase, Phase::Failed(Why::Offline));
    assert_eq!(core.choose(0).unwrap(), Next::FetchList);
    assert_eq!(core.refresh(&going, Trouble::None), Next::Close);
}

/// The catalog as a table, to see the words the screen picks.
struct Table;

impl Say for Table {
    fn lookup(&mut self, key: &str) -> Result<Option<String>, Error> {
        Ok((key != "core.decimal").then(|| format!("<{key}>")))
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let args: Vec<String> = args
            .iter()
            .map(|a| match a {
                Arg::Text { name, value } => format!("{name}={value}"),
                _ => String::new(),
            })
            .collect();
        Ok(format!("<{key} {}>", args.join(" ")))
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        Ok(text.to_string())
    }
}

#[test]
fn the_words_follow_the_phase() {
    let going = Going::default();
    let mut core = request("reader");
    let mut say = Table;
    assert_eq!(core.message(&mut say).unwrap(), "Letters A, Letters B");
    going.set(
        "pack-a",
        Step::Downloading {
            done: 500,
            total: 1000,
        },
    );
    going.queue(true);
    core.refresh(&going, Trouble::None);
    let (done, total) = (size_text(500, ".").unwrap(), size_text(4000, ".").unwrap());
    assert_eq!(
        core.progress_text(&mut say).unwrap(),
        format!("<store.fetch.progress percent=12 % done={done} total={total}>")
    );
    going.queue(false);
    going.set("pack-a", Step::Failed(1));
    core.refresh(&going, Trouble::None);
    assert_eq!(
        core.message(&mut say).unwrap(),
        "<store.request.failed names=Letters A, Letters B>"
    );
    let list = Core::new(Kind::List, "store", Vec::new(), &[]).unwrap();
    assert_eq!(list.message(&mut say).unwrap(), "<store.loading>");
    // Without sizes only the percent is said.
    let bare = Core::new(Kind::Request, "x", strings(&["a"]), &[]).unwrap();
    assert_eq!(bare.progress_text(&mut say).unwrap(), "0 %");
    assert_eq!(bare.message(&mut say).unwrap(), "a");
}

#[test]
fn the_names_are_the_librarys_and_cut_after_a_few() {
    let ids: Vec<String> = (0..12).map(|n| format!("p{n}")).collect();
    let listed: Vec<_> = ids
        .iter()
        .map(|id| entry(id, &id.to_uppercase(), 1))
        .collect();
    let core = Core::new(Kind::All, "store", ids, &listed).unwrap();
    assert_eq!(core.names, "P0, P1, P2, P3, P4, P5, P6, P7, …");
    assert_eq!(core.total(), 12);
}

#[test]
fn the_screen_opens_from_its_parameters_and_checks_every_id() {
    assert_eq!(
        request_ids("a\nBad Name\n../x\nb\na\n\ncore").unwrap(),
        ["a", "b"]
    );
    let many: Vec<String> = (0..50).map(|n| format!("p{n}")).collect();
    assert_eq!(request_ids(&many.join("\n")).unwrap().len(), 32);
    let next = open_next("reader", &strings(&["a", "b"])).unwrap();
    assert_eq!(
        next,
        Next::Open {
            screen: "store.fetch",
            params: vec![(BY, "reader".to_string()), (IDS, "a\nb".to_string())]
        }
    );
    assert_eq!((BY, IDS), ("id", "text"));
}

#[test]
fn the_focus_goes_round_the_rows() {
    assert_eq!(step_focus(0, 2, 1), 1);
    assert_eq!(step_focus(1, 2, 1), 0);
    assert_eq!(step_focus(0, 2, -1), 1);
    assert_eq!(step_focus(1, 2, -1), 0);
    assert_eq!(step_focus(5, 2, 1), 0, "a focus past the rows is the last");
    assert_eq!(step_focus(0, 0, 1), 0);
}

fn metrics(message_needs: u16, hint_needs: u16) -> Metrics {
    Metrics {
        width: 480,
        height: 800,
        header: 60,
        margin: 16,
        gap: 8,
        row: 60,
        message_needs,
        hint_needs,
    }
}

#[test]
fn the_bar_has_room_to_spare_or_goes_close_below_the_message() {
    // Two rows for the message and the bar below with room.
    let l = Layout::of(&metrics(40, 40));
    assert_eq!((l.message.x, l.message.y, l.message.width), (16, 68, 448));
    assert_eq!(l.message.height, 120, "two rows at least");
    assert_eq!(l.hint.y + l.hint.height, 800);
    assert_eq!(l.hint.height, 90, "a row and a half at least");
    assert_eq!(l.bar.y, 210, "the header and two and a half rows");
    assert_eq!((l.bar.height, l.bar.width), (30, 448));
    assert_eq!(l.rows_top, l.bar.y + 120);
    // A message of many lines pushes the bar below it.
    let tall = Layout::of(&metrics(300, 40));
    assert_eq!(tall.bar.y, 68 + 300 + 8);
    // A hint of many lines leaves no room under: close below the message.
    let crowded = Layout::of(&metrics(40, 500));
    assert_eq!(crowded.bar.y, 68 + 40 + 8);
    // A small screen does not overflow.
    let small = Layout::of(&Metrics {
        width: 0,
        height: 0,
        ..metrics(u16::MAX, u16::MAX)
    });
    assert!(small.bar.y >= small.message.y);
}
