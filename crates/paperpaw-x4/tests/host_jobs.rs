//! The kernel as the X4 family's firmware builds it, for plugin jobs and the
//! firmware's update (`paperpaw_x4::host_jobs`, which `shells/xteink` and the
//! emulator's profiles of these devices both call): what makes a kernel take
//! them, and what plugin jobs check a plugin against.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_sdk::{DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat};
use paperpaw_x4::host_jobs::{Family, NoFirmware, offer_plugin_jobs, plugin_host};
use paperpaw_x4::native_offer::{Wired, card_host};

const FAMILIES: [Family; 2] = [Family::Riscv, Family::Xtensa];

fn device() -> DeviceInfo {
    DeviceInfo::new(
        "x4",
        "X4",
        DisplayInfo::new(
            480,
            800,
            220,
            PixelFormat::Gray2,
            paperpaw_platform::Rotation::Deg90,
            None,
        ),
        InputCaps::new(true, false, false, &[]),
        MemoryClass::Small,
    )
}

/// The kernel as `shells/xteink` builds it for jobs: the host, then the plugin
/// host once the services are wired, before the start.
fn kernel_with_host(family: Family) -> Kernel {
    Kernel::builder(device())
        .firmware_host(Box::new(NoFirmware::new(family)))
        .build()
        .unwrap()
}

/// The firmware as it was before this wiring: no firmware host. Its plugin
/// jobs (a store's install, update and removal, a plugin check) were all
/// answered as unsupported, because they share the firmware host's queue.
#[test]
fn a_kernel_without_the_firmware_host_takes_no_plugin_jobs() {
    for family in FAMILIES {
        let mut kernel = Kernel::builder(device()).build().unwrap();
        assert!(!offer_plugin_jobs(
            &mut kernel,
            family.loader(),
            family,
            Wired::ALL
        ));
    }
}

/// With the wiring every device of the family takes them.
#[test]
fn a_kernel_with_the_hosts_of_the_family_takes_plugin_jobs() {
    for family in FAMILIES {
        let mut kernel = kernel_with_host(family);
        assert!(
            offer_plugin_jobs(&mut kernel, family.loader(), family, Wired::ALL),
            "{family:?}"
        );
    }
}

/// A plugin that is installed is one the loader loads: plugin jobs check it
/// against the very lists the loader admits a card plugin against.
#[test]
fn plugin_jobs_check_a_plugin_against_what_the_loader_offers_a_card_plugin() {
    for family in FAMILIES {
        for bits in 0..16u8 {
            let wired = Wired {
                work: bits & 1 != 0,
                card: bits & 2 != 0,
                secrets: bits & 4 != 0,
                wifi: bits & 8 != 0,
                lookup: false,
            };
            let (loader, offered) = plugin_host(family, wired);
            let theirs = card_host(
                wired,
                loader.abi_major,
                loader.minor,
                match family {
                    Family::Riscv => paperpaw_native::header5::X4_PROFILE,
                    Family::Xtensa => paperpaw_native::header5::S3_PROFILE,
                },
            );
            assert_eq!(keys(&offered), keys(&theirs), "{family:?} {wired:?}");
            // A card plugin never installs plugins or firmware.
            assert!(
                !offered
                    .features
                    .iter()
                    .any(|f| f.key == "uses.install" || f.key == "uses.firmware")
            );
        }
    }
}

/// The service and feature keys a host offers.
fn keys<'a>(host: &'a paperpaw_native::metadata::Host<'_>) -> (Vec<&'a str>, Vec<&'a str>) {
    (
        host.services.iter().map(|s| s.key).collect(),
        host.features.iter().map(|f| f.key).collect(),
    )
}

/// The loader a plugin file is checked with is the chip's: the C3's files
/// for the X4 and the X3, the S3's for the X4 Pro and the X4 Classic.
#[test]
fn the_loader_is_the_chips() {
    let c3 = plugin_host(Family::Riscv, Wired::ALL).0;
    let s3 = plugin_host(Family::Xtensa, Wired::ALL).0;
    assert_eq!(
        c3,
        paperpaw_native::header5::Host::x4(paperpaw_native::host_glue::MINOR)
    );
    assert_eq!(
        s3,
        paperpaw_native::header5::Host::x4_pro(paperpaw_native::host_glue::MINOR)
    );
    assert_eq!(Family::Riscv.chip(), paperpaw_sdk::firmware::CHIP_ESP32_C3);
    assert_eq!(Family::Xtensa.chip(), paperpaw_sdk::firmware::CHIP_ESP32_S3);
}
