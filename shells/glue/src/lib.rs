//! What the shells that host the linked interface-7 plugins share (Decision
//! 172): the services and features offered, the host table's heap and clock,
//! the read-only image the plugins' declarations are found in, and
//! registration with the kernel ([`register`]). A shell gives its own device,
//! card, secrets and network.

pub mod clock;
pub mod image;
pub mod net;
pub mod offer;
pub mod services;
pub mod view;

#[cfg(feature = "linked")]
mod register;
#[cfg(feature = "linked")]
pub use register::{PluginJobs, Registered, Shell, Unavailable, register};
