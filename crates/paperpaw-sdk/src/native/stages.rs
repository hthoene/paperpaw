//! Synchronous extension stages with one borrowed reply buffer.
use super::{
    Error, Host,
    declare::records_wire::{self as wire, Record},
};
use paperpaw_native::host_glue::{ERR_NOT_FOUND, PpBuffer, PpBytes};

/// A callback-bound extension stage owned by the calling plugin.
#[derive(Debug)]
pub struct Stage<'cb> {
    host: &'cb Host<'cb>,
    id: &'cb str,
}

impl<'cb> Stage<'cb> {
    /// Bind a full stage id, for example `reader.insert`, to this callback.
    /// The host checks ownership when the stage runs.
    ///
    /// # Errors
    /// Invalid for ids over 128 bytes or empty dot-separated segments, or bytes
    /// outside `[a-z0-9-]` in a segment. Construction makes no host call.
    pub fn new(host: &'cb Host<'cb>, full_id: &'cb str) -> Result<Self, Error> {
        if full_id.len() > wire::STAGE_CALL_STAGE_MAX_BYTES
            || !full_id.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        {
            return Err(Error::Invalid);
        }
        Ok(Self { host, id: full_id })
    }

    /// Bytes required for a largest reply with a 64-byte subscriber id.
    /// The declared `max_reply` is in 1..=3840. Oversized arithmetic saturates.
    #[must_use]
    pub const fn out_len(max_reply: u32) -> usize {
        let body = 86_usize
            .saturating_add(leb_len(max_reply as usize))
            .saturating_add(max_reply as usize);
        2_usize.saturating_add(leb_len(body)).saturating_add(body)
    }

    /// Run subscribers in host order, lending `out` to each reply in turn.
    /// Payloads use the stage owner's schema and are never decoded here.
    pub fn run<'r>(&'r self, input: &'r [u8], out: &'r mut [u8]) -> StageRun<'r> {
        StageRun {
            host: self.host,
            id: self.id,
            input,
            out,
            cursor: 0,
            finished: false,
        }
    }
}

const fn leb_len(mut n: usize) -> usize {
    let mut len = 1;
    while n >= 128 {
        n >>= 7;
        len += 1;
    }
    len
}

/// A lending cursor over subscribers. Each reply borrows the reused buffer.
#[derive(Debug)]
pub struct StageRun<'r> {
    host: &'r Host<'r>,
    id: &'r str,
    input: &'r [u8],
    out: &'r mut [u8],
    cursor: u32,
    finished: bool,
}
impl StageRun<'_> {
    /// Fetch one reply. End-of-stage and any refusal permanently finish the cursor.
    ///
    /// # Errors
    /// Returns the host refusal or an invalid reply envelope. Subscriber failures
    /// are exposed through [`StageReply::status`] and do not stop the cursor.
    #[allow(
        clippy::should_implement_trait,
        reason = "lending replies cannot implement Iterator"
    )]
    pub fn next(&mut self) -> Result<Option<StageReply<'_>>, Error> {
        if self.finished {
            return Ok(None);
        }
        // Fuse before the call: a refused or malformed reply must not retry a subscriber.
        self.finished = true;
        // SAFETY: the callback binding and borrowed input are live; output is exclusive.
        let n = unsafe {
            (self.host.table.stage_run)(
                self.host.ctx,
                PpBytes {
                    ptr: self.id.as_ptr(),
                    len: self.id.len(),
                },
                self.cursor,
                PpBytes {
                    ptr: self.input.as_ptr(),
                    len: self.input.len(),
                },
                PpBuffer {
                    ptr: self.out.as_mut_ptr(),
                    cap: self.out.len(),
                },
            )
        };
        if n == ERR_NOT_FOUND {
            return Ok(None);
        }
        let n = usize::try_from(n).map_err(|_| Error::from_status(n))?;
        let bytes = self.out.get(..n).ok_or(Error::Invalid)?;
        let Record::StageReply(reply) =
            wire::record_decode_one(bytes).map_err(Error::from_status)?
        else {
            return Err(Error::Invalid);
        };
        if reply.index != self.cursor || (reply.status < 0 && !reply.payload.is_empty()) {
            return Err(Error::Invalid);
        }
        if let Some(next) = self.cursor.checked_add(1) {
            self.cursor = next;
            self.finished = false;
        }
        Ok(Some(StageReply { reply }))
    }
}

/// A host-stamped subscriber envelope borrowing the cursor's reply buffer.
#[derive(Clone, Copy, Debug)]
pub struct StageReply<'o> {
    reply: wire::StageReply<'o>,
}
impl<'o> StageReply<'o> {
    /// The subscriber plugin id.
    #[must_use]
    pub fn subscriber(&self) -> &'o str {
        self.reply.subscriber
    }
    /// The cursor index echoed by the host.
    #[must_use]
    pub fn index(&self) -> u32 {
        self.reply.index
    }
    /// The subscriber's result. Nonnegative statuses are successful.
    ///
    /// # Errors
    /// Returns cancellation, busy, invalid framing or the subscriber's refusal.
    pub fn status(&self) -> Result<(), Error> {
        if self.reply.status < 0 {
            Err(Error::from_status(self.reply.status))
        } else {
            Ok(())
        }
    }
    /// Whether the host granted the subscriber permission to override user style.
    #[must_use]
    pub fn style_granted(&self) -> bool {
        self.reply.flags & 1 != 0
    }
    /// The signature declared by this subscriber.
    #[must_use]
    pub fn signature(&self) -> u64 {
        self.reply.signature
    }
    /// Opaque payload in the stage owner's schema, empty on subscriber failure.
    #[must_use]
    pub fn payload(&self) -> &'o [u8] {
        self.reply.payload
    }
}

#[cfg(test)]
#[path = "stages_tests.rs"]
mod tests;
