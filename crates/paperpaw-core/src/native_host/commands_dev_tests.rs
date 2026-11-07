#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::{
    Kernel,
    abi7::commands_wire::{SecretText, commands_decode, commands_encode},
    io::WifiEvent,
    native_host::commands::CommandStage,
};
use paperpaw_native::host_glue::{ERR_CANCELLED, ERR_FULL, OK};
use paperpaw_native::metadata::Feature;
use paperpaw_platform::Instant;
use paperpaw_sdk::{SettingSpec, wifi::WifiState};

const HOST: &[Feature<'static>] = &[
    Feature {
        key: FEATURE_WIFI,
        revision: 1,
    },
    Feature {
        key: FEATURE_POWER,
        revision: 1,
    },
];
const WIFI_ONLY_HOST: &[Feature<'static>] = &[Feature {
    key: FEATURE_WIFI,
    revision: 1,
}];
const PASSWORD: &str = "hunter2-PASSWORD";

fn features(bits: u32) -> AdmittedFeatures {
    AdmittedFeatures::for_test(HOST, bits)
}
const WIFI: u32 = 1;
const POWER: u32 = 2;

fn kernel() -> Kernel {
    Kernel::builder(crate::tests::DEVICE)
        .wifi(true)
        .sleep(true)
        .power_off(true)
        .build()
        .unwrap()
}

fn join(request: u32, ssid: &str) -> WireCommand<'_> {
    WireCommand::WifiJoin {
        request,
        ssid,
        password: SecretText::new(PASSWORD),
        flags: 0,
    }
}

fn host(request: u32, password: &str) -> WireCommand<'_> {
    WireCommand::WifiHost {
        request,
        ssid: "ap",
        password: SecretText::new(password),
    }
}

fn stage_of<'a>(
    owner: &'a Id,
    kernel: &'a Kernel,
    env: &'a paperpaw_sdk::Env<'a>,
    bits: u32,
) -> CommandStage<'a> {
    CommandStage::new(owner, env.settings(), None, 16, 4096).with_dev(DevContext::new(
        features(bits),
        &kernel.radio_requests,
    ))
}

fn submit(stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]) -> i32 {
    let mut bytes = [0; 1024];
    let len = commands_encode(commands, &mut bytes).unwrap();
    stage.stage(&commands_decode(&bytes[..len]).unwrap())
}

fn dev_len(stage: &CommandStage<'_>) -> usize {
    stage.dev_len()
}

struct Clock(u64, Option<u64>);
impl InputClock for Clock {
    fn now_ms(&self) -> u64 {
        self.0
    }
    fn last_input_ms(&self) -> Option<u64> {
        self.1
    }
}
struct Stored;
impl StoredNetworks for Stored {
    fn has_password(&self, ssid: &str) -> bool {
        ssid == "home"
    }
}

/// Stage `commands` for `owner`, then drain them with `clock`.
fn run(kernel: &mut Kernel, owner: &Id, commands: &[WireCommand<'_>], clock: &Clock) -> i32 {
    let (status, output) = {
        let env = kernel.shared.env(Instant(0));
        let mut stage = stage_of(owner, kernel, &env, WIFI | POWER);
        let status = submit(&mut stage, commands);
        (status, stage.into_dev_output())
    };
    let (staged, redraw, _, batch) = output;
    let host = DevHost {
        clock,
        stored: &Stored,
    };
    kernel.drain_native_dev(owner, staged, redraw, batch, &host);
    status
}

fn wifi_calls(kernel: &mut Kernel) -> Vec<WifiCommand> {
    core::mem::take(&mut kernel.io.wifi_calls)
}

fn outcome(kernel: &Kernel, owner: &str) -> Option<(u32, Outcome)> {
    let table = kernel.radio_requests.borrow();
    table.next_outcome(owner).map(|d| (d.request, d.outcome))
}

#[test]
fn a_missing_feature_denies_the_whole_batch_valid_commands_included() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage = stage_of(&owner, &kernel, &env, WIFI);
    let batch = [WireCommand::WifiOn {}, WireCommand::Sleep {}];
    assert_eq!(submit(&mut stage, &batch), ERR_DENIED);
    assert_eq!(dev_len(&stage), 0, "not even the valid wifi_on is kept");
    assert!(kernel.radio_requests.borrow().is_empty());
    let mut stage = stage_of(&owner, &kernel, &env, 0);
    assert_eq!(submit(&mut stage, &[join(1, "net")]), ERR_DENIED);
    assert!(kernel.radio_requests.borrow().is_empty(), "nothing reserved");
    let mut stage = stage_of(&owner, &kernel, &env, WIFI | POWER);
    assert_eq!(submit(&mut stage, &batch), OK);
    assert_eq!(dev_len(&stage), 2);
}

