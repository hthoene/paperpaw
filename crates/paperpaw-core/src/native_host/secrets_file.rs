//! The secrets file, read and written line by line (`docs/abi/secrets.md` 3).
//!
//! Interface 6's format: one line per secret, `owner.name<TAB>label<TAB>c?<TAB>value`,
//! tab, line break and backslash in the first two fields written `\t`, `\n`,
//! `\r`, `\\`. The value field is the veiled value in hex (interface 6) or `s1:`
//! and the sealed value in hex (decision 138). The file is never held whole: a
//! scan reads it through one [`READ_BUF`] buffer and hands over each line (at
//! most [`LINE_MAX`] bytes) in place; a longer line, and a line this host does
//! not understand, is handed over as it is and written back as it is.
//!
//! Every buffer is one fallible allocation made up front, wiped when dropped.
use alloc::vec::Vec;
use core::fmt;
use paperpaw_native::host_glue::{ERR_FULL, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE};
use paperpaw_platform::{Storage, StorageError};

/// The file, in the data folder of the card (the private folder on Android).
pub(super) const FILE: &str = ".paperpaw/secrets.txt";
/// The new file, until it replaces the old by rename.
pub(super) const TEMP: &str = ".paperpaw/secrets.new";
/// The one read buffer.
pub(super) const READ_BUF: usize = 4_096;
/// Longest line this host reads or writes, without its line break: the names,
/// the label and a 1,024-byte value sealed (about 2.5 KiB).
pub(super) const LINE_MAX: usize = 2_560;
/// Largest secret value, in bytes.
pub(super) const VALUE_MAX: usize = 1_024;
/// Largest sealed value (nonce, value, tag) the sealer is asked to open or make.
pub const SEALED_MAX: usize = 12 + VALUE_MAX + 16;
/// The file's size limit, in bytes.
pub(super) const FILE_MAX: u64 = 64 * 1024;
/// A name or an owner, in bytes.
pub(super) const NAME_MAX: usize = 64;
/// The marker of a sealed value.
pub(super) const SEALED_MARK: &[u8] = b"s1:";
/// Interface 6's veil, kept so its files read back (it is not encryption).
const VEIL: &[u8] = b"QuireInk keeps this out of sight, not safe";

pub(super) use super::secret_bytes::wipe;

/// A fixed buffer, one allocation, never grown, wiped when dropped. It may
/// hold a secret, so it never prints its content.
pub(super) struct Buf {
    data: Vec<u8>,
    len: usize,
}

impl fmt::Debug for Buf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Buf({} bytes)", self.len)
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        wipe(&mut self.data);
    }
}

impl Buf {
    pub(super) fn new(capacity: usize) -> Result<Self, i32> {
        let mut data = Vec::new();
        data.try_reserve_exact(capacity)
            .map_err(|_| ERR_NO_MEMORY)?;
        data.resize(capacity, 0);
        Ok(Self { data, len: 0 })
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }

