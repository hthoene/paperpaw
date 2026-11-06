#!/usr/bin/env python3
"""Validate the draft ABI schema and generate deterministic M1 declarations.

This draft generates raw declarations and the bounded settings wire codec.
A checked-in lock is independent of normal generation; updating it is explicit.
"""
import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from abi_metadata_schema import compatible_metadata, resolved_metadata, validate_metadata
from abi_entry import validate_entry, compatible_entry, c_entry, rust_entry
from abi_settings import validate_settings, compatible_settings, rust_settings
from abi_callbacks import rust_callbacks
from abi_records import validate_records, compatible_records, rust_records
from abi_snapshot import validate_snapshot, compatible_snapshot, rust_snapshot
from abi_color import validate_color, compatible_color, rust_color
TYPES = {'context': 'void *', 'pointer': 'void *', 'bytes': 'PpBytes',
         'buffer': 'PpBuffer', 'handle': 'uint32_t', 'usize': 'size_t',
         'u16': 'uint16_t', 'u32': 'uint32_t', 'u64': 'uint64_t',
         'i32': 'int32_t', 'f32': 'float', 'f64': 'double',
         'out_ptr': 'void **', 'out_u64': 'uint64_t *',
         'out_handle': 'uint32_t *', 'out_f32': 'float *'}
WIRE = {'u32', 'u64', 'i32', 'str', 'bytes'}
C_RESERVED = set('auto break case char const continue default do double else enum extern float for goto if inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void volatile while alignas alignof bool constexpr false nullptr static_assert thread_local true typeof typeof_unqual'.split())
C_RESERVED.update('abstract as async await become box crate dyn final fn gen impl in let loop macro match mod move mut override priv pub ref self Self super trait try type unsafe unsized use virtual where yield'.split())
HOST_PREFIX = {'size', 'major', 'minor'}
C_RESERVED.update(re.findall(r'[A-Za-z_][A-Za-z_0-9]*', ' '.join(TYPES.values())))


def require(ok, message):
    if not ok:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f'duplicate JSON key: {key}')
        result[key] = value
    return result


def load(path):
    def invalid(value):
        raise ValueError(f'nonfinite JSON number: {value}')
    return json.loads(path.read_text(), object_pairs_hook=unique_object,
                      parse_constant=invalid)


def canonical(value):
    return json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True) + '\n'


def keys(obj, expected):
    require(type(obj) is dict and set(obj) == set(expected.split()),
            f'expected keys: {expected}')


def integer(value, maximum, minimum=0):
    require(type(value) is int and minimum <= value <= maximum,
            f'integer outside {minimum}..{maximum}: {value!r}')


def name(value, dotted=False):
    pattern = r'[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*' if dotted else r'[a-z][a-z0-9_]*'
    require(type(value) is str and re.fullmatch(pattern, value), 'invalid name')


def declarations(items, minor):
    require(type(items) is list, 'declarations must be a list')
    ids, names = set(), set()
    for item in items:
        require(type(item) is dict, 'declaration must be an object')
        integer(item.get('id'), 65535)
        name(item.get('name'), True)
        integer(item.get('since'), minor, 1)
        require(item['id'] not in ids and item['name'] not in names,
                'duplicate declaration id/name')
        ids.add(item['id'])
        names.add(item['name'])


def validate(schema):
    keys(schema, 'format major minor host_entries records' +
         (' statuses' if 'statuses' in schema else '') +
         (' metadata' if 'metadata' in schema else '') +
         (' entry_contract' if 'entry_contract' in schema else '') +
         (' settings_value' if 'settings_value' in schema else '') +
         (' record_codec' if 'record_codec' in schema else '') +
         (' snapshot_value' if 'snapshot_value' in schema else '') +
         (' color_value' if 'color_value' in schema else ''))
    require(type(schema['format']) is int and schema['format'] == 1,
            'unsupported schema format')
    integer(schema['major'], 65535, 1)
    integer(schema['minor'], 65535, 1)
    validate_settings(schema)
    validate_snapshot(schema)
    validate_color(schema)
    if 'entry_contract' in schema:
        validate_entry(schema)
    if 'metadata' in schema:
        validate_metadata(schema)
    if 'statuses' in schema:
        validate_statuses(schema['statuses'], schema['minor'])
    rust_callbacks(schema)  # Validate any recognized tail signatures before output.
    for kind in ('host_entries', 'records'):
        declarations(schema[kind], schema['minor'])
    c_names = set()
    for entry in schema['host_entries']:
        keys(entry, 'id name since args result effect ownership reentry max_bytes')
        cname = entry['name'].replace('.', '_')
        require(cname not in C_RESERVED | HOST_PREFIX, 'reserved C host member')
        require(cname not in c_names, 'colliding generated C names')
        c_names.add(cname)
        require(entry['result'] in ('status', 'length', 'count'), 'invalid result')
        require(entry['effect'] in ('query', 'consume', 'mutate'), 'invalid effect')
        require(entry['ownership'] in ('call', 'owner', 'release'), 'invalid ownership')
        require(entry['reentry'] == 'deny', 'only non-reentrant M1 entries supported')
        require(entry['result'] != 'length' or entry['effect'] == 'query',
                'length-query retry may not consume or mutate')
        integer(entry['max_bytes'], 65536)
        require(type(entry['args']) is list, 'args must be a list')
        names = set()
        for arg in entry['args']:
            keys(arg, 'name type')
            name(arg['name'])
            require(arg['name'] not in C_RESERVED, 'reserved C argument')
            require(arg['name'] not in names, 'duplicate argument name')
            names.add(arg['name'])
            require(arg['type'] in TYPES, 'unknown argument type')
        if any(a['type'] in ('bytes', 'buffer') for a in entry['args']):
            require(entry['max_bytes'] > 0, 'buffer entries need a positive cap')
    for record in schema['records']:
        validate_record(record, schema['minor'])
    validate_records(schema)
    return schema


