//! Every rule of `docs/abi/work.md` against a fake clock: the form of
//! `declare`, the limits, when work is due, the holds, the radio gap, runs and
//! their 10-minute end, and refusal under memory pressure.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native_host::shell::tests::FAIL_NTH;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::ERR_NOT_FOUND;

const A: u64 = 1;
const B: u64 = 2;

struct Fake {
    now: Cell<u64>,
    level: Cell<u32>,
    stretch: Cell<u32>,
    radio: Cell<bool>,
    screen: Cell<bool>,
    up: Cell<bool>,
    denied: RefCell<Vec<u64>>,
    manual: RefCell<Vec<u64>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            now: Cell::new(1_000),
            level: Cell::new(0),
            stretch: Cell::new(1),
            radio: Cell::new(true),
            screen: Cell::new(false),
            up: Cell::new(true),
            denied: RefCell::default(),
            manual: RefCell::default(),
        }
    }
    fn at(&self, now: u64) {
        self.now.set(now);
    }
}

impl WorkEnv for Fake {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn power(&self) -> Power {
        Power {
            level: self.level.get(),
            stretch: self.stretch.get(),
            radio_on: self.radio.get(),
        }
    }
    fn screen_hold(&self) -> bool {
        self.screen.get()
    }
    fn network_up(&self) -> bool {
        self.up.get()
    }
    fn network_allowed(&self, owner: u64) -> bool {
        !self.denied.borrow().contains(&owner)
    }
    fn saving_manual(&self, owner: u64) -> bool {
        self.manual.borrow().contains(&owner)
    }
    fn subscribed(&self, _: u64, _: u32) -> bool {
        true
    }
}

fn spec(key: &str) -> Spec<'_> {
    Spec {
        token: 7,
        needs: 0,
        flags: 0,
        moments: 0,
        after_ms: 0,
        within_ms: 0,
        every_ms: 0,
        at_ms: 0,
        key,
    }
}

fn sched() -> WorkSched {
    WorkSched::new(Limits::OTHER)
}

/// One pass: marks what is due and delivers everything for every plugin.
fn pass(s: &WorkSched, env: &Fake) -> Vec<Event> {
    s.begin_pass(env).unwrap();
    let mut events = Vec::new();
    while let Some(event) = s.next_event(env, &|_| true).unwrap() {
        events.push(event);
        if let Event::Due(due) = event {
            s.callback_returned(due.run, env.now_ms());
        }
    }
    events
}

fn dues(events: &[Event]) -> Vec<(u64, String, u32)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Due(d) => Some((d.owner, String::from(d.key.as_str()), d.why)),
            Event::Refused(_) => None,
        })
        .collect()
}

#[test]
fn the_form_of_declare_is_checked_at_its_limits() {
    let s = sched();
    let env = Fake::new();
    let ok = |s: &WorkSched, spec: Spec<'_>| s.declare(A, &spec, env.now_ms());
    // Keys: 1..=64 bytes, no control characters.
    assert_eq!(ok(&s, spec("a")), Ok(()));
    assert_eq!(ok(&s, spec(&"k".repeat(64))), Ok(()));
    for bad in ["", &"k".repeat(65), "a\nb", "a\u{7f}b", "a\0"] {
        assert_eq!(ok(&s, spec(bad)), Err(ERR_INVALID), "{bad:?}");
    }
    // Unknown bits are unsupported, before the rules.
    for (needs, flags, moments) in [(2, 0, 0), (0, 2, 0), (0, 0, 8), (0, 0, 0x8000_0000)] {
        let mut bad = spec("x");
        (bad.needs, bad.flags, bad.moments) = (needs, flags, moments);
        assert_eq!(
            ok(&s, bad),
            Err(ERR_UNSUPPORTED),
            "{needs} {flags} {moments}"
        );
    }
    // Spans: 7 days is the limit, one more is invalid.
    for field in 0..3 {
        for (value, want) in [(MAX_SPAN_MS, Ok(())), (MAX_SPAN_MS + 1, Err(ERR_INVALID))] {
            let mut sp = spec("span");
            match field {
                0 => sp.after_ms = value,
                1 => sp.within_ms = value,
                _ => sp.every_ms = value,
            }
            assert_eq!(ok(&s, sp), want, "field {field} {value}");
        }
    }
    // Repeat: 60 s at least, or none.
    for (every, want) in [(0, Ok(())), (59_999, Err(ERR_INVALID)), (60_000, Ok(()))] {
        let mut sp = spec("every");
        sp.every_ms = every;
        assert_eq!(ok(&s, sp), want, "{every}");
    }
    // An exact time excludes everything else.
    let mut exact = spec("exact");
    exact.at_ms = 5;
    assert_eq!(ok(&s, exact), Ok(()));
    for what in 0..4 {
        let mut sp = exact;
        match what {
            0 => sp.after_ms = 1,
            1 => sp.within_ms = 1,
            2 => sp.moments = 1,
            _ => sp.flags = 1,
        }
        assert_eq!(ok(&s, sp), Err(ERR_INVALID), "{what}");
    }
    // An exact time may repeat.
    exact.every_ms = 60_000;
    assert_eq!(ok(&s, exact), Ok(()));
}

