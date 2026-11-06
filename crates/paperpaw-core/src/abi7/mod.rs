//! Interface-7 host adapter components, not activated in dispatch yet.
pub(crate) mod calls;
pub(crate) mod context;
pub(crate) mod graphics;
mod handles;
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
