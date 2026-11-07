"""Validate and emit the bounded slot-15 command batch codec."""
from abi_declarations import growth_fields, is_growth_command


def _require(ok, message):
    if not ok:
        raise ValueError(message)


def _field(name, kind, since, max_bytes):
    return dict(name=name, type=kind, since=since, optional=False, ordinal=None,
                default=None, max_bytes=max_bytes)


def _status(schema, name, value, since):
    matches = [item for item in schema.get('statuses', [])
               if type(item) is dict and item.get('name') == name]
    _require(len(matches) == 1 and type(matches[0].get('value')) is int and
             matches[0]['value'] == value and type(matches[0].get('since')) is int and
             1 <= matches[0]['since'] <= since, 'command codec status contract')


# Commands beyond SetSetting (docs/abi/screens-input.md section 3): each is
# optional, and when declared its fields are exactly these (id caps from the
# schema, at most 255 bytes).
EXTRA_COMMANDS = {
    'command.open_screen': ('OpenScreen', 'screen'),
    'command.run_action': ('RunAction', 'action'),
    'command.redraw': ('Redraw', 'mode'),
    'command.close_screen': ('CloseScreen', None),
    'command.set_item_state': ('SetItemState', 'item'),
}

# Fields of command.set_item_state, in order: a relative item id, flag bits,
# then a label and a badge, each a catalog key and an i18n argument batch.
# Label and badge share their caps.
ITEM_STATE_FIELDS = (('item', 'str'), ('flags', 'u32'), ('label', 'str'),
                     ('label_args', 'bytes'), ('badge', 'str'), ('badge_args', 'bytes'))
# Flag bits of command.set_item_state (docs/abi/screens-input.md section 3.1).
ITEM_FLAGS = (('HIDDEN', 'Not shown in menus.'),
              ('FAINT', 'Shown faint; it cannot be chosen.'),
              ('SWITCH', 'Shown with a switch; ON sets it on.'),
              ('ON', 'The switch is on; needs SWITCH.'),
              ('KEEP', 'The host keeps this state across restarts until the plugin changes.'))


def _extras(schema):
    """The declared extra command records, in EXTRA_COMMANDS order."""
    records = {item.get('name'): item for item in schema.get('records', [])
               if type(item) is dict}
    return [records[name] for name in EXTRA_COMMANDS if name in records]


def _growth(schema):
    """The growth-block commands, in schema order, generated from their fields."""
    return [item for item in schema.get('records', []) if is_growth_command(item)]


def _variant(name):
    """`command.bind_key` is `BindKey`; `command.a.b_c` is `ABC` (dots as underscores)."""
    rest = name.split('.', 1)[1].replace('.', '_')
    return ''.join(part.capitalize() for part in rest.split('_'))


def _validate_extra(record, descriptor, schema):
    _require(type(record.get('id')) is int and 0 <= record['id'] <= 65535,
             'command tag must be an unsigned declaration ID')
    _require(type(record.get('since')) is int and
             descriptor['since'] <= record['since'] <= schema.get('minor', 0),
             'command since outside codec/schema minor')
    fields, since = record.get('fields'), record['since']
    _require(type(fields) is list, 'command fields must be a list')
    name, (_, field) = record['name'], EXTRA_COMMANDS[record['name']]
    if name == 'command.close_screen':
        expected = []
    elif name == 'command.set_item_state':
        _require(len(fields) == len(ITEM_STATE_FIELDS) and
                 all(type(item) is dict and type(item.get('max_bytes')) is int
                     for item in fields), 'item state command fields')
        caps = [item['max_bytes'] for item in fields]
        _require(1 <= caps[0] <= 255 and 1 <= caps[2] <= 255 and caps[2] == caps[4] and
                 1 <= caps[3] <= 1024 and caps[3] == caps[5], 'item state command caps')
        expected = [_field(field, kind, since, 0 if kind == 'u32' else cap)
                    for (field, kind), cap in zip(ITEM_STATE_FIELDS, caps)]
    elif name == 'command.redraw':
        expected = [_field(field, 'u32', since, 0)]
    else:
        _require(len(fields) == 1 and type(fields[0]) is dict and
                 type(fields[0].get('max_bytes')) is int and
                 1 <= fields[0]['max_bytes'] <= 255, 'command id cap')
        expected = [_field(field, 'str', since, fields[0]['max_bytes'])]
    _require(fields == expected, 'unsupported command record fields')


