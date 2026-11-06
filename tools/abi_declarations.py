"""Generic wire records and their generated borrowed-field codec.

Every record in `abi/abi.def` other than the three initial shapes and the slot-15
command is generated from its declared fields: closed wire types, required
fields first, then optional appended fields (a presence bit and a default).
"""
import re

# Records with their own hand-written codec; every other record is generated.
INITIAL_RECORDS = ('registration.event', 'registration.draw', 'event.tick')
COMMAND_RECORDS = ('command.set_setting', 'command.open_screen', 'command.run_action',
                   'command.redraw', 'command.close_screen')

# Tag allocation (interface-7 contract, shared rule 1). Tags are never reused.
# A name is matched exactly first, then by its first segment; 63 is reserved.
# Tag families, each a tuple of inclusive ranges. The second range of a family
# is its growth block (N3, #241): reserved now so later families never collide.
# Decoders classify records by exact tag only, never by range; the ranges only
# check where a schema author puts a new record.
DECLARATIVE = ((5, 31), (224, 511))
TAG_RANGES = {
    'registration.event': ((1, 3),), 'registration.draw': ((1, 3),), 'event.tick': ((1, 3),),
    'registration': DECLARATIVE,
    'command.set_setting': ((4, 4),),
    # Declarative registration (PR #158, section 8): settings and resources,
    # then actions, menus, screens and icons (Part 5).
    'setting': DECLARATIVE, 'resource': DECLARATIVE, 'action': DECLARATIVE,
    'menu': DECLARATIVE, 'screen': DECLARATIVE, 'icon': DECLARATIVE,
    'format': DECLARATIVE, 'stage': ((512, 1023),),
    'event': ((32, 39), (64, 191)), 'input': ((40, 43), (192, 223)), 'draw': ((192, 223),),
    'i18n': ((44, 47),), 'service': ((48, 55), (1024, 1535)),
    'command': ((56, 62), (1536, 2047)),
}
KEYWORDS = frozenset(
    'as break const continue crate else enum extern false fn for if impl in let loop match '
    'mod move mut pub ref return self Self static struct super trait true type unsafe use '
    'where while async await dyn abstract become box do final macro override priv typeof '
    'unsized virtual yield try gen'.split())
MAX_FIELD_BYTES = 4096
MAX_OPTIONAL = 8
SCALAR_BYTES = {'u32': 4, 'i32': 4, 'u64': 8}
RUST_TYPES = {'str': "&'a str", 'bytes': "&'a [u8]", 'u32': 'u32', 'i32': 'i32',
              'u64': 'u64'}
NAME = re.compile(r'[a-z][a-z0-9]*(_[a-z0-9]+)*')
RECORD_NAME = re.compile(r'[a-z][a-z0-9]*(_[a-z0-9]+)*(\.[a-z][a-z0-9]*(_[a-z0-9]+)*)+')


def _require(ok, message):
    if not ok:
        raise ValueError(message)


def tag_range(name):
    """The inclusive tag range of a record's family, or None for an unknown family."""
    return TAG_RANGES.get(name) or TAG_RANGES.get(name.split('.')[0])


def check_tags(records):
    """Every record's tag lies in its family's range, and no tag is declared twice."""
    seen = {}
    for item in records:
        if type(item) is not dict:
            continue
        name, tag = item.get('name'), item.get('id')
        _require(type(name) is str, 'record name must be text')
        _require(type(tag) is int and 0 <= tag <= 65535,
                 'record tag must be an unsigned declaration ID')
        bounds = tag_range(name)
        _require(bounds is not None,
                 f'record {name} is in no tag family; add its family to TAG_RANGES with a range')
        _require(any(low <= tag <= high for low, high in bounds),
                 f'record {name} has tag {tag} outside its family ranges '
                 + ', '.join(f'{low}-{high}' for low, high in bounds))
        _require(tag not in seen, f'duplicate record codec tag {tag} '
                 f'({seen.get(tag)} and {name})')
        seen[tag] = name


