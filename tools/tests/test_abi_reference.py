"""Trusted Git references keep candidate edits from laundering interface changes."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    'reference', Path(__file__).parents[1] / 'abi-reference-check.py')
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)

API = reference.DIRECTORY + '/sample.api'
SEVEN = reference.blessings_path(7)


def source(interface, draft=None):
    text = f'pub const INTERFACE: u16 = {interface};\n'
    return text if draft is None else text + f'pub const DRAFT: u16 = {draft};\n'


class Fixture(unittest.TestCase):
    """A repository whose base commit has `files`."""
    files = {}

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.run_git('init', '-q')
        for name, content in {
            reference.SUMMARY: 'interface = 6\nfingerprint = kept\n',
            reference.DIRECTORY + '/crates.txt': 'sample\n',
            API: 'pub struct Api\n',
            reference.DIRECTORY + '/sample.shape': 'size=24\n',
            **self.files,
        }.items():
            self.write(name, content)
        self.commit('trusted base')
        self.base = self.run_git('rev-parse', 'HEAD').decode().strip()

    def run_git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.root), *args], stderr=subprocess.PIPE)

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    def commit(self, message):
        self.run_git('add', '-A')
        self.run_git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                     'commit', '-qm', message)

    def check(self):
        return reference.check(self.root, self.base)


class FrozenTests(Fixture):
    files = {reference.SOURCE: source(6, 0)}

    def test_unchanged_reference_bytes_pass(self):
        self.assertIn('4 frozen interface-6 reference files match', self.check())

    def test_reference_changes_are_refused_even_if_committed(self):
        for name in [reference.SUMMARY, API, reference.DIRECTORY + '/crates.txt']:
            with self.subTest(name=name):
                path = self.root / name
                original = path.read_bytes()
                path.write_bytes(original + b'candidate bless\n')
                self.commit('candidate baseline mutation')
                with self.assertRaisesRegex(ValueError, 'frozen interface-6 reference changed'):
                    self.check()
                path.write_bytes(original)

    def test_missing_extra_and_symlink_references_are_refused(self):
        path = self.root / API
        original = path.read_bytes()
        path.unlink()
        with self.assertRaises(ValueError):
            self.check()
        path.write_bytes(original)
        extra = path.with_name('extra.api')
        extra.write_text('candidate-only\n')
        with self.assertRaises(ValueError):
            self.check()
        extra.unlink()
        path.unlink()
        path.symlink_to('sample.shape')
        with self.assertRaisesRegex(ValueError, 'regular file'):
            self.check()

    def test_unknown_zero_symbolic_and_missing_base_fail(self):
        for base in ('HEAD', '0' * 40, 'f' * 40, ''):
            with self.subTest(base=base), \
                    self.assertRaises((ValueError, subprocess.CalledProcessError)):
                reference.check(self.root, base)

    def test_source_cannot_hide_a_constant_in_an_expression(self):
        for text in ['pub const INTERFACE: u16 = 3 + 3;\npub const DRAFT: u16 = 0;\n',
                     'pub const INTERFACE: u16 = 6;\npub const DRAFT: u16 = 0 + 1;\n',
                     'pub const INTERFACE: u16 = 6;\n']:
            with self.subTest(text=text):
                self.write(reference.SOURCE, text)
                with self.assertRaisesRegex(ValueError, 'cannot establish'):
                    self.check()

    def test_leading_zeros_do_not_bypass_a_frozen_reference(self):
        for spelling in ('06', '0006'):
            with self.subTest(spelling=spelling):
                self.write(reference.SOURCE, f'pub const INTERFACE: u16 = {spelling};\n'
                           'pub const DRAFT: u16 = 0;\n')
                self.write(API, 'pub struct Api\n')
                self.assertIn('reference files match', self.check())
                self.write(API, 'candidate changed API\n')
                with self.assertRaisesRegex(ValueError, 'frozen interface-6 reference changed'):
                    self.check()

    def test_a_frozen_interface_never_becomes_a_draft_or_moves_back(self):
        self.write(reference.SOURCE, source(6, 1))
        with self.assertRaisesRegex(ValueError, 'cannot become a draft'):
            self.check()
        self.write(reference.SOURCE, source(5, 0))
        with self.assertRaisesRegex(ValueError, 'backwards'):
            self.check()
        self.write(reference.SOURCE, source(8, 1))
        with self.assertRaisesRegex(ValueError, 'one at a time'):
            self.check()


class RaiseTests(Fixture):
    """The base is from before drafts: interface 6, no `DRAFT`."""
    files = {reference.SOURCE: source(6)}

    def test_a_raise_must_be_named(self):
        self.write(reference.SOURCE, source(7, 1))
        self.write(API, 'pub struct Api7\n')
        with self.assertRaisesRegex(ValueError, 'must name the change'):
            self.check()
        self.write(SEVEN, '# Blessings of interface 7.\n')
        with self.assertRaisesRegex(ValueError, 'gains no line'):
            self.check()
        self.write(SEVEN, '# Blessings of interface 7.\n1 raised\n')
        with self.assertRaisesRegex(ValueError, 'a blessing is'):
            self.check()
        self.write(SEVEN, '2 early raise, draft baseline (#228)\n')
        with self.assertRaisesRegex(ValueError, 'another draft'):
            self.check()
        self.write(SEVEN, '# Blessings of interface 7.\n\n1 early raise, draft baseline (#228)\n')
        self.assertIn('interface 6 raised to 7 draft 1, named (1)', self.check())


class DraftTests(Fixture):
    files = {reference.SOURCE: source(7, 1),
             SEVEN: '1 early raise, draft baseline (#228)\n'}

    def test_an_unchanged_draft_passes(self):
        self.assertIn('draft-1 reference files match', self.check())

    def test_a_changed_draft_must_be_named_by_an_appended_line(self):
        self.write(API, 'pub struct Api2\n')
        with self.assertRaisesRegex(ValueError, 'gains no line'):
            self.check()
        self.write(SEVEN, '1 hyphenation from the card (#228)\n')
        with self.assertRaisesRegex(ValueError, 'only appended'):
            self.check()
        self.write(SEVEN, '1 early raise, draft baseline (#228)\n1 declare module (#180)\n')
        self.assertIn('draft 1 blessed as draft 1, named (1)', self.check())
        self.write(SEVEN, '1 early raise, draft baseline (#228)\n1 declare module (#180)\n'
                   '2 hyphenation from the card (#228)\n')
        with self.assertRaisesRegex(ValueError, 'another draft'):
            self.check()
        self.write(reference.SOURCE, source(7, 2))
        self.assertIn('draft 1 blessed as draft 2, named (2)', self.check())

    def test_a_draft_only_moves_on_or_freezes(self):
        self.write(reference.SOURCE, source(7, 2))
        with self.assertRaisesRegex(ValueError, 'gains no line'):
            self.check()
        self.write(reference.SOURCE, source(7, 0))
        self.write(SEVEN, '1 early raise, draft baseline (#228)\n0 frozen by the owner (#228)\n')
        self.assertIn('blessed as draft 0', self.check())


class LaterDraftTests(Fixture):
    files = {reference.SOURCE: source(7, 3),
             SEVEN: '1 early raise (#228)\n2 a (#1)\n3 b (#2)\n'}

    def test_a_draft_never_moves_back(self):
        self.write(reference.SOURCE, source(7, 2))
        self.write(SEVEN, '1 early raise (#228)\n2 a (#1)\n3 b (#2)\n2 back (#3)\n')
        with self.assertRaisesRegex(ValueError, 'only moves on'):
            self.check()


if __name__ == '__main__':
    unittest.main()
