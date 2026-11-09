//! `core.device` ops 8, 10, 11 and 12 answered from the kernel's own models
//! (`kernel_models.rs`), through a frame that has the kernel's `Env`.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::tests::{Fake, input_of, records, schema_op};
use super::*;
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::host_services::{Caller, HostServices};
use crate::native_host::registrations::Record;
use alloc::{rc::Rc, string::ToString, vec, vec::Vec};
use paperpaw_platform::Instant;
use paperpaw_sdk::Id;
use paperpaw_sdk::bluetooth::{Bluetooth, Bonded, DeviceId, DeviceKind, RadioState};
use paperpaw_sdk::wifi::WifiState;

/// A frame with the kernel's `Env`, as the dispatcher's has.
struct KernelView<'a> {
    env: &'a paperpaw_sdk::Env<'a>,
    hosts: HostServices,
    /// The plugin the service dispatch says called.
    caller: &'a str,
}

impl Frame for KernelView<'_> {
    fn settings(&self) -> &paperpaw_sdk::Settings {
        self.env.settings()
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        self.env.i18n()
    }
    fn kernel_env(&self) -> Option<&paperpaw_sdk::Env<'_>> {
        Some(self.env)
    }
}

impl KernelView<'_> {
    fn run(&self, id: u32) -> Result<Vec<u8>, i32> {
        let op: &ServiceOp = schema_op(id);
        let caller = Caller {
            owner: 5,
            plugin: self.caller,
            features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
            hosts: &self.hosts,
        };
        let mut scratch = vec![0_u8; op.max_reply];
        let length = backend(self, &caller, op, &input_of(id), &mut scratch)?;
        Ok(scratch[..length].to_vec())
    }
}

fn kernel(wifi: bool, bluetooth: u8) -> crate::Kernel {
    crate::Kernel::builder(crate::tests::DEVICE)
        .wifi(wifi)
        .bluetooth(bluetooth)
        .build()
        .unwrap()
}

/// Reads op `id` as plugin `plugin`, the kernel's `Env` built afresh.
fn read(kernel: &crate::Kernel, host: Fake, plugin: &str, id: u32) -> Result<Vec<u8>, i32> {
    read_nested(kernel, host, plugin, plugin, id)
}

/// Reads op `id` as the service caller `caller` while the kernel's tracker names
/// `running` (a nested call: the tracker is the top-level callback's plugin).
fn read_nested(
    kernel: &crate::Kernel,
    host: Fake,
    running: &str,
    caller: &str,
    id: u32,
) -> Result<Vec<u8>, i32> {
    let _running = kernel.shared.meter.enter(running);
    let env = kernel.shared.env(Instant(0));
    let hosts = HostServices::new();
    hosts.wire_device(Rc::new(host)).unwrap();
    KernelView {
        env: &env,
        hosts,
        caller,
    }
    .run(id)
}

fn wifi_state(kernel: &crate::Kernel) -> u32 {
    let reply = read(kernel, Fake::default(), "reader", 8).unwrap();
    let [Record::ServiceDevWifi(wifi)] = records(&reply)[..] else {
        panic!("op 8")
    };
    wifi.state
}

/// What an earlier plugin's callback did to the radio, the kernel applied
/// before the next plugin's call: that one reads the new state. The shell's
/// host (here a fake with no Wi-Fi at all) is not asked.
#[test]
fn a_later_plugin_reads_the_radio_state_an_earlier_one_changed() {
    let mut kernel = kernel(true, 4);
    assert_eq!(wifi_state(&kernel), 0);
    kernel.wifi_event(
        Instant(1),
        crate::io::WifiEvent::State(WifiState::Online {
            ssid: "home".into(),
            address: "192.168.1.20".into(),
            name: Some("paperpaw.local".into()),
        }),
    );
    let reply = read(&kernel, Fake::default(), "reader", 8).unwrap();
    let [Record::ServiceDevWifi(wifi)] = records(&reply)[..] else {
        panic!("op 8")
    };
    assert_eq!(
        (wifi.state, wifi.ssid, wifi.address, wifi.host_name),
        (3, "home", "192.168.1.20", "paperpaw.local")
    );
    kernel.wifi_event(Instant(2), crate::io::WifiEvent::State(WifiState::Off));
    assert_eq!(wifi_state(&kernel), 0);

    // Bluetooth: op 10 follows the radio.
    kernel.shared.bluetooth = Some(Bluetooth::new(4));
    let radio = |kernel: &crate::Kernel| {
        let reply = read(kernel, Fake::default(), "reader", 10).unwrap();
        let [Record::ServiceDevBluetooth(bt)] = records(&reply)[..] else {
            panic!("op 10")
        };
        (bt.radio, bt.max_links)
    };
    assert_eq!(radio(&kernel), (0, 4));
    kernel.shared.bluetooth.as_mut().unwrap().state = RadioState::On;
    assert_eq!(radio(&kernel), (1, 4));
    kernel.shared.bluetooth.as_mut().unwrap().state = RadioState::Off;
    assert_eq!(radio(&kernel), (0, 4));
}

