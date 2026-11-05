"""Restricted child operations. No hardware imports/actions at import time.

Only the supervising runner calls main after explicit execution authorization.
All tool/serial work runs in a killable child under the parent's wall deadline.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import sys
import time

PORT = "rfc2217://esp-host:4000?ign_set_control"
MAC = "f8:5b:1b:5d:a2:ac"
COMMANDS = (
    "setting:core.sleep-after", "set:core.sleep-after=0",
    "setting:x4.power-off-after",
    "set:x4.power-off-after=x4.power-off-after.never",
    "version", "boot", "wifi:status",
    *(f"textprobe ascii {path} cold clean raster"
      for path in ("control", "direct", "adapter")),
)


def guard_serial(serial_class):
    """Refuse a second open and any I/O after an observed transport exception."""
    original_open = serial_class.open
    original_read, original_write = serial_class.read, serial_class.write

    def open_once(port, *args, **kwargs):
        if getattr(port, "_probe_ever_opened", False):
            raise RuntimeError("reopen forbidden")
        port._probe_ever_opened = True
        return original_open(port, *args, **kwargs)

    def checked(operation):
        def io(port, *args, **kwargs):
            if getattr(port, "_probe_failed", False):
                raise RuntimeError("I/O after transport failure forbidden")
            try:
                return operation(port, *args, **kwargs)
            except BaseException:
                port._probe_failed = True
                raise
        return io

    serial_class.open = open_once
    serial_class.read, serial_class.write = checked(original_read), checked(original_write)


def retry_policy(loader):
    """Disable known esptool 5.6.0 block and full-image recovery paths."""
    loader.WRITE_BLOCK_ATTEMPTS = 1
    loader.ESPLoader.WRITE_FLASH_ATTEMPTS = 1
    loader.DEFAULT_OPEN_PORT_ATTEMPTS = 1
    # CLI reads these environment overrides separately from loader constants.
    os.environ["ESPTOOL_OPEN_PORT_ATTEMPTS"] = "1"
    os.environ["ESPTOOL_CONNECT_ATTEMPTS"] = "1"
    assert os.environ["ESPTOOL_OPEN_PORT_ATTEMPTS"] == "1"
    assert loader.WRITE_BLOCK_ATTEMPTS == 1
    assert loader.ESPLoader.WRITE_FLASH_ATTEMPTS == 1
    assert loader.DEFAULT_OPEN_PORT_ATTEMPTS == 1


def guard_protocol(loader):
    """Turn first protocol failure into a non-recoverable worker error.

    esptool's sync/autodetection loops catch FatalError even with attempts=1.
    Conversion prevents those handlers from retrying. A shared connect latch
    also forbids a second reset/connect across different loader instances.
    """
    connected = False

    def checked(operation):
        def once(device, *args, **kwargs):
            if getattr(device, "_probe_protocol_failed", False):
                raise RuntimeError("protocol I/O after failure forbidden")
            try:
                return operation(device, *args, **kwargs)
            except loader.FatalError as error:
                device._probe_protocol_failed = True
                if hasattr(device, "_port"):
                    device._port._probe_failed = True
                raise RuntimeError("protocol failure; recovery forbidden") from error
        return once

    for name in ("command", "check_command", "sync", "get_chip_id",
                 "get_security_info", "read_reg"):
        setattr(loader.ESPLoader, name, checked(getattr(loader.ESPLoader, name)))
    connect = checked(loader.ESPLoader.connect)

    def connect_once(device, *args, **kwargs):
        nonlocal connected
        if connected:
            raise RuntimeError("second connect/reset forbidden")
        connected = True
        return connect(device, *args, **kwargs)
    loader.ESPLoader.connect = connect_once


def tool_args(action, directory):
    prefix = ["--chip", "auto" if action == "identity" else "esp32c3",
              "--port", PORT, "--connect-attempts", "1", "--before",
              "default-reset" if action == "identity" else "no-reset",
              "--after", "no-reset" if action == "identity" else "no-reset-stub"]
    operations = {
        "identity": ["--no-stub", "read-mac"],
        "bootloader": ["read-flash", "0x0", "0x8000", str(directory / "bootloader.bin")],
        "partitions": ["read-flash", "0x8000", "0x1000", str(directory / "partitions.bin")],
        "otadata": ["read-flash", "0xd000", "0x2000", str(directory / "otadata.bin")],
        "flash": ["write-flash", "0x10000", str(directory / "application.bin")],
    }
    return prefix + operations[action]


def tool(action, directory):
    import esptool
    import esptool.loader as loader
    import serial.rfc2217
    if esptool.__version__ != "5.6.0":
        raise RuntimeError("unreviewed esptool version")
    retry_policy(loader)
    guard_protocol(loader)
    guard_serial(serial.rfc2217.Serial)
    print("RUNNER_POLICY block=1 image=1 open=1 connect=1", flush=True)
    esptool.main(tool_args(action, directory))


def wire_command(command):
    """Match tools/x4-remote.py's native command/ack grammar."""
    if command not in COMMANDS:
        raise RuntimeError("command not in fixed procedure")
    if command.startswith("setting:"):
        return "setting " + command.split(":", 1)[1], "PP:SETTING "
    if command.startswith("set:"):
        key, value = command[4:].split("=", 1)
        return f"set {key} {value.encode().hex()}", "PP:OK"
    if command == "wifi:status":
        return "wifi status", "PP:WIFI "
    if command == "boot":
        return "boot", "PP:NAPS "
    return command, "PP:END"


def remote(args):
    import serial.rfc2217
    guard_serial(serial.rfc2217.Serial)
    spec = importlib.util.spec_from_file_location("remote", args.remote)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    port = module.open_port(PORT)
    try:
        if args.action == "boot":
            from esptool.reset import HardReset
            HardReset(port, uses_usb=True).reset()
        else:
            command, terminal = wire_command(args.command)
            port.write(command.encode("ascii") + b"\n")
        # Parent enforces the absolute deadline, including open/close. No retries.
        data = b""
        drain_until = None
        while True:
            if drain_until is not None and time.monotonic() >= drain_until:
                return
            chunk = port.read(port.in_waiting or 1)
            if not chunk:
                continue
            # Stream the entire raw chunk before parsing or returning: the
            # parent must see partial/trailing fatal evidence immediately.
            sys.stdout.buffer.write(chunk)
            sys.stdout.buffer.flush()
            data += chunk
            if len(data) > 65536:
                raise RuntimeError("unterminated remote output")
            done = False
            while b"\n" in data:
                line, data = data.split(b"\n", 1)
                text = line.decode("utf-8", "strict").rstrip("\r")
                if args.action == "boot":
                    if "startup: kernel ready" in text:
                        done = True
                elif text.startswith(terminal):
                    if args.command == "boot":
                        # Existing remote.boot drains trailing panic evidence.
                        drain_until = time.monotonic() + 0.5
                    else:
                        done = True
            if done:
                return
    finally:
        port.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--action", required=True, choices=(
        "identity", "bootloader", "partitions", "otadata", "flash", "boot", "command"))
    parser.add_argument("--remote", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--approval-ref", required=True)
    parser.add_argument("--command", default="")
    args = parser.parse_args()
    if not args.approval_ref.strip():
        parser.error("execution approval reference required")
    if args.action in ("boot", "command"):
        remote(args)
    else:
        tool(args.action, args.directory)


if __name__ == "__main__":
    main()
