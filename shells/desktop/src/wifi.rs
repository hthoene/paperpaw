//! A pretend radio for profiles of devices where PaperPaw manages Wi-Fi
//! (the X4): it answers the kernel's Wi-Fi calls as such a device would,
//! with made-up networks and delays, so that the `wifi` plugin and what
//! waits for a network can be tried and tested here. Once "online", the
//! computer's own network is used.
//!
//! The networks: `PaperPaw Test` (any password but `wrong`), `Cafe`
//! (open) and `Far Away` (out of reach once joined); with `--wifi online`
//! also `Workshop Wi-Fi`, joined from the start ([`Wifi::online`]).

use std::collections::VecDeque;

use paperpaw_core::{JoinFailure, WifiCommand, WifiEvent, WifiNetwork, WifiState};
use paperpaw_platform::Instant;

/// How long the pretend radio takes to start, scan and join.
const START_MS: u64 = 200;
const SCAN_MS: u64 = 1500;
const JOIN_MS: u64 = 1000;

/// The address the emulated device gets.
const ADDRESS: &str = "192.168.1.42";
/// The device's address in its own network (`WifiCommand::Host`), as the
/// X4 has it: what its pages are shown at there (`served_at`), though
/// they are served on this computer.
const HOST_ADDRESS: &str = "192.168.4.1";

/// The pretend radio's strongest network, joined with any password but
/// `wrong`.
pub const TEST_NETWORK: &str = "PaperPaw Test";
/// The network an emulator started with `--wifi online` is in, known as
/// joined before (with [`WORKSHOP_PASSWORD`]).
pub const WORKSHOP_NETWORK: &str = "Workshop Wi-Fi";
pub const WORKSHOP_PASSWORD: &str = "workshop";

/// The networks the pretend radio finds, strongest first: ssid, signal in
/// dBm, secured. Static, so the emulator's `core.device` host lists them
/// without allocating (`device_host`).
pub(crate) const PRETEND_SCAN: &[(&str, i8, bool)] = &[
    (TEST_NETWORK, -48, true),
    ("Cafe", -70, false),
    ("Far Away", -88, true),
];

/// The networks the pretend radio finds; `workshop` adds the one an
/// emulator started with `--wifi online` is in.
pub(crate) fn pretend_networks(workshop: bool) -> Vec<WifiNetwork> {
    let net = |ssid: &str, signal, secured| WifiNetwork::new(String::from(ssid), signal, secured);
    let mut networks: Vec<WifiNetwork> = PRETEND_SCAN
        .iter()
        .map(|&(ssid, signal, secured)| net(ssid, signal, secured))
        .collect();
    if workshop {
        networks.insert(0, net(WORKSHOP_NETWORK, -40, true));
    }
    networks
}

#[derive(Debug, Default)]
pub struct Wifi {
    state: WifiState,
    /// What is reported when, in the order of time.
    due: VecDeque<(Instant, WifiEvent)>,
    /// The workshop's network is in reach ([`Wifi::online`]).
    workshop: bool,
}

impl Wifi {
    /// The radio is on and joined to a network.
    #[must_use]
    pub fn is_online(&self) -> bool {
        matches!(self.state, WifiState::Online { .. })
    }

    fn networks(&self) -> Vec<WifiNetwork> {
        pretend_networks(self.workshop)
    }

    /// The radio on and in the workshop's network now, as a device that
    /// started on USB in a network joined before: what `--wifi online`
    /// starts with. It stays in reach when the radio goes off and on.
    pub fn online(&mut self, now: Instant) {
        self.workshop = true;
        self.due.clear();
        self.set(
            now,
            WifiState::Online {
                ssid: String::from(WORKSHOP_NETWORK),
                address: String::from(ADDRESS),
                name: None,
            },
        );
    }

