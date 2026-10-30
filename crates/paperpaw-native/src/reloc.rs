//! Moving an image to where it runs: the words that hold an address get
//! the distance moved added, the firmware's table's word its address, and
//! the instructions that reach across the hot code's edge are written
//! anew for where both sides run.

use crate::header::{CODE_OFFSET, Header, Problem};

/// What a patch writes (format 3): part of the distance from an
/// instruction to what it reaches, on RISC-V. A patch is two words: the
/// kind and the site (`site | kind << 28`, the site an image offset below
/// [`PatchKind::SITES`]), then a reference, which says what for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatchKind {
    /// The upper 20 bits, in an `auipc` (U-type); the reference is the
    /// image offset it reaches.
    Hi20 = 1,
    /// The lower 12 bits, in an I-type instruction (`jalr`, `addi`, a
    /// load); the reference is the site of its `auipc`.
    Lo12I = 2,
    /// The lower 12 bits, in an S-type instruction (a store); the
    /// reference is the site of its `auipc`.
    Lo12S = 3,
}

impl PatchKind {
    /// The sites a patch can name: the image offsets below this.
    pub const SITES: u32 = 1 << 22;

    /// The first word of a patch of this kind at `site`.
    pub const fn record(self, site: u32) -> u32 {
        site | (self as u32) << 28
    }

    /// The kind and the site of a patch's first word.
    pub const fn of(record: u32) -> Option<(PatchKind, u32)> {
        let site = record & 0x0FFF_FFFF;
        if site >= Self::SITES {
            return None;
        }
        let kind = match record >> 28 {
            1 => PatchKind::Hi20,
            2 => PatchKind::Lo12I,
            3 => PatchKind::Lo12S,
            _ => return None,
        };
        Some((kind, site))
    }

    /// The bits of the instruction a patch keeps.
    const fn kept(self) -> u32 {
        match self {
            PatchKind::Hi20 => 0x0000_0FFF,
            PatchKind::Lo12I => 0x000F_FFFF,
            PatchKind::Lo12S => 0x01FF_F07F,
        }
    }

    /// The bits a patch writes for the distance `distance`, as `auipc`
    /// and its partner split it: the upper part rounded, so the lower one
    /// is signed.
    const fn bits(self, distance: u32) -> u32 {
        let hi = distance.wrapping_add(0x800) & 0xFFFF_F000;
        let lo = distance.wrapping_sub(hi);
        match self {
            PatchKind::Hi20 => hi,
            PatchKind::Lo12I => (lo & 0xFFF) << 20,
            PatchKind::Lo12S => ((lo & 0xFE0) << 20) | ((lo & 0x1F) << 7),
        }
    }
}

/// Checks the words that follow a file's image: first the words to
/// relocate (increasing, each a whole word of the image, none the
/// table's word or in the hot code), then the patches (in order, each at
/// an instruction in the code, a lower part's `auipc` patched too), as
/// many as the header says.
pub fn check_relocations(header: &Header, words: &[u32]) -> Result<(), Problem> {
    if words.len() != header.tail_words() {
        return Err(Problem::Relocation);
    }
    let (relocs, patches) = words.split_at(header.relocs as usize);
    let mut next = 0u32;
    for &at in relocs {
        let fits = at >= next
            && at.is_multiple_of(4)
            && at.checked_add(4).is_some_and(|end| end <= header.image_len)
            && at != header.api
            && !header.in_hot(at);
        if !fits {
            return Err(Problem::Relocation);
        }
        next = at + 4;
    }
    let patches = patches.as_chunks::<2>().0;
    let hi20 = &patches[..high_parts(patches)];
    let mut last = None;
    for &[record, reference] in patches {
        let (kind, site) = PatchKind::of(record).ok_or(Problem::Relocation)?;
        let fits = last.is_none_or(|last| record > last)
            && site >= header.code_at
            && site.is_multiple_of(2)
            && site + 4 <= header.image_len
            && match kind {
                PatchKind::Hi20 => reference < header.image_len,
                PatchKind::Lo12I | PatchKind::Lo12S => target(hi20, reference).is_some(),
            };
        if !fits {
            return Err(Problem::Relocation);
        }
        last = Some(record);
    }
    Ok(())
}

