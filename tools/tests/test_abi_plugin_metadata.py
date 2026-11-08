"""Every plugin's checked-in ABI metadata is what its input says.

`plugins/<name>/native/metadata[.host].json` is the input (target, host calls
and callbacks used, floor, services, features); the `.bin` is what the plugin
file carries and the `.inspected.json` what a reader sees of it. A changed
input with bytes or inspection left as they were is a plugin that claims what
it no longer is. Regenerate with `tools/abi-metadata.py` (see the plugin's
`PORTING-7.md`).
"""
import importlib.util
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))

from abi_metadata import decode, encode  # noqa: E402

SPEC = importlib.util.spec_from_file_location('abi_gen', ROOT / 'tools/abi-gen.py')
GEN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GEN)


class PluginMetadataIsFresh(unittest.TestCase):
    def test_bytes_and_inspection_follow_the_input(self):
        schema = GEN.load(ROOT / 'abi/abi.def')
        GEN.validate(schema)
        inputs = sorted(ROOT.glob('plugins/*/native/metadata*.json'))
        inputs = [p for p in inputs if not p.name.endswith('.inspected.json')]
        self.assertTrue(inputs, 'no plugin metadata found')
        for source in inputs:
            with self.subTest(source=str(source.relative_to(ROOT))):
                stem = source.name[: -len('.json')]
                build = GEN.load(source)
                encoded = encode(schema, build)
                bin_path = source.with_name(stem + '.bin')
                self.assertEqual(bin_path.read_bytes(), encoded,
                                 f'{bin_path.name} is stale: regenerate it')
                rendered = json.dumps(decode(schema, encoded), sort_keys=True,
                                      indent=2, ensure_ascii=True) + '\n'
                inspected = source.with_name(stem + '.inspected.json')
                self.assertEqual(inspected.read_text(encoding='utf-8'), rendered,
                                 f'{inspected.name} is stale: regenerate it')


if __name__ == '__main__':
    unittest.main()