def validate_commands(schema):
    """The closed descriptor admits the SetSetting record and the optional
    commands of EXTRA_COMMANDS, nothing else."""
    records = schema.get('records', [])
    _require(type(records) is list, 'records must be a list')
    recognized = [item for item in records
                  if type(item) is dict and item.get('name') == 'command.set_setting']
    if not recognized and 'command_codec' not in schema:
        return
    descriptor = schema.get('command_codec')
    _require(type(descriptor) is dict and set(descriptor) == {'since', 'max_records'},
             'command codec descriptor keys')
    _require(type(descriptor['since']) is int and
             1 <= descriptor['since'] <= schema.get('minor', 0),
             'command codec since outside schema minor')
    _require(type(descriptor['max_records']) is int and
             1 <= descriptor['max_records'] <= 8, 'command codec count cap')
    _require(len(recognized) == 1, 'command codec needs exactly one SetSetting record')
    known = {'command.set_setting', *EXTRA_COMMANDS}
    _require(all(item.get('name') in known or is_growth_command(item) for item in records
                 if type(item) is dict and str(item.get('name', '')).startswith('command.')),
             'unknown command record')
    for extra in _extras(schema):
        _validate_extra(extra, descriptor, schema)
    variants = {'SetSetting', *(EXTRA_COMMANDS[item['name']][0] for item in _extras(schema))}
    for item in _growth(schema):
        _require(type(item.get('since')) is int and
                 descriptor['since'] <= item['since'] <= schema.get('minor', 0),
                 'command since outside codec/schema minor')
        for field in growth_fields(item):
            _require(field['since'] <= schema.get('minor', 0),
                     f'{item["name"]}.{field["name"]}: since outside schema minor')
            _require(not field['optional'], f'{item["name"]}: growth commands have no optional '
                     'fields yet')
        variant = _variant(item['name'])
        _require(variant not in variants, f'{item["name"]}: variant {variant} taken')
        variants.add(variant)
    ids = [item['id'] for item in [recognized[0], *_extras(schema), *_growth(schema)]]
    _require(len(set(ids)) == len(ids), 'duplicate command tag')
    record = recognized[0]
    _require(type(record.get('id')) is int and 0 <= record['id'] <= 65535,
             'command tag must be an unsigned declaration ID')
    _require(type(record.get('since')) is int and
             descriptor['since'] <= record['since'] <= schema.get('minor', 0),
             'command since outside codec/schema minor')
    value = schema.get('settings_value')
    _require(type(value) is dict and type(value.get('max_bytes')) is int,
             'command values need the settings value contract')
    fields = record.get('fields')
    _require(type(fields) is list and len(fields) == 2 and
             type(fields[0]) is dict and type(fields[0].get('max_bytes')) is int and
             1 <= fields[0]['max_bytes'] <= 255, 'command key cap')
    expected = [_field('key', 'str', record['since'], fields[0]['max_bytes']),
                _field('value', 'bytes', record['since'], value['max_bytes'])]
    _require(fields == expected, 'unsupported command record fields')
    since = descriptor['since']
    for name, number in [('ERR_UNSUPPORTED', -1), ('ERR_INVALID', -2),
                         ('ERR_NO_SPACE', -3), ('ERR_UTF8', -5)]:
        _status(schema, name, number, since)
    entries = [item for item in schema.get('host_entries', [])
               if type(item) is dict and item.get('name') == 'commands.submit']
    _require(len(entries) == 1 and type(entries[0].get('max_bytes')) is int and
             entries[0]['max_bytes'] == 4096, 'command codec byte cap')


def compatible_commands(old, new, minimum_since):
    """The emitted descriptor is immutable after publication."""
    validate_commands(old)
    validate_commands(new)
    before, after = old.get('command_codec'), new.get('command_codec')
    if before is None:
        if after is not None:
            _require(after['since'] >= minimum_since, 'backdated command codec')
        return
    _require(after == before, 'changed or removed command codec contract')


