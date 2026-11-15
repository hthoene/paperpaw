//! Interface-7 plugin files (format 5) as code this firmware loads: what a
//! start once an update is kept goes by (`code_to_load`), and what was
//! fetched for an update (`fetched_for`).

use paperpaw_native::header5::Host;
use paperpaw_native::host_glue::MINOR;
use paperpaw_platform::Storage;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::native::Native;

use super::{code_to_load, fetched_for};
use crate::Kernel;
use crate::tests::DEVICE;

/// A format-5 file of the X4 (ESP32-C3), as `paperpaw-sign pack5` made it.
const FILE: &[u8] = include_bytes!("../../paperpaw-native/tests/data/abi7-x4-min.ppn");
const X4: Host = Host::x4(MINOR);
const INI: &str = "[plugin]\nid = clock\nname = Clock\nversion = 1.0.0\n";

fn card() -> MemoryStorage {
    let card = MemoryStorage::new().with("plugins/clock/plugin.ini", INI);
    card.write("plugins/clock/code.ppn", FILE).unwrap();
    card
}

/// The SHA-256 the file's header carries (its bytes 64 to 96).
fn sha256() -> [u8; 32] {
    FILE[64..96].try_into().unwrap()
}

/// An update on trial starts without code plugins; once it is kept the
/// firmware asks whether a start with them would load any, and starts again
/// if so (`shells/xteink/src/firmware.rs`). A card whose only code plugin is
/// an interface-7 file is such a card: before, only files of the format
/// before were seen, and the plugin stayed unloaded until some later start.
#[test]
fn a_kept_trial_with_one_interface_7_card_plugin_asks_for_the_restart() {
    let native = Native::new("code.ppn");
    let mut kernel = Kernel::builder(DEVICE)
        .storage(card())
        .native_after_trial(native.clone())
        .build()
        .unwrap();
    // The trial's start loaded no code.
    assert!(kernel.plugins().get("clock").is_some());
    kernel.trial_kept();
    assert!(code_to_load(kernel.storage(), &native, Some(&X4)));
}

#[test]
fn only_a_file_the_loader_reads_is_code_to_load() {
    let native = Native::new("code.ppn");
    // A device whose loader reads no interface-7 file (it names none).
    assert!(!code_to_load(&card(), &native, None));
    // Made for the X4: the X4 Pro's loader refuses it.
    assert!(!code_to_load(&card(), &native, Some(&Host::x4_pro(MINOR))));
    // Needing a newer minor than this firmware has.
    let old = Host::x4(0);
    let needs = u16::from_le_bytes([FILE[10], FILE[11]]);
    assert_eq!(code_to_load(&card(), &native, Some(&old)), needs == 0);
    // Cut short: its length no longer holds to its header.
    let cut = MemoryStorage::new().with("plugins/clock/plugin.ini", INI);
    cut.write("plugins/clock/code.ppn", &FILE[..FILE.len() - 1])
        .unwrap();
    assert!(!code_to_load(&cut, &native, Some(&X4)));
    // Not a plugin file at all, and a file shorter than a header: no panic.
    for bytes in [&b"PPNP"[..], &[0u8; 200][..], &[][..]] {
        let other = MemoryStorage::new().with("plugins/clock/plugin.ini", INI);
        other.write("plugins/clock/code.ppn", bytes).unwrap();
        assert!(!code_to_load(&other, &native, Some(&X4)));
    }
    // A folder being installed or put aside is not one to load.
    let aside = MemoryStorage::new();
    aside.write("plugins/.clock.old/code.ppn", FILE).unwrap();
    assert!(!code_to_load(&aside, &native, Some(&X4)));
}

/// An interface-7 build fetched for an update waits for the firmware that
/// loads it, named by the update plugin, as a build of the format before did.
#[test]
fn an_interface_7_build_fetched_for_an_update_waits_for_this_firmware() {
    let native = Native::new("code.ppn");
    let card = MemoryStorage::new().with("plugins/.clock.next/plugin.ini", INI);
    card.write("plugins/.clock.next/code.ppn", FILE).unwrap();
    // Put there otherwise: not taken.
    assert!(fetched_for(&card, &native, Some(&X4)).is_empty());
    assert!(!code_to_load(&card, &native, Some(&X4)));
    assert!(paperpaw_sdk::native::remember_fetched(
        &card,
        "clock",
        &sha256()
    ));
    assert_eq!(
        fetched_for(&card, &native, Some(&X4)),
        [("clock".into(), sha256())]
    );
    assert!(code_to_load(&card, &native, Some(&X4)));
    // For the X4 Pro nothing waits.
    assert!(fetched_for(&card, &native, Some(&Host::x4_pro(MINOR))).is_empty());
}
