"""Offline validation of one ascii/cold/clean/raster textprobe invocation.

Schema: shells/xteink/src/textprobe/mod.rs at 3e3b38ff73c83297d7767507696fd0dfa2be1ce8.
No device access. Admission alignment describes the reserve block only, and
painted stack depth does not establish exact minimum SP or renderer alignment.
"""

import math
import re
import struct


ASCII_SHA256 = "c93eee2d0db02f10acc7460d9576e122dcf8cd53c4bf8dfcae1b3e74ebcfff5a"
PATHS = ("control", "direct", "adapter")
MIN_STACK = 10240
MAX_PEAK = 222822
MIN_FREE = 39322
_ORDER = ("BEGIN", "STACK_SETUP", "SAMPLE", "BEFORE", "AFTER", "RESULT",
          "STACK", "LIMITS", "END", "TRANSPORT_END")
_FIELDS = {
    "BEGIN": "version case path cache heap mode fixture font repeats",
    "STACK_SETUP": "requested charged size",
    "SAMPLE": "repeat",
    "BEFORE": "used free largest stack_left stack_size total boot_peak",
    "AFTER": "used free largest stack_left stack_size total boot_peak",
    "RESULT": ("version repeat status advance_bits pixels baseline_used end_used "
               "peak_used allocations requested_bytes largest_request deallocations "
               "freed_requested_bytes failed_allocations elapsed_us overflow "
               "scope_valid min_free retained_delta"),
    "STACK": ("repeat method total unused used guard_valid irq_masked irq_body_us "
              "admission_requested admission_align admission_charged admission_released"),
    "LIMITS": ("stack_scope_supported request_alignment semantic_hash "
               "probe_resolution heap_profile warmups"),
    "END": "",
    "TRANSPORT_END": "",
}
_STRINGS = {"case", "path", "cache", "heap", "mode", "fixture", "font",
            "status", "pixels", "method", "stack_scope_supported",
            "request_alignment", "semantic_hash", "heap_profile"}
_FATAL_LOG = re.compile(
    r"\b(reset|panic|reboot(?:ing)?|brownout|watchdog|disconnected|disconnect|"
    r"timeout|EOF)\b|rst:|ESP-ROM:|Guru Meditation|assert(?:ion)? failed|"
    r"abort\(\)|connection lost|link lost|timed out|serial.*closed", re.IGNORECASE)


class ProbeError(ValueError):
    """Invalid, failed, or out-of-scope probe stream."""


def _require(condition, message):
    if not condition:
        raise ProbeError(message)


def _integer(value, field):
    signed = field == "retained_delta"
    _require(re.fullmatch(r"-?[0-9]+" if signed else r"[0-9]+", value),
             f"invalid integer: {field}")
    number = int(value)
    bits = 64 if field in ("elapsed_us", "irq_body_us", "retained_delta") else 32
    lower, upper = (-(1 << 63), (1 << 63) - 1) if signed else (0, (1 << bits) - 1)
    _require(lower <= number <= upper, f"integer out of range: {field}")
    return number


def _fields(kind, tokens):
    record = {}
    expected = set(_FIELDS[kind].split())
    for token in tokens:
        _require(token.count("=") == 1, "malformed field")
        key, value = token.split("=")
        _require(key not in record, f"duplicate field: {key}")
        _require(key in expected and value, f"unknown or empty field: {key}")
        record[key] = value if key in _STRINGS else _integer(value, key)
    _require(set(record) == expected, f"missing fields in {kind}")
    return record


def _fixed(record, expected):
    for key, value in expected.items():
        _require(record[key] == value, f"unexpected {key}: {record[key]}")


