//! The interface-7 progress sync (slice 1): the settings only. The account,
//! the sync itself and the overlay need the network host, the secrets and the
//! book events (`PORTING-7.md`); their logic is in `native_model`, tested
//! without a host.
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

mod image {
    include!(concat!(env!("OUT_DIR"), "/kosync_declarations.rs"));
}

/// The interface-7 progress sync.
#[derive(Debug, Default)]
pub struct NativeKosync;

impl Plugin for NativeKosync {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
