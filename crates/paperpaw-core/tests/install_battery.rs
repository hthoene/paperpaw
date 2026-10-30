//! Firmware is installed only with battery enough: below
//! `INSTALL_BATTERY` percent and not charging, the kernel refuses an
//! install of any kind, whoever asks for it, and the shell is not asked;
//! charging, or without a battery reading, it goes through.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::sdk::builtin::events;
use paperpaw_core::sdk::firmware::{
    CHIP_ESP32_C3, Firmware, FirmwareCommand, FirmwareState, INSTALL_BATTERY, InstallFailure,
};
use paperpaw_core::sdk::{
    ActionSpec, Command, Id, Manifest, MemoryStorage, Plugin, RegisterError, Registrar, Version,
};
use paperpaw_core::{Io, Kernel};
use paperpaw_platform::{
    Battery, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    SystemStatus,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test",
    DisplayInfo::new(120, 160, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

const GO: Id = Id::new_static("asker.go");

/// A plugin that sends what the test queues, and notes the firmware's
/// state as it hears of it.
struct Asker {
    queue: Rc<RefCell<Vec<Command>>>,
    states: Rc<RefCell<Vec<FirmwareState>>>,
}

impl Plugin for Asker {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("asker"), "Asker", Version::new(1, 0, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let queue = self.queue.clone();
        r.add_action(ActionSpec::new(GO, "go").handler(move |_, _, out| {
            out.extend(queue.borrow_mut().drain(..));
        }))?;
        let states = self.states.clone();
        r.on(events::FIRMWARE_CHANGED, move |env, (), _| {
            if let Some(firmware) = env.firmware() {
                states.borrow_mut().push(firmware.state.clone());
            }
        })?;
        Ok(())
    }
}

struct Test {
    kernel: Kernel,
    queue: Rc<RefCell<Vec<Command>>>,
    states: Rc<RefCell<Vec<FirmwareState>>>,
}

fn kernel() -> Test {
    let queue = Rc::<RefCell<Vec<Command>>>::default();
    let states = Rc::<RefCell<Vec<FirmwareState>>>::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .firmware(Firmware::new("0.5.0", "paperpaw-xteink", CHIP_ESP32_C3))
        .plugin(Asker {
            queue: queue.clone(),
            states: states.clone(),
        })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    Test {
        kernel,
        queue,
        states,
    }
}

fn battery(test: &mut Test, percent: u8, charging: bool) {
    test.kernel.set_status(
        Instant(10),
        SystemStatus::default().with_battery(Some(Battery::new(percent, charging, None))),
    );
}

/// The firmware commands the shell is asked for, after a plugin sent
/// `command`.
fn asked(test: &mut Test, command: FirmwareCommand) -> Vec<FirmwareCommand> {
    test.queue.borrow_mut().push(Command::Firmware(command));
    test.kernel.run_action(Instant(20), GO);
    test.kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Firmware(command) => Some(command),
            _ => None,
        })
        .collect()
}

fn installs() -> [FirmwareCommand; 2] {
    [
        FirmwareCommand::install("updates/a.bin", None, false),
        FirmwareCommand::install_foreign("crosspoint.bin"),
    ]
}

#[test]
fn below_the_least_charge_no_install_reaches_the_shell() {
    for install in installs() {
        let mut test = kernel();
        battery(&mut test, INSTALL_BATTERY - 1, false);
        assert!(asked(&mut test, install).is_empty());
        assert_eq!(
            test.states.borrow().last(),
            Some(&FirmwareState::Failed(InstallFailure::LowBattery))
        );
    }
}

#[test]
fn charging_or_charged_enough_or_unmeasured_it_installs() {
    for (status, label) in [
        (Some((INSTALL_BATTERY - 1, true)), "charging"),
        (Some((INSTALL_BATTERY, false)), "charged enough"),
        (None, "no battery reading"),
    ] {
        for install in installs() {
            let mut test = kernel();
            if let Some((percent, charging)) = status {
                battery(&mut test, percent, charging);
            }
            assert_eq!(asked(&mut test, install.clone()), [install], "{label}");
        }
    }
}

#[test]
fn a_restart_is_never_held() {
    let mut test = kernel();
    battery(&mut test, 5, false);
    assert_eq!(
        asked(&mut test, FirmwareCommand::Restart),
        [FirmwareCommand::Restart]
    );
}
