//! Interface-7 host adapter components, not activated in dispatch yet.
pub(crate) mod calls;
pub(crate) mod context;
pub(crate) mod core_frame;
pub(crate) mod graphics;
pub(crate) mod handles;
pub(crate) mod lifecycle;
pub(crate) mod queries;
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    reason = "generated settings wire codec"
)]
pub(crate) mod settings_wire;
pub(crate) mod snapshot;
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    reason = "generated environment snapshot codec"
)]
pub(crate) mod snapshot_wire;

#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    reason = "generated packed-color codec"
)]
pub(crate) mod color_wire;

#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    reason = "generated slot-15 command batch codec"
)]
pub(crate) mod commands_wire;
#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    reason = "generated service op tables (abi/generated/services_wire.rs, byte for byte)"
)]
pub(crate) mod services_wire;
