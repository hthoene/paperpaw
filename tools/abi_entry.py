"""Validate and emit the inactive draft plugin entry contract.

These declarations describe raw tables only. They do not read tables, call
callbacks or infer target-specific sizes, alignments or offsets.
"""
import re


_TABLES = {'PpPlugin', 'PpEntry'}
_FIELD_TYPES = {'u16', 'u32', 'pointer', 'bytes'}
_C_TYPES = {'u16': 'uint16_t', 'u32': 'uint32_t', 'i32': 'int32_t',
            'pointer': 'void *', 'bytes': 'PpBytes', 'buffer': 'PpBuffer',
            'host_ptr': 'const PpHost *', 'plugin_out': 'const PpPlugin **'}
_RUST_TYPES = {'u16': 'u16', 'u32': 'u32', 'i32': 'i32',
               'pointer': '*mut core::ffi::c_void', 'bytes': 'PpBytes', 'buffer': 'PpBuffer',
               'host_ptr': '*const PpHost', 'plugin_out': '*mut *const PpPlugin'}
_RESULTS = {'i32': ('int32_t', 'i32'), 'void': ('void', '()')}
_PREFIXES = {
    'PpPlugin': [dict(name='size', type='u32'), dict(name='state', type='pointer'),
                 dict(name='drop', callback=dict(args=[dict(name='state', type='pointer')],
                                                result='void', nullable=True))],
    'PpEntry': [dict(name='size', type='u32'), dict(name='abi_major', type='u16'),
                dict(name='needs_minor', type='u16'), dict(name='metadata', type='bytes'),
                dict(name='make', callback=dict(args=[dict(name='host', type='host_ptr'),
                                                     dict(name='host_ctx', type='pointer'),
                                                     dict(name='out', type='plugin_out')],
                                                result='i32', nullable=True))],
}
_RESERVED = set('''auto break case char const continue default do double else
enum extern float for goto if inline int long register restrict return short
signed sizeof static struct switch typedef union unsigned void volatile while
alignas alignof bool constexpr false nullptr static_assert thread_local true
typeof typeof_unqual _Alignas _Alignof _Atomic _BitInt _Bool _Complex _Decimal32
_Decimal64 _Decimal128 _Generic _Imaginary _Noreturn _Static_assert _Thread_local
abstract as async await become box crate dyn final fn gen impl in let loop macro
match mod move mut override priv pub ref self Self super trait try type unsafe
unsized use virtual where yield union macro_rules raw safe yeet'''.split())
_GENERATED = {'PpPlugin', 'PpEntry', 'PpHost', 'PpBytes', 'PpBuffer',
              'METADATA_SYMBOL', 'ENTRY_SYMBOL', 'HOST_SYMBOL',
              'PP_METADATA_SYMBOL', 'PP_ENTRY_SYMBOL', 'PP_HOST_SYMBOL',
              'PP_OK', 'PP_ABI_MAJOR', 'PP_ABI_MINOR',
              'uint16_t', 'uint32_t', 'int32_t', 'size_t', 'core'}


def _require(ok, message):
    if not ok:
        raise ValueError(message)


def _keys(value, names):
    _require(type(value) is dict and set(value) == set(names.split()),
             'entry contract keys: ' + names)


def _integer(value, low, high):
    _require(type(value) is int and low <= value <= high,
             'entry contract integer outside bounds')


def _identifier(value):
    _require(type(value) is str and re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*', value),
             'invalid entry contract identifier')
    _require(value not in _RESERVED | _GENERATED and not value.startswith(('_', 'PP_ERR_')),
             'reserved entry contract identifier')


def _known(value, choices, message):
    _require(type(value) is str and value in choices, message)


def _callback(callback):
    _keys(callback, 'args result nullable')
    _require(type(callback['nullable']) is bool, 'entry callback nullable must be bool')
    _known(callback['result'], _RESULTS, 'unknown entry callback result')
    _require(type(callback['args']) is list, 'entry callback args must be a list')
    names = set()
    for arg in callback['args']:
        _keys(arg, 'name type')
        _identifier(arg['name'])
        _require(arg['name'] not in names, 'duplicate entry callback argument')
        names.add(arg['name'])
        _known(arg['type'], _C_TYPES, 'unknown entry callback argument type')


