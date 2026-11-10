//! When the radio is on and which network is joined: the decisions, kept
//! apart from the page so they can be tried without a device.
//!
//! - The radio comes on when a plugin wants the network and the user
//!   lets it ([`ON_DEMAND`](crate::ON_DEMAND)); then a secured network
//!   joined before is joined: the one the user picked last, if it is in
//!   reach and *Join first* says so ([`JOIN_FIRST`](crate::JOIN_FIRST)),
//!   else the strongest; the next one if that fails, the one picked last
//!   after a second try (open networks only when chosen, see
//!   [`best_known`]).
//! - A scan that finds no network joined before is made again, up to
//!   [`SCANS`] times, before none counts as in reach; so is one that
//!   misses the network to join first, before another one is joined.
//! - A round that joins nothing is tried again a little later while the
//!   network is still wanted or charging keeps the radio on (e.g. while a
//!   router starts, or once a first try had no answer). The network is
//!   wanted for as long as what waits for it does: up to
//!   [`NETWORK_WAIT_MS`] after it was asked for, and not after the plugin
//!   said none is coming (`Command::NoNetwork`), whatever the kernel's
//!   flag says.
//! - It goes off a while after nothing uses the network
//!   ([`IDLE_OFF`](crate::IDLE_OFF)), unless the user switched it on on
//!   the page (it stays on until switched off there) or it stays on
//!   while charging ([`WHILE_CHARGING`](crate::WHILE_CHARGING)).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Instant;
use paperpaw_sdk::wifi::{JoinFailure, NETWORK_WAIT_MS, Wifi, WifiCommand, WifiNetwork, WifiState};

/// Scans in a row that find no network joined before, until none counts
/// as in reach: the first after the radio comes on often finds none.
pub const SCANS: u8 = 3;

/// What the radio is for now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Goal {
    /// Nothing to join.
    #[default]
    Nothing,
    /// The strongest network joined before, once networks are found.
    Known,
    /// The network the user chose, with the password given.
    Chosen { ssid: String, password: String },
}

/// What the plugin keeps between events.
#[derive(Debug, Default)]
pub struct Policy {
    pub goal: Goal,
    /// Networks tried this round (to try the next one).
    pub tried: Vec<String>,
    /// Scans this round that found no network joined before.
    pub scans: u8,
    /// The user switched the radio on (on the page): it stays on.
    pub manual: bool,
    /// A network chosen with a password not kept yet: kept once joined,
    /// and offered again when the network is chosen after a failure
    /// (let go only when the password was wrong).
    pub pending: Option<(String, String)>,
    /// Counts the times the radio was wanted; a timer to switch it off
    /// carries the count it was set at and does nothing once it changed.
    pub generation: u32,
    /// Charging kept the radio on, as last seen (to act on changes only).
    pub charging_kept: bool,
    /// When a plugin asked for the network last; `None` once the plugin
    /// said none is coming, which fails what waited.
    pub asked_at: Option<Instant>,
    /// The setup with a phone is under way: it says what the radio does,
    /// nothing is joined by itself meanwhile.
    pub phone: bool,
    /// The network to join first when in reach: the one the user picked
    /// last ([`JOIN_FIRST`](crate::JOIN_FIRST)); none for the strongest.
    pub first: Option<String>,
    /// `first` failed once this round and was tried again.
    pub first_again: bool,
}

/// Of `networks` joined before (`known`) and not tried yet, the one to
/// join without asking: `first` if it is among them, else the strongest.
/// Only secured networks with their password: anyone can set up an open
/// network by any name, so an open one (or an open twin of a secured one)
/// is joined only when chosen on the page.
pub fn best_known<'a>(
    networks: &[WifiNetwork],
    known: &'a [(String, String)],
    tried: &[String],
    first: Option<&str>,
) -> Option<&'a (String, String)> {
    let joinable = |n: &&WifiNetwork| n.secured && !tried.contains(&n.ssid);
    let known_as = |n: &WifiNetwork| {
        known
            .iter()
            .find(|(ssid, password)| *ssid == n.ssid && !password.is_empty())
    };
    // Networks come strongest first.
    networks
        .iter()
        .filter(joinable)
        .filter(|n| Some(n.ssid.as_str()) == first)
        .find_map(known_as)
        .or_else(|| networks.iter().filter(joinable).find_map(known_as))
}

impl Policy {
    /// Whether work still waits for the network at `now`: the kernel says
    /// so, and it can still be waiting, asked for less than
    /// [`NETWORK_WAIT_MS`] ago and not failed by the plugin.
    pub fn still_wanted(&self, wifi: &Wifi, now: Instant) -> bool {
        wifi.wanted
            && self
                .asked_at
                .is_some_and(|at| now < at.add_millis(NETWORK_WAIT_MS))
    }