def wire_records(schema):
    """The generated records, in schema order, with checked fields."""
    out = []
    for item in schema.get('records', []):
        if type(item) is dict and item.get('name') not in INITIAL_RECORDS + COMMAND_RECORDS:
            out.append(dict(name=item['name'], id=item['id'], since=item['since'],
                            fields=_fields(item)))
    return out


# The values a scalar field (and so its default) can hold.
SCALAR_RANGES = {'u32': (0, 2**32 - 1), 'u64': (0, 2**64 - 1), 'i32': (-2**31, 2**31 - 1)}


def _fields(record):
    """Required fields first, then optional ones with consecutive ordinals."""
    name, fields = record['name'], record.get('fields')
    _require(type(fields) is list and fields, f'{name}: fields must be a non-empty list')
    out, seen, optional = [], set(), 0
    for field in fields:
        _require(type(field) is dict, f'{name}: field must be an object')
        label, kind, cap = field.get('name'), field.get('type'), field.get('max_bytes')
        _require(type(label) is str and NAME.fullmatch(label) and label not in KEYWORDS,
                 f'{name}: field name is not a plain lowercase Rust identifier')
        _require(label not in seen, f'{name}: duplicate field {label}')
        seen.add(label)
        _require(kind in RUST_TYPES, f'{name}.{label}: wire type outside u32 u64 i32 str bytes')
        _require(type(cap) is int and (0 if kind in SCALAR_BYTES else 1) <= cap <= (
                 0 if kind in SCALAR_BYTES else MAX_FIELD_BYTES),
                 f'{name}.{label}: str/bytes need a max_bytes cap of 1-{MAX_FIELD_BYTES}, '
                 'scalars need 0')
        _require(type(field.get('since')) is int and type(field.get('optional')) is bool,
                 f'{name}.{label}: since and optional are required')
        if field['optional']:
            _require(field.get('ordinal') == optional and type(field['ordinal']) is int,
                     f'{name}.{label}: optional ordinals must be 0, 1, ... in field order')
            optional += 1
            _require(optional <= MAX_OPTIONAL,
                     f'{name}: at most {MAX_OPTIONAL} optional fields')
            default = field.get('default')
            if kind in SCALAR_BYTES:
                # `null`: no default, the field is `Option` (present iff its bit is set),
                # where every value is one (no value may stand for "absent").
                low, high = SCALAR_RANGES[kind]
                _require(default is None or (type(default) is int and low <= default <= high),
                         f'{name}.{label}: default must be null or an integer in the range of {kind}')
            else:
                _require(default == '' and type(default) is str,
                         f'{name}.{label}: str/bytes default must be empty')
        else:
            _require(not optional, f'{name}.{label}: required field after an optional one')
            _require(field.get('ordinal') is None and field.get('default') is None,
                     f'{name}.{label}: required field has no ordinal or default')
            _require(field['since'] == record['since'],
                     f'{name}.{label}: required fields start with their record')
        _require(record['since'] <= field['since'],
                 f'{name}.{label}: field older than its record')
        items = field.get('list')
        if items is not None:
            _require(kind == 'str' and type(items) is dict
                     and set(items) == {'separator', 'max_items', 'item_max_bytes'},
                     f'{name}.{label}: list needs separator, max_items, item_max_bytes on a str')
            _require(type(items['separator']) is str and len(items['separator']) == 1
                     and items['separator'].isascii(),
                     f'{name}.{label}: list separator must be one ASCII character')
            _require(type(items['max_items']) is int and 1 <= items['max_items'] <= 255
                     and type(items['item_max_bytes']) is int
                     and 1 <= items['item_max_bytes'] <= cap,
                     f'{name}.{label}: list bounds outside 1-255 items, 1-cap bytes')
        out.append(dict(name=label, kind=kind, cap=cap, items=items, optional=field['optional'],
                        since=field['since'],
                        ordinal=field['ordinal'], default=field['default'],
                        maybe=field['optional'] and kind in SCALAR_BYTES and field['default'] is None))
    return out


