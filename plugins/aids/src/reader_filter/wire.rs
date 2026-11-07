// Generated stage codec; do not edit.
pub const RECORD_MAX_BYTES: usize = 3840;
pub const RECORD_MAX_RECORDS: usize = 33;
struct Reader<'a> {
    remaining: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], i32> {
        if len > self.remaining.len() {
            return Err(-2);
        }
        let (value, rest) = self.remaining.split_at(len);
        self.remaining = rest;
        Ok(value)
    }

    fn varint(&mut self) -> Result<u32, i32> {
        let mut value = 0_u32;
        for index in 0..5 {
            let byte = self.take(1)?[0];
            if index == 4 && byte > 0x0f {
                return Err(-2);
            }
            value |= u32::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                if index > 0 && byte == 0 {
                    return Err(-2);
                }
                return Ok(value);
            }
        }
        Err(-2)
    }

    fn scalar<const N: usize>(&mut self) -> Result<[u8; N], i32> {
        let mut value = [0; N];
        value.copy_from_slice(self.take(N)?);
        Ok(value)
    }

    fn record(&mut self) -> Result<(u32, Reader<'a>), i32> {
        let tag = self.varint()?;
        let len = self.varint()? as usize;
        let body = Reader {
            remaining: self.take(len)?,
        };
        Ok((tag, body))
    }

    fn bitmap(&mut self) -> Result<(), i32> {
        let len = self.varint()? as usize;
        // Unknown optional bits and fields are skipped using the record length.
        self.take(len)?;
        Ok(())
    }

    fn finish(self) -> Result<(), i32> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(-2)
        }
    }
}

fn varint_len(mut value: u32) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        len += 1;
        value >>= 7;
    }
    len
}

fn record_len(tag: u32, fields: u32) -> usize {
    let body_len = fields + 1; // The initial bitmap is empty.
    varint_len(tag) + varint_len(body_len) + body_len as usize
}

fn write_varint(mut value: u32, out: &mut [u8], offset: &mut usize) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out[*offset] = byte | if value == 0 { 0 } else { 0x80 };
        *offset += 1;
        if value == 0 {
            return;
        }
    }
}

fn write_bytes(bytes: &[u8], out: &mut [u8], offset: &mut usize) {
    out[*offset..*offset + bytes.len()].copy_from_slice(bytes);
    *offset += bytes.len();
}

fn write_header(tag: u32, fields: u32, out: &mut [u8], offset: &mut usize) {
    write_varint(tag, out, offset);
    write_varint(fields + 1, out, offset);
    write_varint(0, out, offset);
}


// Declaration records and the mixed-batch API.
/// Wire tag of the `filter.rules` record.
pub const FILTER_RULES_TAG: u32 = 1;
/// Maximum bytes of `filter.rules.caption_word`.
pub const FILTER_RULES_CAPTION_WORD_MAX_BYTES: usize = 96;

/// The `filter.rules` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterRules<'a> {
    /// Little-endian u64.
    pub signature: u64,
    /// Little-endian u32.
    pub bold: u32,
    /// Little-endian u32.
    pub pictures: u32,
    /// UTF-8 text of at most 96 bytes.
    pub caption_word: &'a str,
}

impl<'a> FilterRules<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::FilterRules(Self {
            signature: u64::from_le_bytes(body.scalar()?),
            bold: u32::from_le_bytes(body.scalar()?),
            pictures: u32::from_le_bytes(body.scalar()?),
            caption_word: body.string(FILTER_RULES_CAPTION_WORD_MAX_BYTES)?,
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 8;
        len += 4;
        len += 4;
        len += blob_len(self.caption_word.len(), FILTER_RULES_CAPTION_WORD_MAX_BYTES)?;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.signature.to_le_bytes(), out, offset);
        write_bytes(&self.bold.to_le_bytes(), out, offset);
        write_bytes(&self.pictures.to_le_bytes(), out, offset);
        write_blob(self.caption_word.as_bytes(), out, offset);
    }
}
/// Wire tag of the `filter.pair` record.
pub const FILTER_PAIR_TAG: u32 = 2;
/// Maximum bytes of `filter.pair.from`.
pub const FILTER_PAIR_FROM_MAX_BYTES: usize = 96;
/// Maximum bytes of `filter.pair.to`.
pub const FILTER_PAIR_TO_MAX_BYTES: usize = 96;

/// The `filter.pair` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterPair<'a> {
    /// UTF-8 text of at most 96 bytes.
    pub from: &'a str,
    /// UTF-8 text of at most 96 bytes.
    pub to: &'a str,
    /// Little-endian u32.
    pub whole_word: u32,
}

impl<'a> FilterPair<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::FilterPair(Self {
            from: body.string(FILTER_PAIR_FROM_MAX_BYTES)?,
            to: body.string(FILTER_PAIR_TO_MAX_BYTES)?,
            whole_word: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += blob_len(self.from.len(), FILTER_PAIR_FROM_MAX_BYTES)?;
        len += blob_len(self.to.len(), FILTER_PAIR_TO_MAX_BYTES)?;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_blob(self.from.as_bytes(), out, offset);
        write_blob(self.to.as_bytes(), out, offset);
        write_bytes(&self.whole_word.to_le_bytes(), out, offset);
    }
}

