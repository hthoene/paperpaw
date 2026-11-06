//! The X4's plugin file for interface 7 (format 5), as `abi/ppn5.md` proposes
//! it: a reader of the 96-byte header and every refusal that needs no more
//! than the header, the file's length and the entry the loader has
//! relocated. Nothing here loads, relocates or runs a plugin, and no
//! firmware calls it yet (slice 1 of #787).
//!
//! The order is the loader's (ADR 0034 section 6, steps 1 to 7 of the
//! note): magic and length, format, chip, interface major, needed minor,
//! reserved fields, then the parts. A refusal by any of the first 16 bytes
//! comes before any other field is read. The digest and the signature
//! (step 8) are fed by the caller with [`Header5::digest`] and
//! [`Header5::holds`], as for format 4. After relocation and before `make`
//! the caller gives [`Header5::check_entry`] what it found (step 9).
//!
//! Only the format's own numbers are here; those of the generated metadata
//! record (`abi/generated/metadata-layout.json`) reach this module through
//! [`RecordFacts`], read by the generated bounded reader, never by offsets
//! typed here.

use paperpaw_digest::sha256::Sha256;

use crate::header::{CODE_OFFSET, Chip, MAGIC, MAX_IMAGE_LEN};
use crate::index::HOT_MAX;
use crate::signature::SIGNATURE_LEN;

/// The format this module reads.
pub const FORMAT: u16 = 5;
/// The header's length.
pub const HEADER_LEN: usize = 96;
/// The bytes of the header its digest covers (all before the digest).
const DIGESTED_LEN: usize = 64;
/// The most the metadata record may be (`max_bytes` of its descriptor).
pub const META_MAX: u32 = 2048;
/// The interface major this firmware family speaks.
pub const ABI_MAJOR: u16 = 7;
/// The entry's length on the ESP32-C3 (profile `esp32c3-riscv32imc-ilp32-soft-le`).
pub const X4_ENTRY_BYTES: u32 = 20;

/// What the host (the firmware) is, to compare a file with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    /// The chip it runs on. Only the ESP32-C3 has a profile (`abi/entry.md`).
    pub target: Chip,
    /// The interface major it speaks.
    pub abi_major: u16,
    /// Its minor: the newest it knows.
    pub minor: u16,
    /// The length of the entry in its target profile.
    pub entry_bytes: u32,
}

impl Host {
    /// The X4 (ESP32-C3) at interface 7, minor `minor`.
    pub const fn x4(minor: u16) -> Host {
        Host {
            target: Chip::Esp32C3,
            abi_major: ABI_MAJOR,
            minor,
            entry_bytes: X4_ENTRY_BYTES,
        }
    }
}

/// Why a file is refused, in the order of the checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// Too short to say, or not a plugin file.
    NotAPlugin,
    /// It has the magic but not the 96 bytes of the header.
    Truncated,
    /// A format below 5: built for an older PaperPaw.
    OlderFormat,
    /// A format above 5: needs a newer PaperPaw.
    NewerFormat,
    /// A chip this build does not know.
    UnknownChip,
    /// A chip for which this host has no profile (the X4 Pro's until an
    /// Xtensa profile exists).
    NoProfile,
    /// Made for another chip than the host's.
    OtherChip,
    /// Built for an older interface major.
    OlderInterface,
    /// Built for a newer interface major: needs a newer PaperPaw.
    NewerInterface,
    /// Needs a newer minor than the host's: needs a newer PaperPaw.
    NeedsNewerMinor,
    /// `flags` or the reserved bytes are set: a later format's, refused as a
    /// newer format.
    Reserved,
    /// The parts do not lie where they can; the string names the rule.
    Malformed(&'static str),
    /// The file is not as long as its header says (with or without a
    /// signature).
    Length,
}