#[test]
fn tables_are_bounded_per_plugin_and_per_device_and_a_replaced_key_counts_once() {
    for (limits, per_plugin, device) in [(Limits::X4, 8, 32), (Limits::OTHER, 32, 256)] {
        let s = WorkSched::new(limits);
        for n in 0..per_plugin {
            s.declare(A, &spec(&alloc::format!("k{n}")), 0).unwrap();
        }
        // Limit + 1 for the plugin; the same key again is a replace.
        assert_eq!(s.declare(A, &spec("one-more"), 0), Err(ERR_FULL));
        assert_eq!(s.declare(A, &spec("k0"), 0), Ok(()));
        assert_eq!(s.counts().0, per_plugin);
        // Other plugins fill the rest of the device, then it is full for all.
        let plugins = device / per_plugin;
        for owner in 2..=plugins as u64 {
            for n in 0..per_plugin {
                s.declare(owner, &spec(&alloc::format!("k{n}")), 0).unwrap();
            }
        }
        assert_eq!(s.counts().0, device);
        assert_eq!(s.declare(99, &spec("x"), 0), Err(ERR_FULL));
        // Replacing a key on a full device still works; cancelling makes room.
        assert_eq!(s.declare(2, &spec("k1"), 0), Ok(()));
        s.cancel(2, "k1").unwrap();
        assert_eq!(s.declare(99, &spec("x"), 0), Ok(()));
    }
}

#[test]
fn a_refusing_heap_leaves_the_table_as_it_was() {
    let s = sched();
    // The first entry: the key's block fails.
    FAIL_NTH.with(|n| n.set(1));
    let first = s.declare(A, &spec("a"), 0);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(first, Err(ERR_NO_MEMORY));
    assert_eq!(s.counts().0, 0);
    // A table that must grow: four entries fill the first buffer, the fifth
    // gets its key and then fails to grow the table.
    for n in 0..4 {
        s.declare(A, &spec(&alloc::format!("k{n}")), 0).unwrap();
    }
    FAIL_NTH.with(|n| n.set(2));
    let grown = s.declare(A, &spec("k4"), 0);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(grown, Err(ERR_NO_MEMORY));
    assert_eq!(s.counts().0, 4);
    // A replace whose key block fails keeps the old entry intact.
    let mut changed = spec("k0");
    changed.token = 99;
    FAIL_NTH.with(|n| n.set(1));
    let replaced = s.declare(A, &changed, 0);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(replaced, Err(ERR_NO_MEMORY));
    let mut tokens = Vec::new();
    s.rows(
        A,
        Power {
            level: 0,
            stretch: 1,
            radio_on: true,
        },
        &mut |r| {
            tokens.push(r.token);
            true
        },
    )
    .unwrap();
    assert_eq!(tokens, [7, 7, 7, 7]);
    // And it works again afterwards.
    assert_eq!(s.declare(A, &spec("k4"), 0), Ok(()));
}

#[test]
fn a_work_is_due_exactly_when_its_time_has_passed() {
    let s = sched();
    let env = Fake::new();
    // Nothing set: at once. Only after_ms: when it has passed.
    s.declare(A, &spec("now"), 1_000).unwrap();
    let mut later = spec("later");
    later.after_ms = 5_000;
    s.declare(A, &later, 1_000).unwrap();
    let got = dues(&pass(&s, &env));
    assert_eq!(got, [(A, "now".into(), why::TIME)]);
    env.at(5_999);
    assert!(pass(&s, &env).is_empty(), "one ms early");
    env.at(6_000);
    assert_eq!(dues(&pass(&s, &env)), [(A, "later".into(), why::TIME)]);
    // A once-work is gone after its run.
    assert_eq!(s.counts(), (0, 0));
}

#[test]
fn a_deadline_is_after_plus_within_stretched_in_saving() {
    let s = sched();
    let env = Fake::new();
    let mut sp = spec("dl");
    sp.moments = 1;
    sp.after_ms = 1_000;
    sp.within_ms = 10_000;
    s.declare(A, &sp, 1_000).unwrap(); // ready 2_000, deadline 12_000
    env.at(11_999);
    assert!(pass(&s, &env).is_empty());
    env.at(12_000);
    assert_eq!(dues(&pass(&s, &env)), [(A, "dl".into(), why::DEADLINE)]);
    // Stretched three times in saving: 2_000 + 30_000.
    s.declare(A, &sp, 12_000).unwrap(); // ready 13_000
    env.level.set(1);
    env.stretch.set(3);
    env.at(42_999);
    assert!(pass(&s, &env).is_empty());
    env.at(43_000);
    assert_eq!(dues(&pass(&s, &env)), [(A, "dl".into(), why::DEADLINE)]);
    // Stretch 0 and 1 mean not stretched.
    env.stretch.set(0);
    s.declare(A, &sp, 50_000).unwrap(); // deadline 61_000
    env.at(60_999);
    assert!(pass(&s, &env).is_empty());
    env.at(61_000);
    assert_eq!(pass(&s, &env).len(), 1);
}

