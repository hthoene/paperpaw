//! Bounded UTF-16 conversion for native format preparation callbacks.
use crate::utf16::Decoder;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    storage::{self, Root},
};

/// The conversion cache belongs to the host's document lease, not the plugin install.
pub const UTF8_CACHE: &str = "text.utf8";
const INPUT: usize = 1024;
const OUTPUT: usize = 1542;

/// Owned conversion state; no callback binding survives a call.
#[derive(Debug)]
pub struct Preparation {
    decoder: Decoder,
    offset: u64,
    size: u64,
    big: bool,
    started: bool,
    done: bool,
    failed: bool,
    input: Vec<u8>,
    output: Vec<u8>,
}

impl Preparation {
    /// Allocate bounded scratch before touching the conversion cache.
    /// The caller has checked the UTF-16 BOM and passes the complete file size.
    pub fn new(size: u64, big: bool) -> Result<Self, Error> {
        if size < 2 || size > u64::from(u32::MAX) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            decoder: Decoder::default(),
            offset: 2,
            size,
            big,
            started: false,
            done: false,
            failed: false,
            input: scratch(INPUT)?,
            output: scratch(OUTPUT)?,
        })
    }

    /// How far the conversion is, in millionths.
    #[must_use]
    pub fn progress_ppm(&self) -> u32 {
        u32::try_from(self.offset.saturating_mul(1_000_000) / self.size.max(1)).unwrap_or(1_000_000)
    }

    /// Convert at most 1 KiB per callback. The host owns the `.complete` stamp.
    /// An I/O refusal poisons this preparation: restart with a new instance,
    /// which truncates any partially appended cache before retrying.
    pub fn step(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        self.advance(
            |offset, out| storage::read_at(host, Root::Book, "", offset, out),
            |first, bytes| {
                if first {
                    storage::write(host, Root::DocumentCache, UTF8_CACHE, bytes)
                } else {
                    storage::append(host, Root::DocumentCache, UTF8_CACHE, bytes)
                }
            },
        )
    }

    /// One piece through any reader and writer (`write` gets `true` first).
    ///
    /// # Errors
    /// What `read` or `write` refuse; the preparation is then spent.
    pub fn advance(
        &mut self,
        mut read: impl FnMut(u64, &mut [u8]) -> Result<usize, Error>,
        mut write: impl FnMut(bool, &[u8]) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        if self.failed {
            return Err(Error::State);
        }
        if self.done {
            return Ok(true);
        }
        let result = self.piece(&mut read, &mut write);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn piece(
        &mut self,
        read: &mut impl FnMut(u64, &mut [u8]) -> Result<usize, Error>,
        write: &mut impl FnMut(bool, &[u8]) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let count = (self.size - self.offset).min(INPUT as u64) as usize;
        let n = if count == 0 {
            0
        } else {
            read(self.offset, &mut self.input[..count])?
        };
        if n > count || (n == 0 && count != 0) {
            return Err(Error::Io);
        }
        let next = self.offset + n as u64;
        let mut decoder = self.decoder;
        let progress = decoder.convert(
            &self.input[..n],
            &mut self.output,
            self.big,
            next == self.size,
        );
        // OUTPUT holds the worst-case expansion, including a pending surrogate.
        if progress.read != n {
            return Err(Error::NoSpace);
        }
        write(!self.started, &self.output[..progress.written])?;
        self.started = true;
        self.decoder = decoder;
        self.offset = next;
        self.done = progress.done;
        Ok(self.done)
    }
}

fn scratch(size: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    bytes.resize(size, 0);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::String, vec};

    #[test]
    fn short_reads_and_chunk_boundaries_preserve_text() {
        let text = "ab😀\n".repeat(700);
        for big in [false, true] {
            let mut source = vec![0; 2];
            for unit in text.encode_utf16() {
                source.extend_from_slice(&if big {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                });
            }
            source.push(0xab); // Legacy ignores an odd trailing byte.
            let mut state = Preparation::new(source.len() as u64, big).unwrap();
            let mut output = Vec::new();
            loop {
                let done = state
                    .advance(
                        |offset, buf| {
                            let n = buf.len().min(73);
                            buf[..n].copy_from_slice(&source[offset as usize..offset as usize + n]);
                            Ok(n)
                        },
                        |first, bytes| {
                            if first {
                                output.clear();
                            }
                            output.extend_from_slice(bytes);
                            Ok(())
                        },
                    )
                    .unwrap();
                if done {
                    break;
                }
            }
            assert_eq!(String::from_utf8(output).unwrap(), text);
        }
    }

    #[test]
    fn append_refusal_cannot_be_retried_on_a_partial_cache() {
        let mut state = Preparation::new(4, false).unwrap();
        let result = state.advance(
            |_, out| {
                out.copy_from_slice(&[65, 0]);
                Ok(2)
            },
            |_, _| Err(Error::Full),
        );
        assert_eq!(result, Err(Error::Full));
        assert_eq!(state.offset, 2);
        assert_eq!(
            state.advance(
                |_, _| panic!("must not read"),
                |_, _| panic!("must not append")
            ),
            Err(Error::State)
        );
    }
}
