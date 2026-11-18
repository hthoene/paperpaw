//! An attachment as a file: the bytes of its part, as the server sent them in
//! base64, decoded in pieces from one file of the data folder into another, so
//! that nothing of it is held whole (a part is up to `store::MAX_PART` bytes).
use super::codec::Base64;
use super::fail::Fail;
use super::mem::vec_with;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;

/// The bytes read from the part at a time (on the heap: a frame may not hold
/// them).
const PIECE: usize = 3840;

/// Decodes the base64 file `from` into the file `to` (made, or made empty
/// first); the size of the file made.
///
/// # Errors
/// [`Fail::Storage`] where the card says no, [`Fail::NoMemory`].
pub fn decode_file(storage: &dyn Storage, from: &str, to: &str) -> Result<u64, Fail> {
    storage.write(to, b"")?;
    let mut piece = vec_with(PIECE)?;
    piece.resize(PIECE, 0);
    let mut decoded: Vec<u8> = vec_with(PIECE)?;
    let mut base64 = Base64::default();
    let (mut offset, mut size) = (0u64, 0u64);
    loop {
        let n = storage.read_at(from, offset, &mut piece)?;
        if n == 0 {
            break;
        }
        offset += n as u64;
        decoded.clear();
        base64.push(piece.get(..n).unwrap_or_default(), &mut decoded)?;
        size += decoded.len() as u64;
        if !decoded.is_empty() {
            storage.append(to, &decoded)?;
        }
    }
    decoded.clear();
    base64.finish(&mut decoded)?;
    if !decoded.is_empty() {
        size += decoded.len() as u64;
        storage.append(to, &decoded)?;
    }
    Ok(size)
}