#[test]
fn a_moment_counts_only_after_after_ms_and_a_moment_only_work_has_no_deadline() {
    let s = sched();
    let env = Fake::new();
    let mut sp = spec("m");
    sp.moments = 2 | 4; // online, charging
    sp.after_ms = 5_000;
    s.declare(A, &sp, 1_000).unwrap(); // ready 6_000
    env.at(5_999);
    s.moment(Moment::Online, &env);
    env.at(100_000);
    assert!(
        pass(&s, &env).is_empty(),
        "the early moment was ignored, no deadline"
    );
    s.moment(Moment::Wake, &env);
    assert!(pass(&s, &env).is_empty(), "a moment it did not ask for");
    s.moment(Moment::Charging, &env);
    assert_eq!(dues(&pass(&s, &env)), [(A, "m".into(), why::CHARGING)]);
    // Each moment gives its own reason.
    for (moment, want) in [(Moment::Wake, why::WAKE), (Moment::Online, why::ONLINE)] {
        let mut sp = spec("m2");
        sp.moments = 7;
        s.declare(A, &sp, env.now_ms()).unwrap();
        env.at(env.now_ms() + 1);
        s.moment(moment, &env);
        assert_eq!(dues(&pass(&s, &env)), [(A, "m2".into(), want)]);
    }
}

#[test]
fn an_exact_timer_ignores_screens_and_power_and_the_past_is_due_at_once() {
    let s = sched();
    let env = Fake::new();
    let mut sp = spec("t");
    sp.at_ms = 9_000;
    s.declare(A, &sp, 1_000).unwrap();
    env.screen.set(true);
    env.level.set(1);
    env.stretch.set(5);
    env.at(8_999);
    assert!(pass(&s, &env).is_empty());
    env.at(9_000);
    assert_eq!(dues(&pass(&s, &env)), [(A, "t".into(), why::TIME)]);
    // In the past: at once.
    sp.at_ms = 10;
    s.declare(A, &sp, 9_000).unwrap();
    assert_eq!(dues(&pass(&s, &env)), [(A, "t".into(), why::TIME)]);
    // Other work waits behind a real-time screen and runs when it ends.
    s.declare(A, &spec("plain"), 9_000).unwrap();
    assert!(pass(&s, &env).is_empty());
    env.screen.set(false);
    assert_eq!(dues(&pass(&s, &env)), [(A, "plain".into(), why::TIME)]);
}

#[test]
fn a_repeating_work_is_due_again_after_its_run_and_never_before() {
    let s = sched();
    let env = Fake::new();
    let mut sp = spec("every");
    sp.every_ms = 60_000;
    s.declare(A, &sp, 1_000).unwrap();
    let events = pass(&s, &env);
    assert_eq!(events.len(), 1);
    // Re-armed from the end of the run.
    env.at(60_999);
    assert!(pass(&s, &env).is_empty());
    env.at(61_000);
    assert_eq!(pass(&s, &env).len(), 1);
    // While its run is open it is not due again.
    let s = sched();
    let env = Fake::new();
    s.declare(A, &sp, 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("due")
    };
    s.resource_opened(due.run).unwrap();
    s.callback_returned(due.run, env.now_ms());
    env.at(500_000);
    assert!(pass(&s, &env).is_empty(), "the run is open");
    s.resource_closed(due.run, env.now_ms());
    env.at(559_999);
    assert!(pass(&s, &env).is_empty());
    env.at(560_000);
    assert_eq!(pass(&s, &env).len(), 1);
    // An exact repeating timer is re-armed the same way.
    let s = sched();
    let env = Fake::new();
    let mut timer = spec("tick");
    timer.at_ms = 2_000;
    timer.every_ms = 60_000;
    s.declare(A, &timer, 1_000).unwrap();
    env.at(2_000);
    assert_eq!(pass(&s, &env).len(), 1);
    env.at(61_999);
    assert!(pass(&s, &env).is_empty());
    env.at(62_000);
    assert_eq!(pass(&s, &env).len(), 1);
}

fn net(key: &str) -> Spec<'_> {
    let mut sp = spec(key);
    sp.needs = 1;
    sp
}

#[test]
fn offline_and_a_manual_saving_setting_hold_network_work_and_leaving_them_releases_it() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &net("n"), 1_000).unwrap();
    s.declare(A, &spec("local"), 1_000).unwrap();
    env.level.set(2);
    // Work without the network need is not held.
    assert_eq!(dues(&pass(&s, &env)), [(A, "local".into(), why::TIME)]);
    let row = |level| {
        let mut rows = Vec::new();
        s.rows(
            A,
            Power {
                level,
                stretch: 1,
                radio_on: true,
            },
            &mut |r| {
                rows.push((r.state, r.reason));
                true
            },
        )
        .unwrap();
        rows
    };
    assert_eq!(row(2), [(1, REASON_OFFLINE)]);
    // Leaving the mode releases it, with a hint for the plugins.
    env.level.set(0);
    let marked = s.begin_pass(&env).unwrap();
    assert!(marked.allowed);
    assert_eq!(row(0), [(0, 0)]);
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("released work is due")
    };
    s.callback_returned(due.run, env.now_ms());
    // Saving held to manual: reason 12, for that plugin only.
    s.declare(A, &net("m"), 1_000).unwrap();
    s.declare(B, &net("m"), 1_000).unwrap();
    env.level.set(1);
    env.manual.borrow_mut().push(A);
    let events = pass(&s, &env);
    assert_eq!(dues(&events), [(B, "m".into(), why::TIME)]);
    assert_eq!(row(1), [(1, REASON_SAVING)]);
}

