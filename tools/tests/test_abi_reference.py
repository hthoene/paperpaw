"""Trusted Git references keep candidate edits from laundering interface changes."""
import importlib.util
from pathlib import Path
import json
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


def summary(interface, draft=None, fingerprint='kept'):
    """interface.txt: `draft` is a line only while there is one."""
    text = f'interface = {interface}\n'
    return text + (f'draft = {draft}\n' if draft is not None else '') + f'fingerprint = {fingerprint}\n'


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
    files = {reference.SOURCE: source(6)}

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
        for text in ['pub const INTERFACE: u16 = 3 + 3;\n',
                     'pub const INTERFACE: u16 = 6;\npub const DRAFT: u16 = 0 + 1;\n',
                     'pub const INTERFACE: u16 = 6;\npub(crate) const DRAFT: u16 = 1;\n',
                     'pub const INTERFACE: u16 = 6;\npub const DRAFT: u16 = 1;\n'
                     'pub const DRAFT: u16 = 1;\n']:
            with self.subTest(text=text):
                self.write(reference.SOURCE, text)
                with self.assertRaisesRegex(ValueError, 'cannot establish'):
                    self.check()

    def test_leading_zeros_do_not_bypass_a_frozen_reference(self):
        for spelling in ('06', '0006'):
            with self.subTest(spelling=spelling):
                self.write(reference.SOURCE, f'pub const INTERFACE: u16 = {spelling};\n')
                self.write(API, 'pub struct Api\n')
                self.assertIn('reference files match', self.check())
                self.write(API, 'candidate changed API\n')
                with self.assertRaisesRegex(ValueError, 'frozen interface-6 reference changed'):
                    self.check()

    def test_a_frozen_interface_never_becomes_a_draft_or_moves_back(self):
        self.write(reference.SOURCE, source(6, 1))
        with self.assertRaisesRegex(ValueError, 'cannot become a draft'):
            self.check()
        self.write(reference.SOURCE, source(6, 0))
        with self.assertRaisesRegex(ValueError, 'never 0'):
            self.check()
        self.write(reference.SOURCE, source(5))
        with self.assertRaisesRegex(ValueError, 'backwards'):
            self.check()
        self.write(reference.SOURCE, source(8))
        with self.assertRaisesRegex(ValueError, 'one at a time'):
            self.check()


class RaiseTests(Fixture):
    """The base is from before drafts: interface 6, no `DRAFT`."""
    files = {reference.SOURCE: source(6)}

    def raise_to(self, interface=7, draft=1, stated=None):
        self.write(reference.SOURCE, source(interface, draft))
        self.write(reference.SUMMARY, summary(interface, draft if stated is None else stated))

    def test_a_raise_must_be_named(self):
        self.raise_to()
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

    def test_a_raise_lands_at_draft_1_never_later_or_frozen(self):
        self.write(SEVEN, '2 early raise (#228)\n')
        self.raise_to(draft=2)
        with self.assertRaisesRegex(ValueError, 'lands at draft 1, not 2'):
            self.check()
        self.write(SEVEN, '0 raise and freeze (#228)\n')
        self.write(reference.SOURCE, source(7))
        self.write(reference.SUMMARY, summary(7))
        with self.assertRaisesRegex(ValueError, 'lands at draft 1, not frozen'):
            self.check()

    def test_interface_txt_must_say_what_the_source_says(self):
        self.write(SEVEN, '1 early raise (#228)\n')
        for stated, text in [(2, 'a draft that is not the source\'s'), (None, 'no draft line')]:
            with self.subTest(text=text):
                self.write(reference.SOURCE, source(7, 1))
                self.write(reference.SUMMARY, summary(7, stated))
                with self.assertRaisesRegex(ValueError, 'says interface 7 draft'):
                    self.check()
        self.write(reference.SUMMARY, summary(6, 1))
        with self.assertRaisesRegex(ValueError, 'says interface 6 draft 1'):
            self.check()
        self.write(reference.SUMMARY, 'interface = 7\ninterface = 7\ndraft = 1\n')
        with self.assertRaisesRegex(ValueError, 'cannot establish'):
            self.check()
        self.write(reference.SUMMARY, summary(7, 0))
        with self.assertRaisesRegex(ValueError, 'freezing deletes the line'):
            self.check()
        self.write(reference.SUMMARY, summary(7, 1))
        self.assertIn('raised to 7 draft 1', self.check())


class StaleHistoryTests(Fixture):
    """The base has a history of interface 7 already, before its raise."""
    files = {reference.SOURCE: source(6), SEVEN: '1 stale (#1)\n'}

    def test_a_raise_refuses_an_existing_history(self):
        self.write(reference.SOURCE, source(7, 1))
        self.write(reference.SUMMARY, summary(7, 1))
        self.write(SEVEN, '1 stale (#1)\n1 early raise (#228)\n')
        with self.assertRaisesRegex(ValueError, 'exists in the base already'):
            self.check()


