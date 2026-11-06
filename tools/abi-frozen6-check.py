#!/usr/bin/env python3
"""Compare interface-6 reference bytes with a caller-supplied trusted commit.

This supplements the candidate's semantic compatibility checks. It does not
establish caller provenance or defend against changes to the checker itself.
"""
import argparse
from pathlib import Path
import re
import subprocess

SOURCE = 'crates/paperpaw-sdk/src/native.rs'
DIRECTORY = 'crates/paperpaw-sdk/interface'
SUMMARY = 'crates/paperpaw-sdk/interface.txt'
VERSION = re.compile(rb'^pub const INTERFACE: u16 = ([0-9]+);$', re.M)


def git(root, *arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments], stderr=subprocess.PIPE)


def check(root, base):
    if not re.fullmatch(r'[0-9a-f]{40}', base) or base == '0' * 40:
        raise ValueError('an exact independently trusted base commit is required')
    if git(root, 'rev-parse', base + '^{commit}').decode().strip() != base:
        raise ValueError('base is not the exact trusted commit')
    versions = VERSION.findall((root / SOURCE).read_bytes())
    if len(versions) != 1:
        raise ValueError('cannot establish the candidate native::INTERFACE')
    if int(versions[0]) < 6:
        raise ValueError('legacy interface version cannot move backwards')
    if versions[0] != b'6':
        return 'native::INTERFACE is not 6; the frozen-v6 rule does not apply'
    if VERSION.findall(git(root, 'show', base + ':' + SOURCE)) != [b'6']:
        raise ValueError('interface 6 requires a trusted interface-6 base')
    names = git(root, 'ls-tree', '-r', '--name-only', base, '--', DIRECTORY, SUMMARY)
    trusted = set(names.decode().splitlines())
    if SUMMARY not in trusted or not any(name.startswith(DIRECTORY + '/') for name in trusted):
        raise ValueError('trusted interface-6 reference files are missing')
    current = {SUMMARY}
    current.update(str(path.relative_to(root)) for path in (root / DIRECTORY).rglob('*')
                   if path.is_file() or path.is_symlink())
    if current != trusted:
        raise ValueError('interface-6 reference file set changed against trusted base')
    for name in sorted(trusted):
        path = root / name
        if path.is_symlink() or not path.is_file():
            raise ValueError('interface-6 reference must be a regular file: ' + name)
        if path.read_bytes() != git(root, 'show', base + ':' + name):
            raise ValueError('frozen interface-6 reference changed: ' + name)
    return f'{len(trusted)} interface-6 reference files match trusted base {base}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', required=True, help='trusted PR-base commit, not candidate HEAD')
    args = parser.parse_args()
    try:
        print(check(Path(__file__).resolve().parents[1], args.base))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, 'frozen interface-6 check failed: ' + str(error) + '\n')


if __name__ == '__main__':
    main()