#[test]
fn the_radio_is_asked_for_at_most_once_per_fifteen_minutes_on_the_device() {
    let s = sched();
    let env = Fake::new();
    env.up.set(false);
    let mut sp = net("deadline");
    sp.moments = 2;
    sp.within_ms = 1_000;
    s.declare(A, &sp, 1_000).unwrap(); // deadline 2_000
    s.declare(B, &sp, 1_000).unwrap();
    env.at(2_000);
    assert!(s.begin_pass(&env).unwrap().network_wanted);
    assert!(
        s.next_event(&env, &|_| true).unwrap().is_none(),
        "no network yet"
    );
    // Inside the gap: no second wish, from any plugin's work.
    env.at(2_000 + NETWORK_GAP_MS - 1);
    assert!(!s.begin_pass(&env).unwrap().network_wanted);
    env.at(2_000 + NETWORK_GAP_MS);
    assert!(s.begin_pass(&env).unwrap().network_wanted);
    // The network comes up: every plugin's work rides along.
    env.up.set(true);
    s.moment(Moment::Online, &env);
    let events = pass(&s, &env);
    assert_eq!(dues(&events).len(), 2);
    // Optional work and moment-only work never ask.
    let s = sched();
    env.up.set(false);
    let mut optional = net("opt");
    optional.flags = 1;
    optional.within_ms = 1_000;
    optional.moments = 2;
    let mut moment_only = net("mo");
    moment_only.moments = 2;
    s.declare(A, &optional, 1_000).unwrap();
    s.declare(A, &moment_only, 1_000).unwrap();
    env.at(1_000_000);
    assert!(!s.begin_pass(&env).unwrap().network_wanted);
}

#[test]
fn network_work_whose_time_has_passed_is_joined_to_one_connection() {
    let s = sched();
    let env = Fake::new();
    let mut a = net("due");
    a.moments = 2;
    a.within_ms = 1_000;
    let mut b = net("waiting");
    b.moments = 2; // after_ms passed, no deadline, waits for a moment
    let mut c = net("early");
    c.after_ms = 100_000;
    c.moments = 2;
    s.declare(A, &a, 1_000).unwrap();
    s.declare(B, &b, 1_000).unwrap();
    s.declare(B, &c, 1_000).unwrap();
    env.at(2_000);
    let got = dues(&pass(&s, &env));
    // `due` by its deadline, `waiting` rides along; `early` is not yet ready.
    assert_eq!(got.len(), 2);
    assert!(got.contains(&(A, "due".into(), why::DEADLINE)));
    assert!(got.contains(&(B, "waiting".into(), why::ONLINE)));
}

#[test]
fn optional_network_work_in_saving_runs_only_with_the_radio_on() {
    let s = sched();
    let env = Fake::new();
    let mut sp = net("weather");
    sp.flags = 1;
    sp.moments = 1 | 2;
    sp.within_ms = 1_000;
    s.declare(A, &sp, 1_000).unwrap();
    env.level.set(1);
    env.radio.set(false);
    // No deadline in saving, and a moment with the radio off does not count.
    env.at(1_000_000);
    s.moment(Moment::Wake, &env);
    assert!(pass(&s, &env).is_empty());
    env.radio.set(true);
    s.moment(Moment::Online, &env);
    assert_eq!(dues(&pass(&s, &env)), [(A, "weather".into(), why::ONLINE)]);
}

#[test]
fn due_work_goes_in_due_order_and_a_busy_plugin_does_not_hold_up_others() {
    let s = sched();
    let env = Fake::new();
    let mut late = spec("b-late");
    late.at_ms = 3_000;
    let mut early = spec("a-early");
    early.at_ms = 2_000;
    s.declare(A, &late, 1_000).unwrap();
    s.declare(B, &early, 1_000).unwrap();
    env.at(5_000);
    s.begin_pass(&env).unwrap();
    // B first (2_000 before 3_000); with B busy A goes first.
    let Some(Event::Due(first)) = s.next_event(&env, &|o| o == A).unwrap() else {
        panic!("a")
    };
    assert_eq!((first.owner, first.key.as_str()), (A, "b-late"));
    let Some(Event::Due(second)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("b")
    };
    assert_eq!((second.owner, second.key.as_str()), (B, "a-early"));
    assert!(s.next_event(&env, &|_| true).unwrap().is_none());
}

#[test]
fn work_declared_while_delivering_is_due_at_the_next_pass_and_cannot_loop() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("again"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let mut delivered = 0;
    while let Some(event) = s.next_event(&env, &|_| true).unwrap() {
        let Event::Due(due) = event else { continue };
        delivered += 1;
        // The callback declares itself again with nothing set.
        s.declare(A, &spec("again"), env.now_ms()).unwrap();
        s.callback_returned(due.run, env.now_ms());
        assert!(delivered < 5, "a loop inside one pass");
    }
    assert_eq!(delivered, 1);
    // The next pass has it.
    assert_eq!(pass(&s, &env).len(), 1);
}

#[test]
fn work_declared_from_a_runs_callbacks_waits_at_least_one_second() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("outer"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("due")
    };
    {
        let _guard = s.enter_run_callback(A, due.run);
        assert!(s.quiet(A));
        assert!(!s.quiet(B));
        // Its own key and another, with nothing set and with a past time.
        s.declare(A, &spec("outer"), env.now_ms()).unwrap();
        s.declare(A, &spec("other"), env.now_ms()).unwrap();
        let mut past = spec("past");
        past.at_ms = 1;
        s.declare(A, &past, env.now_ms()).unwrap();
        // Another plugin's declaration is not from this run.
        s.declare(B, &spec("b"), env.now_ms()).unwrap();
    }
    assert!(!s.quiet(A));
    s.callback_returned(due.run, env.now_ms());
    env.at(1_999);
    let got: Vec<_> = dues(&pass(&s, &env)).into_iter().map(|d| d.1).collect();
    assert_eq!(got, ["b"], "only the other plugin's, 999 ms after");
    env.at(2_000);
    let mut got: Vec<_> = dues(&pass(&s, &env)).into_iter().map(|d| d.1).collect();
    got.sort();
    assert_eq!(got, ["other", "outer", "past"]);
}

