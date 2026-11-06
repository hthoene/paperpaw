#!/usr/bin/env python3
"""#716 bounded X4 runner. Default mode is offline policy output, never serial.

Live use requires a separate owner/Manager authorization reference and physical
TEST-X4 mapping confirmation. Neither flag creates that authorization. Import,
--help, policy display and the test suite do not open hardware.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from x4textprobe_process import Processes, Stopped, supervise
from x4textprobe_records import Probe, compare

SOURCE = "3e3b38ff73c83297d7767507696fd0dfa2be1ce8"
APP_SHA = "4f3f1daba6a4a2addd1b389ea3742874c7a83979aa5d60fbf876aebfb3f830cb"
MERGED_SHA = "76c2c5e01a3f60ea88ac2d2f25f095d44631d79883485f61dacf6258e5b57726"
MAC = "f8:5b:1b:5d:a2:ac"
POLICY = dict(command_s=30, rom_s=30, flash_s=180, boot_s=30,
              irq_stop_us=10000, irq_advisory_us=1000, source=SOURCE)
PATHS = ("control", "direct", "adapter")


def require(condition, message):
    if not condition:
        raise Stopped(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def artifacts(app, merged):
    application, combined = Path(app).read_bytes(), Path(merged).read_bytes()
    require(len(application) == 5804272 and digest(application) == APP_SHA,
            "unpinned application")
    require(len(combined) == 5869808 and digest(combined) == MERGED_SHA,
            "unpinned merged reference")
    require(combined[0x10000:] == application, "application/reference mismatch")
    return application, combined


def layout(table):
    require(len(table) == 4096, "wrong partition sector size")
    expected = [(1, 2, 0x9000, 0x4000), (1, 0, 0xd000, 0x2000),
                (1, 1, 0xf000, 0x1000), (0, 16, 0x10000, 0x7f0000),
                (0, 17, 0x800000, 0x7f0000)]
    rows = []
    for offset in range(0, len(table), 32):
        item = table[offset:offset + 32]
        require(len(item) == 32, "short partition record")
        if item[:2] == b"\xeb\xeb":
            require(hashlib.md5(table[:offset]).digest() == item[16:],
                    "partition checksum mismatch")
            require(rows == expected, "unsupported partition layout")
            return
        require(item[:2] == b"\xaaP", "missing partition checksum")
        _, kind, subtype, address, size, _, flags = struct.unpack("<HBBII16sI", item)
        require(flags == 0, "partition flags unsupported")
        rows.append((kind, subtype, address, size))
    raise Stopped("missing partition checksum")


def ota_valid(data):
    require(len(data) == 8192, "wrong OTA metadata size")
    entries = []
    for at in (0, 4096):
        raw = data[at:at + 32]
        if raw == b"\xff" * 32:
            continue
        sequence, = struct.unpack_from("<I", raw)
        state, stored = struct.unpack_from("<II", raw, 24)
        crc = 0
        for byte in raw[:4]:
            crc ^= byte
            for _ in range(8):
                crc = (crc >> 1) ^ (0xedb88320 if crc & 1 else 0)
        require(0 < sequence < 0xffffffff and stored == (~crc & 0xffffffff),
                "invalid OTA sequence/CRC")
        entries.append((sequence, state))
    require(entries and len({item[0] for item in entries}) == len(entries),
            "ambiguous OTA selection")
    sequence, state = max(entries)
    require((sequence - 1) % 2 == 0 and state == 2, "active slot is not ota_0 VALID")


class Runner:
    """Fixed procedure, injected process factory/clock for offline proof."""

    def __init__(self, factory, directory, app, reference, clock=time.monotonic):
        self.factory, self.directory = factory, Path(directory)
        self.app, self.reference, self.clock = app, reference, clock
        self.stopped = False
        self.operations = []
        self.results = {}

    def call(self, action, seconds, *arguments, consume=None):
        require(not self.stopped, "runner is permanently stopped")
        self.operations.append((action, seconds, arguments))
        deadline = self.clock() + seconds
        try:
            with (self.directory / f"{len(self.operations):02d}-{action}.log").open("wb") as log:
                require(self.clock() < deadline, "deadline elapsed before worker start")
                child = self.factory(action, *arguments)
                return supervise(child, seconds, log, consume, self.clock,
                                 action == "boot", deadline)
        except BaseException:
            self.stopped = True
            raise

    def query(self, command, prefix):
        response = self.call("command", 30, "--command", command)
        if command == "version":
            require(response.splitlines()[-1:] == ["PP:END"], "version response incomplete")
        rows = [line for line in response.splitlines() if line.startswith(prefix)]
        require(len(rows) == 1, f"missing/duplicate {prefix}")
        return rows[0][len(prefix):]

    def ready(self):
        # Explicit approved awake changes only; no old-value restoration.
        old_sleep = self.query("setting:core.sleep-after", "PP:SETTING ")
        require(self.query("set:core.sleep-after=0", "PP:") == "OK", "sleep set refused")
        old_off = self.query("setting:x4.power-off-after", "PP:SETTING ")
        require(self.query("set:x4.power-off-after=x4.power-off-after.never", "PP:") == "OK",
                "switch-off set refused")
        require(self.query("setting:core.sleep-after", "PP:SETTING ") == "0", "sleep not zero")
        require(self.query("setting:x4.power-off-after", "PP:SETTING ") ==
                "x4.power-off-after.never", "switch-off not Never")
        version = self.query("version", "PP:VERSION ")
        require(version.endswith(" xteink-x4"), "candidate does not report X4")
        boot = self.call("command", 30, "--command", "boot")
        boot_rows = [line for line in boot.splitlines() if line.startswith("PP:BOOT ")]
        naps_rows = [line for line in boot.splitlines() if line.startswith("PP:NAPS ")]
        require(len(boot_rows) == 1 and re.fullmatch(
            r"PP:BOOT Some\(CoreUsbUart\) \d+ 0", boot_rows[0])
            and naps_rows == ["PP:NAPS 0"], "unexpected/duplicate boot evidence")
        require(self.query("wifi:status", "PP:WIFI ") == "off", "radio not already off")
        return dict(old_sleep=old_sleep, old_switch_off=old_off, version=version)

    def run(self):
        require(not self.stopped, "runner is permanently stopped")
        try:
            return self._run()
        except BaseException:
            self.stopped = True
            raise

    def _run(self):
        application, combined = artifacts(self.app, self.reference)
        layout(combined[0x8000:0x9000])
        (self.directory / "application.bin").write_bytes(application)
        identity = self.call("identity", 30)
        require("ESP32-C3" in identity and set(re.findall(r"mac:\s*([0-9a-f:]{17})",
                identity.lower())) == {MAC}, "C3/MAC identity mismatch")
        self.call("bootloader", 30)
        require((self.directory / "bootloader.bin").read_bytes() == combined[:0x8000],
                "bootloader reference differs")
        self.call("partitions", 30)
        table = (self.directory / "partitions.bin").read_bytes()
        require(table == combined[0x8000:0x9000], "partition reference differs")
        layout(table)
        self.call("otadata", 30)
        ota_valid((self.directory / "otadata.bin").read_bytes())
        require(digest((self.directory / "application.bin").read_bytes()) == APP_SHA,
                "application changed before write")
        flashed = self.call("flash", 180)
        require("Hash of data verified." in flashed, "flash verification absent")
        reset_lines = []

        def boot_line(line):
            if "rst:" in line:
                require(line.startswith("rst:0x15 ") and not reset_lines,
                        "unexpected/repeated boot reset")
                reset_lines.append(line)
        startup = self.call("boot", 30, consume=boot_line)
        require(reset_lines and "startup: kernel ready" in startup, "boot evidence incomplete")
        metadata = self.ready()
        for path in PATHS:
            probe = Probe(path)
            self.call("command", 30, "--command",
                      f"textprobe ascii {path} cold clean raster", consume=probe.feed)
            self.results[path] = probe.finish()
        compare(self.results["direct"], self.results["adapter"])
        return dict(policy=POLICY, source=SOURCE, app_sha256=APP_SHA,
                    metadata=metadata, results=self.results, hardware_measured=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--approval-ref")
    parser.add_argument("--physical-test-x4-confirmed", action="store_true")
    parser.add_argument("--app", type=Path)
    parser.add_argument("--merged-reference", type=Path)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--python", default="/home/agent/esptool-venv/bin/python")
    parser.add_argument("--remote", default="/home/agent/x4remote/x4-remote.py")
    args = parser.parse_args()
    if not args.execute:
        print(json.dumps(dict(policy=POLICY, hardware_access=False), indent=2))
        return
    if not (args.approval_ref and args.physical_test_x4_confirmed and args.app
            and args.merged_reference and args.evidence):
        parser.error("explicit approval reference, physical mapping, artifacts and new evidence directory required")
    args.evidence.mkdir(parents=True, exist_ok=False)
    factory = Processes(args.python, Path(__file__).with_name("x4textprobe_worker.py"),
                        args.remote, args.evidence, args.approval_ref)
    runner = Runner(factory, args.evidence, args.app, args.merged_reference)
    try:
        result = runner.run()
    except BaseException as error:
        result = dict(stopped=True, error=str(error), operations=runner.operations,
                      completed_results=runner.results, approval_ref=args.approval_ref)
        (args.evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        raise
    result["approval_ref"] = args.approval_ref
    (args.evidence / "result.json").write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