def rust_commands(schema):
    """Emit a no_std module beside the generated settings value codec."""
    validate_commands(schema)
    if 'command_codec' not in schema:
        return ''
    record = next(item for item in schema['records'] if item['name'] == 'command.set_setting')
    statuses = {item['name']: item['value'] for item in schema['statuses']}
    replacements = {
        '@COUNT@': schema['command_codec']['max_records'],
        '@TAG@': record['id'],
        '@KEY@': record['fields'][0]['max_bytes'],
        '@MAX@': next(item['max_bytes'] for item in schema['host_entries']
                      if item['name'] == 'commands.submit'),
        '@INVALID@': statuses['ERR_INVALID'],
        '@UNSUPPORTED@': statuses['ERR_UNSUPPORTED'],
        '@NO_SPACE@': statuses['ERR_NO_SPACE'],
        '@UTF8@': statuses['ERR_UTF8'],
    }
    source = '''// Generated by tools/abi-gen.py; do not edit.
use super::settings_wire::{SettingValueRef, settings_decode, settings_encode};

/// Maximum encoded command batch length.
pub const COMMAND_MAX_BYTES: usize = @MAX@;
/// Maximum commands in one batch.
pub const COMMAND_MAX_RECORDS: usize = @COUNT@;
/// Maximum UTF-8 bytes of one setting key.
pub const COMMAND_MAX_KEY_BYTES: usize = @KEY@;
/// Wire tag of the SetSetting command.
pub const COMMAND_SET_SETTING_TAG: u32 = @TAG@;

/// A command borrowed from one validated batch. No Rust value crosses the ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command<'a> {
    /// Set a setting to a present value of the settings value contract.
    SetSetting {
        /// Nonempty UTF-8 setting key.
        key: &'a str,
        /// Present Boolean, integer or text value; never absent.
        value: SettingValueRef<'a>,
    },
}

/// A fully validated, allocation-free command batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandBatch<'a> {
    records: [Option<Command<'a>>; COMMAND_MAX_RECORDS],
    len: usize,
}

impl<'a> CommandBatch<'a> {
    /// Number of validated commands.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether this batch has no commands.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate over commands by value in wire order.
    pub fn iter(&self) -> impl Iterator<Item = Command<'a>> + '_ {
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

    fn bytes(&mut self) -> Result<&'a [u8], i32> {
        let len = self.varint()? as usize;
        self.take(len)
    }

    fn record(&mut self) -> Result<(u32, Reader<'a>), i32> {
        let tag = self.varint()?;
        let body = Reader {
            remaining: self.bytes()?,
        };
        Ok((tag, body))
    }

    fn bitmap(&mut self) -> Result<(), i32> {
        // Unknown optional bits and fields are skipped using the record length.
        self.bytes().map(|_| ())
    }

    fn finish(self) -> Result<(), i32> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(@INVALID@)
        }
    }
}

fn key_check(key: &[u8]) -> Result<(), i32> {
    if key.is_empty() {
        Err(@INVALID@)
    } else if key.len() > COMMAND_MAX_KEY_BYTES {
        Err(@NO_SPACE@)
    } else {
        Ok(())
    }
}

fn set_setting<'a>(body: &mut Reader<'a>) -> Result<Command<'a>, i32> {
    body.bitmap()?;
    let key = body.bytes()?;
    key_check(key)?;
    let key = core::str::from_utf8(key).map_err(|_| @UTF8@)?;
    let value = settings_decode(body.bytes()?)?;
    if value == SettingValueRef::Absent {
        return Err(@INVALID@);
    }
    Ok(Command::SetSetting { key, value })
}

/// Decode an entire batch before exposing any command.
///
/// Framing must be canonical u32 LEB128. Unknown command tags and unknown
/// setting value kinds return ERR_UNSUPPORTED; malformed records, empty keys
/// and absent values return ERR_INVALID; invalid key or text UTF-8 returns
/// ERR_UTF8; byte, count and key limits return ERR_NO_SPACE. Unknown trailing
/// fields and extended presence bitmaps inside a known record are accepted.
pub fn commands_decode(bytes: &[u8]) -> Result<CommandBatch<'_>, i32> {
    if bytes.len() > COMMAND_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    let len = reader.varint()? as usize;
    if len > COMMAND_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    let mut batch = CommandBatch {
        records: [None; COMMAND_MAX_RECORDS],
        len,
    };
    for slot in &mut batch.records[..len] {
        let (tag, mut body) = reader.record()?;
        if tag != COMMAND_SET_SETTING_TAG {
            return Err(@UNSUPPORTED@);
        }
        *slot = Some(set_setting(&mut body)?);
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

fn set_setting_len(key: &str, value: SettingValueRef<'_>) -> Result<(usize, usize), i32> {
    key_check(key.as_bytes())?;
    if value == SettingValueRef::Absent {
        return Err(@INVALID@);
    }
    let value_len = settings_encode(value, &mut [])?;
    let body =
        1 + varint_len(key.len() as u32) + key.len() + varint_len(value_len as u32) + value_len;
    if body > COMMAND_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    Ok((body, value_len))
}

/// Encode a canonical batch without allocating.
///
/// Every key, value and bound, and the complete output capacity, are checked
/// before any write. Failure leaves the entire output unchanged. Empty keys and
/// absent values return ERR_INVALID; limits and short output return ERR_NO_SPACE.
pub fn commands_encode(commands: &[Command<'_>], out: &mut [u8]) -> Result<usize, i32> {
    if commands.len() > COMMAND_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    let mut required = varint_len(commands.len() as u32);
    for command in commands {
        let Command::SetSetting { key, value } = *command;
        let (body, _) = set_setting_len(key, value)?;
        required += varint_len(COMMAND_SET_SETTING_TAG) + varint_len(body as u32) + body;
        if required > COMMAND_MAX_BYTES {
            return Err(@NO_SPACE@);
        }
    }
    if out.len() < required {
        return Err(@NO_SPACE@);
    }
    let mut offset = 0;
    write_varint(commands.len() as u32, out, &mut offset);
    for command in commands {
        let Command::SetSetting { key, value } = *command;
        let (body, value_len) = set_setting_len(key, value)?;
        write_varint(COMMAND_SET_SETTING_TAG, out, &mut offset);
        write_varint(body as u32, out, &mut offset);
        write_varint(0, out, &mut offset);
        write_varint(key.len() as u32, out, &mut offset);
        out[offset..offset + key.len()].copy_from_slice(key.as_bytes());
        offset += key.len();
        write_varint(value_len as u32, out, &mut offset);
        let target = &mut out[offset..offset + value_len];
        // SAFETY: u8 and MaybeUninit<u8> have the same layout. The encoder only
        // writes initialized bytes, so the slice stays initialized afterwards.
        let target = unsafe { &mut *(target as *mut [u8] as *mut [core::mem::MaybeUninit<u8>]) };
        settings_encode(value, target)?;
        offset += value_len;
    }
    Ok(offset)
}
'''
    for marker, value in replacements.items():
        source = source.replace(marker, str(value))
    extras, growth = _extras(schema), _growth(schema)
    if extras or growth:
        source = _extended(source, extras, statuses, growth)
    return source