/// Why the entry a loader relocated is refused (step 9). No plugin code has
/// run when one of these is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EntryRefusal {
    /// A word to relocate does not lie in `[base, base + image_len)`.
    RelocOutside,
    /// The metadata record disagrees with the header (`abi_major`,
    /// `needs_minor`, or the entry length of the profile).
    RecordDisagrees,
    /// The record is not `meta_len` bytes.
    RecordLength,
    /// The entry's static metadata view is not at `base + meta_at`, or not
    /// `meta_len` long.
    MetadataView,
    /// The entry's static metadata view differs from the record.
    MetadataBytes,
    /// `entry.size` is not the profile's entry length.
    EntrySize,
    /// The entry's `abi_major` or `needs_minor` is not the header's.
    EntryVersion,
    /// `make` is not in `[code_at, image_len)`, or not 2-aligned.
    Make,
}

/// What the generated reader found in the metadata record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordFacts {
    pub abi_major: u16,
    pub needs_minor: u16,
    /// The entry length the record's profile says.
    pub entry_bytes: u32,
}

impl RecordFacts {
    /// The facts of the metadata record `record` (the bytes at `meta_at`),
    /// read from its fixed prefix with every bound checked: `None` if it is
    /// not a record this host reads (wrong magic or layout, a prefix or a
    /// total that is not the record's, reserved flags set, an entry layout
    /// that is not the first).
    ///
    /// This reads the prefix only, the part step 9 compares with the header.
    /// The body (target, exports, services, features) is checked by the
    /// generated bounded reader once Kernel's lands (`abi/metadata.md`,
    /// "Remaining qualification"); the offsets are the descriptor's
    /// (`abi/generated/metadata-layout.json`, `prefix`), pinned by a test
    /// against the generated fixture, and nothing else is read from here.
    pub fn read(record: &[u8]) -> Option<RecordFacts> {
        const MAGIC: [u8; 4] = *b"PPAM";
        const LAYOUT: u16 = 2;
        const PREFIX_BYTES: u16 = 32;
        const ENTRY_LAYOUT: u16 = 1;
        if record.len() < PREFIX_BYTES as usize || record.len() > META_MAX as usize {
            return None;
        }
        let ok = record[..4] == MAGIC
            && le16(record, 4) == LAYOUT
            && le16(record, 6) == PREFIX_BYTES
            && le32(record, 8) as usize == record.len()
            && le16(record, 16) == ENTRY_LAYOUT
            && le32(record, 20) == 0;
        ok.then(|| RecordFacts {
            abi_major: le16(record, 12),
            needs_minor: le16(record, 14),
            entry_bytes: u32::from(le16(record, 18)),
        })
    }
}

/// The entry as the relocated image holds it, before anything is checked:
/// the fields of `PpEntry` on the ESP32-C3 (`abi/generated/paperpaw_abi.rs`,
/// profile `esp32c3-riscv32imc-ilp32-soft-le`, [`X4_ENTRY_BYTES`] long).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEntry {
    pub size: u32,
    pub abi_major: u16,
    pub needs_minor: u16,
    pub metadata_ptr: u32,
    pub metadata_len: u32,
    /// `make`, an instruction address.
    pub make: u32,
}

impl RawEntry {
    /// The entry in `bytes` (little-endian fields in declaration order),
    /// or `None` if it is shorter than the profile's entry.
    pub fn read(bytes: &[u8]) -> Option<RawEntry> {
        let bytes = bytes.get(..X4_ENTRY_BYTES as usize)?;
        Some(RawEntry {
            size: le32(bytes, 0),
            abi_major: le16(bytes, 4),
            needs_minor: le16(bytes, 6),
            metadata_ptr: le32(bytes, 8),
            metadata_len: le32(bytes, 12),
            make: le32(bytes, 16),
        })
    }
}

/// What the loader found in the entry once the image is relocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryFacts<'a> {
    /// `entry.size`.
    pub size: u32,
    pub abi_major: u16,
    pub needs_minor: u16,
    /// The data address `entry.metadata.ptr` holds after relocation. A
    /// relocated pointer is `placed_base + meta_at`, with the base the
    /// image was placed at, not the base it was linked at.
    pub metadata_ptr: u32,
    /// `entry.metadata.len`.
    pub metadata_len: u32,
    /// The bytes that view shows.
    pub metadata: &'a [u8],
    /// Where `entry.make` points, as an offset into the image. After
    /// relocation `entry.make` is a code address: in the flash image it
    /// is `placed_base + CODE_OFFSET + offset`, and the loader takes both
    /// away. If `make` lies in the hot code (copied to the IRAM arena at
    /// `placement.hot`), the loader maps it back first:
    /// `offset = hot_at + (make - placement.hot)`, an offset in the
    /// image like any other. An address that fits neither view is no
    /// offset: the loader passes a value outside `[code_at, image_len)`
    /// (for example `u32::MAX`) and the check refuses it.
    pub make_at: u32,
}

