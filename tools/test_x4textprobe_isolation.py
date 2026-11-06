"""Offline #716 isolation regressions. Every port is fake; SDK CLI never runs.

Fresh interpreters keep these tests independent of suites that already import
esptool. Exact SDK tests explicitly skip when local esptool 5.6.0 is absent.
"""

from pathlib import Path
import subprocess
import sys
import textwrap
import unittest

import x4textprobe_worker as worker


TOOLS = Path(__file__).resolve().parent
EXPECTED_CONFIG = ("[esptool]\nconnect_attempts = 1\n"
                   "open_port_attempts = 1\nwrite_block_attempts = 1\n")


class FakePort:
    """Starts with pyserial's active defaults and records its first open."""

    def __init__(self):
        self.is_open = False
        self._dtr = self._rts = True
        self.control_sets = []
        self.open_states = []
        self.port = self.name = "rfc2217://FAKE-NO-NETWORK"
        self.ignore = None

    @property
    def dtr(self):
        return self._dtr

    @dtr.setter
    def dtr(self, value):
        self.control_sets.append(("dtr", value, self.is_open))
        if self.ignore != "dtr":
            self._dtr = value

    @property
    def rts(self):
        return self._rts

    @rts.setter
    def rts(self, value):
        self.control_sets.append(("rts", value, self.is_open))
        if self.ignore != "rts":
            self._rts = value

    def open(self):
        self.open_states.append((self.dtr, self.rts))
        self.is_open = True

    def close(self):
        self.is_open = False

    def read(self, *_):
        raise AssertionError("fake constructor must perform no reads")

    def write(self, *_):
        raise AssertionError("fake constructor must perform no writes")


_CHILD_IMPORTS = """
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
from unittest import mock
sys.path.insert(0, sys.argv[1])
import x4textprobe_worker as worker
from test_x4textprobe_isolation import FakePort, EXPECTED_CONFIG
assert not any(n == 'esptool' or n.startswith('esptool.') for n in sys.modules)
"""

_SDK_IMPORTS = """
if importlib.util.find_spec('esptool') is None:
    print('exact esptool 5.6.0 unavailable')
    sys.exit(77)
from esp_pylib.config import ToolConfig
import serial

def exact_sdk():
    import esptool
    import esptool.config as config
    import esptool.loader as loader
    if esptool.__version__ != '5.6.0':
        print('exact esptool 5.6.0 unavailable')
        sys.exit(77)
    return esptool, config, loader

def hostile_files(directory):
    directories = [directory / name for name in ('cwd', 'user-config', 'home-search')]
    for item in directories:
        item.mkdir()
        (item / 'esptool.cfg').write_text(
            '[esptool]\\ncustom_reset_sequence = D1|R1|W1\\n'
            'custom_hard_reset_sequence = D1|R1|W1\\n'
            'open_port_attempts = 99\\nconnect_attempts = 99\\n'
            'write_block_attempts = 99\\nreset_delay = 99\\n')
    os.environ.update(ESPTOOL_CFGFILE=str(directories[0] / 'esptool.cfg'),
                      ESPTOOL_OPEN_PORT_ATTEMPTS='99', ESPTOOL_CONNECT_ATTEMPTS='99',
                      ESPTOOL_ENV_FPGA='1', ESPTOOL_PORT='MUST-NOT-OPEN')
    return directories
"""


