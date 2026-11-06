//! Interface-7 host adapter components, not activated in dispatch yet.
pub(crate) mod calls;
mod context;
mod graphics;
mod handles;
pub(crate) mod lifecycle;
mod queries;
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    reason = "generated settings wire codec"
)]
mod settings_wire;
mod snapshot;
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    reason = "generated environment snapshot codec"
)]
mod snapshot_wire;