def _swap(source, old, new):
    _require(source.count(old) == 1, 'command template anchor')
    return source.replace(old, new)


def _const(name):
    return 'COMMAND_' + name.split('.', 1)[1].upper() + '_TAG'


def _extended(source, extras, statuses, growth=()):
    """The codec with the declared extra and growth-block commands. Applied to the
    SetSetting-only source only when either exists, so that source stays byte for
    byte as locked."""
    names = {item['name']: item for item in extras}
    invalid, unsupported = statuses['ERR_INVALID'], statuses['ERR_UNSUPPORTED']
    no_space, utf8 = statuses['ERR_NO_SPACE'], statuses['ERR_UTF8']
    consts, variants, arms, lens, writes = [], [], [], [], []
    for item in extras:
        variant, field = EXTRA_COMMANDS[item['name']]
        tag = _const(item['name'])
        consts.append(f"/// Wire tag of the {variant} command.\npub const {tag}: u32 = {item['id']};\n")
        if field in ('screen', 'action'):
            cap = f"COMMAND_MAX_{field.upper()}_BYTES"
            consts.append(f"/// Maximum UTF-8 bytes of the full {field} id of {variant}.\n"
                          f"pub const {cap}: usize = {item['fields'][0]['max_bytes']};\n")
            variants.append(f"    /// {'Open a registered screen' if field == 'screen' else 'Run a registered action'}, "
                            f"own or foreign, by its full id.\n"
                            f"    {variant} {{\n        /// Nonempty UTF-8 full {field} id.\n"
                            f"        {field}: &'a str,\n    }},\n")
            arms.append(f"            {tag} => {{\n                body.bitmap()?;\n"
                        f"                Command::{variant} {{\n"
                        f"                    {field}: id(body.bytes()?, {cap})?,\n"
                        f"                }}\n            }}\n")
            lens.append(f"        Command::{variant} {{ {field} }} => {{\n"
                        f"            id({field}.as_bytes(), {cap})?;\n"
                        f"            Ok((\n                {tag},\n"
                        f"                1 + varint_len({field}.len() as u32) + {field}.len(),\n"
                        f"            ))\n"
                        f"        }}\n")
            writes.append(f"            Command::{variant} {{ {field} }} => {{\n"
                          f"                write_varint({field}.len() as u32, out, &mut offset);\n"
                          f"                out[offset..offset + {field}.len()].copy_from_slice({field}.as_bytes());\n"
                          f"                offset += {field}.len();\n            }}\n")
        elif field == 'mode':
            consts.append("/// Highest redraw mode: 0 redraw only, 1 fast, 2 partial, 3 full.\n"
                          "pub const COMMAND_REDRAW_MAX_MODE: u32 = 3;\n")
            variants.append(f"    /// Redraw the submitting plugin's screen on top in `mode`.\n"
                            f"    {variant} {{\n        /// 0 redraw only, 1 fast, 2 partial, 3 full.\n"
                            f"        mode: u32,\n    }},\n")
            arms.append(f"            {tag} => {{\n                body.bitmap()?;\n"
                        f"                Command::{variant} {{\n"
                        f"                    mode: mode(u32_le(body.take(4)?))?,\n"
                        f"                }}\n            }}\n")
            lens.append(f"        Command::{variant} {{ mode: value }} => {{\n"
                        f"            mode(value)?;\n            Ok(({tag}, 1 + 4))\n        }}\n")
            writes.append(f"            Command::{variant} {{ mode }} => {{\n"
                          f"                out[offset..offset + 4].copy_from_slice(&mode.to_le_bytes());\n"
                          f"                offset += 4;\n            }}\n")
        elif field == 'item':
            _item_state(item, tag, consts, variants, arms, lens, writes)
        else:
            variants.append(f"    /// Close the submitting plugin's own screen on top.\n    {variant},\n")
            arms.append(f"            {tag} => {{\n                body.bitmap()?;\n"
                        f"                Command::{variant}\n            }}\n")
            lens.append(f"        Command::{variant} => Ok(({tag}, 1)),\n")
            writes.append(f"            Command::{variant} => {{}}\n")
    for item in growth:
        _growth_command(item, consts, variants, arms, lens, writes)
    source = _swap(source, "pub const COMMAND_SET_SETTING_TAG: u32 = ",
                   "".join(consts) + "pub const COMMAND_SET_SETTING_TAG: u32 = ")
    source = _swap(source, "        value: SettingValueRef<'a>,\n    },\n}\n",
                   "        value: SettingValueRef<'a>,\n    },\n" + "".join(variants) + "}\n")
    helpers = ''
    if any(EXTRA_COMMANDS[n][1] in ('screen', 'action', 'item') for n in names):
        helpers += f"""
fn id(bytes: &[u8], cap: usize) -> Result<&str, i32> {{
    if bytes.is_empty() {{
        Err({invalid})
    }} else if bytes.len() > cap {{
        Err({no_space})
    }} else {{
        core::str::from_utf8(bytes).map_err(|_| {utf8})
    }}
}}
"""
    if 'command.redraw' in names or 'command.set_item_state' in names:
        helpers += """
fn u32_le(bytes: &[u8]) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(bytes);
    u32::from_le_bytes(value)
}
"""
    if 'command.redraw' in names:
        helpers += f"""
fn mode(mode: u32) -> Result<u32, i32> {{
    if mode > COMMAND_REDRAW_MAX_MODE {{
        Err({invalid})
    }} else {{
        Ok(mode)
    }}
}}
"""
    if 'command.set_item_state' in names:
        helpers += _ITEM_STATE_HELPERS.replace('@INVALID@', str(invalid)).replace(
            '@NO_SPACE@', str(no_space)).replace('@UTF8@', str(utf8))
    if growth:
        helpers += _GROWTH_HELPERS.replace('@NO_SPACE@', str(no_space)).replace(
            '@UTF8@', str(utf8))
    if any(field['items'] for item in growth for field in growth_fields(item)):
        helpers += _LIST_HELPER.replace('@INVALID@', str(invalid))
    source = _swap(source, "\n/// Decode an entire batch before exposing any command.",
                   helpers + "\n/// Decode an entire batch before exposing any command.")
    source = _swap(source, f"""        if tag != COMMAND_SET_SETTING_TAG {{
            return Err({unsupported});
        }}
        *slot = Some(set_setting(&mut body)?);
""", f"""        *slot = Some(match tag {{
            COMMAND_SET_SETTING_TAG => set_setting(&mut body)?,
{''.join(arms)}            _ => return Err({unsupported}),
        }});
""")
    body_len = """
/// A command's tag and body length, every bound checked.
fn command_len(command: &Command<'_>) -> Result<(u32, usize), i32> {
    match *command {
        Command::SetSetting { key, value } => {
            let (body, _) = set_setting_len(key, value)?;
            Ok((COMMAND_SET_SETTING_TAG, body))
        }
""" + "".join(lens) + """    }
}
"""
    source = _swap(source, "\n/// Encode a canonical batch without allocating.",
                   body_len + "\n/// Encode a canonical batch without allocating.")
    source = _swap(source, """        let Command::SetSetting { key, value } = *command;
        let (body, _) = set_setting_len(key, value)?;
        required += varint_len(COMMAND_SET_SETTING_TAG) + varint_len(body as u32) + body;
""", """        let (tag, body) = command_len(command)?;
        required += varint_len(tag) + varint_len(body as u32) + body;
""")
    writes = ['    ' + line if line else line for line in ''.join(writes).split('\n')]
    source = _swap(source, """        let Command::SetSetting { key, value } = *command;
        let (body, value_len) = set_setting_len(key, value)?;
        write_varint(COMMAND_SET_SETTING_TAG, out, &mut offset);
        write_varint(body as u32, out, &mut offset);
        write_varint(0, out, &mut offset);
        write_varint(key.len() as u32, out, &mut offset);""", """        let (tag, body) = command_len(command)?;
        write_varint(tag, out, &mut offset);
        write_varint(body as u32, out, &mut offset);
        write_varint(0, out, &mut offset);
        let Command::SetSetting { key, value } = *command else {
            match *command {
""" + "\n".join(writes) + """                Command::SetSetting { .. } => {}
            }
            continue;
        };
        let (_, value_len) = set_setting_len(key, value)?;
        write_varint(key.len() as u32, out, &mut offset);""")
    return source


