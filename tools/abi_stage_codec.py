"""Canonical bounded codecs for owner-local stage schemas (no global tags)."""
import re

from abi_records import record_codec_source
from abi_declarations import wire_records



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
    names, tags = set(), set()
    for record in records:
        name, tag = record["name"], record["id"]
        if not re.fullmatch(r"[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+", name):
            raise ValueError("stage record name")
        if name in names or type(tag) is not int or not 1 <= tag <= 65535 or tag in tags:
            raise ValueError("duplicate or invalid local tag/name")
        names.add(name)
        tags.add(tag)
    declared = wire_records(schema)
    if len(declared) != len(records):
        raise ValueError("global registration/command names are not stage records")
    statuses = dict(ERR_INVALID=-2, ERR_UNSUPPORTED=-1, ERR_NO_SPACE=-3, ERR_UTF8=-5)
    return record_codec_source(declared, count, cap, statuses=statuses)
