#!/usr/bin/env python3
"""Compare the plugin interface's reference files with a trusted base commit.

The reference files are `crates/paperpaw-sdk/interface.txt` and the folder
`crates/paperpaw-sdk/interface/` (ADR 0026). Against the caller-supplied
trusted base (the PR's base, or the commit before a push):

- `native::INTERFACE` never moves back and is raised by one at most, and
  `interface.txt` states the same interface and the same draft as
  `native::INTERFACE` and `native::DRAFT`;
- a raise is only allowed from a frozen interface (no `native::DRAFT`: freezing
  deletes it, and an interface from before drafts has none) and lands at draft
  1, never later and never straight at the freeze; freezing is a step of its own;
  the history of the new interface (`blessings.txt`) must not exist yet;
- a frozen interface keeps its reference bytes and file set exactly, and never
  becomes a draft again;
- a draft (`DRAFT`, never 0) stays, moves to the next draft (exactly one more) or
  is frozen, never back and never skipping one. At a draft any reference change
  passes, unnamed: there is one interface 7 until 1.0 (owner decision
  2026-11-10), and the pull request that changes what plugins see carries the
  files `tools/interface.py` and the SDK's bless write;
- a raise and a freeze are named: `abi/interface-<N>/blessings.txt` keeps the
  base's lines as they were (checked every time) and gains at least one, and
  its last line names the candidate's draft (0 for the freeze);
- a frozen interface 7 or later ships its ABI lock released (`released_minor`
  at least the schema's minor in `abi/abi.lock`), so the append-only lock
  checks hold from the freeze on.

A blessing line is `<draft> <what changed> (#<ticket or PR>)`, with draft 0
for the freeze; lines that
start with `#` and blank lines are comments. This supplements the
candidate's semantic compatibility checks (`tools/abi-check.sh`, the SDK's
interface test). It does not establish caller provenance or defend against
changes to the checker itself.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess

SOURCE = 'crates/paperpaw-sdk/src/native.rs'
DIRECTORY = 'crates/paperpaw-sdk/interface'
SUMMARY = 'crates/paperpaw-sdk/interface.txt'
VERSION = re.compile(rb'^pub const INTERFACE: u16 = ([0-9]+);$', re.M)
DRAFT = re.compile(rb'^pub const DRAFT: u16 = ([0-9]+);$', re.M)
DRAFT_NAMED = re.compile(rb'\bconst\s+DRAFT\b')
ENTRY = re.compile(r'([0-9]+) \S.* \(#[0-9]+\)')
SUMMARY_INTERFACE = re.compile(rb'^interface = ([0-9]+)$', re.M)
SUMMARY_DRAFT = re.compile(rb'^draft = ([0-9]+)$', re.M)


def git(root, *arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments], stderr=subprocess.PIPE)


def blessings_path(interface):
    return f'abi/interface-{interface}/blessings.txt'


def constant(pattern, text, name):
    found = pattern.findall(text)
    if len(found) != 1:
        raise ValueError('cannot establish native::' + name)
    return int(found[0])


def draft_of(text):
    """`native::DRAFT`, 0 where there is none (frozen); never one in an expression."""
    if not DRAFT_NAMED.search(text):
        return 0
    draft = constant(DRAFT, text, 'DRAFT')
    if draft == 0:
        raise ValueError('native::DRAFT is never 0: freezing deletes it')
    return draft


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


def summary_values(text):
    """`interface` and `draft` (0 where there is none) that interface.txt states."""
    found = SUMMARY_INTERFACE.findall(text)
    drafts = SUMMARY_DRAFT.findall(text)
    if len(found) != 1 or len(drafts) > 1:
        raise ValueError('cannot establish interface and draft in interface.txt')
    draft = int(drafts[0]) if drafts else 0
    if drafts and draft == 0:
        raise ValueError('draft = 0 in interface.txt: freezing deletes the line')
    return int(found[0]), draft


LOCK = 'abi/abi.lock'


def released_with_the_freeze(root, interface):
    """A frozen interface with a C boundary (7 on) ships its lock released:
    `released_minor` at least the schema's minor, so the append-only checks of
    `tools/abi-gen.py` hold from the freeze on (Senior's M1 on #1055)."""
    if interface < 7:
        return
    path = root / LOCK
    try:
        lock = json.loads(path.read_text(encoding='utf-8'))
        released, minor = lock['released_minor'], lock['schema']['minor']
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise ValueError(f'a frozen interface {interface} needs a readable {LOCK}: {error}')
    if type(released) is not int or type(minor) is not int or released < minor:
        raise ValueError(f'interface {interface} is frozen but {LOCK} has released_minor '
                         f'{released} below the schema minor {minor}: the freeze releases the lock')


def history(root, base, interface, raised=False):
    """`blessings.txt`'s entries now (None where there is no file) and in the
    base; refuses a rewrite of the base's, a removal, and (`raised`) a base
    that has the history already."""
    name = blessings_path(interface)
    path = root / name
    listed = git(root, 'ls-tree', '--name-only', base, '--', name).decode().split()
    if raised and listed:
        raise ValueError(f'{name} exists in the base already; a raise starts the history')
    if path.is_symlink() or (path.exists() and not path.is_file()):
        raise ValueError(name + ' must be a regular file')
    before = entries(git(root, 'show', base + ':' + name).decode()) if listed else []
    if not path.exists():
        if listed:
            raise ValueError(name + ' is gone; blessings are only appended')
        return None, before
    now = entries(path.read_text(encoding='utf-8'))
    if now[:len(before)] != before:
        raise ValueError(name + ' changed lines it kept; blessings are only appended')
    return now, before


def named(interface, history_now, before, draft):
    """The blessing that names this change: appended, ending at `draft`."""
    now, name = history_now, blessings_path(interface)
    if now is None:
        raise ValueError(name + ' must name the change')
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
    draft = draft_of(source)
    base_interface = constant(VERSION, trusted_source, 'INTERFACE')
    base_draft = draft_of(trusted_source)
    if interface < base_interface:
        raise ValueError('native::INTERFACE cannot move backwards')
    if interface > base_interface + 1:
        raise ValueError('native::INTERFACE is raised by one at a time')
    raised = interface > base_interface
    if raised:
        if base_draft != 0:
            raise ValueError(f'interface {base_interface} is still a draft: freeze it before '
                             f'interface {interface} is raised')
        if draft != 1:
            raise ValueError(f'a raise lands at draft 1, not {draft or "frozen"}: freezing is a step of its own')
    elif base_draft == 0:
        if draft != 0:
            raise ValueError(f'interface {interface} is frozen and cannot become a draft again')
    elif draft != 0 and draft < base_draft:
        raise ValueError(f'native::DRAFT moves from {base_draft} to {draft}: a draft only moves on')
    elif draft > base_draft + 1:
        raise ValueError(f'native::DRAFT moves from {base_draft} to {draft}: one draft at a time')
    if draft == 0:
        released_with_the_freeze(root, interface)
    stated = summary_values((root / SUMMARY).read_bytes())
    if stated != (interface, draft):
        raise ValueError(f'{SUMMARY} says interface {stated[0]} draft {stated[1]}, '
                         f'native::INTERFACE {interface} and native::DRAFT {draft} say otherwise')
    trusted, current = references(root, base)
    now, before = history(root, base, interface, raised)
    if raised:
        count = named(interface, now, before, draft)
        return f'interface {base_interface} raised to {interface} draft {draft}, named ({count})'
    if base_draft == 0:
        if changed(root, base, trusted, current):
            raise ValueError(f'frozen interface-{interface} reference changed; only a raise changes it')
        return f'{len(trusted)} frozen interface-{interface} reference files match trusted base {base}'
    if draft == 0:
        count = named(interface, now, before, draft)
        return f'interface {interface} draft {base_draft} moved to frozen, named ({count})'
    if draft != base_draft:
        return f'interface {interface} draft {base_draft} moved to the next draft {draft}'
    if changed(root, base, trusted, current):
        return f'interface {interface} draft {draft}: reference files changed in place (one interface 7)'
    return f'{len(trusted)} interface-{interface} draft-{draft} reference files match trusted base {base}'


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
