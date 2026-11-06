// Generated stage codec; do not edit.
pub const RECORD_MAX_BYTES: usize = 4096;
pub const RECORD_MAX_RECORDS: usize = 8;
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
/// Wire tag of the `insert.query` record.
pub const INSERT_QUERY_TAG: u32 = 16;
/// Maximum bytes of `insert.query.language`.
pub const INSERT_QUERY_LANGUAGE_MAX_BYTES: usize = 16;

/// The `insert.query` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertQuery<'a> {
    /// Little-endian u32.
    pub section: u32,
    /// Little-endian u32.
    pub start: u32,
    /// Little-endian u32.
    pub end: u32,
    /// Little-endian u32.
    pub kind: u32,
    /// Little-endian u32.
    pub width: u32,
    /// Little-endian u32.
    pub page_height: u32,
    /// Little-endian u32.
    pub font_size: u32,
    /// UTF-8 text of at most 16 bytes.
    pub language: &'a str,
}

impl<'a> InsertQuery<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::InsertQuery(Self {
            section: u32::from_le_bytes(body.scalar()?),
            start: u32::from_le_bytes(body.scalar()?),
            end: u32::from_le_bytes(body.scalar()?),
            kind: u32::from_le_bytes(body.scalar()?),
            width: u32::from_le_bytes(body.scalar()?),
            page_height: u32::from_le_bytes(body.scalar()?),
            font_size: u32::from_le_bytes(body.scalar()?),
            language: body.string(INSERT_QUERY_LANGUAGE_MAX_BYTES)?,
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += blob_len(self.language.len(), INSERT_QUERY_LANGUAGE_MAX_BYTES)?;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.section.to_le_bytes(), out, offset);
        write_bytes(&self.start.to_le_bytes(), out, offset);
        write_bytes(&self.end.to_le_bytes(), out, offset);
        write_bytes(&self.kind.to_le_bytes(), out, offset);
        write_bytes(&self.width.to_le_bytes(), out, offset);
        write_bytes(&self.page_height.to_le_bytes(), out, offset);
        write_bytes(&self.font_size.to_le_bytes(), out, offset);
        write_blob(self.language.as_bytes(), out, offset);
    }
}
/// Wire tag of the `insert.item` record.
pub const INSERT_ITEM_TAG: u32 = 17;

/// The `insert.item` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertItem {
    /// Little-endian u32.
    pub side: u32,
    /// Little-endian u32.
    pub id: u32,
    /// Little-endian u32.
    pub height: u32,
    /// Little-endian u32.
    pub flags: u32,
}

impl InsertItem {
    fn decode<'a>(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::InsertItem(Self {
            side: u32::from_le_bytes(body.scalar()?),
            id: u32::from_le_bytes(body.scalar()?),
            height: u32::from_le_bytes(body.scalar()?),
            flags: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.side.to_le_bytes(), out, offset);
        write_bytes(&self.id.to_le_bytes(), out, offset);
        write_bytes(&self.height.to_le_bytes(), out, offset);
        write_bytes(&self.flags.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `style.query` record.
pub const STYLE_QUERY_TAG: u32 = 32;
/// Maximum bytes of `style.query.text`.
pub const STYLE_QUERY_TEXT_MAX_BYTES: usize = 1024;

/// The `style.query` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleQuery<'a> {
    /// Little-endian u32.
    pub section: u32,
    /// Little-endian u32.
    pub start: u32,
    /// Little-endian u32.
    pub end: u32,
    /// UTF-8 text of at most 1024 bytes.
    pub text: &'a str,
    /// Little-endian u32.
    pub chunk: u32,
    /// Little-endian u32.
    pub final_chunk: u32,
}

