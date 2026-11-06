#!/usr/bin/env python3
"""Generate metadata-only vectors with an explicitly synthetic target profile."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path

from abi_metadata import decode, encode
from abi_metadata_schema import resolved_metadata

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('fixture_generator', ROOT / 'tools/abi-gen.py')
GEN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GEN)


def outputs():
    schema = copy.deepcopy(GEN.load(ROOT / 'abi/abi.def'))
    # Metadata-only fixtures exclude the unrelated settings codec descriptor.
    schema.pop('settings_value', None)
    # Slot-15 command values depend on that codec and are excluded with it.
    schema.pop('command_codec', None)
    schema['records'] = [item for item in schema['records']
                         if item['name'] != 'command.set_setting']
    schema['metadata']['profiles'] = [dict(name='test-only-le64', pointer_bits=64,
        byte_order='little', calling_convention='synthetic-not-loadable',
        entry_layout=65535, entry_bytes=64, entry_align=8)]
    GEN.validate(schema)
    build = dict(target='test-only-le64', used=[], floor=schema['minor'], services=[], features=[])
    good = encode(schema, build)
    fields = {x['name']: x for x in resolved_metadata(schema)['prefix']['fields']}
    cases = [('valid-empty-requirements', good, True), ('truncated-prefix', good[:3], False),
             ('truncated-tail', good[:-1], False), ('trailing-byte', good + b'\0', False)]
    for field, value in [('magic', b'NOPE'), ('flags', 1), ('exports', 3), ('total_bytes', 0)]:
        changed = bytearray(good)
        at, width = fields[field]['offset'], fields[field]['width']
        changed[at:at + width] = value if type(value) is bytes else value.to_bytes(width, 'little')
        cases.append(('invalid-' + field.replace('_', '-'), bytes(changed), False))
    folder = ROOT / 'abi/fixtures/metadata'
    result = {folder / 'schema.json': GEN.canonical(schema),
              folder / 'build.json': GEN.canonical(build)}
    manifest = dict(format=1, test_only=True, schema='schema.json', build='build.json',
                    scope='metadata bytes only; synthetic target is never executable', cases=[])
    for name, data, expected in cases:
        try:
            decode(schema, data)
            actual = True
        except ValueError:
            actual = False
        if actual != expected:
            raise ValueError('fixture verdict changed: ' + name)
        path = name + '.hex'
        result[folder / path] = data.hex() + '\n'
        manifest['cases'].append(dict(name=name, hex=path, expected=dict(
            metadata='accept' if expected else 'reject', compatibility='not_run',
            container='not_run', signature='not_run', execution='not_run')))
    result[folder / 'manifest.json'] = GEN.canonical(manifest)
    actual_schema = GEN.validate(GEN.load(ROOT / 'abi/abi.def'))
    if 'entry_contract' in actual_schema:
        entry_folder = ROOT / 'abi/fixtures/entry'
        entry_manifest = dict(format=1, schema='../../abi.json',
                              scope='declared target metadata only; no executable artifact', cases=[])
        for profile in actual_schema['metadata']['profiles']:
            name = profile['name']
            request = dict(target=name, used=[], floor=actual_schema['minor'],
                           services=[], features=[])
            encoded = encode(actual_schema, request)
            decode(actual_schema, encoded)
            result[entry_folder / (name + '.hex')] = encoded.hex() + '\n'
            result[entry_folder / (name + '.json')] = GEN.canonical(request)
            entry_manifest['cases'].append(dict(name=name, hex=name + '.hex', input=name + '.json',
                expected=dict(metadata='accept', compatibility='not_run', container='not_run',
                              signature='not_run', execution='not_run')))
        result[entry_folder / 'manifest.json'] = GEN.canonical(entry_manifest)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    for path, content in outputs().items():
        if args.check:
            if not path.exists() or path.read_text() != content:
                raise SystemExit('stale fixture: ' + str(path))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)


if __name__ == '__main__':
    main()
