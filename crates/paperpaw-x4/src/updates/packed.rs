//! Packed update files: the image deflated, inflated again while it is
//! written (#304), so that an update downloads 3.8 MB instead of 6.0.
//!
//! A packed update is an ordinary update file (`paperpaw_update`): an
//! "image" and PaperPaw's 128-byte trailer, signed as any other. What the
//! signature covers here is the *container*, which holds the real image:
//!
//! | Bytes | What |
//! |-------|------|
//! | 0..`stored` | the image's first `stored` bytes as they are, but for the first byte, which is `0` (the ESP image's `0xE9` is put back on the way out) |
//! | `stored`..end − 16 | the rest of the image as a raw deflate stream (window 32 KiB) |
//! | end − 16..end | `PPackedZ`, the image's length and `stored` (little-endian, 4 bytes each) |
//!
//! Signed with the container, a damaged or changed file fails the signature
//! as any other, and a signature on the container fixes the image it
//! unpacks to: inflating is deterministic. The first byte makes the file
//! no app image to a firmware that does not know the format (Beta 3 and
//! older refuse it as "no firmware image" instead of installing a
//! container the boot loader cannot start); the footer is inside what is
//! signed, so nothing that says "packed" can be changed unseen. The raw
//! update file stays what Beta 3 installs.
//!
//! [`Source`] hands out the image's bytes in order, from either kind of
//! file, and gives the signature's check the container's bytes as they are
//! read: the check is the same either way.

use alloc::boxed::Box;
use alloc::vec::Vec;

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};
use paperpaw_platform::Storage;
use paperpaw_sdk::firmware::APP_HEAD_LEN;

/// The footer's first bytes.
pub const MAGIC: [u8; 8] = *b"PPackedZ";
/// The footer's length.
pub const FOOTER_LEN: usize = 16;
/// The first byte of an app image, which the container keeps as `0`.
pub const IMAGE_MAGIC: u8 = 0xE9;
/// The deflate history: the stream's window, and the one large piece an
/// unpacking takes (a power of two: the window wraps).
pub const WINDOW: usize = 32 * 1024;
/// The compressed data is read from the card this much at a time.
const INPUT: usize = 4096;

/// Why the image's bytes could not be had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    /// The card did not give what was asked.
    Card,
    /// The container is not what its footer says: the deflate data is
    /// damaged, too short or too long.
    Damaged,
    /// There is no memory for the window (nothing was written).
    NoMemory,
}

/// What the footer says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footer {
    /// The image's length once unpacked.
    pub raw_len: u32,
    /// The first bytes of the image kept as they are.
    pub stored: u32,
}

impl Footer {
    /// The footer in `bytes`, for a container of `container_len` bytes (the
    /// footer's included); `None` where these are not a packed file's.
    #[must_use]
    pub fn read(bytes: &[u8], container_len: u32) -> Option<Self> {
        if bytes.len() != FOOTER_LEN || bytes[..8] != MAGIC {
            return None;
        }
        let word = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let (raw_len, stored) = (word(8), word(12));
        let head = u32::try_from(APP_HEAD_LEN).ok()?;
        let fits = stored >= head
            && stored <= raw_len
            && u64::from(stored) + FOOTER_LEN as u64 <= u64::from(container_len);
        fits.then_some(Self { raw_len, stored })
    }
}

impl Footer {
    /// The footer's bytes.
    #[must_use]
    pub fn to_bytes(self) -> [u8; FOOTER_LEN] {
        let mut bytes = [0; FOOTER_LEN];
        bytes[..8].copy_from_slice(&MAGIC);
        bytes[8..12].copy_from_slice(&self.raw_len.to_le_bytes());
        bytes[12..].copy_from_slice(&self.stored.to_le_bytes());
        bytes
    }
}

/// The container of `image` (see the [module documentation](self)): its first
/// `stored` bytes as they are but for the first, the rest deflated (level
/// `level`, 0 to 10), and the footer. What release tooling signs for a
/// packed update (never called by the firmware: the linker drops it); `None` where `image` is too short or `stored` does not
/// fit it.
#[must_use]
pub fn pack(image: &[u8], stored: usize, level: u8) -> Option<Vec<u8>> {
    let raw_len = u32::try_from(image.len()).ok()?;
    if stored < APP_HEAD_LEN || stored > image.len() || *image.first()? != IMAGE_MAGIC {
        return None;
    }
    let footer = Footer {
        raw_len,
        stored: u32::try_from(stored).ok()?,
    };
    let mut container = image[..stored].to_vec();
    container[0] = 0;
    container.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(
        &image[stored..],
        level,
    ));
    container.extend_from_slice(&footer.to_bytes());
    Some(container)
}

/// The image's bytes in order, from a plain or a packed update file.
#[derive(Debug)]
pub struct Source {
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    /// The file's bytes are the image's, from `at`.
    Plain {
        at: u64,
    },
    Packed(Box<Packed>),
}

struct Packed {
    footer: Footer,
    /// Where the deflate data ends: the footer begins.
    end: u32,
    /// Bytes of the container read from the card so far.
    at: u32,
    /// Bytes of the image handed out so far.
    produced: u32,
    decoder: Vec<DecompressorOxide>,
    window: Vec<u8>,
    input: Vec<u8>,
    /// Compressed bytes read and not inflated yet: `input[start..stop]`.
    start: usize,
    stop: usize,
    /// Where the window is written next.
    position: usize,
    /// Inflated bytes at hand: `left` from `ready` in the window.
    ready: usize,
    left: usize,
    /// The deflate stream ended.
    done: bool,
    /// The footer's bytes were given to the check.
    footer_taken: bool,
}

