//! Interface-7 plugin files (format 5) as code this firmware loads: what a
//! start once an update is kept goes by (`code_to_load`), and what was
//! fetched for an update (`fetched_for`).

use alloc::boxed::Box;

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

/// What a card that comes from Beta 3 holds: plugin files of interface 6
/// (format 4). This one is a file Beta 3 shipped, unchanged
/// (`tests/data/beta3-hyphenation-it-1.0.3-x4.source.txt`).
const BETA3: &[u8] = include_bytes!("../tests/data/beta3-hyphenation-it-1.0.3-x4.ppn");

/// A loader as the devices' is: it runs interface-7 files and takes any
/// other for none of its own.
struct Interface7Only;
impl super::CodeLoader for Interface7Only {
    fn load_native(
        &mut self,
        storage: &dyn Storage,
        code: &super::CodeFile<'_>,
    ) -> Result<Option<Box<dyn super::NativeAdmission>>, paperpaw_sdk::PluginProblem> {
        let mut head = [0u8; 6];
        let read = storage.read_at(code.path, 0, &mut head).unwrap_or(0);
        let seven = read == 6 && head[..4] == *b"PPNP" && head[4] == 5;
        Ok(seven.then(|| Box::new(super::Admitted) as Box<dyn super::NativeAdmission>))
    }
}

/// A plugin of Beta 3 left on the card is listed, switched on and failed,
/// as built for interface 6: not loaded, not taken for damaged, not gone,
/// and its files are as they were. The interface-7 plugin beside it runs.
#[test]
fn a_plugin_file_of_beta_3_is_listed_as_built_for_an_older_paperpaw() {
    use paperpaw_sdk::{PluginKind, PluginProblem, PluginState};
    let card = card()
        .with(
            "plugins/hyphenation-it/plugin.ini",
            "[plugin]\nid = hyphenation-it\nname = Hyphenation: Italian\nversion = 1.0.3\n",
        )
        .with("plugins/hyphenation-it/code.ppn", BETA3);
    // Beta 3's store named its code as installed; so is the new plugin's.
    let beta3 = paperpaw_native::Header::read(BETA3).unwrap().sha256;
    assert!(paperpaw_sdk::native::remember_code(
        &card,
        "hyphenation-it",
        &beta3
    ));
    assert!(paperpaw_sdk::native::remember_code(
        &card,
        "clock",
        &sha256()
    ));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(card)
        .code_plugins(Native::new("code.ppn"), Box::new(Interface7Only))
        .build()
        .unwrap();
    let old = kernel.plugins().get("hyphenation-it").unwrap();
    assert_eq!(
        (old.state, old.enabled, old.kind, old.problem),
        (
            PluginState::Failed,
            true,
            PluginKind::Code,
            Some(PluginProblem::OlderInterface { built_for: 6 })
        )
    );
    assert_eq!(old.manifest.version, paperpaw_sdk::Version::new(1, 0, 3));
    let new = kernel.plugins().get("clock").unwrap();
    assert_eq!((new.state, new.problem), (PluginState::Running, None));
    assert_eq!(
        kernel
            .storage()
            .size("plugins/hyphenation-it/code.ppn")
            .ok(),
        Some(BETA3.len() as u64)
    );
    let warnings = kernel.take_warnings();
    assert!(
        warnings.iter().any(|w| w
            .message
            .contains("plugins/hyphenation-it/code.ppn: not loaded (OlderInterface")),
        "{warnings:?}"
    );
}

/// Why a file the loader takes for none of its own does not run, from the
/// file itself; whatever its bytes, nothing panics.
#[test]
fn a_file_that_is_no_interface_7_file_says_why_it_does_not_run() {
    use paperpaw_sdk::PluginProblem;
    let why = |bytes: &[u8]| {
        let card = MemoryStorage::new();
        card.write("code.ppn", bytes).unwrap();
        super::file_problem(&card, "code.ppn")
    };
    let older = PluginProblem::OlderInterface { built_for: 6 };
    assert_eq!(why(BETA3), older);
    // Its header alone says so: the file may be cut after it.
    let head = &BETA3[..paperpaw_native::HEADER_LEN];
    assert_eq!(why(head), older);
    // The format before Beta 3's, of the same interface.
    let mut format3 = head.to_vec();
    format3[4] = 3;
    assert_eq!(why(&format3), older);
    // A build of the format before that said interface 7 of itself (a
    // draft's): the format is not this interface's.
    let mut draft = paperpaw_native::Header::read(head).unwrap();
    assert_eq!(draft.interface, 6);
    draft.interface = 7;
    draft.fingerprint = (draft.fingerprint & 0x0000_ffff_ffff_ffff) | (7 << 48);
    assert_eq!(why(&draft.to_bytes()), older);
    // An interface-7 file where the loader runs none.
    assert_eq!(why(FILE), PluginProblem::Unsupported);
    // A format after this build's.
    let mut newer = FILE.to_vec();
    newer[4] = 9;
    assert_eq!(why(&newer), PluginProblem::NeedsNewerPaperPaw);
    // No plugin file, a header cut short, nothing at all.
    for bytes in [&b"not a plugin"[..], &BETA3[..40], &b"PPNP"[..], &[][..]] {
        assert_eq!(why(bytes), PluginProblem::Damaged, "{bytes:?}");
    }
    assert_eq!(
        super::file_problem(&MemoryStorage::new(), "none.ppn"),
        PluginProblem::Damaged
    );
}