#[test]
fn a_feature_the_host_does_not_advertise_is_unsupported() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let admitted = AdmittedFeatures::for_test(WIFI_ONLY_HOST, 1);
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096)
        .with_dev(DevContext::new(admitted, &kernel.radio_requests));
    assert_eq!(
        submit(&mut stage, &[WireCommand::WifiOn {}, WireCommand::Sleep {}]),
        ERR_UNSUPPORTED
    );
    assert_eq!(submit(&mut stage, &[WireCommand::WifiOn {}]), OK);
    // A stage the host wired nothing for refuses DEV commands as before.
    let mut bare = CommandStage::new(&owner, env.settings(), None, 16, 4096);
    assert_eq!(submit(&mut bare, &[WireCommand::Sleep {}]), ERR_UNSUPPORTED);
}

#[test]
fn install_and_bluetooth_commands_stay_unsupported() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage = stage_of(&owner, &kernel, &env, WIFI | POWER);
    let removal = WireCommand::RemovePlugin {
        request: 1,
        plugin: "x",
        flags: 0,
    };
    assert_eq!(submit(&mut stage, &[WireCommand::WifiOn {}, removal]), ERR_UNSUPPORTED);
    assert_eq!(dev_len(&stage), 0);
    let scan = WireCommand::BtScan { lasting_ms: 1 };
    assert_eq!(submit(&mut stage, &[scan]), ERR_UNSUPPORTED);
}

#[test]
fn batch_limits_sit_exactly_at_two_wifi_and_one_power_command() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage = stage_of(&owner, &kernel, &env, WIFI | POWER);
    let (on, off, scan) = (
        WireCommand::WifiOn {},
        WireCommand::WifiOff {},
        WireCommand::WifiScan {},
    );
    assert_eq!(submit(&mut stage, &[on, off, scan]), ERR_INVALID);
    assert_eq!(submit(&mut stage, &[on, off]), OK);
    let sleep = WireCommand::Sleep {};
    assert_eq!(
        submit(&mut stage, &[sleep, WireCommand::Restart {}]),
        ERR_INVALID
    );
    assert_eq!(submit(&mut stage, &[WireCommand::NetworkNone {}, sleep]), OK);
    // go_online and wear count: one wifi, one power, plus a third wifi.
    assert_eq!(
        submit(
            &mut stage,
            &[on, WireCommand::GoOnline { for_ms: 5 }, scan]
        ),
        ERR_INVALID
    );
    assert_eq!(dev_len(&stage), 4, "refused batches kept nothing");
}