/// A format-5 header, read and checked to hold together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header5 {
    pub target: Chip,
    pub abi_major: u16,
    pub needs_minor: u16,
    /// The data address the image is linked at.
    pub base: u32,
    /// The image's bytes: read-only data, then code.
    pub image_len: u32,
    pub code_at: u32,
    /// Where the entry lies in the image.
    pub entry: u32,
    /// Where the metadata record lies in the image.
    pub meta_at: u32,
    pub meta_len: u32,
    /// How many words are to relocate.
    pub relocs: u32,
    pub align: u32,
    pub hot_at: u32,
    pub hot_len: u32,
    pub patches: u32,
    /// SHA-256 of the header's first 64 bytes and of all after the header.
    pub sha256: [u8; 32],
    /// The file ends with an author's signature.
    pub signed: bool,
}

impl Header5 {
    /// The header at the start of `head` (at least [`HEADER_LEN`] bytes),
    /// for a file of `file_len` bytes, checked against `host`.
    pub fn read(head: &[u8], file_len: u64, host: &Host) -> Result<Header5, Refusal> {
        if head.len() < 4 || head[..4] != MAGIC {
            return Err(Refusal::NotAPlugin);
        }
        let Some(head) = head.get(..HEADER_LEN) else {
            return Err(Refusal::Truncated);
        };
        match le16(head, 4) {
            FORMAT => {}
            format if format < FORMAT => return Err(Refusal::OlderFormat),
            _ => return Err(Refusal::NewerFormat),
        }
        let target = Chip::from_id(le16(head, 6)).ok_or(Refusal::UnknownChip)?;
        if host.target != Chip::Esp32C3 {
            return Err(Refusal::NoProfile);
        }
        if target != host.target {
            return Err(if target == Chip::Esp32S3 {
                Refusal::NoProfile
            } else {
                Refusal::OtherChip
            });
        }
        let abi_major = le16(head, 8);
        if abi_major < host.abi_major {
            return Err(Refusal::OlderInterface);
        }
        if abi_major > host.abi_major {
            return Err(Refusal::NewerInterface);
        }
        let needs_minor = le16(head, 10);
        if needs_minor > host.minor {
            return Err(Refusal::NeedsNewerMinor);
        }
        if le32(head, 12) != 0 || le32(head, 60) != 0 {
            return Err(Refusal::Reserved);
        }
        let mut sha256 = [0; 32];
        sha256.copy_from_slice(&head[DIGESTED_LEN..HEADER_LEN]);
        let mut header = Header5 {
            target,
            abi_major,
            needs_minor,
            base: le32(head, 16),
            image_len: le32(head, 20),
            code_at: le32(head, 24),
            entry: le32(head, 28),
            meta_at: le32(head, 32),
            meta_len: le32(head, 36),
            relocs: le32(head, 40),
            align: le32(head, 44),
            hot_at: le32(head, 48),
            hot_len: le32(head, 52),
            patches: le32(head, 56),
            sha256,
            signed: false,
        };
        header.holds_together(host)?;
        header.signed = header.signed_for(file_len)?;
        Ok(header)
    }

