//! Callback-bound, allocation-free reply output.
use super::Error;
use core::mem::MaybeUninit;

/// A writer over the reply buffer lent by the host for this event callback.
/// Only successfully written bytes count towards the callback result.
///
/// ```compile_fail
/// use paperpaw_sdk::native::reply::Reply;
/// fn escape<'a>(reply: Reply<'a>) -> Reply<'static> { reply }
/// ```
#[derive(Debug)]
pub struct Reply<'cb> {
    bytes: &'cb mut [MaybeUninit<u8>],
    written: usize,
}
impl<'cb> Reply<'cb> {
    pub(super) fn new(bytes: &'cb mut [MaybeUninit<u8>]) -> Self {
        Self { bytes, written: 0 }
    }

    /// Number of bytes already written.
    #[must_use]
    pub fn len(&self) -> usize {
        self.written
    }

    /// Whether nothing has been written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.written == 0
    }

    /// Bytes still available in the host buffer.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.written
    }

    /// Append bytes without allocating. The family defines the reply schema.
    ///
    /// # Errors
    /// Returns NoSpace without changing the output if the bytes do not fit.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > self.remaining() {
            return Err(Error::NoSpace);
        }
        let end = self.written + bytes.len();
        for (slot, byte) in self.bytes[self.written..end].iter_mut().zip(bytes) {
            slot.write(*byte);
        }
        self.written = end;
        Ok(())
    }

    /// Lets `encode` write straight into the rest of the host buffer (no copy
    /// through the stack) and counts the bytes it reports. The rest is zeroed
    /// first, so `encode` sees initialized bytes only.
    ///
    /// # Errors
    /// What `encode` returns; nothing is counted then, and the bytes written
    /// before stay as they were. `NoSpace` if `encode` reports more bytes than
    /// the rest holds.
    pub fn write_with(
        &mut self,
        encode: impl FnOnce(&mut [u8]) -> Result<usize, Error>,
    ) -> Result<(), Error> {
        let rest = &mut self.bytes[self.written..];
        for slot in rest.iter_mut() {
            slot.write(0);
        }
        // SAFETY: every slot of `rest` was initialized by the loop above, and
        // `MaybeUninit<u8>` has the layout of `u8`.
        let rest = unsafe { &mut *(core::ptr::from_mut(rest) as *mut [u8]) };
        let len = encode(rest)?;
        if len > rest.len() {
            return Err(Error::NoSpace);
        }
        self.written += len;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_with_encodes_in_place_and_a_refusal_counts_nothing() {
        let mut buffer = [MaybeUninit::new(9); 6];
        let mut reply = Reply::new(&mut buffer);
        reply.write(b"ab").unwrap();
        assert_eq!(
            reply.write_with(|out| {
                out[..2].copy_from_slice(b"cd");
                Err(Error::NoSpace)
            }),
            Err(Error::NoSpace)
        );
        assert_eq!(reply.len(), 2);
        assert_eq!(
            reply.write_with(|out| {
                assert_eq!(out.len(), 4);
                out[..3].copy_from_slice(b"xyz");
                Ok(3)
            }),
            Ok(())
        );
        assert_eq!(reply.write_with(|_| Ok(5)), Err(Error::NoSpace));
        assert_eq!((reply.len(), reply.remaining()), (5, 1));
        // SAFETY: all bytes began initialized and remain initialized.
        let bytes = buffer.map(|byte| unsafe { byte.assume_init() });
        assert_eq!(&bytes[..2], b"ab");
        assert_eq!(&bytes[2..5], b"xyz");
    }

    #[test]
    fn overflow_preserves_written_prefix_and_zero_capacity_refuses_data() {
        let mut buffer = [MaybeUninit::new(9); 3];
        let mut reply = Reply::new(&mut buffer);
        assert_eq!(reply.write(b"ab"), Ok(()));
        assert_eq!(reply.write(b"cd"), Err(Error::NoSpace));
        assert_eq!((reply.len(), reply.remaining()), (2, 1));
        assert_eq!(reply.write(b"c"), Ok(()));
        // SAFETY: all bytes began initialized and remain initialized.
        assert_eq!(buffer.map(|byte| unsafe { byte.assume_init() }), *b"abc");
        let mut empty = [];
        let mut reply = Reply::new(&mut empty);
        assert_eq!(reply.write(b""), Ok(()));
        assert!(reply.is_empty());
        assert_eq!(reply.write(b"x"), Err(Error::NoSpace));
    }
}