class SerialIsolationTests(unittest.TestCase):
    def port_class(self):
        # Each guard owns one closure; tests never patch a shared real class.
        class Port(FakePort):
            pass
        worker.guard_serial(Port)
        return Port

    def test_inactive_controls_are_established_while_closed_at_first_open(self):
        port = self.port_class()()
        self.assertIs(port.dtr, True)
        self.assertIs(port.rts, True)
        port.open()
        self.assertEqual(port.open_states, [(False, False)])
        self.assertEqual(port.control_sets, [("dtr", False, False), ("rts", False, False)])

    def test_second_instance_and_reopen_are_forbidden(self):
        port_type = self.port_class()
        first, second = port_type(), port_type()
        first.open()
        first.close()
        for port in (second, first):
            with self.subTest(second=port is second), self.assertRaises(RuntimeError):
                port.open()
        self.assertEqual(first.open_states, [(False, False)])
        self.assertEqual(second.open_states, [])
        self.assertEqual(second.control_sets, [])

    def test_ignored_setters_and_already_open_ports_fail_before_original_open(self):
        for ignored in ("dtr", "rts"):
            port = self.port_class()()
            port.ignore = ignored
            with self.subTest(ignored=ignored), self.assertRaises(RuntimeError):
                port.open()
            self.assertEqual(port.open_states, [])
            self.assertFalse(port.is_open)
        port = self.port_class()()
        port.is_open = True
        with self.assertRaises(RuntimeError):
            port.open()
        self.assertEqual(port.control_sets, [])
        self.assertEqual(port.open_states, [])

    def test_raising_control_setter_never_opens_transport(self):
        class Port(FakePort):
            @FakePort.dtr.setter
            def dtr(self, value):
                raise OSError("synthetic control setter failure")
        worker.guard_serial(Port)
        port = Port()
        with self.assertRaises(RuntimeError):
            port.open()
        self.assertEqual(port.open_states, [])
        self.assertFalse(port.is_open)

    def test_remote_preserves_pending_input_while_sdk_retains_protocol_flush(self):
        pending = b"rst:0x15 startup evidence\nPP:PANIC synthetic previous fault\n"
        for preserve in (True, False):
            class Port(FakePort):
                def __init__(self):
                    super().__init__()
                    self.pending = pending
                    self.flushes = 0

                def reset_input_buffer(self):
                    self.flushes += 1
                    self.pending = b""

                def open(self):
                    super().open()
                    # Match RFC2217.open's initial input purge.
                    self.reset_input_buffer()

            worker.guard_serial(Port, preserve_input=preserve)
            port = Port()
            port.open()
            with self.subTest(preserve=preserve):
                self.assertEqual(port.open_states, [(False, False)])
                self.assertEqual(port.pending, pending if preserve else b"")
                self.assertEqual(port.flushes, 0 if preserve else 1)


