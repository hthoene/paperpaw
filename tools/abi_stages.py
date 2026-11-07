"""Validate stage-local tag tables and emit borrowed codecs using abi_records."""
import json

from abi_services import KEY, MAX_CALL_BYTES, _integer, _module_name, _require
from abi_records import local_records, local_codec_source
from abi_declarations import SCALAR_BYTES

SCHEMA_KEYS = set('key major revision since test_only max_subscribers max_input max_reply '
                  'input reply reply_rows records'.split())
FIELD_KEYS = set('name type since optional ordinal default max_bytes'.split())
RECORD_KEYS = {'id', 'name', 'since', 'fields'}
# A stage whose replies are a service's rows (`core.format`: the DOC rows and the
# block stream) names more reply kinds than a verdict stage.
MAX_INPUT_TAGS = 8
MAX_REPLY_TAGS = 32


def _mirrored_fields(stage, record, source):
    """The fields of the `abi.def` record a stage record mirrors, each at its stage revision.

    A field the service row had when the stage schema appeared (its `since` minor at most the
    stage's) takes the stage record's `since`. A field appended to the row later must be named
    in the record's `field_since` with the stage revision that took it, so its stamp never moves
    when the stage's revision grows again and the compatibility check sees it as appended.
    """
    later = record.get('field_since', {})
    _require(type(later) is dict and all(type(v) is int for v in later.values()) and
             set(later) <= {f['name'] for f in source['fields']},
             f'{stage.get("key")}: {record.get("name")}: field_since names fields of '
             f'{record.get("mirrors")} with a revision')
    out = []
    for field in source['fields']:
        if field['name'] in later:
            since = later[field['name']]
        else:
            _require(field['since'] <= stage.get('since', 0),
                     f'{stage.get("key")}: {record.get("name")}: {record.get("mirrors")}.'
                     f'{field["name"]} is newer than the stage schema; name the stage revision '
                     'that takes it in field_since')
            since = record['since']
        out.append(dict(field, since=since))
    return out


def expand_mirrors(stages, records):
    """Fill in each `mirrors` record's fields from the kernel record it names.

    A mirrored record is the same record as a service row under a stage-local tag:
    the host passes it on by changing only its tag (`SERVICE_TAGS`). Its fields
    are never written twice, so the two cannot drift apart.
    """
    by_name = {item.get('name'): item for item in records if type(item) is dict}
    out = []
    for stage in stages:
        grown = []
        for record in stage.get('records', []) if type(stage) is dict else []:
            if type(record) is dict and 'mirrors' in record and 'fields' not in record:
                source = by_name.get(record['mirrors'])
                _require(type(record['mirrors']) is str and record['mirrors'].startswith('service.')
                         and source is not None,
                         f'{stage.get("key")}: {record.get("name")} mirrors an abi.def service record')
                record = dict(record, fields=_mirrored_fields(stage, record, source))
            grown.append(record)
        out.append(dict(stage, records=grown) if type(stage) is dict else stage)
    return out


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
    optional = sum(f['optional'] for f in record['fields'])
    # The shared field validator currently permits eight bits. Keep the size
    # calculation expressed in bytes instead of assuming a one-byte bitmap.
    bitmap_bytes = (optional + 7) // 8
    body = _varint_len(bitmap_bytes) + bitmap_bytes
    for field in record['fields']:
        body += SCALAR_BYTES.get(field['kind'], _varint_len(field['cap']) + field['cap'])
    return _varint_len(record['id']) + _varint_len(body) + body


def _validate_stage(stage, minor, plugin=False):
    """A host stage (`core.*`, `since` at most the interface minor) or, with `plugin`, a
    plugin-defined one (`<plugin>.<name>`, never `core.*`, `since` its own schema version)."""
    _require(type(stage) is dict and set(stage) == SCHEMA_KEYS, 'stage schema keys')
    key = stage['key']
    owned = type(key) is str and key.startswith('core.') != plugin
    _require(type(key) is str and KEY.fullmatch(key) and owned and len(key.encode()) <= 128,
             f'stage key {key!r} must be {"<plugin>.<name>, not core.*" if plugin else "core.*"}'
             ' and at most 128 bytes')
    for name, low, high in [('major', 1, 65535), ('revision', 0, 65535), ('since', 1, minor),
                            ('max_subscribers', 1, 32), ('reply_rows', 1, 64),
                            ('max_input', 1, 3840), ('max_reply', 1, 3840)]:
        _integer(stage[name], low, high, f'{key}: {name} {low}..={high}')
    _require(type(stage['test_only']) is bool, f'{key}: test_only is true or false')
    records = stage['records']
    _require(type(records) is list and records, f'{key}: records must be a non-empty list')
    for record in records:
        _require(type(record) is dict and set(record) in (
                     RECORD_KEYS, RECORD_KEYS | {'mirrors'}, RECORD_KEYS | {'mirrors', 'field_since'}),
                 f'{key}: record keys')
        # Mirrors copy host service rows; a plugin's schema publishes its own records.
        _require(not plugin or 'mirrors' not in record, f'{key}: a plugin stage mirrors no row')
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
    for side, most in (('input', MAX_INPUT_TAGS), ('reply', MAX_REPLY_TAGS)):
        tags = stage[side]
        _require(type(tags) is list and 1 <= len(tags) <= most and
                 all(type(t) is str and t in names for t in tags) and
                 len(set(tags)) == len(tags),
                 f'{key}: {side} needs 1..={most} distinct local records')
    _require(not set(stage['input']) & set(stage['reply']), f'{key}: record in input and reply')
    _require(set(names) == set(stage['input']) | set(stage['reply']),
             f'{key}: every record belongs to input or reply')
    for side in ('input', 'reply'):
        largest = max(_framed_cap(r) for r in declared if r['name'] in stage[side])
        # Count prefix is part of every batch, including an empty reply.
        required = 1 + largest
        cap = stage['max_' + side]
        _require(required <= cap, f'{key}: {side} encoded size {required} exceeds cap {cap}')