_ITEM_STATE_HELPERS = """
fn item_flags(flags: u32) -> Result<u32, i32> {
    if flags & !COMMAND_ITEM_FLAGS != 0
        || (flags & COMMAND_ITEM_ON != 0 && flags & COMMAND_ITEM_SWITCH == 0)
    {
        Err(@INVALID@)
    } else {
        Ok(flags)
    }
}

/// A label or badge: a catalog key, or empty for none; arguments need a key.
fn message<'a>(key: &'a [u8], args: &[u8]) -> Result<&'a str, i32> {
    if key.len() > COMMAND_MAX_ITEM_KEY_BYTES || args.len() > COMMAND_MAX_ITEM_ARGS_BYTES {
        Err(@NO_SPACE@)
    } else if key.is_empty() && !args.is_empty() {
        Err(@INVALID@)
    } else {
        core::str::from_utf8(key).map_err(|_| @UTF8@)
    }
}

fn item_state<'a>(body: &mut Reader<'a>) -> Result<Command<'a>, i32> {
    body.bitmap()?;
    let item = id(body.bytes()?, COMMAND_MAX_ITEM_BYTES)?;
    let flags = item_flags(u32_le(body.take(4)?))?;
    let (label, label_args) = (body.bytes()?, body.bytes()?);
    let label = message(label, label_args)?;
    let (badge, badge_args) = (body.bytes()?, body.bytes()?);
    let badge = message(badge, badge_args)?;
    Ok(Command::SetItemState {
        item,
        flags,
        label,
        label_args,
        badge,
        badge_args,
    })
}

fn field_len(len: usize) -> usize {
    varint_len(len as u32) + len
}

fn write_field(bytes: &[u8], out: &mut [u8], offset: &mut usize) {
    write_varint(bytes.len() as u32, out, offset);
    out[*offset..*offset + bytes.len()].copy_from_slice(bytes);
    *offset += bytes.len();
}
"""


