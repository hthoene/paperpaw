//! Baseline JPEG, decoded one row of MCUs at a time. For gray only the
//! first component (luma) is transformed; the others are entropy-decoded
//! and dropped. For a sink that wants colour, the chroma components are
//! transformed too, stretched to luma's resolution and turned into RGB.
//! Progressive JPEG goes on in [`progressive`](crate::progressive).
//!
//! Where the data ends before the last block (a marker comes), the
//! blocks after it have only the DC they had last, as zeros would give
//! them, without being decoded; and the blocks the scans go through are
//! bounded by the bytes read ([`BLOCKS_PER_BYTE`]), so a small file
//! cannot ask for long work.

use alloc::vec::Vec;

use crate::{Downscaler, Fit, ImageError, ImageSink, Reader, block_side, zeroed};

/// Natural order of the zig-zag sequence.
pub(crate) const ZIGZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

#[derive(Clone)]
pub(crate) struct Huffman {
    /// Largest code of each length (1 to 16), -1 if none.
    max_code: [i32; 18],
    /// Index into `values` of the first code of each length, minus that code.
    offset: [i32; 17],
    values: Vec<u8>,
    /// The codes of up to [`FAST_BITS`] bits by the next bits: their
    /// length times 256 plus their value, 0 where a longer code begins.
    /// On the heap: eight tables of them in a frame took 4 KB of stack,
    /// and were moved whole from there.
    fast: Vec<u16>,
}

/// The quantization tables and Huffman tables (four of each kind, by the
/// number a segment gives) a frame header and its segments set. On the
/// heap, in three lists of four: kept in `decode`'s frame (and moved whole
/// into the progressive decoder's), they took 3.8 KB of the stack. The
/// heap they take, besides the tables' own codes and lookups (as before):
/// 512 B of quantization tables and four DC and four AC slots of 164 B on
/// a 32-bit target (656 B each list), 1,824 B in all, freed with the decode.
pub(crate) struct Tables {
    pub quant: Vec<[u16; 64]>,
    pub dc: Vec<Option<Huffman>>,
    pub ac: Vec<Option<Huffman>>,
}

impl Tables {
    // Not inlined: its zeros are its own frame's, not `decode`'s.
    #[inline(never)]
    pub(crate) fn new() -> Result<Self, ImageError> {
        let mut quant: Vec<[u16; 64]> = Vec::new();
        quant
            .try_reserve_exact(4)
            .map_err(|_| ImageError::OutOfMemory)?;
        quant.resize(4, [0u16; 64]);
        Ok(Self {
            quant,
            dc: zeroed(4)?,
            ac: zeroed(4)?,
        })
    }
}

/// Bits of the codes looked up at once.
const FAST_BITS: u32 = 8;