class FreshInterpreterTests(unittest.TestCase):
    def run_child(self, body, sdk=False):
        code = textwrap.dedent(_CHILD_IMPORTS)
        if sdk:
            code += textwrap.dedent(_SDK_IMPORTS)
        code += textwrap.dedent(body)
        result = subprocess.run([sys.executable, "-B", "-I", "-c", code, str(TOOLS)],
                                capture_output=True, text=True, timeout=15, check=False)
        if result.returncode == 77 and sdk:
            self.skipTest(result.stdout.strip())
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_private_environment_is_exact_and_restored_on_success_and_failure(self):
        self.run_child("""
            os.environ.update(ESPTOOL_CFGFILE='/invented/hostile.cfg',
                              ESPTOOL_OPEN_PORT_ATTEMPTS='99', ESPTOOL_PORT='NO-PORT',
                              ESPTOOL_FUTURE_OVERRIDE='hostile')
            before = {k: v for k, v in os.environ.items() if k.startswith('ESPTOOL_')}
            old_home = os.environ.get('HOME')
            for fail in (False, True):
                try:
                    with worker.sdk_environment() as path:
                        assert path.read_text() == EXPECTED_CONFIG
                        assert path.stat().st_mode & 0o777 == 0o400
                        assert {k: v for k, v in os.environ.items() if k.startswith('ESPTOOL_')} == {
                            'ESPTOOL_CFGFILE': str(path), 'ESPTOOL_OPEN_PORT_ATTEMPTS': '1',
                            'ESPTOOL_CONNECT_ATTEMPTS': '1'}
                        assert os.environ.get('HOME') == old_home
                        os.environ['ESPTOOL_INJECTED_IN_CONTEXT'] = 'removed-on-exit'
                        if fail:
                            raise RuntimeError('synthetic body failure')
                except RuntimeError as error:
                    assert fail and str(error) == 'synthetic body failure'
                assert not path.exists()
                assert {k: v for k, v in os.environ.items() if k.startswith('ESPTOOL_')} == before
                assert os.environ.get('HOME') == old_home
        """)

    def test_sdk_preimport_is_refused_without_changing_environment(self):
        self.run_child("""
            for name in ('esptool', 'esptool.loader'):
                sys.modules[name] = object()
                before = dict(os.environ)
                try:
                    with worker.sdk_environment():
                        raise AssertionError('preimported SDK must not be admitted')
                except RuntimeError as error:
                    assert 'imported before' in str(error)
                finally:
                    del sys.modules[name]
                assert dict(os.environ) == before
        """)

    def test_exact_sdk_linux_constructor_opens_only_fake_inactive_port(self):
        self.run_child("""
            with worker.sdk_environment() as path, \\
                 mock.patch.object(serial, 'serial_for_url', side_effect=AssertionError('hardware forbidden')) as forbidden:
                esptool, config, loader = exact_sdk()
                worker.check_sdk_config(path, config, loader)
                class Port(FakePort):
                    pass
                worker.guard_serial(Port)
                port = Port()
                with mock.patch.object(serial, 'serial_for_url', return_value=port) as constructor, \\
                     mock.patch.object(sys, 'platform', 'linux'):
                    device = loader.ESPLoader(worker.PORT)
                    assert device._port is port
                    constructor.assert_called_once_with(worker.PORT, exclusive=True, do_not_open=True)
                    assert port.open_states == [(False, False)]
                    assert port.control_sets == [('dtr', False, False), ('rts', False, False)]
                    try:
                        Port().open()
                        raise AssertionError('second instance opened')
                    except RuntimeError:
                        pass
                    port.close()
                forbidden.assert_not_called()
        """, sdk=True)

    def test_exact_sdk_ignores_hostile_search_and_environment_before_import(self):
        self.run_child("""
            with tempfile.TemporaryDirectory() as root:
                directories = hostile_files(Path(root))
                before = {k: v for k, v in os.environ.items() if k.startswith('ESPTOOL_')}
                old_home = os.environ.get('HOME')
                expected = dict(connect_attempts='1', open_port_attempts='1', write_block_attempts='1')
                with mock.patch.object(ToolConfig, '_default_search_dirs', return_value=directories) as search, \\
                     mock.patch.object(serial, 'serial_for_url', side_effect=AssertionError('hardware forbidden')) as port:
                    with worker.sdk_environment() as path:
                        esptool, config, loader = exact_sdk()
                        worker.check_sdk_config(path, config, loader)
                        assert dict(loader.cfg) == expected
                        assert loader.DEFAULT_CONNECT_ATTEMPTS == loader.DEFAULT_OPEN_PORT_ATTEMPTS == 1
                        assert loader.WRITE_BLOCK_ATTEMPTS == 1
                        parser, selected = config.load_config_file(verbose=True)
                        assert Path(selected).resolve() == path.resolve()
                        assert dict(parser['esptool']) == expected
                        worker.check_sdk_config(path, config, loader)
                        search.assert_not_called()
                        port.assert_not_called()
                assert os.environ.get('HOME') == old_home
                assert {k: v for k, v in os.environ.items() if k.startswith('ESPTOOL_')} == before
        """, sdk=True)

    def test_exact_sdk_tampered_missing_or_unset_pin_refuses_before_cli_or_serial(self):
        for damage in ("tamper", "missing", "unset", "wrong-section",
                       "unset-after-import", "inject-after-import"):
            with self.subTest(damage=damage):
                self.run_child(f"damage = {damage!r}\n" + textwrap.dedent("""
                    with tempfile.TemporaryDirectory() as root:
                        directories = hostile_files(Path(root))
                        with mock.patch.object(ToolConfig, '_default_search_dirs', return_value=directories), \\
                             mock.patch.object(serial, 'serial_for_url', side_effect=AssertionError('hardware forbidden')) as port:
                            with worker.sdk_environment() as path:
                                if damage.endswith('-after-import'):
                                    exact_sdk()
                                if damage in ('tamper', 'wrong-section'):
                                    path.chmod(0o600)
                                    path.write_text(EXPECTED_CONFIG + 'custom_reset_sequence = D1|R1\\n'
                                                    if damage == 'tamper' else '[other]\\nvalue = 1\\n')
                                elif damage == 'missing':
                                    path.unlink()
                                elif damage in ('unset', 'unset-after-import'):
                                    del os.environ['ESPTOOL_CFGFILE']
                                else:
                                    os.environ['ESPTOOL_PORT'] = 'MUST-NOT-OPEN'
                                esptool, config, loader = exact_sdk()
                                if damage.endswith('-after-import'):
                                    # Cached private config must not mask changed environment.
                                    assert Path(config.load_config_file()[1]) == path
                                    assert dict(loader.cfg) == dict(connect_attempts='1',
                                        open_port_attempts='1', write_block_attempts='1')
                                elif damage != 'tamper':
                                    # Prove the real SDK permissively selected ambient config.
                                    assert Path(config.load_config_file()[1]) == directories[0] / 'esptool.cfg'
                                    assert loader.cfg['open_port_attempts'] == '99'
                                with mock.patch.object(esptool, 'main', side_effect=AssertionError('CLI forbidden')) as cli:
                                    try:
                                        worker.tool('identity', Path(root), path)
                                        raise AssertionError('damaged config was admitted')
                                    except (RuntimeError, OSError):
                                        pass
                                    cli.assert_not_called()
                                    port.assert_not_called()
                """), sdk=True)


if __name__ == "__main__":
    unittest.main()