def _item_state(item, tag, consts, variants, arms, lens, writes):
    """The SetItemState pieces of the extended codec."""
    caps = [field['max_bytes'] for field in item['fields']]
    consts.append(f"/// Maximum UTF-8 bytes of the relative item id of SetItemState.\n"
                  f"pub const COMMAND_MAX_ITEM_BYTES: usize = {caps[0]};\n"
                  f"/// Maximum UTF-8 bytes of the label or badge catalog key of SetItemState.\n"
                  f"pub const COMMAND_MAX_ITEM_KEY_BYTES: usize = {caps[2]};\n"
                  f"/// Maximum bytes of the label or badge argument batch of SetItemState.\n"
                  f"pub const COMMAND_MAX_ITEM_ARGS_BYTES: usize = {caps[3]};\n")
    for bit, (flag, doc) in enumerate(ITEM_FLAGS):
        consts.append(f"/// SetItemState flag: {doc}\npub const COMMAND_ITEM_{flag}: u32 = {1 << bit};\n")
    consts.append(f"/// Every SetItemState flag; other bits are refused.\n"
                  f"pub const COMMAND_ITEM_FLAGS: u32 = {(1 << len(ITEM_FLAGS)) - 1};\n")
    variants.append(
        "    /// Set what menus show of one of the submitting plugin's own actions or\n"
        "    /// settings; the host resolves the keys in its catalogs when it shows them.\n"
        "    SetItemState {\n"
        "        /// Nonempty UTF-8 item id, relative to the submitting plugin.\n"
        "        item: &'a str,\n"
        "        /// `COMMAND_ITEM_*` bits; `COMMAND_ITEM_ON` needs `COMMAND_ITEM_SWITCH`.\n"
        "        flags: u32,\n"
        "        /// Catalog key shown in place of the title, or empty for the title.\n"
        "        label: &'a str,\n"
        "        /// Encoded i18n argument batch for `label`, or empty.\n"
        "        label_args: &'a [u8],\n"
        "        /// Catalog key of a short text at the item's right, or empty for none.\n"
        "        badge: &'a str,\n"
        "        /// Encoded i18n argument batch for `badge`, or empty.\n"
        "        badge_args: &'a [u8],\n"
        "    },\n")
    arms.append(f"            {tag} => item_state(&mut body)?,\n")
    lens.append(
        "        Command::SetItemState {\n"
        "            item,\n            flags,\n            label,\n            label_args,\n"
        "            badge,\n            badge_args,\n"
        "        } => {\n"
        "            id(item.as_bytes(), COMMAND_MAX_ITEM_BYTES)?;\n"
        "            item_flags(flags)?;\n"
        "            message(label.as_bytes(), label_args)?;\n"
        "            message(badge.as_bytes(), badge_args)?;\n"
        "            let fields = [\n"
        "                item.as_bytes(),\n                label.as_bytes(),\n                label_args,\n"
        "                badge.as_bytes(),\n                badge_args,\n            ];\n"
        "            let body = fields.iter().map(|field| field_len(field.len()));\n"
        f"            Ok(({tag}, 1 + 4 + body.sum::<usize>()))\n"
        "        }\n")
    writes.append(
        "            Command::SetItemState {\n"
        "                item,\n                flags,\n                label,\n"
        "                label_args,\n                badge,\n                badge_args,\n"
        "            } => {\n"
        "                write_field(item.as_bytes(), out, &mut offset);\n"
        "                out[offset..offset + 4].copy_from_slice(&flags.to_le_bytes());\n"
        "                offset += 4;\n"
        "                for field in [label.as_bytes(), label_args, badge.as_bytes(), badge_args] {\n"
        "                    write_field(field, out, &mut offset);\n"
        "                }\n"
        "            }\n")


