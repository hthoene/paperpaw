//! The interface-7 translation plugin (slice 1): the panel's screen (its offer
//! for the slot `core.translate` is in the image but not submitted) (the language setting follows with
//! draft 29, which lets a choice have 33 values). Asking DeepL
//! needs the network host and the secrets; showing the translation needs the
//! screen hosts. What the plugin decides without a host is in
//! `native_model`, tested without one (`PORTING-7.md`).
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

#[expect(
    dead_code,
    reason = "the slot offer is in the image, submitted once the host admits registry declarations"
)]
mod image {
    include!(concat!(env!("OUT_DIR"), "/translate_declarations.rs"));
}

/// The interface-7 translation plugin.
#[derive(Debug, Default)]
pub struct NativeTranslate;

impl Plugin for NativeTranslate {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREEN {
            registrations::submit(host, batch)?;
        }
        Ok(Self)
    }
}
