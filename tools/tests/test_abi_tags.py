"""Records are classified by exact tag only, never by range (N3, #241).

Tag families own growth blocks (`TAG_RANGES` in `tools/abi_declarations.py`).
A decoder that classified by range would treat a record added to such a block
later as one it already knows, so 1.0 plugins would misread new records. The
ranges only check where a schema author places a record.
"""
from pathlib import Path
import re
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))

from abi_declarations import TAG_RANGES  # noqa: E402

SOURCES = [
    'abi/generated/records_wire.rs',
    'abi/generated/commands_wire.rs',
    'crates/paperpaw-core/src/native_host',
    'crates/paperpaw-sdk/src/native',
]
# A range pattern over a tag: `TAG..`, `..=TAG`, `tag >= `, `tag < ` and so on.
RANGE = re.compile(r'_TAG\s*\.\.|\.\.=?\s*[A-Z_]*_TAG\b|\btag\s*(<=?|>=?)\s*\w'
                   r'|\b\w+\s*(<=?|>=?)\s*tag\b|\b\d+\s*\.\.=?\s*\d+\s*=>')


def rust_files():
    for source in SOURCES:
        path = ROOT / source
        if path.is_dir():
            yield from sorted(path.rglob('*.rs'))
        elif path.exists():
            yield path


class ExactTagTests(unittest.TestCase):
    def test_decoders_never_classify_records_by_tag_range(self):
        found = []
        for path in rust_files():
            for number, line in enumerate(path.read_text().splitlines(), 1):
                code = line.split('//')[0]
                if RANGE.search(code):
                    found.append(f'{path.relative_to(ROOT)}:{number}: {line.strip()}')
        self.assertEqual(found, [], 'classify records by exact tag only')

    def test_growth_blocks_are_reserved_and_disjoint_between_unrelated_families(self):
        self.assertIn((64, 191), TAG_RANGES['event'])
        self.assertIn((192, 223), TAG_RANGES['input'])
        self.assertIn((224, 511), TAG_RANGES['setting'])
        self.assertEqual(TAG_RANGES['stage'], ((512, 1023),))
        self.assertIn((1024, 1535), TAG_RANGES['service'])
        self.assertIn((1536, 2047), TAG_RANGES['command'])
        # Families that share a block on purpose: all declarations (submitted
        # through slot 16), and draw frames inside the input block.
        shared = {frozenset({'setting', 'resource', 'action', 'menu', 'screen', 'icon',
                             'format', 'registration'}), frozenset({'input', 'draw'})}
        top = {name: spans for name, spans in TAG_RANGES.items() if '.' not in name}
        for first, spans in top.items():
            for second, others in top.items():
                if first >= second or any({first, second} <= group for group in shared):
                    continue
                for low, high in spans:
                    for other_low, other_high in others:
                        with self.subTest(first=first, second=second):
                            self.assertTrue(high < other_low or other_high < low)


if __name__ == '__main__':
    unittest.main()
