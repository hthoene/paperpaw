//! The interface-7 KOReader exchange (slice 1): nothing is declared yet. The
//! exchange itself needs the book events, the document and annotations
//! facades, the files facade and the book-data declaration (`PORTING-7.md`);
//! the files' reading and writing is in `native_model`, tested without a host.
use paperpaw_sdk::native::{Error, Host, Plugin};

/// The interface-7 KOReader exchange.
#[derive(Debug, Default)]
pub struct NativeKoReader;

impl Plugin for NativeKoReader {
    fn make(_host: &Host<'_>) -> Result<Self, Error> {
        Ok(Self)
    }
}
