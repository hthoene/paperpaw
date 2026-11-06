"""Trusted Git references prevent candidate baseline edits laundering changes."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('frozen6', Path(__file__).parents[1] / 'abi-frozen6-check.py')
frozen6 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(frozen6)


class Frozen6Tests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.run_git('init', '-q')
        self.files = {
            frozen6.SOURCE: 'pub const INTERFACE: u16 = 6;\n',
            frozen6.SUMMARY: 'interface = 6\nfingerprint = kept\n',
            frozen6.DIRECTORY + '/crates.txt': 'sample\n',
            frozen6.DIRECTORY + '/sample.api': 'pub struct Api\n',
            frozen6.DIRECTORY + '/sample.shape': 'size=24\n',
        }
        for name, content in self.files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        self.run_git('add', '.')
        self.run_git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                     'commit', '-qm', 'trusted base')
        self.base = self.run_git('rev-parse', 'HEAD').decode().strip()

    def run_git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.root), *args], stderr=subprocess.PIPE)

    def test_unchanged_reference_bytes_pass(self):
        self.assertIn('4 interface-6 reference files match', frozen6.check(self.root, self.base))

    def test_candidate_reference_changes_are_refused_even_if_committed(self):
        for name in self.files:
            if name == frozen6.SOURCE:
                continue
            with self.subTest(name=name):
                path = self.root / name
                original = path.read_bytes()
                path.write_bytes(original + b'candidate bless\n')
                self.run_git('add', name)
                self.run_git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                             'commit', '-qm', 'candidate baseline mutation')
                with self.assertRaisesRegex(ValueError, 'frozen interface-6 reference changed'):
                    frozen6.check(self.root, self.base)
                path.write_bytes(original)

    def test_missing_extra_and_symlink_references_are_refused(self):
        path = self.root / (frozen6.DIRECTORY + '/sample.api')
        original = path.read_bytes()
        path.unlink()
        with self.assertRaises(ValueError):
            frozen6.check(self.root, self.base)
        path.write_bytes(original)
        extra = path.with_name('extra.api')
        extra.write_text('candidate-only\n')
        with self.assertRaises(ValueError):
            frozen6.check(self.root, self.base)
        extra.unlink()
        path.unlink()
        path.symlink_to('sample.shape')
        with self.assertRaisesRegex(ValueError, 'regular file'):
            frozen6.check(self.root, self.base)

    def test_unknown_zero_symbolic_and_missing_base_fail(self):
        for base in ('HEAD', '0' * 40, 'f' * 40, ''):
            with self.subTest(base=base), self.assertRaises((ValueError, subprocess.CalledProcessError)):
                frozen6.check(self.root, base)

    def test_source_cannot_hide_interface_in_an_unrecognized_expression(self):
        (self.root / frozen6.SOURCE).write_text('pub const INTERFACE: u16 = 3 + 3;\n')
        with self.assertRaisesRegex(ValueError, 'cannot establish'):
            frozen6.check(self.root, self.base)

    def test_coordinated_actual_interface_raise_has_explicit_result(self):
        (self.root / frozen6.SOURCE).write_text('pub const INTERFACE: u16 = 7;\n')
        self.assertIn('rule does not apply', frozen6.check(self.root, self.base))


if __name__ == '__main__':
    unittest.main()