    /// The network is wanted: the radio on, or networks looked for.
    pub fn wanted(&mut self, state: &WifiState, known: &[(String, String)]) -> Vec<WifiCommand> {
        self.generation = self.generation.wrapping_add(1);
        if known.is_empty() {
            return Vec::new();
        }
        match state {
            WifiState::Off => {
                self.start_round(Goal::Known);
                alloc::vec![WifiCommand::On]
            }
            WifiState::On => {
                self.start_round(Goal::Known);
                alloc::vec![WifiCommand::Scan]
            }
            WifiState::Joining { .. } | WifiState::Online { .. } => Vec::new(),
            _ => Vec::new(),
        }
    }

    /// Tries the networks joined before again (after a round found none
    /// or failed): networks looked for anew.
    pub fn retry(&mut self) -> Vec<WifiCommand> {
        self.start_round(Goal::Known);
        alloc::vec![WifiCommand::Scan]
    }

    fn start_round(&mut self, goal: Goal) {
        self.goal = goal;
        self.first_again = false;
        self.tried.clear();
        self.scans = 0;
    }

    /// The user chose a network (with its password, or a kept one).
    pub fn choose(&mut self, state: &WifiState, ssid: &str, password: &str) -> Vec<WifiCommand> {
        self.start_round(Goal::Chosen {
            ssid: String::from(ssid),
            password: String::from(password),
        });
        if state.is_on() {
            alloc::vec![WifiCommand::Join {
                ssid: String::from(ssid),
                password: String::from(password),
            }]
        } else {
            self.manual = true;
            alloc::vec![WifiCommand::On]
        }
    }

    /// The radio's state changed.
    pub fn changed(&mut self, state: &WifiState) -> Vec<WifiCommand> {
        match state {
            WifiState::On => match &self.goal {
                Goal::Nothing => Vec::new(),
                Goal::Known => alloc::vec![WifiCommand::Scan],
                Goal::Chosen { ssid, password } => alloc::vec![WifiCommand::Join {
                    ssid: ssid.clone(),
                    password: password.clone(),
                }],
            },
            WifiState::Online { .. } => {
                self.goal = Goal::Nothing;
                self.tried.clear();
                Vec::new()
            }
            WifiState::Off => {
                self.goal = Goal::Nothing;
                self.manual = false;
                Vec::new()
            }
            WifiState::Joining { .. } => Vec::new(),
            _ => Vec::new(),
        }
    }

    /// Networks were found: the best one joined before, if one is wanted.
    /// `None` when there is none to try.
    pub fn scanned(
        &mut self,
        networks: &[WifiNetwork],
        known: &[(String, String)],
    ) -> Option<Vec<WifiCommand>> {
        if self.goal != Goal::Known {
            return Some(Vec::new());
        }
        // The network to join first not heard: the first scans after the
        // radio came on often miss a weaker one (seen on the X4 after
        // its restart for a stalled connection), so it is looked for
        // again before another one is joined.
        if let Some(first) = &self.first
            && self.scans + 1 < SCANS
            && !self.tried.contains(first)
            && known
                .iter()
                .any(|(ssid, password)| ssid == first && !password.is_empty())
            && !networks.iter().any(|n| n.ssid == *first)
        {
            self.scans += 1;
            return Some(alloc::vec![WifiCommand::Scan]);
        }
        let (ssid, password) = best_known(networks, known, &self.tried, self.first.as_deref())?;
        self.tried.push(ssid.clone());
        Some(alloc::vec![WifiCommand::Join {
            ssid: ssid.clone(),
            password: password.clone(),
        }])
    }

    /// Joining `ssid` failed (`reason`): the next network joined before,
    /// if any is wanted. `None` when there is none to try. A password
    /// typed for it is kept to offer again, unless it was wrong.
    pub fn failed(
        &mut self,
        ssid: &str,
        reason: JoinFailure,
        networks: &[WifiNetwork],
        known: &[(String, String)],
    ) -> Option<Vec<WifiCommand>> {
        if reason == JoinFailure::Password
            && self
                .pending
                .as_ref()
                .is_some_and(|(pending, _)| pending == ssid)
        {
            self.pending = None;
        }
        match self.goal {
            Goal::Known => {
                // The network to join first gets a second try before the
                // next: one of its access points, weak or far, refuses a
                // join now and then (seen on the X4 at -72 and -90 dBm),
                // and the next network would then stay joined.
                if !self.first_again
                    && self.first.as_deref() == Some(ssid)
                    && networks.iter().any(|n| n.ssid == ssid)
                    && let Some((ssid, password)) = known
                        .iter()
                        .find(|(known, password)| known == ssid && !password.is_empty())
                {
                    self.first_again = true;
                    return Some(alloc::vec![WifiCommand::Join {
                        ssid: ssid.clone(),
                        password: password.clone(),
                    }]);
                }
                if !self.tried.iter().any(|t| t == ssid) {
                    self.tried.push(String::from(ssid));
                }
                self.scanned(networks, known)
            }
            Goal::Chosen { .. } => {
                self.goal = Goal::Nothing;
                Some(Vec::new())
            }
            Goal::Nothing => Some(Vec::new()),
        }
    }