    /// Forgets the content and wipes the whole buffer: a failed `open` of a
    /// sealer may have written plaintext past `len`.
    pub(super) fn clear(&mut self) {
        wipe(&mut self.data);
        self.len = 0;
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<(), i32> {
        let end = self.len.checked_add(bytes.len()).ok_or(ERR_NO_SPACE)?;
        self.data
            .get_mut(self.len..end)
            .ok_or(ERR_NO_SPACE)?
            .copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    fn push_hex(&mut self, bytes: &[u8]) -> Result<(), i32> {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes {
            self.push(&[
                DIGITS[usize::from(byte >> 4)],
                DIGITS[usize::from(byte & 15)],
            ])?;
        }
        Ok(())
    }

    fn push_escaped(&mut self, text: &str) -> Result<(), i32> {
        for byte in text.bytes() {
            match byte {
                b'\\' => self.push(b"\\\\")?,
                b'\t' => self.push(b"\\t")?,
                b'\n' => self.push(b"\\n")?,
                b'\r' => self.push(b"\\r")?,
                other => self.push(&[other])?,
            }
        }
        Ok(())
    }
}

pub(super) fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn is_hex(text: &[u8]) -> bool {
    text.len().is_multiple_of(2) && text.iter().all(|byte| nibble(*byte).is_some())
}

/// `text` (valid hex of even length) into `out`, which it replaces.
fn unhex_into(text: &[u8], out: &mut Buf) -> Result<(), i32> {
    out.clear();
    let mut pairs = text.iter();
    while let (Some(high), Some(low)) = (pairs.next(), pairs.next()) {
        let (Some(high), Some(low)) = (nibble(*high), nibble(*low)) else {
            return Err(ERR_IO);
        };
        out.push(&[(high << 4) | low])?;
    }
    Ok(())
}

pub(super) fn veil(bytes: &mut [u8]) {
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte ^= VEIL[index % VEIL.len()];
    }
}

/// Seals and opens values on a device that has a key (decision 138): the X4
/// (AES-256-GCM with a device key; Firmware's), tests (a fake). One that does
/// not [`seal`](Sealer::seals) writes nothing: `keep` is refused with `ERR_STATE`
/// (a device without a key has no `core.secrets`; veiled values of interface 6
/// are still read and are removed or rewritten sealed only where a sealer
/// seals).
///
/// `aad` is the line's owner, a zero byte and its name: a value copied to
/// another line does not open there. Nothing here allocates, logs or keeps the
/// value past the call.
///
/// Duties of an implementation that seals: every `seal` draws a **fresh
/// 12-byte nonce from a strong random source**, never a counter that restarts
/// and never one reused under the device key (a repeated nonce under AES-GCM
/// leaks the key stream and the authentication key). On the ESP32-C3 the
/// hardware RNG is only truly random while the radio or the bootloader's
/// entropy source is on; a sealer that cannot get such a nonce answers an
/// error from `seal` (the `keep` then fails with nothing changed) and never
/// falls back to a weak source.
pub trait Sealer {
    /// Whether new values are written sealed (`s1:`) and veiled ones found are
    /// rewritten sealed at the next save.
    fn seals(&self) -> bool;
    /// Seals `plain` into `out` (the nonce, the sealed value and the tag; at
    /// most [`SEALED_MAX`] bytes) and returns the length.
    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32>;
    /// Opens `sealed` into `out` and returns the length. Any error means the
    /// value is absent here (another device, a lost key, a damaged line).
    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32>;
}

/// What a read or write of the file failed with, as the contract says.
pub(super) fn status(error: StorageError) -> i32 {
    match error {
        StorageError::Full => ERR_FULL,
        StorageError::OutOfMemory => ERR_NO_MEMORY,
        _ => ERR_IO,
    }
}

/// One piece of the file.
pub(super) enum Piece<'a> {
    /// A whole line of at most [`LINE_MAX`] bytes, with its line break when the
    /// file has one.
    Line(&'a [u8]),
    /// A part of a longer line (without any line break). `end` marks the last.
    Opaque { bytes: &'a [u8], end: bool },
}

pub(super) enum Flow {
    Go,
    Stop,
}

/// Reads the file through `buf` (of [`READ_BUF`] bytes) and hands over each
/// line in place. A file that is not there is an empty one.
pub(super) fn scan(
    storage: &dyn Storage,
    path: &str,
    buf: &mut Buf,
    visit: &mut dyn FnMut(Piece<'_>) -> Result<Flow, i32>,
) -> Result<(), i32> {
    let (mut offset, mut start, mut end) = (0_u64, 0_usize, 0_usize);
    let (mut eof, mut opaque) = (false, false);
    loop {
        loop {
            let window = &buf.data[start..end];
            let Some(at) = window.iter().position(|byte| *byte == b'\n') else {
                if window.len() >= LINE_MAX {
                    let piece = Piece::Opaque {
                        bytes: window,
                        end: false,
                    };
                    if matches!(visit(piece)?, Flow::Stop) {
                        return Ok(());
                    }
                    opaque = true;
                    start = end;
                }
                break;
            };
            let piece = if opaque || at >= LINE_MAX {
                Piece::Opaque {
                    bytes: &window[..at],
                    end: true,
                }
            } else {
                Piece::Line(&window[..=at])
            };
            opaque = false;
            if matches!(visit(piece)?, Flow::Stop) {
                return Ok(());
            }
            start += at + 1;
        }
        if eof {
            let window = &buf.data[start..end];
            if opaque {
                let piece = Piece::Opaque {
                    bytes: window,
                    end: true,
                };
                visit(piece)?;
            } else if !window.is_empty() {
                visit(Piece::Line(window))?;
            }
            return Ok(());
        }
        if start > 0 {
            buf.data.copy_within(start..end, 0);
            end -= start;
            start = 0;
        }
        match storage.read_at(path, offset, &mut buf.data[end..]) {
            Ok(0) => eof = true,
            Ok(read) => {
                offset += read as u64;
                end += read;
            }
            Err(StorageError::NotFound) if offset == 0 => return Ok(()),
            Err(error) => return Err(status(error)),
        }
    }
}

/// The unescaped first fields of a line, so rows borrow from here.
pub(super) struct Scratch {
    id: Buf,
    label: Buf,
}

impl Scratch {
    pub(super) fn new() -> Result<Self, i32> {
        Ok(Self {
            id: Buf::new(2 * NAME_MAX * 2 + 2)?,
            label: Buf::new(NAME_MAX * 2)?,
        })
    }
}

/// How a line keeps its value.
#[derive(Clone, Copy)]
pub(super) enum Field<'a> {
    /// Interface 6's veiled value, in hex.
    Veiled(&'a [u8]),
    /// A sealed value (after the marker), in hex.
    Sealed(&'a [u8]),
}

/// A line this host understands.
#[derive(Clone, Copy)]
pub(super) struct Row<'a> {
    pub(super) owner: &'a str,
    pub(super) name: &'a str,
    pub(super) label: &'a str,
    pub(super) changeable: bool,
    pub(super) field: Field<'a>,
}

impl fmt::Debug for Row<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Row")
            .field("owner", &self.owner)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

fn unescape_into(text: &[u8], out: &mut Buf) -> Result<(), i32> {
    out.clear();
    let mut index = 0;
    while index < text.len() {
        let byte = text[index];
        let next = text.get(index + 1).copied();
        let escaped = match (byte, next) {
            (b'\\', Some(b'\\')) => Some(b'\\'),
            (b'\\', Some(b't')) => Some(b'\t'),
            (b'\\', Some(b'n')) => Some(b'\n'),
            (b'\\', Some(b'r')) => Some(b'\r'),
            _ => None,
        };
        match escaped {
            Some(byte) => {
                out.push(&[byte])?;
                index += 2;
            }
            None => {
                out.push(&[byte])?;
                index += 1;
            }
        }
    }
    Ok(())
}

/// No control character except tab, line break and carriage return (the file
/// escapes those): the rule for names and labels at the API.
pub(super) fn text_fine(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
}

/// Reads a line, or `None` for one this host does not understand (and keeps).
pub(super) fn parse<'a>(line: &'a [u8], scratch: &'a mut Scratch) -> Option<Row<'a>> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let mut fields = line.splitn(4, |byte| *byte == b'\t');
    let (id, label, flags, value) = (
        fields.next()?,
        fields.next()?,
        fields.next()?,
        fields.next()?,
    );
    if id.len() > scratch.id.data.len() || label.len() > scratch.label.data.len() {
        return None;
    }
    let changeable = match flags {
        b"c" => true,
        b"" => false,
        _ => return None,
    };
    let field = if let Some(sealed) = value.strip_prefix(SEALED_MARK) {
        (is_hex(sealed) && sealed.len() <= 2 * SEALED_MAX).then_some(Field::Sealed(sealed))?
    } else {
        (is_hex(value) && value.len() <= 2 * VALUE_MAX).then_some(Field::Veiled(value))?
    };
    unescape_into(id, &mut scratch.id).ok()?;
    unescape_into(label, &mut scratch.label).ok()?;
    let id = core::str::from_utf8(scratch.id.as_bytes()).ok()?;
    let label = core::str::from_utf8(scratch.label.as_bytes()).ok()?;
    let (owner, name) = id.split_once('.')?;
    // A line the API would refuse (too long, control characters) is one this
    // host does not understand: it is kept as it is and never listed.
    (!owner.is_empty()
        && !name.is_empty()
        && owner.len() <= NAME_MAX
        && name.len() <= NAME_MAX
        && label.len() <= NAME_MAX
        && !owner.chars().any(char::is_control)
        && text_fine(name)
        && text_fine(label))
    .then_some(Row {
        owner,
        name,
        label,
        changeable,
        field,
    })
}

/// What opening a value needs.
pub(super) struct Opener<'a> {
    pub(super) sealer: &'a dyn Sealer,
    pub(super) plain: Buf,
    pub(super) blob: Buf,
}

impl<'a> Opener<'a> {
    pub(super) fn new(sealer: &'a dyn Sealer) -> Result<Self, i32> {
        Ok(Self {
            sealer,
            plain: Buf::new(SEALED_MAX)?,
            blob: Buf::new(SEALED_MAX)?,
        })
    }