/// How many of `patches` (in order) are upper parts: they come first.
fn high_parts(patches: &[[u32; 2]]) -> usize {
    patches.partition_point(|p| p[0] < PatchKind::Lo12I.record(0))
}

/// What the `auipc` patched at `site` reaches, by the upper parts `hi20`.
fn target(hi20: &[[u32; 2]], site: u32) -> Option<u32> {
    let record = PatchKind::Hi20.record(site);
    hi20.binary_search_by_key(&record, |p| p[0])
        .ok()
        .map(|i| hi20[i][1])
}

/// Relocates an image, a piece at a time, for the data address it is
/// placed at, and for where its hot code runs.
#[derive(Debug)]
pub struct Relocator<'a> {
    header: &'a Header,
    words: &'a [u32],
    patches: &'a [[u32; 2]],
    /// How many of `patches` are upper parts.
    hi20: usize,
    /// The data address the image is placed at.
    base: u32,
    /// What every address in the image moves by.
    delta: u32,
    /// The instruction address the hot code runs at.
    hot: u32,
    /// The address of the firmware's table.
    api: u32,
    /// The addresses of the firmware's tables of shared functions, by
    /// family number (0: none).
    tables: [u32; crate::TABLE_SYMBOLS.len()],
}

impl<'a> Relocator<'a> {
    /// For the image of `header` placed at data address `base`, the
    /// `words` that follow its image in the file (checked here: those to
    /// relocate, then the patches), and the table at `api`. Its hot code
    /// runs in place unless [`Relocator::hot_at`] says otherwise.
    pub fn new(header: &'a Header, words: &'a [u32], base: u32, api: u32) -> Result<Self, Problem> {
        check_relocations(header, words)?;
        if !base.is_multiple_of(header.align)
            || base
                .checked_add(header.image_len)
                .and_then(|end| end.checked_add(CODE_OFFSET))
                .is_none()
        {
            return Err(Problem::Malformed);
        }
        let (words, patches) = words.split_at(header.relocs as usize);
        let patches = patches.as_chunks::<2>().0;
        Ok(Self {
            header,
            words,
            patches,
            hi20: high_parts(patches),
            base,
            delta: base.wrapping_sub(header.base),
            hot: base + CODE_OFFSET + header.hot_at,
            api,
            tables: [0; crate::TABLE_SYMBOLS.len()],
        })
    }

    /// Gives the plugin the firmware's tables of shared functions, at
    /// these addresses by family number (0: the firmware has none): they
    /// are written into the words `paperpaw-plugin.x` makes for them, as
    /// the table's address is into `PAPERPAW_API`.
    #[must_use]
    pub fn tables(mut self, tables: [u32; crate::TABLE_SYMBOLS.len()]) -> Self {
        self.tables = tables;
        self
    }

    /// Runs the hot code at instruction address `hot` (a multiple of 4),
    /// not in place: the device copies it there once relocated. Without
    /// hot code it changes nothing.
    pub fn hot_at(mut self, hot: u32) -> Result<Self, Problem> {
        if self.header.hot_len == 0 {
            return Ok(self);
        }
        if !hot.is_multiple_of(4) || hot.checked_add(self.header.hot_len).is_none() {
            return Err(Problem::Malformed);
        }
        self.hot = hot;
        Ok(self)
    }

    /// Whether the hot code runs elsewhere than in place.
    fn hot_moved(&self) -> bool {
        self.hot != self.base + CODE_OFFSET + self.header.hot_at
    }

