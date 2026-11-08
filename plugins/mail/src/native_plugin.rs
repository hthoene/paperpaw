//! The interface-7 Mail, slice 1: the settings are declared to the host. The
//! connection to the servers (sockets), the password's keeping (secrets), the
//! background work, the screens and the widgets wait for their host services
//! and are not here yet (see `PORTING-7.md`); the interface-6 plugin is still
//! in place beside this one.
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

mod image {
    include!(concat!(env!("OUT_DIR"), "/mail_declarations.rs"));
}

/// Mail's interface-7 state: none yet.
#[derive(Debug, Default)]
pub struct NativeMail;

impl Plugin for NativeMail {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
