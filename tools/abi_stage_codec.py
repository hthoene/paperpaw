"""Canonical bounded codecs for owner-local stage schemas (no global tags)."""
import re
import json
from pathlib import Path

from abi_records import record_codec_source
from abi_declarations import wire_records, _camel, _const



def stage_codec_source(schema):
    """Generate a no_std, allocation-free codec with exact owner-local tags."""
    cap, count = schema["max_bytes"], schema["max_records"]
    if type(cap) is not int or not 1 <= cap <= 4096:
        raise ValueError("stage byte cap")
    if type(count) is not int or not 1 <= count <= 64:
        raise ValueError("stage record cap")
    records = schema["records"]
    if not records or len(records) > 256:
        raise ValueError("stage schema record count")
    names, tags, rust_types, rust_consts = set(), set(), set(), set()
    for record in records:
        name, tag = record["name"], record["id"]
        if not re.fullmatch(r"[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+", name):
            raise ValueError("stage record name")
        if name in names or type(tag) is not int or not 1 <= tag <= 65535 or tag in tags:
            raise ValueError("duplicate or invalid local tag/name")
        if _camel(name) in rust_types or _const(name) in rust_consts:
            raise ValueError("colliding Rust stage record names")
        rust_types.add(_camel(name))
        rust_consts.add(_const(name))
        names.add(name)
        tags.add(tag)
    declared = wire_records(schema)
    if len(declared) != len(records):
        raise ValueError("global registration/command names are not stage records")
    abi = json.loads((Path(__file__).resolve().parents[1] / 'abi/abi.def').read_text())
    statuses = {item['name']: item['value'] for item in abi['statuses']}
    return record_codec_source(declared, count, cap, statuses=statuses)
