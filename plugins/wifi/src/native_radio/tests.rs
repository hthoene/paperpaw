//! The five scenarios of the interface-6 plugin's `tests/radio.rs`, run on a
//! small simulated host (the radio doing as told, timers run each second),
//! and the refusals of allocation.
use super::*;
use crate::fail_alloc::sweep;
use alloc::{string::ToString, vec};

const HOME: &str = "Home";

fn known() -> Vec<Known> {
    vec![Known {
        ssid: HOME.to_string(),
        password: Password::new("pw").unwrap(),
    }]
}

fn net(ssid: &str) -> Net {
    Net {
        ssid: ssid.to_string(),
        secured: true,
    }
}

/// What the radio is told.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Told {
    On,
    Scan,
    Join(String),
    Off,
}

struct Sim {
    policy: Policy,
    known: Vec<Known>,
    radio: Radio,
    now_ms: u64,
    wanted: bool,
    at_rest: bool,
    timers: Vec<Timer>,
    notices: Vec<Notice>,
    in_reach: Vec<Net>,
}

impl Sim {
    fn new() -> Self {
        Self {
            policy: Policy::default(),
            known: known(),
            radio: Radio::Off,
            now_ms: 0,
            wanted: false,
            at_rest: false,
            timers: Vec::new(),
            notices: Vec::new(),
            in_reach: Vec::new(),
        }
    }

