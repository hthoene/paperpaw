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
}

#[cfg(test)]
mod tests {
    use super::*;
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
