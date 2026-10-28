#!/usr/bin/env python3
"""Plan, verify or resume a crates.io workspace publication from a clean commit."""

import argparse
import concurrent.futures
import datetime as dt
import email.utils
import json
import pathlib
import re
import subprocess
import sys
import time
import tomllib
import urllib.error

import published_crates as registry

ROOT = pathlib.Path(__file__).resolve().parent.parent
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
DATE = r"(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun),?\s+\d{1,2}\s+\w+\s+\d{4}\s+\d{2}:\d{2}:\d{2}\s+(?:GMT|UTC|[+-]\d{4})"


def output(args):
    return subprocess.run(args, cwd=ROOT, check=True, capture_output=True, text=True).stdout


def clean_commit(expected=None):
    head = output(["git", "rev-parse", "HEAD"]).strip()
    if expected is not None and head != expected:
        raise ValueError("HEAD changed during publication; resume from the recorded commit")
    if output(["git", "status", "--porcelain", "--untracked-files=normal"]).strip():
        raise ValueError("the checkout must be clean, including untracked source files")
    return head


def packages():
    metadata = json.loads(output(["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"]))
    members = set(metadata["workspace_members"])
    return sorted((p for p in metadata["packages"] if p["id"] in members
                   and (p["publish"] is None or "crates-io" in p["publish"])), key=lambda p: p["name"])


def remaining(items, workspace):
    """Every reused version must have the same package sources, including libraries."""
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        entries = list(pool.map(lambda p: registry.published_version(p["name"], p["version"]), items))
    pending, existing = [], []
    for package, entry in zip(items, entries):
        name, version = package["name"], package["version"]
        if entry is None:
            pending.append(name)
            continue
        if entry["yanked"]:
            raise ValueError(f"{name} {version} is yanked; choose a new version")
        archived = registry.archive_files(name, version, entry["cksum"])
        names = output(["cargo", "package", "--list", "--no-verify", "--locked", "-p", name]).splitlines()
        folder = pathlib.Path(package["manifest_path"]).parent
        changes = registry.changed_files(folder, names, archived, workspace)
        if changes:
            raise ValueError(f"{name} {version} differs from crates.io ({', '.join(changes[:5])}); "
                             "choose a new version, never overwrite a published crate")
        existing.append(name)
    return pending, existing


def journal(path, commit, items):
    identity = {"commit": commit, "packages": {p["name"]: p["version"] for p in items}}
    data = json.loads(path.read_text(encoding="utf-8")) if path.exists() else identity | {"attempts": []}
    if any(data.get(key) != value for key, value in identity.items()):
        raise ValueError(f"{path}: publication journal belongs to another commit or package plan")
    return data


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def rate_limited(text):
    text = ANSI.sub("", text)
    return bool(re.search(r"(?:status[^\n]{0,60}\b429\b|\bgot\s+429\b|\(429\)|"
                          r"\b429\s+Too Many Requests\b)", text, re.IGNORECASE))


# The registry holds back new versions of existing crates for a while after a
# burst of them; a whole release can meet its limit many times. This many
# waits (each as long as the registry says) cover a release without a restart.
DEFAULT_MAX_RETRIES = 60


def retry_delay(text, attempt, now=None):
    """Honor server deadlines; use a bounded fallback only when no hint was given."""
    now = time.time() if now is None else now
    text = ANSI.sub("", text)
    hints = []
    for match in re.finditer(r"(?:Please try again after\s+|Retry-After\s*:\s*)([^\r\n]+)", text, re.IGNORECASE):
        hint = match[1].strip().strip("\"'")
        if re.fullmatch(r"\d+", hint):
            hints.append(float(hint))
        elif date := re.match(DATE, hint, re.IGNORECASE):
            when = email.utils.parsedate_to_datetime(date[0])
            hints.append(when.timestamp() - now)
    return max(0.0, max(hints)) + 5 if hints else min(30 * 2 ** min(attempt, 5), 900)