    /// Calls a handler with the facts, the policy taken out meanwhile.
    fn call(
        &mut self,
        networks: &[Net],
        run: impl FnOnce(&mut Policy, &Facts<'_>) -> Result<Vec<Action>, Error>,
    ) -> Vec<Action> {
        self.call_with(networks, |_| {}, run)
    }

    /// As `call`, with the facts changed first.
    fn call_with(
        &mut self,
        networks: &[Net],
        change: impl FnOnce(&mut Facts<'_>),
        run: impl FnOnce(&mut Policy, &Facts<'_>) -> Result<Vec<Action>, Error>,
    ) -> Vec<Action> {
        let mut policy = core::mem::take(&mut self.policy);
        let mut facts = self.facts(networks);
        change(&mut facts);
        let actions = run(&mut policy, &facts).unwrap();
        self.policy = policy;
        actions
    }

    fn facts<'a>(&'a self, networks: &'a [Net]) -> Facts<'a> {
        Facts {
            radio: self.radio,
            busy: false,
            wanted: self.wanted,
            networks,
            known: &self.known,
            now_ms: self.now_ms,
            charging_keeps: false,
            at_rest: self.at_rest,
            on_demand: true,
            idle_ms: 60_000,
            join_first: None,
            joined: None,
        }
    }

    /// Does what `actions` ask, answering the radio's commands as the radio
    /// does (a scan finds `in_reach`; joining times out). Returns what the
    /// radio was told.
    fn run(&mut self, actions: Vec<Action>) -> Vec<Told> {
        self.run_as(actions, true)
    }

    /// As `run`; without `answer` the radio's commands are only recorded.
    fn run_as(&mut self, actions: Vec<Action>, answer: bool) -> Vec<Told> {
        let mut told = Vec::new();
        let mut queue: Vec<Action> = actions;
        queue.reverse();
        while let Some(action) = queue.pop() {
            let more = match action {
                Action::Step(Step::On) => {
                    told.push(Told::On);
                    if !answer {
                        continue;
                    }
                    self.radio = Radio::On;
                    let nets = Vec::new();
                    self.call(&nets, |p, f| p.on_changed(f))
                }
                Action::Step(Step::Scan) => {
                    told.push(Told::Scan);
                    if !answer {
                        continue;
                    }
                    let nets = core::mem::take(&mut self.in_reach);
                    let more = self.call(&nets, |p, f| p.on_scanned(f));
                    self.in_reach = nets;
                    more
                }
                Action::Step(Step::Join { ssid, .. }) => {
                    told.push(Told::Join(ssid.clone()));
                    if !answer {
                        continue;
                    }
                    let nets = core::mem::take(&mut self.in_reach);
                    let more = self.call(&nets, |p, f| p.on_failed(f, &ssid, Failure::Timeout));
                    self.in_reach = nets;
                    more
                }
                Action::Off => {
                    told.push(Told::Off);
                    if !answer {
                        continue;
                    }
                    self.radio = Radio::Off;
                    let nets = Vec::new();
                    self.call(&nets, |p, f| p.on_changed(f))
                }
                Action::Timer(timer) => {
                    self.timers.push(timer);
                    Vec::new()
                }
                Action::Notify(notice) => {
                    self.notices.push(notice);
                    Vec::new()
                }
                Action::NoNetwork => {
                    // The host fails what waited; nothing waits any more and
                    // the network is idle.
                    self.wanted = false;
                    let nets = Vec::new();
                    self.call(&nets, |p, f| p.on_idle(f))
                }
                Action::Picked(_) | Action::KeepSecret { .. } => Vec::new(),
            };
            for action in more.into_iter().rev() {
                queue.push(action);
            }
        }
        told
    }

    /// The radio's scan found `networks`.
    fn scanned(&mut self, networks: &[Net]) -> Vec<Told> {
        self.now_ms += 1000;
        let actions = self.call(networks, |p, f| p.on_scanned(f));
        self.run_as(actions, false)
    }

    /// `seconds` later, the timers run each second.
    fn wait(&mut self, seconds: u64) -> Vec<Told> {
        let mut told = Vec::new();
        for _ in 0..seconds {
            self.now_ms += 1000;
            let due: Vec<Timer> = self
                .timers
                .iter()
                .copied()
                .filter(|t| match t {
                    Timer::IdleOff { due_ms, .. } | Timer::Retry { due_ms, .. } => {
                        *due_ms <= self.now_ms
                    }
                })
                .collect();
            self.timers.retain(|t| !due.contains(t));
            for timer in due {
                let nets = core::mem::take(&mut self.in_reach);
                let actions = match timer {
                    Timer::IdleOff { generation, .. } => {
                        self.call(&nets, |p, f| p.on_idle_timer(f, generation))
                    }
                    Timer::Retry { generation, .. } => {
                        self.call(&nets, |p, f| p.on_retry_timer(f, generation))
                    }
                };
                self.in_reach = nets;
                told.extend(self.run(actions));
            }
        }
        told
    }

    /// Tells the policy a screen is shown.
    fn screen_shown(&mut self, at_rest: bool) -> Vec<Told> {
        self.now_ms += 1000;
        self.at_rest = at_rest;
        let nets = Vec::new();
        let actions = self.call(&nets, |p, f| p.on_screen_shown(f));
        self.run_as(actions, false)
    }
}

/// A plugin wants the network: the radio comes on and looks for networks
/// (what `want` of the interface-6 test checks).
fn wanted_and_on() -> Sim {
    let mut sim = Sim::new();
    sim.in_reach = Vec::new();
    // The scan the radio's switch-on starts is answered by the test.
    let nets = Vec::new();
    sim.now_ms += 1000;
    sim.wanted = true;
    let actions = sim.call(&nets, |p, f| p.on_wanted(f));
    assert_eq!(actions, [Action::Step(Step::On)]);
    sim.radio = Radio::On;
    let actions = sim.call(&nets, |p, f| p.on_changed(f));
    assert_eq!(actions, [Action::Step(Step::Scan)]);
    sim
}

#[test]
fn the_radio_goes_off_when_no_known_network_is_in_reach() {
    let mut sim = wanted_and_on();
    // None in reach: looked for again a few times, then the request fails.
    let mut scans = 1;
    while sim.scanned(&[]) == [Told::Scan] {
        scans += 1;
        assert!(scans < 10, "looks for ever");
    }
    assert_eq!(scans, 3);
    assert_eq!(sim.notices, [Notice::NoneInReach]);
    // Nothing waits any more: no more rounds, and the radio goes off.
    assert_eq!(sim.wait(5 * 60), [Told::Off]);
}

/// The first scan after the radio came on often finds nothing: it is made
/// again, and the network found then joined, without the request failing.
#[test]
fn a_scan_that_finds_nothing_is_made_again() {
    let mut sim = wanted_and_on();
    assert_eq!(sim.scanned(&[]), [Told::Scan]);
    sim.in_reach = vec![net(HOME)];
    let nets = [net(HOME)];
    let actions = sim.call(&nets, |p, f| p.on_scanned(f));
    assert!(
        matches!(&actions[..], [Action::Step(Step::Join { ssid, .. })] if ssid == HOME),
        "{actions:?}"
    );
    assert!(sim.notices.is_empty());
}

#[test]
fn the_radio_goes_off_once_joining_failed_for_as_long_as_work_waits() {
    let mut sim = wanted_and_on();
    sim.in_reach = vec![net(HOME)];
    let nets = [net(HOME)];
    let actions = sim.call(&nets, |p, f| p.on_scanned(f));
    // The join times out: tried again a while, as long as the request
    // waits; then no more, and the radio goes off.
    let mut told = sim.run(actions);
    told.extend(sim.wait(5 * 60));
    let rounds = told.iter().filter(|t| **t == Told::Scan).count();
    assert!((1..=3).contains(&rounds), "{told:?}");
    assert_eq!(told.last(), Some(&Told::Off), "{told:?}");
    assert_eq!(told.iter().filter(|t| **t == Told::Off).count(), 1);
}

/// Back home from a tool that used the network, the radio goes off at once,
/// not after its idle time.
#[test]
fn the_radio_goes_off_at_once_back_home() {
    let mut sim = wanted_and_on();
    while sim.scanned(&[]) == [Told::Scan] {}
    // The request failed, nothing waits; the radio waits for its idle time
    // while the tool shows.
    assert_eq!(sim.screen_shown(false), []);
    assert_eq!(sim.wait(10), []);
    assert_eq!(sim.screen_shown(true), [Told::Off]);
}

/// Home while the network is still wanted: the radio stays on.
#[test]
fn the_radio_stays_on_at_home_while_work_waits() {
    let mut sim = wanted_and_on();
    assert_eq!(sim.screen_shown(true), []);
}

#[test]
fn a_superseded_join_changes_and_says_nothing() {
    let mut sim = wanted_and_on();
    let nets = [net(HOME)];
    let before = sim.policy.tried.len();
    let actions = sim.call(&nets, |p, f| p.on_failed(f, HOME, Failure::Superseded));
    assert!(actions.is_empty());
    assert_eq!(sim.policy.tried.len(), before);
}

#[test]
fn a_chosen_network_that_failed_says_why_and_is_let_go() {
    let mut sim = Sim::new();
    sim.radio = Radio::On;
    let nets = [net("Cafe")];
    let actions = sim.call(&nets, |p, f| p.on_password(f, "Cafe", "secret"));
    assert!(matches!(&actions[..], [Action::Step(Step::Join { .. })]));
    let actions = sim.call(&nets, |p, f| p.on_failed(f, "Cafe", Failure::Password));
    assert!(sim.policy.pending.is_none(), "a wrong password is let go");
    assert_eq!(
        actions,
        [Action::Notify(Notice::Failed {
            ssid: "Cafe".to_string(),
            failure: Failure::Password
        })]
    );
}

#[test]
fn the_password_of_a_joined_network_is_kept_and_the_pick_remembered() {
    let mut sim = Sim::new();
    sim.radio = Radio::On;
    let nets = [net("Cafe")];
    sim.call(&nets, |p, f| p.on_password(f, "Cafe", "secret"));
    sim.radio = Radio::Online;
    let actions = sim.call_with(&nets, |f| f.joined = Some("Cafe"), |p, f| p.on_changed(f));
    assert_eq!(actions[0], Action::Picked("Cafe".to_string()));
    assert!(matches!(
        &actions[1],
        Action::KeepSecret { ssid, password } if ssid == "Cafe" && password.as_str() == "secret"
    ));
    assert!(sim.policy.pending.is_none());
    // Joined again: picked already, nothing to keep.
    let actions = sim.call_with(
        &nets,
        |f| {
            f.joined = Some("Cafe");
            f.join_first = Some("Cafe");
        },
        |p, f| p.on_changed(f),
    );
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::Picked(_) | Action::KeepSecret { .. }))
    );
}

