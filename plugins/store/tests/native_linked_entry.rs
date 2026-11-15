//! The built-in descriptor exposes the same checked metadata as the packed entry.
#![cfg(feature = "linked")]

#[test]
fn linked_entry_preserves_generated_metadata_and_native_factory() {
    let entry = paperpaw_plugin_store::NATIVE_ENTRY.entry();
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    let expected = include_bytes!("../native/metadata.host.bin");
    #[cfg(all(target_arch = "riscv32", target_os = "none"))]
    let expected = include_bytes!("../native/metadata.bin");
    assert!(entry.make.is_some());
    assert_eq!(entry.abi_major, 7);
    assert_eq!(entry.metadata.len, expected.len());
    // SAFETY: Linked owns immutable static metadata for the program's lifetime.
    let metadata = unsafe { std::slice::from_raw_parts(entry.metadata.ptr, entry.metadata.len) };
    assert_eq!(metadata, expected);
}

/// The store restarts PaperPaw (*Restart now*, *Switch on and restart*): the
/// host denies `command.restart` to a plugin that did not declare `uses.power`.
#[test]
fn the_metadata_names_the_features_the_store_commands_need() {
    let entry = paperpaw_plugin_store::NATIVE_ENTRY.entry();
    // SAFETY: Linked owns immutable static metadata for the program's lifetime.
    let metadata = unsafe { std::slice::from_raw_parts(entry.metadata.ptr, entry.metadata.len) };
    let has = |key: &str| {
        metadata
            .windows(key.len())
            .any(|window| window == key.as_bytes())
    };
    for key in ["uses.power", "uses.install", "uses.network"] {
        assert!(has(key), "{key} is not declared");
    }
}