    /// Whether the radio may go off now that nothing uses the network.
    pub fn may_go_off(&self, charging_keeps_it: bool) -> bool {
        !self.manual && !charging_keeps_it && self.goal == Goal::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The network picked last comes first when in reach, though weaker;
    /// the strongest when it is not, or when it failed.
    #[test]
    fn the_network_picked_last_comes_first() {
        let known = alloc::vec![
            (String::from("Hannes"), String::from("a")),
            (String::from("FRITZ!Box"), String::from("b")),
        ];
        let networks = [net("Hannes", -45), net("FRITZ!Box", -62)];
        let mut policy = Policy {
            first: Some(String::from("FRITZ!Box")),
            ..Policy::default()
        };
        policy.start_round(Goal::Known);
        let join = |ssid: &str, password: &str| WifiCommand::Join {
            ssid: String::from(ssid),
            password: String::from(password),
        };
        assert_eq!(
            policy.scanned(&networks, &known).unwrap(),
            [join("FRITZ!Box", "b")]
        );
        // A second try, then the next.
        assert_eq!(
            policy
                .failed("FRITZ!Box", JoinFailure::Timeout, &networks, &known)
                .unwrap(),
            [join("FRITZ!Box", "b")]
        );
        assert_eq!(
            policy
                .failed("FRITZ!Box", JoinFailure::Password, &networks, &known)
                .unwrap(),
            [join("Hannes", "a")]
        );
        // Not heard: looked for again, then the strongest.
        policy.start_round(Goal::Known);
        for _ in 1..SCANS {
            assert_eq!(
                policy.scanned(&networks[..1], &known).unwrap(),
                [WifiCommand::Scan]
            );
        }
        assert_eq!(
            policy.scanned(&networks[..1], &known).unwrap(),
            [join("Hannes", "a")]
        );
        // Heard on the next scan: joined.
        policy.start_round(Goal::Known);
        assert_eq!(
            policy.scanned(&networks[..1], &known).unwrap(),
            [WifiCommand::Scan]
        );
        assert_eq!(
            policy.scanned(&networks, &known).unwrap(),
            [join("FRITZ!Box", "b")]
        );
        // The strongest first, as set.
        policy.first = None;
        policy.start_round(Goal::Known);
        assert_eq!(
            policy.scanned(&networks, &known).unwrap(),
            [join("Hannes", "a")]
        );
    }

    /// After the radio started anew (switched off and on, or restarted
    /// for a stalled connection), the network picked last is joined
    /// again, though another one joined before is stronger.
    #[test]
    fn the_network_picked_last_is_joined_again_after_a_restart() {
        let known = alloc::vec![
            (String::from("Hannes"), String::from("a")),
            (String::from("FRITZ!Box"), String::from("b")),
        ];
        let networks = [net("Hannes", -45), net("FRITZ!Box", -72)];
        let mut policy = Policy {
            first: Some(String::from("FRITZ!Box")),
            ..Policy::default()
        };
        let joins_fritz = |policy: &mut Policy| {
            assert_eq!(policy.wanted(&WifiState::Off, &known), [WifiCommand::On]);
            assert_eq!(policy.changed(&WifiState::On), [WifiCommand::Scan]);
            assert_eq!(
                policy.scanned(&networks, &known).unwrap(),
                [join("FRITZ!Box", "b")]
            );
            let online = WifiState::Online {
                ssid: String::from("FRITZ!Box"),
                address: String::from("192.168.178.39"),
                name: None,
            };
            assert!(policy.changed(&online).is_empty());
        };
        joins_fritz(&mut policy);
        assert!(policy.changed(&WifiState::Off).is_empty());
        joins_fritz(&mut policy);
        // A failed try last round does not use up this round's second try.
        policy.first_again = true;
        assert!(policy.changed(&WifiState::Off).is_empty());
        assert_eq!(policy.wanted(&WifiState::Off, &known), [WifiCommand::On]);
        assert_eq!(policy.changed(&WifiState::On), [WifiCommand::Scan]);
        policy.scanned(&networks, &known);
        assert_eq!(
            policy
                .failed("FRITZ!Box", JoinFailure::Other, &networks, &known)
                .unwrap(),
            [join("FRITZ!Box", "b")]
        );
    }

    fn net(ssid: &str, signal: i8) -> WifiNetwork {
        WifiNetwork::new(String::from(ssid), signal, true)
    }

    fn known() -> Vec<(String, String)> {
        alloc::vec![
            (String::from("Home"), String::from("pw")),
            (String::from("Office"), String::from("pw2")),
        ]
    }

    fn join(ssid: &str, password: &str) -> WifiCommand {
        WifiCommand::Join {
            ssid: String::from(ssid),
            password: String::from(password),
        }
    }

    #[test]
    fn wanted_joins_the_strongest_known_then_the_next() {
        let mut policy = Policy::default();
        let known = known();
        assert_eq!(policy.wanted(&WifiState::Off, &known), [WifiCommand::On]);
        assert_eq!(policy.changed(&WifiState::On), [WifiCommand::Scan]);
        let networks = [net("Cafe", -40), net("Office", -50), net("Home", -60)];
        assert_eq!(
            policy.scanned(&networks, &known).unwrap(),
            [join("Office", "pw2")]
        );
        assert_eq!(
            policy
                .failed("Office", JoinFailure::Timeout, &networks, &known)
                .unwrap(),
            [join("Home", "pw")]
        );
        assert_eq!(
            policy.failed("Home", JoinFailure::Timeout, &networks, &known),
            None
        );
        let online = WifiState::Online {
            ssid: String::from("Home"),
            address: String::from("10.0.0.2"),
            name: None,
        };
        assert!(policy.changed(&online).is_empty());
        assert!(policy.may_go_off(false));
        assert!(!policy.may_go_off(true));
    }

    #[test]
    fn only_secured_networks_are_joined_by_themselves() {
        let mut policy = Policy::default();
        let known = alloc::vec![
            (String::from("Home"), String::from("pw")),
            (String::from("Cafe"), String::new()),
        ];
        policy.wanted(&WifiState::On, &known);
        // An open network named as a secured one joined before (a twin set
        // up by someone else), and an open one joined before: neither.
        let open = |ssid: &str| WifiNetwork::new(ssid, -40, false);
        assert_eq!(policy.scanned(&[open("Home"), open("Cafe")], &known), None);
        policy.wanted(&WifiState::On, &known);
        assert_eq!(
            policy
                .scanned(&[open("Cafe"), net("Home", -60)], &known)
                .unwrap(),
            [join("Home", "pw")]
        );
    }

    #[test]
    fn nothing_known_nothing_done() {
        let mut policy = Policy::default();
        assert!(policy.wanted(&WifiState::Off, &[]).is_empty());
        assert_eq!(policy.scanned(&[net("Cafe", -40)], &known()).unwrap(), []);
    }

    #[test]
    fn a_chosen_network_switches_the_radio_on_and_keeps_it_on() {
        let mut policy = Policy::default();
        assert_eq!(
            policy.choose(&WifiState::Off, "Cafe", ""),
            [WifiCommand::On]
        );
        assert_eq!(policy.changed(&WifiState::On), [join("Cafe", "")]);
        assert!(policy.manual);
        policy.goal = Goal::Nothing;
        assert!(!policy.may_go_off(false));
        assert!(policy.changed(&WifiState::Off).is_empty());
        assert!(!policy.manual);
    }

    #[test]
    fn a_wrong_password_forgets_what_was_typed() {
        let mut policy = Policy {
            pending: Some((String::from("Cafe"), String::from("typo"))),
            ..Policy::default()
        };
        policy.choose(&WifiState::On, "Cafe", "typo");
        assert_eq!(
            policy
                .failed("Cafe", JoinFailure::Password, &[], &[])
                .unwrap(),
            []
        );
        assert_eq!(policy.pending, None);
        assert_eq!(policy.goal, Goal::Nothing);
    }

    /// Any other failure (no answer, refused, broken off) keeps what was
    /// typed, to offer again: the user saw it fail, the password was not
    /// said to be wrong.
    #[test]
    fn another_failure_keeps_what_was_typed() {
        for reason in [
            JoinFailure::Timeout,
            JoinFailure::NotFound,
            JoinFailure::Other,
        ] {
            let mut policy = Policy {
                pending: Some((String::from("Hannes"), String::from("secret"))),
                ..Policy::default()
            };
            policy.choose(
                &WifiState::Online {
                    ssid: "Home".into(),
                    address: "a".into(),
                    name: None,
                },
                "Hannes",
                "secret",
            );
            assert_eq!(policy.failed("Hannes", reason, &[], &[]).unwrap(), []);
            assert_eq!(
                policy.pending,
                Some((String::from("Hannes"), String::from("secret"))),
                "{reason:?}"
            );
        }
    }
}
