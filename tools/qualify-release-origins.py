#!/usr/bin/env python3
"""Download and audit released update origins on the host; never contact a device."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
TAGS = ("v0.6.0-beta.1", "v0.6.0-beta.2")
DEVICES = ("x4", "x4-pro")


def run(*args, cwd=ROOT):
    return subprocess.run(args, cwd=cwd, check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180).stdout.strip()


def verified(path):
    """Match one checksum record to this basename, never execute its filename."""
    fields = path.with_name(path.name + ".sha256").read_text().strip().split()
    if len(fields) != 2 or not re.fullmatch(r"[0-9a-fA-F]{64}", fields[0]):
        raise ValueError(f"{path.name}: expected one SHA-256 record")
    if fields[1].removeprefix("*") != path.name:
        raise ValueError(f"{path.name}: checksum names another file")
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != fields[0].lower():
        raise ValueError(f"{path.name}: checksum mismatch")
    return actual


def audit(args):
    rows = []
    manifest = []
    for tag in args.tag or TAGS:
        if not re.fullmatch(r"v0\.6\.0-beta\.[1-3]", tag):
            raise ValueError("origins must be v0.6.0-beta.1, .2 or .3")
        directory = args.cache / tag
        directory.mkdir(parents=True, exist_ok=True)
        for device in DEVICES:
            prefix = f"paperpaw-{device}-{tag[1:]}"
            names = [prefix + ending for ending in (".bin", "-install.bin", "-update.bin")]
            if args.download:
                for name in names:
                    if not all((directory / (name + suffix)).exists() for suffix in ("", ".sha256")):
                        run("gh", "release", "download", tag, "--repo", "hthoene/paperpaw",
                            "--dir", str(directory), "--clobber", "--pattern", name,
                            "--pattern", name + ".sha256")
            paths = [directory / name for name in names]
            hashes = {path.name: verified(path) for path in paths}
            checked = run(str(args.sign_tool), "check-image", str(paths[0]))
            signature = run(str(args.sign_tool), "show", str(paths[2]))
            rows.append({"tag": tag, "device": device, "sha256": hashes,
                         "check_image": checked, "signature": signature,
                         "actual_old_installer": "NOT_RUN", "device_result": "NOT_RUN"})
            manifest.append("\t".join((tag, device, *(str(p) for p in paths))))
    manifest_path = args.cache / "origins.tsv"
    manifest_path.write_text("\n".join(manifest) + "\n")
    env = dict(os.environ, PAPERPAW_RELEASE_ORIGINS=str(manifest_path))
    for device in DEVICES:
        env.pop("PAPERPAW_CANDIDATE_" + device.upper().replace("-", "_"), None)
    candidates = {}
    for item in args.candidate:
        device, separator, filename = item.partition("=")
        if not separator or device not in DEVICES or device in candidates:
            raise ValueError("candidate must be a unique x4=PATH or x4-pro=PATH")
        path = Path(filename).resolve()
        candidates[device] = {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        env["PAPERPAW_CANDIDATE_" + device.upper().replace("-", "_")] = str(path)
    command = ["cargo", "test", "--locked", "-p", "paperpaw-x4", "--test",
               "release_origins", "--", "--ignored", "--nocapture"]
    result = subprocess.run(command, cwd=ROOT, env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    report = {"source": run("git", "rev-parse", "HEAD"), "rows": rows,
              "host_test_command": command, "host_test_log": result.stdout,
              "host_result": "PASS" if result.returncode == 0 else "FAIL",
              "candidates": candidates,
              "target_image_fit": "HOST_CHECKED" if set(candidates) == set(DEVICES) and result.returncode == 0
              else "INCOMPLETE_OR_FAILED",
              "qualification": "INCOMPLETE_DEVICE_AND_OLD_INSTALLER_GATES"}
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(result.stdout)
    print(f"Host result: {report['host_result']}; full qualification INCOMPLETE; {args.out}")
    return result.returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--sign-tool", type=Path, required=True,
                        help="current checkout's built paperpaw-sign; read-only commands only")
    parser.add_argument("--candidate", action="append", default=[], metavar="DEVICE=APP_BIN",
                        help="optional final app-only image for fit/header checks against every origin")
    parser.add_argument("--tag", action="append", help="default: beta.1 and beta.2; .3 must exist")
    parser.add_argument("--download", action="store_true", help="fetch missing release asset/checksum pairs")
    args = parser.parse_args()
    args.cache = args.cache.resolve()
    args.sign_tool = args.sign_tool.resolve()
    args.out = args.out.resolve()
    if args.out.exists():
        parser.error("output already exists; use a new evidence filename")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    try:
        return audit(args)
    except (OSError, ValueError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        args.out.write_text(json.dumps({"host_result": "FAIL", "error": str(error),
                                       "qualification": "INCOMPLETE"}, indent=2) + "\n")
        print(f"origin audit failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