    /// Does what the kernel asked.
    pub fn apply(&mut self, call: WifiCommand, now: Instant) {
        let after = |ms| now.add_millis(ms);
        match call {
            WifiCommand::On if self.state == WifiState::Off => {
                self.due.clear();
                self.set(after(START_MS), WifiState::On);
            }
            WifiCommand::On => {}
            WifiCommand::Off => {
                self.due.clear();
                self.set(now, WifiState::Off);
            }
            WifiCommand::Scan if self.state.is_on() => {
                self.report(after(SCAN_MS), WifiEvent::Scanned(self.networks()));
            }
            WifiCommand::Scan => {}
            WifiCommand::Join { ssid, password } if self.state.is_on() => {
                self.due.retain(|(_, e)| !matches!(e, WifiEvent::State(_)));
                self.set(now, WifiState::Joining { ssid: ssid.clone() });
                let network = self.networks().into_iter().find(|n| n.ssid == ssid);
                let failure = match &network {
                    None => Some(JoinFailure::NotFound),
                    Some(n) if n.signal < -85 => Some(JoinFailure::Timeout),
                    Some(n) if n.secured && (password.is_empty() || password == "wrong") => {
                        Some(JoinFailure::Password)
                    }
                    Some(_) => None,
                };
                match failure {
                    Some(reason) => {
                        self.set(after(JOIN_MS), WifiState::On);
                        self.report(after(JOIN_MS), WifiEvent::Failed { ssid, reason });
                    }
                    None => self.set(
                        after(JOIN_MS),
                        WifiState::Online {
                            ssid,
                            address: String::from(ADDRESS),
                            name: None,
                        },
                    ),
                }
            }
            WifiCommand::Join { ssid, .. } => self.report(
                now,
                WifiEvent::Failed {
                    ssid,
                    reason: JoinFailure::Other,
                },
            ),
            WifiCommand::Leave if self.state.is_on() => {
                self.due.clear();
                self.set(now, WifiState::On);
            }
            WifiCommand::Leave => {}
            // The device's own network: its pages served on this computer.
            WifiCommand::Host { ssid, .. } if self.state.is_on() => {
                self.due.retain(|(_, e)| !matches!(e, WifiEvent::State(_)));
                self.set(
                    after(START_MS),
                    WifiState::Hosting {
                        ssid,
                        address: String::from(HOST_ADDRESS),
                        guests: 0,
                    },
                );
            }
            // Hosting with the radio off, and a command to come.
            _ => {}
        }
    }

    /// A phone joins the device's own network and is given an address
    /// (the script's `phone-joins`): there is no phone here.
    pub fn phone_joins(&mut self, now: Instant) {
        if let WifiState::Hosting {
            ssid,
            address,
            guests,
        } = &self.state
        {
            let state = WifiState::Hosting {
                ssid: ssid.clone(),
                address: address.clone(),
                guests: guests.saturating_add(1),
            };
            self.set(now, state);
        }
    }

    /// Reports `state` at `at` (the state counts as it from then on).
    fn set(&mut self, at: Instant, state: WifiState) {
        self.state = state.clone();
        self.report(at, WifiEvent::State(state));
    }

    /// Reports `event` at `at`, after what is due before or then: a state
    /// reported now does not wait for a scan still going on.
    fn report(&mut self, at: Instant, event: WifiEvent) {
        let place = self.due.partition_point(|(due, _)| *due <= at);
        self.due.insert(place, (at, event));
    }

    /// Where a server that started at `address` (this computer's, with
    /// its port) is shown: in the device's own network at the device's
    /// address there, as on the X4, never at this computer's (a phone in
    /// that network knows no other).
    pub fn served_at(&self, address: &str) -> String {
        match address.rsplit_once(':') {
            Some((_, port)) if self.state.is_hosting() => format!("{HOST_ADDRESS}:{port}"),
            _ => String::from(address),
        }
    }

    /// When something is to be reported next.
    pub fn next_due(&self) -> Option<Instant> {
        self.due.front().map(|(at, _)| *at)
    }

