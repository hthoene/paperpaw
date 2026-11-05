"""Independent mutations of explicit draft ABI metadata requirements."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

PATH = Path(__file__).resolve().parents[1] / 'abi_requirements.py'
SPEC = importlib.util.spec_from_file_location('abi_requirements', PATH)
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)


class RequirementTests(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((PATH.parents[1] / 'abi/abi.def').read_text())
        self.schema['minor'] = 4
        self.schema['host_entries'][0]['since'] = 2
        self.schema['records'] = [
            {'id': 0, 'name': 'test.record', 'since': 2, 'fields': [
                self.field('early', 2), self.field('late', 3)]},
            {'id': 1, 'name': 'test.other', 'since': 4, 'fields': [
                self.field('other', 4)]}]

    @staticmethod
    def field(name, since):
        return {'name': name, 'type': 'u32', 'since': since, 'optional': True,
                'ordinal': since, 'default': 0, 'max_bytes': 0}

    def reject(self, used, floor=0, schema=None):
        with self.assertRaises(ValueError):
            ABI.required_minor(self.schema if schema is None else schema, used, floor)

    def test_current_schema_and_conservative_empty_list(self):
        schema = json.loads((PATH.parents[1] / 'abi/abi.def').read_text())
        self.assertEqual(ABI.required_minor(schema, [], schema['minor']), schema['minor'])
        for floor in range(self.schema['minor']):
            with self.subTest(floor=floor):
                self.reject([], floor)

    def test_host_record_field_and_floor_aggregate_by_maximum(self):
        host = {'kind': 'host', 'name': 'heap.alloc'}
        record = {'kind': 'record', 'name': 'test.record'}
        field = {'kind': 'field', 'record': 'test.record', 'name': 'late'}
        for used, floor, expected in [([host], 0, 2), ([record], 0, 2),
                                      ([field], 0, 3), ([host, record, field], 0, 3),
                                      ([host, record, field], 4, 4)]:
            with self.subTest(used=used, floor=floor):
                self.assertEqual(ABI.required_minor(self.schema, used, floor), expected)

    def test_field_also_aggregates_enclosing_record_since(self):
        # Full schema validation is separate. Resolve both values even when
        # this isolated mutation makes the field older than its record.
        self.schema['records'][0]['since'] = 4
        used = [{'kind': 'field', 'record': 'test.record', 'name': 'early'}]
        self.assertEqual(ABI.required_minor(self.schema, used, 0), 4)

    def test_callback_requirements_include_tail_and_contract_floors(self):
        schema = copy.deepcopy(self.schema)
        plugin = schema['entry_contract']['tables'][0]
        plugin['fields'][-1]['since'] = 3
        used = [{'kind': 'callback', 'name': 'draw'}]
        self.assertEqual(ABI.required_minor(schema, used, 0), 3)
        schema['entry_contract']['since'] = 4
        self.assertEqual(ABI.required_minor(schema, used, 0), 4)
        for name in ('missing', 'state', 'size'):
            self.reject([{'kind': 'callback', 'name': name}], schema=schema)
        self.reject(used + used, schema=schema)
        del schema['entry_contract']
        self.reject(used, schema=schema)

    def test_floor_rejects_boolean_and_noninteger_or_out_of_range_values(self):
        for floor in (True, False, -1, 5, 1.0, '1', None, [], {}):
            with self.subTest(floor=floor):
                self.reject([{'kind': 'host', 'name': 'heap.alloc'}], floor)

    def test_used_requires_list_of_objects(self):
        for used in (None, {}, (), 'heap.alloc', True, [None], [[]], ['heap.alloc']):
            with self.subTest(used=used):
                self.reject(used, 4)

    def test_unknown_or_malformed_kinds_are_rejected(self):
        for kind in ('service', 'feature', 'Host', '', None, True, [], {}):
            with self.subTest(kind=kind):
                self.reject([{'kind': kind, 'name': 'heap.alloc'}])
        self.reject([{'name': 'heap.alloc'}])

    def test_every_requirement_shape_is_exact(self):
        examples = [{'kind': 'host', 'name': 'heap.alloc'},
                    {'kind': 'record', 'name': 'test.record'},
                    {'kind': 'field', 'record': 'test.record', 'name': 'early'}]
        for example in examples:
            for key in example:
                with self.subTest(kind=example['kind'], missing=key):
                    changed = dict(example)
                    del changed[key]
                    self.reject([changed])
            for key in ('needs_minor', 'since', 'typo'):
                with self.subTest(kind=example['kind'], extra=key):
                    self.reject([dict(example, **{key: 0})])
        self.reject([{'kind': 'host', 'record': 'test.record', 'name': 'heap.alloc'}])

    def test_unknown_names_and_wrong_name_types_are_rejected(self):
        for name in ('missing', 'Heap.alloc', '', None, True, [], {}):
            with self.subTest(name=name):
                self.reject([{'kind': 'host', 'name': name}])
                self.reject([{'kind': 'record', 'name': name}])
                self.reject([{'kind': 'field', 'record': name, 'name': 'early'}])
                self.reject([{'kind': 'field', 'record': 'test.record', 'name': name}])
        self.reject([{'kind': 'field', 'record': 'test.record', 'name': 'other'}])

    def test_duplicates_fail_even_with_different_object_key_order(self):
        for item in ({'kind': 'host', 'name': 'heap.alloc'},
                     {'kind': 'record', 'name': 'test.record'},
                     {'kind': 'field', 'record': 'test.record', 'name': 'early'}):
            with self.subTest(item=item):
                self.reject([item, dict(reversed(list(item.items())))])

    def test_unknown_requirement_after_valid_one_cannot_be_ignored(self):
        valid = {'kind': 'host', 'name': 'heap.alloc'}
        for invalid in ({'kind': 'host', 'name': 'missing'},
                        {'kind': 'record', 'name': 'missing'},
                        {'kind': 'field', 'record': 'test.record', 'name': 'missing'}):
            with self.subTest(invalid=invalid):
                self.reject([valid, invalid], 4)

    def test_input_is_not_mutated_and_order_does_not_change_result(self):
        used = [{'kind': 'host', 'name': 'heap.alloc'},
                {'kind': 'field', 'record': 'test.record', 'name': 'late'}]
        before_schema, before_used = copy.deepcopy(self.schema), copy.deepcopy(used)
        self.assertEqual(ABI.required_minor(self.schema, used, 0), 3)
        self.assertEqual(ABI.required_minor(self.schema, list(reversed(used)), 0), 3)
        self.assertEqual(self.schema, before_schema)
        self.assertEqual(used, before_used)

    def test_invalid_schema_versions_and_since_values_fail(self):
        used = [{'kind': 'host', 'name': 'heap.alloc'}]
        for value in (True, False, None, '1', 1.0, -1, 0, 65536):
            with self.subTest(minor=value):
                schema = copy.deepcopy(self.schema)
                schema['minor'] = value
                self.reject(used, schema=schema)
        for kind in ('host', 'record', 'field'):
            for value in (True, False, None, '1', 1.0, -1, 0, 5):
                with self.subTest(kind=kind, since=value):
                    schema = copy.deepcopy(self.schema)
                    item = (schema['host_entries'][0] if kind == 'host' else
                            schema['records'][0] if kind == 'record' else
                            schema['records'][0]['fields'][0])
                    item['since'] = value
                    self.reject(used, schema=schema)

    def test_ambiguous_schema_names_fail(self):
        for collection in ('host_entries', 'records', 'fields'):
            with self.subTest(collection=collection):
                schema = copy.deepcopy(self.schema)
                items = (schema['records'][0]['fields'] if collection == 'fields'
                         else schema[collection])
                items.append(copy.deepcopy(items[0]))
                self.reject([{'kind': 'host', 'name': 'heap.alloc'}], schema=schema)


if __name__ == '__main__':
    unittest.main()
