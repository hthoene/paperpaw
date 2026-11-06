"""Closed declaration record schemas and their generated borrowed-field codec."""

# Closed schemas: exact ordered fields, (name, wire type, byte cap); the cap is 0
# for scalars. Every field is required. There are no optional fields, no wire
# defaults, and no field is ever appended to a record after release.
DECLS = {
    'setting.group': [('name', 'str', 96), ('title', 'str', 96), ('order', 'i32', 0),
                      ('page', 'str', 96), ('parent', 'str', 128)],
    'setting.spec': [('name', 'str', 96), ('kind', 'u32', 0), ('flags', 'u32', 0),
                     ('group', 'str', 128), ('title', 'str', 96), ('help', 'str', 96),
                     ('default', 'bytes', 512), ('min', 'i32', 0), ('max', 'i32', 0),
                     ('choices', 'str', 1024)],
    'setting.extra': [('name', 'str', 96), ('unit', 'str', 96), ('section', 'str', 96),
                      ('follows', 'str', 128), ('shown_id', 'str', 128),
                      ('shown_value', 'str', 128), ('editor_kind', 'u32', 0),
                      ('editor', 'str', 96), ('step', 'i32', 0), ('flags', 'u32', 0),
                      ('check', 'u32', 0)],
    'resource.hyphenation': [('language', 'str', 35), ('path', 'str', 100)],
    'resource.font_face': [('name', 'str', 96), ('path', 'str', 100),
                           ('family', 'str', 64), ('style', 'u32', 0),
                           ('weight', 'u32', 0), ('role', 'str', 96),
                           ('priority', 'i32', 0)],
    'resource.font_extension': [('name', 'str', 96), ('path', 'str', 100),
                                ('extends', 'str', 128)],
}
SCALAR_BYTES = {'u32': 4, 'i32': 4, 'u64': 8}
RUST_TYPES = {'str': "&'a str", 'bytes': "&'a [u8]", 'u32': 'u32', 'i32': 'i32',
              'u64': 'u64'}


def _camel(name):
    return ''.join(part.capitalize() for part in name.replace('_', '.').split('.'))


def _const(name):
    return name.upper().replace('.', '_')


def _record_source(name, tag):
    """One borrowed record struct with decode, length and write steps."""
    title, prefix = _camel(name), _const(name)
    fields = DECLS[name]
    out = [f'/// Wire tag of the `{name}` record.', f'pub const {prefix}_TAG: u32 = {tag};']
    for field, _kind, cap in fields:
        if cap:
            out += [f'/// Maximum bytes of `{name}.{field}`.',
                    f'pub const {prefix}_{field.upper()}_MAX_BYTES: usize = {cap};']
    out += ['', f'/// The `{name}` record, borrowed from its batch.',
            '#[derive(Clone, Copy, Debug, PartialEq, Eq)]', f"pub struct {title}<'a> {{"]
    for field, kind, cap in fields:
        text = {'str': f'UTF-8 text of at most {cap} bytes.',
                'bytes': f'Opaque bytes, at most {cap}.'}.get(kind, f'Little-endian {kind}.')
        out += [f'    /// {text}', f'    pub {field}: {RUST_TYPES[kind]},']
    out += ['}', '', f"impl<'a> {title}<'a> {{",
            "    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {",
            '        body.bitmap()?;', f'        Ok(Record::{title}(Self {{']
    for field, kind, _cap in fields:
        cap = f'{prefix}_{field.upper()}_MAX_BYTES'
        if kind in SCALAR_BYTES:
            value = f'{kind}::from_le_bytes(body.scalar()?)'
        else:
            value = f'body.{"string" if kind == "str" else "blob"}({cap})?'
        out.append(f'            {field}: {value},')
    out += ['        }))', '    }', '', '    fn body_len(&self) -> Result<usize, i32> {',
            '        let mut len = 1; // The initial bitmap is empty.']
    for field, kind, _cap in fields:
        if kind in SCALAR_BYTES:
            out.append(f'        len += {SCALAR_BYTES[kind]};')
        else:
            cap = f'{prefix}_{field.upper()}_MAX_BYTES'
            args = f'self.{field}.len(), {cap}'
            line = f'        len += blob_len({args})?;'
            # Wrap where rustfmt does (call arguments over 60 columns), so the
            # output stays rustfmt-stable.
            out += [line] if len(args) <= 60 else [
                '        len += blob_len(', f'            self.{field}.len(),',
                f'            {cap},', '        )?;']
    out += ['        Ok(len)', '    }', '',
            '    fn write(&self, out: &mut [u8], offset: &mut usize) {']
    for field, kind, _cap in fields:
        if kind in SCALAR_BYTES:
            out.append(f'        write_bytes(&self.{field}.to_le_bytes(), out, offset);')
        elif kind == 'str':
            out.append(f'        write_blob(self.{field}.as_bytes(), out, offset);')
        else:
            out.append(f'        write_blob(self.{field}, out, offset);')
    out += ['    }', '}', '']
    return '\n'.join(out)