def validate_stages(schema):
    """Stage tags are local; no metadata.services catalog entry is needed."""
    stages = schema.get('stages', [])
    _require(type(stages) is list, 'stages must be a list')
    kernel = {item.get('name'): item for item in schema.get('records', []) if type(item) is dict}
    for stage in stages:
        _validate_stage(stage, schema.get('minor', 0))
        for record in stage['records']:
            if 'mirrors' in record:
                source = kernel.get(record['mirrors'])
                _require(record['mirrors'].startswith('service.') and source is not None and
                         record['fields'] == _mirrored_fields(stage, record, source),
                         f'{stage["key"]}: {record["name"]} must hold exactly the fields of '
                         f'{record["mirrors"]}')
        # Retagging a mirrored row changes only its tag's length: the whole stage reply,
        # each row grown by that, must still fit a service reply (MAX_CALL_BYTES).
        growth = max((_varint_len(kernel[r['mirrors']]['id']) - _varint_len(r['id'])
                      for r in stage['records'] if 'mirrors' in r), default=0)
        retagged = stage['max_reply'] + stage['reply_rows'] * max(growth, 0)
        _require(retagged <= MAX_CALL_BYTES,
                 f'{stage["key"]}: a retagged reply of {retagged} bytes exceeds the service '
                 f'reply of {MAX_CALL_BYTES}')
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
            # A mirror's `field_since` only grows with the fields it stamps, which the
            # field check below compares.
            _require({k:v for k,v in prior.items() if k not in ('fields', 'field_since')} ==
                     {k:v for k,v in current.items() if k not in ('fields', 'field_since')},
                     f'{key}: changed record')
            fields = prior['fields']
            _require(current['fields'][:len(fields)] == fields, f'{key}: changed record field')
            for field in current['fields'][len(fields):]:
                _require(field['optional'] and field['since'] > stage['revision'],
                         f'{key}: appended fields need next revision and optional')
        for record in records[len(stage['records']):]:
            _require(record['since'] > stage['revision'], f'{key}: new record needs next revision')


def source_stages(schema):
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
    kernel = {item['name']: item['id'] for item in schema.get('records', [])}
    for stage in stages:
        source += _stage_source(stage, kernel)
    return source


def plugin_stages_source(stages):
    """One codec module per plugin-defined stage schema (`stages-services.md` section 3: the
    stage's owner publishes it; the host checks only framing), through the same validators and
    record emitters as the host's stages, plus the same `STAGES` table. No lock, no digest:
    the owning crate keeps the output and checks it with `tools/stage-codec.py --check`."""
    _require(type(stages) is list and stages, 'plugin stage schemas: at least one')
    for stage in stages:
        _validate_stage(stage, 65535, plugin=True)
    keys = [(s['key'], s['major']) for s in stages]
    _require(keys == sorted(set(keys)), 'stage schemas sorted by key and major, one per key and major')
    modules = [_module_name(*k) for k in keys]
    _require(len(set(modules)) == len(modules), 'stage module name collision')
    source = '// Generated by tools/stage-codec.py; do not edit.\n'
    source += '#![allow(dead_code, reason = "shared codec helpers also cover future schema fields")]\n'
    source += TABLE_TEMPLATE.replace('One host stage', 'One stage')
    entries = ''.join('    StageSchema {\n' + ''.join(
        f'        {field}: {module}::{constant},\n' for field, constant in TABLE_FIELDS) + '    },\n'
        for module in modules)
    source += ('\n/// Every stage schema of this crate, sorted by key and major.\n'
               f'pub static STAGES: &[StageSchema] = &[\n{entries}];\n')
    return source + ''.join(_stage_source(stage, {}) for stage in stages)


def rustfmt(source):
    import subprocess
    return subprocess.run(['rustfmt', '--edition', '2024', '--emit', 'stdout'],
                          input=source, text=True, check=True, capture_output=True).stdout


def rust_stages(schema):
    """Formatted copies, checked independently of the frozen lock digest."""
    source = source_stages(schema)
    return rustfmt(source) if source else ''


def stages_digest(schema):
    """Pin emitter semantics before formatting, independent of rustfmt changes."""
    import hashlib
    return hashlib.sha256(source_stages(schema).encode()).hexdigest()


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


def _stage_source(stage, kernel):
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
    mirrored = [(r['id'], r['mirrors']) for r in stage['records'] if 'mirrors' in r]
    if mirrored:
        pairs = ', '.join(f'({local}, {kernel[name]})' for local, name in mirrored)
        consts += ('    /// Each mirrored record: its local tag and the kernel tag of the service row\n'
                   '    /// it is the same record as (only the tag changes).\n'
                   f'    pub const SERVICE_TAGS: &[(u32, u32)] = &[{pairs}];\n')
    source = f'\n/// Local records of `{stage["key"]}` major {stage["major"]}.\npub mod {_module_name(stage["key"], stage["major"])} {{\n' + consts
    return source + local_codec_source(stage) + '}\n'
