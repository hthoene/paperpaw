"""Versioned boundary partition: no unchecked namespace or reexport escape."""
import sys
from pathlib import Path
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from abi_partition import audit_rustdoc, partition


class PartitionTests(unittest.TestCase):
    def test_exact_public_and_private_flat_paths(self):
        for path in ('paperpaw_native::host_glue::PpPlugin',
                     'paperpaw_native::host_glue::generated::PpPlugin'):
            text = f'struct {path} {{ size: u32 }}\n'
            self.assertEqual(partition('paperpaw-native', text), ('', text))

    def test_reference_is_not_a_definition(self):
        for text in ('struct paperpaw_native::interface::Api { p: paperpaw_native::host_glue::PpPlugin }\n',
                     'pub type paperpaw_sdk::Alias = paperpaw_native::host_glue::PpPlugin\n'):
            with self.assertRaises(ValueError):
                partition('paperpaw-native', text)

    def test_no_prefix_lookalike_or_other_crate_exemption(self):
        text = 'struct paperpaw_native::host_glue_old::PpPlugin { size: u32 }\n'
        self.assertEqual(partition('paperpaw-native', text), (text, ''))
        with self.assertRaises(ValueError):
            partition('paperpaw-sdk', 'pub mod paperpaw_native::host_glue\n')

    def test_trait_impl_owned_by_self(self):
        text = 'impl paperpaw_native::host_glue::Trait for paperpaw_sdk::Old {}\n'
        with self.assertRaises(ValueError):
            partition('paperpaw-native', text)
        text = 'impl core::fmt::Debug for paperpaw_native::host_glue::PpPlugin {}\n'
        self.assertEqual(partition('paperpaw-native', text), ('', text))

    def doc(self, indirect=False):
        def item(name, inner, visibility='public'):
            return dict(name=name, crate_id=0, visibility=visibility, inner=inner)
        target = '4' if indirect else '3'
        index = {
            '0': item('paperpaw_native', {'module': {'items': [1, 2, 4]}}),
            '1': item('host_glue', {'module': {'items': [3]}}),
            '2': item('LegacyAlias', {'use': {'id': target, 'source': 'renamed', 'is_glob': False}}),
            '3': item('PpPlugin', {'struct': {'kind': {'plain': {'fields': []}}}}),
            '4': item('HiddenAlias', {'type_alias': {'type': {'resolved_path': {'id': '3'}}}}, 'default'),
        }
        paths = {'3': {'path': ['paperpaw_native', 'host_glue', 'PpPlugin']}}
        return {'root': '0', 'index': index, 'paths': paths}

    def test_public_renamed_reexport_outside_partition_fails(self):
        for indirect in (False, True):
            with self.assertRaisesRegex(ValueError, 'LegacyAlias'):
                audit_rustdoc(self.doc(indirect))

    def test_reexport_inside_flat_module_is_allowed(self):
        doc = self.doc()
        doc['index']['0']['inner']['module']['items'] = [1, 4]
        doc['index']['1']['inner']['module']['items'] = [2, 3]
        audit_rustdoc(doc)

    def test_private_alias_in_legacy_field_still_reaches_flat(self):
        doc = self.doc(True)
        doc['index']['2']['inner'] = {'struct_field': {'resolved_path': {'id': '4'}}}
        doc['index']['2']['visibility'] = 'public'
        with self.assertRaises(ValueError):
            audit_rustdoc(doc)


if __name__ == '__main__':
    unittest.main()
