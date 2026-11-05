"""Round trips and independent hostile mutations of draft metadata blocks."""
import copy
import importlib.util
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
import abi_metadata as ABI
from abi_layout import SCALARS, field_layout

SPEC = importlib.util.spec_from_file_location('abi_metadata_cli', TOOLS / 'abi-metadata.py')
CLI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLI)


class MetadataTests(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((TOOLS.parent / 'abi/abi.def').read_text())
        self.schema['minor'] = 4
        self.metadata = self.schema['metadata']
        # These profiles and catalog entries are synthetic. They make no claim
        # about any production target's entry layout, calling convention or size.
        self.metadata['profiles'] = [dict(name='synthetic32', pointer_bits=32,
            byte_order='little', calling_convention='test_c', entry_layout=123,
            entry_bytes=48, entry_align=4), dict(name='synthetic64', pointer_bits=64,
            byte_order='little', calling_convention='test_c', entry_layout=456,
            entry_bytes=80, entry_align=8)]
        self.metadata['services'] = [
            dict(key='test.alpha', major=2, semantic_revision=3, min_table_bytes=24, since=2),
            dict(key='test.bravo', major=1, semantic_revision=1, min_table_bytes=16, since=3)]
        self.metadata['features'] = [dict(key='test.delta', revision=2, since=4),
                                     dict(key='test.echo', revision=1, since=2)]
        self.build = dict(target='synthetic32', used=[dict(kind='host', name='heap.alloc')],
                          floor=0, services=['test.bravo', 'test.alpha'],
                          features=['test.echo', 'test.delta'])
        self.data = ABI.encode(self.schema, self.build)

    def prefix(self, data, **changes):
        result = bytearray(data)
        fields = field_layout(self.metadata['prefix'])['fields']
        for field in fields:
            if field['name'] in changes:
                struct.pack_into('<' + SCALARS[field['type']][0], result,
                                 field['offset'], changes[field['name']])
        return bytes(result)

    def locations(self):
        position = field_layout(self.metadata['prefix'])['size'] + len(self.build['target'])
        result = []
        for kind, items, text in [('export', self.metadata['exports'], 'name'),
                                  ('service', sorted(self.metadata['services'], key=lambda i: i['key']), 'key'),
                                  ('feature', sorted(self.metadata['features'], key=lambda i: i['key']), 'key')]:
            layout = field_layout(self.metadata['records'][kind])
            for item in items:
                result.append((kind, position, layout))
                position += layout['size'] + len(item[text].encode('utf-8'))
        return result

    def record_change(self, location, field_name, value):
        _, position, layout = location
        field = next(f for f in layout['fields'] if f['name'] == field_name)
        data = bytearray(self.data)
        struct.pack_into('<' + SCALARS[field['type']][0], data,
                         position + field['offset'], value)
        return bytes(data)

    def reject(self, data):
        with self.assertRaises(ValueError):
            ABI.decode(self.schema, data)

    def test_roundtrip_and_deterministic_catalog_order(self):
        decoded = ABI.decode(self.schema, self.data)
        self.assertEqual(decoded['needs_minor'], 4)
        self.assertEqual(decoded['target'], 'synthetic32')
        self.assertEqual(decoded['entry'], dict(layout=123, bytes=48, align=4))
        self.assertEqual(decoded['profile'], self.metadata['profiles'][0])
        self.assertEqual(decoded['exports'], [dict(name='PAPERPAW_ENTRY', kind=1, expected_bytes=48)])
        self.assertEqual([item['key'] for item in decoded['services']], ['test.alpha', 'test.bravo'])
        self.assertEqual([item['key'] for item in decoded['features']], ['test.delta', 'test.echo'])
        reordered = dict(self.build, services=list(reversed(self.build['services'])),
                         features=list(reversed(self.build['features'])))
        self.assertEqual(ABI.encode(self.schema, reordered), self.data)
        json.dumps(decoded)

    def test_profile_drives_entry_export_size(self):
        decoded = ABI.decode(self.schema, ABI.encode(self.schema, dict(self.build, target='synthetic64')))
        self.assertEqual(decoded['entry'], dict(layout=456, bytes=80, align=8))
        self.assertEqual([e['expected_bytes'] for e in decoded['exports']], [80])

    def test_every_truncation_boundary_and_forged_length_fails(self):
        for end in range(len(self.data)):
            with self.subTest(end=end):
                self.reject(self.data[:end])
                if end >= field_layout(self.metadata['prefix'])['size']:
                    self.reject(self.prefix(self.data[:end], total_bytes=end))

    def test_prefix_magic_layout_major_lengths_reserved_flags_and_counts(self):
        mutations = dict(magic=b'XXXX', layout=99, prefix_bytes=31, total_bytes=len(self.data) + 1,
                         abi_major=self.schema['major'] + 1, entry_layout=999,
                         entry_bytes=49, flags=1, needs_minor=0, target_bytes=0)
        for field, value in mutations.items():
            with self.subTest(field=field):
                self.reject(self.prefix(self.data, **{field: value}))
        for kind in ('exports', 'services', 'features'):
            self.reject(self.prefix(self.data, **{kind: self.metadata['max_' + kind] + 1}))
            self.reject(self.prefix(self.data, **{kind: 0}))
        self.reject(self.prefix(self.data, target_bytes=self.metadata['max_key_bytes'] + 1))

    def test_all_record_lengths_and_key_lengths_are_checked(self):
        for location in self.locations():
            kind, _, layout = location
            length = 'name_bytes' if kind == 'export' else 'key_bytes'
            for field, value in [('record_bytes', 0), ('record_bytes', 65535),
                                 (length, 0), (length, self.metadata['max_key_bytes'] + 1)]:
                with self.subTest(kind=kind, field=field, value=value):
                    self.reject(self.record_change(location, field, value))
            if kind != 'service':
                self.reject(self.record_change(location, 'reserved', 1))
            self.assertGreater(layout['size'], 0)

    def test_export_name_kind_and_expected_size_are_exact(self):
        for location in self.locations():
            if location[0] != 'export':
                continue
            for field in ('kind', 'expected_bytes'):
                self.reject(self.record_change(location, field, 255))
            _, position, layout = location
            data = bytearray(self.data)
            data[position + layout['size']] = ord('X')
            self.reject(bytes(data))

    def test_catalog_requirements_cannot_be_overridden(self):
        for location in self.locations():
            if location[0] == 'export':
                continue
            kind = location[0]
            fields = ('major', 'semantic_revision', 'min_table_bytes') if kind == 'service' else ('revision',)
            for field in fields:
                self.reject(self.record_change(location, field, 99))
        self.reject(self.prefix(self.data, needs_minor=3))

    def test_excessive_minor_is_structurally_valid(self):
        decoded = ABI.decode(self.schema, self.prefix(self.data, needs_minor=65535))
        self.assertEqual(decoded['needs_minor'], 65535)
        self.assertNotIn('loadable', decoded)

    def test_unknown_target_and_catalog_keys_in_both_directions(self):
        for key, value in [('target', 'unknown'), ('services', ['unknown']), ('features', ['unknown'])]:
            with self.subTest(key=key):
                with self.assertRaises(ValueError):
                    ABI.encode(self.schema, dict(self.build, **{key: value}))
        position = field_layout(self.metadata['prefix'])['size']
        data = bytearray(self.data)
        data[position] = ord('X')
        self.reject(bytes(data))
        for _, position, layout in self.locations()[2:]:
            data = bytearray(self.data)
            data[position + layout['size']] = ord('X')
            self.reject(bytes(data))
        self.metadata['profiles'] = []
        with self.assertRaises(ValueError):
            ABI.encode(self.schema, self.build)

    def test_invalid_utf8_in_each_text_location(self):
        positions = [field_layout(self.metadata['prefix'])['size']]
        positions += [position + layout['size'] for _, position, layout in self.locations()]
        for position in positions:
            data = bytearray(self.data)
            data[position] = 255
            self.reject(bytes(data))

    def test_duplicate_catalog_keys_fail_on_encode_and_decode(self):
        for kind in ('services', 'features'):
            with self.assertRaises(ValueError):
                ABI.encode(self.schema, dict(self.build, **{kind: [self.build[kind][0]] * 2}))
        # Each pair deliberately has equally long UTF-8 keys, so replacing the
        # second key preserves the entire record framing and isolates duplicates.
        for kind in ('service', 'feature'):
            locations = [item for item in self.locations() if item[0] == kind]
            _, first, layout = locations[0]
            _, second, _ = locations[1]
            key_size = len('test.alpha') if kind == 'service' else len('test.delta')
            data = bytearray(self.data)
            data[second + layout['size']:second + layout['size'] + key_size] = \
                data[first + layout['size']:first + layout['size'] + key_size]
            self.reject(bytes(data))

    def test_build_keys_types_overrides_and_caps(self):
        for build in (None, [], dict(self.build, needs_minor=1), dict(self.build, exports=[]),
                      dict(self.build, services=None), dict(self.build, features='test.echo'),
                      dict(self.build, target=[]), dict(self.build, floor=True)):
            with self.subTest(build=build):
                with self.assertRaises(ValueError):
                    ABI.encode(self.schema, build)
        for key in self.build:
            build = dict(self.build)
            del build[key]
            with self.assertRaises(ValueError):
                ABI.encode(self.schema, build)
        for kind in ('services', 'features'):
            with self.assertRaises(ValueError):
                ABI.encode(self.schema, dict(self.build, **{kind: ['test.alpha'] *
                           (self.metadata['max_' + kind] + 1)}))
        self.reject(self.data + b'\0')
        self.reject(self.prefix(self.data + b'\0', total_bytes=len(self.data) + 1))
        self.reject(b'\0' * (self.metadata['max_bytes'] + 1))
        self.metadata['max_bytes'] = len(self.data) - 1
        with self.assertRaises(ValueError):
            ABI.encode(self.schema, self.build)

    def test_exact_byte_and_key_caps_are_accepted(self):
        self.metadata['max_bytes'] = len(self.data)
        self.assertEqual(ABI.encode(self.schema, self.build), self.data)
        self.assertEqual(ABI.decode(self.schema, self.data)['total_bytes'], len(self.data))
        name = 's' * self.metadata['max_key_bytes']
        self.metadata['profiles'][0]['name'] = name
        self.metadata['max_bytes'] = 2048
        build = dict(self.build, target=name)
        self.assertEqual(ABI.decode(self.schema, ABI.encode(self.schema, build))['target'], name)

    def test_zero_requirement_caps_allow_empty_lists_and_reject_counts(self):
        self.metadata['max_services'] = self.metadata['max_features'] = 0
        build = dict(self.build, services=[], features=[])
        data = ABI.encode(self.schema, build)
        decoded = ABI.decode(self.schema, data)
        self.assertEqual(decoded['services'], [])
        self.assertEqual(decoded['features'], [])
        self.assertEqual(decoded['needs_minor'], 1)
        for kind in ('services', 'features'):
            with self.assertRaises(ValueError):
                ABI.encode(self.schema, dict(build, **{kind: self.build[kind]}))
            self.reject(self.prefix(data, **{kind: 1}))

    def test_exact_service_and_feature_count_caps_are_accepted(self):
        for kind in ('services', 'features'):
            prototype = self.metadata[kind][0]
            self.metadata[kind] = [dict(prototype, key=f'test.k{index:03}')
                                   for index in range(self.metadata['max_' + kind])]
        build = dict(self.build, **{kind: [item['key'] for item in self.metadata[kind]]
                                   for kind in ('services', 'features')})
        decoded = ABI.decode(self.schema, ABI.encode(self.schema, build))
        for kind in ('services', 'features'):
            self.assertEqual(len(decoded[kind]), self.metadata['max_' + kind])

    def test_malformed_metadata_schema_raises_value_error(self):
        for mutation in (None, {}, dict(self.metadata, typo=True),
                         dict(self.metadata, prefix=None), dict(self.metadata, records={})):
            schema = dict(self.schema, metadata=mutation)
            with self.subTest(mutation=mutation):
                with self.assertRaises(ValueError):
                    ABI.encode(schema, self.build)
                with self.assertRaises(ValueError):
                    ABI.decode(schema, self.data)

    def test_all_single_bit_mutations_return_a_complete_result_or_value_error(self):
        for position in range(len(self.data)):
            for bit in range(8):
                data = bytearray(self.data)
                data[position] ^= 1 << bit
                try:
                    result = ABI.decode(self.schema, bytes(data))
                except ValueError:
                    continue
                self.assertEqual(result['total_bytes'], len(self.data))
                self.assertEqual(result['target'], self.build['target'])
                self.assertEqual(result['exports'], ABI.decode(self.schema, self.data)['exports'])

    def test_inputs_are_not_mutated(self):
        schema, build = copy.deepcopy(self.schema), copy.deepcopy(self.build)
        ABI.encode(self.schema, self.build)
        ABI.decode(self.schema, self.data)
        self.assertEqual(self.schema, schema)
        self.assertEqual(self.build, build)

    def test_cli_roundtrip_and_invalid_build_preserves_both_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            schema, source, binary, inspected = [root / name for name in
                                               ('schema.json', 'build.json', 'meta.bin', 'meta.json')]
            schema.write_text(json.dumps(self.schema))
            source.write_text(json.dumps(self.build))
            argv = ['--schema', str(schema), '--input', str(source), '--bytes', str(binary),
                    '--json', str(inspected)]
            CLI.run(argv)
            self.assertEqual(binary.read_bytes(), self.data)
            self.assertEqual(json.loads(inspected.read_text()), ABI.decode(self.schema, self.data))
            before = binary.read_bytes(), inspected.read_bytes()
            for bad in ('{"target":"a","target":"b"}', '{"nested":{"a":1,"a":2}}',
                        '{"target":"a","\\u0074arget":"b"}', '{"floor":NaN}',
                        json.dumps(dict(self.build, needs_minor=1))):
                source.write_text(bad)
                with self.assertRaises(ValueError):
                    CLI.run(argv)
                self.assertEqual((binary.read_bytes(), inspected.read_bytes()), before)
            source.write_text(json.dumps(self.build))
            malformed = copy.deepcopy(self.schema)
            malformed['host_entries'][0]['args'][0]['type'] = 'unknown_type'
            for bad_schema in (json.dumps(malformed), '{"minor":1,"minor":2}'):
                schema.write_text(bad_schema)
                with self.assertRaises(ValueError):
                    CLI.run(argv)
                self.assertEqual((binary.read_bytes(), inspected.read_bytes()), before)

    def test_cli_rejects_shared_paths_symlinks_and_hard_links(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            schema, source, output, alias = [root / name for name in ('schema', 'build', 'out', 'alias')]
            schema.write_text(json.dumps(self.schema))
            source.write_text(json.dumps(self.build))
            output.write_bytes(b'unchanged')
            for linked in ('same', 'symlink', 'hardlink'):
                if linked == 'same':
                    destination = output
                elif linked == 'symlink':
                    alias.symlink_to(output)
                    destination = alias
                else:
                    alias.unlink()
                    alias.hardlink_to(output)
                    destination = alias
                with self.assertRaises(ValueError):
                    CLI.run(['--schema', str(schema), '--input', str(source), '--bytes', str(output),
                             '--json', str(destination)])
                self.assertEqual(output.read_bytes(), b'unchanged')
            with self.assertRaises(ValueError):
                CLI.run(['--schema', str(schema), '--input', str(source), '--bytes', str(source),
                         '--json', str(output)])


if __name__ == '__main__':
    unittest.main()
