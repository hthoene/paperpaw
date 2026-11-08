use super::*;
use crate::fail_alloc::sweep;
use crate::policy::{Goal as OldGoal, Policy as Old};
use alloc::{format, string::ToString};
use paperpaw_sdk::wifi::{JoinFailure, WifiCommand, WifiNetwork, WifiState};

fn net(ssid: &str, secured: bool) -> Net {
    Net {
        ssid: ssid.to_string(),
        secured,
    }
}

fn known(list: &[(&str, &str)]) -> Vec<Known> {
    list.iter()
        .map(|(ssid, password)| Known {
            ssid: ssid.to_string(),
            password: Password::new(password).unwrap(),
        })
        .collect()
}

fn join(ssid: &str, password: &str) -> Step {
    Step::Join {
        ssid: ssid.to_string(),
        password: Password::new(password).unwrap(),
    }
}

#[test]
fn the_strongest_known_comes_first_then_the_next() {
    let mut policy = Policy::default();
    let known = known(&[("Home", "pw"), ("Office", "pw2")]);
    assert_eq!(policy.wanted(Radio::Off, &known).unwrap(), [Step::On]);
    assert_eq!(policy.changed(Radio::On).unwrap(), [Step::Scan]);
    let networks = [net("Cafe", true), net("Office", true), net("Home", true)];
    assert_eq!(
        policy.scanned(&networks, &known).unwrap().unwrap(),
        [join("Office", "pw2")]
    );
    assert_eq!(
        policy
            .failed("Office", Failure::Timeout, &networks, &known)
            .unwrap()
            .unwrap(),
        [join("Home", "pw")]
    );
    assert_eq!(
        policy
            .failed("Home", Failure::Timeout, &networks, &known)
            .unwrap(),
        None
    );
    assert!(policy.changed(Radio::Online).unwrap().is_empty());
    assert!(policy.may_go_off(false));
    assert!(!policy.may_go_off(true));
}

#[test]
fn only_secured_networks_are_joined_by_themselves() {
    let mut policy = Policy::default();
    let known = known(&[("Home", "pw"), ("Cafe", "")]);
    policy.wanted(Radio::On, &known).unwrap();
    assert_eq!(
        policy
            .scanned(&[net("Home", false), net("Cafe", false)], &known)
            .unwrap(),
        None
    );
    policy.wanted(Radio::On, &known).unwrap();
    assert_eq!(
        policy
            .scanned(&[net("Cafe", false), net("Home", true)], &known)
            .unwrap()
            .unwrap(),
        [join("Home", "pw")]
    );
}

#[test]
fn a_chosen_network_switches_the_radio_on_and_keeps_it_on() {
    let mut policy = Policy::default();
    assert_eq!(policy.choose(Radio::Off, "Cafe", "").unwrap(), [Step::On]);
    assert_eq!(policy.changed(Radio::On).unwrap(), [join("Cafe", "")]);
    assert!(policy.manual);
    policy.goal = Goal::Nothing;
    assert!(!policy.may_go_off(false));
    assert!(policy.changed(Radio::Off).unwrap().is_empty());
    assert!(!policy.manual);
}