#[test]
fn field_rules_hold_at_their_boundaries() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage = stage_of(&owner, &kernel, &env, WIFI | POWER);
    let mut ok = |commands: &[WireCommand<'_>]| submit(&mut stage, commands);
    assert_eq!(ok(&[join(0, "net")]), ERR_INVALID, "request 0");
    assert_eq!(ok(&[join(1, "")]), ERR_INVALID, "empty ssid");
    let (seven, eight) = ("p".repeat(7), "p".repeat(8));
    assert_eq!(ok(&[host(2, &seven)]), ERR_INVALID, "host password 7");
    assert_eq!(ok(&[host(2, &eight)]), OK, "host password 8");
    assert_eq!(ok(&[host(3, &"p".repeat(63))]), OK, "host password 63");
    let open = WireCommand::WifiJoin {
        request: 4,
        ssid: "open",
        password: SecretText::new(""),
        flags: 0,
    };
    assert_eq!(ok(&[open]), OK, "join with no password");
    let sixty_three = "p".repeat(63);
    let long = WireCommand::WifiJoin {
        request: 5,
        ssid: "n",
        password: SecretText::new(&sixty_three),
        flags: 1,
    };
    assert_eq!(ok(&[long]), OK, "join password 63");
    assert_eq!(ok(&[join(2, "again")]), ERR_INVALID, "request 2 is waiting");
    assert_eq!(ok(&[join(9, "a"), join(9, "b")]), ERR_INVALID, "twice in a batch");
    for (for_ms, status) in [(0, ERR_INVALID), (1, OK), (600_000, OK), (600_001, ERR_INVALID)] {
        assert_eq!(ok(&[WireCommand::GoOnline { for_ms }]), status, "go_online {for_ms}");
    }
    let wear = |mode| WireCommand::Wear { mode, flags: 1 };
    assert_eq!(ok(&[wear("core.saving")]), OK);
    assert_eq!(ok(&[wear("core.nothing")]), ERR_NOT_FOUND);
    assert_eq!(ok(&[wear("")]), ERR_NOT_FOUND);
    // An id that is a preset of another kind is not a power mode.
    assert_eq!(ok(&[wear("probe")]), ERR_NOT_FOUND);
}

#[test]
fn the_guard_alone_may_restart_or_power_off_and_sleep_stays_open() {
    let kernel = kernel();
    let env = kernel.shared.env(Instant(0));
    let stack = [paperpaw_sdk::StackEntry::new(
        Id::new_static("lock.screen"),
        None,
        false,
        paperpaw_sdk::ScreenKey::new(1),
    )
    .guarding(true)];
    let context = DevContext::new(features(WIFI | POWER), &kernel.radio_requests).guarded_by(&stack);
    let power = Id::new_static("power");
    let lock = Id::new_static("lock");
    let mut stage = CommandStage::new(&power, env.settings(), None, 16, 4096).with_dev(context);
    assert_eq!(submit(&mut stage, &[WireCommand::Restart {}]), ERR_DENIED);
    assert_eq!(submit(&mut stage, &[WireCommand::PowerOff {}]), ERR_DENIED);
    assert_eq!(submit(&mut stage, &[WireCommand::Sleep {}]), OK);
    let mut stage = CommandStage::new(&lock, env.settings(), None, 16, 4096).with_dev(context);
    assert_eq!(submit(&mut stage, &[WireCommand::Restart {}]), OK);
    let mut stage = CommandStage::new(&power, env.settings(), None, 16, 4096)
        .with_dev(DevContext::new(features(WIFI | POWER), &kernel.radio_requests));
    assert_eq!(submit(&mut stage, &[WireCommand::PowerOff {}]), OK, "no guard");
}