#[test]
fn a_run_ends_with_its_callback_and_its_last_resource_and_never_outlives_ten_minutes() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("w"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("due")
    };
    s.resource_opened(due.run).unwrap();
    s.resource_opened(due.run).unwrap();
    s.callback_returned(due.run, 1_000);
    let state = |s: &WorkSched| {
        let mut rows = Vec::new();
        s.rows(
            A,
            Power {
                level: 0,
                stretch: 1,
                radio_on: true,
            },
            &mut |r| {
                rows.push(r.state);
                true
            },
        )
        .unwrap();
        rows
    };
    assert_eq!(state(&s), [2], "running while a resource is open");
    s.resource_closed(due.run, 2_000);
    assert_eq!(state(&s), [2]);
    // 10 minutes from work_due: the limit, one ms before it, and the end.
    assert!(s.take_expired(1_000 + RUN_MAX_MS - 1).is_none());
    let expired = s.take_expired(1_000 + RUN_MAX_MS).unwrap();
    assert_eq!((expired.id, expired.owner), (due.run, A));
    assert!(s.take_expired(1_000 + RUN_MAX_MS).is_none(), "once");
    s.end_run(due.run, 1_000 + RUN_MAX_MS);
    assert!(state(&s).is_empty(), "the once-work is gone");
    assert_eq!(s.counts(), (0, 0));
    // Not stretched in saving: the limit is the same.
    assert_eq!(RUN_MAX_MS, 600_000);
    assert_eq!((REQUEST_WORK_ENDED, SOCKET_WORK_ENDED), (14, 27));
}

#[test]
fn declaring_again_while_running_waits_for_the_end_and_cancel_lets_the_run_finish() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("w"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("due")
    };
    s.callback_returned(due.run, 1_000); // no resources: the run ends at once
    env.at(2_000);
    s.declare(A, &spec("w"), 2_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("due again")
    };
    s.resource_opened(due.run).unwrap();
    s.callback_returned(due.run, 2_000);
    s.declare(A, &spec("w"), 3_000).unwrap();
    assert_eq!(s.counts(), (1, 1), "one entry, one run");
    s.resource_closed(due.run, 4_000);
    env.at(4_000);
    assert_eq!(dues(&pass(&s, &env)).len(), 1, "due again after the run");
    // Cancel while running: the entry goes, the run finishes alone.
    s.declare(A, &spec("c"), 5_000).unwrap();
    env.at(5_000);
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(c)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("c")
    };
    s.resource_opened(c.run).unwrap();
    s.callback_returned(c.run, 5_000);
    s.cancel(A, "c").unwrap();
    assert_eq!(s.counts().0, 0);
    s.resource_closed(c.run, 6_000);
    assert_eq!(s.counts(), (0, 0));
    assert_eq!(s.resource_opened(c.run), Err(ERR_NOT_FOUND));
}

#[test]
fn cancel_removes_by_key_or_all_a_missing_key_is_fine_and_pending_work_is_not_called() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("a"), 1_000).unwrap();
    s.declare(A, &spec("b"), 1_000).unwrap();
    s.declare(B, &spec("a"), 1_000).unwrap();
    assert_eq!(s.cancel(A, "missing"), Ok(()));
    assert_eq!(s.counts().0, 3);
    s.begin_pass(&env).unwrap(); // all three are pending now
    s.cancel(A, "a").unwrap();
    let mut left = Vec::new();
    while let Some(Event::Due(d)) = s.next_event(&env, &|_| true).unwrap() {
        left.push((d.owner, String::from(d.key.as_str())));
    }
    left.sort();
    assert_eq!(left, [(A, "b".into()), (B, "a".into())]);
    s.declare(A, &spec("x"), 1_000).unwrap();
    s.declare(A, &spec("y"), 1_000).unwrap();
    s.cancel(A, "").unwrap();
    s.begin_pass(&env).unwrap();
    // Only B's (finished, removed) and none of A's remain.
    assert!(s.next_event(&env, &|_| true).unwrap().is_none());
}

#[test]
fn turning_the_network_off_takes_waiting_network_work_out_for_good() {
    let s = sched();
    let env = Fake::new();
    let mut sp = net("n");
    sp.moments = 2;
    s.declare(A, &sp, 1_000).unwrap();
    s.declare(A, &spec("local"), 5_000_000).unwrap();
    env.denied.borrow_mut().push(A);
    s.begin_pass(&env).unwrap();
    let Some(Event::Refused(r)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("refused")
    };
    assert_eq!(
        (r.owner, r.token, r.reason, r.key.as_str()),
        (A, 7, REFUSED_NETWORK_OFF, "n")
    );
    assert_eq!(s.counts().0, 1, "only the local work is left");
    assert!(s.next_event(&env, &|_| true).unwrap().is_none(), "once");
}

