//! The interface-7 plain text format: one `core.format` provider that keeps its
//! open documents between the host's calls (the host retains the instance).
use crate::{
    declarations::FORMAT_TOKEN,
    provider::{HostIo, TxtProvider},
};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/format_txt_declarations.rs"));
}

/// The plain text format's instance.
#[derive(Debug, Default)]
pub struct NativeTxt {
    provider: TxtProvider,
}

impl Plugin for NativeTxt {
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
