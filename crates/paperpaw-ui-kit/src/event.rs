//! Reading the events a plugin handles.
//!
//! [`read_event!`](crate::read_event!) reads an incoming event if it is of one of the kinds listed and
//! links only the decoders of those kinds, where `Incoming::decode` links all of them.

/// Reads `$incoming` (an `events::Incoming`) as the first of the listed kinds
/// (`events::kinds::*`) it is: `Ok`-wrapped by `?` into `Option<Event>`, `None` for an
/// event of any other kind, whose body is not read. A listed kind with a malformed body
/// is the error `Event::decode` gives, returned from the calling function.
///
/// ```ignore
/// let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::ScreenClosed) else {
///     return Ok(());
/// };
/// match event { /* arms for those two kinds */ _ => Ok(()) }
/// ```
#[macro_export]
macro_rules! read_event {
    ($incoming:expr; $($kind:ty),+ $(,)?) => {{
        let incoming = &$incoming;
        let mut found = ::core::option::Option::None;
        $(
            if found.is_none() {
                found = incoming.decode_as::<$kind>()?;
            }
        )+
        found
    }};
}

#[cfg(test)]
mod tests {
    use paperpaw_sdk::native::{
        Error,
        events::{Event, Incoming, kinds},
    };

    const STARTED: [u8; 12] = [1, 32, 9, 0, 7, 0, 0, 0, 0, 0, 0, 0];
    const ACTION: [u8; 15] = [1, 39, 12, 0, 7, 0, 0, 0, 0, 0, 0, 0, 2, b'o', b'k'];

    fn read(bytes: &[u8]) -> Result<Option<Event<'_>>, Error> {
        let incoming = Incoming::new(bytes)?;
        Ok(read_event!(incoming; kinds::ActionRun, kinds::Resume))
    }

    #[test]
    fn a_listed_kind_is_read_as_decode_reads_it() {
        assert_eq!(read(&ACTION), Ok(Some(Event::decode(&ACTION).unwrap())));
    }

    #[test]
    fn another_kind_is_none_and_the_plugin_falls_through() {
        assert_eq!(read(&STARTED), Ok(None));
    }

    #[test]
    fn a_malformed_body_of_a_listed_kind_is_the_error_decode_gives() {
        let mut bad = ACTION;
        bad[13] = 0xff;
        assert_eq!(read(&bad), Err(Error::Utf8));
        assert_eq!(Event::decode(&bad), Err(Error::Utf8));
    }

    #[test]
    fn a_malformed_body_of_another_kind_is_not_read() {
        // Started with a body too short for its field: only asking for it reads it.
        let short = [1, 32, 3, 0, 7, 0];
        assert_eq!(read(&short), Ok(None));
        assert!(Event::decode(&short).is_err());
    }
}
