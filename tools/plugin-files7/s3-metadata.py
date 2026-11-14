#!/usr/bin/env python3
"""The ESP32-S3's ABI metadata of every interface-7 plugin (target 9, abi/ppn5.md).

A plugin's `native/metadata.json` is its metadata for the X4 (profile
`esp32c3-riscv32imc-ilp32-soft-le`). The S3's is the same record for the profile
`esp32s3-xtensa-lx7-ilp32-windowed-le` (abi/entry.md): what the plugin uses and
needs does not depend on the chip. This writes `native/metadata.s3.inspected.json` and
`native/metadata.s3.bin` beside it through `tools/abi-metadata.py`, the one
generator, and `--check` fails if any is missing or differs (CI):

    tools/plugin-files7/s3-metadata.py [--check] [plugin-id ...]

Exit status: 0 done (or all current), 1 a file differs or is missing (--check) or
the generator refused one, 2 usage.
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
X4_TARGET = "esp32c3-riscv32imc-ilp32-soft-le"
S3_TARGET = "esp32s3-xtensa-lx7-ilp32-windowed-le"


def generate(folder: Path, scratch: Path) -> tuple[bytes, str]:
    record = json.loads((folder / "metadata.json").read_text())
    if record.get("target") != X4_TARGET:
        raise SystemExit(f"{folder}/metadata.json: target is {record.get('target')!r}, not {X4_TARGET}")
    record["target"] = S3_TARGET
    source = scratch / "in.json"
    source.write_text(json.dumps(record, indent=2) + "\n")
    out_bin, out_json = scratch / "metadata.s3.bin", scratch / "metadata.s3.inspected.json"
    for stale in (out_bin, out_json):
        stale.unlink(missing_ok=True)
    done = subprocess.run(
        [sys.executable, str(ROOT / "tools/abi-metadata.py"), "--schema", str(ROOT / "abi/abi.json"),
         "--input", str(source), "--bytes", str(out_bin), "--json", str(out_json)],
        capture_output=True, text=True,
    )
    if done.returncode != 0 or not out_bin.is_file() or not out_json.is_file():
        raise SystemExit(f"{folder}: the generator refused it:\n{done.stderr}{done.stdout}")
    return out_bin.read_bytes(), out_json.read_text()


def main(argv: list[str]) -> int:
    check = "--check" in argv
    ids = [a for a in argv if not a.startswith("--")]
    if any(a.startswith("--") and a != "--check" for a in argv):
        print(__doc__, file=sys.stderr)
        return 2
    folders = sorted(p.parent for p in (ROOT / "plugins").glob("*/native/metadata.json"))
    if ids:
        folders = [f for f in folders if f.parent.name in ids]
        if len(folders) != len(ids):
            print("no such plugin with a native/metadata.json", file=sys.stderr)
            return 2
    bad = 0
    with tempfile.TemporaryDirectory() as tmp:
        for folder in folders:
            data, text = generate(folder, Path(tmp))
            have_bin, have_json = folder / "metadata.s3.bin", folder / "metadata.s3.inspected.json"
            if check:
                if not have_bin.is_file() or have_bin.read_bytes() != data or not have_json.is_file() \
                        or have_json.read_text() != text:
                    print(f"{folder.parent.name}: native/metadata.s3.* is missing or not what the generator writes")
                    bad += 1
            else:
                have_bin.write_bytes(data)
                have_json.write_text(text)
    if check and bad:
        print(f"{bad} plugin(s): run tools/plugin-files7/s3-metadata.py", file=sys.stderr)
    return 1 if bad else 0


sys.exit(main(sys.argv[1:]))
