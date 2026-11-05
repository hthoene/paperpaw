"""Offline worker/SDK regressions. Fake ports only, no constructors open serial."""
import contextlib
import importlib.util
import io
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

import x4textprobe_worker as worker
from x4textprobe_process import Stopped, supervise
from test_x4textprobe_runner import Clock, FakeChild


class WorkerStreamTests(unittest.TestCase):
    def run_remote(self, payload, command="version", partial=False):
        class Port:
            in_waiting = 1
            def __init__(self):
                self.chunks, self.written, self.closed = [payload], [], False
            def open(self):
                raise AssertionError("fake module supplies precreated port only")
            def read(self, *_):
                if self.chunks:
                    return self.chunks.pop(0)
                raise OSError("synthetic EOF")
            def write(self, data):
                self.written.append(data)
            def close(self):
                self.closed = True
        port = Port()
        module = SimpleNamespace(open_port=lambda _: port)
        spec = SimpleNamespace(loader=SimpleNamespace(exec_module=lambda _: None))
        serial = SimpleNamespace(rfc2217=SimpleNamespace(Serial=Port))
        output = io.BytesIO()
        args = SimpleNamespace(action="command", command=command, remote="fake-module")
        with mock.patch.dict(sys.modules, {"serial": serial, "serial.rfc2217": serial.rfc2217}), \
             mock.patch.object(worker.importlib.util, "spec_from_file_location", return_value=spec), \
             mock.patch.object(worker.importlib.util, "module_from_spec", return_value=module), \
             mock.patch.object(worker.sys, "stdout", SimpleNamespace(buffer=output)):
            if partial:
                with self.assertRaises(OSError):
                    worker.remote(args)
            else:
                worker.remote(args)
        return port, output.getvalue()

    def test_terminal_does_not_discard_trailing_fatal_evidence(self):
        for payload in (b"PP:END\nPP:PANIC synthetic fault\n", b"PP:END\nLost connection"):
            port, raw = self.run_remote(payload)
            self.assertEqual(raw, payload)
            self.assertEqual(port.written, [b"version\n"])
            self.assertTrue(port.closed)
            child = FakeChild(Clock(), [raw])
            with self.assertRaises(Stopped):
                supervise(child, 30, io.BytesIO(), clock=child.clock)

    def test_partial_fatal_is_forwarded_without_newline(self):
        payload = b"Lost connection without newline"
        port, raw = self.run_remote(payload, partial=True)
        self.assertEqual(raw, payload)
        self.assertTrue(port.closed)
        self.assertEqual(len(port.written), 1)

    def test_native_ack_grammar_uses_one_send(self):
        for command, response, expected in (
            ("setting:core.sleep-after", b"PP:SETTING 0\n", b"setting core.sleep-after\n"),
            ("set:core.sleep-after=0", b"PP:OK\n", b"set core.sleep-after 30\n"),
            ("wifi:status", b"PP:WIFI off\n", b"wifi status\n"),
        ):
            port, raw = self.run_remote(response, command)
            self.assertEqual(port.written, [expected])
            self.assertEqual(raw, response)
            self.assertTrue(port.closed)

    def test_log_open_failure_starts_no_child(self):
        from x4textprobe_runner import Runner
        factory = mock.Mock(side_effect=AssertionError("must not start hardware child"))
        live = Runner(factory, Path("fake"), None, None)
        with mock.patch.object(Path, "open", side_effect=OSError("synthetic disk full")), \
             self.assertRaises(OSError):
            live.call("identity", 30)
        factory.assert_not_called()
        self.assertTrue(live.stopped)

    def test_slow_log_open_never_starts_late_hardware_child(self):
        from x4textprobe_runner import Runner
        clock = Clock()
        factory = mock.Mock(side_effect=AssertionError("late start forbidden"))
        live = Runner(factory, Path("fake"), None, None, clock)
        def delayed_open(*args, **kwargs):
            clock.now = 31
            return io.BytesIO()
        with mock.patch.object(Path, "open", side_effect=delayed_open), self.assertRaises(Stopped):
            live.call("identity", 30)
        factory.assert_not_called()
        self.assertTrue(live.stopped)

    def test_log_write_failure_occurs_after_worker_closed(self):
        clock = Clock()
        child = FakeChild(clock, [b"complete\n"])
        class BrokenLog:
            def write(self, data):
                if not child.closed:
                    raise AssertionError("log write while worker live")
                raise OSError("synthetic disk full")
            def flush(self):
                pass
        with self.assertRaises(OSError):
            supervise(child, 30, BrokenLog(), clock=clock)
        self.assertTrue(child.closed)