impl<'a> StyleQuery<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::StyleQuery(Self {
            section: u32::from_le_bytes(body.scalar()?),
            start: u32::from_le_bytes(body.scalar()?),
            end: u32::from_le_bytes(body.scalar()?),
            text: body.string(STYLE_QUERY_TEXT_MAX_BYTES)?,
            chunk: u32::from_le_bytes(body.scalar()?),
            final_chunk: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += blob_len(self.text.len(), STYLE_QUERY_TEXT_MAX_BYTES)?;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.section.to_le_bytes(), out, offset);
        write_bytes(&self.start.to_le_bytes(), out, offset);
        write_bytes(&self.end.to_le_bytes(), out, offset);
        write_blob(self.text.as_bytes(), out, offset);
        write_bytes(&self.chunk.to_le_bytes(), out, offset);
        write_bytes(&self.final_chunk.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `style.block` record.
pub const STYLE_BLOCK_TAG: u32 = 33;

/// The `style.block` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleBlock {
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub align: Option<u32>,
    /// Little-endian i32. Appended and optional; `None` when absent.
    pub indent: Option<i32>,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub space: Option<u32>,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub size: Option<u32>,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub line_height: Option<u32>,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub margin_start: Option<u32>,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub margin_end: Option<u32>,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub drop_cap: Option<u32>,
}

