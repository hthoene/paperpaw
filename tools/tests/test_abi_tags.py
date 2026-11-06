"""Records are classified by exact tag only, never by range (N3, #241).

Tag families own growth blocks (`TAG_RANGES` in `tools/abi_declarations.py`).
A decoder that classified by range would treat a record added to such a block
later as one it already knows, so 1.0 plugins would misread new records. The
ranges only check where a schema author places a record.
"""
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))

from abi_declarations import TAG_RANGES, wire_records  # noqa: E402

SOURCES = [
    'abi/generated/records_wire.rs',
    'abi/generated/commands_wire.rs',
    'crates/paperpaw-core/src/native_host',
    'crates/paperpaw-core/src/abi7',
    'crates/paperpaw-sdk/src/native',
    'shells',
]
# A range pattern over a tag: `TAG..`, `..=TAG`, `tag >= `, `tag < `, a numeric
# range arm, and a numeric range on any line that names a tag
# (`matches!(tag, 32..=39)`, `(32..40).contains(&tag)`).
RANGE = re.compile(r'_TAG\s*\.\.|\.\.=?\s*[A-Z_]*_TAG\b|\btag\s*(<=?|>=?)\s*\w'
                   r'|\b\w+\s*(<=?|>=?)\s*tag\b|\b\d+\s*\.\.=?\s*\d+\s*=>')
NUMERIC_RANGE = re.compile(r'\b\d+\s*\.\.=?\s*\d+')
TAGGED = re.compile(r'\btag\b|_tag\b|\bTAG\b|_TAG\b', re.IGNORECASE)
# Shell files that handle plugin-interface records; the rest of a shell
# (reset reasons, key codes) has its own numeric ranges.
ABI_CODE = re.compile(r'records_decode|commands_decode|_TAG\b|paperpaw_abi|host_glue')
# Copies of the generated codec that abi-gen keeps byte for byte the same.
COPIES = {
    'abi/generated/records_wire.rs': ['crates/paperpaw-core/src/abi7/records_wire.rs',
                                      'crates/paperpaw-sdk/src/native/declare/records_wire.rs'],
}


def varint(value):
    out = bytearray()
    while True:
        byte = value & 0x7f
        value >>= 7
        out.append(byte | (0x80 if value else 0))
        if not value:
            return bytes(out)


def single(tag):
    """A batch of one record with the tag and an empty body."""
    return varint(1) + varint(tag) + varint(0)


def rust_files():
    for source in SOURCES:
        path = ROOT / source
        if path.is_dir():
            yield from sorted(p for p in path.rglob('*.rs')
                              if 'target' not in p.relative_to(ROOT).parts
                              and (source != 'shells' or ABI_CODE.search(p.read_text())))
        elif path.exists():
            yield path


class ExactTagTests(unittest.TestCase):
    def test_decoders_never_classify_records_by_tag_range(self):
        found = []
        for path in rust_files():
            for number, line in enumerate(path.read_text().splitlines(), 1):
                code = line.split('//')[0]
                if RANGE.search(code) or (NUMERIC_RANGE.search(code) and TAGGED.search(code)):
                    found.append(f'{path.relative_to(ROOT)}:{number}: {line.strip()}')
        self.assertEqual(found, [], 'classify records by exact tag only')

    def test_every_tag_up_to_2047_is_classified_exactly_by_the_decoders(self):
        """Behavioural: whatever form a decoder uses, a tag it does not know
        exactly is ERR_UNSUPPORTED, and a tag it knows is never."""
        for generated, copies in COPIES.items():
            for copy in copies:
                self.assertEqual((ROOT / copy).read_bytes(), (ROOT / generated).read_bytes(), copy)
        schema = json.loads((ROOT / 'abi/abi.def').read_text())
        statuses = {item['name']: item['value'] for item in schema['statuses']}
        # The batch codec: registrations plus declared wire records; tick (3) and
        # commands have decoders of their own, checked below or by their tests.
        records = sorted([1, 2] + [r['id'] for r in wire_records(schema)])
        commands = sorted(r['id'] for r in schema['records'] if r['name'].startswith('command.'))
        tags = list(range(2048)) + [4095, 65535, 1 << 20, (1 << 32) - 1]
        cases = ',\n'.join(f'    ({tag}, &{list(single(tag))})' for tag in tags)
        test = f"""#![allow(dead_code, unused_imports)]
mod records {{ include!("{(ROOT / 'abi/generated/records_wire.rs').as_posix()}"); }}
mod settings_wire {{ include!("{(ROOT / 'abi/generated/settings_value.rs').as_posix()}"); }}
mod commands {{ include!("{(ROOT / 'abi/generated/commands_wire.rs').as_posix()}"); }}
const RECORDS: &[u32] = &{records};
const COMMANDS: &[u32] = &{commands};
const UNSUPPORTED: i32 = {statuses['ERR_UNSUPPORTED']};
const CASES: &[(u32, &[u8])] = &[
{cases}
];
#[test] fn exact() {{
    let mut wrong = Vec::new();
    for &(tag, bytes) in CASES {{
        let record = records::records_decode(bytes).err() == Some(UNSUPPORTED);
        let single = records::records_decode_single(bytes).err() == Some(UNSUPPORTED);
        let command = commands::commands_decode(bytes).err() == Some(UNSUPPORTED);
        if record == RECORDS.contains(&tag) || single != record {{
            wrong.push(("records", tag));
        }}
        if command == COMMANDS.contains(&tag) {{
            wrong.push(("commands", tag));
        }}
    }}
    assert!(wrong.is_empty(), "{{wrong:?}}");
}}
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'tags.rs').write_text(test)
            built = subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tags.rs'),
                                    '-o', str(path / 'tags')], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stderr[-3000:])
            result = subprocess.run([str(path / 'tags')], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

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