def validate_wire_records(schema, descriptor):
    """Generated records have checked tags, names and fields."""
    declared = wire_records(schema)
    check_tags(schema['records'])
    titles, consts = set(), set()
    for record in declared:
        _require(RECORD_NAME.fullmatch(record['name']) is not None,
                 f'record name {record["name"]!r} is not dotted lowercase')
        _require(type(record['since']) is int and
                 descriptor['since'] <= record['since'] <= schema.get('minor', 0),
                 'record since outside codec/schema minor')
        title, const = _camel(record['name']), _const(record['name'])
        _require(title not in titles and const not in consts and title != 'Registration',
                 f'record {record["name"]} collides with another generated name')
        titles.add(title)
        consts.add(const)
        for field in record['fields']:
            _require(field['since'] <= schema.get('minor', 0),
                     f'{record["name"]}.{field["name"]}: since outside schema minor')
    return declared


def _camel(name):
    return ''.join(part.capitalize() for part in name.replace('_', '.').split('.'))


def _const(name):
    return name.upper().replace('.', '_')


def _call(indent, prefix, args, suffix):
    """A call as rustfmt lays it out: arguments over 60 columns go one per line."""
    if len(args) <= 60:
        return [f'{indent}{prefix}({args}){suffix}']
    return ([f'{indent}{prefix}('] + [f'{indent}    {arg},' for arg in args.split(', ')]
            + [f'{indent}){suffix}'])


def _mask(ordinal):
    return f'0x{1 << ordinal:02x}'


def _decode_value(field, prefix):
    kind, cap = field['kind'], f'{prefix}_{field["name"].upper()}_MAX_BYTES'
    if not field['optional']:
        if kind in SCALAR_BYTES:
            return [f'{kind}::from_le_bytes(body.scalar()?)']
        return _call('', f'body.{"string" if kind == "str" else "blob"}', cap, '?')
    bits = f'bits, {_mask(field["ordinal"])}'
    if field['maybe']:
        return _call('', f'body.maybe_{kind}', bits, '?')
    if kind in SCALAR_BYTES:
        return _call('', f'body.opt_{kind}', f'{bits}, {field["default"]}', '?')
    return _call('', f'body.opt_{"string" if kind == "str" else "blob"}', f'{bits}, {cap}', '?')


def _lifetime(record):
    """`<'a>` when a field borrows from the batch; scalar-only records carry none."""
    return "<'a>" if any(f['kind'] in ('str', 'bytes') for f in record['fields']) else ''