def validate_statuses(statuses, minor):
    require(type(statuses) is list and statuses, 'statuses must be a nonempty list')
    names, values = set(), set()
    for item in statuses:
        keys(item, 'name value since')
        label = item['name']
        require(type(label) is str and re.fullmatch(r'OK|ERR_[A-Z][A-Z_0-9]*', label),
                'invalid status name')
        integer(item['value'], 0, -(2**31) + 1)
        integer(item['since'], minor, 1)
        require((label == 'OK') == (item['value'] == 0), 'only OK is zero')
        require(label not in names and item['value'] not in values, 'duplicate status')
        names.add(label)
        values.add(item['value'])
    require('OK' in names, 'missing OK status')


def validate_record(record, minor):
    keys(record, 'id name since fields')
    require(type(record['fields']) is list, 'fields must be a list')
    names, ordinals = set(), set()
    for field in record['fields']:
        keys(field, 'name type since optional ordinal default max_bytes')
        name(field['name'])
        require(field['name'] not in names, 'duplicate field name')
        names.add(field['name'])
        require(field['type'] in WIRE, 'invalid wire type')
        integer(field['since'], minor, record['since'])
        require(type(field['optional']) is bool, 'optional must be bool')
        integer(field['max_bytes'], 65536)
        if field['type'] in ('str', 'bytes'):
            require(field['max_bytes'] > 0, 'wire string/blob needs cap')
        if not field['optional']:
            require(field['ordinal'] is None and field['default'] is None,
                    'required field has no optional ordinal/default')
            continue
        integer(field['ordinal'], 255)
        require(field['ordinal'] not in ordinals, 'duplicate optional ordinal')
        ordinals.add(field['ordinal'])
        default, kind = field['default'], field['type']
        if kind == 'u32':
            integer(default, 2**32 - 1)
        elif kind == 'u64':
            integer(default, 2**64 - 1)
        elif kind == 'i32':
            integer(default, 2**31 - 1, -(2**31))
        else:
            require(default == '' and type(default) is str,
                    'M1 strings/blobs only support an empty default')


def compatible(old, new, released_minor=None):
    validate(old)
    validate(new)
    if released_minor is None:
        released_minor = old['minor']
    integer(released_minor, old['minor'])
    minimum_since = max(old['minor'], released_minor + 1)
    compatible_metadata(old, new, minimum_since)
    compatible_entry(old, new, minimum_since)
    compatible_settings(old, new, minimum_since)
    compatible_records(old, new, minimum_since)
    compatible_snapshot(old, new, minimum_since)
    compatible_color(old, new, minimum_since)
    require(old['format'] == new['format'] and old['major'] == new['major'],
            'format/major change needs separate reviewed baseline')
    require(old['minor'] <= new['minor'] <= old['minor'] + 1,
            'minor must stay or advance by one')
    prior_statuses, current_statuses = old.get('statuses', []), new.get('statuses', [])
    require(current_statuses[:len(prior_statuses)] == prior_statuses, 'changed/removed status')
    for item in current_statuses[len(prior_statuses):]:
        require(item['since'] >= minimum_since, 'backdated status')
    for kind in ('host_entries', 'records'):
        prior, current = old[kind], new[kind]
        require(len(current) >= len(prior), 'removed declaration')
        for index, before in enumerate(prior):
            after = current[index]
            if kind == 'host_entries':
                require(before == after, 'changed/reordered host entry')
            else:
                require({k: v for k, v in before.items() if k != 'fields'} ==
                        {k: v for k, v in after.items() if k != 'fields'},
                        'changed/reordered record')
                fields = before['fields']
                require(after['fields'][:len(fields)] == fields, 'changed record field')
                for field in after['fields'][len(fields):]:
                    require(field['optional'], 'appended required field')
                    require(field['since'] >= minimum_since, 'backdated appended field')
        for item in current[len(prior):]:
            require(item['since'] >= minimum_since, 'backdated appended declaration')