    /// The value of `row` in `self.plain`, or `false` when it is absent here
    /// (a sealed value that does not open, a veiled one that is not text).
    pub(super) fn open(&mut self, row: &Row<'_>) -> bool {
        self.plain.clear();
        self.blob.clear();
        let opened = match row.field {
            Field::Veiled(hex) => unhex_into(hex, &mut self.plain).map(|()| {
                let len = self.plain.len;
                veil(&mut self.plain.data[..len]);
            }),
            Field::Sealed(hex) => unhex_into(hex, &mut self.blob).and_then(|()| {
                let mut aad = [0_u8; 2 * NAME_MAX + 1];
                let aad = aad_of(row.owner, row.name, &mut aad)?;
                let length = self
                    .sealer
                    .open(aad, self.blob.as_bytes(), &mut self.plain.data)
                    .map_err(|_| ERR_IO)?;
                if length > VALUE_MAX {
                    return Err(ERR_IO);
                }
                self.plain.len = length;
                Ok(())
            }),
        };
        self.blob.clear();
        let text = opened.is_ok()
            && self.plain.len <= VALUE_MAX
            && core::str::from_utf8(self.plain.as_bytes()).is_ok();
        if !text {
            self.plain.clear();
        }
        text
    }

    pub(super) fn value(&self) -> &str {
        core::str::from_utf8(self.plain.as_bytes()).unwrap_or_default()
    }
}

pub(super) fn aad_of<'b>(
    owner: &str,
    name: &str,
    out: &'b mut [u8; 2 * NAME_MAX + 1],
) -> Result<&'b [u8], i32> {
    let total = owner.len() + 1 + name.len();
    if owner.len() > NAME_MAX || name.len() > NAME_MAX {
        return Err(ERR_IO);
    }
    out[..owner.len()].copy_from_slice(owner.as_bytes());
    out[owner.len()] = 0;
    out[owner.len() + 1..total].copy_from_slice(name.as_bytes());
    Ok(&out[..total])
}