def declaration_source(records, statuses):
    """Mixed-batch API: registrations plus capped, borrowed declaration records."""
    names = list(DECLS)
    arms = lambda text: ''.join(text.replace('@T@', _camel(n)).replace('@C@', _const(n)) + '\n'
                                for n in names)
    source = '\n// Declaration records and the mixed-batch API.\n'
    source += ''.join(_record_source(name, records[name]['id']) for name in names)
    source += (DECL_TEMPLATE
               .replace('@VARIANTS@', arms("    /// A `@T@` record.\n    @T@(@T@<'a>),").rstrip('\n'))
               .replace('@DECODE@', arms('        @C@_TAG => @T@::decode(body)?,').rstrip('\n'))
               .replace('@TAGS@', arms('            Record::@T@(_) => @C@_TAG,').rstrip('\n'))
               .replace('@LENS@', arms('            Record::@T@(record) => record.body_len(),')
                        .rstrip('\n'))
               .replace('@WRITES@', arms('            Record::@T@(record) => record.write(out, offset),')
                        .rstrip('\n')))
    for marker, name in [('@INVALID@', 'ERR_INVALID'), ('@UNSUPPORTED@', 'ERR_UNSUPPORTED'),
                         ('@NO_SPACE@', 'ERR_NO_SPACE'), ('@UTF8@', 'ERR_UTF8')]:
        source = source.replace(marker, str(statuses[name]))
    return source


DECL_TEMPLATE = '''
/// A registration or declaration record, borrowed from one validated batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Record<'a> {
    /// A callback registration.
    Registration(Registration),
@VARIANTS@
}

impl<'a> Reader<'a> {
    fn blob(&mut self, cap: usize) -> Result<&'a [u8], i32> {
        let len = self.varint()? as usize;
        if len > cap {
            return Err(@NO_SPACE@);
        }
        self.take(len)
    }

    fn string(&mut self, cap: usize) -> Result<&'a str, i32> {
        core::str::from_utf8(self.blob(cap)?).map_err(|_| @UTF8@)
    }
}

fn record_decode<'a>(tag: u32, body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
    Ok(match tag {
        REGISTRATION_EVENT_TAG => {
            body.bitmap()?;
            Record::Registration(Registration::Event {
                event_tag: u32::from_le_bytes(body.scalar()?),
                token: u32::from_le_bytes(body.scalar()?),
            })
        }
        REGISTRATION_DRAW_TAG => {
            body.bitmap()?;
            Record::Registration(Registration::Draw {
                token: u32::from_le_bytes(body.scalar()?),
            })
        }
@DECODE@
        _ => return Err(@UNSUPPORTED@),
    })
}

/// A fully validated, allocation-free batch of registrations and declarations.
///
/// It keeps the borrowed, already validated input and decodes each record again
/// while iterating, so it holds no array of records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordBatch<'a> {
    records: &'a [u8],
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
    pub fn iter(&self) -> RecordIter<'a> {
        RecordIter {
            remaining: self.records,
            left: self.len,
        }
    }
}

/// Iterator over a validated [`RecordBatch`].
#[derive(Clone, Copy, Debug)]
pub struct RecordIter<'a> {
    remaining: &'a [u8],
    left: usize,
}

impl<'a> Iterator for RecordIter<'a> {
    type Item = Record<'a>;

    fn next(&mut self) -> Option<Record<'a>> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let mut reader = Reader {
            remaining: self.remaining,
        };
        let (tag, mut body) = reader.record().ok()?;
        self.remaining = reader.remaining;
        record_decode(tag, &mut body).ok()
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
    if bytes.len() > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    let len = reader.varint()? as usize;
    if len > REGISTRATION_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    let records = reader.remaining;
    for _ in 0..len {
        let (tag, mut body) = reader.record()?;
        record_decode(tag, &mut body)?;
    }
    reader.finish()?;
    Ok(RecordBatch { records, len })
}

fn blob_len(len: usize, cap: usize) -> Result<usize, i32> {
    if len > cap {
        Err(@NO_SPACE@)
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
            Record::Registration(Registration::Event { .. }) => REGISTRATION_EVENT_TAG,
            Record::Registration(Registration::Draw { .. }) => REGISTRATION_DRAW_TAG,
@TAGS@
        }
    }

    fn body_len(&self) -> Result<usize, i32> {
        match self {
            Record::Registration(Registration::Event { .. }) => Ok(9),
            Record::Registration(Registration::Draw { .. }) => Ok(5),
@LENS@
        }
    }

    fn write(&self, out: &mut [u8], offset: &mut usize) {
        match self {
            Record::Registration(Registration::Event { event_tag, token }) => {
                write_bytes(&event_tag.to_le_bytes(), out, offset);
                write_bytes(&token.to_le_bytes(), out, offset);
            }
            Record::Registration(Registration::Draw { token }) => {
                write_bytes(&token.to_le_bytes(), out, offset);
            }
@WRITES@
        }
    }
}

/// Encode a canonical mixed batch without allocating.
///
/// Every field cap, the byte and count limits and the complete output capacity
/// are checked before any write. Failure returns ERR_NO_SPACE and leaves the
/// entire output unchanged.
pub fn records_encode(records: &[Record<'_>], out: &mut [u8]) -> Result<usize, i32> {
    if records.len() > REGISTRATION_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    let mut required = varint_len(records.len() as u32);
    for record in records {
        let body = record.body_len()?;
        required += varint_len(record.tag()) + varint_len(body as u32) + body;
        if required > REGISTRATION_MAX_BYTES {
            return Err(@NO_SPACE@);
        }
    }
    if out.len() < required {
        return Err(@NO_SPACE@);
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
'''
