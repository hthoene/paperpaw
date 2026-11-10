//! The interface-7 reader (entry slice): what the reader declares reaches the
//! host (settings, the six screens and the slots they fill, actions, menu
//! entries, key bindings, the events heard and the three stages). The screens
//! have no view yet: an opened screen draws nothing and takes no input, and
//! the plugin is not in the app's `NATIVE_PLUGINS` until they have one
//! (`PORTING-7.md`). What the reader decides without a host is in the
//! `native_*` models, tested without one.
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        events::Incoming,
        registrations,
        reply::Reply,
    },
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/reader_declarations.rs"));
}

/// The batches the reader submits, in the order the host needs them: the
/// settings and screens before what refers to them.
const BATCHES: [&[&[u8]]; 7] = [
    image::SETTINGS,
    image::SCREENS,
    image::NAVIGATION,
    image::KEYS,
    image::HOLD_KEYS,
    image::EVENTS,
    image::STAGES,
];

/// The interface-7 reader.
#[derive(Debug, Default)]
pub struct NativeReader;

impl Plugin for NativeReader {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batches in BATCHES {
            for batch in batches {
                registrations::submit(host, batch)?;
            }
        }
        Ok(Self)
    }

    fn event(
        &mut self,
        _host: &Host<'_>,
        _token: u32,
        _incoming: Incoming<'_>,
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn draw(
        &mut self,
        _host: &Host<'_>,
        _token: u32,
        _frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        Err(Error::State)
    }

    fn input(&mut self, _host: &Host<'_>, _token: u32, _input: Input<'_>) -> Result<Handled, Error> {
        Ok(Handled::No)
    }
}