    /// Relocates `piece`, the image's bytes from offset `at` on (a
    /// multiple of 4; every piece but the image's last a whole number of
    /// words). Each word moved must hold an address in the image, in
    /// either bus's view of it, and one in the hot code a code address
    /// if the hot code moves. An instruction patched may lie across two
    /// pieces: each gets its part.
    pub fn apply(&self, at: u32, piece: &mut [u8]) -> Result<(), Problem> {
        if !at.is_multiple_of(4) {
            return Err(Problem::Relocation);
        }
        let end = at.saturating_add(piece.len() as u32);
        let first = self.words.partition_point(|&w| w < at);
        for &word in self.words[first..].iter().take_while(|&&w| w < end) {
            let i = (word - at) as usize;
            let Some(bytes) = piece.get_mut(i..i + 4) else {
                return Err(Problem::Relocation);
            };
            let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let moved = self.moved(value).ok_or(Problem::Relocation)?;
            bytes.copy_from_slice(&moved.to_le_bytes());
        }
        let api = self.header.api;
        if (at..end).contains(&api) {
            let i = (api - at) as usize;
            let Some(bytes) = piece.get_mut(i..i + 4) else {
                return Err(Problem::Relocation);
            };
            bytes.copy_from_slice(&self.api.to_le_bytes());
        }
        for (n, table) in self.tables.iter().enumerate() {
            let word = api + 4 * (n as u32 + 1);
            if (at..end).contains(&word) {
                let i = (word - at) as usize;
                let Some(bytes) = piece.get_mut(i..i + 4) else {
                    return Err(Problem::Relocation);
                };
                bytes.copy_from_slice(&table.to_le_bytes());
            }
        }
        self.patch(at, end, piece)
    }

    /// Where the address `value` in the image as linked lies once placed
    /// (`None` if it is no address in the image, by data or by code, its
    /// end included: a slice may point there).
    fn moved(&self, value: u32) -> Option<u32> {
        let len = self.header.image_len;
        let code = value.wrapping_sub(self.header.base + CODE_OFFSET);
        let data = value.wrapping_sub(self.header.base);
        if code <= len {
            Some(if self.header.in_hot(code) {
                self.runs_at(code)
            } else {
                value.wrapping_add(self.delta)
            })
        } else if data <= len && !(self.header.in_hot(data) && self.hot_moved()) {
            Some(value.wrapping_add(self.delta))
        } else {
            None
        }
    }

    /// The address image offset `at` is reached at once placed: its data
    /// address in the read-only data, its instruction address in the code.
    fn runs_at(&self, at: u32) -> u32 {
        if at < self.header.code_at {
            self.base + at
        } else if self.header.in_hot(at) {
            self.hot + (at - self.header.hot_at)
        } else {
            self.base + CODE_OFFSET + at
        }
    }

