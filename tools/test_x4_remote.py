"""Offline checks for native plugin trust metadata; never opens a serial port."""
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("x4_remote", Path(__file__).with_name("x4-remote.py"))
remote = importlib.util.module_from_spec(SPEC)
# The tested helpers never use serial I/O. CI need not install pyserial.
with mock.patch.dict(sys.modules, {"serial": mock.Mock(), "serial.tools": mock.Mock()}):
    SPEC.loader.exec_module(remote)


class PluginDigest(unittest.TestCase):
    def test_native_header_formats_use_their_actual_digest(self):
        digest = bytes(range(32))
        for version, at in [(2, 48), (3, 60), (4, 84), (5, 64)]:
            with self.subTest(version=version):
                header = bytearray(at + 32)
                header[:6] = b"PPNP" + version.to_bytes(2, "little")
                header[at:] = digest
                self.assertEqual(remote.plugin_digest(header + b"image"), digest)

    def test_bad_unknown_and_truncated_headers_are_rejected(self):
        for data in [b"", b"PPNP", b"OTHER!", b"PPNP\x01\x00" + bytes(92),
                     b"PPNP\x06\x00" + bytes(120), b"PPNP\x02\x00" + bytes(73),
                     b"PPNP\x03\x00" + bytes(85), b"PPNP\x04\x00" + bytes(109), b"PPNP\x05\x00" + bytes(89)]:
            with self.subTest(data=data[:6]):
                with self.assertRaises(ValueError):
                    remote.plugin_digest(data)

    def test_retrust_replaces_only_the_named_plugins_entry(self):
        digest = bytes(range(32))
        header = b"PPNP\x03\x00" + bytes(54) + digest
        files = {"plugins/fixture/code.ppn": header,
                 "plugins/fixture/plugin.ini": b"[plugin]\nid = ota-smoke\n",
                 ".paperpaw/plugin-code.txt": b"other\tkeep\nota-smoke\twrong\n"}
        device = object.__new__(remote.X4)
        device.read = files.__getitem__
        device.write = files.__setitem__
        device.ask = lambda *_: ["STAT f 0"]
        device.trust_code("fixture")
        self.assertEqual(files[".paperpaw/plugin-code.txt"],
                         f"other\tkeep\nota-smoke\t{digest.hex()}\n".encode())


if __name__ == "__main__":
    unittest.main()