/// A registration or declaration record, borrowed from one validated batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Record<'a> {
    /// A `FilterRules` record.
    FilterRules(FilterRules<'a>),
    /// A `FilterPair` record.
    FilterPair(FilterPair<'a>),
}

impl<'a> Reader<'a> {
    fn blob(&mut self, cap: usize) -> Result<&'a [u8], i32> {
        let len = self.varint()? as usize;
        if len > cap {
            return Err(-3);
        }
        self.take(len)
    }

    fn string(&mut self, cap: usize) -> Result<&'a str, i32> {
        core::str::from_utf8(self.blob(cap)?).map_err(|_| -5)
    }
}

fn record_decode<'a>(tag: u32, body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
    Ok(match tag {
        FILTER_RULES_TAG => FilterRules::decode(body)?,
        FILTER_PAIR_TAG => FilterPair::decode(body)?,
        _ => return Err(-1),
    })
}

/// A fully validated, allocation-free batch of registrations and declarations.
///
/// Every record is decoded once, by [`records_decode`], and kept as decoded:
/// what is staged is exactly what was validated, and iteration cannot fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordBatch<'a> {
    records: [Option<Record<'a>>; RECORD_MAX_RECORDS],
    len: usize,
}

impl<'a> RecordBatch<'a> {
    /// Number of validated records.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether this batch has no records.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate over records by value in wire order.
    pub fn iter(&self) -> RecordIter<'_, 'a> {
        RecordIter {
            records: self.records[..self.len].iter(),
        }
    }
}

/// Iterator over a validated [`RecordBatch`].
#[derive(Clone, Debug)]
pub struct RecordIter<'b, 'a> {
    records: core::slice::Iter<'b, Option<Record<'a>>>,
}

impl<'a> Iterator for RecordIter<'_, 'a> {
    type Item = Record<'a>;

    fn next(&mut self) -> Option<Record<'a>> {
        // The first `len` places all hold a record (`records_decode`).
        self.records.find_map(|record| *record)
    }
}

/// Decode an entire mixed batch before exposing any record.
///
/// Accepts the registration tags and every declaration tag. Framing must be
/// canonical u32 LEB128. Unknown tags (and the tick event) return
/// ERR_UNSUPPORTED; malformed records return ERR_INVALID; invalid UTF-8 returns
/// ERR_UTF8; byte, count and per-field cap violations return ERR_NO_SPACE.
/// Unknown trailing fields and extended presence bitmaps are accepted.
pub fn records_decode(bytes: &[u8]) -> Result<RecordBatch<'_>, i32> {
    if bytes.len() > RECORD_MAX_BYTES {
        return Err(-3);
    }
    let mut reader = Reader { remaining: bytes };
    let len = reader.varint()? as usize;
    if len > RECORD_MAX_RECORDS {
        return Err(-3);
    }
    let mut records = [None; RECORD_MAX_RECORDS];
    for place in records.iter_mut().take(len) {
        let (tag, mut body) = reader.record()?;
        *place = Some(record_decode(tag, &mut body)?);
    }
    reader.finish()?;
    Ok(RecordBatch { records, len })
}

fn blob_len(len: usize, cap: usize) -> Result<usize, i32> {
    if len > cap {
        Err(-3)
    } else {
        Ok(varint_len(len as u32) + len)
    }
}

fn write_blob(bytes: &[u8], out: &mut [u8], offset: &mut usize) {
    write_varint(bytes.len() as u32, out, offset);
    write_bytes(bytes, out, offset);
}

impl Record<'_> {
    fn tag(&self) -> u32 {
        match self {
            Record::FilterRules(_) => FILTER_RULES_TAG,
            Record::FilterPair(_) => FILTER_PAIR_TAG,
        }
    }

    fn body_len(&self) -> Result<usize, i32> {
        match self {
            Record::FilterRules(record) => record.body_len(),
            Record::FilterPair(record) => record.body_len(),
        }
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        match self {
            Record::FilterRules(record) => record.write(out, offset),
            Record::FilterPair(record) => record.write(out, offset),
        }
    }
}

/// Encode a canonical mixed batch without allocating.
///
/// Every field cap, the byte and count limits and the complete output capacity
/// are checked before any write. Failure returns ERR_NO_SPACE and leaves the
/// entire output unchanged.
pub fn records_encode(records: &[Record<'_>], out: &mut [u8]) -> Result<usize, i32> {
    if records.len() > RECORD_MAX_RECORDS {
        return Err(-3);
    }
    let mut required = varint_len(records.len() as u32);
    for record in records {
        let body = record.body_len()?;
        required += varint_len(record.tag()) + varint_len(body as u32) + body;
        if required > RECORD_MAX_BYTES {
            return Err(-3);
        }
    }
    if out.len() < required {
        return Err(-3);
    }
    let mut offset = 0;
    write_varint(records.len() as u32, out, &mut offset);
    for record in records {
        let body = record.body_len()?;
        write_varint(record.tag(), out, &mut offset);
        write_varint(body as u32, out, &mut offset);
        write_varint(0, out, &mut offset);
        record.write(out, &mut offset);
    }
    Ok(offset)
}
