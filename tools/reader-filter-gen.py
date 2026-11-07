#!/usr/bin/env python3
"""Generate reader.filter v1 using the shared stage codec; --check checks drift."""
import argparse
import json
from pathlib import Path
from abi_stage_codec import stage_codec_source

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
schema = json.loads((root / 'plugins/aids/reader-filter.def').read_text())
source = stage_codec_source(schema)
target = root / 'plugins/aids/src/reader_filter/wire.rs'
if args.check:
    if target.read_text() != source:
        raise SystemExit('reader.filter codec is stale')
else:
    target.write_text(source)
