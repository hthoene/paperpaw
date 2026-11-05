#!/usr/bin/env python3
"""Validate the draft ABI schema and generate deterministic M1 declarations.

This skeleton does not generate wire codecs, safe wrappers or artifact layouts.
A checked-in lock is independent of normal generation; updating it is explicit.
"""
import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
TYPES = {'context': 'void *', 'pointer': 'void *', 'bytes': 'PpBytes',
         'buffer': 'PpBuffer', 'handle': 'uint32_t', 'usize': 'size_t',
         'u16': 'uint16_t', 'u32': 'uint32_t', 'u64': 'uint64_t',
         'i32': 'int32_t', 'f32': 'float', 'f64': 'double',
         'out_ptr': 'void **', 'out_u64': 'uint64_t *',
         'out_handle': 'uint32_t *', 'out_f32': 'float *'}
WIRE = {'u32', 'u64', 'i32', 'str', 'bytes'}


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
    keys(schema, 'format major minor host_entries records')
    require(type(schema['format']) is int and schema['format'] == 1,
            'unsupported schema format')
    integer(schema['major'], 65535, 1)
    integer(schema['minor'], 65535, 1)
    for kind in ('host_entries', 'records'):
        declarations(schema[kind], schema['minor'])
    c_names = set()
    for entry in schema['host_entries']:
        keys(entry, 'id name since args result effect ownership reentry max_bytes')
        cname = entry['name'].replace('.', '_')
        require(cname not in c_names, 'colliding generated C names')
        c_names.add(cname)
        require(entry['result'] in ('status', 'length'), 'invalid result')
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
            require(arg['name'] not in names, 'duplicate argument name')
            names.add(arg['name'])
            require(arg['type'] in TYPES, 'unknown argument type')
        if any(a['type'] in ('bytes', 'buffer') for a in entry['args']):
            require(entry['max_bytes'] > 0, 'buffer entries need a positive cap')
    for record in schema['records']:
        validate_record(record, schema['minor'])
    return schema


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


def compatible(old, new):
    validate(old)
    validate(new)
    require(old['format'] == new['format'] and old['major'] == new['major'],
            'format/major change needs separate reviewed baseline')
    require(old['minor'] <= new['minor'] <= old['minor'] + 1,
            'minor must stay or advance by one')
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
                    require(field['since'] > old['minor'], 'backdated appended field')
        for item in current[len(prior):]:
            require(item['since'] > old['minor'], 'backdated appended declaration')


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
    lines += ['} PpHost;', '#endif', '']
    return '\n'.join(lines)


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
    if args.baseline:
        compatible(load(args.baseline), schema)
    if lock.exists():
        compatible(load(lock), schema)
    elif not args.write_lock:
        raise ValueError('missing lock; explicit --write-lock required')
    outputs = {folder / 'abi.json': canonical(schema),
               folder / 'generated' / 'paperpaw-abi.h': header(schema)}
    if args.check:
        require(load(lock) == schema, 'lock stale; explicitly review/update it')
        for path, wanted in outputs.items():
            require(path.exists() and path.read_text() == wanted, f'stale generated file: {path}')
    else:
        for path, content in outputs.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        if args.write_lock:
            lock.write_text(canonical(schema))
    return 0


if __name__ == '__main__':
    try:
        sys.exit(run())
    except (ValueError, OSError, TypeError, KeyError) as error:
        print(f'abi-gen: {error}', file=sys.stderr)
        sys.exit(1)
