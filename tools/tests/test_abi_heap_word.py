"""The heap word and PpHeap in the entry contract (`abi/heap-word.md`)."""
import copy
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
from abi_callbacks import rust_callbacks
from abi_entry import c_entry, compatible_entry, rust_entry, validate_entry


class HeapWordTests(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((ROOT / 'abi/abi.def').read_text())
        self.contract = self.schema['entry_contract']

    def without_heap(self):
        prior = copy.deepcopy(self.schema)
        del prior['entry_contract']['heap_word']
        prior['entry_contract']['tables'].pop()
        return prior

    def test_the_word_names_ppheap_in_rodata_and_pairs_with_the_table(self):
        word = self.contract['heap_word']
        self.assertEqual((word['symbol'], word['alias'], word['section'], word['table']),
                         ('PAPERPAW_HEAP', 'paperpaw_heap_local', '.rodata.paperpaw_heap',
                          'PpHeap'))
        self.assertEqual([t['name'] for t in self.contract['tables']],
                         ['PpPlugin', 'PpEntry', 'PpHeap'])
        no_table = copy.deepcopy(self.schema)
        no_table['entry_contract']['tables'].pop()
        with self.assertRaisesRegex(ValueError, 'then PpHeap exactly with a heap word'):
            validate_entry(no_table)
        no_word = copy.deepcopy(self.schema)
        del no_word['entry_contract']['heap_word']
        with self.assertRaisesRegex(ValueError, 'then PpHeap exactly with a heap word'):
            validate_entry(no_word)

    def test_malformed_words_are_refused(self):
        cases = [('section', '.data.paperpaw_heap', 'named .rodata section'),
                 ('section', '.rodata', 'named .rodata section'),
                 ('table', 'PpHost', 'points at PpHeap'),
                 ('symbol', 'PAPERPAW_ENTRY', 'duplicate entry contract symbol'),
                 ('alias', '_hidden', 'reserved entry contract identifier')]
        for key, value, message in cases:
            with self.subTest(key=key, value=value):
                candidate = copy.deepcopy(self.schema)
                candidate['entry_contract']['heap_word'][key] = value
                with self.assertRaisesRegex(ValueError, message):
                    validate_entry(candidate)
        extra = copy.deepcopy(self.schema)
        extra['entry_contract']['heap_word']['extra'] = 1
        with self.assertRaisesRegex(ValueError, 'keys'):
            validate_entry(extra)

    def test_ppheap_has_the_fixed_context_free_prefix(self):
        fields = self.contract['tables'][2]['fields']
        self.assertEqual([f['name'] for f in fields],
                         ['size', 'major', 'minor', 'alloc', 'dealloc', 'realloc', 'abort'])
        for field in fields[3:]:
            self.assertNotIn('context', [a['type'] for a in field['callback']['args']])
        changed = copy.deepcopy(self.schema)
        changed['entry_contract']['tables'][2]['fields'][3]['callback']['result'] = 'i32'
        with self.assertRaisesRegex(ValueError, 'unsupported entry table prefix'):
            validate_entry(changed)

    def test_adding_the_word_is_compatible_once_and_never_changes(self):
        prior = self.without_heap()
        validate_entry(prior)
        compatible_entry(prior, self.schema, 1)
        with self.assertRaisesRegex(ValueError, 'backdated heap word'):
            compatible_entry(prior, self.schema, 2)
        with self.assertRaisesRegex(ValueError, 'changed entry locator or export semantics'):
            compatible_entry(self.schema, prior, 1)
        moved = copy.deepcopy(self.schema)
        moved['entry_contract']['heap_word']['alias'] = 'other_alias'
        with self.assertRaisesRegex(ValueError, 'changed entry locator or export semantics'):
            compatible_entry(self.schema, moved, 1)

    def test_without_a_word_the_emitted_contract_is_unchanged(self):
        prior = self.without_heap()
        for text in (c_entry(prior), rust_entry(prior)):
            self.assertNotIn('PpHeap', text)
            self.assertNotIn('HEAP', text)

    def test_generated_heap_readers_check_major_and_size_first(self):
        readers = rust_callbacks(self.schema)
        for name in ('alloc', 'dealloc', 'realloc', 'abort'):
            self.assertIn(f'pub unsafe fn read_heap_{name}(table: *const PpHeap)', readers)
        self.assertIn('major == MAJOR && usize::try_from(size).is_ok_and(|size| size >= end)',
                      readers)
        self.assertNotIn('read_heap_', rust_callbacks(self.without_heap()))

    def test_emission_defines_the_word_and_a_checked_reader(self):
        c, rust = c_entry(self.schema), rust_entry(self.schema)
        for needle in ('#define PP_HEAP_SYMBOL "PAPERPAW_HEAP"',
                       '.pushsection .rodata.paperpaw_heap',
                       '.hidden paperpaw_heap_local',
                       'visibility("hidden")',
                       'static inline const PpHeap *pp_heap(void)',
                       'offsetof(PpHeap, abort) + sizeof(heap->abort)',
                       '/* Does not return. */'):
            self.assertIn(needle, c)
        self.assertIn('pub const HEAP_SYMBOL: &str = "PAPERPAW_HEAP";', rust)
        self.assertIn('pub abort: Option<unsafe extern "C" fn(code: i32, msg: *const u8, '
                      'msg_len: usize) -> !>,', rust)
        self.assertIn('pub alloc: Option<unsafe extern "C" fn(size: usize, align: usize) -> '
                      '*mut core::ffi::c_void>,', rust)


if __name__ == '__main__':
    unittest.main()
