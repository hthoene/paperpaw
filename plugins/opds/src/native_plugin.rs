//! The interface-7 catalogs (slice 1): the settings only. Browsing,
//! downloading and the sync need the network host, secrets, library import
//! and the files facade (`PORTING-7.md`); the feed model is in
//! `native_model`, tested without a host.
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

mod image {
    include!(concat!(env!("OUT_DIR"), "/opds_declarations.rs"));
}

/// The interface-7 catalogs.
#[derive(Debug, Default)]
pub struct NativeOpds;

impl Plugin for NativeOpds {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