    /// The next thing to report, if due.
    pub fn take_due(&mut self, now: Instant) -> Option<WifiEvent> {
        if self.due.front().is_some_and(|(at, _)| *at <= now) {
            self.due.pop_front().map(|(_, event)| event)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(wifi: &mut Wifi, until: u64) -> Vec<WifiEvent> {
        let mut events = Vec::new();
        while let Some(event) = wifi.take_due(Instant(until)) {
            events.push(event);
        }
        events
    }

    /// Pages served in the device's own network show at its address
    /// there, as on the X4; in another network, at this computer's.
    #[test]
    fn pages_in_the_own_network_show_at_its_address() {
        let mut wifi = Wifi::default();
        wifi.apply(WifiCommand::On, Instant(0));
        run(&mut wifi, 1000);
        assert_eq!(wifi.served_at("192.168.178.48:80"), "192.168.178.48:80");
        wifi.apply(
            WifiCommand::Host {
                ssid: String::from("PaperPaw X4"),
                password: String::from("acdefhkmnprtwxy"),
            },
            Instant(1000),
        );
        run(&mut wifi, 2000);
        assert_eq!(wifi.served_at("192.168.178.48:80"), "192.168.4.1:80");
        assert_eq!(wifi.served_at("192.168.178.48:8081"), "192.168.4.1:8081");
    }

    #[test]
    fn joins_and_fails_like_a_device() {
        let mut wifi = Wifi::default();
        wifi.apply(WifiCommand::On, Instant(0));
        assert_eq!(run(&mut wifi, 1000), [WifiEvent::State(WifiState::On)]);
        wifi.apply(
            WifiCommand::Join {
                ssid: String::from("PaperPaw Test"),
                password: String::from("wrong"),
            },
            Instant(1000),
        );
        let events = run(&mut wifi, 3000);
        assert!(events.contains(&WifiEvent::Failed {
            ssid: String::from("PaperPaw Test"),
            reason: JoinFailure::Password
        }));
        wifi.apply(
            WifiCommand::Join {
                ssid: String::from("Cafe"),
                password: String::new(),
            },
            Instant(3000),
        );
        let events = run(&mut wifi, 5000);
        assert!(matches!(
            events.last(),
            Some(WifiEvent::State(WifiState::Online { .. }))
        ));
        wifi.apply(WifiCommand::Off, Instant(5000));
        assert_eq!(run(&mut wifi, 5000), [WifiEvent::State(WifiState::Off)]);
    }

    /// Online from the start, the workshop's network stays in reach: off
    /// and on, it is found and joined again.
    #[test]
    fn online_at_once_and_again_after_off_and_on() {
        let mut wifi = Wifi::default();
        wifi.online(Instant(0));
        assert!(matches!(
            run(&mut wifi, 0).as_slice(),
            [WifiEvent::State(WifiState::Online { ssid, .. })] if ssid == WORKSHOP_NETWORK
        ));
        wifi.apply(WifiCommand::Off, Instant(10));
        wifi.apply(WifiCommand::On, Instant(20));
        wifi.apply(WifiCommand::Scan, Instant(1000));
        let scanned = run(&mut wifi, 3000);
        assert!(scanned.iter().any(|e| matches!(
            e,
            WifiEvent::Scanned(networks) if networks[0].ssid == WORKSHOP_NETWORK
        )));
        wifi.apply(
            WifiCommand::Join {
                ssid: String::from(WORKSHOP_NETWORK),
                password: String::from(WORKSHOP_PASSWORD),
            },
            Instant(3000),
        );
        assert!(matches!(
            run(&mut wifi, 5000).last(),
            Some(WifiEvent::State(WifiState::Online { .. }))
        ));
        // Without it, the workshop's network is not there.
        assert!(
            Wifi::default()
                .networks()
                .iter()
                .all(|n| n.ssid != WORKSHOP_NETWORK)
        );
    }

    #[test]
    fn reports_come_in_the_order_of_time() {
        let mut wifi = Wifi::default();
        wifi.apply(WifiCommand::On, Instant(0));
        run(&mut wifi, 1000);
        // A scan takes 1.5 s; joining meanwhile says so at once.
        wifi.apply(WifiCommand::Scan, Instant(1000));
        wifi.apply(
            WifiCommand::Join {
                ssid: String::from("Cafe"),
                password: String::new(),
            },
            Instant(1100),
        );
        assert_eq!(
            run(&mut wifi, 1100),
            [WifiEvent::State(WifiState::Joining {
                ssid: String::from("Cafe")
            })]
        );
        // Joined after 1 s, before the scan ends.
        assert!(matches!(
            run(&mut wifi, 2100).as_slice(),
            [WifiEvent::State(WifiState::Online { .. })]
        ));
        assert!(matches!(
            run(&mut wifi, 2500).as_slice(),
            [WifiEvent::Scanned(_)]
        ));
    }
}
