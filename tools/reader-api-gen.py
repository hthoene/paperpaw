#!/usr/bin/env python3
"""Generate the reader-owned payload codec; --check refuses stale output."""
import argparse
import json
from pathlib import Path

from abi_stage_codec import stage_codec_source

ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates/paperpaw-reader-api"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    schema = json.loads((CRATE / "reader.def").read_text())
    records = {}
    for path in schema["schemas"]:
        local = json.loads((CRATE / path).read_text())
        if local["schema"] != schema["version"]:
            raise ValueError("unsupported reader schema version")
        for record in local["records"]:
            tag = record["id"]
            if tag in records and record != records[tag]:
                raise ValueError("inconsistent shared reader record")
            records[tag] = record
    schema["records"] = [records[tag] for tag in sorted(records)]
    for key in ('max_block_inserts', 'max_chapter_inserts'):
        if type(schema[key]) is not int or not 1 <= schema[key] <= 64:
            raise ValueError('invalid reader insert cap')
    if schema['max_block_inserts'] > schema['max_records']:
        raise ValueError('insert reply exceeds record cap')
    source = stage_codec_source(schema)
    source += (f"\npub const MAX_BLOCK_INSERTS: usize = {schema['max_block_inserts']};\n"
               f"pub const MAX_CHAPTER_INSERTS: u32 = {schema['max_chapter_inserts']};\n")
    target = CRATE / "src/wire.rs"
    if args.check:
        if not target.exists() or target.read_text() != source:
            raise SystemExit("reader codec is stale")
    else:
        target.write_text(source)


if __name__ == "__main__":
    main()
