//! The interface-7 Reading aids: the three settings. The rules themselves are
//! the reader's to ask for (the `reader.filter` stage, `READER-FILTER.md`),
//! so there is nothing else to do until the reader calls it; see
//! `PORTING-7.md`.
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

mod image {
    include!(concat!(env!("OUT_DIR"), "/aids_declarations.rs"));
}

/// The interface-7 Reading aids.
#[derive(Debug, Default)]
pub struct NativeAids;

impl Plugin for NativeAids {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
