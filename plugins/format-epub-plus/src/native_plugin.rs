//! The interface-7 EPUB plugin for picture books and math: a `core.format` provider of fixed-layout
//! books that keeps its open documents between the host's calls (the host retains the instance),
//! and the `core.convert` hook that turns the `<math>` an EPUB format hands on into text (#429).
use crate::declarations::{CONVERT_TOKEN, FORMAT_TOKEN};
use paperpaw_epub_family::{epub_plus_provider, provider::FormatProvider, serve};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    convert::{Asked, Converted},
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
};

mod image {
    include!(concat!(
        env!("OUT_DIR"),
        "/format_epub_plus_declarations.rs"
    ));
}

/// The format's instance.
#[derive(Debug)]
pub struct NativeEpubPlus {
    provider: FormatProvider,
}

impl Plugin for NativeEpubPlus {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::FORMATS.iter().chain(image::STAGES) {
            registrations::submit(host, batch)?;
        }
        Ok(Self {
            provider: epub_plus_provider(),
        })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token != FORMAT_TOKEN && token != CONVERT_TOKEN {
            return Ok(());
        }
        // Only the stage call is read, so only its decoder is linked.
        let Some(Event::StageCall { stage, payload, .. }) =
            incoming.decode_as::<kinds::StageCall>()?
        else {
            return Ok(());
        };
        match (token, stage) {
            (FORMAT_TOKEN, "core.format") => {
                reply.write_with(|out| serve(&mut self.provider, host, payload, out))
            }
            // The formula of a `<math>` an EPUB format handed on (#429).
            (CONVERT_TOKEN, paperpaw_sdk::native::convert::STAGE) => {
                let asked = Asked::decode(payload)?;
                let text = (asked.element() == "math")
                    .then(|| paperpaw_epub_family::math_text(asked.markup()))
                    .flatten()
                    .filter(|text| text.len() <= paperpaw_sdk::native::convert::TEXT_MAX);
                reply.write_with(|out| match &text {
                    Some(text) => Converted::text(text).encode(out),
                    None => Converted::pass().encode(out),
                })
            }
            _ => Ok(()),
        }
    }
}