impl core::fmt::Debug for Packed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Packed")
            .field("footer", &self.footer)
            .field("produced", &self.produced)
            .finish_non_exhaustive()
    }
}

/// `len` zeroed bytes, or none where there is no room.
fn zeroed(len: usize) -> Result<Vec<u8>, Fail> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Fail::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

impl Source {
    /// A plain file, whose bytes are the image's.
    #[must_use]
    pub fn plain() -> Self {
        Self {
            kind: Kind::Plain { at: 0 },
        }
    }

    /// A packed file of `container_len` bytes (without the trailer) as its
    /// `footer` says. The window and the decoder are taken now, fallibly,
    /// before anything is written (the largest first).
    pub fn packed(footer: Footer, container_len: u32) -> Result<Self, Fail> {
        let window = zeroed(WINDOW)?;
        let mut decoder: Vec<DecompressorOxide> = Vec::new();
        decoder.try_reserve_exact(1).map_err(|_| Fail::NoMemory)?;
        // Made in place: as a value its 11 KB would be on the stack.
        decoder.resize_with(1, DecompressorOxide::default);
        let input = zeroed(INPUT)?;
        Ok(Self {
            kind: Kind::Packed(Box::new(Packed {
                footer,
                end: container_len - FOOTER_LEN as u32,
                at: footer.stored,
                produced: 0,
                decoder,
                window,
                input,
                start: 0,
                stop: 0,
                position: 0,
                ready: 0,
                left: 0,
                done: false,
                footer_taken: false,
            })),
        })
    }

    /// Fills `out` with the image's next bytes from the file at `path`; each
    /// byte of the file as it is read is given to `take` (what the
    /// signature covers), in order. The last call, the one that reaches the
    /// image's end, also reads the footer and ends the deflate stream, so
    /// that `take` has had the whole file then.
    pub fn read(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        out: &mut [u8],
        take: &mut dyn FnMut(&[u8]),
    ) -> Result<(), Fail> {
        match &mut self.kind {
            Kind::Plain { at } => {
                read_exact(storage, path, *at, out)?;
                take(out);
                *at += out.len() as u64;
                Ok(())
            }
            Kind::Packed(packed) => packed.read(storage, path, out, take),
        }
    }
}

fn read_exact(storage: &dyn Storage, path: &str, at: u64, buf: &mut [u8]) -> Result<(), Fail> {
    match storage.read_at(path, at, buf) {
        Ok(n) if n == buf.len() => Ok(()),
        _ => Err(Fail::Card),
    }
}

impl Packed {
    fn read(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        out: &mut [u8],
        take: &mut dyn FnMut(&[u8]),
    ) -> Result<(), Fail> {
        let mut filled = 0;
        while filled < out.len() {
            if self.produced >= self.footer.raw_len {
                return Err(Fail::Damaged);
            }
            let wanted = out.len() - filled;
            if self.produced < self.footer.stored {
                // The image's first bytes, as they are in the file.
                let n = wanted.min((self.footer.stored - self.produced) as usize);
                let part = &mut out[filled..filled + n];
                read_exact(storage, path, u64::from(self.produced), part)?;
                take(part);
                if self.produced == 0 {
                    part[0] = IMAGE_MAGIC;
                }
                self.produced += n as u32;
                filled += n;
                continue;
            }
            if self.left == 0 {
                if self.done {
                    return Err(Fail::Damaged);
                }
                self.inflate(storage, path, take)?;
                continue;
            }
            let n = wanted
                .min(self.left)
                .min((self.footer.raw_len - self.produced) as usize);
            out[filled..filled + n].copy_from_slice(&self.window[self.ready..self.ready + n]);
            self.ready += n;
            self.left -= n;
            self.produced += n as u32;
            filled += n;
        }
        if self.produced == self.footer.raw_len {
            self.finish(storage, path, take)?;
        }
        Ok(())
    }

    /// The deflate stream's next piece into the window.
    fn inflate(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        take: &mut dyn FnMut(&[u8]),
    ) -> Result<(), Fail> {
        if self.start == self.stop && self.at < self.end {
            let n = INPUT.min((self.end - self.at) as usize);
            read_exact(storage, path, u64::from(self.at), &mut self.input[..n])?;
            take(&self.input[..n]);
            self.at += n as u32;
            self.start = 0;
            self.stop = n;
        }
        let more = self.at < self.end;
        let flags = if more {
            inflate_flags::TINFL_FLAG_HAS_MORE_INPUT
        } else {
            0
        };
        let (status, used, written) = decompress(
            &mut self.decoder[0],
            &self.input[self.start..self.stop],
            &mut self.window,
            self.position,
            flags,
        );
        self.start += used;
        (self.ready, self.left) = (self.position, written);
        self.position = (self.position + written) & (WINDOW - 1);
        match status {
            TINFLStatus::Done => self.done = true,
            TINFLStatus::HasMoreOutput => {}
            TINFLStatus::NeedsMoreInput if more || self.start < self.stop => {}
            _ => return Err(Fail::Damaged),
        }
        Ok(())
    }

    /// The image is all out: the stream must end where the footer begins,
    /// and the footer is given to the check.
    fn finish(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        take: &mut dyn FnMut(&[u8]),
    ) -> Result<(), Fail> {
        if self.footer_taken {
            return Ok(());
        }
        while !self.done {
            self.inflate(storage, path, take)?;
            if self.left > 0 {
                return Err(Fail::Damaged);
            }
        }
        if self.left > 0 || self.start != self.stop || self.at != self.end {
            return Err(Fail::Damaged);
        }
        let mut footer = [0; FOOTER_LEN];
        read_exact(storage, path, u64::from(self.end), &mut footer)?;
        take(&footer);
        self.footer_taken = true;
        Ok(())
    }
}
