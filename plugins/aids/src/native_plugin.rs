//! The interface-7 Reading aids: the three settings, and the rules for the
//! reader. The reader asks the `reader.filter` stage once for each book it
//! opens (and when a setting changes); the plugin answers with the rules of
//! its settings, and the reader applies them to each block (see
//! `READER-FILTER.md` and `PORTING-7.md`).
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
    settings::{self, SettingValue},
};

use crate::reader_filter;

mod image {
    include!(concat!(env!("OUT_DIR"), "/aids_declarations.rs"));
}

const BOLD: &str = "aids.bold-beginnings";
const REPLACE: &str = "aids.replace";
const PICTURES: &str = "aids.pictures";
/// The most bytes of a settings value on the wire (header included).
const VALUE: usize = 4096;
/// The most bytes of one stage reply.
const REPLY: usize = 3840;

/// The interface-7 Reading aids.
#[derive(Debug, Default)]
pub struct NativeAids;

impl Plugin for NativeAids {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        // A host that does not take the subscription yet leaves the settings
        // as they are: the reader has nothing to ask for.
        for batch in image::STAGES {
            match registrations::submit(host, batch) {
                Ok(()) | Err(Error::Unsupported) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Self)
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        _token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(Event::StageCall {
            stage: "reader.filter",
            payload,
            ..
        }) = incoming.decode_as::<kinds::StageCall>()?
        else {
            return Ok(());
        };
        rules(host, payload, reply)
    }
}

/// One buffer of `len` zeroed bytes, or [`Error::NoMemory`].
fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// Answers the ask with the rules the settings hold now. Rules the reply
/// cannot hold are refused, so the reader applies none of this plugin's.
fn rules(host: &Host<'_>, ask: &[u8], reply: &mut Reply<'_>) -> Result<(), Error> {
    let mut flag = [0u8; 32];
    let bold = matches!(
        settings::read_setting(host, BOLD, &mut flag)?,
        SettingValue::Bool(true)
    );
    let mut list = buffer(VALUE)?;
    let replacements = match settings::read_setting(host, REPLACE, &mut list)? {
        SettingValue::Text(text) => text,
        _ => "",
    };
    let mut choice = [0u8; 64];
    let pictures = match settings::read_setting(host, PICTURES, &mut choice)? {
        SettingValue::Text(text) => text,
        _ => crate::pictures::SHOW,
    };
    let mut out = buffer(REPLY)?;
    let written =
        reader_filter::answer(ask, bold, replacements, pictures, &mut out).map_err(|error| {
            match error {
                reader_filter::Error::NoMemory => Error::NoMemory,
                reader_filter::Error::Limit | reader_filter::Error::Wire(_) => Error::NoSpace,
                _ => Error::Invalid,
            }
        })?;
    reply.write(out.get(..written).ok_or(Error::NoSpace)?)
}