def _record_source(record):
    """One borrowed record struct with decode, length and write steps."""
    name, tag, fields = record['name'], record['id'], record['fields']
    title, prefix = _camel(name), _const(name)
    optional = [f for f in fields if f['optional']]
    out = [f'/// Wire tag of the `{name}` record.', f'pub const {prefix}_TAG: u32 = {tag};']
    for field in fields:
        if field['cap']:
            out += [f'/// Maximum bytes of `{name}.{field["name"]}`.',
                    f'pub const {prefix}_{field["name"].upper()}_MAX_BYTES: usize = '
                    f'{field["cap"]};']
        items = field.get('items')
        if items:
            constant = f'{prefix}_{field["name"].upper()}'
            out += [f'/// Separator of the list in `{name}.{field["name"]}`.',
                    # Python's repr of one ASCII character is a valid Rust char literal.
                    f'pub const {constant}_SEPARATOR: char = {items["separator"]!r};',
                    f'/// Most items in `{name}.{field["name"]}`.',
                    f'pub const {constant}_MAX_ITEMS: usize = {items["max_items"]};',
                    f'/// Longest item of `{name}.{field["name"]}` in bytes.',
                    f'pub const {constant}_ITEM_MAX_BYTES: usize = {items["item_max_bytes"]};']
    out += ['', f'/// The `{name}` record, borrowed from its batch.',
            '#[derive(Clone, Copy, Debug, PartialEq, Eq)]', f"pub struct {title}{_lifetime(record)} {{"]
    for field in fields:
        kind, cap = field['kind'], field['cap']
        text = {'str': f'UTF-8 text of at most {cap} bytes.',
                'bytes': f'Opaque bytes, at most {cap}.'}.get(kind, f'Little-endian {kind}.')
        rust = RUST_TYPES[kind]
        if field['maybe']:
            text += ' Appended and optional; `None` when absent.'
            rust = f'Option<{rust}>'
        elif field['optional']:
            absent = field['default'] if kind in SCALAR_BYTES else 'empty'
            text += f' Appended and optional; {absent} when absent.'
        out += [f'    /// {text}', f'    pub {field["name"]}: {rust},']
    borrows = _lifetime(record)
    out += ['}', '', f"impl<'a> {title}<'a> {{" if borrows else f'impl {title} {{',
            "    fn decode(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {" if borrows
            else "    fn decode<'a>(body: &mut Reader<'a>) -> Result<Record<'a>, i32> {",
            '        let bits = body.bitmap_bits()?;' if optional else '        body.bitmap()?;',
            f'        Ok(Record::{title}(Self {{']
    for field in fields:
        value = _decode_value(field, prefix)
        out.append(f'            {field["name"]}: {value[0]}' + ('' if len(value) > 1 else ','))
        out += [f'            {line}' for line in value[1:]]
        if len(value) > 1:
            out[-1] += ','
    out += ['        }))', '    }', '']
    if optional:
        out += ['    fn bits(&self) -> u8 {', '        let mut bits = 0;']
        for field in optional:
            if field['maybe']:
                test = f'self.{field["name"]}.is_some()'
            elif field['kind'] in SCALAR_BYTES:
                test = f'self.{field["name"]} != {field["default"]}'
            else:
                test = f'!self.{field["name"]}.is_empty()'
            out += [f'        if {test} {{', f'            bits |= {_mask(field["ordinal"])};',
                    '        }']
        out += ['        bits', '    }', '']
    out += ['    fn body_len(&self) -> Result<usize, i32> {']
    if optional:
        out += ['        let bits = self.bits();',
                '        // The bitmap is one byte when any optional field is present.',
                '        let mut len = 1 + usize::from(bits != 0);']
    else:
        out.append('        let mut len = 1; // The initial bitmap is empty.')
    for field in fields:
        if field['optional']:
            continue
        out += _length_lines(field, prefix, '        ')
    for field in optional:
        out.append(f'        if bits & {_mask(field["ordinal"])} != 0 {{')
        out += _length_lines(field, prefix, '            ')
        out.append('        }')
    out += ['        Ok(len)', '    }', '',
            '    fn write(&self, out: &mut [u8], offset: &mut usize) {']
    if any(not f['maybe'] for f in optional):
        out.append('        let bits = self.bits();')
    for field in fields:
        if field['maybe']:
            out.append(f'        if let Some(value) = self.{field["name"]} {{')
            out.append('            write_bytes(&value.to_le_bytes(), out, offset);')
            out.append('        }')
        elif field['optional']:
            out.append(f'        if bits & {_mask(field["ordinal"])} != 0 {{')
            out.append(_write_line(field, '            '))
            out.append('        }')
        else:
            out.append(_write_line(field, '        '))
    out += ['    }', '}', '']
    return '\n'.join(out)


def _length_lines(field, prefix, indent):
    kind, label = field['kind'], field['name']
    if kind in SCALAR_BYTES:
        return [f'{indent}len += {SCALAR_BYTES[kind]};']
    cap = f'{prefix}_{label.upper()}_MAX_BYTES'
    args = f'self.{label}.len(), {cap}'
    # Wrap where rustfmt does (call arguments over 60 columns), so the output
    # stays rustfmt-stable.
    if len(args) <= 60:
        return [f'{indent}len += blob_len({args})?;']
    return [f'{indent}len += blob_len(', f'{indent}    self.{label}.len(),',
            f'{indent}    {cap},', f'{indent})?;']


def _write_line(field, indent):
    kind, label = field['kind'], field['name']
    if kind in SCALAR_BYTES:
        return f'{indent}write_bytes(&self.{label}.to_le_bytes(), out, offset);'
    if kind == 'str':
        return f'{indent}write_blob(self.{label}.as_bytes(), out, offset);'
    return f'{indent}write_blob(self.{label}, out, offset);'


