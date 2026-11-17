//! Network work and Wi-Fi as plugins see them: work waits for Wi-Fi and
//! goes on once online, fails when no network comes; scans and failures
//! reach plugins; Wi-Fi commands are ignored where the system manages it.
use super::*;

#[test]
fn network_work_waits_for_wifi_and_goes_on_once_online() {
    use crate::{WifiCommand, WifiEvent, WifiState};
    let log = log();
    let mut kernel = wifi_kernel(&log);
    assert!(kernel.shared.place(Instant(0)).caps().network);
    assert_eq!(
        kernel.shared.place(Instant(0)).wifi().unwrap().state,
        WifiState::Off
    );
    get(&mut kernel);
    // Held while offline; plugins hear it once, and ask for the radio.
    assert!(kernel.take_requests().is_empty());
    assert_eq!(log.borrow().as_slice(), ["core.network-wanted "]);
    assert_eq!(kernel.take_wifi_calls(), [WifiCommand::On]);
    assert!(kernel.shared.wifi.as_ref().unwrap().wanted);
    get(&mut kernel);
    assert_eq!(log.borrow().len(), 1);
    // Online: both go to the shell.
    let online = WifiState::Online {
        ssid: String::from("Home"),
        address: String::from("10.0.0.2"),
        name: None,
    };
    kernel.wifi_event(Instant(10), WifiEvent::State(online));
    let calls = kernel.take_requests();
    assert_eq!(calls.len(), 2);
    assert!(!kernel.shared.wifi.as_ref().unwrap().wanted);
    // Answered: nothing uses the network any more.
    for call in calls {
        kernel.http_response(
            Instant(20),
            crate::HttpResponse {
                id: call.id,
                status: 200,
                ..Default::default()
            },
        );
    }
    assert_eq!(
        log.borrow().as_slice(),
        [
            "core.network-wanted ",
            "core.wifi-changed ",
            "net.reply 200",
            "net.reply 200",
            "core.network-idle "
        ]
    );
}

#[test]
fn network_work_fails_when_no_network_comes() {
    let log = log();
    let mut kernel = wifi_kernel(&log);
    get(&mut kernel);
    let deadline = Instant(paperpaw_sdk::wifi::NETWORK_WAIT_MS);
    assert_eq!(kernel.finish().wake_at(), Some(deadline));
    kernel.tick(Instant(1000));
    assert_eq!(log.borrow().len(), 1);
    kernel.tick(deadline);
    // Failed as without a network; then nothing uses it (the radio asked
    // for may go off again).
    assert_eq!(
        log.borrow().as_slice(),
        ["core.network-wanted ", "net.reply 0", "core.network-idle "]
    );
    assert!(
        !kernel.shared.wifi.as_ref().unwrap().wanted,
        "the radio may go off"
    );
    assert!(kernel.take_requests().is_empty());
    assert_eq!(kernel.finish().wake_at(), None);
}

#[test]
fn scans_and_failures_reach_plugins() {
    use crate::{JoinFailure, WifiEvent, WifiNetwork};
    let log = log();
    let mut kernel = wifi_kernel(&log);
    let net = |ssid: &str, signal| WifiNetwork::new(String::from(ssid), signal, true);
    kernel.wifi_event(
        Instant(1),
        WifiEvent::Scanned(alloc::vec![
            net("Weak", -80),
            net("", -40),
            net("Home", -50),
            net("Weak", -60)
        ]),
    );
    // Strongest first, each name once, hidden ones left out.
    let wifi = kernel.shared.wifi.as_ref().unwrap();
    let names: Vec<_> = wifi
        .networks
        .iter()
        .map(|n| (n.ssid.as_str(), n.signal))
        .collect();
    assert_eq!(names, [("Home", -50), ("Weak", -60)]);
    kernel.wifi_event(
        Instant(2),
        WifiEvent::Failed {
            ssid: String::from("Home"),
            reason: JoinFailure::Password,
        },
    );
    assert_eq!(
        log.borrow().as_slice(),
        ["core.wifi-scanned ", "core.wifi-failed password"]
    );
}

#[test]
fn wifi_commands_are_ignored_where_the_system_manages_it() {
    let mut kernel = Kernel::builder(DEVICE).network(true).build().unwrap();
    kernel.start(Instant(0));
    assert!(kernel.shared.place(Instant(0)).wifi().is_none());
    kernel
        .output
        .commands
        .push(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::On));
    kernel.finish();
    assert!(kernel.take_wifi_calls().is_empty());
    // Requests go to the shell at once.
    kernel.request(paperpaw_sdk::HttpRequest::new(
        "GET",
        "http://example.org/",
        Id::new_static("net.reply"),
    ));
    assert_eq!(kernel.take_requests().len(), 1);
}