    /// The parts lie where they can (step 7 of the note), everything
    /// checked before the arithmetic that needs it.
    fn holds_together(&self, host: &Host) -> Result<(), Refusal> {
        let bad = Refusal::Malformed;
        if self.image_len == 0 || self.image_len > MAX_IMAGE_LEN {
            return Err(bad("image_len is 0 or above MAX_IMAGE_LEN"));
        }
        if self.code_at > self.image_len || !self.code_at.is_multiple_of(4) {
            return Err(bad("code_at is beyond the image or not 4-aligned"));
        }
        if !self.align.is_power_of_two() || !(4..=4096).contains(&self.align) {
            return Err(bad("align is not a power of two in 4..=4096"));
        }
        if !self.base.is_multiple_of(self.align) {
            return Err(bad("base is not a multiple of align"));
        }
        if self
            .base
            .checked_add(self.image_len)
            .and_then(|end| end.checked_add(CODE_OFFSET))
            .is_none()
        {
            return Err(bad("the image does not fit the address space"));
        }
        if self.meta_len == 0 || self.meta_len > META_MAX {
            return Err(bad("meta_len is 0 or above META_MAX"));
        }
        if !self.meta_at.is_multiple_of(4)
            || self
                .meta_at
                .checked_add(self.meta_len)
                .is_none_or(|end| end > self.code_at)
        {
            return Err(bad(
                "the metadata record is not 4-aligned in the read-only data",
            ));
        }
        if !self.entry.is_multiple_of(8)
            || self
                .entry
                .checked_add(host.entry_bytes)
                .is_none_or(|end| end > self.code_at)
        {
            return Err(bad("the entry is not 8-aligned in the read-only data"));
        }
        if self.relocs > self.image_len / 4 {
            return Err(bad("more words to relocate than the image holds"));
        }
        if self.hot_len == 0 {
            if self.hot_at != 0 {
                return Err(bad("hot_at is set without hot code"));
            }
        } else if self.hot_at < self.code_at
            || !self.hot_at.is_multiple_of(4)
            || !self.hot_len.is_multiple_of(4)
            || self.hot_len > HOT_MAX
            || self
                .hot_at
                .checked_add(self.hot_len)
                .is_none_or(|end| end > self.image_len)
        {
            return Err(bad("the hot code lies outside the code or above HOT_MAX"));
        }
        if self.patches > (self.image_len - self.code_at) / 2 {
            return Err(bad("more patches than the code's instructions"));
        }
        Ok(())
    }

    /// The file's length without its author's signature.
    pub fn file_len(&self) -> u64 {
        HEADER_LEN as u64
            + u64::from(self.image_len)
            + 4 * u64::from(self.relocs)
            + 8 * u64::from(self.patches)
    }

    /// Where the words to relocate begin in the file; the patches follow
    /// them ([`Header5::patches_at`]).
    pub fn relocs_at(&self) -> u64 {
        HEADER_LEN as u64 + u64::from(self.image_len)
    }

    /// Where the patches begin in the file.
    pub fn patches_at(&self) -> u64 {
        self.relocs_at() + 4 * u64::from(self.relocs)
    }

    /// Whether the image offset `at` lies in the hot code.
    pub fn in_hot(&self, at: u32) -> bool {
        at.wrapping_sub(self.hot_at) < self.hot_len
    }

    /// Whether a file of `len` bytes ends with a signature; any other
    /// length is [`Refusal::Length`].
    fn signed_for(&self, len: u64) -> Result<bool, Refusal> {
        match len.checked_sub(self.file_len()) {
            Some(0) => Ok(false),
            Some(n) if n == SIGNATURE_LEN as u64 => Ok(true),
            _ => Err(Refusal::Length),
        }
    }

    /// The digest begun with the header's part: fed with every byte after
    /// the header, in order (through a fixed buffer), it ends as
    /// [`Header5::sha256`] (see [`Header5::holds`]).
    pub fn digest(&self) -> Sha256 {
        let mut sha = Sha256::new();
        sha.update(&self.digest_prefix());
        sha
    }

    /// Whether `sha`, begun by [`Header5::digest`] and fed with all after
    /// the header, is the header's.
    pub fn holds(&self, sha: Sha256) -> bool {
        sha.finish() == self.sha256
    }

