#!/usr/bin/env python3
"""Builds the checked-out PaperPaw and flashes it to an Xteink reader over
USB, then shows its log (shells/xteink/README.md describes the steps this
runs, one by one).

Usage:
  python tools/flash-x4.py                  the X4 (ESP32-C3), log after
  python tools/flash-x4.py x4-pro           the X4 Pro (needs espup's `esp` toolchain)
  python tools/flash-x4.py x4-classic       the X4 Classic (experimental, as the Pro)
  python tools/flash-x4.py x3               the X3 (experimental, only unlocked units)
  python tools/flash-x4.py --port COM10     the port, where there are several
  python tools/flash-x4.py --no-monitor     flash and stop, no log
  python tools/flash-x4.py --yes            do not ask before flashing
  python tools/flash-x4.py --dry-run        say what would run, run nothing
  python tools/flash-x4.py --backup x4-stock-firmware.bin
                                            read the whole flash into a file first,
                                            to go back to what the reader had

It flashes what is checked out here: to flash `main`, switch to it first
(`git switch main && git pull`). The build is the developer firmware
(feature `remote`: USB log and control, see tools/x4-remote.py), as
`cargo run --release` in shells/xteink makes it; released firmware leaves
`remote` out. Books and settings on the card are not touched, the
partition table and the bootloader are written, and the OTA data is
erased so that the new image starts.

The tools it needs: Rust (rustup fetches the pinned release by itself),
espflash, CMake, Ninja and, for the X4 and the X3, clang (mbedTLS is
built with the firmware). For the X4 Pro and the X4 Classic also espup's
`esp` toolchain with its environment loaded (`export-esp.sh` or
`export-esp.ps1`): see shells/xteink/versions.env for the versions.
"""

import argparse
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIRMWARE = ROOT / "shells" / "xteink"

# The device: the cargo alias that builds, flashes and shows the log
# (shells/xteink/.cargo/config.toml), whether it needs the Xtensa
# toolchain, the chip for espflash, and the target's name (for its runner).
DEVICES = {
    "x4": {
        "name": "Xteink X4",
        "cargo": ["cargo", "run", "--release"],
        "xtensa": False,
        "chip": "esp32c3",
        "triple": "riscv32imc-unknown-none-elf",
        "partitions": None,
    },
    "x3": {
        "name": "Xteink X3 (experimental)",
        "cargo": ["cargo", "x3-run"],
        "xtensa": False,
        "chip": "esp32c3",
        "triple": "riscv32imc-unknown-none-elf",
        "partitions": None,
    },
    "x4-pro": {
        "name": "Xteink X4 Pro",
        "cargo": ["cargo", "+esp", "x4pro-run"],
        "xtensa": True,
        "chip": "esp32s3",
        "triple": "xtensa-esp32s3-none-elf",
        "partitions": None,
    },
    "x4-classic": {
        "name": "Xteink X4 Classic (experimental)",
        "cargo": ["cargo", "+esp", "x4classic-run"],
        "xtensa": True,
        "chip": "esp32s3",
        "triple": "xtensa-esp32s3-none-elf",
        # Its own partitions, which keep the factory's NVS
        # (shells/xteink/README.md, "The X4 Classic").
        "partitions": "partitions-classic.csv",
    },
}


def run(command, env=None, dry=False, cwd=FIRMWARE):
    """Runs `command`; returns its exit status. Prints it first."""
    shown = shlex.join(command)
    print(f"\n$ cd {cwd.relative_to(ROOT) if cwd != ROOT else '.'} && {shown}")
    if dry:
        return 0
    return subprocess.call(command, cwd=cwd, env=env)


def output(command, cwd=ROOT):
    try:
        done = subprocess.run(
            command, cwd=cwd, capture_output=True, text=True, check=False
        )
    except OSError:
        return ""
    return done.stdout.strip()


def pinned(name):
    """A version `shells/xteink/versions.env` pins, or None."""
    try:
        for line in (FIRMWARE / "versions.env").read_text().splitlines():
            if line.startswith(name + "="):
                return line.split("=", 1)[1].strip()
    except OSError:
        pass
    return None


def source_state():
    """The branch and commit that will be built, and whether it is clean."""
    branch = output(["git", "rev-parse", "--abbrev-ref", "HEAD"]) or "?"
    commit = output(["git", "rev-parse", "--short", "HEAD"]) or "?"
    dirty = output(["git", "status", "--porcelain"])
    return branch, commit, bool(dirty)


