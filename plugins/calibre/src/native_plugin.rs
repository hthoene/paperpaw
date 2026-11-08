//! The interface-7 wireless library (slice 1): the setting only. The
//! connection to Calibre, the screen and the books received need the socket
//! host, the library import and the files host (`PORTING-7.md`); the wire
//! format is in `native_model`, tested without a host.
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

mod image {
    include!(concat!(env!("OUT_DIR"), "/calibre_declarations.rs"));
}

/// The interface-7 wireless library.
#[derive(Debug, Default)]
pub struct NativeCalibre;

impl Plugin for NativeCalibre {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