#[test]
fn the_phone_setup_and_a_hosted_network_leave_the_radio_alone() {
    let mut sim = Sim::new();
    let nets = [net(HOME)];
    sim.policy.phone = true;
    assert!(sim.call(&nets, |p, f| p.on_wanted(f)).is_empty());
    assert!(sim.call(&nets, |p, f| p.on_scanned(f)).is_empty());
    sim.policy.phone = false;
    sim.radio = Radio::Hosting;
    assert!(sim.call(&nets, |p, f| p.on_wanted(f)).is_empty());
    assert!(sim.call(&nets, |p, f| p.on_screen_shown(f)).is_empty());
}

#[test]
fn charging_keeps_the_radio_on_and_then_lets_it_go() {
    let mut sim = Sim::new();
    let nets = [net(HOME)];
    let keep = |f: &mut Facts<'_>| f.charging_keeps = true;
    assert_eq!(
        sim.call_with(&nets, keep, |p, f| p.on_charging(f, false)),
        [Action::Step(Step::On)]
    );
    // No change of charging: nothing.
    assert!(
        sim.call_with(&nets, keep, |p, f| p.on_charging(f, false))
            .is_empty()
    );
    sim.radio = Radio::On;
    sim.policy.goal = Goal::Nothing;
    let actions = sim.call(&nets, |p, f| p.on_charging(f, false));
    assert!(matches!(
        &actions[..],
        [Action::Timer(Timer::IdleOff { due_ms: 60_000, .. })]
    ));
}

