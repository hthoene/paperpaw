//! The interface-7 Wikipedia lookup (slice 1): the screen's declaration only;
//! the provider's is built into the image but not submitted: the host refuses
//! lookup providers until the stage `core.lookup` is live. Fetching
//! a summary needs the network host and the answer of a provider that fetches
//! needs the asynchronous stage reply; drawing the summary needs the screen
//! hosts. What the plugin decides without a host is in `native_model`, tested
//! without one (`PORTING-7.md`).
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

#[allow(
    dead_code,
    reason = "the provider is in the image, submitted once the host admits lookup providers"
)]
mod image {
    include!(concat!(env!("OUT_DIR"), "/wikipedia_declarations.rs"));
}

/// The interface-7 Wikipedia lookup.
#[derive(Debug, Default)]
pub struct NativeWikipedia;

impl Plugin for NativeWikipedia {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREEN {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