class ProtocolPolicyTests(unittest.TestCase):
    def test_first_protocol_timeout_escapes_retry_handlers_and_prevents_reconnect(self):
        class Fatal(Exception):
            pass
        calls = []
        class Loader:
            def command(self):
                return None
            check_command = command
            get_security_info = command
            read_reg = command
            def get_chip_id(self):
                raise Fatal("detection failed")
            def sync(self):
                calls.append("sync")
                raise Fatal("no reply")
            def connect(self):
                calls.append("reset")
                # Models the real reviewed SDK's hardcoded five-sync loop.
                for _ in range(5):
                    try:
                        self.sync()
                        break
                    except Fatal:
                        pass
        layer = SimpleNamespace(ESPLoader=Loader, FatalError=Fatal)
        worker.guard_protocol(layer)
        device = Loader()
        device._port = SimpleNamespace()
        with self.assertRaises(RuntimeError):
            device.connect()
        self.assertEqual(calls, ["reset", "sync"])
        self.assertTrue(device._port._probe_failed)
        with self.assertRaises(RuntimeError):
            Loader().connect()
        self.assertEqual(calls, ["reset", "sync"])
        with self.assertRaises(RuntimeError):
            device.read_reg()


# These additional tests exercise the exact installed esptool source, not serial.
# CI without that optional local tool reports explicit skips, not passes.
try:
    import esptool
    import esptool.loader as sdk
    import esptool.cmds as cmds
    SDK_AVAILABLE = esptool.__version__ == "5.6.0"
except ImportError:
    SDK_AVAILABLE = False


@unittest.skipUnless(SDK_AVAILABLE, "optional exact esptool5.6.0 unavailable")
class ExactSdkTests(unittest.TestCase):
    def guards(self, sync=None, chip=None):
        stack = contextlib.ExitStack()
        names = ("command", "check_command", "sync", "get_chip_id", "get_security_info", "read_reg", "connect")
        for name in names:
            stack.enter_context(mock.patch.object(sdk.ESPLoader, name, getattr(sdk.ESPLoader, name)))
        if sync is not None:
            sdk.ESPLoader.sync = sync
        if chip is not None:
            sdk.ESPLoader.get_chip_id = chip
        worker.guard_protocol(sdk)
        return stack

    def test_actual_connect_attempt_cannot_repeat_failed_sync(self):
        calls = []
        def failed_sync(_):
            calls.append("sync")
            raise sdk.FatalError("synthetic no response")
        method = sdk.ESPLoader._connect_attempt
        with self.guards(sync=failed_sync):
            fake = SimpleNamespace(_port=SimpleNamespace(flushOutput=lambda: None),
                                   flush_input=lambda: None)
            fake.sync = lambda: sdk.ESPLoader.sync(fake)
            with self.assertRaises(RuntimeError):
                method(fake, lambda: self.fail("no-reset must not reset"), mode="no-reset")
        self.assertEqual(calls, ["sync"])

    def test_actual_autodetection_failure_cannot_fallback_or_reset(self):
        calls = []
        def failed_chip(_):
            calls.append("chip")
            raise sdk.FatalError("synthetic chip detection failure")
        with self.guards(chip=failed_chip):
            fake = SimpleNamespace(serial_port="rfc2217://FAKE", _port=SimpleNamespace(),
                                   connect=lambda *a, **kw: calls.append("initial-connect"))
            fake.get_chip_id = lambda: sdk.ESPLoader.get_chip_id(fake)
            fake.read_reg = lambda *_: self.fail("fallback read is forbidden")
            with mock.patch.object(cmds, "ESPLoader", return_value=fake), self.assertRaises(RuntimeError):
                cmds.detect_chip(port=SimpleNamespace(), connect_attempts=1)
        self.assertEqual(calls, ["initial-connect", "chip"])


if __name__ == "__main__":
    unittest.main()
