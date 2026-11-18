#!/usr/bin/env python3
"""Emit draft ABI metadata bytes and inspected JSON, without signing artifacts."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

from abi_metadata import decode, encode


SPEC = importlib.util.spec_from_file_location('abi_gen', Path(__file__).with_name('abi-gen.py'))
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


def run(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--schema', type=Path, required=True)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--bytes', type=Path, required=True)
    parser.add_argument('--json', type=Path, required=True)
    args = parser.parse_args(argv)
    paths = [path.resolve() for path in (args.schema, args.input, args.bytes, args.json)]
    if len(set(paths)) != len(paths):
        raise ValueError('schema, input, bytes and JSON paths must be distinct')
    # Resolve symlinks above and detect existing hard links as well. Inputs must
    # survive a failed build, and neither output may replace the other.
    for index, path in enumerate(paths):
        for other in paths[index + 1:]:
            if path.exists() and other.exists() and path.samefile(other):
                raise ValueError('input and output paths must name distinct files')
    schema, build = GENERATOR.load(args.schema), GENERATOR.load(args.input)
    GENERATOR.validate(schema)
    encoded = encode(schema, build)
    inspected = decode(schema, encoded)
    rendered = json.dumps(inspected, sort_keys=True, indent=2, ensure_ascii=True) + '\n'
    # Validate both outputs completely before opening either destination.
    args.bytes.write_bytes(encoded)
    args.json.write_text(rendered, encoding='utf-8')


if __name__ == '__main__':
    try:
        run()
    except (OSError, ValueError) as error:
        print(f'abi-metadata: {error}', file=sys.stderr)
        sys.exit(1)
