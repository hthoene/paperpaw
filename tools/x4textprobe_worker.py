"""Restricted child operations. No hardware imports/actions at import time.

Only the supervising runner calls main after explicit execution authorization.
All tool/serial work runs in a killable child under the parent's wall deadline.
"""
import argparse
import contextlib
import importlib.util
import os
from pathlib import Path
import sys
import time
import tempfile

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


def guard_serial(serial_class, preserve_input=False):
    """Refuse a second open and any I/O after an observed transport exception."""
    original_open = serial_class.open
    original_read, original_write = serial_class.read, serial_class.write
    opened = False

    def open_once(port, *args, **kwargs):
        nonlocal opened
        if opened or getattr(port, "is_open", False):
            raise RuntimeError("second open/reopen forbidden")
        opened = True  # A failed first-open attempt is terminal too.
        # RFC2217 transmits initial states; ign_set_control only ignores ACKs.
        # Set both while CLOSED, before even the SDK's Linux constructor opens.
        port.dtr = False
        port.rts = False
        if port.dtr is not False or port.rts is not False:
            raise RuntimeError("inactive control lines not established")
        port._probe_ever_opened = True
        return original_open(port, *args, **kwargs)

    def checked(operation, control=False):
        def io(port, *args, **kwargs):
            if getattr(port, "_probe_failed", False):
                raise RuntimeError("I/O after transport failure forbidden")
            try:
                return operation(port, *args, **kwargs)
            except BaseException as error:
                port._probe_failed = True
                if control and isinstance(error, OSError):
                    # ResetStrategy swallows EINVAL/ENOTTY and retries other
                    # OSErrors. Escape those handlers on the very first error.
                    raise RuntimeError("control-line failure; recovery forbidden") from error
                raise
        return io

    serial_class.open = open_once
    serial_class.read, serial_class.write = checked(original_read), checked(original_write)
    for name in ("setDTR", "setRTS", "_update_dtr_state", "_update_rts_state"):
        if hasattr(serial_class, name):
            setattr(serial_class, name, checked(getattr(serial_class, name), control=True))
    for name in ("dtr", "rts"):
        descriptor = getattr(serial_class, name, None)
        if isinstance(descriptor, property) and descriptor.fset is not None:
            setattr(serial_class, name, property(descriptor.fget,
                    checked(descriptor.fset, control=True), descriptor.fdel, descriptor.__doc__))
    for name in ("reset_input_buffer", "reset_output_buffer", "flushInput", "flushOutput", "flush"):
        if hasattr(serial_class, name):
            setattr(serial_class, name, checked(getattr(serial_class, name)))
    if preserve_input and hasattr(serial_class, "reset_input_buffer"):
        # RFC2217.open normally purges pending device logs. Application evidence
        # must survive first open; SDK ROM sessions retain their protocol flush.
        serial_class.reset_input_buffer = checked(lambda port: None)


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


SDK_CONFIG = ("[esptool]\nconnect_attempts = 1\n"
              "open_port_attempts = 1\nwrite_block_attempts = 1\n")


@contextlib.contextmanager
def sdk_environment():
    """Pin configuration BEFORE import; never consult ambient reset sequences."""
    if any(name == "esptool" or name.startswith("esptool.") for name in sys.modules):
        raise RuntimeError("esptool was imported before configuration isolation")
    saved = {key: value for key, value in os.environ.items() if key.startswith("ESPTOOL_")}
    with tempfile.TemporaryDirectory(prefix="textprobe-sdk-") as directory:
        path = Path(directory) / "esptool.cfg"
        path.write_text(SDK_CONFIG, encoding="ascii")
        path.chmod(0o400)
        for key in saved:
            del os.environ[key]
        os.environ.update(ESPTOOL_CFGFILE=str(path), ESPTOOL_OPEN_PORT_ATTEMPTS="1",
                          ESPTOOL_CONNECT_ATTEMPTS="1")
        try:
            yield path
        finally:
            for key in list(os.environ):
                if key.startswith("ESPTOOL_"):
                    del os.environ[key]
            os.environ.update(saved)


def check_sdk_config(path, config, loader):
    """The SDK is permissive on a bad override: require exact path/content."""
    parser, selected = config.load_config_file()
    expected = dict(connect_attempts="1", open_port_attempts="1", write_block_attempts="1")
    environment = {key: value for key, value in os.environ.items() if key.startswith("ESPTOOL_")}
    pinned = dict(ESPTOOL_CFGFILE=str(path), ESPTOOL_OPEN_PORT_ATTEMPTS="1",
                  ESPTOOL_CONNECT_ATTEMPTS="1")
    if (environment != pinned or path.read_text(encoding="ascii") != SDK_CONFIG or selected is None
            or Path(selected).resolve() != path.resolve()
            or dict(parser["esptool"]) != expected or dict(loader.cfg) != expected):
        raise RuntimeError("SDK config override missing, changed or ambient")


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


def tool(action, directory, config_path):
    import esptool
    import esptool.loader as loader
    import esptool.config as config
    import serial.rfc2217
    if esptool.__version__ != "5.6.0":
        raise RuntimeError("unreviewed esptool version")
    check_sdk_config(config_path, config, loader)
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


def remote(args, config_path=None):
    if args.action == "command":
        command, terminal = wire_command(args.command)
    if args.action == "boot":
        # Even boot imports must see the pinned config before any port opens.
        import esptool
        import esptool.config as config
        import esptool.loader as loader
        if esptool.__version__ != "5.6.0" or config_path is None:
            raise RuntimeError("unreviewed boot SDK/configuration")
        check_sdk_config(config_path, config, loader)
        from esptool.reset import HardReset
    import serial.rfc2217
    guard_serial(serial.rfc2217.Serial, preserve_input=True)
    spec = importlib.util.spec_from_file_location("remote", args.remote)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    port = module.open_port(PORT)
    try:
        if args.action == "boot":
            HardReset(port, uses_usb=True).reset()
        else:
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
    with sdk_environment() as config_path:
        if args.action in ("boot", "command"):
            remote(args, config_path)
        else:
            tool(args.action, args.directory, config_path)


if __name__ == "__main__":
    main()
