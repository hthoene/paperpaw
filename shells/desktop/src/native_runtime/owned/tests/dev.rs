//! #772: the developer switch's loader (`dev`) on real libraries: off by
//! default, kinds told from the bytes, the folder's id and not the library's,
//! clean refusals, unload and reload.
use super::super::dev::{DevPlugins, Kind, Refusal, classify};
use super::e2e::sample_bytes;
use super::foreign::fixture_custom;
use super::*;
use paperpaw_core::native_host::registrations::{
    REGISTRATION_MAX_BYTES, Record, SettingSpec as SpecRecord, records_encode,
};

const TEXT_X: &[u8] = &[1, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, b'x'];

/// A card root with `plugins/<id>/code-host.so` holding `bytes` for each id.
fn card(files: &[(&str, &[u8])]) -> std::path::PathBuf {
    let root = crate::private::dir("paperpaw-dev772").unwrap();
    for (id, bytes) in files {
        let folder = root.join("plugins").join(id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(crate::native::file_name()), bytes).unwrap();
    }
    root
}

/// Whether a private copy of plugin `id` is mapped in this process.
fn mapped_copy(id: &str) -> bool {
    std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .lines()
        .any(|line| line.contains("paperpaw-abi7") && line.contains(&format!("/{id}-")))
}

/// A tiny no_std library exporting `source`'s symbols, built with rustc alone.
fn tiny(name: &str, source: &str) -> Vec<u8> {
    let root = crate::private::dir("paperpaw-dev772-tiny").unwrap();
    let input = root.join(format!("{name}.rs"));
    std::fs::write(
        &input,
        format!(
            "#![no_std]\n#[panic_handler]\nfn panic(_: &core::panic::PanicInfo) -> ! {{ loop {{}} }}\n{source}"
        ),
    )
    .unwrap();
    let out = root.join(format!("{name}.so"));
    let result = Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-type=cdylib",
            "-Cpanic=abort",
            "-Clink-arg=-nostartfiles",
            "-Clink-arg=-nostdlib",
        ])
        .arg(&input)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = std::fs::read(&out).unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    bytes
}

fn interface6_like() -> Vec<u8> {
    tiny(
        "six",
        "#[unsafe(no_mangle)] pub static mut PAPERPAW_API: usize = 1;\n\
         #[unsafe(no_mangle)] pub static PAPERPAW_ENTRY: usize = 1;\n",
    )
}

fn both() -> Vec<u8> {
    tiny(
        "both",
        "#[unsafe(no_mangle)] pub static mut PAPERPAW_API: usize = 1;\n\
         #[unsafe(no_mangle)] pub static PAPERPAW_METADATA: [u8; 4] = [1; 4];\n",
    )
}

fn neither() -> Vec<u8> {
    tiny(
        "neither",
        "#[unsafe(no_mangle)] pub extern \"C\" fn other() {}\n",
    )
}

#[test]
fn the_kind_is_told_from_the_bytes_and_unclear_is_never_guessed() {
    assert_eq!(classify(sample_bytes()), Kind::Interface7);
    assert_eq!(classify(&interface6_like()), Kind::Interface6);
    assert_eq!(classify(&both()), Kind::Unclear, "both: not guessed");
    assert_eq!(classify(&neither()), Kind::Unclear, "neither: not guessed");
    assert_eq!(classify(b""), Kind::Unclear);
    assert_eq!(classify(b"\x7fELF"), Kind::Unclear);
    assert_eq!(classify(&[0xA5; 4096]), Kind::Unclear);
}