#[test]
fn a_wrong_password_forgets_what_was_typed_another_failure_keeps_it() {
    let pending = || Some(("Cafe".to_string(), Password::new("typo").unwrap()));
    let mut policy = Policy {
        pending: pending(),
        ..Policy::default()
    };
    policy.choose(Radio::On, "Cafe", "typo").unwrap();
    assert!(
        policy
            .failed("Cafe", Failure::Password, &[], &[])
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert_eq!(policy.pending, None);
    assert_eq!(policy.goal, Goal::Nothing);
    for failure in [Failure::Timeout, Failure::NotFound, Failure::Other] {
        let mut policy = Policy {
            pending: pending(),
            ..Policy::default()
        };
        policy.choose(Radio::Online, "Cafe", "typo").unwrap();
        policy.failed("Cafe", failure, &[], &[]).unwrap();
        assert_eq!(policy.pending, pending(), "{failure:?}");
    }
}

#[test]
fn a_superseded_join_changes_nothing() {
    let mut policy = Policy::default();
    policy.choose(Radio::On, "Cafe", "pw").unwrap();
    let goal = policy.goal.try_clone_for_test();
    assert!(
        policy
            .failed("Cafe", Failure::Superseded, &[], &[])
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert_eq!(policy.goal, goal);
    assert_eq!(Failure::from_reason(4), Failure::Superseded);
    assert_eq!(Failure::from_reason(6), Failure::Other);
    assert_eq!(Failure::from_reason(99), Failure::Other);
}

impl Goal {
    fn try_clone_for_test(&self) -> Self {
        match self {
            Self::Nothing => Self::Nothing,
            Self::Known => Self::Known,
            Self::Chosen { ssid, password } => Self::Chosen {
                ssid: ssid.clone(),
                password: password.try_clone().unwrap(),
            },
        }
    }
}

#[test]
fn the_network_is_wanted_for_as_long_as_the_plugin_waits() {
    let mut policy = Policy::default();
    assert!(!policy.still_wanted(true, 0));
    policy.asked_at = Some(1_000);
    assert!(policy.still_wanted(true, 1_000 + NETWORK_WAIT_MS - 1));
    assert!(!policy.still_wanted(true, 1_000 + NETWORK_WAIT_MS));
    assert!(!policy.still_wanted(false, 1_000));
    policy.asked_at = Some(u64::MAX);
    assert!(policy.still_wanted(true, u64::MAX - 1));
}

#[test]
fn a_password_is_not_printed() {
    let text = format!("{:?}", join("Cafe", "hunter2"));
    assert!(!text.contains("hunter2"), "{text}");
}

// The interface-6 policy and this one, driven by the same events.

struct Rng(u64);
impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

const SSIDS: [&str; 4] = ["Home", "Office", "Cafe", "FRITZ!Box"];

fn old_steps(commands: &[WifiCommand]) -> Vec<String> {
    commands
        .iter()
        .map(|c| match c {
            WifiCommand::On => "on".to_string(),
            WifiCommand::Scan => "scan".to_string(),
            WifiCommand::Join { ssid, password } => format!("join {ssid} {password}"),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn new_steps(steps: &[Step]) -> Vec<String> {
    steps
        .iter()
        .map(|s| match s {
            Step::On => "on".to_string(),
            Step::Scan => "scan".to_string(),
            Step::Join { ssid, password } => format!("join {ssid} {}", password.as_str()),
        })
        .collect()
}

fn goal_name(goal: &Goal) -> String {
    match goal {
        Goal::Nothing => "nothing".to_string(),
        Goal::Known => "known".to_string(),
        Goal::Chosen { ssid, password } => format!("chosen {ssid} {}", password.as_str()),
    }
}

fn old_goal_name(goal: &OldGoal) -> String {
    match goal {
        OldGoal::Nothing => "nothing".to_string(),
        OldGoal::Known => "known".to_string(),
        OldGoal::Chosen { ssid, password } => format!("chosen {ssid} {password}"),
    }
}

fn old_state(radio: Radio) -> WifiState {
    match radio {
        Radio::Off => WifiState::Off,
        Radio::On => WifiState::On,
        Radio::Joining => WifiState::Joining {
            ssid: "x".to_string(),
        },
        Radio::Online => WifiState::Online {
            ssid: "x".to_string(),
            address: "a".to_string(),
            name: None,
        },
        Radio::Hosting => WifiState::Hosting {
            ssid: "x".to_string(),
            address: "a".to_string(),
            guests: 0,
        },
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one scripted run of both policies, read top to bottom"
)]
fn it_decides_as_the_interface_6_policy_does() {
    let radios = [
        Radio::Off,
        Radio::On,
        Radio::Joining,
        Radio::Online,
        Radio::Hosting,
    ];
    for seed in 0..200 {
        let mut rng = Rng(seed);
        let mut old = Old::default();
        let mut new = Policy::default();
        let book: Vec<(&str, &str)> = SSIDS
            .iter()
            .take(1 + rng.next(4) as usize)
            .map(|s| (*s, if rng.next(4) == 0 { "" } else { "pw" }))
            .collect();
        let old_known: Vec<(String, String)> = book
            .iter()
            .map(|(s, p)| (s.to_string(), p.to_string()))
            .collect();
        let new_known = known(&book);
        if rng.next(2) == 0 {
            let first = SSIDS[rng.next(4) as usize].to_string();
            old.first = Some(first.clone());
            new.first = Some(first);
        }
        for step in 0..40 {
            let radio = radios[rng.next(5) as usize];
            let mut spec: Vec<(&str, i8, bool)> = Vec::new();
            for ssid in SSIDS {
                if rng.next(3) != 0 {
                    spec.push((ssid, -40 - rng.next(40) as i8, rng.next(5) != 0));
                }
            }
            let old_nets: Vec<WifiNetwork> = spec
                .iter()
                .map(|(s, sig, sec)| WifiNetwork::new(*s, *sig, *sec))
                .collect();
            let new_nets: Vec<Net> = spec.iter().map(|(s, _, sec)| net(s, *sec)).collect();
            let ssid = SSIDS[rng.next(4) as usize];
            let (failure, old_failure) = match rng.next(4) {
                0 => (Failure::NotFound, JoinFailure::NotFound),
                1 => (Failure::Password, JoinFailure::Password),
                2 => (Failure::Timeout, JoinFailure::Timeout),
                _ => (Failure::Other, JoinFailure::Other),
            };
            let label = format!("seed {seed} step {step}");
            match rng.next(7) {
                0 => {
                    let a = old.wanted(&old_state(radio), &old_known);
                    let b = new.wanted(radio, &new_known).unwrap();
                    assert_eq!(old_steps(&a), new_steps(&b), "{label} wanted");
                }
                1 => {
                    let a = old.retry();
                    let b = new.retry().unwrap();
                    assert_eq!(old_steps(&a), new_steps(&b), "{label} retry");
                }
                2 => {
                    let password = if rng.next(2) == 0 { "" } else { "typed" };
                    let a = old.choose(&old_state(radio), ssid, password);
                    let b = new.choose(radio, ssid, password).unwrap();
                    assert_eq!(old_steps(&a), new_steps(&b), "{label} choose");
                    old.pending = Some((ssid.to_string(), password.to_string()));
                    new.pending = Some((ssid.to_string(), Password::new(password).unwrap()));
                }
                3 => {
                    let a = old.changed(&old_state(radio));
                    let b = new.changed(radio).unwrap();
                    assert_eq!(old_steps(&a), new_steps(&b), "{label} changed");
                }
                4 => {
                    let a = old.scanned(&old_nets, &old_known);
                    let b = new.scanned(&new_nets, &new_known).unwrap();
                    assert_eq!(
                        a.as_deref().map(old_steps),
                        b.as_deref().map(new_steps),
                        "{label} scanned"
                    );
                }
                5 => {
                    let a = old.failed(ssid, old_failure, &old_nets, &old_known);
                    let b = new.failed(ssid, failure, &new_nets, &new_known).unwrap();
                    assert_eq!(
                        a.as_deref().map(old_steps),
                        b.as_deref().map(new_steps),
                        "{label} failed"
                    );
                }
                _ => {
                    let charging = rng.next(2) == 0;
                    assert_eq!(
                        old.may_go_off(charging),
                        new.may_go_off(charging),
                        "{label}"
                    );
                    let now = rng.next(100_000);
                    let asked = rng.next(100_000);
                    old.asked_at = Some(paperpaw_sdk::Instant(asked));
                    new.asked_at = Some(asked);
                    let mut wifi = paperpaw_sdk::wifi::Wifi::default();
                    wifi.wanted = true;
                    assert_eq!(
                        old.still_wanted(&wifi, paperpaw_sdk::Instant(now)),
                        new.still_wanted(true, now),
                        "{label} wait"
                    );
                }
            }
            assert_eq!(old_goal_name(&old.goal), goal_name(&new.goal), "{label}");
            assert_eq!(old.tried, new.tried, "{label} tried");
            assert_eq!(
                (old.scans, old.manual, old.generation, old.first_again),
                (new.scans, new.manual, new.generation, new.first_again),
                "{label} state"
            );
            assert_eq!(
                old.pending.as_ref().map(|(s, p)| (s.as_str(), p.as_str())),
                new.pending.as_ref().map(|(s, p)| (s.as_str(), p.as_str())),
                "{label} pending"
            );
        }
    }
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    let known = known(&[("Home", "pw"), ("Office", "pw2")]);
    let networks = [net("Office", true), net("Home", true), net("Cafe", false)];
    sweep(|| {
        let mut policy = Policy {
            first: Some(paperpaw_ui_kit::text::copy("Home")?),
            ..Policy::default()
        };
        let mut n = policy.wanted(Radio::Off, &known)?.len();
        n += policy.changed(Radio::On)?.len();
        n += policy
            .scanned(&networks[1..], &known)?
            .map_or(0, |s| s.len());
        n += policy
            .failed("Home", Failure::Timeout, &networks, &known)?
            .map_or(0, |s| s.len());
        n += policy
            .failed("Home", Failure::Timeout, &networks, &known)?
            .map_or(0, |s| s.len());
        n += policy.retry()?.len();
        n += policy.choose(Radio::On, "Cafe", "pw")?.len();
        n += policy.choose(Radio::Off, "Cafe", "pw")?.len();
        n += policy.changed(Radio::On)?.len();
        Ok(n)
    });
}
