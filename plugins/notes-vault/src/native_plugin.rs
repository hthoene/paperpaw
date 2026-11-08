//! The interface-7 notes vault (slice 1): the settings only. The pages, the
//! Markdown format and the widgets need the document, files and widget hosts
//! and the work service (`PORTING-7.md`); their model is in `native_model`,
//! tested without a host.
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

mod image {
    include!(concat!(env!("OUT_DIR"), "/notes_vault_declarations.rs"));
}

/// The interface-7 notes vault.
#[derive(Debug, Default)]
pub struct NativeNotesVault;

impl Plugin for NativeNotesVault {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