#[test]
fn without_the_switch_nothing_is_read_or_loaded() {
    let root = card(&[("sample", sample_bytes())]);
    let found = DevPlugins::load_folder_if(&root, false);
    assert!(found.ids().is_empty() && found.refused().is_empty());
    assert!(!mapped_copy("sample"));
    drop(found);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_sample_in_a_plugin_folder_loads_ticks_and_unloads_in_order() {
    let root = card(&[("tickdraw", sample_bytes())]);
    let mut found = DevPlugins::load_folder_if(&root, true);
    assert_eq!(found.ids(), ["tickdraw"], "{:?}", found.refused());
    assert_eq!(found.declared("tickdraw"), Some(0), "staged, none applied");
    assert!(mapped_copy("tickdraw"));
    // One tick registration, called without an environment: the sample's tick
    // needs none (only its draw asks the host, and that waits for S2).
    let ticks = found.tick(41);
    assert_eq!(ticks.len(), 1);
    assert_eq!(ticks[0].0, "tickdraw");
    assert_eq!(ticks[0].1, [host_glue::OK], "{ticks:?}");
    found.unload();
    assert!(found.ids().is_empty());
    assert!(!mapped_copy("tickdraw"), "unmapped after unload");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_plugin_folder_removed_while_loaded_unloads_cleanly() {
    let root = card(&[("vanish", sample_bytes())]);
    let found = DevPlugins::load_folder_if(&root, true);
    assert_eq!(found.ids(), ["vanish"]);
    std::fs::remove_dir_all(root.join("plugins/vanish")).unwrap();
    // The private copy is what is mapped: still callable, then released.
    assert_eq!(found.tick(1).len(), 1);
    assert!(mapped_copy("vanish"));
    drop(found);
    assert!(!mapped_copy("vanish"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_same_plugin_reloads_after_an_unload() {
    let root = card(&[("again", sample_bytes())]);
    let mut found = DevPlugins::load_folder_if(&root, true);
    assert_eq!(found.ids(), ["again"]);
    found.unload();
    assert!(!mapped_copy("again"));
    found = DevPlugins::load_folder_if(&root, true);
    assert_eq!(found.ids(), ["again"], "{:?}", found.refused());
    assert_eq!(found.tick(2).len(), 1);
    drop(found);
    assert!(!mapped_copy("again"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn garbled_libraries_and_other_kinds_are_refused_or_passed_over_cleanly() {
    let six = interface6_like();
    let (both, neither) = (both(), neither());
    let truncated = sample_bytes()[..sample_bytes().len() / 2].to_vec();
    let root = card(&[
        ("six", six.as_slice()),
        ("both", both.as_slice()),
        ("neither", neither.as_slice()),
        ("junk", b"not a library"),
        ("empty", b""),
        ("cut", truncated.as_slice()),
        ("good", sample_bytes()),
    ]);
    let found = DevPlugins::load_folder_if(&root, true);
    assert_eq!(found.ids(), ["good"], "{:?}", found.refused());
    let refused: Vec<(&str, &Refusal)> = found
        .refused()
        .iter()
        .map(|(id, why)| (id.as_str(), why))
        .collect();
    // Interface 6 is the other loader's: passed over, not refused.
    assert!(refused.iter().all(|(id, _)| *id != "six"), "{refused:?}");
    for id in ["both", "neither", "junk", "empty", "cut"] {
        assert!(
            refused
                .iter()
                .any(|(name, why)| *name == id && matches!(why, Refusal::Kind(Kind::Unclear))),
            "{id}: {refused:?}"
        );
    }
    for id in ["six", "both", "neither", "junk", "empty", "cut"] {
        assert!(!mapped_copy(id), "{id} must never be mapped");
    }
    drop(found);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_folder_that_is_no_plugin_id_is_refused_before_its_file_is_read() {
    let root = card(&[("Not-An-Id", sample_bytes()), ("two words", sample_bytes())]);
    let found = DevPlugins::load_folder_if(&root, true);
    assert!(found.ids().is_empty());
    assert_eq!(found.refused().len(), 2);
    assert!(found.refused().iter().all(|(_, why)| *why == Refusal::Id));
    drop(found);
    std::fs::remove_dir_all(&root).unwrap();
}

/// An interface-7 library with an extra export is refused by the strict
/// freestanding rule before any code of it ran.
#[test]
fn a_library_exporting_more_than_its_entry_symbols_is_refused_before_it_runs() {
    let metadata = crate::native_runtime::tests::metadata_bytes();
    let bytes = tiny(
        "loud",
        &format!(
            "#[unsafe(no_mangle)] pub static PAPERPAW_METADATA: [u8; {}] = {metadata:?};\n\
             #[unsafe(no_mangle)] pub static PAPERPAW_ENTRY: usize = 1;\n\
             #[unsafe(no_mangle)] pub extern \"C\" fn extra() {{}}\n",
            metadata.len()
        ),
    );
    assert_eq!(classify(&bytes), Kind::Interface7);
    let root = card(&[("loud", bytes.as_slice())]);
    let found = DevPlugins::load_folder_if(&root, true);
    assert!(found.ids().is_empty());
    assert!(
        matches!(
            found.refused(),
            [(id, Refusal::Runtime(RuntimeError::Export))] if id == "loud"
        ),
        "{:?}",
        found.refused()
    );
    assert!(!mapped_copy("loud"), "never mapped");
    drop(found);
    std::fs::remove_dir_all(&root).unwrap();
}

/// The id is the folder's, never the library's: the same library declares a
/// setting in the group `weather.main` without declaring the group. Under the
/// folder `weather` that group is its own and missing, so it is refused at
/// commit; under the folder `other` the group is another plugin's and the
/// declaration stands (the kernel then refuses it for now, as declarations in
/// the image cannot be read back until the draft-4 views are wired).
#[test]
fn a_declaration_under_another_plugins_id_is_decided_by_the_folders_id() {
    let mut valid = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(
        &[Record::SettingSpec(SpecRecord {
            name: "s",
            kind: 3,
            flags: 0,
            group: "weather.main",
            title: "title",
            help: "",
            default: TEXT_X,
            min: 0,
            max: 0,
            choices: "",
        })],
        &mut valid,
    )
    .unwrap();
    valid.truncate(len);
    let library = fixture_custom(valid, &[], false, "", "");
    let bytes = std::fs::read(&library.path).unwrap();
    let root = card(&[("weather", bytes.as_slice()), ("other", bytes.as_slice())]);
    let found = DevPlugins::load_folder_if(&root, true);
    // Under `other` the declaration stands, and the kernel refuses it only
    // because declarations in the image cannot be read back yet (temporary,
    // until the draft-4 views are wired): visibly, not loaded without them.
    assert!(found.ids().is_empty(), "{:?}", found.refused());
    assert!(
        found
            .refused()
            .iter()
            .any(|(id, why)| id == "other"
                && format!("{why}").contains("DeclarationViewsUnavailable")),
        "{:?}",
        found.refused()
    );
    // Under its own id the group is missing: refused at the cross-record
    // check, before the image question comes up.
    assert!(
        found.refused().iter().any(|(id, why)| id == "weather"
            && matches!(why, Refusal::Lifecycle(_) | Refusal::Runtime(_))
            && !format!("{why}").contains("DeclarationViewsUnavailable")),
        "refused under its own id: {:?}",
        found.refused()
    );
    assert_eq!(found.refused().len(), 2, "{:?}", found.refused());
    assert!(!mapped_copy("weather"), "released after the refusal");
    drop(found);
    std::fs::remove_dir_all(&root).unwrap();
}

/// Through the device: off by default nothing loads; with the switch the
/// plugin is loaded at start, released and loaded again at a restart, and
/// released with the device.
#[test]
fn the_device_loads_at_start_and_again_at_restart_only_with_the_switch() {
    // The switch is the process's: no other device meanwhile.
    let _one = crate::one_device_at_a_time();
    let root = card(&[("devplug", sample_bytes())]);
    let profile = crate::profiles::find("boox-go-7").unwrap();
    let open = || crate::device::library(&root).unwrap();
    let off = crate::device::Device::new(profile, None, open(), true).unwrap();
    assert!(off.abi7_plugins().ids().is_empty(), "off by default");
    drop(off);
    assert!(!mapped_copy("devplug"));

    crate::native_runtime::set_enabled(true);
    let mut on = crate::device::Device::new(profile, None, open(), true).unwrap();
    crate::native_runtime::set_enabled(false);
    assert_eq!(on.abi7_plugins().ids(), ["devplug"]);
    assert!(mapped_copy("devplug"));
    on.restart();
    // The switch is off again: the restart released it and loads none.
    assert!(on.abi7_plugins().ids().is_empty());
    assert!(!mapped_copy("devplug"));
    drop(on);
    std::fs::remove_dir_all(&root).unwrap();
}

/// With the switch on the interface-6 loader takes interface-6 libraries
/// only: one exporting both kinds, neither, or interface 7 is not its to take.
/// With the switch off it decides as before (no classification at all).
#[test]
fn the_interface_6_loader_takes_only_interface_6_with_the_switch_on() {
    use crate::native::interface6_only;
    let (six, both, neither) = (interface6_like(), both(), neither());
    assert!(interface6_only(&six, true));
    for refused in [sample_bytes(), &both, &neither, b"junk"] {
        assert!(!interface6_only(refused, true));
    }
    for any in [&six[..], sample_bytes(), &both, &neither, b"junk"] {
        assert!(interface6_only(any, false), "off: as before");
    }
}

/// The private copy is written under a name of its own and renamed, so no
/// cut-short copy is ever found under the trusted name, and it is removed
/// with the plugin; loading again writes it anew.
#[test]
fn the_private_copy_is_renamed_into_place_and_removed_at_unload() {
    let copies = |id: &str| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(super::super::dev::copies_dir().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(&format!("{id}-")))
            .collect();
        names.sort();
        names
    };
    let root = card(&[("copied", sample_bytes())]);
    let mut found = DevPlugins::load_folder_if(&root, true);
    assert_eq!(found.ids(), ["copied"]);
    let names = copies("copied");
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(!names[0].contains(".part"), "{names:?}");
    found.unload();
    assert!(copies("copied").is_empty(), "removed after the unmap");
    let found = DevPlugins::load_folder_if(&root, true);
    assert_eq!(found.ids(), ["copied"], "{:?}", found.refused());
    assert_eq!(copies("copied").len(), 1);
    drop(found);
    assert!(copies("copied").is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}
