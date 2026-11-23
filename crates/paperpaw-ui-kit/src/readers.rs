//! The reading apps the device has for a format (the document service's
//! `readers`), as the file lists offer them under *Open with…*.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    document::{Documents, ReaderCursor},
};

use crate::{
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
};

/// Reading apps listed at most.
pub const MOST: usize = 12;
/// The reading apps' list, a page of it: the request.
const INPUT: usize = 256;

/// The reading apps of books of `format` (all where it is empty): their
/// screens and titles, at most [`MOST`]. None where the service cannot list
/// them; the list changing between its pages starts it again, once.
///
/// # Errors
/// `NoMemory`.
pub fn of_format(
    host: &Host<'_>,
    documents: &Documents,
    format: &str,
) -> Result<Vec<(String, String)>, Error> {
    let (mut input, mut out) = (buffer(INPUT)?, buffer(SERVICE_BUFFER)?);
    let mut found: Vec<(String, String)> = Vec::new();
    let mut cursor = ReaderCursor::start();
    let mut again = true;
    loop {
        let page = match documents.readers(host, format, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::State) if again => {
                again = false;
                found.clear();
                cursor = ReaderCursor::start();
                continue;
            }
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Vec::new()),
        };
        for reader in page.rows() {
            if found.len() >= MOST {
                return Ok(found);
            }
            push(&mut found, (copy(reader.screen())?, copy(reader.title())?))?;
        }
        match page.end().next() {
            Some(next) => cursor = next,
            None => return Ok(found),
        }
    }
}