    /// The header's first 64 bytes, as its digest covers them.
    pub fn digest_prefix(&self) -> [u8; DIGESTED_LEN] {
        let mut bytes = [0; DIGESTED_LEN];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..6].copy_from_slice(&FORMAT.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.target.id().to_le_bytes());
        bytes[8..10].copy_from_slice(&self.abi_major.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.needs_minor.to_le_bytes());
        for (i, value) in [
            self.base,
            self.image_len,
            self.code_at,
            self.entry,
            self.meta_at,
            self.meta_len,
            self.relocs,
            self.align,
            self.hot_at,
            self.hot_len,
            self.patches,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[16 + 4 * i..20 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// The header's 96 bytes.
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut bytes = [0; HEADER_LEN];
        bytes[..DIGESTED_LEN].copy_from_slice(&self.digest_prefix());
        bytes[DIGESTED_LEN..].copy_from_slice(&self.sha256);
        bytes
    }

    /// Whether `value`, a word to relocate read before relocation, is an
    /// address in the image as linked: by the data bus's view
    /// `[base, base + image_len]` or the instruction bus's
    /// `[base + CODE_OFFSET, base + CODE_OFFSET + image_len]`, the end
    /// included (a slice may point there). The same rule the relocation
    /// itself applies, so a code pointer (`make`, a vtable) is no refusal.
    pub fn word_in_image(&self, value: u32) -> bool {
        let len = self.image_len;
        value.wrapping_sub(self.base) <= len
            || value.wrapping_sub(self.base.wrapping_add(CODE_OFFSET)) <= len
    }

    /// Every word to relocate, as read before relocation, lies in the image.
    pub fn check_reloc_words(
        &self,
        words: impl IntoIterator<Item = u32>,
    ) -> Result<(), EntryRefusal> {
        words
            .into_iter()
            .all(|word| self.word_in_image(word))
            .then_some(())
            .ok_or(EntryRefusal::RelocOutside)
    }

    /// Where `make` lies in the image, for [`EntryFacts::make_at`]: an
    /// offset into the image for `make` an instruction address in the
    /// image placed at `placed_base` (`placed_base + CODE_OFFSET + offset`),
    /// or in the hot code copied to `hot` (`hot_at + (make - hot)`; `hot` 0:
    /// not copied). Any other address gives `u32::MAX`, which
    /// [`Header5::check_entry`] refuses.
    pub fn make_offset(&self, make: u32, placed_base: u32, hot: u32) -> u32 {
        if hot != 0 {
            let in_hot = make.wrapping_sub(hot);
            if in_hot < self.hot_len {
                return self.hot_at + in_hot;
            }
        }
        let in_image = make.wrapping_sub(placed_base.wrapping_add(CODE_OFFSET));
        if in_image < self.image_len {
            in_image
        } else {
            u32::MAX
        }
    }

    /// Step 9 once the image is relocated and before `make`: the record
    /// agrees with the header, and the entry with both. `placed_base` is
    /// the data address the image was relocated to (`placement.base`),
    /// which may differ from the linked `self.base`: a relocated
    /// `metadata.ptr` is `placed_base + meta_at`.
    pub fn check_entry(
        &self,
        placed_base: u32,
        host: &Host,
        record: &RecordFacts,
        record_bytes: &[u8],
        entry: &EntryFacts<'_>,
    ) -> Result<(), EntryRefusal> {
        if record.abi_major != self.abi_major
            || record.needs_minor != self.needs_minor
            || record.entry_bytes != host.entry_bytes
        {
            return Err(EntryRefusal::RecordDisagrees);
        }
        if record_bytes.len() != self.meta_len as usize {
            return Err(EntryRefusal::RecordLength);
        }
        if placed_base.checked_add(self.meta_at) != Some(entry.metadata_ptr)
            || entry.metadata_len != self.meta_len
            || entry.metadata.len() != self.meta_len as usize
        {
            return Err(EntryRefusal::MetadataView);
        }
        if entry.metadata != record_bytes {
            return Err(EntryRefusal::MetadataBytes);
        }
        if entry.size != host.entry_bytes {
            return Err(EntryRefusal::EntrySize);
        }
        if entry.abi_major != self.abi_major || entry.needs_minor != self.needs_minor {
            return Err(EntryRefusal::EntryVersion);
        }
        if entry.make_at < self.code_at
            || entry.make_at >= self.image_len
            || !entry.make_at.is_multiple_of(2)
        {
            return Err(EntryRefusal::Make);
        }
        Ok(())
    }
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests;
