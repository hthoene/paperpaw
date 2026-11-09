//! The interface-7 Word documents format: one `core.format` provider that keeps its
//! open documents between the host's calls (the host retains the instance).
use crate::{
    declarations::FORMAT_TOKEN,
    provider::{DocxProvider, HostIo},
};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/format_docx_declarations.rs"));
}

/// The Word documents format's instance.
#[derive(Debug, Default)]
pub struct NativeDocx {
    provider: DocxProvider,
}

impl Plugin for NativeDocx {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::FORMATS {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
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
        reply.write_with(|out| self.provider.call(&HostIo::new(host), payload, out))
    }
}
