//! Linked and packed entries share the native implementation and metadata.
// Metadata describes the actual target, not just pointer width.
#[cfg(all(
    any(feature = "linked", feature = "native-entry"),
    target_arch = "x86_64",
    target_os = "linux"
))]
const MARKDOWN_ABI_METADATA: &[u8] = include_bytes!("../native/metadata.host.bin");
#[cfg(all(
    any(feature = "linked", feature = "native-entry"),
    target_arch = "riscv32",
    target_os = "none"
))]
const MARKDOWN_ABI_METADATA: &[u8] = include_bytes!("../native/metadata.bin");
// Linked on any other target: the host's metadata, which the SDK sets to the
// profile of the target the program is built for (Decision 172).
#[cfg(all(
    feature = "linked",
    not(any(
        all(target_arch = "x86_64", target_os = "linux"),
        all(target_arch = "riscv32", target_os = "none")
    ))
))]
const MARKDOWN_ABI_METADATA: &[u8] = include_bytes!("../native/metadata.host.bin");
#[cfg(all(
    feature = "native-entry",
    not(feature = "linked"),
    not(any(
        all(target_arch = "x86_64", target_os = "linux"),
        all(target_arch = "riscv32", target_os = "none")
    ))
))]
compile_error!("MARKDOWN needs a generated ABI metadata profile for this target");

#[cfg(feature = "linked")]
paperpaw_sdk::native_plugin!(
    crate::native_plugin::NativeMarkdown,
    metadata = MARKDOWN_ABI_METADATA,
    linked = NATIVE_ENTRY
);

// With both features enabled (all-features checks), the linked form wins.
#[cfg(all(feature = "native-entry", not(feature = "linked")))]
paperpaw_sdk::native_plugin!(
    crate::native_plugin::NativeMarkdown,
    metadata = MARKDOWN_ABI_METADATA
);
