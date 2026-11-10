//! The interface-7 plugin store (slice 1): the settings only. Browsing,
//! installing and the downloads need the network host, the install facade
//! and the files facade (`PORTING-7.md`); the rules are in
//! `native_model`, tested without a host.
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

mod image {
    include!(concat!(env!("OUT_DIR"), "/store_declarations.rs"));
}

/// The interface-7 catalogs.
#[derive(Debug, Default)]
pub struct NativeStore;

impl Plugin for NativeStore {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