impl StyleBlock {
    fn decode<'a>(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        let bits = body.bitmap_bits()?;
        Ok(Record::StyleBlock(Self {
            align: body.maybe_u32(bits, 0x01)?,
            indent: body.maybe_i32(bits, 0x02)?,
            space: body.maybe_u32(bits, 0x04)?,
            size: body.maybe_u32(bits, 0x08)?,
            line_height: body.maybe_u32(bits, 0x10)?,
            margin_start: body.maybe_u32(bits, 0x20)?,
            margin_end: body.maybe_u32(bits, 0x40)?,
            drop_cap: body.maybe_u32(bits, 0x80)?,
        }))
    }

    fn bits(&self) -> u8 {
        let mut bits = 0;
        if self.align.is_some() {
            bits |= 0x01;
        }
        if self.indent.is_some() {
            bits |= 0x02;
        }
        if self.space.is_some() {
            bits |= 0x04;
        }
        if self.size.is_some() {
            bits |= 0x08;
        }
        if self.line_height.is_some() {
            bits |= 0x10;
        }
        if self.margin_start.is_some() {
            bits |= 0x20;
        }
        if self.margin_end.is_some() {
            bits |= 0x40;
        }
        if self.drop_cap.is_some() {
            bits |= 0x80;
        }
        bits
    }

    fn body_len(&self) -> Result<usize, i32> {
        let bits = self.bits();
        // The bitmap is one byte when any optional field is present.
        let mut len = 1 + usize::from(bits != 0);
        if bits & 0x01 != 0 {
            len += 4;
        }
        if bits & 0x02 != 0 {
            len += 4;
        }
        if bits & 0x04 != 0 {
            len += 4;
        }
        if bits & 0x08 != 0 {
            len += 4;
        }
        if bits & 0x10 != 0 {
            len += 4;
        }
        if bits & 0x20 != 0 {
            len += 4;
        }
        if bits & 0x40 != 0 {
            len += 4;
        }
        if bits & 0x80 != 0 {
            len += 4;
        }
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        if let Some(value) = self.align {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.indent {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.space {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.size {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.line_height {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.margin_start {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.margin_end {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.drop_cap {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
    }
}
/// Wire tag of the `style.run` record.
pub const STYLE_RUN_TAG: u32 = 34;
/// Maximum bytes of `style.run.family`.
pub const STYLE_RUN_FAMILY_MAX_BYTES: usize = 64;

/// The `style.run` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleRun<'a> {
    /// Little-endian u32.
    pub start: u32,
    /// Little-endian u32.
    pub end: u32,
    /// Little-endian u32.
    pub set: u32,
    /// Little-endian u32.
    pub clear: u32,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub color: Option<u32>,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub weight: Option<u32>,
    /// UTF-8 text of at most 64 bytes. Appended and optional; empty when absent.
    pub family: &'a str,
    /// Little-endian i32. Appended and optional; `None` when absent.
    pub letter_spacing: Option<i32>,
    /// Little-endian i32. Appended and optional; `None` when absent.
    pub word_spacing: Option<i32>,
}

impl<'a> StyleRun<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        let bits = body.bitmap_bits()?;
        Ok(Record::StyleRun(Self {
            start: u32::from_le_bytes(body.scalar()?),
            end: u32::from_le_bytes(body.scalar()?),
            set: u32::from_le_bytes(body.scalar()?),
            clear: u32::from_le_bytes(body.scalar()?),
            color: body.maybe_u32(bits, 0x01)?,
            weight: body.maybe_u32(bits, 0x02)?,
            family: body.opt_string(bits, 0x04, STYLE_RUN_FAMILY_MAX_BYTES)?,
            letter_spacing: body.maybe_i32(bits, 0x08)?,
            word_spacing: body.maybe_i32(bits, 0x10)?,
        }))
    }

    fn bits(&self) -> u8 {
        let mut bits = 0;
        if self.color.is_some() {
            bits |= 0x01;
        }
        if self.weight.is_some() {
            bits |= 0x02;
        }
        if !self.family.is_empty() {
            bits |= 0x04;
        }
        if self.letter_spacing.is_some() {
            bits |= 0x08;
        }
        if self.word_spacing.is_some() {
            bits |= 0x10;
        }
        bits
    }

    fn body_len(&self) -> Result<usize, i32> {
        let bits = self.bits();
        // The bitmap is one byte when any optional field is present.
        let mut len = 1 + usize::from(bits != 0);
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        if bits & 0x01 != 0 {
            len += 4;
        }
        if bits & 0x02 != 0 {
            len += 4;
        }
        if bits & 0x04 != 0 {
            len += blob_len(self.family.len(), STYLE_RUN_FAMILY_MAX_BYTES)?;
        }
        if bits & 0x08 != 0 {
            len += 4;
        }
        if bits & 0x10 != 0 {
            len += 4;
        }
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        let bits = self.bits();
        write_bytes(&self.start.to_le_bytes(), out, offset);
        write_bytes(&self.end.to_le_bytes(), out, offset);
        write_bytes(&self.set.to_le_bytes(), out, offset);
        write_bytes(&self.clear.to_le_bytes(), out, offset);
        if let Some(value) = self.color {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.weight {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if bits & 0x04 != 0 {
            write_blob(self.family.as_bytes(), out, offset);
        }
        if let Some(value) = self.letter_spacing {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.word_spacing {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
    }
}
/// Wire tag of the `decor.query` record.
pub const DECOR_QUERY_TAG: u32 = 48;
/// Maximum bytes of `decor.query.text`.
pub const DECOR_QUERY_TEXT_MAX_BYTES: usize = 1024;

/// The `decor.query` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecorQuery<'a> {
    /// Little-endian u32.
    pub section: u32,
    /// Little-endian u32.
    pub start: u32,
    /// Little-endian u32.
    pub end: u32,
    /// UTF-8 text of at most 1024 bytes.
    pub text: &'a str,
    /// Little-endian u32.
    pub chunk: u32,
    /// Little-endian u32.
    pub final_chunk: u32,
}

impl<'a> DecorQuery<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::DecorQuery(Self {
            section: u32::from_le_bytes(body.scalar()?),
            start: u32::from_le_bytes(body.scalar()?),
            end: u32::from_le_bytes(body.scalar()?),
            text: body.string(DECOR_QUERY_TEXT_MAX_BYTES)?,
            chunk: u32::from_le_bytes(body.scalar()?),
            final_chunk: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += blob_len(self.text.len(), DECOR_QUERY_TEXT_MAX_BYTES)?;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.section.to_le_bytes(), out, offset);
        write_bytes(&self.start.to_le_bytes(), out, offset);
        write_bytes(&self.end.to_le_bytes(), out, offset);
        write_blob(self.text.as_bytes(), out, offset);
        write_bytes(&self.chunk.to_le_bytes(), out, offset);
        write_bytes(&self.final_chunk.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `decor.range` record.
pub const DECOR_RANGE_TAG: u32 = 49;
/// Maximum bytes of `decor.range.role`.
pub const DECOR_RANGE_ROLE_MAX_BYTES: usize = 96;

/// The `decor.range` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecorRange<'a> {
    /// Little-endian u32.
    pub start: u32,
    /// Little-endian u32.
    pub end: u32,
    /// UTF-8 text of at most 96 bytes.
    pub role: &'a str,
    /// Little-endian u32.
    pub id: u32,
    /// Little-endian u32.
    pub flags: u32,
}

impl<'a> DecorRange<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::DecorRange(Self {
            start: u32::from_le_bytes(body.scalar()?),
            end: u32::from_le_bytes(body.scalar()?),
            role: body.string(DECOR_RANGE_ROLE_MAX_BYTES)?,
            id: u32::from_le_bytes(body.scalar()?),
            flags: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += blob_len(self.role.len(), DECOR_RANGE_ROLE_MAX_BYTES)?;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.start.to_le_bytes(), out, offset);
        write_bytes(&self.end.to_le_bytes(), out, offset);
        write_blob(self.role.as_bytes(), out, offset);
        write_bytes(&self.id.to_le_bytes(), out, offset);
        write_bytes(&self.flags.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `layer.query` record.
pub const LAYER_QUERY_TAG: u32 = 64;

/// The `layer.query` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerQuery {
    /// Little-endian u32.
    pub section: u32,
    /// Little-endian u32.
    pub offset: u32,
    /// Little-endian u32.
    pub insert: u32,
    /// Little-endian u32.
    pub width: u32,
    /// Little-endian u32.
    pub height: u32,
    /// Little-endian u32.
    pub chunk: u32,
    /// Little-endian u32.
    pub final_chunk: u32,
}

impl LayerQuery {
    fn decode<'a>(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::LayerQuery(Self {
            section: u32::from_le_bytes(body.scalar()?),
            offset: u32::from_le_bytes(body.scalar()?),
            insert: u32::from_le_bytes(body.scalar()?),
            width: u32::from_le_bytes(body.scalar()?),
            height: u32::from_le_bytes(body.scalar()?),
            chunk: u32::from_le_bytes(body.scalar()?),
            final_chunk: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.section.to_le_bytes(), out, offset);
        write_bytes(&self.offset.to_le_bytes(), out, offset);
        write_bytes(&self.insert.to_le_bytes(), out, offset);
        write_bytes(&self.width.to_le_bytes(), out, offset);
        write_bytes(&self.height.to_le_bytes(), out, offset);
        write_bytes(&self.chunk.to_le_bytes(), out, offset);
        write_bytes(&self.final_chunk.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `layer.item` record.
pub const LAYER_ITEM_TAG: u32 = 65;

/// The `layer.item` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerItem {
    /// Little-endian u32.
    pub id: u32,
    /// Little-endian u32.
    pub side: u32,
    /// Little-endian u32.
    pub room: u32,
    /// Little-endian u32.
    pub flags: u32,
}

impl LayerItem {
    fn decode<'a>(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::LayerItem(Self {
            id: u32::from_le_bytes(body.scalar()?),
            side: u32::from_le_bytes(body.scalar()?),
            room: u32::from_le_bytes(body.scalar()?),
            flags: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.id.to_le_bytes(), out, offset);
        write_bytes(&self.side.to_le_bytes(), out, offset);
        write_bytes(&self.room.to_le_bytes(), out, offset);
        write_bytes(&self.flags.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `status.query` record.
pub const STATUS_QUERY_TAG: u32 = 80;
/// Maximum bytes of `status.query.title`.
pub const STATUS_QUERY_TITLE_MAX_BYTES: usize = 512;
/// Maximum bytes of `status.query.chapter`.
pub const STATUS_QUERY_CHAPTER_MAX_BYTES: usize = 256;

/// The `status.query` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusQuery<'a> {
    /// Little-endian u32.
    pub section: u32,
    /// Little-endian u32.
    pub offset: u32,
    /// Little-endian u32.
    pub insert: u32,
    /// Little-endian u32.
    pub progress_ppm: u32,
    /// UTF-8 text of at most 512 bytes.
    pub title: &'a str,
    /// UTF-8 text of at most 256 bytes.
    pub chapter: &'a str,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub page: Option<u32>,
    /// Little-endian u32. Appended and optional; `None` when absent.
    pub pages: Option<u32>,
}

impl<'a> StatusQuery<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        let bits = body.bitmap_bits()?;
        Ok(Record::StatusQuery(Self {
            section: u32::from_le_bytes(body.scalar()?),
            offset: u32::from_le_bytes(body.scalar()?),
            insert: u32::from_le_bytes(body.scalar()?),
            progress_ppm: u32::from_le_bytes(body.scalar()?),
            title: body.string(STATUS_QUERY_TITLE_MAX_BYTES)?,
            chapter: body.string(STATUS_QUERY_CHAPTER_MAX_BYTES)?,
            page: body.maybe_u32(bits, 0x01)?,
            pages: body.maybe_u32(bits, 0x02)?,
        }))
    }

    fn bits(&self) -> u8 {
        let mut bits = 0;
        if self.page.is_some() {
            bits |= 0x01;
        }
        if self.pages.is_some() {
            bits |= 0x02;
        }
        bits
    }

    fn body_len(&self) -> Result<usize, i32> {
        let bits = self.bits();
        // The bitmap is one byte when any optional field is present.
        let mut len = 1 + usize::from(bits != 0);
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += blob_len(self.title.len(), STATUS_QUERY_TITLE_MAX_BYTES)?;
        len += blob_len(self.chapter.len(), STATUS_QUERY_CHAPTER_MAX_BYTES)?;
        if bits & 0x01 != 0 {
            len += 4;
        }
        if bits & 0x02 != 0 {
            len += 4;
        }
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.section.to_le_bytes(), out, offset);
        write_bytes(&self.offset.to_le_bytes(), out, offset);
        write_bytes(&self.insert.to_le_bytes(), out, offset);
        write_bytes(&self.progress_ppm.to_le_bytes(), out, offset);
        write_blob(self.title.as_bytes(), out, offset);
        write_blob(self.chapter.as_bytes(), out, offset);
        if let Some(value) = self.page {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
        if let Some(value) = self.pages {
            write_bytes(&value.to_le_bytes(), out, offset);
        }
    }
}
/// Wire tag of the `status.item` record.
pub const STATUS_ITEM_TAG: u32 = 81;
/// Maximum bytes of `status.item.text`.
pub const STATUS_ITEM_TEXT_MAX_BYTES: usize = 512;

/// The `status.item` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusItem<'a> {
    /// Little-endian u32.
    pub id: u32,
    /// Little-endian u32.
    pub width: u32,
    /// Little-endian u32.
    pub height: u32,
    /// UTF-8 text of at most 512 bytes.
    pub text: &'a str,
    /// Little-endian u32.
    pub flags: u32,
}

impl<'a> StatusItem<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::StatusItem(Self {
            id: u32::from_le_bytes(body.scalar()?),
            width: u32::from_le_bytes(body.scalar()?),
            height: u32::from_le_bytes(body.scalar()?),
            text: body.string(STATUS_ITEM_TEXT_MAX_BYTES)?,
            flags: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += blob_len(self.text.len(), STATUS_ITEM_TEXT_MAX_BYTES)?;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.id.to_le_bytes(), out, offset);
        write_bytes(&self.width.to_le_bytes(), out, offset);
        write_bytes(&self.height.to_le_bytes(), out, offset);
        write_blob(self.text.as_bytes(), out, offset);
        write_bytes(&self.flags.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `draw.item` record.
pub const DRAW_ITEM_TAG: u32 = 96;

/// The `draw.item` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawItem {
    /// Little-endian u32.
    pub section: u32,
    /// Little-endian u32.
    pub offset: u32,
    /// Little-endian u32.
    pub insert: u32,
    /// Little-endian u32.
    pub id: u32,
}

impl DrawItem {
    fn decode<'a>(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::DrawItem(Self {
            section: u32::from_le_bytes(body.scalar()?),
            offset: u32::from_le_bytes(body.scalar()?),
            insert: u32::from_le_bytes(body.scalar()?),
            id: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.section.to_le_bytes(), out, offset);
        write_bytes(&self.offset.to_le_bytes(), out, offset);
        write_bytes(&self.insert.to_le_bytes(), out, offset);
        write_bytes(&self.id.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `input.tap` record.
pub const INPUT_TAP_TAG: u32 = 97;

/// The `input.tap` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputTap {
    /// Little-endian u32.
    pub id: u32,
    /// Little-endian i32.
    pub x: i32,
    /// Little-endian i32.
    pub y: i32,
}

impl InputTap {
    fn decode<'a>(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::InputTap(Self {
            id: u32::from_le_bytes(body.scalar()?),
            x: i32::from_le_bytes(body.scalar()?),
            y: i32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.id.to_le_bytes(), out, offset);
        write_bytes(&self.x.to_le_bytes(), out, offset);
        write_bytes(&self.y.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `page.line` record.
pub const PAGE_LINE_TAG: u32 = 98;

/// The `page.line` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageLine {
    /// Little-endian u32.
    pub index: u32,
    /// Little-endian u32.
    pub start: u32,
    /// Little-endian u32.
    pub end: u32,
    /// Little-endian i32.
    pub x: i32,
    /// Little-endian i32.
    pub y: i32,
    /// Little-endian u32.
    pub width: u32,
    /// Little-endian u32.
    pub height: u32,
}

impl PageLine {
    fn decode<'a>(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::PageLine(Self {
            index: u32::from_le_bytes(body.scalar()?),
            start: u32::from_le_bytes(body.scalar()?),
            end: u32::from_le_bytes(body.scalar()?),
            x: i32::from_le_bytes(body.scalar()?),
            y: i32::from_le_bytes(body.scalar()?),
            width: u32::from_le_bytes(body.scalar()?),
            height: u32::from_le_bytes(body.scalar()?),
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.index.to_le_bytes(), out, offset);
        write_bytes(&self.start.to_le_bytes(), out, offset);
        write_bytes(&self.end.to_le_bytes(), out, offset);
        write_bytes(&self.x.to_le_bytes(), out, offset);
        write_bytes(&self.y.to_le_bytes(), out, offset);
        write_bytes(&self.width.to_le_bytes(), out, offset);
        write_bytes(&self.height.to_le_bytes(), out, offset);
    }
}
/// Wire tag of the `page.word` record.
pub const PAGE_WORD_TAG: u32 = 99;
/// Maximum bytes of `page.word.text`.
pub const PAGE_WORD_TEXT_MAX_BYTES: usize = 256;