def wait_until(deadline):
    while (left := deadline - time.time()) > 0:
        when = dt.datetime.fromtimestamp(deadline, dt.timezone.utc).isoformat(timespec="seconds")
        print(f"Registry rate limit: retry after {when} ({left:.0f}s remaining).", flush=True)
        time.sleep(min(left, 30))


def run_cargo(args, log):
    tail = ""
    with log.open("w", encoding="utf-8") as stream:
        process = subprocess.Popen(args, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   text=True, encoding="utf-8", errors="replace")
        try:
            for line in process.stdout:
                print(line, end="", flush=True)
                stream.write(line)
                stream.flush()
                tail = (tail + line)[-65536:]
        except KeyboardInterrupt:
            process.terminate()
            process.wait()
            raise
        return process.wait(), tail


def command(existing, publish):
    args = ["cargo", "publish", "--workspace", "--locked"]
    if not publish:
        args.append("--dry-run")
    for name in existing:
        args.extend(["--exclude", name])
    return args


def execute(options):
    commit = clean_commit()
    items = packages()
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]
    path = options.journal or ROOT / "target/release" / f"publish-workspace-{commit[:12]}.json"
    data = journal(path, commit, items)
    retries = 0
    while True:
        clean_commit(commit)
        if options.publish and (deadline := data.get("retry_at")):
            wait_until(deadline)
        pending, existing = remaining(items, workspace)
        data.update({"pending": pending, "verified_existing": existing})
        if options.publish:
            data["retry_at"] = None
        save(path, data)
        print(f"Commit {commit}: {len(existing)} published and verified, {len(pending)} missing.", flush=True)
        for package in items:
            if package["name"] in pending:
                print(f"  {package['name']} {package['version']}", flush=True)
        print(f"Publication journal: {path}", flush=True)
        if not pending or not (options.publish or options.dry_run):
            return
        if set(pending) & set(data.get("awaiting_index", [])):
            if time.time() > data["index_deadline"]:
                raise ValueError("Cargo reported success, but the registry index still lacks packages; "
                                 "inspect the registry before resuming")
            print("Cargo reported success; waiting for registry visibility before any further upload.", flush=True)
            wait_until(time.time() + 30)
            continue
        clean_commit(commit)
        args = command(existing, options.publish)
        log = path.parent / f"{path.stem}-attempt-{len(data['attempts']) + 1}.log"
        event = {"at": dt.datetime.now(dt.timezone.utc).isoformat(), "command": args, "log": str(log)}
        data["attempts"].append(event)
        save(path, data)
        code, text = run_cargo(args, log)
        event["exit_code"] = code
        save(path, data)
        if code == 0:
            if options.dry_run:
                print("Cargo publish dry run passed; nothing uploaded.", flush=True)
                return
            # Do not resubmit a successful batch while an index cache
            # still has the old view. Require registry confirmation.
            data.update({"awaiting_index": pending, "index_deadline": time.time() + 600})
            save(path, data)
            continue
        if not options.publish or not rate_limited(text):
            raise ValueError(f"Cargo stopped with exit code {code}; see {log}. No automatic retry.")
        data["retry_at"] = time.time() + retry_delay(text, retries)
        save(path, data)
        if retries >= options.max_retries:
            raise ValueError(f"rate-limit retry limit reached; resume later with the same command ({path})")
        retries += 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--plan", action="store_true", help="verify existing registry packages and list missing ones (default)")
    mode.add_argument("--dry-run", action="store_true", help="also package and build missing crates without uploading")
    mode.add_argument("--publish", action="store_true", help="upload missing crates; resume safely after rate limits")
    parser.add_argument("--journal", type=pathlib.Path, help="journal path; default: target/release/publish-workspace-<commit>.json")
    parser.add_argument("--max-retries", type=int, default=DEFAULT_MAX_RETRIES,
                        help=f"automatic 429 retries this invocation (default: {DEFAULT_MAX_RETRIES})")
    options = parser.parse_args()
    if options.max_retries < 0:
        parser.error("--max-retries must be nonnegative")
    execute(options)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit("Interrupted; resume with the same command and commit.")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"publish-workspace.py: {error}")