impl Huffman {
    fn new(counts: &[u8; 16], values: Vec<u8>) -> Result<Self, ImageError> {
        let mut max_code = [-1i32; 18];
        let mut offset = [0i32; 17];
        let mut fast: Vec<u16> = crate::zeroed(1 << FAST_BITS)?;
        let (mut code, mut k) = (0i32, 0i32);
        for len in 1..=16 {
            let count = i32::from(counts[len - 1]);
            offset[len] = k - code;
            if len <= FAST_BITS as usize {
                let spread = FAST_BITS as usize - len;
                for c in 0..count {
                    let Some(&value) = values.get((k + c) as usize) else {
                        break;
                    };
                    let first = ((code + c) as usize) << spread;
                    let entry = (len as u16) << 8 | u16::from(value);
                    for slot in fast.iter_mut().skip(first).take(1 << spread) {
                        *slot = entry;
                    }
                }
            }
            code += count;
            k += count;
            max_code[len] = if count > 0 { code - 1 } else { -1 };
            code <<= 1;
        }
        // Sentinel: longer codes do not exist.
        max_code[17] = i32::MAX;
        Ok(Self {
            max_code,
            offset,
            values,
            fast,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Component {
    pub id: u8,
    pub h: usize,
    pub v: usize,
    pub quant: usize,
    pub dc: usize,
    pub ac: usize,
    pub prediction: i32,
}

/// Blocks the scans of a JPEG may go through for each byte of it read.
/// A real image takes a bit a block at least for its first scan, and
/// goes through its blocks in a few scans: far less. A small damaged or
/// crafted file that declares a large image, ends its data early or
/// repeats its scans is stopped here.
const BLOCKS_PER_BYTE: u64 = 256;

/// Entropy-coded data: bits with byte stuffing and markers.
pub(crate) struct Bits<'r, 'a> {
    pub reader: &'r mut Reader<'a>,
    bits: u32,
    count: u32,
    /// A marker ended the data; zeros are read from here on.
    pub marker: Option<u8>,
    /// The data ended without a marker; zeros are read from here on, to
    /// look ahead, but taking them is an error.
    ended: bool,
    /// How many of the last bits taken in are such zeros.
    padding: u32,
}

impl<'r, 'a> Bits<'r, 'a> {
    pub(crate) fn new(reader: &'r mut Reader<'a>) -> Self {
        Self {
            reader,
            bits: 0,
            count: 0,
            marker: None,
            ended: false,
            padding: 0,
        }
    }

    /// Takes bytes until more than 24 bits are at hand (zeros after a
    /// marker or the end of the data, which a file without its end
    /// marker reaches while its last bits are looked ahead of). Not
    /// inlined: the bits at hand are looked at where they are asked for,
    /// and this is called once in a few of them.
    #[inline(never)]
    fn fill(&mut self) -> Result<(), ImageError> {
        while self.count <= 24 {
            let byte = if self.marker.is_some() || self.ended {
                None
            } else {
                match self.reader.next()? {
                    Some(0xff) => {
                        let mut next = self.reader.next()?;
                        while next == Some(0xff) {
                            next = self.reader.next()?;
                        }
                        match next {
                            Some(0) => Some(0xff),
                            Some(marker) => {
                                self.marker = Some(marker);
                                None
                            }
                            None => {
                                self.ended = true;
                                None
                            }
                        }
                    }
                    Some(byte) => Some(byte),
                    None => {
                        self.ended = true;
                        None
                    }
                }
            };
            if byte.is_none() {
                self.padding = self.padding.saturating_add(8);
            }
            self.bits = self.bits << 8 | u32::from(byte.unwrap_or(0));
            self.count += 8;
        }
        Ok(())
    }

    /// Whether the data ran out: zeros after a marker were taken as
    /// bits (the bits not taken yet are the last ones taken in). Zeros
    /// after the end of the data are an error: it was cut short.
    pub(crate) fn exhausted(&self) -> Result<bool, ImageError> {
        let out = self.padding > self.count;
        if out && self.ended {
            return Err(ImageError::Invalid("unexpected end"));
        }
        Ok(out)
    }

    /// Whether the data goes on: it ran out at a restart marker, and a
    /// restart is coming (`restart_interval` is not 0).
    pub(crate) fn resumes(&self, restart_interval: u32) -> bool {
        restart_interval > 0 && matches!(self.marker, Some(0xd0..=0xd7))
    }

    /// Whether `blocks` gone through are no more than the data read so
    /// far may ask for ([`BLOCKS_PER_BYTE`]).
    pub(crate) fn affords(&self, blocks: u64) -> bool {
        blocks <= self.reader.position().saturating_mul(BLOCKS_PER_BYTE)
    }

    /// The next `n` bits (at most 24), not taken. Inlined: with the bits
    /// at hand it is a shift and a mask, asked for twice a coefficient.
    #[inline(always)]
    fn peek(&mut self, n: u32) -> Result<u32, ImageError> {
        if self.count < n {
            self.fill()?;
        }
        Ok((self.bits >> (self.count - n)) & ((1 << n) - 1))
    }

    pub(crate) fn bit(&mut self) -> Result<u32, ImageError> {
        let bit = self.peek(1)?;
        self.count -= 1;
        Ok(bit)
    }

    #[inline]
    pub(crate) fn receive(&mut self, n: u32) -> Result<i32, ImageError> {
        if n > 16 {
            return self.receive_wide(n);
        }
        if n == 0 {
            return Ok(0);
        }
        let v = self.peek(n)?;
        self.count -= n;
        Ok(v as i32)
    }

    /// [`Bits::receive`] of more than 16 bits (no valid file asks for it).
    #[inline(never)]
    fn receive_wide(&mut self, n: u32) -> Result<i32, ImageError> {
        let high = self.receive(n - 16)?;
        Ok(high << 16 | self.receive(16)?)
    }

    pub(crate) fn extend(v: i32, n: u32) -> i32 {
        if n == 0 {
            0
        } else if v < 1 << (n - 1) {
            v - (1 << n) + 1
        } else {
            v
        }
    }

    #[inline]
    pub(crate) fn decode(&mut self, table: &Huffman) -> Result<u8, ImageError> {
        let next = self.peek(FAST_BITS)?;
        let entry = table.fast[next as usize];
        if entry != 0 {
            self.count -= u32::from(entry >> 8);
            return Ok(entry as u8);
        }
        self.decode_long(table, next)
    }

    /// A code longer than [`FAST_BITS`], whose first bits are `next`: bit
    /// by bit from there.
    #[inline(never)]
    fn decode_long(&mut self, table: &Huffman, next: u32) -> Result<u8, ImageError> {
        self.count -= FAST_BITS;
        let mut code = next as i32;
        for len in FAST_BITS as usize + 1..=16 {
            code = (code << 1) | self.bit()? as i32;
            if code <= table.max_code[len] {
                let index = (table.offset[len] + code) as usize;
                return table
                    .values
                    .get(index)
                    .copied()
                    .ok_or(ImageError::Invalid("bad Huffman code"));
            }
        }
        Err(ImageError::Invalid("bad Huffman code"))
    }

    /// After a restart interval: realign and expect an RST marker.
    pub(crate) fn restart(&mut self) -> Result<(), ImageError> {
        self.count = 0;
        self.padding = 0;
        match self.marker.take() {
            Some(0xd0..=0xd7) => Ok(()),
            Some(_) => Err(ImageError::Invalid("missing restart marker")),
            None => {
                // Find the marker in the data.
                loop {
                    if self.reader.byte()? != 0xff {
                        continue;
                    }
                    let mut next = self.reader.byte()?;
                    while next == 0xff {
                        next = self.reader.byte()?;
                    }
                    match next {
                        0xd0..=0xd7 => return Ok(()),
                        0 => {}
                        _ => return Err(ImageError::Invalid("missing restart marker")),
                    }
                }
            }
        }
    }
}

/// The cosine basis c[u][x] = C(u) cos((2x+1)uπ/2n) / 2, scaled by 2^12,
/// for blocks of n = 8, 4, 2 or 1 pixels a side.
const C8: [[i32; 8]; 8] = [
    [1448, 1448, 1448, 1448, 1448, 1448, 1448, 1448],
    [2009, 1703, 1138, 400, -400, -1138, -1703, -2009],
    [1892, 784, -784, -1892, -1892, -784, 784, 1892],
    [1703, -400, -2009, -1138, 1138, 2009, 400, -1703],
    [1448, -1448, -1448, 1448, 1448, -1448, -1448, 1448],
    [1138, -2009, 400, 1703, -1703, -400, 2009, -1138],
    [784, -1892, 1892, -784, -784, 1892, -1892, 784],
    [400, -1138, 1703, -2009, 2009, -1703, 1138, -400],
];
const C4: [[i32; 8]; 8] = [
    [1448, 1448, 1448, 1448, 0, 0, 0, 0],
    [1892, 784, -784, -1892, 0, 0, 0, 0],
    [1448, -1448, -1448, 1448, 0, 0, 0, 0],
    [784, -1892, 1892, -784, 0, 0, 0, 0],
    [0; 8],
    [0; 8],
    [0; 8],
    [0; 8],
];
const C2: [[i32; 8]; 8] = [
    [1448, 1448, 0, 0, 0, 0, 0, 0],
    [1448, -1448, 0, 0, 0, 0, 0, 0],
    [0; 8],
    [0; 8],
    [0; 8],
    [0; 8],
    [0; 8],
    [0; 8],
];
const C1: [[i32; 8]; 8] = [
    [1448, 0, 0, 0, 0, 0, 0, 0],
    [0; 8],
    [0; 8],
    [0; 8],
    [0; 8],
    [0; 8],
    [0; 8],
    [0; 8],
];

/// Coefficients are kept within this (a valid one is within 2048), so
/// sums stay within 32 bits: the processor of the X4 multiplies 64-bit
/// numbers slowly.
const COEFFICIENT: i32 = 4095;

/// Inverse DCT of the `n`×`n` lowest frequencies of a block, which gives
/// the block at `n`/8 of its size (`n` = 8, 4, 2 or 1).
pub(crate) fn idct_scaled(coef: &[i32; 64], n: usize, out: &mut [u8], stride: usize) {
    if n == 4 {
        return idct4(coef, out, stride);
    }
    if n == 8 {
        return idct8(coef, out, stride);
    }
    let c = match n {
        8 => &C8,
        4 => &C4,
        2 => &C2,
        _ => &C1,
    };
    let n = n.clamp(1, 8);
    let mut tmp = [0i32; 64];
    // Rows: frequencies u (horizontal) to positions x.
    for v in 0..n {
        let row = &coef[v * 8..v * 8 + n];
        if row.iter().all(|&c| c == 0) {
            continue;
        }
        for x in 0..n {
            let mut sum = 0i32;
            for (u, &value) in row.iter().enumerate() {
                sum += value.clamp(-COEFFICIENT, COEFFICIENT) * c[u][x];
            }
            tmp[v * 8 + x] = sum >> 10;
        }
    }
    // Columns.
    for x in 0..n {
        for y in 0..n {
            let mut sum = 0i32;
            for v in 0..n {
                sum += tmp[v * 8 + x] * c[v][y];
            }
            // Scales: 2^12 · 2^12 / 2^10 = 2^14, plus the level shift.
            let value = ((sum + (1 << 13)) >> 14) + 128;
            out[y * stride + x] = value.clamp(0, 255) as u8;
        }
    }
}

/// [`C4`] applied to four frequencies, with 6 multiplications instead of
/// 16 (the same sums, so the same numbers).
fn idct4_line(f: [i32; 4]) -> [i32; 4] {
    let even = (f[0] + f[2]) * 1448;
    let odd = (f[0] - f[2]) * 1448;
    let a = f[1] * 1892 + f[3] * 784;
    let b = f[1] * 784 - f[3] * 1892;
    [even + a, odd + b, odd - b, even - a]
}

/// [`idct_scaled`] at half the size (`n` = 4), the most common.
fn idct4(coef: &[i32; 64], out: &mut [u8], stride: usize) {
    let mut tmp = [[0i32; 4]; 4];
    for (v, row) in tmp.iter_mut().enumerate() {
        let f = core::array::from_fn(|u| coef[v * 8 + u].clamp(-COEFFICIENT, COEFFICIENT));
        if f != [0; 4] {
            *row = idct4_line(f).map(|sum| sum >> 10);
        }
    }
    for x in 0..4 {
        let column = idct4_line([tmp[0][x], tmp[1][x], tmp[2][x], tmp[3][x]]);
        for (y, sum) in column.into_iter().enumerate() {
            out[y * stride + x] = (((sum + (1 << 13)) >> 14) + 128).clamp(0, 255) as u8;
        }
    }
}

/// [`C8`] applied to eight frequencies, with 22 multiplications instead
/// of 64. The basis is even in its even frequencies and odd in its odd
/// ones (`c[u][7 - x]` is `c[u][x]`, negated for odd `u`), so each half of
/// the line is the even frequencies' sum plus or minus the odd ones'; the
/// same products are added, in another order, so the numbers are the same
/// (nothing overflows: see [`COEFFICIENT`]).
#[inline(always)]
fn idct8_line(f: [i32; 8]) -> [i32; 8] {
    let e0 = (f[0] + f[4]) * 1448;
    let e1 = (f[0] - f[4]) * 1448;
    let a = f[2] * 1892 + f[6] * 784;
    let b = f[2] * 784 - f[6] * 1892;
    let (p, q, r, s) = (e0 + a, e1 + b, e1 - b, e0 - a);
    // Without odd frequencies (most lines of a photograph's blocks) the
    // line is its own mirror.
    if f[1] | f[3] | f[5] | f[7] == 0 {
        return [p, q, r, s, s, r, q, p];
    }
    let t = f[1] * 2009 + f[3] * 1703 + f[5] * 1138 + f[7] * 400;
    let u = f[1] * 1703 - f[3] * 400 - f[5] * 2009 - f[7] * 1138;
    let v = f[1] * 1138 - f[3] * 2009 + f[5] * 400 + f[7] * 1703;
    let w = f[1] * 400 - f[3] * 1138 + f[5] * 1703 - f[7] * 2009;
    [p + t, q + u, r + v, s + w, s - w, r - v, q - u, p - t]
}

/// [`idct_scaled`] at full size (`n` = 8): what a picture less than twice
/// the size it shows at is decoded at, every block of it. The general
/// sums took most of the time of such a picture's decoding (#250).
fn idct8(coef: &[i32; 64], out: &mut [u8], stride: usize) {
    let mut tmp = [[0i32; 8]; 8];
    for (row, to) in coef.as_chunks::<8>().0.iter().zip(tmp.iter_mut()) {
        // A row without frequencies (most, in a photograph) stays zeros.
        if *row != [0; 8] {
            *to = idct8_line(row.map(|c| c.clamp(-COEFFICIENT, COEFFICIENT))).map(|sum| sum >> 10);
        }
    }
    for x in 0..8 {
        let t = &tmp;
        let column = idct8_line([
            t[0][x], t[1][x], t[2][x], t[3][x], t[4][x], t[5][x], t[6][x], t[7][x],
        ]);
        for (y, sum) in column.into_iter().enumerate() {
            out[y * stride + x] = (((sum + (1 << 13)) >> 14) + 128).clamp(0, 255) as u8;
        }
    }
}

/// The level of a block with only its DC coefficient, as
/// [`idct_scaled`] gives it.
pub(crate) fn dc_level(dc: i32) -> u8 {
    let tmp = (dc.clamp(-COEFFICIENT, COEFFICIENT) * 1448) >> 10;
    (((tmp * 1448 + (1 << 13)) >> 14) + 128).clamp(0, 255) as u8
}

// Not inlined into `decode`, so each format's frame is on the stack alone,
// not all three together.
#[inline(never)]
pub(crate) fn decode(
    reader: &mut Reader<'_>,
    fit: Fit,
    sink: &mut dyn ImageSink,
) -> Result<(), ImageError> {
    if reader.byte()? != 0xff || reader.byte()? != 0xd8 {
        return Err(ImageError::Invalid("not a JPEG"));
    }
    let mut tables = Tables::new()?;
    let mut components: Vec<Component> = Vec::new();
    let (mut width, mut height) = (0u32, 0u32);
    let mut restart_interval = 0u32;
    let mut progressive = false;

    loop {
        // Next marker.
        let mut marker = reader.byte()?;
        if marker != 0xff {
            return Err(ImageError::Invalid("expected a marker"));
        }
        while marker == 0xff {
            marker = reader.byte()?;
        }
        match marker {
            0xd8 | 0x01 | 0xd0..=0xd7 => continue,
            0xd9 => return Err(ImageError::Invalid("no image data")),
            _ => {}
        }
        let len = usize::from(reader.u16_be()?)
            .checked_sub(2)
            .ok_or(ImageError::Invalid("bad segment length"))?;
        match marker {
            0xdb => read_quant(reader, len, &mut tables.quant)?,
            0xc0..=0xc2 => {
                progressive = marker == 0xc2;
                (width, height) = read_frame(reader, len, &mut components)?;
            }
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
                return Err(ImageError::Unsupported("lossless or arithmetic-coded JPEG"));
            }
            0xc4 => read_huffman(reader, len, &mut tables.dc, &mut tables.ac)?,
            0xdd => {
                restart_interval = u32::from(reader.u16_be()?);
                reader.skip(len.saturating_sub(2))?;
            }
            0xda if progressive => {
                if components.is_empty() {
                    return Err(ImageError::Invalid("scan before frame"));
                }
                let frame = crate::progressive::Frame {
                    width,
                    height,
                    components,
                    tables,
                    restart_interval,
                };
                return crate::progressive::decode(reader, frame, fit, sink);
            }
            0xda => {
                read_scan_header(reader, &mut components)?;
                return scan(
                    reader,
                    &mut components,
                    &tables,
                    (width, height, restart_interval),
                    fit,
                    sink,
                );
            }
            _ => reader.skip(len)?,
        }
    }
}

/// A frame header (`SOF`) of `len` bytes, its components into
/// `components`: the image's width and height.
fn read_frame(
    reader: &mut Reader<'_>,
    len: usize,
    components: &mut Vec<Component>,
) -> Result<(u32, u32), ImageError> {
    if reader.byte()? != 8 {
        return Err(ImageError::Unsupported("12-bit JPEG"));
    }
    let height = u32::from(reader.u16_be()?);
    let width = u32::from(reader.u16_be()?);
    let count = reader.byte()?;
    if width == 0 || height == 0 || !(count == 1 || count == 3) {
        return Err(ImageError::Unsupported("JPEG components"));
    }
    components.clear();
    // The room for all of them first. A refusal is an error here, and
    // `push` never grows the list.
    components
        .try_reserve_exact(usize::from(count))
        .map_err(|_| ImageError::OutOfMemory)?;
    for _ in 0..count {
        let id = reader.byte()?;
        let hv = reader.byte()?;
        let quant = usize::from(reader.byte()? & 3);
        let (h, v) = (usize::from(hv >> 4), usize::from(hv & 15));
        if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
            return Err(ImageError::Invalid("bad sampling"));
        }
        components.push(Component {
            id,
            h,
            v,
            quant,
            ..Component::default()
        });
    }
    reader.skip(len.saturating_sub(6 + 3 * usize::from(count)))?;
    Ok((width, height))
}

/// A baseline scan's header (`SOS`): the tables each component takes.
fn read_scan_header(
    reader: &mut Reader<'_>,
    components: &mut [Component],
) -> Result<(), ImageError> {
    let count = usize::from(reader.byte()?);
    if components.is_empty() || count != components.len() {
        return Err(ImageError::Unsupported("non-interleaved JPEG scans"));
    }
    for _ in 0..count {
        let id = reader.byte()?;
        let tables = reader.byte()?;
        let component = components
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(ImageError::Invalid("unknown component"))?;
        component.dc = usize::from(tables >> 4 & 3);
        component.ac = usize::from(tables & 3);
    }
    reader.skip(3)
}

/// A quantization table segment (`DQT`) of `len` bytes.
pub(crate) fn read_quant(
    reader: &mut Reader<'_>,
    len: usize,
    quant: &mut [[u16; 64]],
) -> Result<(), ImageError> {
    let mut left = len;
    while left > 0 {
        let pq_tq = reader.byte()?;
        let table = quant
            .get_mut(usize::from(pq_tq & 15))
            .ok_or(ImageError::Invalid("bad table"))?;
        let wide = pq_tq >> 4 != 0;
        for entry in table.iter_mut() {
            *entry = if wide {
                reader.u16_be()?
            } else {
                u16::from(reader.byte()?)
            };
        }
        left = left.saturating_sub(1 + if wide { 128 } else { 64 });
    }
    Ok(())
}

/// A Huffman table segment (`DHT`) of `len` bytes.
pub(crate) fn read_huffman(
    reader: &mut Reader<'_>,
    len: usize,
    dc_tables: &mut [Option<Huffman>],
    ac_tables: &mut [Option<Huffman>],
) -> Result<(), ImageError> {
    let mut left = len;
    while left >= 17 {
        let tc_th = reader.byte()?;
        let mut counts = [0u8; 16];
        reader.fill(&mut counts)?;
        let total: usize = counts.iter().map(|&c| usize::from(c)).sum();
        if total > 256 {
            return Err(ImageError::Invalid("bad Huffman table"));
        }
        let mut values = zeroed(total)?;
        reader.fill(&mut values)?;
        let table = Some(Huffman::new(&counts, values)?);
        let index = usize::from(tc_th & 3);
        let tables = if tc_th >> 4 == 0 {
            &mut *dc_tables
        } else {
            &mut *ac_tables
        };
        *tables
            .get_mut(index)
            .ok_or(ImageError::Invalid("bad Huffman table"))? = table;
        left = left
            .checked_sub(17 + total)
            .ok_or(ImageError::Invalid("bad Huffman table"))?;
    }
    reader.skip(left)
}

#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn scan(
    reader: &mut Reader<'_>,
    components: &mut [Component],
    tables: &Tables,
    (width, height, restart_interval): (u32, u32, u32),
    fit: Fit,
    sink: &mut dyn ImageSink,
) -> Result<(), ImageError> {
    let (quant, dc_tables, ac_tables) = (&tables.quant, &tables.dc, &tables.ac);
    for c in components.iter() {
        if dc_tables[c.dc].is_none() || ac_tables[c.ac].is_none() {
            return Err(ImageError::Invalid("missing Huffman table"));
        }
    }
    let ((h_max, v_max), (sx, sy)) = sampling(components);
    let mcus_x = (width as usize).div_ceil(8 * h_max);
    let mcus_y = (height as usize).div_ceil(8 * v_max);

    // Each block is decoded at n×n pixels of its 8×8 (8, 4, 2 or 1). An
    // image made smaller is made smaller as it is decoded, from its lowest
    // frequencies, not decoded whole and averaged. Then it is averaged to
    // the size it shows at.
    let size = fit.size(width, height);
    let n = block_side(width, height, size);
    let scale = (8 / n) as u32;
    let (out_width, out_height) = (width.div_ceil(scale), height.div_ceil(scale));
    let mcu_h = n * v_max;
    let row_width = mcus_x * n * h_max;

    // Colour where it is wanted and the image has it (luma and chroma).
    let color = sink.wants_color() && components.len() == 3;
    let mut scaler = Downscaler::new(out_width, out_height, size, if color { 3 } else { 1 })?;
    sink.start(scaler.out_width(), scaler.out_height())?;
    let height = out_height;

    let mut planes = Planes::new(color, row_width, mcu_h)?;
    let mut block = [0u8; 64];
    // Zeroed for each block that is shown. A dropped one keeps none of its
    // coefficients: chroma for gray, two of three blocks.
    let mut coef = [0i32; 64];
    let mut bits = Bits::new(reader);
    let mut until_restart = restart_interval;
    let mut emitted = 0u32;
    let blocks_per_mcu: u64 = components.iter().map(|c| (c.h * c.v) as u64).sum();
    let mut blocks = 0u64;
    // The data ended, or asked for more work than it may. The rest of the
    // image is as data of zeros leaves it: each block at the level of its
    // component's last DC.
    let mut ended = false;

    for _ in 0..mcus_y {
        if ended {
            break;
        }
        for mx in 0..mcus_x {
            if restart_interval > 0 && !ended {
                count_restart(&mut bits, components, &mut until_restart, restart_interval)?;
            }
            blocks += blocks_per_mcu;
            let exhausted = bits.exhausted()?;
            if exhausted && !bits.resumes(restart_interval) || !bits.affords(blocks) {
                ended = true;
            }
            // Without data, up to the next restart or for good, each block
            // has only its DC, the last one.
            let flat = ended || exhausted;
            for (index, component) in components.iter_mut().enumerate() {
                let dc = dc_tables[component.dc]
                    .as_ref()
                    .ok_or(ImageError::Invalid("table"))?;
                let ac = ac_tables[component.ac]
                    .as_ref()
                    .ok_or(ImageError::Invalid("table"))?;
                let q = &quant[component.quant];
                // Whether the component's blocks are wanted.
                let shown = index == 0 || color;
                // The block's stretch where the component has less
                // resolution than the largest.
                let (sx, sy) = if index == 0 {
                    (sx, sy)
                } else {
                    (h_max / component.h, v_max / component.v)
                };
                for by in 0..component.v {
                    for bx in 0..component.h {
                        if flat {
                            if !shown {
                                continue;
                            }
                            let dc_value = component.prediction.saturating_mul(i32::from(q[0]));
                            block[..n * n].fill(dc_level(dc_value));
                        } else {
                            let tables = (dc, ac, q);
                            let at = (&mut coef, &mut block);
                            decode_block(&mut bits, tables, component, shown, n, at)?;
                            if !shown {
                                continue;
                            }
                        }
                        let plane = planes.plane(index);
                        let x0 = (mx * h_max + bx * sx) * n;
                        let y0 = by * sy * n;
                        place_block(&block, plane, n, (sx, sy), (x0, y0), row_width);
                    }
                }
            }
        }
        // Data cut short in this MCU row is an error.
        bits.exhausted()?;
        planes.hand_over(
            (row_width, mcu_h),
            (&mut emitted, height),
            &mut scaler,
            sink,
        )?;
    }
    if ended && emitted < height {
        // Every row below is alike, each block at the same levels.
        let level =
            |c: &Component| dc_level(c.prediction.saturating_mul(i32::from(quant[c.quant][0])));
        let alike = planes.alike(components, row_width, level);
        scaler.rows_alike(alike, height - emitted, sink)?;
    }
    scaler.finish(sink)
}

/// Counts an MCU towards the next restart marker, and reads that marker
/// where it is due: the components predict their DC from zero again.
#[inline(always)]
fn count_restart(
    bits: &mut Bits<'_, '_>,
    components: &mut [Component],
    until_restart: &mut u32,
    restart_interval: u32,
) -> Result<(), ImageError> {
    if *until_restart == 0 {
        bits.restart()?;
        for c in components.iter_mut() {
            c.prediction = 0;
        }
        *until_restart = restart_interval;
    }
    *until_restart -= 1;
    Ok(())
}

/// The largest sampling factors of a baseline scan's components, and how
/// far luma is stretched to them.
fn sampling(components: &mut [Component]) -> ((usize, usize), (usize, usize)) {
    // A single component is not interleaved: its MCU is one block.
    let single = components.len() == 1;
    if single {
        components[0].h = 1;
        components[0].v = 1;
    }
    let h_max = components.iter().map(|c| c.h).max().unwrap_or(1);
    let v_max = components.iter().map(|c| c.v).max().unwrap_or(1);
    let luma = components[0];
    // Luma at less than full resolution is rare. It is stretched.
    ((h_max, v_max), (h_max / luma.h, v_max / luma.v))
}

/// One row of MCUs of a baseline scan: luma at full resolution, and for
/// colour each chroma component stretched to it and a row of RGB.
struct Planes {
    color: bool,
    rows: Vec<u8>,
    chroma: [Vec<u8>; 2],
    rgb: Vec<u8>,
}

impl Planes {
    fn new(color: bool, row_width: usize, mcu_h: usize) -> Result<Self, ImageError> {
        let rows: Vec<u8> = zeroed(row_width * mcu_h)?;
        let chroma: [Vec<u8>; 2] = if color {
            [zeroed(row_width * mcu_h)?, zeroed(row_width * mcu_h)?]
        } else {
            [Vec::new(), Vec::new()]
        };
        let rgb: Vec<u8> = if color {
            zeroed(row_width * 3)?
        } else {
            Vec::new()
        };
        Ok(Planes {
            color,
            rows,
            chroma,
            rgb,
        })
    }

