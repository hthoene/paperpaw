"""Validate and emit the initial bounded registration and tick record codecs."""
from abi_declarations import (check_tags, declaration_source, validate_wire_records,
                              wire_records)


def _require(ok, message):
    if not ok:
        raise ValueError(message)


# This describes supported facade shapes, never their wire tags.
SHAPES = {
    'registration.event': [('event_tag', 'u32'), ('token', 'u32')],
    'registration.draw': [('token', 'u32')],
    'event.tick': [('now_ms', 'u64')],
}


def validate_records(schema):
    """Older schemas have no codec; recognized records have exact initial shapes."""
    records = schema.get('records', [])
    _require(type(records) is list, 'records must be a list')
    recognized = [item for item in records
                  if type(item) is dict and item.get('name') in SHAPES]
    if not recognized and 'record_codec' not in schema:
        return
    descriptor = schema.get('record_codec')
    _require(type(descriptor) is dict and set(descriptor) == {'since', 'max_records'},
             'record codec descriptor keys')
    _require(type(descriptor['since']) is int and
             1 <= descriptor['since'] <= schema.get('minor', 0),
             'record codec since outside schema minor')
    _require(type(descriptor['max_records']) is int and
             1 <= descriptor['max_records'] <= 8, 'record codec count cap')
    _require(len(recognized) == len(SHAPES) and
             {item['name'] for item in recognized} == set(SHAPES),
             'initial records must include every supported shape')
    _require(len({record['id'] for record in recognized}) == len(recognized),
             'duplicate record codec tag')
    for record in recognized:
        _require(type(record.get('id')) is int and 0 <= record['id'] <= 65535,
                 'record tag must be an unsigned declaration ID')
        _require(type(record.get('since')) is int and
                 descriptor['since'] <= record['since'] <= schema.get('minor', 0),
                 'record since outside codec/schema minor')
        fields = record.get('fields')
        expected = [dict(name=name, type=kind, since=record['since'], optional=False,
                         ordinal=None, default=None, max_bytes=0)
                    for name, kind in SHAPES[record['name']]]
        _require(fields == expected and all(type(field['since']) is int and
                 type(field['max_bytes']) is int for field in fields),
                 'unsupported initial record fields')
    check_tags(records)
    declared = validate_wire_records(schema, descriptor)
    statuses = schema.get('statuses', [])
    needed = [('ERR_UNSUPPORTED', -1), ('ERR_INVALID', -2), ('ERR_NO_SPACE', -3)]
    if declared:
        needed.append(('ERR_UTF8', -5))
    for name, value in needed:
        matches = [item for item in statuses if item.get('name') == name]
        _require(len(matches) == 1 and type(matches[0].get('value')) is int and
                 matches[0]['value'] == value and
                 type(matches[0].get('since')) is int and
                 1 <= matches[0]['since'] <= descriptor['since'],
                 'record codec status contract')
    entries = [item for item in schema.get('host_entries', [])
               if item.get('name') == 'registrations.submit']
    _require(len(entries) == 1 and type(entries[0].get('max_bytes')) is int and
             entries[0]['max_bytes'] == 4096, 'registration codec byte cap')


def compatible_records(old, new, minimum_since):
    """The emitted codec resource descriptor is an immutable wire contract."""
    validate_records(old)
    validate_records(new)
    before, after = old.get('record_codec'), new.get('record_codec')
    if before is None:
        if after is not None:
            _require(after['since'] >= minimum_since, 'backdated record codec')
        return
    _require(after == before, 'changed or removed record codec contract')


def rust_records(schema):
    """Emit a standalone no_std module, keeping existing raw modules unchanged."""
    validate_records(schema)
    records = {item['name']: item for item in schema.get('records', [])}
    if not set(SHAPES) <= records.keys():
        return ''
    statuses = {item['name']: item['value'] for item in schema['statuses']}
    replacements = {
        '@COUNT@': schema['record_codec']['max_records'],
        '@EVENT@': records['registration.event']['id'],
        '@DRAW@': records['registration.draw']['id'],
        '@TICK@': records['event.tick']['id'],
        '@MAX@': next(item['max_bytes'] for item in schema['host_entries']
                     if item['name'] == 'registrations.submit'),
        '@INVALID@': statuses['ERR_INVALID'],
        '@UNSUPPORTED@': statuses['ERR_UNSUPPORTED'],
        '@NO_SPACE@': statuses['ERR_NO_SPACE'],
    }
    return record_codec_source(wire_records(schema),
                               schema['record_codec']['max_records'],
                               replacements['@MAX@'], statuses=statuses,
                               kernel_replacements=replacements)


