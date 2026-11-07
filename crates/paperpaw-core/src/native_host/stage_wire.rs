//! Framing-only checks for opaque stage-local record batches.
//! Stage payload tags belong to their owner's schema, not the kernel codec.
use paperpaw_native::host_glue::ERR_INVALID;

pub(super) fn validate_reply(bytes: &[u8], max_bytes: usize, max_records: u32) -> Result<(), i32> {
    if bytes.len() > max_bytes {
        return Err(ERR_INVALID);
    }
    visit_batch(bytes, max_records, |_, _| Ok(()))
}

/// Walk opaque canonical frames. The visitor may decode a service record using
/// the generated kernel codec; stage subscribers keep their own payload schema.
pub(super) fn visit_batch(
    bytes: &[u8],
    max_records: u32,
    mut visit: impl FnMut(u32, &[u8]) -> Result<(), i32>,
) -> Result<(), i32> {
    let mut at = 0;
    let count = integer(bytes, &mut at)?;
    if count > max_records {
        return Err(ERR_INVALID);
    }
    for _ in 0..count {
        let start = at;
        let tag = integer(bytes, &mut at)?;
        let length = usize::try_from(integer(bytes, &mut at)?).map_err(|_| ERR_INVALID)?;
        at = at
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or(ERR_INVALID)?;
        visit(tag, &bytes[start..at])?;
    }
    if at == bytes.len() {
        Ok(())
    } else {
        Err(ERR_INVALID)
    }
}

fn integer(bytes: &[u8], at: &mut usize) -> Result<u32, i32> {
    let mut value = 0;
    for shift in (0..=28).step_by(7) {
        let byte = *bytes.get(*at).ok_or(ERR_INVALID)?;
        *at += 1;
        if shift == 28 && byte > 15 {
            return Err(ERR_INVALID);
        }
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return if shift != 0 && byte == 0 {
                Err(ERR_INVALID)
            } else {
                Ok(value)
            };
        }
    }
    Err(ERR_INVALID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_unknown_tags_and_empty_batches_need_no_kernel_schema() {
        // One record with tag u32::MAX and three opaque bytes.
        let bytes = [1, 255, 255, 255, 255, 15, 3, 0x91, 0xff, 0];
        assert_eq!(validate_reply(&bytes, bytes.len(), 1), Ok(()));
        assert_eq!(validate_reply(&[0], 1, 1), Ok(()));
        assert_eq!(validate_reply(&[2, 0, 0, 1, 0], 5, 2), Ok(()));
        assert_eq!(validate_reply(&[2, 0, 0, 1, 0], 5, 1), Err(ERR_INVALID));
    }

    #[test]
    fn truncation_noncanonical_integers_and_trailing_bytes_are_refused() {
        let valid = [1, 0x80, 1, 2, 0xaa, 0xbb];
        for len in 0..valid.len() {
            assert_eq!(
                validate_reply(&valid[..len], valid.len(), 1),
                Err(ERR_INVALID)
            );
        }
        for invalid in [
            &[0, 0][..],
            &[0x80, 0][..],
            &[1, 0x80, 0, 0][..],
            &[1, 1, 0x80, 0][..],
            &[1, 255, 255, 255, 255, 16, 0][..],
            &[1, 0, 255, 255, 255, 255, 15][..],
            &[65][..],
        ] {
            assert_eq!(validate_reply(invalid, 3840, 64), Err(ERR_INVALID));
        }
        assert_eq!(validate_reply(&valid, valid.len() - 1, 1), Err(ERR_INVALID));
    }
}