/// The `page.word` record, borrowed from its batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageWord<'a> {
    /// Little-endian u32.
    pub index: u32,
    /// Little-endian u32.
    pub start: u32,
    /// Little-endian u32.
    pub end: u32,
    /// Little-endian i32.
    pub x: i32,
    /// Little-endian i32.
    pub y: i32,
    /// Little-endian u32.
    pub width: u32,
    /// Little-endian u32.
    pub height: u32,
    /// UTF-8 text of at most 256 bytes.
    pub text: &'a str,
}

impl<'a> PageWord<'a> {
    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
        body.bitmap()?;
        Ok(Record::PageWord(Self {
            index: u32::from_le_bytes(body.scalar()?),
            start: u32::from_le_bytes(body.scalar()?),
            end: u32::from_le_bytes(body.scalar()?),
            x: i32::from_le_bytes(body.scalar()?),
            y: i32::from_le_bytes(body.scalar()?),
            width: u32::from_le_bytes(body.scalar()?),
            height: u32::from_le_bytes(body.scalar()?),
            text: body.string(PAGE_WORD_TEXT_MAX_BYTES)?,
        }))
    }

    fn body_len(&self) -> Result<usize, i32> {
        let mut len = 1; // The initial bitmap is empty.
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += 4;
        len += blob_len(self.text.len(), PAGE_WORD_TEXT_MAX_BYTES)?;
        Ok(len)
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        write_bytes(&self.index.to_le_bytes(), out, offset);
        write_bytes(&self.start.to_le_bytes(), out, offset);
        write_bytes(&self.end.to_le_bytes(), out, offset);
        write_bytes(&self.x.to_le_bytes(), out, offset);
        write_bytes(&self.y.to_le_bytes(), out, offset);
        write_bytes(&self.width.to_le_bytes(), out, offset);
        write_bytes(&self.height.to_le_bytes(), out, offset);
        write_blob(self.text.as_bytes(), out, offset);
    }
}