def header(schema):
    lines = ['// Generated by tools/abi-gen.py; do not edit.',
             '// Draft M1 declarations only; not a frozen ABI.',
             '#ifndef PAPERPAW_ABI_H', '#define PAPERPAW_ABI_H',
             '#include <stddef.h>', '#include <stdint.h>',
             'typedef struct { const uint8_t *ptr; size_t len; } PpBytes;',
             'typedef struct { uint8_t *ptr; size_t cap; } PpBuffer;',
             'typedef struct {', '    uint32_t size;', '    uint16_t major;',
             '    uint16_t minor;']
    for entry in schema['host_entries']:
        args = ', '.join(TYPES[a['type']] + ' ' + a['name'] for a in entry['args']) or 'void'
        lines.append(f"    int32_t (*{entry['name'].replace('.', '_')})({args});")
    lines += ['} PpHost;']
    if 'statuses' in schema:
        lines += [f"#define PP_ABI_MAJOR {schema['major']}",
                  f"#define PP_ABI_MINOR {schema['minor']}"]
        lines += [f"#define PP_{x['name']} ({x['value']})" for x in schema['statuses']]
    if 'entry_contract' in schema:
        lines += [c_entry(schema)]
    lines += ['#endif', '']
    return '\n'.join(lines)


def rust_module(schema):
    """Raw declarations only. Safe prefix access and callbacks come separately."""
    scalar = {'uint16_t': 'u16', 'uint32_t': 'u32', 'uint64_t': 'u64',
              'int32_t': 'i32', 'size_t': 'usize', 'float': 'f32', 'double': 'f64',
              'void *': '*mut core::ffi::c_void',
              'void **': '*mut *mut core::ffi::c_void',
              'uint64_t *': '*mut u64', 'uint32_t *': '*mut u32',
              'float *': '*mut f32', 'PpBytes': 'PpBytes', 'PpBuffer': 'PpBuffer'}
    lines = ['// Generated by tools/abi-gen.py; do not edit.',
             '// Draft raw M1 types; no validation or safe table access is implied.',
             '/// Bytes borrowed only for the declared call or lease lifetime.',
             '#[repr(C)]', '#[derive(Clone, Copy)]', 'pub struct PpBytes {',
             '    /// Caller-owned readable storage.', '    pub ptr: *const u8,',
             '    /// Accessible bytes; null is permitted only with zero length.',
             '    pub len: usize,', '}',
             '/// Caller-owned writable output storage.', '#[repr(C)]',
             '#[derive(Clone, Copy)]', 'pub struct PpBuffer {',
             '    /// Valid writable storage, disjoint from inputs unless specified.',
             '    pub ptr: *mut u8,', '    /// Accessible output capacity in bytes.',
             '    pub cap: usize,', '}',
             '/// Full current host layout. Do not dereference a short table as this type.',
             '#[repr(C)]', 'pub struct PpHost {',
             '    /// Accessible table bytes.', '    pub size: u32,',
             '    /// ABI major.', '    pub major: u16,',
             '    /// Highest implemented minor.', '    pub minor: u16,']
    for entry in schema['host_entries']:
        args = ', '.join(a['name'] + ': ' + scalar[TYPES[a['type']]] for a in entry['args'])
        lines += [f"    /// {entry['name']}: {entry['effect']}, {entry['result']}, since {entry['since']}.",
                  f"    pub {entry['name'].replace('.', '_')}: unsafe extern \"C\" fn({args}) -> i32,"]
    lines += ['}']
    if 'statuses' in schema:
        lines += ['/// Generated ABI major.', f"pub const MAJOR: u16 = {schema['major']};",
                  '/// Generated ABI minor.', f"pub const MINOR: u16 = {schema['minor']};"]
        for item in schema['statuses']:
            lines += [f"/// {item['name']}: status value since minor {item['since']}.",
                      f"pub const {item['name']}: i32 = {item['value']};"]
    if 'entry_contract' in schema:
        lines += [rust_entry(schema).rstrip('\n')]
    if 'settings_value' in schema:
        lines += [rust_settings(schema).rstrip('\n')]
    readers = rust_callbacks(schema)
    if readers:
        lines += [readers.rstrip('\n')]
    lines += ['']
    return '\n'.join(lines)


def lock_record(schema, released_minor=0):
    """Keep exact emitted declarations as well as logical schema semantics."""
    return {'lock_format': 6, 'released_minor': released_minor,
            'schema': schema, 'emitted_c': header(schema),
            'emitted_rust': rust_module(schema),
            'emitted_metadata': resolved_metadata(schema),
            'emitted_records': rust_records(schema),
            'emitted_snapshot': rust_snapshot(schema),
            'emitted_color': rust_color(schema)}