def _without_registration(source):
    """Remove only the global registration alternatives from the shared template."""
    source = source.replace("    /// A callback registration.\n    Registration(Registration),\n", "")
    # Remove complete match arms, preserving all shared framing/field code.
    lines = source.splitlines(keepends=True)
    out = []
    index = 0
    while index < len(lines):
        line = lines[index]
        if ("REGISTRATION_EVENT_TAG => {" in line
                or "REGISTRATION_DRAW_TAG => {" in line
                or "Record::Registration(" in line):
            depth = line.count("{") - line.count("}")
            index += 1
            while depth:
                if index >= len(lines):
                    raise ValueError("unterminated registration template arm")
                depth += lines[index].count("{") - lines[index].count("}")
                index += 1
            continue
        out.append(line)
        index += 1
    result = "".join(out)
    if "Registration" in result or "REGISTRATION_EVENT_TAG" in result:
        raise ValueError("global registration leaked into local codec")
    return result



def record_codec_source(records, max_records, max_bytes, *, statuses,
                        kernel_replacements=None):
    """Shared canonical framing; only the kernel path has initial registrations."""
    if kernel_replacements is not None:
        source = RECORD_TEMPLATE
        for marker, value in kernel_replacements.items():
            source = source.replace(marker, str(value))
        if records:
            source += declaration_source(records, statuses)
        return source
    # Extract unchanged canonical primitives. Marker changes fail generation.
    reader = RECORD_TEMPLATE.split("struct Reader<'a> {", 1)[1].split(
        "/// Decode an entire batch before exposing any registration.", 1)[0]
    writes = RECORD_TEMPLATE.split("fn varint_len(", 1)[1].split(
        "/// Encode a canonical batch without allocating.", 1)[0]
    source = ("// Generated stage codec; do not edit.\n"
              f"pub const REGISTRATION_MAX_BYTES: usize = {max_bytes};\n"
              f"pub const REGISTRATION_MAX_RECORDS: usize = {max_records};\n"
              + "struct Reader<'a> {" + reader + "fn varint_len(" + writes)
    source += _without_registration(declaration_source(records, statuses))
    if not any(field['kind'] in ('str', 'bytes')
               for record in records for field in record['fields']):
        for name in ('Record', 'RecordBatch'):
            for lifetime in ("'a", "'_"):
                source = source.replace(f"{name}<{lifetime}>", name)
        source = source.replace("RecordIter<'b, 'a>", "RecordIter<'b>")
        source = source.replace("RecordIter<'_, 'a>", "RecordIter<'_>")
    return source.replace("@INVALID@", str(statuses['ERR_INVALID']))


