"""Compile probes for borrowed ABI frame views, independent of Cargo artifacts."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
MODULE = ROOT / 'crates/paperpaw-core/src/abi7/context.rs'
PREFIX = '''#![allow(dead_code, reason = "compile-only lifetime probe")]
extern crate self as paperpaw_sdk;
extern crate self as paperpaw_i18n;
pub struct Settings;
pub struct Translator;
#[path = MODULE_PATH]
mod context;
'''.replace('MODULE_PATH', json.dumps(str(MODULE)))


class FrameLifetimes(unittest.TestCase):
    def test_owned_results_compile_but_frame_references_cannot_escape(self):
        cases = [
            ('owned', 'fn valid(s: &context::FrameSlot) { '
             'let _: Result<usize, _> = s.with(|_| 7); }', True),
            ('frame', 'fn escape(s: &context::FrameSlot) -> &dyn context::Frame { '
             's.with(|f| f).unwrap() }', False),
            ('derived', 'fn escape(s: &context::FrameSlot) -> &Settings { '
             's.with(|f| f.settings()).unwrap() }', False),
            ('capture', 'fn escape(s: &context::FrameSlot) { let mut out = None; '
             's.with(|f| out = Some(f)).unwrap(); let _ = out; }', False),
        ]
        with tempfile.TemporaryDirectory(prefix='paperpaw-frame-') as directory:
            folder = Path(directory)
            for name, body, accepted in cases:
                with self.subTest(case=name):
                    source = folder / (name + '.rs')
                    source.write_text(PREFIX + body)
                    result = subprocess.run(
                        ['rustc', '--edition', '2024', '--crate-type', 'lib',
                         str(source), '--out-dir', str(folder)],
                        capture_output=True, text=True, check=False)
                    if accepted:
                        self.assertEqual(result.returncode, 0, result.stderr)
                    else:
                        self.assertNotEqual(result.returncode, 0, 'borrow escaped')
                        self.assertRegex(result.stderr, r'lifetime may not live long enough|E0521')


if __name__ == '__main__':
    unittest.main()
