#!/usr/bin/env python3
"""Generate the codec of a plugin's own stage schemas (stages-services.md section 3).

The schemas are `<key>@<major>.def` files in one folder, in the format of `abi/stages`
(without `mirrors`; optional `input_rows` 2..=64), keyed `<plugin>.<name>`, or payload
schemas (`payload`, `major`, `revision`, `max_bytes`, `records`: records carried in a host
frame, one at a time, no reply). The output is one Rust file of borrowed codecs and a
`STAGES` table of the stages, made by the same validators and emitters as the host's stages.
`--check` refuses a stale or missing file and writes nothing.

    python3 tools/stage-codec.py <schemas folder> <output .rs> [--check] [--plugin <id>]

`--plugin <id>` also refuses a schema whose key is not `<id>.<name>`. Output is formatted with
the repository's pinned rustfmt, so `--check` agrees on every machine.
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from abi_stages import load_stages, plugin_stages_source, rustfmt  # noqa: E402


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('schemas', type=Path, help='folder of <key>@<major>.def files')
    parser.add_argument('output', type=Path, help='the generated Rust file')
    parser.add_argument('--check', action='store_true', help='refuse stale output, write nothing')
    parser.add_argument('--plugin', help='the owning plugin id: every key must be <id>.<name>')
    args = parser.parse_args(argv)
    try:
        stages = load_stages(args.schemas)
        if args.plugin is not None:
            check_owner(stages, args.plugin)
        source = rustfmt(plugin_stages_source(stages))
        current = None
        if args.check and args.output.is_file():
            current = args.output.read_text()
    except (ValueError, KeyError, TypeError) as error:
        print(f'stage-codec: {error}', file=sys.stderr)
        return 1
    except OSError as error:
        print(f'stage-codec: {error.filename or args.schemas}: {error.strerror}', file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as error:
        print(f'stage-codec: rustfmt failed: {error.stderr.strip()}', file=sys.stderr)
        return 1
    if args.check:
        if current != source:
            print(f'stage-codec: {args.output} is stale; run tools/stage-codec.py '
                  f'{args.schemas} {args.output}', file=sys.stderr)
            return 1
        return 0
    try:
        args.output.write_text(source)
    except OSError as error:
        print(f'stage-codec: cannot write {args.output}: {error.strerror}', file=sys.stderr)
        return 1
    return 0


def check_owner(stages, plugin):
    """Every schema key (a stage's `key`, a payload's `payload`) is `<plugin>.<name>`."""
    if not re.fullmatch(r'[a-z0-9-]{1,40}', plugin):
        raise ValueError(f'--plugin {plugin!r} is not a plugin id ([a-z0-9-], 1-40)')
    for stage in stages:
        key = stage.get('key', stage.get('payload')) if type(stage) is dict else None
        if type(key) is not str or not key.startswith(plugin + '.'):
            raise ValueError(f'schema {key!r} is not owned by plugin {plugin!r}')


if __name__ == '__main__':
    sys.exit(main())