class Probe:
    """Feed complete text lines; only the final PP:END returns True.

    ``result`` is None until both end markers validate. Errors are terminal.
    No EOF is implicit: a caller must treat result=None as incomplete at EOF.
    """

    def __init__(self, path):
        _require(path in PATHS, "unsupported path")
        self.path = path
        self.result = None
        self._records = {}
        self._index = 0
        self._failed = False
        self._advisories = []

    def feed(self, line):
        _require(not self._failed, "probe already failed")
        try:
            return self._feed(line)
        except ValueError:
            self._failed = True
            self.result = None
            raise

    def finish(self):
        """Return the validated result; an EOF never substitutes for a marker."""
        if self.result is None:
            self._failed = True
        _require(not self._failed and self.result is not None,
                 "incomplete or failed probe: both end markers required")
        return self.result

    def _feed(self, line):
        _require(isinstance(line, str) and len(line) <= 4096, "invalid or oversized line")
        line = line.rstrip("\r\n")
        _require("\n" not in line and "\r" not in line, "expected one complete line")
        _require(not _FATAL_LOG.search(line), "STOP: reset/panic/loss indicator")
        if not line.startswith("PP:"):
            _require("PP:" not in line, "embedded protocol record")
            return False
        _require(line.isascii() and all(char >= " " or char == "\t" for char in line),
                 "invalid protocol characters")
        _require(self._index < len(_ORDER), "record after transport end")
        tokens = line.split()
        _require(tokens and tokens[0].isascii(), "invalid protocol record")
        name = tokens[0]
        kind = _ORDER[self._index]
        expected_name = "PP:END" if kind == "TRANSPORT_END" else "PP:TEXTPROBE_" + kind
        _require(name == expected_name, f"unexpected record: {name}")
        record = _fields(kind, tokens[1:])
        self._validate(kind, record)
        self._records[kind.lower()] = record
        self._index += 1
        if kind != "TRANSPORT_END":
            return False
        self.result = {key: value for key, value in self._records.items()
                       if key not in ("end", "transport_end")}
        self.result.update(path=self.path, advisories=list(self._advisories))
        self.result["setup"] = self._records["stack_setup"]
        return True

    def _validate(self, kind, record):
        if kind == "BEGIN":
            _fixed(record, dict(version=1, case="ascii", path=self.path, cache="cold",
                                heap="clean", mode="raster", fixture=ASCII_SHA256,
                                font="synthetic-all", repeats=1))
        elif kind == "STACK_SETUP":
            _fixed(record, dict(requested=24704, size=24576))
            _require(record["charged"] >= record["requested"], "stack charge below request")
        elif kind in ("BEFORE", "AFTER"):
            self._snapshot(record)
        elif kind == "SAMPLE":
            _fixed(record, dict(repeat=0))
        elif kind == "RESULT":
            self._sample(record)
        elif kind == "STACK":
            self._stack(record)
        elif kind == "LIMITS":
            _fixed(record, dict(stack_scope_supported="painted-depth-only",
                                request_alignment="unsupported", semantic_hash="unsupported",
                                probe_resolution=64, heap_profile="clean", warmups=0))

    def _snapshot(self, record):
        _require(record["used"] + record["free"] == record["total"], "heap total mismatch")
        _require(record["largest"] <= record["free"], "largest block exceeds free heap")
        _require(record["used"] <= record["boot_peak"] <= record["total"], "invalid boot peak")
        _require(MIN_STACK <= record["stack_left"] <= record["stack_size"],
                 "normal stack headroom below 10240 or invalid size")
        _require(record["used"] <= MAX_PEAK and record["free"] >= MIN_FREE,
                 "snapshot heap headroom exceeded")
        _require(self._records["stack_setup"]["charged"] <= record["used"],
                 "dedicated stack missing from absolute heap accounting")
        if "before" in self._records:
            before = self._records["before"]
            _require(record["total"] == before["total"], "heap total changed")
            _require(record["stack_size"] == before["stack_size"], "normal stack size changed")
            _require(record["boot_peak"] >= before["boot_peak"], "boot peak decreased")

    def _sample(self, record):
        _fixed(record, dict(version=1, repeat=0, status="ok", failed_allocations=0,
                            overflow=0, scope_valid=1))
        advance = struct.unpack("!f", struct.pack("!I", record["advance_bits"]))[0]
        _require(math.isfinite(advance) and advance >= 0, "invalid advance bits")
        _require(re.fullmatch(r"[0-9a-f]{64}", record["pixels"]), "invalid pixel hash")
        before, after = self._records["before"], self._records["after"]
        _require(record["baseline_used"] == before["used"], "baseline differs from BEFORE")
        _require(record["end_used"] == after["used"], "end differs from AFTER")
        peak = record["peak_used"]
        _require(max(record["baseline_used"], record["end_used"]) <= peak <= before["total"],
                 "invalid scope peak")
        _require(peak <= after["boot_peak"], "scope peak exceeds boot peak")
        _require(record["min_free"] == before["total"] - peak, "min_free mismatch")
        _require(peak <= MAX_PEAK and record["min_free"] >= MIN_FREE,
                 "scope heap headroom exceeded")
        _require(record["retained_delta"] == record["end_used"] - record["baseline_used"],
                 "retained delta mismatch")
        _require(record["largest_request"] <= record["requested_bytes"], "largest request exceeds traffic")
        if record["allocations"] == 0:
            _require(record["requested_bytes"] == record["largest_request"] == 0,
                     "allocation traffic without allocations")
            _require(peak == record["baseline_used"], "peak grew without allocations")
        else:
            _require(record["largest_request"] > 0, "allocations without requested bytes")
            _require(record["allocations"] <= record["requested_bytes"]
                     <= record["allocations"] * record["largest_request"],
                     "inconsistent allocation traffic")
        _require((record["deallocations"] == 0) == (record["freed_requested_bytes"] == 0),
                 "inconsistent deallocation traffic")
        _require(record["freed_requested_bytes"] >= record["deallocations"],
                 "freed traffic below deallocation count")

    def _stack(self, record):
        _fixed(record, dict(repeat=0, method="paint-dedicated", total=24576, guard_valid=1,
                            irq_masked=1, admission_requested=98304, admission_align=4,
                            admission_released=1))
        _require(record["unused"] + record["used"] == record["total"], "stack total mismatch")
        _require(record["unused"] >= MIN_STACK, "dedicated stack headroom below 10240")
        _require(record["admission_requested"] <= record["admission_charged"]
                 <= self._records["before"]["free"], "invalid admission charge")
        _require(record["irq_body_us"] >= self._records["result"]["elapsed_us"],
                 "draw elapsed exceeds IRQ body")
        _require(record["irq_body_us"] <= 10000, "STOP: IRQ body exceeds 10000 us")
        if record["irq_body_us"] > 1000:
            self._advisories.append(f"IRQ body exceeds 1000 us: {record['irq_body_us']}")


def compare(direct, adapter):
    """Return parity fields, or raise ValueError; baseline heap is not parity.

    Inputs are completed Probe objects or their result dictionaries. Control is
    excluded. A passing comparison does not establish semantic hash equivalence.
    """
    direct = direct.result if isinstance(direct, Probe) else direct
    adapter = adapter.result if isinstance(adapter, Probe) else adapter
    _require(isinstance(direct, dict) and isinstance(adapter, dict), "incomplete comparison")
    _require(direct.get("path") == "direct" and adapter.get("path") == "adapter",
             "compare requires direct then adapter; control is not parity")
    fields = ("advance_bits", "pixels", "allocations", "requested_bytes", "largest_request",
              "deallocations", "freed_requested_bytes", "retained_delta")
    try:
        left, right = direct["result"], adapter["result"]
        parity = {field: left[field] for field in fields}
        for field in fields:
            _require(left[field] == right[field], f"parity mismatch: {field}")
        parity["peak_extra"] = left["peak_used"] - left["baseline_used"]
        _require(parity["peak_extra"] == right["peak_used"] - right["baseline_used"],
                 "parity mismatch: peak_extra")
    except (KeyError, TypeError) as error:
        raise ProbeError("incomplete comparison records") from error
    return parity