#[test]
fn the_fifth_request_is_full_and_the_first_four_are_still_delivered() {
    let mut kernel = kernel();
    let owners = ["a", "b"].map(Id::new_static);
    let clock = Clock(5_000, Some(5_000));
    for (index, request) in (1..=4).enumerate() {
        let owner = &owners[index % 2];
        assert_eq!(run(&mut kernel, owner, &[join(request, "net")], &clock), OK);
    }
    assert_eq!(kernel.radio_requests.borrow().len(), 4);
    let status = run(&mut kernel, &owners[0], &[join(5, "net")], &clock);
    assert_eq!(status, ERR_FULL);
    assert_eq!(kernel.radio_requests.borrow().len(), 4, "the 5th changed nothing");
    // Each of the four ends exactly once: three superseded, the last online.
    kernel.wifi_event(
        Instant(6_000),
        WifiEvent::State(WifiState::Online {
            ssid: "net".into(),
            address: String::new(),
            name: None,
        }),
    );
    for (owner, requests) in [("a", [1, 3]), ("b", [2, 4])] {
        for request in requests {
            let (got, outcome) = outcome(&kernel, owner).unwrap();
            assert_eq!(got, request);
            let superseded = request != 4;
            assert_eq!(outcome.reason, if superseded { 4 } else { 0 });
            assert!(kernel.radio_requests.borrow_mut().delivered(owner, request));
        }
    }
    assert!(kernel.radio_requests.borrow().is_empty(), "room released");
    assert_eq!(run(&mut kernel, &owners[0], &[join(5, "net")], &clock), OK);
}

#[test]
fn a_full_table_rolls_back_the_first_of_two_requests() {
    let mut kernel = kernel();
    let (a, b) = (Id::new_static("a"), Id::new_static("b"));
    let clock = Clock(0, Some(0));
    for request in 1..=3 {
        assert_eq!(run(&mut kernel, &b, &[join(request, "n")], &clock), OK);
    }
    let status = run(&mut kernel, &a, &[join(1, "x"), host(2, "password-ok")], &clock);
    assert_eq!(status, ERR_FULL);
    assert_eq!(kernel.radio_requests.borrow().len(), 3);
    assert!(!kernel.radio_requests.borrow().holds("a", 1));
}

#[test]
fn a_join_reaches_the_radio_and_ends_once_by_what_the_radio_says() {
    let mut kernel = kernel();
    let owner = Id::new_static("wifi");
    let clock = Clock(10_000, Some(9_000));
    assert_eq!(run(&mut kernel, &owner, &[join(3, "net")], &clock), OK);
    let calls = wifi_calls(&mut kernel);
    assert!(matches!(
        calls.as_slice(),
        [WifiCommand::Join { ssid, password }] if ssid == "net" && password == PASSWORD
    ));
    assert!(outcome(&kernel, "wifi").is_none(), "still joining");
    kernel.wifi_event(
        Instant(11_000),
        WifiEvent::Failed {
            ssid: "net".into(),
            reason: paperpaw_sdk::wifi::JoinFailure::Password,
        },
    );
    let (request, done) = outcome(&kernel, "wifi").unwrap();
    assert_eq!((request, done.status, done.reason), (3, ERR_DENIED, 2));
    kernel.wifi_event(Instant(12_000), WifiEvent::State(WifiState::Off));
    assert_eq!(outcome(&kernel, "wifi").unwrap().1, done, "never ends twice");
}

#[test]
fn a_newer_join_from_any_plugin_supersedes_exactly_once() {
    let mut kernel = kernel();
    let (a, b) = (Id::new_static("a"), Id::new_static("b"));
    let clock = Clock(1_000, Some(1_000));
    assert_eq!(run(&mut kernel, &a, &[join(1, "one")], &clock), OK);
    assert_eq!(run(&mut kernel, &b, &[join(1, "two")], &clock), OK);
    let (_, first) = outcome(&kernel, "a").unwrap();
    assert_eq!((first.status, first.reason), (ERR_CANCELLED, 4));
    assert!(outcome(&kernel, "b").is_none());
    // A host supersedes a join the same way, even in one batch.
    assert_eq!(run(&mut kernel, &b, &[join(2, "x"), host(3, "password-ok")], &clock), OK);
    let table = kernel.radio_requests.borrow();
    assert!(table.holds("b", 1) && table.holds("b", 2) && table.holds("b", 3));
    drop(table);
    assert_eq!(outcome(&kernel, "b").map(|(r, o)| (r, o.reason)), Some((1, 4)));
    kernel.radio_requests.borrow_mut().delivered("b", 1);
    assert_eq!(outcome(&kernel, "b").map(|(r, o)| (r, o.reason)), Some((2, 4)));
}