def problems(device, spec, need_clang):
    """What is missing for a build, one line each."""
    found = []
    for tool in ("cargo", "rustup", "espflash", "cmake", "ninja"):
        if shutil.which(tool) is None:
            hint = {
                "espflash": "cargo install espflash --version "
                f"{pinned('ESPFLASH_VERSION') or '4'} --locked",
                "rustup": "https://rustup.rs",
                "cmake": "install CMake",
                "ninja": "install Ninja",
            }.get(tool, "")
            found.append(f"`{tool}` is not on the PATH" + (f" ({hint})" if hint else ""))
    if need_clang and shutil.which("clang") is None and not os.environ.get("CLANG_PATH"):
        found.append(
            "clang is not on the PATH (set CLANG_PATH; "
            f"CI uses {pinned('MBEDTLS_CLANG_PACKAGE') or 'a recent clang'})"
        )
    if spec["xtensa"]:
        toolchains = output(["rustup", "toolchain", "list"])
        if "esp" not in toolchains:
            found.append(
                "the `esp` Rust toolchain is missing: espup install --targets esp32s3 "
                f"--toolchain-version {pinned('ESP_RUST_VERSION') or '?'} "
                f"--crosstool-toolchain-version {pinned('ESP_GCC_VERSION') or '?'}"
            )
        if shutil.which("xtensa-esp32s3-elf-gcc") is None:
            found.append(
                "the Xtensa GCC is not on the PATH: load espup's environment "
                "(export-esp.sh, or export-esp.ps1 in PowerShell) in this shell first"
            )
    return found


def main():
    parser = argparse.ArgumentParser(
        description="Builds the checked-out PaperPaw and flashes it to an Xteink reader.",
        epilog="See the top of this file, and shells/xteink/README.md.",
    )
    parser.add_argument("device", nargs="?", default="x4", choices=DEVICES)
    parser.add_argument("--port", help="the serial port, e.g. COM10 or /dev/ttyACM0")
    parser.add_argument("--no-monitor", action="store_true", help="flash, show no log")
    parser.add_argument("--yes", action="store_true", help="do not ask before flashing")
    parser.add_argument("--dry-run", action="store_true", help="print the steps, run none")
    parser.add_argument("--backup", metavar="FILE", help="save the reader's flash to FILE first")
    args = parser.parse_args()
    spec = DEVICES[args.device]

    env = dict(os.environ)
    # mbedTLS is built by CMake and Ninja (shells/xteink/README.md).
    env.setdefault("CMAKE_GENERATOR", "Ninja")
    if args.port:
        env["ESPFLASH_PORT"] = args.port

    branch, commit, dirty = source_state()
    print(f"Device:  {spec['name']}")
    print(f"Source:  {branch} @ {commit}" + ("  (with changes not committed)" if dirty else ""))
    if args.port:
        print(f"Port:    {args.port}")

    missing = problems(args.device, spec, need_clang=not spec["xtensa"])
    if missing:
        print("\nNot ready:")
        for line in missing:
            print(f"  - {line}")
        if not args.dry_run:
            return 1
        print("  (dry run: going on)")

    if args.backup:
        target = Path(args.backup)
        if target.exists():
            print(f"\n{target} is there already: not overwritten. Move it or name another file.")
            return 1
        backup = ["espflash", "read-flash", "0x0", "0x1000000", str(target)]
        if run(backup, env=env, dry=args.dry_run, cwd=ROOT) != 0:
            return 1

    if not args.yes and not args.dry_run:
        print(
            "\nThis replaces the firmware on the reader (the card is not touched). "
            "To be able to go back, save its flash first (--backup)."
        )
        if args.device == "x4-pro":
            print("A first flash over the stock firmware needs the power button held until it ends.")
        if input("Flash now? [y/N] ").strip().lower() not in ("y", "yes"):
            print("Not flashed.")
            return 1

    command = list(spec["cargo"])
    if args.no_monitor:
        # The runner of `cargo run` is `espflash flash --monitor ...`
        # (shells/xteink/.cargo/config.toml); the same without the log. A
        # `--config` beats the file's and the alias's own.
        runner = ["espflash", "flash", "--chip", spec["chip"], "--erase-parts", "otadata"]
        if spec["partitions"]:
            runner += ["--partition-table", spec["partitions"]]
        command += ["--config", f'target.{spec["triple"]}.runner = "{" ".join(runner)}"']

    status = run(command, env=env, dry=args.dry_run)
    if status != 0:
        print(f"\nFailed (status {status}): nothing more was done.")
    elif not args.dry_run and args.no_monitor:
        print("\nFlashed. The log: python tools/x4-remote.py --log 30")
    return status


if __name__ == "__main__":
    sys.exit(main())