class DraftTests(Fixture):
    files = {reference.SOURCE: source(7, 1),
             reference.SUMMARY: summary(7, 1),
             API: 'pub struct Api\npub struct Other\n',
             SEVEN: '1 early raise, draft baseline (#228)\n'}

    def named(self, *lines):
        self.write(SEVEN, '1 early raise, draft baseline (#228)\n' + ''.join(line + '\n' for line in lines))

    def test_an_unchanged_draft_passes(self):
        self.assertIn('draft-1 reference files match', self.check())

    def test_any_change_passes_in_place_unnamed(self):
        """One interface 7 until 1.0: additions, removals, changed or reordered
        lines, a removed file and another fingerprint all pass at the draft."""
        changes = [(API, 'pub struct Api\npub struct Added\npub struct Other\n'),
                   (API, 'pub struct Other\npub struct Api\n'),
                   (API, 'pub struct Other\n'),
                   (reference.SUMMARY, summary(7, 1, fingerprint='other')),
                   (reference.DIRECTORY + '/new.api', 'pub struct New\n')]
        for name, text in changes:
            with self.subTest(name=name, text=text):
                self.write(name, text)
                self.assertIn('changed in place', self.check())
        (self.root / API).unlink()
        self.assertIn('changed in place', self.check())

    def test_a_history_rewrite_is_refused_even_where_nothing_else_changed(self):
        original = (self.root / SEVEN).read_text()
        for text in ['1 early raise, rewritten (#228)\n', '', '1 early raise, draft baseline (#228)\n'
                     '1 first (#1)\n']:
            with self.subTest(text=text):
                self.write(SEVEN, text)
                if text.startswith('1 early raise, draft baseline'):
                    # Only an appended line, with nothing else changed: harmless.
                    self.assertIn('draft-1 reference files match', self.check())
                else:
                    with self.assertRaisesRegex(ValueError, 'only appended'):
                        self.check()
        (self.root / SEVEN).unlink()
        with self.assertRaisesRegex(ValueError, 'is gone'):
            self.check()
        self.write(SEVEN, original)
        self.assertIn('draft-1 reference files match', self.check())

    def test_the_next_draft_needs_no_name(self):
        self.write(reference.SOURCE, source(7, 2))
        self.write(reference.SUMMARY, summary(7, 2, fingerprint='moved'))
        self.write(API, 'pub struct Api2\n')
        self.assertIn('moved to the next draft 2', self.check())

    def test_a_draft_cannot_skip_one_or_be_stated_wrongly(self):
        self.write(reference.SOURCE, source(7, 3))
        self.write(reference.SUMMARY, summary(7, 3))
        self.named('3 skipped (#1)')
        with self.assertRaisesRegex(ValueError, 'one draft at a time'):
            self.check()
        self.write(reference.SOURCE, source(7, 2))
        with self.assertRaisesRegex(ValueError, 'says interface 7 draft 3'):
            self.check()

    def test_a_raise_needs_a_frozen_interface(self):
        self.write(reference.SOURCE, source(8, 1))
        self.write(reference.SUMMARY, summary(8, 1))
        self.write(reference.blessings_path(8), '1 raise (#1)\n')
        with self.assertRaisesRegex(ValueError, 'still a draft'):
            self.check()

    def lock(self, released, minor=1):
        self.write(reference.LOCK, json.dumps({'released_minor': released,
                                               'schema': {'minor': minor}}))

    def test_the_freeze_releases_the_lock(self):
        self.write(reference.SOURCE, source(7))
        self.write(reference.SUMMARY, summary(7))
        self.named('0 frozen by the owner (#228)')
        for content in [None, '{', '{"released_minor": 0, "schema": {"minor": 1}}',
                        '{"released_minor": 1, "schema": {"minor": 2}}']:
            with self.subTest(content=content):
                if content is None:
                    (self.root / reference.LOCK).unlink(missing_ok=True)
                else:
                    self.write(reference.LOCK, content)
                with self.assertRaisesRegex(ValueError, 'frozen'):
                    self.check()
        self.lock(released=1)
        self.assertIn('moved to frozen', self.check())
        # A draft needs no released lock.
        self.write(reference.SOURCE, source(7, 1))
        self.write(reference.SUMMARY, summary(7, 1))
        self.lock(released=0)
        self.assertIn('reference files', self.check())

    def test_freezing_is_its_own_named_step(self):
        self.write(reference.SOURCE, source(7))
        self.write(reference.SUMMARY, summary(7))
        self.lock(released=1)
        with self.assertRaisesRegex(ValueError, 'gains no line'):
            self.check()
        self.named('0 frozen by the owner (#228)')
        self.assertIn('moved to frozen, named (1)', self.check())
        self.commit('frozen')
        self.base = self.run_git('rev-parse', 'HEAD').decode().strip()
        self.assertIn('frozen interface-7 reference files match', self.check())
        self.write(API, 'pub struct Api3\n')
        self.named('0 frozen by the owner (#228)', '0 after the freeze (#1)')
        with self.assertRaisesRegex(ValueError, 'frozen interface-7 reference changed'):
            self.check()
        self.write(reference.SOURCE, source(7, 1))
        with self.assertRaisesRegex(ValueError, 'cannot become a draft'):
            self.check()


class LaterDraftTests(Fixture):
    files = {reference.SOURCE: source(7, 3), reference.SUMMARY: summary(7, 3),
             SEVEN: '1 early raise (#228)\n2 a (#1)\n3 b (#2)\n'}

    def test_a_draft_never_moves_back(self):
        self.write(reference.SOURCE, source(7, 2))
        self.write(reference.SUMMARY, summary(7, 2))
        self.write(SEVEN, '1 early raise (#228)\n2 a (#1)\n3 b (#2)\n2 back (#3)\n')
        with self.assertRaisesRegex(ValueError, 'only moves on'):
            self.check()


if __name__ == '__main__':
    unittest.main()
