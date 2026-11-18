//! The SDK's private copies of the generated wire codecs (`tools/abi-gen.py`
//! writes them byte for byte from `abi/abi.def`, like `declare/records_wire.rs`).
//! The typed facades build on these; no wire framing is written by hand, and
//! nothing here is public API.

// The settings value codec keeps its path in `declare` (the interface's
// shape names it there); the command codec reaches it by this name.
pub(crate) use super::declare::settings_value as settings_wire;
#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    unreachable_pub,
    reason = "generated snapshot codec (abi/generated/snapshot_wire.rs, byte for byte)"
)]
pub(crate) mod snapshot_wire;
#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    unreachable_pub,
    reason = "generated colour codec (abi/generated/color_wire.rs, byte for byte)"
)]
pub(crate) mod color_wire;
#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    unreachable_pub,
    reason = "generated command codec (abi/generated/commands_wire.rs, byte for byte)"
)]
pub(crate) mod commands_wire;
#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    unreachable_pub,
    reason = "generated service op tables (abi/generated/services_wire.rs, byte for byte)"
)]
pub(crate) mod services_wire;