def check_lock(prior, schema):
    require(type(prior) is dict, 'lock must be an object')
    version = prior.get('lock_format')
    require(type(version) is int and version in (1, 2, 3, 4, 5, 6), 'unsupported lock format')
    keys(prior, 'lock_format released_minor schema emitted_c' +
         (' emitted_rust' if version >= 2 else '') +
         (' emitted_metadata' if version >= 3 else '') +
         (' emitted_records' if version >= 4 else '') +
         (' emitted_snapshot' if version >= 5 else '') +
         (' emitted_color' if version >= 6 else ''))
    validate(prior['schema'])
    integer(prior['released_minor'], prior['schema']['minor'])
    # Re-render the OLD schema using THIS generator. This detects changes in
    # type mappings, prefix/view structs, result types and any header template,
    # even when both candidate schema and candidate lock were regenerated.
    require(prior['emitted_c'] == header(prior['schema']),
            'emitted C contract changed from prior lock')
    if version >= 2:
        require(prior['emitted_rust'] == rust_module(prior['schema']),
                'emitted Rust contract changed from prior lock')
    if version >= 3:
        require(prior['emitted_metadata'] == resolved_metadata(prior['schema']),
                'emitted metadata layout changed from prior lock')
    if version >= 4:
        require(prior['emitted_records'] == rust_records(prior['schema']),
                'emitted records contract changed from prior lock')
    if version >= 5:
        require(prior['emitted_snapshot'] == rust_snapshot(prior['schema']),
                'emitted snapshot contract changed from prior lock')
    if version >= 6:
        require(prior['emitted_color'] == rust_color(prior['schema']),
                'emitted color contract changed from prior lock')
    compatible(prior['schema'], schema, prior['released_minor'])


def run(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--write-lock', action='store_true')
    parser.add_argument('--baseline', type=Path)
    args = parser.parse_args(argv)
    require(not (args.check and args.write_lock), 'check never writes the lock')
    folder = args.root / 'abi'
    schema = validate(load(folder / 'abi.def'))
    lock = folder / 'abi.lock'
    release_floor = 0
    if args.baseline:
        baseline = load(args.baseline)
        check_lock(baseline, schema)
        release_floor = baseline['released_minor']
    released_minor = release_floor
    if lock.exists():
        prior = load(lock)
        check_lock(prior, schema)
        released_minor = prior['released_minor']
        require(released_minor >= release_floor,
                'candidate release boundary is below trusted baseline')
    elif not args.write_lock:
        raise ValueError('missing lock; explicit --write-lock required')
    outputs = {folder / 'abi.json': canonical(schema),
               folder / 'generated' / 'paperpaw-abi.h': header(schema),
               folder / 'generated' / 'paperpaw_abi.rs': rust_module(schema),
               args.root / 'crates/paperpaw-native/src/host_glue/paperpaw_abi.rs': rust_module(schema)}
    if 'metadata' in schema:
        outputs[folder / 'generated' / 'metadata-layout.json'] = canonical(resolved_metadata(schema))
    if 'settings_value' in schema:
        codec = rust_settings(schema)
        outputs[folder / 'generated' / 'settings_value.rs'] = codec
        outputs[args.root / 'crates/paperpaw-core/src/abi7/settings_wire.rs'] = codec
    codec = rust_records(schema)
    if codec:
        outputs[folder / 'generated' / 'records_wire.rs'] = codec
        outputs[args.root / 'crates/paperpaw-core/src/abi7/records_wire.rs'] = codec
    codec = rust_snapshot(schema)
    if codec:
        outputs[folder / 'generated' / 'snapshot_wire.rs'] = codec
        outputs[args.root / 'crates/paperpaw-core/src/abi7/snapshot_wire.rs'] = codec
    codec = rust_color(schema)
    if codec:
        outputs[folder / 'generated' / 'color_wire.rs'] = codec
        outputs[args.root / 'crates/paperpaw-core/src/abi7/color_wire.rs'] = codec
    if args.check:
        require(load(lock) == lock_record(schema, released_minor), 'lock stale; explicitly review/update it')
        for path, wanted in outputs.items():
            require(path.exists() and path.read_text() == wanted, f'stale generated file: {path}')
    else:
        for path, content in outputs.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        if args.write_lock:
            lock.write_text(canonical(lock_record(schema, released_minor)))
    return 0


if __name__ == '__main__':
    try:
        sys.exit(run())
    except (ValueError, OSError, TypeError, KeyError) as error:
        print(f'abi-gen: {error}', file=sys.stderr)
        sys.exit(1)