/// The kernel decides what exists: a host that says it has Wi-Fi and
/// Bluetooth is not believed where the kernel has no model of them.
#[test]
fn without_a_kernel_model_the_part_is_not_here() {
    let kernel = kernel(false, 0);
    for id in [8, 9, 10, 11] {
        assert_eq!(
            read(&kernel, Fake::full(), "reader", id),
            Err(ERR_NOT_FOUND)
        );
    }
}

fn bonded(count: u32, given: &[&str]) -> Vec<Bonded> {
    (1..=count)
        .map(|n| {
            Bonded::new(
                DeviceId::new(n),
                alloc::format!("dev{n}"),
                DeviceKind::Sensor,
            )
            .given_to(given.iter().map(|g| Id::parse(g).unwrap()).collect())
        })
        .collect()
}

/// The bonded list, empty and full (16), for an ordinary plugin (only what is
/// given to it, without `given`) and for the Bluetooth manager (all, with it).
#[test]
fn the_bonded_list_is_the_plugins_own_and_the_managers_is_all_of_it() {
    let mut kernel = kernel(true, 4);
    let mut bluetooth = Bluetooth::new(4);
    kernel.shared.bluetooth = Some(bluetooth.clone());
    let list = |kernel: &crate::Kernel, plugin: &str| {
        let reply = read(kernel, Fake::default(), plugin, 11).unwrap();
        records(&reply)
            .into_iter()
            .filter_map(|row| match row {
                Record::ServiceDevBonded(row) => Some((row.device, row.given.to_string())),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(list(&kernel, "reader").is_empty());
    // Sixteen bonded devices, all given to `reader`; the manager is `ble`.
    bluetooth.set_bonded(bonded(16, &["reader"]));
    bluetooth.set_manager(Some(Id::new_static("ble")));
    kernel.shared.bluetooth = Some(bluetooth.clone());
    let own = list(&kernel, "reader");
    assert_eq!(own.len(), 16);
    assert!(own.iter().all(|(_, given)| given.is_empty()));
    let all = list(&kernel, "ble");
    assert_eq!(all.len(), 16);
    assert!(all.iter().all(|(_, given)| given == "reader"));
    // Another plugin sees none of them.
    assert!(list(&kernel, "other").is_empty());
    // The manager: the list a plugin reads changes with the kernel's.
    let mut fewer = bluetooth;
    fewer.set_bonded(bonded(3, &["reader"]));
    kernel.shared.bluetooth = Some(fewer);
    assert_eq!(list(&kernel, "reader").len(), 3);
    assert_eq!(list(&kernel, "ble").len(), 3);
}

/// A page read across a change of the list is `ERR_STATE`, as with the shell's
/// own lists: the generation follows the bonded devices the plugin may use.
#[test]
fn the_bonded_generation_follows_the_list() {
    let mut kernel = kernel(true, 4);
    let mut bluetooth = Bluetooth::new(4);
    bluetooth.set_bonded(bonded(2, &["reader"]));
    kernel.shared.bluetooth = Some(bluetooth.clone());
    let generation = |kernel: &crate::Kernel| {
        let reply = read(kernel, Fake::default(), "reader", 11).unwrap();
        match records(&reply).last() {
            Some(Record::ServiceEnd(end)) => end.generation,
            other => panic!("{other:?}"),
        }
    };
    let before = generation(&kernel);
    assert_eq!(generation(&kernel), before);
    bluetooth.set_bonded(bonded(3, &["reader"]));
    kernel.shared.bluetooth = Some(bluetooth);
    assert_ne!(generation(&kernel), before);
}

/// Op 12 is the kernel's power level and mode.
#[test]
fn power_is_the_kernels_level_and_mode() {
    let mut kernel = kernel(false, 0);
    let level = |kernel: &crate::Kernel| {
        let reply = read(kernel, Fake::default(), "reader", 12).unwrap();
        let [Record::ServiceDevPower(power)] = records(&reply)[..] else {
            panic!("op 12")
        };
        power.level
    };
    assert_eq!(level(&kernel), 0);
    kernel.shared.power.level = paperpaw_sdk::power::PowerLevel::Saving;
    assert_eq!(level(&kernel), 1);
}

/// A plugin called inside the Bluetooth manager's callback (a stage subscriber,
/// a compose child) is asked as itself: the service's caller, not the kernel's
/// tracker of the running plugin, decides what it sees.
#[test]
fn a_nested_call_sees_only_its_own_devices() {
    let mut kernel = kernel(true, 4);
    let mut bluetooth = Bluetooth::new(4);
    let mut devices = bonded(2, &["reader"]);
    devices.push(
        Bonded::new(DeviceId::new(9), "secret-sensor", DeviceKind::Sensor)
            .given_to(vec![Id::parse("other").unwrap()]),
    );
    bluetooth.set_bonded(devices);
    bluetooth.set_manager(Some(Id::new_static("ble")));
    kernel.shared.bluetooth = Some(bluetooth);
    let rows = |reply: Vec<u8>| {
        records(&reply)
            .into_iter()
            .filter_map(|row| match row {
                Record::ServiceDevBonded(row) => Some((row.device, row.given.to_string())),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // The manager's callback is running, `reader` is called inside it.
    let nested = rows(read_nested(&kernel, Fake::default(), "ble", "reader", 11).unwrap());
    assert_eq!(nested.iter().map(|row| row.0).collect::<Vec<_>>(), [1, 2]);
    assert!(nested.iter().all(|(_, given)| given.is_empty()));
    // And the manager called inside another plugin's callback is still the
    // manager.
    let manager = rows(read_nested(&kernel, Fake::default(), "reader", "ble", 11).unwrap());
    assert_eq!(manager.len(), 3);
    assert!(manager.iter().any(|(_, given)| given == "reader"));
}

/// The paging generation is never 0, whatever the shell's is.
#[test]
fn the_bonded_generation_is_never_zero() {
    let mut kernel = kernel(true, 4);
    let mut bluetooth = Bluetooth::new(4);
    bluetooth.set_bonded(bonded(2, &["reader"]));
    kernel.shared.bluetooth = Some(bluetooth);
    let generation = |host: Fake| {
        let reply = read(&kernel, host, "reader", 11).unwrap();
        match records(&reply).last() {
            Some(Record::ServiceEnd(end)) => end.generation,
            other => panic!("{other:?}"),
        }
    };
    let hash = generation(Fake::default());
    // A shell generation that makes the sum wrap to 0.
    let zero = Fake::default().with_generation(0_u32.wrapping_sub(hash));
    assert_eq!(generation(zero), 1);
}

/// A shell that advertises a radio the kernel has no model of is found out by
/// the check on the combined host, which the shell's own host check cannot do.
#[test]
fn the_caps_check_runs_on_the_combined_host() {
    let host = Fake::full();
    // Kernel with Wi-Fi and Bluetooth: the full fake agrees with it.
    let mut both = kernel(true, 4);
    both.shared.bluetooth = Some(Bluetooth::new(4));
    assert_eq!(both.check_device_host(&host), Ok(()));
    // Without Wi-Fi the kernel answers "not found" where the shell's caps say
    // Wi-Fi.
    let mut no_wifi = kernel(false, 4);
    no_wifi.shared.bluetooth = Some(Bluetooth::new(4));
    assert!(no_wifi.check_device_host(&Fake::full()).is_err());
    let no_bluetooth = kernel(true, 0);
    assert!(no_bluetooth.check_device_host(&Fake::full()).is_err());
}