# Shared canonical framing template. The global generator output is unchanged.
RECORD_TEMPLATE = '''// Generated by tools/abi-gen.py; do not edit.
/// Maximum encoded registration batch or tick event length.
pub const REGISTRATION_MAX_BYTES: usize = @MAX@;
/// Maximum registrations in one batch.
pub const REGISTRATION_MAX_RECORDS: usize = @COUNT@;
/// Wire tag of the tick event.
pub const EVENT_TICK_TAG: u32 = @TICK@;
const REGISTRATION_EVENT_TAG: u32 = @EVENT@;
const REGISTRATION_DRAW_TAG: u32 = @DRAW@;

/// A declarative callback registration. Callback state stays with its producer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Registration {
    /// Dispatch an event to the given callback token.
    Event {
        /// Event record tag to receive.
        event_tag: u32,
        /// Producer-owned callback identity.
        token: u32,
    },
    /// Invoke the given draw callback token.
    Draw {
        /// Producer-owned callback identity.
        token: u32,
    },
}

/// A fully validated, allocation-free registration batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistrationBatch {
    records: [Option<Registration>; REGISTRATION_MAX_RECORDS],
    len: usize,
}

impl RegistrationBatch {
    /// Number of validated registrations.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether this batch has no registrations.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate over registrations by value in wire order.
    pub fn iter(&self) -> impl Iterator<Item = Registration> + '_ {
        self.records[..self.len].iter().flatten().copied()
    }
}

struct Reader<'a> {
    remaining: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], i32> {
        if len > self.remaining.len() {
            return Err(@INVALID@);
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
                return Err(@INVALID@);
            }
            value |= u32::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                if index > 0 && byte == 0 {
                    return Err(@INVALID@);
                }
                return Ok(value);
            }
        }
        Err(@INVALID@)
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
            Err(@INVALID@)
        }
    }
}

/// Decode an entire batch before exposing any registration.
///
/// Framing must be canonical u32 LEB128. Unknown tags return ERR_UNSUPPORTED;
/// malformed records return ERR_INVALID; byte/count limits return ERR_NO_SPACE.
/// Unknown trailing fields and extended presence bitmaps are accepted.
pub fn registrations_decode(bytes: &[u8]) -> Result<RegistrationBatch, i32> {
    if bytes.len() > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    let len = reader.varint()? as usize;
    if len > REGISTRATION_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    let mut batch = RegistrationBatch {
        records: [None; REGISTRATION_MAX_RECORDS],
        len,
    };
    for slot in &mut batch.records[..len] {
        let (tag, mut body) = reader.record()?;
        if tag != REGISTRATION_EVENT_TAG && tag != REGISTRATION_DRAW_TAG {
            return Err(@UNSUPPORTED@);
        }
        body.bitmap()?;
        *slot = Some(match tag {
            REGISTRATION_EVENT_TAG => Registration::Event {
                event_tag: u32::from_le_bytes(body.scalar()?),
                token: u32::from_le_bytes(body.scalar()?),
            },
            _ => Registration::Draw {
                token: u32::from_le_bytes(body.scalar()?),
            },
        });
    }
    reader.finish()?;
    Ok(batch)
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

/// Encode a canonical batch without allocating.
///
/// Every bound and the complete output capacity are checked before any write.
/// Failure returns ERR_NO_SPACE and leaves the entire output unchanged.
pub fn registrations_encode(records: &[Registration], out: &mut [u8]) -> Result<usize, i32> {
    if records.len() > REGISTRATION_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    let required = varint_len(records.len() as u32)
        + records
            .iter()
            .map(|record| match record {
                Registration::Event { .. } => record_len(REGISTRATION_EVENT_TAG, 8),
                Registration::Draw { .. } => record_len(REGISTRATION_DRAW_TAG, 4),
            })
            .sum::<usize>();
    if required > REGISTRATION_MAX_BYTES || out.len() < required {
        return Err(@NO_SPACE@);
    }
    let mut offset = 0;
    write_varint(records.len() as u32, out, &mut offset);
    for record in records {
        match *record {
            Registration::Event { event_tag, token } => {
                write_header(REGISTRATION_EVENT_TAG, 8, out, &mut offset);
                write_bytes(&event_tag.to_le_bytes(), out, &mut offset);
                write_bytes(&token.to_le_bytes(), out, &mut offset);
            }
            Registration::Draw { token } => {
                write_header(REGISTRATION_DRAW_TAG, 4, out, &mut offset);
                write_bytes(&token.to_le_bytes(), out, &mut offset);
            }
        }
    }
    Ok(offset)
}

/// Encode one tick event as a one-record batch, leaving output unchanged on error.
pub fn tick_encode(now_ms: u64, out: &mut [u8]) -> Result<usize, i32> {
    let required = 1 + record_len(EVENT_TICK_TAG, 8);
    if out.len() < required {
        return Err(@NO_SPACE@);
    }
    let mut offset = 0;
    write_varint(1, out, &mut offset);
    write_header(EVENT_TICK_TAG, 8, out, &mut offset);
    write_bytes(&now_ms.to_le_bytes(), out, &mut offset);
    Ok(offset)
}

/// Decode one tick event with bounded framing and little-endian milliseconds.
///
/// Exactly one known event is required. Extended bitmaps and trailing fields
/// within its record are accepted; bytes outside the batch are malformed.
pub fn tick_decode(bytes: &[u8]) -> Result<u64, i32> {
    if bytes.len() > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    if reader.varint()? != 1 {
        return Err(@INVALID@);
    }
    let (tag, mut body) = reader.record()?;
    if tag != EVENT_TICK_TAG {
        return Err(@UNSUPPORTED@);
    }
    body.bitmap()?;
    let now_ms = u64::from_le_bytes(body.scalar()?);
    reader.finish()?;
    Ok(now_ms)
}
'''
