#!/usr/bin/env python3
"""Compare the plugin interface's reference files with a trusted base commit.

The reference files are `crates/paperpaw-sdk/interface.txt` and the folder
`crates/paperpaw-sdk/interface/` (ADR 0026). Against the caller-supplied
trusted base (the PR's base, or the commit before a push):

- `native::INTERFACE` never moves back and is raised by one at most;
- a frozen interface (`native::DRAFT` 0 at the base) keeps its reference
  bytes and file set exactly, and never becomes a draft again;
- a draft (`DRAFT` other than 0) moves on to a higher draft or to 0
  (frozen), never back;
- every raise, and every change of a draft's reference files or of
  `DRAFT`, is named: `abi/interface-<N>/blessings.txt` keeps the base's
  lines as they were and gains at least one, and its last line names the
  candidate's draft.

A blessing line is `<draft> <what changed> (#<ticket or PR>)`; lines that
start with `#` and blank lines are comments. This supplements the
candidate's semantic compatibility checks (`tools/abi-check.sh`, the SDK's
interface test). It does not establish caller provenance or defend against
changes to the checker itself.
"""
import argparse
from pathlib import Path
import re
import subprocess

SOURCE = 'crates/paperpaw-sdk/src/native.rs'
DIRECTORY = 'crates/paperpaw-sdk/interface'
SUMMARY = 'crates/paperpaw-sdk/interface.txt'
VERSION = re.compile(rb'^pub const INTERFACE: u16 = ([0-9]+);$', re.M)
DRAFT = re.compile(rb'^pub const DRAFT: u16 = ([0-9]+);$', re.M)
ENTRY = re.compile(r'([0-9]+) \S.* \(#[0-9]+\)')


def git(root, *arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments], stderr=subprocess.PIPE)


def blessings_path(interface):
    return f'abi/interface-{interface}/blessings.txt'


def constant(pattern, text, name, missing=None):
    found = pattern.findall(text)
    if not found and missing is not None:
        return missing
    if len(found) != 1:
        raise ValueError('cannot establish native::' + name)
    return int(found[0])


def entries(text):
    lines = [line for line in text.splitlines() if line.strip() and not line.startswith('#')]
    for line in lines:
        if not ENTRY.fullmatch(line):
            raise ValueError('a blessing is "<draft> <what changed> (#<ticket>)": ' + line)
    return lines


def references(root, base):
    """The reference files of the base and of the candidate, by name."""
    names = git(root, 'ls-tree', '-r', '--name-only', base, '--', DIRECTORY, SUMMARY)
    trusted = set(names.decode().splitlines())
    if SUMMARY not in trusted or not any(name.startswith(DIRECTORY + '/') for name in trusted):
        raise ValueError('trusted reference files are missing')
    current = set()
    for path in [root / SUMMARY, *(root / DIRECTORY).rglob('*')]:
        if path.is_symlink() or (path.exists() and not path.is_file() and not path.is_dir()):
            raise ValueError('a reference must be a regular file: ' + str(path.relative_to(root)))
        if path.is_file():
            current.add(str(path.relative_to(root)))
    return trusted, current


def changed(root, base, trusted, current):
    if current != trusted:
        return True
    return any((root / name).read_bytes() != git(root, 'show', base + ':' + name)
               for name in sorted(trusted))


def named(root, base, interface, draft, raised):
    """The blessing that names this change: appended, ending at `draft`."""
    name = blessings_path(interface)
    path = root / name
    if path.is_symlink() or not path.is_file():
        raise ValueError(name + ' must name the change')
    now = entries(path.read_text(encoding='utf-8'))
    listed = git(root, 'ls-tree', '--name-only', base, '--', name).decode().split()
    before = [] if raised or not listed else entries(git(root, 'show', base + ':' + name).decode())
    if now[:len(before)] != before:
        raise ValueError(name + ' changed lines it kept; blessings are only appended')
    if len(now) == len(before):
        raise ValueError(name + ' gains no line naming the change')
    if int(ENTRY.fullmatch(now[-1]).group(1)) != draft:
        raise ValueError(f'the last line of {name} names another draft than native::DRAFT {draft}')
    return len(now) - len(before)


def check(root, base):
    if not re.fullmatch(r'[0-9a-f]{40}', base) or base == '0' * 40:
        raise ValueError('an exact independently trusted base commit is required')
    if git(root, 'rev-parse', base + '^{commit}').decode().strip() != base:
        raise ValueError('base is not the exact trusted commit')
    source = (root / SOURCE).read_bytes()
    trusted_source = git(root, 'show', base + ':' + SOURCE)
    interface = constant(VERSION, source, 'INTERFACE')
    draft = constant(DRAFT, source, 'DRAFT')
    base_interface = constant(VERSION, trusted_source, 'INTERFACE')
    # Before drafts existed, every interface was frozen.
    base_draft = constant(DRAFT, trusted_source, 'DRAFT', missing=0)
    if interface < base_interface:
        raise ValueError('native::INTERFACE cannot move backwards')
    if interface > base_interface + 1:
        raise ValueError('native::INTERFACE is raised by one at a time')
    trusted, current = references(root, base)
    if interface > base_interface:
        count = named(root, base, interface, draft, raised=True)
        return f'interface {base_interface} raised to {interface} draft {draft}, named ({count})'
    differs = changed(root, base, trusted, current)
    if base_draft == 0:
        if draft != 0:
            raise ValueError(f'interface {interface} is frozen and cannot become a draft again')
        if differs:
            raise ValueError(f'frozen interface-{interface} reference changed; only a raise changes it')
        return f'{len(trusted)} frozen interface-{interface} reference files match trusted base {base}'
    if draft != base_draft and draft != 0 and draft < base_draft:
        raise ValueError(f'native::DRAFT moves from {base_draft} to {draft}: a draft only moves on')
    if not differs and draft == base_draft:
        return f'{len(trusted)} interface-{interface} draft-{draft} reference files match trusted base {base}'
    count = named(root, base, interface, draft, raised=False)
    return f'interface {interface} draft {base_draft} blessed as draft {draft}, named ({count})'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', required=True, help='trusted PR-base commit, not candidate HEAD')
    args = parser.parse_args()
    try:
        print(check(Path(__file__).resolve().parents[1], args.base))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, 'interface reference check failed: ' + str(error) + '\n')


if __name__ == '__main__':
    main()