#[test]
fn unsupported_and_needs_user_end_at_once_without_touching_the_radio() {
    let owner = Id::new_static("wifi");
    let mut elsewhere = Kernel::builder(crate::tests::DEVICE).build().unwrap();
    let clock = Clock(0, Some(0));
    assert_eq!(run(&mut elsewhere, &owner, &[join(1, "net")], &clock), OK);
    let (request, done) = outcome(&elsewhere, "wifi").unwrap();
    assert_eq!((request, done.status, done.reason), (1, ERR_UNSUPPORTED, 5));
    assert!(wifi_calls(&mut elsewhere).is_empty());

    let mut kernel = kernel();
    let stale = Clock(10_000, Some(7_000));
    assert_eq!(run(&mut kernel, &owner, &[join(1, "net")], &stale), OK);
    let (_, done) = outcome(&kernel, "wifi").unwrap();
    assert_eq!((done.status, done.reason), (ERR_DENIED, 6));
    assert!(wifi_calls(&mut kernel).is_empty(), "nothing joined");
    // A refused request supersedes nothing: the waiting one stays waiting.
    let fresh = Clock(10_000, Some(10_000));
    assert_eq!(run(&mut kernel, &owner, &[join(2, "net")], &fresh), OK);
    assert_eq!(run(&mut kernel, &owner, &[join(3, "net")], &stale), OK);
    let table = kernel.radio_requests.borrow();
    assert_eq!(table.next_outcome("wifi").unwrap().request, 1);
    drop(table);
    kernel.radio_requests.borrow_mut().delivered("wifi", 1);
    assert_eq!(outcome(&kernel, "wifi").unwrap().0, 3, "2 still waits");
}

#[test]
fn the_input_window_for_join_ends_after_2000_ms_inclusive() {
    for (since, honoured) in [(1_999, true), (2_000, true), (2_001, false)] {
        let mut kernel = kernel();
        let owner = Id::new_static("wifi");
        let clock = Clock(50_000, Some(50_000 - since));
        assert_eq!(run(&mut kernel, &owner, &[join(1, "net")], &clock), OK);
        assert_eq!(wifi_calls(&mut kernel).len(), usize::from(honoured), "{since} ms");
        assert_eq!(outcome(&kernel, "wifi").is_some(), !honoured, "{since} ms");
    }
    // No input yet, and the clock before the input, are handled.
    let mut kernel = kernel();
    let owner = Id::new_static("wifi");
    assert_eq!(run(&mut kernel, &owner, &[join(1, "net")], &Clock(5, None)), OK);
    assert_eq!(outcome(&kernel, "wifi").unwrap().1.reason, 6);
}

#[test]
fn a_stored_password_or_the_network_manager_needs_no_input_but_a_host_does() {
    let stale = Clock(100_000, Some(0));
    let owner = Id::new_static("wifi");
    let mut kernel = kernel();
    assert_eq!(run(&mut kernel, &owner, &[join(1, "home")], &stale), OK);
    assert_eq!(wifi_calls(&mut kernel).len(), 1, "stored password");
    assert_eq!(run(&mut kernel, &owner, &[host(2, "password-ok")], &stale), OK);
    assert!(wifi_calls(&mut kernel).is_empty(), "a host always needs the user");
    let (request, done) = outcome(&kernel, "wifi").unwrap();
    assert_eq!((request, done.reason), (2, 6));
}