def _reader_extra(declared):
    """Reader methods for optional fields; only the ones some record uses."""
    optional = [f for r in declared for f in r['fields'] if f['optional']]
    if not optional:
        return ''
    kinds = {f['kind'] for f in optional}
    out = ['''
    fn bitmap_bits(&mut self) -> Result<u8, i32> {
        let len = self.varint()? as usize;
        // At most eight optional fields: the first bitmap byte is all that is known.
        Ok(self.take(len)?.first().copied().unwrap_or(0))
    }
''']
    for kind in sorted({f['kind'] for f in optional if f['maybe']}):
        out.append(f'''
    fn maybe_{kind}(&mut self, bits: u8, mask: u8) -> Result<Option<{kind}>, i32> {{
        if bits & mask == 0 {{
            return Ok(None);
        }}
        Ok(Some({kind}::from_le_bytes(self.scalar()?)))
    }}
''')
    for kind in sorted({f['kind'] for f in optional if not f['maybe']} & set(SCALAR_BYTES)):
        out.append(f'''
    fn opt_{kind}(&mut self, bits: u8, mask: u8, default: {kind}) -> Result<{kind}, i32> {{
        if bits & mask == 0 {{
            return Ok(default);
        }}
        Ok({kind}::from_le_bytes(self.scalar()?))
    }}
''')
    if 'bytes' in kinds or 'str' in kinds:
        out.append('''
    fn opt_blob(&mut self, bits: u8, mask: u8, cap: usize) -> Result<&'a [u8], i32> {
        if bits & mask == 0 {
            return Ok(&[]);
        }
        self.blob(cap)
    }
''')
    if 'str' in kinds:
        out.append('''
    fn opt_string(&mut self, bits: u8, mask: u8, cap: usize) -> Result<&'a str, i32> {
        core::str::from_utf8(self.opt_blob(bits, mask, cap)?).map_err(|_| @UTF8@)
    }
''')
    return ''.join(out)


def _record_extra(declared):
    """Record methods that give optional fields their presence bitmap."""
    arms = ''.join(f'            Record::{_camel(r["name"])}(record) => record.bits(),\n'
                   for r in declared if any(f['optional'] for f in r['fields']))
    return '''
    fn bits(&self) -> u8 {
        match self {
''' + arms + '''            _ => 0,
        }
    }

    fn write_bitmap(&self, out: &mut [u8], offset: &mut usize) {
        let bits = self.bits();
        write_varint(u32::from(bits != 0), out, offset);
        if bits != 0 {
            write_bytes(&[bits], out, offset);
        }
    }
''' if arms else ''


def declaration_source(declared, statuses):
    """Mixed-batch API: registrations plus capped, borrowed wire records."""
    arms = lambda text: ''.join(text.replace('@T@', _camel(r['name']))
                                .replace('@L@', _lifetime(r)).replace('@C@', _const(r['name']))
                                + '\n' for r in declared)
    has_optional = any(f['optional'] for r in declared for f in r['fields'])
    source = '\n// Declaration records and the mixed-batch API.\n'
    source += ''.join(_record_source(record) for record in declared)
    source += (DECL_TEMPLATE
               .replace('@READER_EXTRA@', _reader_extra(declared))
               .replace('@RECORD_EXTRA@', _record_extra(declared))
               .replace('@BITMAP@', 'record.write_bitmap(out, &mut offset);' if has_optional
                        else 'write_varint(0, out, &mut offset);')
               .replace('@VARIANTS@', arms("    /// A `@T@` record.\n    @T@(@T@@L@),").rstrip('\n'))
               .replace('@DECODE@', arms('        @C@_TAG => @T@::decode(body)?,').rstrip('\n'))
               .replace('@TAGS@', arms('            Record::@T@(_) => @C@_TAG,').rstrip('\n'))
               .replace('@LENS@', arms('            Record::@T@(record) => record.body_len(),')
                        .rstrip('\n'))
               .replace('@WRITES@', arms('            Record::@T@(record) => record.write(out, offset),')
                        .rstrip('\n')))
    if any(r['name'].startswith('service.') for r in declared):
        source += BARE_TEMPLATE.replace('@BITMAP@', 'record.write_bitmap(out, &mut offset);'
                                        if has_optional else 'write_varint(0, out, &mut offset);')
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
@READER_EXTRA@}

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
/// Every record is decoded once, by [`records_decode`], and kept as decoded:
/// what is staged is exactly what was validated, and iteration cannot fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordBatch<'a> {
    records: [Option<Record<'a>>; REGISTRATION_MAX_RECORDS],
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
    if bytes.len() > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    let len = reader.varint()? as usize;
    if len > REGISTRATION_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    let mut records = [None; REGISTRATION_MAX_RECORDS];
    for place in records.iter_mut().take(len) {
        let (tag, mut body) = reader.record()?;
        *place = Some(record_decode(tag, &mut body)?);
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
@RECORD_EXTRA@}

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
        @BITMAP@
        record.write(out, &mut offset);
    }
    Ok(offset)
}
'''


# Bare-record framing of service blobs (PR #198): emitted only with service
# records, so a schema without them renders exactly as before.
BARE_TEMPLATE = '''
/// Bytes of one bare record: tag, body length and body, with no count.
///
/// The framing of a service blob (`docs/abi/proposals/interface-7-usage-clock.md`):
/// a scalar service is exactly one such record, a table page several of them.
/// A field over its cap, or a record over the batch byte limit, returns
/// ERR_NO_SPACE.
pub fn record_framed_len(record: &Record<'_>) -> Result<usize, i32> {
    let body = record.body_len()?;
    let len = varint_len(record.tag()) + varint_len(body as u32) + body;
    if len > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    Ok(len)
}