/// Writes a line with this value into `line` (replacing it), sealed. A sealer
/// that does not seal is refused with `ERR_STATE`: a value is never written
/// veiled or in the clear (decision 140). `blob` is scratch of [`SEALED_MAX`]
/// bytes.
pub(super) fn build(
    line: &mut Buf,
    blob: &mut Buf,
    sealer: &dyn Sealer,
    row: (&str, &str, &str, bool),
    value: &[u8],
) -> Result<(), i32> {
    let (owner, name, label, changeable) = row;
    if !sealer.seals() {
        return Err(ERR_STATE);
    }
    line.clear();
    line.push_escaped(owner)?;
    line.push(b".")?;
    line.push_escaped(name)?;
    line.push(b"\t")?;
    line.push_escaped(label)?;
    line.push(if changeable { b"\tc\t" } else { b"\t\t" })?;
    blob.clear();
    let mut aad = [0_u8; 2 * NAME_MAX + 1];
    let aad = aad_of(owner, name, &mut aad)?;
    let length = sealer.seal(aad, value, &mut blob.data)?;
    if length > SEALED_MAX {
        return Err(ERR_IO);
    }
    blob.len = length;
    line.push(SEALED_MARK)?;
    line.push_hex(blob.as_bytes())?;
    blob.clear();
    line.push(b"\n")
}
