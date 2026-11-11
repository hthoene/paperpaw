//! The interface-7 HTML files format: one `core.format` provider that keeps its
//! open documents between the host's calls (the host retains the instance).
use crate::declarations::FORMAT_TOKEN;
use paperpaw_epub_family::{html_provider, provider::FormatProvider, serve};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/format_html_declarations.rs"));
}

/// The format's instance.
#[derive(Debug)]
pub struct NativeHtml {
    provider: FormatProvider,
}

impl Plugin for NativeHtml {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::FORMATS {
            registrations::submit(host, batch)?;
        }
        Ok(Self {
            provider: html_provider(),
        })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token != FORMAT_TOKEN {
            return Ok(());
        }
        // Only the stage call is read, so only its decoder is linked.
        let Some(Event::StageCall {
            stage: "core.format",
            payload,
            ..
        }) = incoming.decode_as::<kinds::StageCall>()?
        else {
            return Ok(());
        };
        reply.write_with(|out| serve(&mut self.provider, host, payload, out))
    }
}
