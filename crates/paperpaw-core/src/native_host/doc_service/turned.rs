//! Op 16 with `flags` bit 0, `turned` (`docs/abi/document.md` 5): the picture
//! the format made is turned a quarter clockwise by the host, once, and
//! answered as a prepared image of the caller.
use super::{DocAsk, DocHost, Record, check, encode, wire};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

/// `doc_image_ask.flags` bit 0: the picture turned a quarter clockwise.
const TURNED: u32 = 1;

/// The flags of an ask: a bit this host does not know is refused.
pub(super) fn asked(flags: u32) -> Result<(), i32> {
    if flags & !TURNED == 0 {
        Ok(())
    } else {
        Err(ERR_UNSUPPORTED)
    }
}

/// The checked answer of a read (`out[..length]`): where an op 16 asked for
/// the picture turned, the one `doc_image` it holds becomes the turned
/// picture's (its own id, the turned size, `flags` bit 2). A picture that
/// came without an id (none was free) is passed on as it is, not turned.
/// The new length.
pub(super) fn answer(
    host: &dyn DocHost,
    record: &DocAsk<'_>,
    out: &mut [u8],
    length: usize,
) -> Result<usize, i32> {
    let DocAsk::Question(ask) = record else {
        return Ok(length);
    };
    if ask.flags & TURNED == 0 {
        return Ok(length);
    }
    let image: wire::ServiceDocImage = check::only(out.get(..length).unwrap_or_default())?;
    if image.image == 0 {
        return Ok(length);
    }
    let turned = host.turned(image.image)?;
    encode(
        &Record::ServiceDocImage(wire::ServiceDocImage {
            index: image.index,
            ..turned
        }),
        out,
    )
}