#[test]
fn nothing_to_join_fails_what_waits_at_once() {
    let mut sim = Sim::new();
    sim.known = Vec::new();
    sim.wanted = true;
    let nets = [];
    let actions = sim.call(&nets, |p, f| p.on_wanted(f));
    assert_eq!(actions, [Action::NoNetwork]);
    assert_eq!(sim.policy.asked_at, None);
}

#[test]
fn a_wifi_failure_reason_is_a_policy_failure() {
    use paperpaw_sdk::native::events::WifiFailure as Reason;
    for (reason, failure) in [
        (Reason::NotFound, Failure::NotFound),
        (Reason::Password, Failure::Password),
        (Reason::Timeout, Failure::Timeout),
        (Reason::Superseded, Failure::Superseded),
        (Reason::Unsupported, Failure::Other),
        (Reason::NeedsUser, Failure::Other),
        (Reason::Other, Failure::Other),
    ] {
        assert_eq!(Failure::from(reason), failure);
    }
}

#[test]
fn refused_allocations_end_in_no_memory_and_the_policy_recovers() {
    let known = known();
    let networks = [net("Cafe"), net(HOME)];
    sweep(|| {
        let mut policy = Policy::default();
        let f = Facts {
            radio: Radio::On,
            busy: false,
            wanted: true,
            networks: &networks,
            known: &known,
            now_ms: 5000,
            charging_keeps: false,
            at_rest: true,
            on_demand: true,
            idle_ms: 60_000,
            join_first: Some(HOME),
            joined: None,
        };
        policy.on_wanted(&f)?;
        policy.on_changed(&f)?;
        policy.on_scanned(&f)?;
        policy.on_failed(&f, HOME, Failure::Timeout)?;
        policy.on_password(&f, "Cafe", "pw")?;
        policy.on_idle(&f)?;
        policy.on_screen_shown(&f)?;
        policy.on_idle_timer(&f, policy.generation)?;
        policy.on_retry_timer(&f, policy.generation)?;
        policy.on_charging(&f, true)?;
        Ok(policy.tried.len())
    });
}
