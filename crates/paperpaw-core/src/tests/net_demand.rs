//! What `core.net`, `core.sockets` and the work scheduler want of the
//! network, as the network manager and the shell hear it (`device.md` 3):
//! `event.network_wanted` once where no network is up, `event.network_idle`
//! after the last use, the flags of `Env::wifi`, and a one-shot wish that
//! lapses.
use super::net_policy_wiring::booting_with;
use super::*;
use crate::native_host::net_pool::{DEMAND_NET, DEMAND_SOCK, WORK_WISH_MS};
use crate::{WifiEvent, WifiState};
use paperpaw_platform::Storage as _;
use paperpaw_sdk::MemoryStorage;

fn count(log: &Log, what: &str) -> usize {
    log.borrow().iter().filter(|e| **e == what).count()
}

fn flags(kernel: &Kernel) -> (bool, bool) {
    let wifi = kernel.shared.wifi.as_ref().unwrap();
    (wifi.wanted, wifi.busy)
}

fn started() -> (Kernel, Log) {
    let (mut kernel, _, log) = booting_with(MemoryStorage::new(), true);
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    (kernel, log)
}

fn online(kernel: &mut Kernel, at: u64) {
    kernel.wifi_event(
        Instant(at),
        WifiEvent::State(WifiState::Online {
            ssid: String::from("Home"),
            address: String::from("10.0.0.2"),
            name: None,
        }),
    );
}

fn demand(kernel: &Kernel, bit: u8, on: bool) {
    let (pool, _) = kernel.shared.native_instances.net_demand().unwrap();
    pool.demand(bit, on);
}

#[test]
fn nothing_wants_nothing_is_told() {
    let (mut kernel, log) = started();
    kernel.tick(Instant(1_000));
    assert_eq!(flags(&kernel), (false, false));
    assert!(log.borrow().is_empty());
}

#[test]
fn a_live_service_without_a_network_is_wanted_once_and_idle_after_its_last_use() {
    let (mut kernel, log) = started();
    demand(&kernel, DEMAND_SOCK, true);
    kernel.tick(Instant(1_000));
    assert_eq!(count(&log, "wk wanted"), 1);
    assert_eq!(flags(&kernel), (true, true));
    kernel.tick(Instant(2_000));
    assert_eq!(count(&log, "wk wanted"), 1, "told once for the wait");
    // A network comes: no longer waiting, still in use.
    online(&mut kernel, 3_000);
    assert_eq!(flags(&kernel), (false, true));
    assert_eq!(count(&log, "wk idle"), 0);
    // A request joins, the socket goes: still in use.
    demand(&kernel, DEMAND_NET, true);
    demand(&kernel, DEMAND_SOCK, false);
    kernel.tick(Instant(4_000));
    assert_eq!(count(&log, "wk idle"), 0);
    // The last one goes: idle, once.
    demand(&kernel, DEMAND_NET, false);
    kernel.tick(Instant(5_000));
    assert_eq!(count(&log, "wk idle"), 1);
    assert_eq!(flags(&kernel), (false, false));
    kernel.tick(Instant(6_000));
    assert_eq!(count(&log, "wk idle"), 1);
}

#[test]
fn a_wish_of_work_is_wanted_once_stands_for_a_while_and_lapses() {
    let (mut kernel, log) = started();
    kernel.now = Instant(10_000);
    kernel.native_work_wish();
    kernel.native_work_wish();
    kernel.finish();
    assert_eq!(count(&log, "wk wanted"), 1);
    // Wished for, not in use: the radio is not held on for it.
    assert_eq!(flags(&kernel), (true, false));
    assert_eq!(
        kernel.service_due(),
        Some(Instant(10_000 + WORK_WISH_MS)),
        "the kernel wakes when it lapses, and not before"
    );
    kernel.tick(Instant(10_000 + WORK_WISH_MS - 1));
    assert_eq!(flags(&kernel), (true, false));
    kernel.tick(Instant(10_000 + WORK_WISH_MS));
    assert_eq!(flags(&kernel), (false, false));
    assert_eq!(count(&log, "wk idle"), 0, "nothing was in use");
    assert_eq!(kernel.service_due(), None);
    // A wish again is a new one.
    kernel.now = Instant(100_000);
    kernel.native_work_wish();
    kernel.finish();
    assert_eq!(count(&log, "wk wanted"), 2);
}

#[test]
fn a_network_that_comes_ends_the_wish_of_work() {
    let (mut kernel, log) = started();
    kernel.now = Instant(1_000);
    kernel.native_work_wish();
    kernel.finish();
    assert_eq!(flags(&kernel), (true, false));
    online(&mut kernel, 2_000);
    assert_eq!(flags(&kernel), (false, false));
    assert_eq!(count(&log, "wk wanted"), 1);
    // And on a network that is up, a wish asks for nothing.
    kernel.now = Instant(3_000);
    kernel.native_work_wish();
    kernel.finish();
    assert_eq!(count(&log, "wk wanted"), 1);
}

#[test]
fn a_service_that_stays_live_keeps_the_demand_after_the_wish_lapses() {
    let (mut kernel, log) = started();
    kernel.now = Instant(0);
    kernel.native_work_wish();
    demand(&kernel, DEMAND_NET, true);
    kernel.finish();
    kernel.tick(Instant(WORK_WISH_MS));
    assert_eq!(flags(&kernel), (true, true), "the request still waits");
    // The wait the manager was told of ended at the same moment as the
    // wish; a request that still waits is told again once, not in a loop.
    let told = count(&log, "wk wanted");
    kernel.tick(Instant(WORK_WISH_MS + 1_000));
    assert_eq!(count(&log, "wk wanted"), told);
}
