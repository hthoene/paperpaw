use super::*;
use crate::fail_alloc::sweep;
use alloc::{format, string::ToString};

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