def validate_entry(schema):
    """Validate an optional closed raw-table grammar; reject malformed values."""
    _require(type(schema) is dict, 'entry schema must be an object')
    if 'entry_contract' not in schema:
        return
    minor = schema.get('minor')
    _integer(minor, 1, 65535)
    contract = schema['entry_contract']
    _keys(contract, 'since locator entry_symbol tables')
    _integer(contract['since'], 1, minor)
    locator = contract['locator']
    _keys(locator, 'symbol kind length binding visibility')
    _require(locator['kind'] == 'readonly_bytes' and locator['length'] == 'total_bytes' and
             locator['binding'] == 'global' and locator['visibility'] == 'default',
             'unsupported entry metadata locator')
    symbols = [locator['symbol'], contract['entry_symbol']]
    for symbol in symbols:
        _identifier(symbol)
    _require(len(set(symbols)) == len(symbols), 'duplicate entry contract symbol')
    tables = contract['tables']
    _require(type(tables) is list, 'entry tables must be a list')
    names = set()
    for table in tables:
        _keys(table, 'name since fields')
        _known(table['name'], _TABLES, 'unknown entry table')
        _require(table['name'] not in names, 'duplicate entry table')
        names.add(table['name'])
        _integer(table['since'], contract['since'], minor)
        _require(type(table['fields']) is list and table['fields'],
                 'entry table fields must be a nonempty list')
        field_names = set()
        for field in table['fields']:
            _keys(field, 'name since callback' if type(field) is dict and 'callback' in field
                  else 'name since type')
            _identifier(field['name'])
            _require(field['name'] not in field_names, 'duplicate entry field')
            field_names.add(field['name'])
            _integer(field['since'], table['since'], minor)
            if 'callback' in field:
                _callback(field['callback'])
            else:
                _known(field['type'], _FIELD_TYPES, 'unknown entry field type')
        prefix = _PREFIXES[table['name']]
        _require([{k: v for k, v in field.items() if k != 'since'}
                  for field in table['fields'][:len(prefix)]] == prefix,
                 'unsupported entry table prefix')
        if table['name'] == 'PpEntry':
            # Metadata profiles freeze the exact entry size. Only PpPlugin grows.
            _require(len(table['fields']) == len(prefix), 'PpEntry fields are fixed')
    _require([table['name'] for table in tables] == ['PpPlugin', 'PpEntry'],
             'entry contract needs ordered PpPlugin and PpEntry tables')
    _require('metadata' in schema, 'entry contract needs metadata')
    if 'metadata' in schema:
        metadata = schema['metadata']
        _require(type(metadata) is dict and type(metadata.get('exports')) is list,
                 'entry contract needs metadata export declarations')
        exports = metadata['exports']
        _require(len(exports) == 1 and all(type(item) is dict for item in exports),
                 'entry contract needs one metadata export')
        _require([(item.get('name'), item.get('kind'), item.get('size_source'))
                  for item in exports] ==
                 [(contract['entry_symbol'], 1, 'entry_bytes')],
                 'entry symbols disagree with metadata exports')


def compatible_entry(old, new, minimum_since):
    """Keep symbols and PpEntry exact; permit dated PpPlugin tail fields."""
    validate_entry(old)
    validate_entry(new)
    _integer(minimum_since, 1, 65536)
    before, after = old.get('entry_contract'), new.get('entry_contract')
    if before is None:
        if after is not None:
            _require(after['since'] >= minimum_since, 'backdated entry contract')
        return
    _require(after is not None, 'removed entry contract')
    _require({k: v for k, v in before.items() if k != 'tables'} ==
             {k: v for k, v in after.items() if k != 'tables'},
             'changed entry locator or export semantics')
    prior, current = before['tables'], after['tables']
    _require(len(current) >= len(prior), 'removed entry table')
    for index, table in enumerate(prior):
        candidate = current[index]
        _require({k: v for k, v in table.items() if k != 'fields'} ==
                 {k: v for k, v in candidate.items() if k != 'fields'},
                 'changed or reordered entry table')
        fields = table['fields']
        _require(candidate['fields'][:len(fields)] == fields,
                 'changed or removed entry field')
        for field in candidate['fields'][len(fields):]:
            _require(field['since'] >= minimum_since, 'backdated appended entry field')
    for table in current[len(prior):]:
        _require(table['since'] >= minimum_since, 'backdated appended entry table')


def _symbols(contract):
    return [('METADATA_SYMBOL', contract['locator']['symbol']),
            ('ENTRY_SYMBOL', contract['entry_symbol'])]


def c_entry(schema):
    """Emit declarations after PpHost and PpBytes; emit nothing for older drafts."""
    validate_entry(schema)
    if 'entry_contract' not in schema:
        return ''
    contract = schema['entry_contract']
    lines = ['/* Raw plugin tables. Check accessible bytes before reading fields. */',
             'typedef struct PpPlugin PpPlugin;', 'typedef struct PpEntry PpEntry;']
    for name, symbol in _symbols(contract):
        lines += [f'/* Generated {name.lower()} export name. */', f'#define PP_{name} "{symbol}"']
    for table in contract['tables']:
        lines += [f'/* {table["name"]}: raw table since minor {table["since"]}. */',
                  f'struct {table["name"]} {{']
        for field in table['fields']:
            lines.append(f'    /* {field["name"]}: since minor {field["since"]}. */')
            if 'callback' in field:
                callback = field['callback']
                args = ', '.join(_C_TYPES[a['type']] + ' ' + a['name']
                                 for a in callback['args']) or 'void'
                lines.append(f'    {_RESULTS[callback["result"]][0]} (*{field["name"]})({args});')
            else:
                lines.append(f'    {_C_TYPES[field["type"]]} {field["name"]};')
        lines.append('};')
    return '\n'.join(lines) + '\n'


def rust_entry(schema):
    """Emit core-only raw declarations without imposing crate attributes."""
    validate_entry(schema)
    if 'entry_contract' not in schema:
        return ''
    contract = schema['entry_contract']
    lines = []
    for name, symbol in _symbols(contract):
        lines += [f'/// Generated {name.lower()} export name.',
                  f'pub const {name}: &str = "{symbol}";']
    for table in contract['tables']:
        lines += [f'/// Raw {table["name"]} table since minor {table["since"]}.',
                  '/// Check accessible bytes before reading fields.',
                  '#[repr(C)]', f'pub struct {table["name"]} {{']
        for field in table['fields']:
            lines.append(f'    /// {field["name"]}: since minor {field["since"]}.')
            if 'callback' in field:
                callback = field['callback']
                args = ', '.join(a['name'] + ': ' + _RUST_TYPES[a['type']]
                                 for a in callback['args'])
                kind = f'unsafe extern "C" fn({args}) -> {_RESULTS[callback["result"]][1]}'
                if callback['nullable']:
                    kind = f'Option<{kind}>'
            else:
                kind = _RUST_TYPES[field['type']]
            lines.append(f'    pub {field["name"]}: {kind},')
        lines.append('}')
    return '\n'.join(lines) + '\n'