    /// Writes the patches whose instructions lie in `piece`, from `at` to
    /// `end`, a byte at a time.
    fn patch(&self, at: u32, end: u32, piece: &mut [u8]) -> Result<(), Problem> {
        let hi20 = &self.patches[..self.hi20];
        let lo12 = &self.patches[self.hi20..];
        let lo12s = lo12.partition_point(|p| p[0] < PatchKind::Lo12S.record(0));
        for block in [hi20, &lo12[..lo12s], &lo12[lo12s..]] {
            let first = block.partition_point(|p| (p[0] & 0x0FFF_FFFF) + 4 <= at);
            for &[record, reference] in &block[first..] {
                let (kind, site) = PatchKind::of(record).ok_or(Problem::Relocation)?;
                if site >= end {
                    break;
                }
                let (from, to) = match kind {
                    PatchKind::Hi20 => (site, reference),
                    PatchKind::Lo12I | PatchKind::Lo12S => (
                        reference,
                        target(hi20, reference).ok_or(Problem::Relocation)?,
                    ),
                };
                let distance = self.runs_at(to).wrapping_sub(self.runs_at(from));
                let bits = kind.bits(distance).to_le_bytes();
                let kept = kind.kept().to_le_bytes();
                for i in 0..4 {
                    let Some(byte) = (site + i)
                        .checked_sub(at)
                        .and_then(|j| piece.get_mut(j as usize))
                    else {
                        continue;
                    };
                    *byte = (*byte & kept[i as usize]) | bits[i as usize];
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;
    use crate::header::tests::{chip_header, file, seal};
    use crate::header::{Chip, HEADER_LEN, LINK_BASE};

    fn le(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn words_move_with_the_image_in_any_pieces() {
        let file = file();
        let (header, _) = Header::check_file(&file).unwrap();
        let words = [68, 72];
        let base = 0x3C80_0040;
        for piece in [4, 8, 20, 128] {
            let relocator = Relocator::new(&header, &words, base, 0x3FC8_1234).unwrap();
            let mut image: Vec<u8> = file[HEADER_LEN..HEADER_LEN + 128].to_vec();
            for (i, chunk) in image.chunks_mut(piece).enumerate() {
                relocator.apply((i * piece) as u32, chunk).unwrap();
            }
            assert_eq!(le(&image, 32), 0x3FC8_1234, "the table's word");
            assert_eq!(le(&image, 68), base + 80, "data");
            assert_eq!(le(&image, 72), base + CODE_OFFSET + 96, "code");
            assert_eq!(
                image[96..],
                file[HEADER_LEN + 96..HEADER_LEN + 128],
                "code unchanged"
            );
        }
    }

    #[test]
    fn the_family_words_get_the_tables_in_any_pieces() {
        let file = file();
        let (header, _) = Header::check_file(&file).unwrap();
        let words = [68, 72];
        let tables = [0x3FC8_2000, 0, 0x3FC8_3000, 0, 0, 0, 0, 0x3FC8_4000];
        for piece in [4, 8, 20, 128] {
            let relocator = Relocator::new(&header, &words, 0x3C80_0040, 0x3FC8_1234)
                .unwrap()
                .tables(tables);
            let mut image: Vec<u8> = file[HEADER_LEN..HEADER_LEN + 128].to_vec();
            for (i, chunk) in image.chunks_mut(piece).enumerate() {
                relocator.apply((i * piece) as u32, chunk).unwrap();
            }
            assert_eq!(le(&image, 32), 0x3FC8_1234, "the table's word");
            for (n, table) in tables.iter().enumerate() {
                assert_eq!(le(&image, 36 + 4 * n), *table, "family {n}");
            }
            // Without them the words stay 0: no family shared.
            assert_eq!(le(&image, 68), 0x3C80_0040 + 80, "data still moves");
        }
    }

    #[test]
    fn bad_words_are_refused() {
        let header = chip_header(Chip::Esp32S3, 128, 2);
        for words in [[72, 68], [68, 68], [69, 72], [68, 128], [32, 68], [68, 126]] {
            assert_eq!(
                Relocator::new(&header, &words, LINK_BASE, 0).unwrap_err(),
                Problem::Relocation,
                "{words:?}"
            );
        }
        assert_eq!(
            check_relocations(&header, &[68]),
            Err(Problem::Relocation),
            "fewer than the header says"
        );
        assert_eq!(
            Relocator::new(&header, &[68, 72], LINK_BASE + 8, 0).unwrap_err(),
            Problem::Malformed,
            "a place not aligned as the image wants"
        );
        // A word that holds no address in the image.
        let relocator = Relocator::new(&header, &[68, 72], LINK_BASE, 0).unwrap();
        let mut image = [0u8; 128];
        image[68..72].copy_from_slice(&0x4000_0000u32.to_le_bytes());
        assert_eq!(relocator.apply(0, &mut image), Err(Problem::Relocation));
        assert_eq!(
            relocator.apply(2, &mut image[2..]),
            Err(Problem::Relocation)
        );
    }

    // A small RISC-V image with hot code: 96 bytes of read-only data (the
    // table's words at 32, the address of `hot` at 68, a constant at 80),
    // then cold code from 96 (a `c.nop`, then a call of `hot` at 98, so
    // that it lies across words, and `cold` at 108), then hot code from 128
    // (a load and a store of the constant, a call of `cold`).
    const HOT_AT: u32 = 128;
    const LEN: u32 = 160;
    const CALL_HOT: u32 = 98;
    const COLD: u32 = 108;
    const LOAD: u32 = 128;
    const CALL_COLD: u32 = 140;
    const CONSTANT: u32 = 80;
    const HOT_WORD: usize = 68;
    const CODE: u32 = LINK_BASE + CODE_OFFSET;

    fn auipc(rd: u32, distance: u32) -> u32 {
        PatchKind::Hi20.bits(distance) | (rd << 7) | 0x17
    }

    fn i_type(opcode: u32, funct3: u32, rd: u32, rs1: u32, distance: u32) -> u32 {
        PatchKind::Lo12I.bits(distance) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | opcode
    }

    fn sw(rs2: u32, rs1: u32, distance: u32) -> u32 {
        PatchKind::Lo12S.bits(distance) | (rs2 << 20) | (rs1 << 15) | (2 << 12) | 0x23
    }

    fn put(image: &mut [u8], at: u32, insn: u32) {
        image[at as usize..at as usize + 4].copy_from_slice(&insn.to_le_bytes());
    }

    /// The image as linked and its words after it.
    fn hot_image() -> (Header, Vec<u8>, Vec<u32>) {
        let mut image = alloc::vec![0u8; LEN as usize];
        image[HOT_WORD..HOT_WORD + 4].copy_from_slice(&(CODE + HOT_AT).to_le_bytes());
        image[CONSTANT as usize..CONSTANT as usize + 4].copy_from_slice(b"PPaw");
        for at in (96..LEN).step_by(4) {
            put(&mut image, at, 0x0000_0013);
        }
        image[96..98].copy_from_slice(&0x0001u16.to_le_bytes());
        let to_hot = HOT_AT - CALL_HOT;
        put(&mut image, CALL_HOT, auipc(1, to_hot));
        put(&mut image, CALL_HOT + 4, i_type(0x67, 0, 1, 1, to_hot));
        put(&mut image, COLD, 0x0000_8067);
        let to_constant = (LINK_BASE + CONSTANT).wrapping_sub(CODE + LOAD);
        put(&mut image, LOAD, auipc(10, to_constant));
        put(&mut image, LOAD + 4, i_type(0x03, 2, 11, 10, to_constant));
        put(&mut image, LOAD + 8, sw(11, 10, to_constant));
        let to_cold = COLD.wrapping_sub(CALL_COLD);
        put(&mut image, CALL_COLD, auipc(1, to_cold));
        put(&mut image, CALL_COLD + 4, i_type(0x67, 0, 1, 1, to_cold));
        let patches = [
            [PatchKind::Hi20.record(CALL_HOT), HOT_AT],
            [PatchKind::Hi20.record(LOAD), CONSTANT],
            [PatchKind::Hi20.record(CALL_COLD), COLD],
            [PatchKind::Lo12I.record(CALL_HOT + 4), CALL_HOT],
            [PatchKind::Lo12I.record(LOAD + 4), LOAD],
            [PatchKind::Lo12I.record(CALL_COLD + 4), CALL_COLD],
            [PatchKind::Lo12S.record(LOAD + 8), LOAD],
        ];
        let mut words = alloc::vec![HOT_WORD as u32];
        words.extend(patches.iter().flatten());
        let header = Header {
            image_len: LEN,
            relocs: 1,
            hot_at: HOT_AT,
            hot_len: LEN - HOT_AT,
            patches: patches.len() as u32,
            ..chip_header(Chip::Esp32C3, LEN, 1)
        };
        let file = seal(header, &image, &words);
        let (header, _) = Header::check_file(&file).unwrap();
        (header, image, words)
    }

    /// What the `auipc` at `at` in `image` and its partner at `partner`
    /// reach from `pc`, the address the `auipc` runs at.
    fn reach(image: &[u8], at: u32, partner: u32, pc: u32, store: bool) -> u32 {
        let hi = le(image, at as usize) & 0xFFFF_F000;
        let insn = le(image, partner as usize);
        let lo = if store {
            (((insn as i32) >> 25) << 5) as u32 | ((insn >> 7) & 0x1F)
        } else {
            ((insn as i32) >> 20) as u32
        };
        pc.wrapping_add(hi).wrapping_add(lo)
    }

    /// The image relocated for `base` with hot code at `hot` (0: in
    /// place), in pieces of `piece` bytes.
    fn relocated(base: u32, hot: u32, piece: usize) -> Vec<u8> {
        let (header, mut image, words) = hot_image();
        let mut relocator = Relocator::new(&header, &words, base, 0x3FC8_1234).unwrap();
        if hot != 0 {
            relocator = relocator.hot_at(hot).unwrap();
        }
        for (i, chunk) in image.chunks_mut(piece).enumerate() {
            relocator.apply((i * piece) as u32, chunk).unwrap();
        }
        image
    }

    #[test]
    fn hot_code_is_reached_where_it_runs() {
        let base = 0x3C81_0000;
        let code = base + CODE_OFFSET;
        for hot in [0, 0x4039_0100] {
            let runs = if hot == 0 { code + HOT_AT } else { hot };
            for piece in [4, 8, 12, 20, 64, 128] {
                let image = relocated(base, hot, piece);
                let in_hot = |at: u32| runs + at - HOT_AT;
                assert_eq!(le(&image, HOT_WORD), runs, "the word of `hot`");
                assert_eq!(
                    reach(&image, CALL_HOT, CALL_HOT + 4, code + CALL_HOT, false),
                    runs,
                    "the cold call of `hot` ({piece})"
                );
                assert_eq!(
                    reach(&image, LOAD, LOAD + 4, in_hot(LOAD), false),
                    base + CONSTANT,
                    "the hot load"
                );
                assert_eq!(
                    le(&image, LOAD as usize + 4) & 0xFFFFF,
                    (11 << 7) | (10 << 15) | (2 << 12) | 0x03,
                    "the load's registers"
                );
                assert_eq!(
                    reach(&image, LOAD, LOAD + 8, in_hot(LOAD), true),
                    base + CONSTANT,
                    "the hot store"
                );
                assert_eq!(
                    reach(&image, CALL_COLD, CALL_COLD + 4, in_hot(CALL_COLD), false),
                    code + COLD,
                    "the hot call of `cold`"
                );
            }
        }
    }

    #[test]
    fn in_place_at_the_link_base_nothing_changes() {
        let (_, linked, _) = hot_image();
        let image = relocated(LINK_BASE, 0, 4);
        assert_eq!(le(&image, 32), 0x3FC8_1234);
        assert_eq!(image[36..], linked[36..]);
    }

    #[test]
    fn patches_that_do_not_hold_are_refused() {
        let (header, _, words) = hot_image();
        let mut bad = Vec::new();
        // Out of order, outside the code, at an odd offset, past the end,
        // a kind unknown, a site too far; a lower part without its `auipc`.
        for (i, record) in [
            (2, PatchKind::Hi20.record(LOAD)),
            (1, PatchKind::Hi20.record(92)),
            (1, PatchKind::Hi20.record(129)),
            (2, PatchKind::Hi20.record(LEN - 2)),
            (1, 4 << 28 | LOAD),
            (1, PatchKind::Hi20.record(PatchKind::SITES + LOAD)),
        ] {
            let mut changed = words.clone();
            changed[1 + 2 * i] = record;
            bad.push(changed);
        }
        let mut no_auipc = words.clone();
        no_auipc[1 + 2 * 3 + 1] = CALL_HOT + 2;
        bad.push(no_auipc);
        // A word to relocate in the hot code.
        let mut in_hot = words.clone();
        in_hot[0] = HOT_AT + 4;
        bad.push(in_hot);
        for words in bad {
            assert_eq!(
                check_relocations(&header, &words),
                Err(Problem::Relocation),
                "{words:?}"
            );
        }
        // A word whose data address lies in hot code that moves.
        let mut image = alloc::vec![0u8; LEN as usize];
        image[HOT_WORD..HOT_WORD + 4].copy_from_slice(&(LINK_BASE + HOT_AT).to_le_bytes());
        let relocator = Relocator::new(&header, &words, LINK_BASE, 0).unwrap();
        assert!(relocator.apply(0, &mut image.clone()).is_ok());
        let moved = relocator.hot_at(0x4039_0000).unwrap();
        assert_eq!(moved.apply(0, &mut image), Err(Problem::Relocation));
    }
}
