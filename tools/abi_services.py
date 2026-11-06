"""Validate service schemas (`abi/services/<key>.def`) and emit their op tables.

A service schema names a call service's ops: their ids, the `service.*`
records of `abi.def` each takes and answers, their bounds and the feature
that gates them (`docs/abi/stages-services.md` section 2.3). The records
themselves are kernel records in the generated records codec; this module
emits only the static table the host dispatcher and the SDK share.
"""
import json
import re

KEY = re.compile(r'[a-z][a-z0-9]*(-[a-z0-9]+)*(\.[a-z][a-z0-9]*(-[a-z0-9]+)*)+')
OP_NAME = re.compile(r'[a-z][a-z0-9]*(_[a-z0-9]+)*')
SCHEMA_KEYS = {'key', 'major', 'revision', 'since', 'ops'}
OP_KEYS = {'id', 'name', 'since', 'effect', 'feature', 'input', 'reply', 'max_input',
           'max_reply', 'bound'}
EFFECTS = ('query', 'mutate')
# What a call may wait for. `radio` work is never a call: it is an operation handle.
BOUNDS = ('none', 'card')
MAX_KEY_BYTES = 128
MAX_OPS = 64
MAX_TAGS = 8
MAX_CALL_BYTES = 4096


def _require(ok, message):
    if not ok:
        raise ValueError(message)


def _integer(value, low, high, message):
    _require(type(value) is int and low <= value <= high, message)


def load_services(folder):
    """Every schema of `folder`, sorted by key; the file name is the key."""
    if not folder.is_dir():
        return []
    services = []
    for path in sorted(folder.glob('*.def')):
        with path.open(encoding='utf-8') as handle:
            service = json.load(handle)
        _require(type(service) is dict and service.get('key') == path.stem,
                 f'service schema {path.name} must name its own key')
        services.append(service)
    return services


def _validate_op(op, service, records, features):
    name = service['key']
    _require(type(op) is dict and set(op) == OP_KEYS, f'{name}: op keys')
    _integer(op['id'], 1, 65535, f'{name}: op id 1..=65535')
    _require(type(op['name']) is str and OP_NAME.fullmatch(op['name']) and
             len(op['name']) <= 64, f'{name}: op name')
    _integer(op['since'], 0, service['revision'], f'{name}.{op["name"]}: since within revision')
    _require(op['effect'] in EFFECTS, f'{name}.{op["name"]}: effect')
    _require(op['bound'] in BOUNDS, f'{name}.{op["name"]}: bound is none or card')
    _require(op['feature'] is None or op['feature'] in features,
             f'{name}.{op["name"]}: feature must be a metadata feature key or null')
    for side in ('input', 'reply'):
        tags = op[side]
        _require(type(tags) is list and len(tags) <= MAX_TAGS and
                 len(set(map(str, tags))) == len(tags),
                 f'{name}.{op["name"]}: {side} lists at most {MAX_TAGS} distinct records')
        for record in tags:
            _require(type(record) is str and record.startswith('service.') and record in records,
                     f'{name}.{op["name"]}: {side} names an abi.def service record')
    for bound in ('max_input', 'max_reply'):
        _integer(op[bound], 1, MAX_CALL_BYTES, f'{name}.{op["name"]}: {bound} 1..=4096')


def validate_services(schema):
    """Every schema well formed, in the metadata catalog, ops unique."""
    services = schema.get('services')
    if services is None:
        return
    _require(type(services) is list, 'services must be a list')
    records = {item.get('name') for item in schema.get('records', []) if type(item) is dict}
    metadata = schema.get('metadata', {})
    catalog = {(item.get('key'), item.get('major')): item
               for item in metadata.get('services', []) if type(item) is dict}
    features = {item.get('key') for item in metadata.get('features', []) if type(item) is dict}
    keys = [item.get('key') for item in services if type(item) is dict]
    _require(keys == sorted(keys) and len(set(keys)) == len(keys),
             'service schemas sorted by key, one per key')
    for service in services:
        _require(type(service) is dict and set(service) == SCHEMA_KEYS, 'service schema keys')
        key = service['key']
        _require(type(key) is str and len(key.encode()) <= MAX_KEY_BYTES and KEY.fullmatch(key),
                 f'service key {key!r}')
        _integer(service['major'], 1, 65535, f'{key}: major 1..=65535')
        _integer(service['revision'], 0, 65535, f'{key}: revision 0..=65535')
        _integer(service['since'], 1, schema.get('minor', 0), f'{key}: since within schema minor')
        entry = catalog.get((key, service['major']))
        _require(entry is not None and entry.get('semantic_revision') == service['revision'],
                 f'{key}: needs a metadata catalog entry of the same major and revision')
        ops = service['ops']
        _require(type(ops) is list and 1 <= len(ops) <= MAX_OPS, f'{key}: 1..={MAX_OPS} ops')
        for op in ops:
            _validate_op(op, service, records, features)
        _require(len({op['id'] for op in ops}) == len(ops), f'{key}: duplicate op id')
        _require(len({op['name'] for op in ops}) == len(ops), f'{key}: duplicate op name')
        _require([op['id'] for op in ops] == sorted(op['id'] for op in ops),
                 f'{key}: ops in id order')