    /// The plane of component `index`.
    #[inline(always)]
    fn plane(&mut self, index: usize) -> &mut [u8] {
        match index {
            0 => &mut self.rows,
            1 => &mut self.chroma[0],
            _ => &mut self.chroma[1],
        }
    }

    /// Hands over the rows of this MCU row that belong to the image, up to
    /// its `height`. `emitted` counts them.
    #[inline(always)]
    fn hand_over(
        &mut self,
        (row_width, mcu_h): (usize, usize),
        (emitted, height): (&mut u32, u32),
        scaler: &mut Downscaler,
        sink: &mut dyn ImageSink,
    ) -> Result<(), ImageError> {
        for y in 0..mcu_h {
            if *emitted == height {
                break;
            }
            let luma_row = &self.rows[y * row_width..(y + 1) * row_width];
            if self.color {
                let at = y * row_width;
                let (cb, cr) = (
                    &self.chroma[0][at..at + row_width],
                    &self.chroma[1][at..at + row_width],
                );
                for x in 0..row_width {
                    let pixel = ycbcr_to_rgb(luma_row[x], cb[x], cr[x]);
                    self.rgb[x * 3..x * 3 + 3].copy_from_slice(&pixel);
                }
                scaler.row(&self.rgb, sink)?;
            } else {
                scaler.row(luma_row, sink)?;
            }
            *emitted += 1;
        }
        Ok(())
    }

