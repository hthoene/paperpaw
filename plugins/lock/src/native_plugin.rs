//! The interface-7 screen lock (slice 1): the settings only. The lock screen,
//! *Lock now*, the touch lock's filter and the events that lock the device need
//! the host pieces listed in `PORTING-7.md`; the rules (the code, the wrong
//! tries, the keypad and the pattern) are in `native_model`, tested without a
//! host.
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

mod image {
    include!(concat!(env!("OUT_DIR"), "/lock_declarations.rs"));
}

/// The interface-7 screen lock.
#[derive(Debug, Default)]
pub struct NativeLock;

impl Plugin for NativeLock {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