/// Encode one bare record (see [`record_framed_len`]) at the front of `out`.
///
/// Every cap and the output capacity are checked before any write. Failure
/// returns ERR_NO_SPACE and leaves the entire output unchanged.
pub fn record_encode_one(record: &Record<'_>, out: &mut [u8]) -> Result<usize, i32> {
    let required = record_framed_len(record)?;
    if out.len() < required {
        return Err(@NO_SPACE@);
    }
    let body = record.body_len()?;
    let mut offset = 0;
    write_varint(record.tag(), out, &mut offset);
    write_varint(body as u32, out, &mut offset);
    @BITMAP@
    record.write(out, &mut offset);
    Ok(offset)
}

/// Decode exactly one bare record (see [`record_framed_len`]): no count, and
/// no byte after it. The caller checks that the tag is the one it expects.
/// Errors are those of [`records_decode`].
pub fn record_decode_one(bytes: &[u8]) -> Result<Record<'_>, i32> {
    if bytes.len() > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    let (tag, mut body) = reader.record()?;
    let record = record_decode(tag, &mut body)?;
    reader.finish()?;
    Ok(record)
}
'''


def rust_streams(schema):
    """Single-record and streaming decoders over the records codec (lock format 8).

    A separate section appended to records_wire.rs, so the records section
    re-renders byte-identical for every earlier lock. Emitted only with service
    records, whose table pages are the one record stream a plugin reads in chunks.
    """
    if not any(r['name'].startswith('service.') for r in wire_records(schema)):
        return ''
    statuses = {item['name']: item['value'] for item in schema['statuses']}
    source = STREAM_TEMPLATE
    for marker, name in [('@INVALID@', 'ERR_INVALID'), ('@NO_SPACE@', 'ERR_NO_SPACE')]:
        source = source.replace(marker, str(statuses[name]))
    return source


STREAM_TEMPLATE = '''
// Single-record and streaming decoders (lock format 8).

/// Bytes a [`RecordStream`] keeps between chunks: every framed table row and
/// the end record fit (`docs/abi/proposals/interface-7-usage-clock.md` 2.4).
pub const RECORD_CARRY_MAX_BYTES: usize = 256;

/// Decode the first bare record of `bytes`; returns it and the frame length
/// it consumed (tag, body length and body).
///
/// Unknown trailing fields inside a known record are skipped. An unknown tag,
/// a truncated or non-canonical frame and an over-cap field are errors, with
/// the statuses of [`records_decode`].
pub fn record_decode_prefix(bytes: &[u8]) -> Result<(Record<'_>, usize), i32> {
    let limit = bytes.len().min(REGISTRATION_MAX_BYTES);
    let mut reader = Reader {
        remaining: bytes.get(..limit).unwrap_or(bytes),
    };
    let (tag, mut body) = reader.record()?;
    let record = record_decode(tag, &mut body)?;
    Ok((record, limit - reader.remaining.len()))
}