    /// A row where every block is at its component's `level`.
    fn alike(
        &mut self,
        components: &[Component],
        row_width: usize,
        level: impl Fn(&Component) -> u8,
    ) -> &[u8] {
        if !self.color {
            self.rows[..row_width].fill(level(&components[0]));
            return &self.rows[..row_width];
        }
        let pixel = ycbcr_to_rgb(
            level(&components[0]),
            level(&components[1]),
            level(&components[2]),
        );
        for to in self.rgb.as_chunks_mut::<3>().0 {
            to.copy_from_slice(&pixel);
        }
        &self.rgb
    }
}

/// Decodes the next block of `component` from `bits`. Where it is
/// `shown`, its coefficients go to `coef` and its n×n pixels to `block`.
#[inline(always)]
fn decode_block(
    bits: &mut Bits<'_, '_>,
    (dc, ac, q): (&Huffman, &Huffman, &[u16; 64]),
    component: &mut Component,
    shown: bool,
    n: usize,
    (coef, block): (&mut [i32; 64], &mut [u8; 64]),
) -> Result<(), ImageError> {
    if shown {
        *coef = [0i32; 64];
    }
    let t = bits.decode(dc)?;
    if t > 16 {
        return Err(ImageError::Invalid("bad DC"));
    }
    let diff = Bits::extend(bits.receive(u32::from(t))?, u32::from(t));
    // Damaged data must not overflow.
    component.prediction = component.prediction.wrapping_add(diff);
    coef[0] = component.prediction.saturating_mul(i32::from(q[0]));
    // Whether a coefficient decoded counts at this size.
    let mut shaded = false;
    let mut k = 1;
    while k < 64 {
        let rs = bits.decode(ac)?;
        let (r, s) = (usize::from(rs >> 4), u32::from(rs & 15));
        if s == 0 {
            if r == 15 {
                k += 16;
                continue;
            }
            break;
        }
        k += r;
        if k > 63 {
            return Err(ImageError::Invalid("bad AC"));
        }
        if shown {
            let value = Bits::extend(bits.receive(s)?, s);
            let at = usize::from(ZIGZAG[k]);
            coef[at] = value.saturating_mul(i32::from(q[k]));
            shaded |= at % 8 < n && at / 8 < n;
        } else {
            bits.receive(s)?;
        }
        k += 1;
    }
    if !shown {
        return Ok(());
    }
    if shaded {
        idct_scaled(coef, n, block, n);
    } else {
        // One level: most blocks of a picture made smaller.
        block[..n * n].fill(dc_level(coef[0]));
    }
    Ok(())
}

/// Places the n×n `block` in `plane` at `(x0, y0)`, stretched `sx` times
/// across and `sy` times down.
#[inline(always)]
fn place_block(
    block: &[u8; 64],
    plane: &mut [u8],
    n: usize,
    (sx, sy): (usize, usize),
    (x0, y0): (usize, usize),
    row_width: usize,
) {
    if (sx, sy) == (1, 1) {
        for (y, from) in block.chunks_exact(n).take(n).enumerate() {
            let at = (y0 + y) * row_width + x0;
            plane[at..at + n].copy_from_slice(from);
        }
    } else {
        for y in 0..n * sy {
            for x in 0..n * sx {
                let at = (y0 + y) * row_width + x0 + x;
                plane[at] = block[(y / sy) * n + x / sx];
            }
        }
    }
}

/// JFIF's YCbCr as RGB.
pub(crate) fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let y = i32::from(y);
    let (cb, cr) = (i32::from(cb) - 128, i32::from(cr) - 128);
    let clamp = |v: i32| v.clamp(0, 255) as u8;
    [
        clamp(y + ((91_881 * cr + 32_768) >> 16)),
        clamp(y - ((22_554 * cb + 46_802 * cr + 32_768) >> 16)),
        clamp(y + ((116_130 * cb + 32_768) >> 16)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_size_idct_as_the_general_one() {
        // Any coefficients: the shortcut gives the general sums' numbers.
        let mut seed = 7u32;
        for _ in 0..500 {
            let mut coef = [0i32; 64];
            for c in coef.iter_mut() {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                *c = (seed >> 16) as i32 % 3000 - 1500;
            }
            let (mut fast, mut general) = ([0u8; 16], [0u8; 16]);
            idct4(&coef, &mut fast, 4);
            let c = &C4;
            let mut tmp = [0i32; 64];
            for v in 0..4 {
                for x in 0..4 {
                    tmp[v * 8 + x] = (0..4).map(|u| coef[v * 8 + u] * c[u][x]).sum::<i32>() >> 10;
                }
            }
            for x in 0..4 {
                for y in 0..4 {
                    let sum: i32 = (0..4).map(|v| tmp[v * 8 + x] * c[v][y]).sum();
                    general[y * 4 + x] = (((sum + (1 << 13)) >> 14) + 128).clamp(0, 255) as u8;
                }
            }
            assert_eq!(fast, general);
        }
    }

    /// The sums of [`idct_scaled`] before its shortcuts, for `n` × `n`
    /// frequencies of basis `c`.
    fn general(coef: &[i32; 64], n: usize, c: &[[i32; 8]; 8]) -> [u8; 64] {
        let mut tmp = [0i32; 64];
        for v in 0..n {
            for x in 0..n {
                let sum: i32 = (0..n)
                    .map(|u| coef[v * 8 + u].clamp(-COEFFICIENT, COEFFICIENT) * c[u][x])
                    .sum();
                tmp[v * 8 + x] = sum >> 10;
            }
        }
        let mut out = [0u8; 64];
        for x in 0..n {
            for y in 0..n {
                let sum: i32 = (0..n).map(|v| tmp[v * 8 + x] * c[v][y]).sum();
                out[y * n + x] = (((sum + (1 << 13)) >> 14) + 128).clamp(0, 255) as u8;
            }
        }
        out
    }

    #[test]
    fn full_size_idct_as_the_general_one() {
        let mut seed = 11u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 16) as i32
        };
        // Any coefficients, sparse ones as pictures have them (most lines
        // without odd frequencies, most blocks with few), and ones past
        // what a valid file gives, at the bounds and beyond.
        for round in 0..4000 {
            let mut coef = [0i32; 64];
            for (at, c) in coef.iter_mut().enumerate() {
                let value = next();
                *c = match round % 5 {
                    0 => value % 3000 - 1500,
                    1 if at % 8 < 3 && at / 8 < 3 => value % 600 - 300,
                    2 if at % 2 == 0 && value % 3 == 0 => value % 2000 - 1000,
                    3 => [-40_000_000, -4096, -4095, 0, 4095, 4096, i32::MAX, i32::MIN]
                        [value as usize % 8],
                    4 if value % 7 == 0 => value % 9000 - 4500,
                    _ => 0,
                };
            }
            let mut fast = [0u8; 64];
            idct8(&coef, &mut fast, 8);
            assert_eq!(fast, general(&coef, 8, &C8), "round {round}: {coef:?}");
            let mut through = [0u8; 64];
            idct_scaled(&coef, 8, &mut through, 8);
            assert_eq!(through, fast);
        }
    }

    #[test]
    fn ycbcr_primaries() {
        assert_eq!(ycbcr_to_rgb(128, 128, 128), [128, 128, 128]);
        // Pure red, green and blue as JFIF codes them.
        let close = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2);
        assert!(close(ycbcr_to_rgb(76, 85, 255), [255, 0, 0]));
        assert!(close(ycbcr_to_rgb(150, 44, 21), [0, 255, 0]));
        assert!(close(ycbcr_to_rgb(29, 255, 107), [0, 0, 255]));
    }

    #[test]
    fn idct_of_a_flat_block_is_flat() {
        let mut coef = [0i32; 64];
        // DC 80 → 80/8 = 10 above mid-gray.
        coef[0] = 80;
        let mut out = [0u8; 64];
        idct_scaled(&coef, 8, &mut out, 8);
        assert!(out.iter().all(|&v| (137..=139).contains(&v)), "{out:?}");
    }
}