#[test]
fn a_stopped_plugin_loses_its_work_and_runs_and_no_event_follows() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("a"), 1_000).unwrap();
    s.declare(B, &spec("b"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(a)) = s.next_event(&env, &|o| o == A).unwrap() else {
        panic!("a")
    };
    s.resource_opened(a.run).unwrap();
    // Declared work and a run keep a plugin from sleeping again (#248, D3).
    assert_eq!(s.owner_holds(A), Ok(true));
    s.owner_stopped(A).unwrap();
    assert_eq!(s.owner_holds(A), Ok(false));
    assert_eq!(s.owner_holds(B), Ok(true));
    assert_eq!(s.counts(), (1, 0));
    assert!(s.next_event(&env, &|o| o == A).unwrap().is_none());
    assert!(s.next_event(&env, &|o| o == B).unwrap().is_some());
}

#[test]
fn state_lists_the_plugins_own_work_in_declaration_order_with_its_times() {
    let s = sched();
    let env = Fake::new();
    let mut timed = spec("timed");
    timed.at_ms = 9_000;
    let mut moments = spec("moments");
    moments.moments = 2;
    let mut deadline = spec("deadline");
    deadline.moments = 2;
    deadline.within_ms = 5_000;
    deadline.after_ms = 1_000;
    s.declare(A, &timed, 1_000).unwrap();
    s.declare(B, &spec("b"), 1_000).unwrap();
    s.declare(A, &moments, 1_000).unwrap();
    s.declare(A, &deadline, 1_000).unwrap();
    let mut rows = Vec::new();
    s.rows(A, env.power(), &mut |r| {
        rows.push((String::from(r.key.as_str()), r.state, r.due_ms));
        true
    })
    .unwrap();
    assert_eq!(
        rows,
        [
            ("timed".into(), 0, 9_000),
            ("moments".into(), 0, 0),
            ("deadline".into(), 0, 7_000),
        ]
    );
    // A replace keeps the place in the order.
    s.declare(A, &timed, 2_000).unwrap();
    let mut keys = Vec::new();
    s.rows(A, env.power(), &mut |r| {
        keys.push(String::from(r.key.as_str()));
        true
    })
    .unwrap();
    assert_eq!(keys, ["timed", "moments", "deadline"]);
    // The visitor can stop early.
    let mut seen = 0;
    s.rows(A, env.power(), &mut |_| {
        seen += 1;
        false
    })
    .unwrap();
    assert_eq!(seen, 1);
}