# The item rules of a list field: empty text is no items; otherwise every item
# (between separators) is non-empty and within its cap, and there are at most
# `max_items`. A trailing separator is an empty item and refused. The SDK's encoder
# and the host's decoder both run it, so both refuse the same lists.
_LIST_HELPER = """
fn list(text: &str, separator: char, max_items: usize, item_max: usize) -> Result<&str, i32> {
    if text.is_empty() {
        return Ok(text);
    }
    let mut count = 0;
    for item in text.split(separator) {
        count += 1;
        if count > max_items || item.is_empty() || item.len() > item_max {
            return Err(@INVALID@);
        }
    }
    Ok(text)
}
"""


# Field helpers of the growth-block commands: the generated record codec's rules
# (text and bytes within their caps, empty allowed; little-endian scalars).
_GROWTH_HELPERS = """
fn text(bytes: &[u8], cap: usize) -> Result<&str, i32> {
    if bytes.len() > cap {
        Err(@NO_SPACE@)
    } else {
        core::str::from_utf8(bytes).map_err(|_| @UTF8@)
    }
}

fn blob(bytes: &[u8], cap: usize) -> Result<&[u8], i32> {
    if bytes.len() > cap {
        Err(@NO_SPACE@)
    } else {
        Ok(bytes)
    }
}

fn scalar<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut value = [0; N];
    value.copy_from_slice(bytes);
    value
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
    out[*offset..*offset + bytes.len()].copy_from_slice(bytes);
    *offset += bytes.len();
}
"""

_SCALAR_WIDTH = {'u32': 4, 'i32': 4, 'u64': 8}


def _pattern(indent, variant, names):
    """`Command::Variant { a, b }` as rustfmt lays it out: one field a line once
    the fields take more than 18 columns (its struct literal width)."""
    if len(', '.join(names)) <= 18:
        return f"{indent}Command::{variant} {{ {', '.join(names)} }}"
    lines = [f"{indent}    {name}{'' if name == '..' else ','}\n" for name in names]
    return f"{indent}Command::{variant} {{\n{''.join(lines)}{indent}}}"