/// Decode a batch that holds exactly one record (count 1, then the record, then
/// nothing), without a [`RecordBatch`] on the stack. Event payloads are such
/// batches. Errors are those of [`records_decode`]; any other count is
/// ERR_INVALID.
pub fn records_decode_single(bytes: &[u8]) -> Result<Record<'_>, i32> {
    if bytes.len() > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    if reader.varint()? != 1 {
        return Err(@INVALID@);
    }
    let (tag, mut body) = reader.record()?;
    let record = record_decode(tag, &mut body)?;
    reader.finish()?;
    Ok(record)
}

/// The frame length of the bare record at the front of `bytes`, `None` while
/// its header or body is still incomplete; a non-canonical header is ERR_INVALID.
fn frame_len(bytes: &[u8]) -> Result<Option<usize>, i32> {
    let mut at = 0;
    let mut header = [0_u32; 2];
    for value in &mut header {
        let mut shift = 0;
        loop {
            let Some(&byte) = bytes.get(at) else {
                return Ok(None);
            };
            at += 1;
            if shift == 28 && byte > 0x0f {
                return Err(@INVALID@);
            }
            *value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                if shift > 0 && byte == 0 {
                    return Err(@INVALID@);
                }
                break;
            }
            shift += 7;
        }
    }
    let len = at.saturating_add(header[1] as usize);
    Ok(if len <= bytes.len() { Some(len) } else { None })
}

/// Reads bare records from a byte stream that arrives in chunks of any size,
/// carrying at most [`RECORD_CARRY_MAX_BYTES`] of an incomplete record between
/// chunks. Records are visited in order, borrowed for the visit only.
#[derive(Clone, Debug)]
pub struct RecordStream {
    carry: [u8; RECORD_CARRY_MAX_BYTES],
    len: usize,
}

impl Default for RecordStream {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordStream {
    /// An empty stream.
    pub const fn new() -> Self {
        Self {
            carry: [0; RECORD_CARRY_MAX_BYTES],
            len: 0,
        }
    }

    /// Feed the next chunk and visit every record it completes.
    ///
    /// A record whose frame is longer than [`RECORD_CARRY_MAX_BYTES`] and does
    /// not lie whole inside one chunk is ERR_INVALID. Decode errors are those
    /// of [`record_decode_prefix`]; an error from `visit` stops and is returned.
    pub fn feed(
        &mut self,
        mut chunk: &[u8],
        visit: &mut dyn FnMut(Record<'_>) -> Result<(), i32>,
    ) -> Result<(), i32> {
        if self.len > 0 {
            let old = self.len;
            let moved = chunk.len().min(RECORD_CARRY_MAX_BYTES - old);
            self.carry[old..old + moved].copy_from_slice(&chunk[..moved]);
            match frame_len(&self.carry[..old + moved])? {
                Some(len) => {
                    let (record, _) = record_decode_prefix(&self.carry[..len])?;
                    self.len = 0;
                    visit(record)?;
                    chunk = &chunk[len - old..];
                }
                None if old + moved == RECORD_CARRY_MAX_BYTES => return Err(@INVALID@),
                None => {
                    self.len = old + moved;
                    return Ok(());
                }
            }
        }
        while !chunk.is_empty() {
            match frame_len(chunk)? {
                Some(len) => {
                    let (record, _) = record_decode_prefix(&chunk[..len])?;
                    visit(record)?;
                    chunk = &chunk[len..];
                }
                None if chunk.len() >= RECORD_CARRY_MAX_BYTES => return Err(@INVALID@),
                None => {
                    self.carry[..chunk.len()].copy_from_slice(chunk);
                    self.len = chunk.len();
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// After the last chunk: ERR_INVALID when an incomplete record is left.
    pub fn finish(&self) -> Result<(), i32> {
        if self.len == 0 { Ok(()) } else { Err(@INVALID@) }
    }
}
'''