#[test]
fn the_network_manager_may_join_and_host_without_input() {
    let spec = SettingSpec::text(
        NETWORK_MANAGER,
        "manager",
        "wifi",
        paperpaw_sdk::builtin::groups::OTHER,
    );
    let stale = Clock(100_000, Some(0));
    for (name, honoured) in [("wifi", true), ("other", false)] {
        let mut kernel = kernel();
        kernel.shared.settings.define(spec.clone()).unwrap();
        let owner = Id::new_static(name);
        assert_eq!(run(&mut kernel, &owner, &[join(1, "net")], &stale), OK);
        assert_eq!(run(&mut kernel, &owner, &[host(2, "password-ok")], &stale), OK);
        assert_eq!(wifi_calls(&mut kernel).len(), if honoured { 2 } else { 0 });
    }
}

#[test]
fn go_online_is_honoured_only_within_the_window_else_ok_and_logged() {
    for (since, honoured) in [(1_999, true), (2_000, true), (2_001, false)] {
        let mut kernel = kernel();
        let owner = Id::new_static("probe");
        let clock = Clock(9_000, Some(9_000 - since));
        let go = WireCommand::GoOnline { for_ms: 60_000 };
        assert_eq!(run(&mut kernel, &owner, &[go], &clock), OK, "{since} ms");
        assert_eq!(kernel.works.online_until.is_some(), honoured, "{since} ms");
        let warnings = kernel.take_warnings();
        assert_eq!(
            warnings.iter().any(|w| w.message.contains("go_online ignored")),
            !honoured
        );
        assert!(warnings.iter().all(|w| !w.message.contains("probe-ssid")));
    }
}

#[test]
fn power_commands_apply_after_the_commands_before_them_and_in_order() {
    let mut kernel = kernel();
    let spec = SettingSpec::switch("probe.flag", "flag", false, paperpaw_sdk::builtin::groups::OTHER);
    kernel.shared.settings.define(spec).unwrap();
    let owner = Id::new_static("probe");
    let set = |on| WireCommand::SetSetting {
        key: "probe.flag",
        value: crate::abi7::settings_wire::SettingValueRef::Bool(on),
    };
    let (status, output) = {
        let env = kernel.shared.env(Instant(0));
        let mut stage = stage_of(&owner, &kernel, &env, WIFI | POWER);
        let first = submit(&mut stage, &[WireCommand::WifiOn {}, set(true), WireCommand::Sleep {}]);
        let second = submit(&mut stage, &[set(false)]);
        assert_eq!((first, second), (OK, OK));
        assert_eq!(stage.staged().len(), 2);
        (OK, stage.into_dev_output())
    };
    let (staged, redraw, _, batch) = output;
    let at: Vec<usize> = batch.items.iter().map(|item| item.at).collect();
    assert_eq!(at, [0, 1], "wifi_on before both settings, sleep after the first");
    let clock = Clock(0, None);
    let host = DevHost { clock: &clock, stored: &NoStoredNetworks };
    kernel.drain_native_dev(&owner, staged, redraw, batch, &host);
    assert_eq!(status, OK);
    assert!(kernel.sleep, "the shell sleeps once the frame is shown");
    assert_eq!(wifi_calls(&mut kernel), [WifiCommand::On]);
    assert_eq!(kernel.shared.settings.global("probe.flag"), Some(&SettingValue::Bool(false)));
}

#[test]
fn power_ops_set_the_kernel_flags_and_wear_chooses_the_preset() {
    let mut kernel = kernel();
    let owner = Id::new_static("power");
    let clock = Clock(0, None);
    let wear = WireCommand::Wear {
        mode: "core.saving",
        flags: 1,
    };
    assert_eq!(run(&mut kernel, &owner, &[wear], &clock), OK);
    assert_eq!(kernel.shared.power.mode.as_str(), "core.saving");
    assert!(kernel.shared.power.automatic);
    for (command, flag) in [
        (WireCommand::Restart {}, 0),
        (WireCommand::PowerOff {}, 1),
        (WireCommand::Sleep {}, 2),
    ] {
        assert_eq!(run(&mut kernel, &owner, &[command], &clock), OK);
        assert_eq!(
            [kernel.restart, kernel.power_off, kernel.sleep][flag],
            true
        );
    }
    let mut elsewhere = Kernel::builder(crate::tests::DEVICE).build().unwrap();
    assert_eq!(run(&mut elsewhere, &owner, &[WireCommand::Sleep {}], &clock), OK);
    assert!(!elsewhere.sleep, "ignored where the device cannot");
}

