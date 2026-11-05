"""Offline #716 proof. All transports, artifacts and measurements are synthetic."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import x4textprobe_runner as runner
from x4textprobe_process import Stopped, supervise
from x4textprobe_worker import guard_serial, retry_policy, tool_args, wire_command
from test_x4textprobe_records import synthetic


class Clock:
    def __init__(self):
        self.now = 0

    def __call__(self):
        return self.now


class FakeChild:
    def __init__(self, clock, chunks=(), never=False, code=0, delay=0):
        self.clock, self.chunks = clock, list(chunks)
        self.never, self.code, self.delay = never, code, delay
        self.killed, self.closed, self.reads = False, False, 0

    def read(self, timeout):
        self.reads += 1
        if self.delay:
            self.clock.now += self.delay
            self.delay = 0
        if self.chunks:
            return self.chunks.pop(0)
        if self.never:
            self.clock.now += timeout
        return b""

    def poll(self):
        return None if self.never or self.chunks else self.code

    def kill(self):
        self.killed = True

    def close(self):
        self.closed = True


def table_fixture():
    rows = [(1, 2, 0x9000, 0x4000), (1, 0, 0xd000, 0x2000),
            (1, 1, 0xf000, 0x1000), (0, 16, 0x10000, 0x7f0000),
            (0, 17, 0x800000, 0x7f0000)]
    data = b"".join(struct.pack("<HBBII16sI", 0x50aa, *r, b"test", 0) for r in rows)
    data += b"\xeb\xeb" + b"\xff" * 14 + hashlib.md5(data).digest()
    return data.ljust(4096, b"\xff")


def ota_fixture(sequence=1, state=2):
    raw = bytearray(b"\xff" * 8192)
    struct.pack_into("<I", raw, 0, sequence)
    struct.pack_into("<I", raw, 24, state)
    crc = 0
    for byte in raw[:4]:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ (0xedb88320 if crc & 1 else 0)
    struct.pack_into("<I", raw, 28, ~crc & 0xffffffff)
    return bytes(raw)


class Scenario:
    def __init__(self, directory, clock, bad_at=None, output=None, hanging=False):
        self.directory, self.clock, self.bad_at = directory, clock, bad_at
        self.output, self.hanging = output, hanging
        self.calls, self.children = [], []
        self.table, self.ota = table_fixture(), ota_fixture()
        self.application = b"SYNTHETIC-APP"
        self.combined = b"\xff" * 0x8000 + self.table + b"\xff" * 0x7000 + self.application

    def __call__(self, action, *args):
        self.calls.append((action, args))
        responses = {
            "identity": "ESP32-C3\nMAC: f8:5b:1b:5d:a2:ac\n",
            "flash": "Hash of data verified.\n",
            "boot": "rst:0x15 (USB_UART_CHIP_RESET)\nINFO - startup: kernel ready in 1 ms\n",
        }
        if action == "bootloader":
            (self.directory / "bootloader.bin").write_bytes(self.combined[:0x8000])
        if action == "partitions":
            (self.directory / "partitions.bin").write_bytes(self.table)
        if action == "otadata":
            (self.directory / "otadata.bin").write_bytes(self.ota)
        if action == "command":
            command = args[1]
            if command.startswith("textprobe"):
                data = "\n".join(synthetic(command.split()[2])) + "\n"
            elif command.startswith("setting:"):
                value = "0" if "sleep-after" in command else "x4.power-off-after.never"
                data = f"PP:SETTING {value}\n"
            elif command.startswith("set:"):
                data = "PP:OK\n"
            elif command == "version":
                data = "PP:VERSION synthetic xteink-x4\nPP:END\n"
            elif command == "boot":
                data = "PP:BOOT Some(CoreUsbUart) 2 0\nPP:NAPS 0\n"
            else:
                data = "PP:WIFI off\n"
        else:
            data = responses.get(action, "read complete\n")
        is_bad = len(self.calls) == self.bad_at
        if is_bad:
            data = self.output if self.output is not None else "Lost connection, retrying..."
        child = FakeChild(self.clock, [data.encode()], never=is_bad and self.hanging)
        self.children.append(child)
        return child


class ProcessTests(unittest.TestCase):
    def test_exact_deadlines_without_sleeping_or_ports(self):
        for seconds in (30, 180):
            clock = Clock()
            child = FakeChild(clock, never=True)
            with self.subTest(seconds=seconds), self.assertRaises(Stopped):
                supervise(child, seconds, io.BytesIO(), clock=clock)
            self.assertGreaterEqual(clock.now, seconds)
            self.assertLess(clock.now, seconds + 0.021)
            self.assertTrue(child.killed and child.closed)

    def test_deadline_includes_factory_and_late_complete_output(self):
        clock = Clock()
        child = FakeChild(clock, [b"done\n"], delay=31)
        with self.assertRaises(Stopped):
            supervise(child, 30, io.BytesIO(), clock=clock)
        child = FakeChild(clock, [b"done\n"])
        with self.assertRaises(Stopped):
            supervise(child, 30, io.BytesIO(), clock=clock, deadline=clock.now - 1)
        self.assertTrue(child.killed)

    def test_deadline_retains_last_bytes_without_accepting_late_records(self):
        for delay in (30, 31):
            for payload in (b"PP:PANIC partial", b"PP:END\n", b"rst:0x15\n"):
                clock, log, accepted = Clock(), io.BytesIO(), []
                child = FakeChild(clock, [payload], delay=delay)
                with self.subTest(delay=delay, payload=payload), self.assertRaises(Stopped):
                    supervise(child, 30, log, consume=accepted.append, clock=clock)
                self.assertEqual(log.getvalue(), payload)
                self.assertEqual(accepted, [])
                self.assertEqual(child.reads, 1)
                self.assertTrue(child.killed and child.closed)

    def test_first_partial_report_stops_before_next_read(self):
        for text in (b"Lost connection", b"retrying", b"reconnecting", b"PP:ERR refused",
                     b"PP:TEXTPROBE_ERROR reason=headroom", b"rst:0x15 unexpected"):
            clock = Clock()
            child = FakeChild(clock, [text, b"dangerous later output\n"], never=True)
            log = io.BytesIO()
            with self.subTest(text=text), self.assertRaises(Stopped):
                supervise(child, 30, log, clock=clock)
            self.assertEqual(child.reads, 1)
            self.assertEqual(log.getvalue(), text)
            self.assertTrue(child.killed and child.closed)

    def test_nonzero_exit_and_unterminated_line_fail(self):
        for chunks, code in (([], 1), ([b"partial"], 0)):
            child = FakeChild(Clock(), chunks, code=code)
            with self.assertRaises(Stopped):
                supervise(child, 30, io.BytesIO(), clock=child.clock)
            self.assertTrue(child.killed and child.closed)

    def test_parser_error_kills_child_before_success_exit(self):
        child = FakeChild(Clock(), [b"bad\n"])
        def reject(_):
            raise ValueError("invalid sample")
        with self.assertRaises(ValueError):
            supervise(child, 30, io.BytesIO(), reject, child.clock)
        self.assertTrue(child.killed and child.closed)

    def test_real_process_loss_is_killed_without_hardware(self):
        from x4textprobe_process import Child
        code = "import sys,time;sys.stdout.write('Lost connection');sys.stdout.flush();time.sleep(10)"
        child = Child([sys.executable, "-u", "-I", "-c", code])
        with self.assertRaises(Stopped):
            supervise(child, 1, io.BytesIO())
        self.assertIsNotNone(child.process.poll())


class WorkerTests(unittest.TestCase):
    def test_retry_flags_and_serial_guard_prevent_reopen_or_postfault_io(self):
        loader = SimpleNamespace(WRITE_BLOCK_ATTEMPTS=3, ESPLoader=SimpleNamespace(
            WRITE_FLASH_ATTEMPTS=2), DEFAULT_OPEN_PORT_ATTEMPTS=10)
        retry_policy(loader)
        self.assertEqual((loader.WRITE_BLOCK_ATTEMPTS, loader.ESPLoader.WRITE_FLASH_ATTEMPTS,
                          loader.DEFAULT_OPEN_PORT_ATTEMPTS), (1, 1, 1))
        class Port:
            def __init__(self):
                self.opens, self.writes = 0, 0
            def open(self):
                self.opens += 1
            def read(self):
                raise OSError("lost link")
            def write(self, _):
                self.writes += 1
        guard_serial(Port)
        port = Port()
        port.open()
        with self.assertRaises(RuntimeError):
            port.open()
        self.assertEqual(port.opens, 1)
        second = Port()
        with self.assertRaises(RuntimeError):
            second.open()
        self.assertEqual(second.opens, 0)
        self.assertIs(port.dtr, False)
        self.assertIs(port.rts, False)
        with self.assertRaises(OSError):
            port.read()
        with self.assertRaises(RuntimeError):
            port.write(b"must not resend")
        self.assertEqual(port.writes, 0)

    def test_fixed_esptool_and_command_grammar(self):
        args = tool_args("identity", Path("evidence"))
        self.assertEqual(args[args.index("--chip") + 1], "auto")
        self.assertIn("--no-stub", args)
        self.assertEqual(args[args.index("--connect-attempts") + 1], "1")
        for action in ("bootloader", "partitions", "otadata", "flash"):
            args = tool_args(action, Path("evidence"))
            self.assertEqual(args[args.index("--before") + 1], "no-reset")
            self.assertEqual(args[args.index("--after") + 1], "no-reset-stub")
            self.assertNotIn("--diff-with", args)
        self.assertEqual(tool_args("flash", Path("evidence"))[-2:],
                         ["0x10000", "evidence/application.bin"])
        self.assertEqual(wire_command("set:core.sleep-after=0"),
                         ("set core.sleep-after 30", "PP:OK"))
        self.assertEqual(wire_command("wifi:status"), ("wifi status", "PP:WIFI "))
        self.assertEqual(wire_command("setting:core.sleep-after"),
                         ("setting core.sleep-after", "PP:SETTING "))
        for command in ("restart", "wifi:off", "put", "textprobe ascii direct warm clean raster"):
            with self.subTest(command=command), self.assertRaises(RuntimeError):
                wire_command(command)


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.directory = Path(self.temp.name)
        self.clock = Clock()

    def tearDown(self):
        self.temp.cleanup()

    def execute(self, scenario):
        live = runner.Runner(scenario, self.directory, "not-opened", "not-opened", self.clock)
        with mock.patch.object(runner, "artifacts", return_value=(scenario.application, scenario.combined)), \
             mock.patch.object(runner, "APP_SHA", hashlib.sha256(scenario.application).hexdigest()):
            result = live.run()
        return live, result

    def test_success_is_exact_three_ascii_commands_and_deadlines(self):
        scenario = Scenario(self.directory, self.clock)
        live, result = self.execute(scenario)
        measurements = [args[1] for action, args in scenario.calls
                        if action == "command" and args[1].startswith("textprobe")]
        self.assertEqual(measurements, [f"textprobe ascii {p} cold clean raster"
                                       for p in ("control", "direct", "adapter")])
        self.assertEqual([seconds for action, seconds, _ in live.operations if action == "flash"], [180])
        self.assertTrue(all(seconds == 30 for action, seconds, _ in live.operations if action != "flash"))
        self.assertEqual(sum(action == "identity" for action, _ in scenario.calls), 1)
        self.assertEqual(sum(action == "boot" for action, _ in scenario.calls), 1)
        self.assertEqual(set(result["results"]), {"control", "direct", "adapter"})
        self.assertTrue(all(p.closed for p in scenario.children))
        self.assertNotIn("restore", repr(scenario.calls))

    def test_failure_at_every_operation_stops_permanently_without_recovery(self):
        good = Scenario(self.directory, self.clock)
        self.execute(good)
        for at in range(1, len(good.calls) + 1):
            with self.subTest(at=at):
                scenario = Scenario(self.directory, self.clock, bad_at=at)
                live = runner.Runner(scenario, self.directory, "unused", "unused", self.clock)
                with mock.patch.object(runner, "artifacts", return_value=(scenario.application, scenario.combined)), \
                     mock.patch.object(runner, "APP_SHA", hashlib.sha256(scenario.application).hexdigest()):
                    with self.assertRaises(Stopped):
                        live.run()
                    self.assertTrue(live.stopped)
                    count = len(scenario.calls)
                    with self.assertRaises(Stopped):
                        live.call("identity", 30)
                self.assertEqual(len(scenario.calls), at)
                self.assertEqual(len(scenario.calls), count)
                self.assertTrue(scenario.children[-1].killed)

    def test_identity_layout_ota_and_radio_gate_prevent_later_actions(self):
        for case in ("identity", "table", "slot", "radio"):
            with self.subTest(case=case):
                scenario = Scenario(self.directory, self.clock)
                if case == "identity":
                    scenario.bad_at, scenario.output = 1, "ESP32-S3\nMAC: 00:00:00:00:00:00\n"
                elif case == "table":
                    scenario.table = b"\xff" * 4096
                elif case == "slot":
                    scenario.ota = ota_fixture(sequence=2)
                else:
                    scenario.bad_at, scenario.output = 15, "PP:WIFI online private\n"
                with self.assertRaises(Stopped):
                    self.execute(scenario)
                actions = [action for action, _ in scenario.calls]
                if case != "radio":
                    self.assertNotIn("flash", actions)
                self.assertFalse(any(a == "command" and v[1].startswith("textprobe")
                                     for a, v in scenario.calls))

    def test_each_measurement_gate_prevents_next_command(self):
        mutations = (("irq_body_us=20", "irq_body_us=10001"),
                     ("guard_valid=1", "guard_valid=0"),
                     ("overflow=0", "overflow=1"),
                     ("failed_allocations=0", "failed_allocations=1"),
                     ("scope_valid=1", "scope_valid=0"),
                     ("peak_used=101024", "peak_used=222823"),
                     ("min_free=161120", "min_free=39321"),
                     ("stack_left=20000", "stack_left=10239"),
                     ("unused=20000", "unused=10239"))
        for old, new in mutations:
            with self.subTest(old=old):
                data = "\n".join(synthetic("control")).replace(old, new) + "\n"
                scenario = Scenario(self.directory, self.clock, bad_at=16, output=data)
                with self.assertRaises((Stopped, ValueError)):
                    self.execute(scenario)
                self.assertEqual(len(scenario.calls), 16)
                self.assertTrue(scenario.children[-1].killed)

    def test_missing_either_end_marker_stops_before_direct(self):
        for marker in ("PP:END", "PP:TEXTPROBE_END"):
            with self.subTest(marker=marker):
                lines = [l for l in synthetic("control") if l != marker]
                scenario = Scenario(self.directory, self.clock, bad_at=16,
                                    output="\n".join(lines) + "\n")
                with self.assertRaises(ValueError):
                    self.execute(scenario)
                self.assertEqual(len(scenario.calls), 16)

    def test_default_mode_and_missing_authorization_never_construct_processes(self):
        with mock.patch.object(runner, "Processes", side_effect=AssertionError("hardware forbidden")):
            with mock.patch.object(sys, "argv", ["runner"]), contextlib.redirect_stdout(io.StringIO()) as out:
                runner.main()
            self.assertFalse(json.loads(out.getvalue())["hardware_access"])
            with mock.patch.object(sys, "argv", ["runner", "--execute"]), \
                 contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                runner.main()

    def test_corrupt_candidate_fails_before_process_factory(self):
        scenario = Scenario(self.directory, self.clock)
        bad = self.directory / "bad.bin"
        bad.write_bytes(b"not candidate")
        live = runner.Runner(scenario, self.directory, bad, bad, self.clock)
        with self.assertRaises(Stopped):
            live.run()
        self.assertEqual(scenario.calls, [])

    def test_duplicate_boot_and_nonzero_naps_stop_before_radio_or_probes(self):
        variants = [
            "PP:BOOT Some(CoreUsbUart) 2 0\nPP:BOOT Some(CoreSw) 3 0\nPP:NAPS 0\n",
            "PP:BOOT Some(CoreUsbUart) 2 0\nPP:NAPS 01\n",
            "PP:BOOT Some(CoreUsbUart) 2 0\nPP:NAPS 0\nPP:NAPS 1\n",
        ]
        for output in variants:
            with self.subTest(output=output):
                scenario = Scenario(self.directory, self.clock, bad_at=14, output=output)
                with self.assertRaises(Stopped):
                    self.execute(scenario)
                self.assertEqual(len(scenario.calls), 14)

    def test_partition_checksum_and_ota_crc_trial_ambiguity_refused(self):
        runner.layout(table_fixture())
        runner.ota_valid(ota_fixture())
        for data in (b"", table_fixture()[:-1], b"\x00" * 4096):
            with self.subTest(kind="table"), self.assertRaises(Stopped):
                runner.layout(data)
        for data in (b"", b"\xff" * 8192, ota_fixture(1, 1), ota_fixture(2, 2),
                     bytes(8192)):
            with self.subTest(kind="ota"), self.assertRaises(Stopped):
                runner.ota_valid(data)


if __name__ == "__main__":
    unittest.main()