def _growth_command(item, consts, variants, arms, lens, writes):
    """The pieces of one growth-block command, generated from its fields."""
    name, variant = item['name'], _variant(item['name'])
    base = 'COMMAND_' + name.split('.', 1)[1].upper().replace('.', '_')
    tag = base + '_TAG'
    fields = growth_fields(item)
    consts.append(f"/// Wire tag of the `{name}` command.\npub const {tag}: u32 = {item['id']};\n")
    docs, decode, sized, written, lists = [], [], [], [], []
    for field in fields:
        label, kind = field['name'], field['kind']
        if kind in _SCALAR_WIDTH:
            width = _SCALAR_WIDTH[kind]
            docs.append(f"        /// Little-endian {kind}.\n        {label}: {kind},\n")
            decode.append(f"                    {label}: {kind}::from_le_bytes(scalar(body.take({width})?)),\n")
            written.append(f"                out[offset..offset + {width}].copy_from_slice(&{label}.to_le_bytes());\n"
                           f"                offset += {width};\n")
            continue
        cap = f"{base}_{label.upper()}_MAX_BYTES"
        consts.append(f"/// Maximum bytes of `{name}.{label}`.\npub const {cap}: usize = {field['cap']};\n")
        items = field['items']
        if items:
            stem = f"{base}_{label.upper()}"
            separator = repr(items['separator']).strip("'")
            consts.append(f"/// Item separator of `{name}.{label}`.\n"
                          f"pub const {stem}_SEPARATOR: char = '{separator}';\n"
                          f"/// Maximum items of `{name}.{label}`.\n"
                          f"pub const {stem}_MAX_ITEMS: usize = {items['max_items']};\n"
                          f"/// Maximum bytes of one item of `{name}.{label}`.\n"
                          f"pub const {stem}_ITEM_MAX_BYTES: usize = {items['item_max_bytes']};\n")
        if kind == 'str':
            what = (f"UTF-8 list of at most {items['max_items']} items of at most "
                    f"{items['item_max_bytes']} bytes, at most {field['cap']} bytes in all"
                    if items else f"UTF-8 text of at most {field['cap']} bytes")
            docs.append(f"        /// {what}.\n        {label}: &'a str,\n")
            if items:
                shape = [f"{stem}_SEPARATOR", f"{stem}_MAX_ITEMS", f"{stem}_ITEM_MAX_BYTES"]
                pad = ' ' * 24
                decode.append(f"                    {label}: list(\n{pad}text(body.bytes()?, {cap})?,\n"
                              + "".join(f"{pad}{name},\n" for name in shape)
                              + "                    )?,\n")
                pad = ' ' * 16
                lists.append(f"            list(\n{pad}{label},\n"
                             + "".join(f"{pad}{name},\n" for name in shape)
                             + "            )?;\n")
            else:
                decode.append(f"                    {label}: text(body.bytes()?, {cap})?,\n")
            written.append(f"                write_blob({label}.as_bytes(), out, &mut offset);\n")
        else:
            docs.append(f"        /// At most {field['cap']} bytes.\n        {label}: &'a [u8],\n")
            decode.append(f"                    {label}: blob(body.bytes()?, {cap})?,\n")
            written.append(f"                write_blob({label}, out, &mut offset);\n")
        sized.append((label, cap))
    variants.append(f"    /// The `{name}` command (`docs/abi/registry.md`).\n"
                    f"    {variant} {{\n{''.join(docs)}    }},\n")
    arms.append(f"            {tag} => {{\n                body.bitmap()?;\n"
                f"                Command::{variant} {{\n{''.join(decode)}"
                f"                }}\n            }}\n")
    scalars = sum(_SCALAR_WIDTH.get(field['kind'], 0) for field in fields)
    fixed = f"1 + {scalars}" if scalars else "1"
    if sized:
        names = [label for label, _ in sized] + (['..'] if len(sized) < len(fields) else [])
        lens.append(_pattern('        ', variant, names) + " => {\n"
                    + "".join(lists)
                    + f"            let mut len = {fixed};\n"
                    + "".join(f"            len += blob_len({label}.len(), {cap})?;\n"
                              for label, cap in sized)
                    + f"            Ok(({tag}, len))\n        }}\n")
    else:
        lens.append(f"        Command::{variant} {{ .. }} => Ok(({tag}, {fixed})),\n")
    every = [field['name'] for field in fields]
    writes.append(_pattern('            ', variant, every) + f" => {{\n{''.join(written)}"
                  f"            }}\n")
