#!/usr/bin/env python3
"""Generate the codec of a plugin's own stage schemas (stages-services.md section 3).

The schemas are `<key>@<major>.def` files in one folder, in the format of `abi/stages`
(without `mirrors`; optional `input_rows` 2..=64), keyed `<plugin>.<name>`, or payload
schemas (`payload`, `major`, `revision`, `max_bytes`, `records`: records carried in a host
frame, one at a time, no reply). The output is one Rust file of borrowed codecs and a
`STAGES` table of the stages, made by the same validators and emitters as the host's stages.
`--check` refuses a stale or missing file and writes nothing.

    python3 tools/stage-codec.py <schemas folder> <output .rs> [--check]
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from abi_stages import load_stages, plugin_stages_source, rustfmt  # noqa: E402


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('schemas', type=Path, help='folder of <key>@<major>.def files')
    parser.add_argument('output', type=Path, help='the generated Rust file')
    parser.add_argument('--check', action='store_true', help='refuse stale output, write nothing')
    args = parser.parse_args(argv)
    try:
        source = rustfmt(plugin_stages_source(load_stages(args.schemas)))
    except (ValueError, KeyError, TypeError) as error:
        print(f'stage-codec: {error}', file=sys.stderr)
        return 1
    if args.check:
        current = args.output.read_text() if args.output.is_file() else None
        if current != source:
            print(f'stage-codec: {args.output} is stale; run tools/stage-codec.py '
                  f'{args.schemas} {args.output}', file=sys.stderr)
            return 1
        return 0
    args.output.write_text(source)
    return 0


if __name__ == '__main__':
    sys.exit(main())