/// A registration or declaration record, borrowed from one validated batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Record<'a> {
    /// A `InsertQuery` record.
    InsertQuery(InsertQuery<'a>),
    /// A `InsertItem` record.
    InsertItem(InsertItem),
    /// A `StyleQuery` record.
    StyleQuery(StyleQuery<'a>),
    /// A `StyleBlock` record.
    StyleBlock(StyleBlock),
    /// A `StyleRun` record.
    StyleRun(StyleRun<'a>),
    /// A `DecorQuery` record.
    DecorQuery(DecorQuery<'a>),
    /// A `DecorRange` record.
    DecorRange(DecorRange<'a>),
    /// A `LayerQuery` record.
    LayerQuery(LayerQuery),
    /// A `LayerItem` record.
    LayerItem(LayerItem),
    /// A `StatusQuery` record.
    StatusQuery(StatusQuery<'a>),
    /// A `StatusItem` record.
    StatusItem(StatusItem<'a>),
    /// A `DrawItem` record.
    DrawItem(DrawItem),
    /// A `InputTap` record.
    InputTap(InputTap),
    /// A `PageLine` record.
    PageLine(PageLine),
    /// A `PageWord` record.
    PageWord(PageWord<'a>),
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

    fn bitmap_bits(&mut self) -> Result<u8, i32> {
        let len = self.varint()? as usize;
        // At most eight optional fields: the first bitmap byte is all that is known.
        Ok(self.take(len)?.first().copied().unwrap_or(0))
    }

    fn maybe_i32(&mut self, bits: u8, mask: u8) -> Result<Option<i32>, i32> {
        if bits & mask == 0 {
            return Ok(None);
        }
        Ok(Some(i32::from_le_bytes(self.scalar()?)))
    }

    fn maybe_u32(&mut self, bits: u8, mask: u8) -> Result<Option<u32>, i32> {
        if bits & mask == 0 {
            return Ok(None);
        }
        Ok(Some(u32::from_le_bytes(self.scalar()?)))
    }

    fn opt_blob(&mut self, bits: u8, mask: u8, cap: usize) -> Result<&'a [u8], i32> {
        if bits & mask == 0 {
            return Ok(&[]);
        }
        self.blob(cap)
    }

    fn opt_string(&mut self, bits: u8, mask: u8, cap: usize) -> Result<&'a str, i32> {
        core::str::from_utf8(self.opt_blob(bits, mask, cap)?).map_err(|_| -5)
    }
}

fn record_decode<'a>(tag: u32, body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
    Ok(match tag {
        INSERT_QUERY_TAG => InsertQuery::decode(body)?,
        INSERT_ITEM_TAG => InsertItem::decode(body)?,
        STYLE_QUERY_TAG => StyleQuery::decode(body)?,
        STYLE_BLOCK_TAG => StyleBlock::decode(body)?,
        STYLE_RUN_TAG => StyleRun::decode(body)?,
        DECOR_QUERY_TAG => DecorQuery::decode(body)?,
        DECOR_RANGE_TAG => DecorRange::decode(body)?,
        LAYER_QUERY_TAG => LayerQuery::decode(body)?,
        LAYER_ITEM_TAG => LayerItem::decode(body)?,
        STATUS_QUERY_TAG => StatusQuery::decode(body)?,
        STATUS_ITEM_TAG => StatusItem::decode(body)?,
        DRAW_ITEM_TAG => DrawItem::decode(body)?,
        INPUT_TAP_TAG => InputTap::decode(body)?,
        PAGE_LINE_TAG => PageLine::decode(body)?,
        PAGE_WORD_TAG => PageWord::decode(body)?,
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
            Record::InsertQuery(_) => INSERT_QUERY_TAG,
            Record::InsertItem(_) => INSERT_ITEM_TAG,
            Record::StyleQuery(_) => STYLE_QUERY_TAG,
            Record::StyleBlock(_) => STYLE_BLOCK_TAG,
            Record::StyleRun(_) => STYLE_RUN_TAG,
            Record::DecorQuery(_) => DECOR_QUERY_TAG,
            Record::DecorRange(_) => DECOR_RANGE_TAG,
            Record::LayerQuery(_) => LAYER_QUERY_TAG,
            Record::LayerItem(_) => LAYER_ITEM_TAG,
            Record::StatusQuery(_) => STATUS_QUERY_TAG,
            Record::StatusItem(_) => STATUS_ITEM_TAG,
            Record::DrawItem(_) => DRAW_ITEM_TAG,
            Record::InputTap(_) => INPUT_TAP_TAG,
            Record::PageLine(_) => PAGE_LINE_TAG,
            Record::PageWord(_) => PAGE_WORD_TAG,
        }
    }

    fn body_len(&self) -> Result<usize, i32> {
        match self {
            Record::InsertQuery(record) => record.body_len(),
            Record::InsertItem(record) => record.body_len(),
            Record::StyleQuery(record) => record.body_len(),
            Record::StyleBlock(record) => record.body_len(),
            Record::StyleRun(record) => record.body_len(),
            Record::DecorQuery(record) => record.body_len(),
            Record::DecorRange(record) => record.body_len(),
            Record::LayerQuery(record) => record.body_len(),
            Record::LayerItem(record) => record.body_len(),
            Record::StatusQuery(record) => record.body_len(),
            Record::StatusItem(record) => record.body_len(),
            Record::DrawItem(record) => record.body_len(),
            Record::InputTap(record) => record.body_len(),
            Record::PageLine(record) => record.body_len(),
            Record::PageWord(record) => record.body_len(),
        }
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        match self {
            Record::InsertQuery(record) => record.write(out, offset),
            Record::InsertItem(record) => record.write(out, offset),
            Record::StyleQuery(record) => record.write(out, offset),
            Record::StyleBlock(record) => record.write(out, offset),
            Record::StyleRun(record) => record.write(out, offset),
            Record::DecorQuery(record) => record.write(out, offset),
            Record::DecorRange(record) => record.write(out, offset),
            Record::LayerQuery(record) => record.write(out, offset),
            Record::LayerItem(record) => record.write(out, offset),
            Record::StatusQuery(record) => record.write(out, offset),
            Record::StatusItem(record) => record.write(out, offset),
            Record::DrawItem(record) => record.write(out, offset),
            Record::InputTap(record) => record.write(out, offset),
            Record::PageLine(record) => record.write(out, offset),
            Record::PageWord(record) => record.write(out, offset),
        }
    }

    fn bits(&self) -> u8 {
        match self {
            Record::StyleBlock(record) => record.bits(),
            Record::StyleRun(record) => record.bits(),
            Record::StatusQuery(record) => record.bits(),
            _ => 0,
        }
    }

    fn write_bitmap(&self, out: &mut [u8], offset: &mut usize) {
        let bits = self.bits();
        write_varint(u32::from(bits != 0), out, offset);
        if bits != 0 {
            write_bytes(&[bits], out, offset);
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
        record.write_bitmap(out, &mut offset);
        record.write(out, &mut offset);
    }
    Ok(offset)
}

pub const MAX_BLOCK_INSERTS: usize = 4;
pub const MAX_CHAPTER_INSERTS: u32 = 64;