def compatible_services(old, new):
    """A published schema only grows: same major keeps every op exactly."""
    before = {item['key']: item for item in old.get('services', [])}
    after = {item['key']: item for item in new.get('services', [])}
    for key, service in before.items():
        _require(key in after, f'service schema {key} removed')
        grown = after[key]
        _require(grown['major'] == service['major'], f'{key}: a new major is a new schema')
        _require(grown['since'] == service['since'], f'{key}: since changed')
        _require(grown['revision'] >= service['revision'], f'{key}: revision went back')
        ops = {op['id']: op for op in grown['ops']}
        for op in service['ops']:
            _require(ops.get(op['id']) == op, f'{key}: op {op["id"]} changed or removed')
        for op in grown['ops']:
            if op['id'] not in {item['id'] for item in service['ops']}:
                _require(op['since'] > service['revision'],
                         f'{key}: new op {op["name"]} needs the next revision')


def _module_name(key):
    return key.replace('.', '_').replace('-', '_')


def _const_name(name):
    return name.upper()


def _tags(schema, names):
    records = {item['name']: item['id'] for item in schema['records']}
    return '&[' + ', '.join(str(records[name]) for name in names) + ']'


TEMPLATE = '''// Generated by tools/abi-gen.py; do not edit.
/// What a service call may wait for before it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceBound {
    /// Nothing outside the host's memory.
    None,
    /// One bounded card access.
    Card,
}

/// One operation of a service schema (`abi/services/<key>.def`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceOp {
    /// Operation number, never renumbered.
    pub id: u32,
    /// Operation name.
    pub name: &'static str,
    /// Service revision that introduced it.
    pub since: u16,
    /// Whether it changes host state.
    pub mutates: bool,
    /// Metadata feature key that gates it, if any.
    pub feature: Option<&'static str>,
    /// Record tags its input batch may hold, exactly.
    pub input: &'static [u32],
    /// Record tags its reply batch may hold, exactly.
    pub reply: &'static [u32],
    /// Largest input batch in bytes.
    pub max_input: usize,
    /// Largest reply batch in bytes; the caller's output must hold it.
    pub max_reply: usize,
    /// What the call may wait for.
    pub bound: ServiceBound,
}

/// One call service: its key, version and ops in id order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceSchema {
    /// Service key, as declared in plugin metadata.
    pub key: &'static str,
    /// Major version.
    pub major: u16,
    /// Highest revision of this major.
    pub revision: u16,
    /// Operations in id order.
    pub ops: &'static [ServiceOp],
}

impl ServiceSchema {
    /// The operation `id`, if this schema defines it.
    pub fn op(&self, id: u32) -> Option<&'static ServiceOp> {
        let ops: &'static [ServiceOp] = self.ops;
        ops.iter().find(|op| op.id == id)
    }
}

/// The schema of `key` at `major`, if the interface defines it.
pub fn service_schema(key: &str, major: u16) -> Option<&'static ServiceSchema> {
    SERVICES
        .iter()
        .find(|schema| schema.key == key && schema.major == major)
}

/// Every call service schema, sorted by key.
pub static SERVICES: &[ServiceSchema] = &[
@SCHEMAS@];
@MODULES@'''


def _op_source(schema, op):
    feature = 'None' if op['feature'] is None else f'Some("{op["feature"]}")'
    return (f'            ServiceOp {{\n'
            f'                id: {op["id"]},\n'
            f'                name: "{op["name"]}",\n'
            f'                since: {op["since"]},\n'
            f'                mutates: {"true" if op["effect"] == "mutate" else "false"},\n'
            f'                feature: {feature},\n'
            f'                input: {_tags(schema, op["input"])},\n'
            f'                reply: {_tags(schema, op["reply"])},\n'
            f'                max_input: {op["max_input"]},\n'
            f'                max_reply: {op["max_reply"]},\n'
            f'                bound: ServiceBound::{op["bound"].capitalize()},\n'
            f'            }},\n')


def rust_services(schema):
    """The shared op tables; empty where the schema has no services."""
    validate_services(schema)
    services = schema.get('services') or []
    if not services:
        return ''
    schemas, modules = [], []
    for service in services:
        ops = ''.join(_op_source(schema, op) for op in service['ops'])
        schemas.append(f'    ServiceSchema {{\n'
                       f'        key: "{service["key"]}",\n'
                       f'        major: {service["major"]},\n'
                       f'        revision: {service["revision"]},\n'
                       f'        ops: &[\n{ops}        ],\n'
                       f'    }},\n')
        consts = ''.join(f'    /// Operation `{op["name"]}`.\n'
                         f'    pub const OP_{_const_name(op["name"])}: u32 = {op["id"]};\n'
                         for op in service['ops'])
        modules.append(f'\n/// Constants of `{service["key"]}`.\n'
                       f'pub mod {_module_name(service["key"])} {{\n'
                       f'    /// Service key.\n'
                       f'    pub const KEY: &str = "{service["key"]}";\n'
                       f'    /// Major version.\n'
                       f'    pub const MAJOR: u16 = {service["major"]};\n'
                       f'    /// Highest revision.\n'
                       f'    pub const REVISION: u16 = {service["revision"]};\n'
                       f'{consts}}}\n')
    return TEMPLATE.replace('@SCHEMAS@', ''.join(schemas)).replace('@MODULES@', ''.join(modules))
