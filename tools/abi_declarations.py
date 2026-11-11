"""Generic wire records and their generated borrowed-field codec.

Every record in `abi/abi.def` other than the three initial shapes and the slot-15
command is generated from its declared fields: closed wire types, required
fields first, then optional appended fields (a presence bit and a default).
"""
import re

# Records with their own hand-written codec; every other record is generated.
INITIAL_RECORDS = ('registration.event', 'registration.draw', 'event.tick')
COMMAND_RECORDS = ('command.set_setting', 'command.open_screen', 'command.run_action',
                   'command.redraw', 'command.close_screen', 'command.set_item_state')

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
    'format': DECLARATIVE, 'stage.spec': DECLARATIVE, 'stage': ((512, 1023),),
    # Registry declarations (REG, docs/abi/registry.md section 6).
    'setup': DECLARATIVE, 'slot': DECLARATIVE, 'key': DECLARATIVE, 'preset': DECLARATIVE,
    # UIX declarations (docs/abi/ui.md sections 2, 5, 7); `input.spec` by its full name,
    # beside the `input` family of host-sent inputs.
    'input.spec': DECLARATIVE, 'help': DECLARATIVE, 'widget': DECLARATIVE,
    'status': DECLARATIVE,
    # DOC and LIB declarations (docs/abi/document.md section 1 and 7, library.md section 1).
    'reader': DECLARATIVE, 'lookup': DECLARATIVE, 'book_field': DECLARATIVE,
    'book_badge': DECLARATIVE, 'book_data': DECLARATIVE,
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


# Commands of the growth block (REG, #228) are generated from their fields into the
# slot-15 command codec only (abi_commands.py): commands never travel in a record batch.
GROWTH_COMMANDS = (1536, 2047)


def is_growth_command(item):
    """A `command.*` record in the slot-15 growth block."""
    return (type(item) is dict and str(item.get('name', '')).startswith('command.')
            and type(item.get('id')) is int
            and GROWTH_COMMANDS[0] <= item['id'] <= GROWTH_COMMANDS[1])


def growth_fields(record):
    """The checked fields of a growth-block command, as for a generated record; a
    command that asks for one thing only (`command.show_hints`) has none."""
    if record.get('fields') == []:
        return []
    return _fields(record)


def wire_records(schema):
    """The generated records, in schema order, with checked fields."""
    out = []
    for item in schema.get('records', []):
        if (type(item) is dict and item.get('name') not in INITIAL_RECORDS + COMMAND_RECORDS
                and not is_growth_command(item)):
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
        secret = 'secret' in field
        _require(not secret or (field['secret'] is True and items is None
                                and (kind == 'str' and is_growth_command(record)
                                     or kind == 'str' and name.startswith('event.')
                                     or kind in ('str', 'bytes') and name.startswith('service.'))),
                 f'{name}.{label}: secret must be true, on a str field of a command or an '
                 'event, or a str or bytes field of a service record, not a list')
        out.append(dict(name=label, kind=kind, cap=cap, items=items, optional=field['optional'],
                        secret=secret,
                        since=field['since'],
                        ordinal=field['ordinal'], default=field['default'],
                        maybe=field['optional'] and kind in SCALAR_BYTES and field['default'] is None))
    return out


def validate_wire_records(schema, descriptor):
    """Generated records have checked tags, names and fields."""
    declared = wire_records(schema)
    check_tags(schema['records'])
    validate_record_shapes(declared, descriptor['since'], schema.get('minor', 0))
    return declared