#[test]
fn a_dropped_stage_gives_its_reservations_back_and_a_stop_releases_the_rest() {
    let mut kernel = kernel();
    let owner = Id::new_static("wifi");
    {
        let env = kernel.shared.env(Instant(0));
        let mut stage = stage_of(&owner, &kernel, &env, WIFI);
        assert_eq!(submit(&mut stage, &[join(1, "net")]), OK);
        assert_eq!(kernel.radio_requests.borrow().len(), 1);
        let _ = stage.into_commands();
    }
    assert!(kernel.radio_requests.borrow().is_empty(), "never drained: given back");
    let clock = Clock(0, Some(0));
    assert_eq!(run(&mut kernel, &owner, &[join(1, "net")], &clock), OK);
    kernel.wifi_event(Instant(1), WifiEvent::State(WifiState::Off));
    assert!(outcome(&kernel, "wifi").is_some());
    kernel.radio_sender_stopped("wifi");
    assert!(kernel.radio_requests.borrow().is_empty());
    assert!(outcome(&kernel, "wifi").is_none(), "released with no delivery");
}

#[test]
fn leave_and_off_cancel_a_waiting_join_through_the_drain() {
    let mut kernel = kernel();
    let owner = Id::new_static("wifi");
    let clock = Clock(0, Some(0));
    assert_eq!(run(&mut kernel, &owner, &[join(1, "net")], &clock), OK);
    assert_eq!(run(&mut kernel, &owner, &[WireCommand::WifiLeave {}], &clock), OK);
    assert_eq!(outcome(&kernel, "wifi").unwrap().1.reason, 4);
    assert!(matches!(wifi_calls(&mut kernel).last(), Some(WifiCommand::Leave)));
}

#[test]
fn debug_output_never_shows_a_password() {
    let mut kernel = kernel();
    let owner = Id::new_static("wifi");
    let (stage_text, batch_text, table_text) = {
        let env = kernel.shared.env(Instant(0));
        let mut stage = stage_of(&owner, &kernel, &env, WIFI);
        assert_eq!(submit(&mut stage, &[join(1, "net"), host(2, PASSWORD)]), OK);
        let stage_text = alloc::format!("{stage:?}");
        let dev = stage.dev.as_ref().unwrap();
        let batch_text = alloc::format!("{dev:?} {:?}", dev.items);
        let table_text = alloc::format!("{:?}", kernel.radio_requests.borrow());
        (stage_text, batch_text, table_text)
    };
    let clock = Clock(0, Some(0));
    assert_eq!(run(&mut kernel, &owner, &[join(7, "net")], &clock), OK);
    let call = alloc::format!("{:?}", wifi_calls(&mut kernel));
    let secret = alloc::format!("{:?}", Secret::copy(PASSWORD).unwrap());
    let table = alloc::format!("{:?}", kernel.radio_requests.borrow());
    for text in [stage_text, batch_text, table_text, call, secret, table] {
        assert!(!text.contains(PASSWORD), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
    }
    let wire = alloc::format!("{:?}", join(1, "net"));
    assert!(!wire.contains(PASSWORD));
}

#[test]
fn a_secret_is_wiped_before_it_is_freed() {
    let mut secret = Secret::copy(PASSWORD).unwrap();
    assert_eq!(secret.to_text().unwrap(), PASSWORD);
    secret.wipe();
    assert_eq!(secret.0.len(), PASSWORD.len());
    assert!(secret.0.iter().all(|byte| *byte == 0));
}