#[test]
fn the_nap_wakes_for_the_earliest_exact_time_deadline_or_run_end() {
    let s = sched();
    let env = Fake::new();
    assert_eq!(s.next_wake(&env), None);
    let mut moments_only = spec("m");
    moments_only.moments = 1;
    s.declare(A, &moments_only, 1_000).unwrap();
    assert_eq!(s.next_wake(&env), None, "a moment needs no clock");
    let mut late = spec("late");
    late.at_ms = 90_000;
    s.declare(A, &late, 1_000).unwrap();
    let mut soon = spec("soon");
    soon.moments = 1;
    soon.within_ms = 4_000;
    s.declare(A, &soon, 1_000).unwrap();
    assert_eq!(s.next_wake(&env), Some(5_000));
    // A run that is open wakes the loop for its 10-minute end.
    s.cancel(A, "").unwrap();
    s.declare(A, &spec("now"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("due")
    };
    s.resource_opened(due.run).unwrap();
    assert_eq!(s.next_wake(&env), Some(1_000 + RUN_MAX_MS));
}

#[test]
fn a_busy_table_is_busy_not_a_panic_and_the_run_table_is_bounded() {
    let s = WorkSched::new(Limits::X4);
    let env = Fake::new();
    for n in 0..8 {
        s.declare(A, &spec(&alloc::format!("k{n}")), 1_000).unwrap();
    }
    for n in 0..4 {
        s.declare(B, &spec(&alloc::format!("k{n}")), 1_000).unwrap();
    }
    s.begin_pass(&env).unwrap();
    let mut open = 0;
    while let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() {
        s.resource_opened(due.run).unwrap();
        open += 1;
    }
    assert_eq!(open, 4, "two runs for each of the two plugins");
    // A runs table at the device cap delivers nothing more.
    let s = WorkSched::new(Limits {
        per_plugin: 2,
        device: 2,
        runs_per_plugin: 2,
    });
    s.declare(A, &spec("a"), 1_000).unwrap();
    s.declare(A, &spec("b"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    assert!(s.next_event(&env, &|_| true).unwrap().is_some());
    assert!(s.next_event(&env, &|_| true).unwrap().is_some());
    assert_eq!(s.declare(B, &spec("c"), 1_000), Err(ERR_FULL));
}

#[test]
fn a_run_cannot_start_when_the_heap_refuses_and_the_work_stays_due() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("a"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    FAIL_NTH.with(|n| n.set(1));
    let refused = s.next_event(&env, &|_| true);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(refused.err(), Some(ERR_NO_MEMORY));
    assert_eq!(s.counts(), (1, 0));
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("released work is due")
    };
    s.callback_returned(due.run, env.now_ms());
}

#[test]
fn a_network_refusal_is_latched_and_survives_the_setting_coming_back() {
    let s = sched();
    let env = Fake::new();
    let mut sp = net("n");
    sp.moments = 2;
    s.declare(A, &sp, 1_000).unwrap();
    // The user turns the network off; a pass sees it, but the plugin is busy
    // (it cannot take the event yet).
    env.denied.borrow_mut().push(A);
    s.begin_pass(&env).unwrap();
    assert!(s.next_event(&env, &|_| false).unwrap().is_none());
    // The setting is on again before the plugin can accept: still refused.
    env.denied.borrow_mut().clear();
    s.begin_pass(&env).unwrap();
    let Some(Event::Refused(r)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("refused")
    };
    assert_eq!(
        (r.owner, r.reason, r.key.as_str()),
        (A, REFUSED_NETWORK_OFF, "n")
    );
    assert_eq!(s.counts(), (0, 0));
}

#[test]
fn a_refusal_between_pass_and_event_is_latched_by_the_event_call_itself() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &net("n"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    env.denied.borrow_mut().push(A);
    assert!(s.next_event(&env, &|_| false).unwrap().is_none());
    env.denied.borrow_mut().clear();
    assert!(matches!(
        s.next_event(&env, &|_| true).unwrap(),
        Some(Event::Refused(_))
    ));
}

#[test]
fn pending_work_is_checked_again_when_the_device_state_changes() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &net("n"), 1_000).unwrap();
    s.declare(B, &spec("local"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    // Offline, saving held to manual, no network up: the network work waits
    // again instead of being delivered from the old pass.
    for change in 0..3 {
        match change {
            0 => env.level.set(2),
            1 => {
                env.level.set(1);
                env.manual.borrow_mut().push(A);
            }
            _ => {
                env.level.set(0);
                env.manual.borrow_mut().clear();
                env.up.set(false);
            }
        }
        let got = s.next_event(&env, &|o| o == A).unwrap();
        assert!(got.is_none(), "change {change}");
        env.up.set(true);
        env.level.set(0);
        env.manual.borrow_mut().clear();
        s.begin_pass(&env).unwrap();
    }
    // The device state is fine again: it runs.
    assert!(matches!(
        s.next_event(&env, &|o| o == A).unwrap(),
        Some(Event::Due(_))
    ));
    // Local work is not held by the network.
    env.level.set(2);
    assert!(s.next_event(&env, &|o| o == B).unwrap().is_some());
}

#[test]
fn the_screen_hold_also_takes_pending_work_back() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("a"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    env.screen.set(true);
    assert!(s.next_event(&env, &|_| true).unwrap().is_none());
    env.screen.set(false);
    s.begin_pass(&env).unwrap();
    assert!(s.next_event(&env, &|_| true).unwrap().is_some());
}

#[test]
fn the_nap_ignores_held_and_elapsed_blocked_work() {
    let s = sched();
    let env = Fake::new();
    let mut held = net("held");
    held.at_ms = 1_500;
    s.declare(A, &held, 1_000).unwrap();
    let mut blocked = spec("blocked");
    blocked.moments = 2;
    blocked.within_ms = 200;
    s.declare(A, &blocked, 1_000).unwrap();
    let mut later = spec("later");
    later.at_ms = 9_000;
    s.declare(A, &later, 1_000).unwrap();
    env.level.set(2);
    env.screen.set(true);
    env.at(2_000);
    s.begin_pass(&env).unwrap();
    // The held network work and the deadline held back by the screen are in
    // the past: no wake for them, only for the work still in the future.
    assert_eq!(s.next_wake(&env), Some(9_000));
    env.at(10_000);
    s.begin_pass(&env).unwrap();
    assert_eq!(s.next_wake(&env), None, "nothing left that a clock ends");
}

#[test]
fn one_plugin_cannot_hold_all_the_run_slots() {
    let s = WorkSched::new(Limits::X4);
    let env = Fake::new();
    for n in 0..8 {
        s.declare(A, &spec(&alloc::format!("a{n}")), 1_000).unwrap();
    }
    s.declare(B, &spec("b"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let mut mine = 0;
    let mut b = false;
    while let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() {
        s.resource_opened(due.run).unwrap();
        mine += usize::from(due.owner == A);
        b |= due.owner == B;
    }
    assert_eq!(mine, Limits::X4.runs_per_plugin);
    assert!(b, "another plugin still gets a run");
    // A cancelled work's run goes on: declaring again does not get past the cap.
    s.cancel(A, "").unwrap();
    s.declare(A, &spec("again"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    assert!(s.next_event(&env, &|o| o == A).unwrap().is_none());
}

#[test]
fn a_busy_table_makes_retirement_busy_not_silent() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("a"), 1_000).unwrap();
    {
        let _held = s.state.borrow_mut();
        assert_eq!(
            s.owner_stopped(A),
            Err(paperpaw_native::host_glue::ERR_BUSY)
        );
    }
    assert_eq!(s.counts(), (1, 0), "nothing removed");
    assert_eq!(s.owner_stopped(A), Ok(()));
    assert_eq!(s.counts(), (0, 0));
    let _ = env;
}

/// Opens runs for `owner` until the scheduler delivers no more; returns their ids.
fn open_runs(s: &WorkSched, env: &Fake, owner: u64) -> Vec<u32> {
    let mut runs = Vec::new();
    while let Some(Event::Due(due)) = s.next_event(env, &|o| o == owner).unwrap() {
        s.resource_opened(due.run).unwrap();
        runs.push(due.run);
    }
    runs
}

#[test]
fn the_run_cap_is_8_on_other_devices_with_the_ninth_work_waiting_not_dropped() {
    let s = WorkSched::new(Limits::OTHER);
    let env = Fake::new();
    for n in 0..9 {
        s.declare(A, &spec(&alloc::format!("k{n}")), 1_000).unwrap();
    }
    s.declare(B, &spec("b"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let runs = open_runs(&s, &env, A);
    assert_eq!(runs.len(), Limits::OTHER.runs_per_plugin);
    assert_eq!(runs.len(), 8);
    // Another plugin is not held back by A's cap.
    assert!(open_runs(&s, &env, B).len() == 1);
    // The ninth work is still there, waiting, not an error and not dropped.
    assert_eq!(s.counts(), (9 + 1, 8 + 1));
    assert!(s.next_event(&env, &|o| o == A).unwrap().is_none());
}

#[test]
fn a_waiting_work_starts_in_due_order_when_a_run_ends_without_declaring_again() {
    let s = WorkSched::new(Limits::X4);
    let env = Fake::new();
    // Four works of A, declared so that they came due in the order
    // fourth, third, second, first; the cap is 2.
    for (n, key) in ["first", "second", "third", "fourth"]
        .into_iter()
        .enumerate()
    {
        s.declare(A, &spec(key), 1_000 - n as u64 * 10).unwrap();
    }
    env.at(2_000);
    s.begin_pass(&env).unwrap();
    let mut started: Vec<(u32, String)> = Vec::new();
    let start_all = |s: &WorkSched, started: &mut Vec<(u32, String)>| {
        while let Some(Event::Due(due)) = s.next_event(&env, &|o| o == A).unwrap() {
            s.resource_opened(due.run).unwrap();
            started.push((due.run, String::from(due.key.as_str())));
        }
    };
    start_all(&s, &mut started);
    let keys = |started: &[(u32, String)]| -> Vec<String> {
        started.iter().map(|(_, key)| key.clone()).collect()
    };
    assert_eq!(keys(&started), ["fourth", "third"], "two at once on the X4");
    // A run ends: the next work in due order starts with nothing declared.
    s.end_run(started[0].0, env.now_ms());
    start_all(&s, &mut started);
    assert_eq!(keys(&started), ["fourth", "third", "second"]);
    // Another ends: the last one starts; none was lost or sent as an error.
    s.end_run(started[1].0, env.now_ms());
    start_all(&s, &mut started);
    assert_eq!(keys(&started), ["fourth", "third", "second", "first"]);
    // Everything ended: the table is empty and the plugin may start more.
    for (run, _) in &started[2..] {
        s.end_run(*run, env.now_ms());
    }
    assert_eq!(s.counts(), (0, 0));
}

#[test]
fn the_current_run_is_the_open_callback_of_that_owner_only() {
    let s = sched();
    let env = Fake::new();
    s.declare(A, &spec("outer"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("due")
    };
    assert_eq!(s.current_run(A), None, "no callback on the stack");
    {
        let _guard = s.enter_run_callback(A, due.run);
        assert_eq!(s.current_run(A), Some(u64::from(due.run)));
        assert_eq!(s.current_run(B), None, "another plugin is not in the run");
        {
            let _inner = s.enter_run_callback(B, due.run + 1);
            assert_eq!(s.current_run(A), None);
            assert_eq!(s.current_run(B), Some(u64::from(due.run + 1)));
        }
        assert_eq!(
            s.current_run(A),
            Some(u64::from(due.run)),
            "the outer one is back"
        );
    }
    assert_eq!(s.current_run(A), None, "the guard ended it");
}

#[test]
fn a_close_the_scheduler_could_not_take_waits_for_the_next_pass_and_an_overflow_is_counted() {
    use crate::native_host::owner_directory::OwnerDirectory;
    let s = crate::native_host::shared::Shared::try_new(sched()).unwrap();
    let dir = OwnerDirectory::new();
    dir.attach_sched(s.try_clone().unwrap());
    let env = Fake::new();
    s.declare(A, &spec("w"), 1_000).unwrap();
    s.begin_pass(&env).unwrap();
    let Some(Event::Due(due)) = s.next_event(&env, &|_| true).unwrap() else {
        panic!("due")
    };
    let run = u64::from(due.run);
    dir.resource_opened(run).unwrap();
    s.callback_returned(due.run, 1_000);
    assert_eq!(s.state.borrow().runs.len(), 1, "kept going by the resource");
    {
        // The scheduler is in use at that moment.
        let _busy = s.state.borrow_mut();
        dir.resource_closed(run, 2_000);
    }
    assert!(dir.closes_wait(), "the close was not lost");
    assert_eq!(s.state.borrow().runs.len(), 1);
    dir.retry_closes(3_000);
    assert!(!dir.closes_wait());
    assert!(
        s.state.borrow().runs.is_empty(),
        "the run ends at the retry"
    );
    // The retry that finds the scheduler in use still waits.
    {
        let _busy = s.state.borrow_mut();
        dir.resource_closed(run, 4_000);
        dir.retry_closes(5_000);
    }
    assert!(dir.closes_wait());
    dir.retry_closes(6_000);
    assert!(!dir.closes_wait());
    // More closes than there is room for are counted, not allocated.
    let _busy = s.state.borrow_mut();
    for _ in 0..10 {
        dir.resource_closed(run, 7_000);
    }
    assert_eq!(dir.take_missed(), 2);
}
