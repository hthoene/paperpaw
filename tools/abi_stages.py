"""Validate stage-local tag tables and emit borrowed codecs using abi_records."""
import json

from abi_services import KEY, _integer, _module_name, _require
from abi_records import local_records, local_codec_source
from abi_declarations import SCALAR_BYTES

SCHEMA_KEYS = set('key major revision since test_only max_subscribers max_input max_reply '
                  'input reply reply_rows records'.split())
FIELD_KEYS = set('name type since optional ordinal default max_bytes'.split())


def load_stages(folder):
    """Load sorted schemas, each naming its own file's key and major."""
    stages = []
    for path in sorted(folder.glob('*.def')):
        def unique(pairs):
            out = {}
            for key, value in pairs:
                _require(key not in out, f'{path.name}: duplicate JSON key {key}')
                out[key] = value
            return out
        stage = json.loads(path.read_text(), object_pairs_hook=unique)
        _require(type(stage) is dict and
                 f'{stage.get("key")}@{stage.get("major")}' == path.stem,
                 f'stage schema {path.name} must name its own key and major')
        stages.append(stage)
    return sorted(stages, key=lambda s: (str(s['key']), str(type(s['major'])), s['major']))


def _varint_len(value):
    return max(1, (value.bit_length() + 6) // 7)


def _framed_cap(record):
    optional = any(f['optional'] for f in record['fields'])
    body = 1 + int(optional)
    for field in record['fields']:
        body += SCALAR_BYTES.get(field['kind'], _varint_len(field['cap']) + field['cap'])
    return _varint_len(record['id']) + _varint_len(body) + body


def _validate_stage(stage, minor):
    _require(type(stage) is dict and set(stage) == SCHEMA_KEYS, 'stage schema keys')
    key = stage['key']
    _require(type(key) is str and KEY.fullmatch(key) and key.startswith('core.') and
             len(key.encode()) <= 128, f'stage key {key!r} must be core.* and at most 128 bytes')
    for name, low, high in [('major', 1, 65535), ('revision', 0, 65535), ('since', 1, minor),
                            ('max_subscribers', 1, 32), ('reply_rows', 1, 64),
                            ('max_input', 1, 3840), ('max_reply', 1, 3840)]:
        _integer(stage[name], low, high, f'{key}: {name} {low}..={high}')
    _require(type(stage['test_only']) is bool, f'{key}: test_only is true or false')
    records = stage['records']
    _require(type(records) is list and records, f'{key}: records must be a non-empty list')
    for record in records:
        _require(type(record) is dict and set(record) == {'id', 'name', 'since', 'fields'},
                 f'{key}: record keys')
        _integer(record['id'], 1, 511, f'{key}: record id 1..=511')
        _require(type(record['name']) is str, f'{key}: record name must be text')
        _require(type(record['fields']) is list, f'{key}: fields must be a list')
        for field in record['fields']:
            _require(type(field) is dict and set(field) in (FIELD_KEYS, FIELD_KEYS | {'list'}),
                     f'{key}: field keys')
    ids = [r['id'] for r in records]
    names = [r['name'] for r in records]
    _require(ids == sorted(set(ids)), f'{key}: unique records in ascending id order')
    _require(len(set(names)) == len(names), f'{key}: duplicate record name')
    declared = local_records(records, stage['revision'])
    for side in ('input', 'reply'):
        tags = stage[side]
        _require(type(tags) is list and 1 <= len(tags) <= 8 and
                 all(type(t) is str and t in names for t in tags) and
                 len(set(tags)) == len(tags), f'{key}: {side} needs 1..=8 distinct local records')
    _require(not set(stage['input']) & set(stage['reply']), f'{key}: record in input and reply')
    _require(set(names) == set(stage['input']) | set(stage['reply']),
             f'{key}: every record belongs to input or reply')
    for side in ('input', 'reply'):
        largest = max(_framed_cap(r) for r in declared if r['name'] in stage[side])
        # Count prefix is part of every batch, including an empty reply.
        required = 1 + largest if side == 'input' else _varint_len(stage['reply_rows']) + stage['reply_rows'] * largest
        cap = stage['max_' + side]
        _require(required <= cap, f'{key}: {side} encoded size {required} exceeds cap {cap}')


def validate_stages(schema):
    """Stage tags are local; no metadata.services catalog entry is needed."""
    stages = schema.get('stages', [])
    _require(type(stages) is list, 'stages must be a list')
    for stage in stages:
        _validate_stage(stage, schema.get('minor', 0))
    keys = [(s['key'], s['major']) for s in stages]
    _require(keys == sorted(set(keys)), 'stage schemas sorted by key and major, one per key and major')
    modules = [_module_name(*k) for k in keys]
    _require(len(set(modules)) == len(modules), 'stage module name collision')


def compatible_stages(old, new, minimum_since):
    """Published stages keep existing records/fields and grow by revision."""
    before = {(s['key'], s['major']): s for s in old.get('stages', [])}
    after = {(s['key'], s['major']): s for s in new.get('stages', [])}
    for key, stage in after.items():
        if key not in before:
            _require(stage['since'] >= minimum_since, f'{key}: backdated stage schema')
    for key, stage in before.items():
        _require(key in after, f'stage schema {key} removed')
        grown = after[key]
        for name in ('since', 'test_only'):
            _require(grown[name] == stage[name], f'{key}: {name} changed')
        for name in ('revision', 'max_subscribers', 'max_input', 'max_reply', 'reply_rows'):
            _require(grown[name] >= stage[name], f'{key}: {name} shrank')
        for side in ('input', 'reply'):
            _require(grown[side][:len(stage[side])] == stage[side], f'{key}: {side} removed or reordered')
        records = grown['records']
        _require(len(records) >= len(stage['records']), f'{key}: removed record')
        for prior, current in zip(stage['records'], records):
            _require({k:v for k,v in prior.items() if k != 'fields'} ==
                     {k:v for k,v in current.items() if k != 'fields'}, f'{key}: changed record')
            fields = prior['fields']
            _require(current['fields'][:len(fields)] == fields, f'{key}: changed record field')
            for field in current['fields'][len(fields):]:
                _require(field['optional'] and field['since'] > stage['revision'],
                         f'{key}: appended fields need next revision and optional')
        for record in records[len(stage['records']):]:
            _require(record['since'] > stage['revision'], f'{key}: new record needs next revision')


def rust_stages(schema):
    """Emit one codec per stage; older locks without stages emit no bytes."""
    validate_stages(schema)
    stages = schema.get('stages', [])
    if not stages:
        return ''
    source = '// Generated by tools/abi-gen.py; do not edit.\n'
    source += '#![allow(dead_code, reason = "shared codec helpers also cover future schema fields")]\n'
    source += TABLE_TEMPLATE
    entries = []
    for stage in stages:
        module = _module_name(stage['key'], stage['major'])
        entries.append('    StageSchema {\n' + ''.join(
            f'        {field}: {module}::{constant},\n' for field, constant in TABLE_FIELDS) + '    },\n')
    source += '\n/// Every host-defined stage schema, sorted by key and major.\npub static STAGES: &[StageSchema] = &[\n' + ''.join(entries) + '];\n'
    for stage in stages:
        source += _stage_source(stage)
    # Formatting is part of the emitted contract, independent of module nesting.
    import subprocess
    return subprocess.run(['rustfmt', '--edition', '2024', '--emit', 'stdout'],
                          input=source, text=True, check=True, capture_output=True).stdout


TABLE_FIELDS = [(name, name.upper()) for name in
                ('key', 'major', 'revision', 'test_only', 'max_subscribers', 'max_input',
                 'max_reply', 'reply_rows', 'input_tags', 'reply_tags')]
TABLE_TEMPLATE = '''/// One host stage's local tag table and resource bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct StageSchema {
    /// Stage key.
    pub key: &'static str,
    /// Major version.
    pub major: u16,
    /// Highest revision.
    pub revision: u16,
    /// Device hosts must refuse this schema.
    pub test_only: bool,
    /// Subscriber count limit.
    pub max_subscribers: usize,
    /// Input byte limit.
    pub max_input: usize,
    /// Reply byte limit per subscriber.
    pub max_reply: usize,
    /// Reply record limit per subscriber.
    pub reply_rows: usize,
    /// Input tags, local to this stage.
    pub input_tags: &'static [u32],
    /// Reply tags, local to this stage.
    pub reply_tags: &'static [u32],
}
'''


def _stage_source(stage):
    declared = local_records(stage['records'], stage['revision'])
    consts = ''
    for field, _ in TABLE_FIELDS:
        value = stage.get(field)
        kind = 'usize'
        if field == 'key': kind, value = '&str', json.dumps(value)
        elif field in ('major', 'revision'): kind = 'u16'
        elif field == 'test_only': kind, value = 'bool', str(value).lower()
        elif field.endswith('_tags'):
            names = stage[field.removesuffix('_tags')]
            kind, value = '&[u32]', '&[' + ', '.join(str(r['id']) for r in declared if r['name'] in names) + ']'
        consts += f'    /// Schema {field}.\n    pub const {field.upper()}: {kind} = {value};\n'
    source = f'\n/// Local records of `{stage["key"]}` major {stage["major"]}.\npub mod {_module_name(stage["key"], stage["major"])} {{\n' + consts
    return source + local_codec_source(stage) + '}\n'