def validate_record_shapes(declared, minimum_since, maximum_since):
    """Shared generated-name and revision checks for kernel and local tags."""
    titles, consts = set(), set()
    for record in declared:
        _require(RECORD_NAME.fullmatch(record['name']) is not None,
                 f'record name {record["name"]!r} is not dotted lowercase')
        _require(type(record['since']) is int and
                 minimum_since <= record['since'] <= maximum_since,
                 'record since outside codec/schema minor')
        title, const = _camel(record['name']), _const(record['name'])
        _require(title not in titles and const not in consts and title != 'Registration',
                 f'record {record["name"]} collides with another generated name')
        titles.add(title)
        consts.add(const)
        for field in record['fields']:
            _require(field['since'] <= maximum_since,
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


def _record_source(record, decode_name="decode", record_type="Record<'a>", extensible=False,
                   typed=False):
    """One borrowed record struct with decode, length and write steps; `typed` also gives it
    the typed codec ([`WireEncode`], [`WireRecord`]) a plugin links per record type."""
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
    # A record with a secret field (`"secret": true`) gets its own `Debug`, which
    # shows neither that text nor its length; every other record derives it.
    hidden = [f['name'] for f in fields if f.get('secret')]
    derive = '#[derive(Clone, Copy, PartialEq, Eq)]' if hidden else \
        '#[derive(Clone, Copy, Debug, PartialEq, Eq)]'
    out += ['', f'/// The `{name}` record, borrowed from its batch.',
            derive, f"pub struct {title}{_lifetime(record)} {{"]
    if extensible:
        out.insert(len(out) - 1, '#[non_exhaustive]')
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
    if hidden:
        life = "<'_>" if borrows else ''
        out += ['}', '', f'impl core::fmt::Debug for {title}{life} {{',
                "    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {",
                f'        f.debug_struct("{title}")']
        for field in fields:
            shown = '&".."' if field['name'] in hidden else f'&self.{field["name"]}'
            out.append(f'            .field("{field["name"]}", {shown})')
        out += ['            .finish()', '    }']
    wrapped = record_type
    if typed:
        # The fields decode into the struct alone; the enum wraps it, so a plugin decoding
        # one record type links no other record's decoder.
        out += ['}', '', f"impl<'a> {title}<'a> {{" if borrows else f'impl {title} {{',
                f"    fn {decode_name}(body: &mut Reader<'a>) -> Result<{record_type}, i32> {{"
                if borrows else
                f"    fn {decode_name}<'a>(body: &mut Reader<'a>) -> Result<{record_type}, i32> {{",
                f'        Self::decode_fields(body).map(Record::{title})', '    }', '']
        decode_name, record_type = 'decode_fields', 'Self'
        out += [f"    fn {decode_name}(body: &mut Reader<'a>) -> Result<Self, i32> {{" if borrows
                else f"    fn {decode_name}(body: &mut Reader<'_>) -> Result<Self, i32> {{"]
    else:
        out += ['}', '', f"impl<'a> {title}<'a> {{" if borrows else f'impl {title} {{',
                f"    fn {decode_name}(body: &mut Reader<'a>) -> Result<{record_type}, i32> {{"
                if borrows else
                f"    fn {decode_name}<'a>(body: &mut Reader<'a>) -> Result<{record_type}, i32> {{"]
    out += ['        let bits = body.bitmap_bits()?;' if optional else '        body.bitmap()?;',
            '        Ok(Self {' if typed else f'        Ok(Record::{title}(Self {{']
    for field in fields:
        value = _decode_value(field, prefix)
        out.append(f'            {field["name"]}: {value[0]}' + ('' if len(value) > 1 else ','))
        out += [f'            {line}' for line in value[1:]]
        if len(value) > 1:
            out[-1] += ','
    out += ['        })' if typed else '        }))', '    }', '']
    anonymous = "<'_>" if borrows else ''
    # The steps go through one walk of the fields (`Fields`, FIELDS_TEMPLATE): one function
    # per record type instead of a length, a bitmap and a write step each.
    if optional:
        out += ['    fn bits(&self) -> u8 {', '        fields_bits(self)', '    }', '']
    out += ['    fn body_len(&self) -> Result<usize, i32> {', '        fields_len(self)', '    }', '',
            '    fn write(&self, out: &mut [u8], offset: &mut usize) {',
            '        fields_write(self, out, offset);', '    }', '}', '',
            f'impl Fields for {title}{anonymous} {{',
            '    fn fields(&self, sink: &mut dyn Sink) {' if fields else
            '    fn fields(&self, _: &mut dyn Sink) {']
    for field in fields:
        out += _field_lines(field, prefix)
    out += ['    }', '}', '']
    if typed:
        life = "<'a>" if borrows else ''
        out += [f"impl{life} WireEncode for {title}{life} {{",
                '    fn wire_tag(&self) -> u32 {', f'        {prefix}_TAG', '    }', '',
                '    fn wire_body_len(&self) -> Result<usize, i32> {', '        fields_len(self)',
                '    }', '',
                '    fn wire_write(&self, out: &mut [u8], offset: &mut usize, _: seal::Seal) {',
                '        fields_write_framed(self, out, offset);', '    }', '}', '',
                f"impl<'a> WireRecord<'a> for {title}{life} {{",
                f'    const TAG: u32 = {prefix}_TAG;', '',
                "    fn decode_body(body: &'a [u8]) -> Result<Self, i32> {",
                '        Self::decode_fields(&mut Reader { remaining: body })',
                '    }', '}', '']
    if extensible:
        params = ', '.join(f"{f['name']}: " + (f"Option<{RUST_TYPES[f['kind']]}>"
                           if f['maybe'] else RUST_TYPES[f['kind']]) for f in fields)
        values = ', '.join(f['name'] for f in fields)
        out += [f"impl<'a> {title}<'a> {{" if borrows else f'impl {title} {{',
                '    /// Build a record; encoding validates its bounds.',
                f'    pub const fn new({params}) -> Self {{',
                f'        Self {{ {values} }}', '    }', '}', '']
    return '\n'.join(out)


def _field_lines(field, prefix):
    """One field of a record's walk (`Fields::fields`): the sink call, under its presence test
    when the field is optional (absent fields are not walked)."""
    kind, label = field['kind'], field['name']
    mask = _mask(field['ordinal']) if field['optional'] else '0'
    if kind in SCALAR_BYTES:
        value = 'value' if field['maybe'] else f'self.{label}'
        call, args = 'sink.scalar', f'&{value}.to_le_bytes(), {mask}'
    else:
        value = f'self.{label}.as_bytes()' if kind == 'str' else f'self.{label}'
        call, args = 'sink.blob', f'{value}, {prefix}_{label.upper()}_MAX_BYTES, {mask}'
    if not field['optional']:
        return _call('        ', call, args, ';')
    if field['maybe']:
        test = f'if let Some(value) = self.{label} {{'
    elif kind in SCALAR_BYTES:
        test = f'if self.{label} != {field["default"]} {{'
    else:
        test = f'if !self.{label}.is_empty() {{'
    return [f'        {test}'] + _call('            ', call, args, ';') + ['        }']


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
    source += ''.join(_record_source(record, typed=True) for record in declared)
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
    source += FIELDS_TEMPLATE + TYPED_TEMPLATE
    source += _known_tags_source(declared)
    if any(r['name'].startswith('service.') for r in declared):
        source += (BARE_TEMPLATE
                   .replace('@BITMAP@', 'record.write_bitmap(out, &mut offset);'
                            if has_optional else 'write_varint(0, out, &mut offset);')
                   .replace('@SELF_BITMAP@', 'self.write_bitmap(out, offset);'
                            if has_optional else 'write_varint(0, out, offset);'))
    for marker, name in [('@INVALID@', 'ERR_INVALID'), ('@UNSUPPORTED@', 'ERR_UNSUPPORTED'),
                         ('@NO_SPACE@', 'ERR_NO_SPACE'), ('@UTF8@', 'ERR_UTF8')]:
        source = source.replace(marker, str(statuses[name]))
    return source


def _known_tags_source(declared):
    """`record_tag_known`: the tags `record_decode` knows, as a sorted table."""
    tags = sorted(r['id'] for r in declared)
    _require(all(0 <= t <= 0xFFFF for t in tags), 'record tags must fit u16 for RECORD_TAGS')
    _require(len(set(tags)) == len(tags), 'duplicate record tag')
    rows, line = [], '   '
    for tag in tags:
        item = f' {tag},'
        if len(line) + len(item) > 99:
            rows.append(line)
            line = '   '
        line += item
    rows.append(line)
    table = '\n'.join(rows)
    return f'''
/// The tags of the declared records, ascending (registrations are named by their constants).
const RECORD_TAGS: [u16; {len(tags)}] = [
{table}
];

/// Whether `tag` names a record of this codec ([`Record`]), without decoding anything: a
/// tag this is false for is the one [`RawRecord::record`] refuses as unknown
/// (ERR_UNSUPPORTED). It links a table of two bytes per record, no decoder.
pub fn record_tag_known(tag: u32) -> bool {{
    tag == REGISTRATION_EVENT_TAG
        || tag == REGISTRATION_DRAW_TAG
        || u16::try_from(tag).is_ok_and(|tag| RECORD_TAGS.binary_search(&tag).is_ok())
}}
'''


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


# The one walk of a record's fields every step of its codec goes through (length,
# presence bitmap, bytes): see `_record_source`.
FIELDS_TEMPLATE = '''
/// A record's fields in wire order, walked for its length, its presence bitmap or its
/// bytes: one walk per record type instead of a step function each.
trait Fields {
    fn fields(&self, sink: &mut dyn Sink);
}

/// What a walk does with each field. `mask` is the field's presence bit (0: always on
/// the wire); an optional field is only walked when it is present.
trait Sink {
    fn scalar(&mut self, bytes: &[u8], mask: u8);
    fn blob(&mut self, bytes: &[u8], cap: usize, mask: u8);
}

/// The fields' length and presence bitmap; `over` when a field is over its cap.
struct Measure {
    len: usize,
    bits: u8,
    over: bool,
}

impl Sink for Measure {
    fn scalar(&mut self, bytes: &[u8], mask: u8) {
        self.len += bytes.len();
        self.bits |= mask;
    }

    fn blob(&mut self, bytes: &[u8], cap: usize, mask: u8) {
        self.over |= bytes.len() > cap;
        self.len += varint_len(bytes.len() as u32) + bytes.len();
        self.bits |= mask;
    }
}

/// Writes the fields at `offset`; the caller checked the room.
struct Put<'o> {
    out: &'o mut [u8],
    offset: &'o mut usize,
}

impl Sink for Put<'_> {
    fn scalar(&mut self, bytes: &[u8], _: u8) {
        write_bytes(bytes, self.out, self.offset);
    }

    fn blob(&mut self, bytes: &[u8], _: usize, _: u8) {
        write_blob(bytes, self.out, self.offset);
    }
}

fn measure(record: &dyn Fields) -> Measure {
    let mut measure = Measure {
        len: 0,
        bits: 0,
        over: false,
    };
    record.fields(&mut measure);
    measure
}

/// The presence bitmap of the optional fields.
fn fields_bits(record: &dyn Fields) -> u8 {
    measure(record).bits
}

/// The body length: the presence bitmap (one byte more when an optional field is
/// present) and the fields; a field over its cap is ERR_NO_SPACE.
fn fields_len(record: &dyn Fields) -> Result<usize, i32> {
    let measure = measure(record);
    if measure.over {
        return Err(@NO_SPACE@);
    }
    Ok(1 + usize::from(measure.bits != 0) + measure.len)
}

/// The fields, without the presence bitmap.
fn fields_write(record: &dyn Fields, out: &mut [u8], offset: &mut usize) {
    record.fields(&mut Put { out, offset });
}

/// The body: the presence bitmap, then the fields.
fn fields_write_framed(record: &dyn Fields, out: &mut [u8], offset: &mut usize) {
    let bits = fields_bits(record);
    write_varint(u32::from(bits != 0), out, offset);
    if bits != 0 {
        write_bytes(&[bits], out, offset);
    }
    fields_write(record, out, offset);
}
'''


# Bare-record framing of service blobs (PR #198): emitted only with service
# records, so a schema without them renders exactly as before.
TYPED_TEMPLATE = '''
/// A record that encodes itself: its tag, its body length and its body. A plugin
/// encodes through it (`&dyn WireEncode`), so it links only the codecs of the record
/// types it sends; [`Record`]'s functions link every record type (the host's view).
pub trait WireEncode {
    /// Wire tag.
    fn wire_tag(&self) -> u32;
    /// Body bytes (presence bitmap and fields); a field over its cap is ERR_NO_SPACE.
    fn wire_body_len(&self) -> Result<usize, i32>;
    /// Writes the body; only this codec's functions can call it (they check the room).
    fn wire_write(&self, out: &mut [u8], offset: &mut usize, _: seal::Seal);
}

impl<T: WireEncode + ?Sized> WireEncode for &T {
    fn wire_tag(&self) -> u32 {
        (**self).wire_tag()
    }

    fn wire_body_len(&self) -> Result<usize, i32> {
        (**self).wire_body_len()
    }

    fn wire_write(&self, out: &mut [u8], offset: &mut usize, seal: seal::Seal) {
        (**self).wire_write(out, offset, seal);
    }
}

impl WireEncode for Registration {
    fn wire_tag(&self) -> u32 {
        match self {
            Registration::Event { .. } => REGISTRATION_EVENT_TAG,
            Registration::Draw { .. } => REGISTRATION_DRAW_TAG,
        }
    }

    fn wire_body_len(&self) -> Result<usize, i32> {
        Ok(match self {
            Registration::Event { .. } => 9,
            Registration::Draw { .. } => 5,
        })
    }

    fn wire_write(&self, out: &mut [u8], offset: &mut usize, _: seal::Seal) {
        write_varint(0, out, offset);
        match self {
            Registration::Event { event_tag, token } => {
                write_bytes(&event_tag.to_le_bytes(), out, offset);
                write_bytes(&token.to_le_bytes(), out, offset);
            }
            Registration::Draw { token } => write_bytes(&token.to_le_bytes(), out, offset),
        }
    }
}

/// One record type of the wire: what a plugin decodes, by type, linking only that
/// record's decoder (`RawRecord::decode`).
pub trait WireRecord<'a>: WireEncode + Copy + Sized {
    /// Wire tag.
    const TAG: u32;
    /// Decodes a body (presence bitmap and fields; unknown appended fields and bits are
    /// skipped), with the statuses of [`records_decode`].
    fn decode_body(body: &'a [u8]) -> Result<Self, i32>;
}

mod seal {
    /// Only this codec writes record bodies.
    #[derive(Debug)]
    pub struct Seal;
}

/// A record whose frame is checked and whose body is still undecoded: `RawRecord::decode`
/// decodes it as one record type, [`RawRecord::record`] as whichever its tag names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RawRecord<'a> {
    tag: u32,
    body: &'a [u8],
}

/// Its tag and body length only: the body may hold a secret field.
impl core::fmt::Debug for RawRecord<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawRecord")
            .field("tag", &self.tag)
            .field("body_len", &self.body.len())
            .finish()
    }
}

impl<'a> RawRecord<'a> {
    /// Its wire tag.
    pub fn tag(&self) -> u32 {
        self.tag
    }

    /// Decodes it as `R`; another tag is ERR_UNSUPPORTED (as an unknown tag is).
    pub fn decode<R: WireRecord<'a>>(&self) -> Result<R, i32> {
        if self.tag != R::TAG {
            return Err(@UNSUPPORTED@);
        }
        R::decode_body(self.body)
    }

    /// Decodes it as whichever record its tag names; this links every record's decoder.
    pub fn record(&self) -> Result<Record<'a>, i32> {
        record_decode(
            self.tag,
            &mut Reader {
                remaining: self.body,
            },
        )
    }

    /// The record of a batch that holds exactly one (count 1, the record, nothing after):
    /// the framing of [`records_decode_single`], the body not decoded yet.
    pub fn single(bytes: &'a [u8]) -> Result<Self, i32> {
        if bytes.len() > REGISTRATION_MAX_BYTES {
            return Err(@NO_SPACE@);
        }
        let mut reader = Reader { remaining: bytes };
        if reader.varint()? != 1 {
            return Err(@INVALID@);
        }
        let (tag, body) = reader.record()?;
        reader.finish()?;
        Ok(Self {
            tag,
            body: body.remaining,
        })
    }

    /// Exactly one bare record, nothing after it: the framing of [`record_decode_one`].
    pub fn one(bytes: &'a [u8]) -> Result<Self, i32> {
        if bytes.len() > REGISTRATION_MAX_BYTES {
            return Err(@NO_SPACE@);
        }
        let mut reader = Reader { remaining: bytes };
        let (tag, body) = reader.record()?;
        reader.finish()?;
        Ok(Self {
            tag,
            body: body.remaining,
        })
    }

    /// The first bare record of `bytes` and the frame length it took: the framing of
    /// `record_decode_prefix`.
    pub fn prefix(bytes: &'a [u8]) -> Result<(Self, usize), i32> {
        let limit = bytes.len().min(REGISTRATION_MAX_BYTES);
        let mut reader = Reader {
            remaining: bytes.get(..limit).unwrap_or(bytes),
        };
        let (tag, body) = reader.record()?;
        Ok((
            Self {
                tag,
                body: body.remaining,
            },
            limit - reader.remaining.len(),
        ))
    }
}

/// [`record_framed_len`] of a typed record.
pub fn record_framed_len_of(record: &dyn WireEncode) -> Result<usize, i32> {
    let body = record.wire_body_len()?;
    let len = varint_len(record.wire_tag()) + varint_len(body as u32) + body;
    if len > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    Ok(len)
}

/// [`record_encode_one`] of a typed record: every cap and the room are checked before
/// any write; a failure is ERR_NO_SPACE and leaves `out` unchanged.
pub fn record_encode_of(record: &dyn WireEncode, out: &mut [u8]) -> Result<usize, i32> {
    let required = record_framed_len_of(record)?;
    if out.len() < required {
        return Err(@NO_SPACE@);
    }
    let body = record.wire_body_len()?;
    let mut offset = 0;
    write_varint(record.wire_tag(), out, &mut offset);
    write_varint(body as u32, out, &mut offset);
    record.wire_write(out, &mut offset, seal::Seal);
    Ok(offset)
}

/// [`records_encode`] of typed records: the same framing, limits and statuses.
pub fn records_encode_of(records: &[&dyn WireEncode], out: &mut [u8]) -> Result<usize, i32> {
    if records.len() > REGISTRATION_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    records_encode_counted_of(
        records.iter().copied(),
        REGISTRATION_MAX_RECORDS,
        REGISTRATION_MAX_BYTES,
        out,
    )
}

/// `records_encode_counted_iter` of typed records: a counted batch of at most
/// `max_records` records and `max_bytes` bytes, walked three times (count, size, write),
/// with its statuses; nothing is ever cut.
pub fn records_encode_counted_of<R, I>(
    records: I,
    max_records: usize,
    max_bytes: usize,
    out: &mut [u8],
) -> Result<usize, i32>
where
    R: WireEncode,
    I: Iterator<Item = R> + Clone,
{
    let count = records.clone().take(max_records.saturating_add(1)).count();
    if count > max_records || count > u32::MAX as usize {
        return Err(@NO_SPACE@);
    }
    let mut required = varint_len(count as u32);
    for record in records.clone() {
        required += record_framed_len_of(&record)?;
        if required > max_bytes {
            return Err(@NO_SPACE@);
        }
    }
    if out.len() < required {
        return Err(@NO_SPACE@);
    }
    let mut offset = 0;
    write_varint(count as u32, out, &mut offset);
    let mut written = 0;
    for record in records {
        written += 1;
        let rest = out.get_mut(offset..required).ok_or(@INVALID@)?;
        offset += record_encode_of(&record, rest).map_err(|_| @INVALID@)?;
    }
    if written != count || offset != required {
        return Err(@INVALID@);
    }
    Ok(offset)
}
'''

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

/// Encode a counted batch of at most `max_records` records and `max_bytes`
/// bytes: the framing of [`records_encode`], with the limits a service op
/// names (its `input_rows` and `max_input`) in place of
/// [`REGISTRATION_MAX_RECORDS`]. Every cap and the output capacity are checked
/// before any write. Failure returns ERR_NO_SPACE and leaves the entire output
/// unchanged; nothing is ever cut.
pub fn records_encode_counted(
    records: &[Record<'_>],
    max_records: usize,
    max_bytes: usize,
    out: &mut [u8],
) -> Result<usize, i32> {
    records_encode_counted_iter(records.iter().copied(), max_records, max_bytes, out)
}

/// [`records_encode_counted`] over records made on the fly: no slice of
/// them is ever collected. The iterator is cloned and walked three times
/// (count, size, write), so it must yield the same records each time, as a
/// clone of an iterator over borrowed data does. Every cap and the output
/// capacity are checked before any write, in the order and with the statuses
/// of [`records_encode_counted`]. An iterator that yields other records on
/// the writing walk is refused with ERR_INVALID; the output is then
/// unspecified, and it is never written past `out`.
pub fn records_encode_counted_iter<'r, I>(
    records: I,
    max_records: usize,
    max_bytes: usize,
    out: &mut [u8],
) -> Result<usize, i32>
where
    I: Iterator<Item = Record<'r>> + Clone,
{
    records_encode_counted_of(records, max_records, max_bytes, out)
}

/// The enum is one more record type of the typed codec: its encoders share one body
/// ([`records_encode_counted_of`]) with the typed ones.
impl WireEncode for Record<'_> {
    fn wire_tag(&self) -> u32 {
        self.tag()
    }

    fn wire_body_len(&self) -> Result<usize, i32> {
        self.body_len()
    }

    fn wire_write(&self, out: &mut [u8], offset: &mut usize, _: seal::Seal) {
        @SELF_BITMAP@
        self.write(out, offset);
    }
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
#[derive(Clone)]
pub struct RecordStream {
    carry: [u8; RECORD_CARRY_MAX_BYTES],
    len: usize,
}

/// The carried length only: the carried bytes may be part of a secret field.
impl core::fmt::Debug for RecordStream {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RecordStream")
            .field("carry_len", &self.len)
            .finish()
    }
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
        chunk: &[u8],
        visit: &mut dyn FnMut(Record<'_>) -> Result<(), i32>,
    ) -> Result<(), i32> {
        self.feed_raw(chunk, &mut |raw| visit(raw.record()?))
    }

    /// [`RecordStream::feed`] with each record's frame checked and its body not
    /// decoded: the visitor decodes the record types it expects
    /// (`RawRecord::decode`), so a plugin links no other record's decoder.
    pub fn feed_raw(
        &mut self,
        mut chunk: &[u8],
        visit: &mut dyn FnMut(RawRecord<'_>) -> Result<(), i32>,
    ) -> Result<(), i32> {
        if self.len > 0 {
            let old = self.len;
            let moved = chunk.len().min(RECORD_CARRY_MAX_BYTES - old);
            self.carry[old..old + moved].copy_from_slice(&chunk[..moved]);
            match frame_len(&self.carry[..old + moved])? {
                Some(len) => {
                    let (record, _) = RawRecord::prefix(&self.carry[..len])?;
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
                    let (record, _) = RawRecord::prefix(&chunk[..len])?;
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
