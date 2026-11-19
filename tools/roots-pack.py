"""Builds the data plugin `tls-roots` (#387): Mozilla's whole list of root
certificates for the X4's TLS, which the firmware has only a small part of
built in (`tools/roots/builtin.txt`). The file `roots.pack` is signed with
PaperPaw's plugin key where the environment has it (`PAPERPAW_PLUGIN_KEY`, as
the store's signing step does; the key is on the disk only while it signs);
without it the file is not signed, and a device does not read it.

Usage:
    python tools/roots-pack.py OUT_DIR

Writes OUT_DIR/tls-roots; copy it into the device's plugins folder
(`plugins/tls-roots/roots.pack` is where the firmware looks). The release
builds it so (tools/plugin-files/data.py) and offers it in the store as "All
certificates", off at first; its version, names and descriptions are in
tools/plugin-files/data.toml.
"""

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools" / "plugin-files"))
import data  # noqa: E402

ID = "tls-roots"


def main():
    if len(sys.argv) != 2 or sys.argv[1].startswith("-"):
        sys.exit(__doc__)
    out = Path(sys.argv[1]) / ID
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "paperpaw-sign"],
                   cwd=ROOT, check=True)
    sign = ROOT / "target/release/paperpaw-sign"
    command = [str(sign), "roots-pack", "all", str(out / "roots.pack")]
    key = os.environ.get("PAPERPAW_PLUGIN_KEY")
    if key:
        with tempfile.TemporaryDirectory() as folder:
            file = Path(folder) / "plugin.key"
            fd = os.open(file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, "w") as handle:
                handle.write(key.strip() + "\n")
            subprocess.run(command + ["--key", str(file)], check=True)
    else:
        print("tools/roots-pack.py: no PAPERPAW_PLUGIN_KEY: roots.pack is NOT signed",
              file=sys.stderr)
        subprocess.run(command, check=True)
    fields = data.plugins()[ID]
    (out / "plugin.ini").write_text(
        data.plugin_ini(ID, fields), encoding="utf-8", newline="\n")
    for path, text in data.catalogs(ID, fields):
        (out / path).parent.mkdir(parents=True, exist_ok=True)
        (out / path).write_text(text, encoding="utf-8", newline="\n")
    shutil.copy(ROOT / "tools/licences/texts/CDLA-Permissive-2.0.txt",
                out / "LICENCE-CDLA-Permissive-2.0.txt")
    size = (out / "roots.pack").stat().st_size
    print(f"{out}: {size / 1024:.0f} KiB")


if __name__ == "__main__":
    main()
