//! Linked and packed entries share the native implementation and metadata.
#[cfg(feature = "linked")]
paperpaw_sdk::native_plugin!(
    crate::native_plugin::NativeBattery,
    metadata = include_bytes!("../native/metadata.bin"),
    linked = NATIVE_ENTRY
);

// With both features enabled (all-features checks), the linked form wins.
#[cfg(all(feature = "native-entry", not(feature = "linked")))]
paperpaw_sdk::native_plugin!(
    crate::native_plugin::NativeBattery,
    metadata = include_bytes!("../native/metadata.bin")
);
