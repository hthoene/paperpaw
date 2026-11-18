#!/usr/bin/env python3
"""Unattended soak runner for the X4 (and the emulator), in the #310 script language (#230).

Each round it runs tools/pps/soak/cycle.pps through tools/x4-remote.py (one serial client at a time, the
link is closed between rounds), reads the round's results.json and appends one observation to
samples.csv / samples.jsonl in exactly the format of paperpaw-notes' scripts/check-stable-soak.py
(fixtures/stable-soak), plus telemetry.jsonl (crashes, WARN/ERR counts, uptime, ...). A stop file or
SIGINT/SIGTERM ends it cleanly between rounds; a lost link is retried; `resume` continues a run.

  soak.py start  NAME --port 'rfc2217://esp-host:4001?ign_set_control' --hours 72 [options]
  soak.py start  NAME --emulator BIN --library CARD --minutes 20 --label emulator
  soak.py resume NAME [--hours 72]     soak.py stop NAME     soak.py status NAME     soak.py finalize NAME

Never connects to port 4000 (the X4 Pro) unless SOAK_ALLOW_4000=1. Runs live in ~/dt-jobs/soak/runs/NAME
(SOAK_RUNS overrides). The README beside this file says what needs the owner's hands.
"""

import argparse
import csv
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
REMOTE = REPO / "tools" / "x4-remote.py"
RUNS = Path(os.environ.get("SOAK_RUNS", Path.home() / "dt-jobs" / "soak" / "runs"))
# The validator's columns, in its template's order (fixtures/stable-soak/samples.template.csv).
COLUMNS = ["elapsed_s", "phase", "cycle", "boot_id", "heap_used_b", "heap_free_b", "heap_peak_b", "stack_unused_b",
           "largest_free_b", "next_alloc_b", "allocation_margin_b", "reboots_total", "expected_reboots_total",
           "boot_kind", "boot_ms", "baseline_boot_ms", "current_ma", "baseline_current_ma", "battery_interval_s",
           "result", "trace"]
# check-stable-soak.py's thresholds (the live stop rule; the validator itself judges the finished run).
HEAP_PEAK_MAX = 222822
STACK_MIN = 10240
LINK_WORDS = re.compile(r"link|serial|reconnect|could not open|rfc2217|timed out|timeout|connection|broken pipe|Errno", re.I)
STOP = {"now": False}


def log(text):
    print(f"[soak {time.strftime('%F %T')}] {text}", flush=True)


def load(path, default=None):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return default


def save(path, data):
    tmp = Path(str(path) + ".tmp")
    tmp.write_text(json.dumps(data, indent=2, ensure_ascii=False))
    tmp.replace(path)


def run_dir(name):
    return RUNS / name


def python():
    return os.environ.get("PYTHON") or sys.executable


def remote_args(cfg):
    if cfg.get("emulator"):
        return ["--emulator", cfg["emulator"], "--library", cfg["library"], "--emu-arg=--wifi", "--emu-arg=online"]
    return ["--port", cfg["port"]]


def run_remote(cfg, extra, timeout, out_dir, stdout_name):
    """One x4-remote.py call; its output goes to out_dir/stdout_name. Returns (rc or None on timeout, text)."""
    out_dir.mkdir(parents=True, exist_ok=True)
    cmd = [python(), "-I", str(REMOTE), *remote_args(cfg), *extra]
    try:
        done = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, errors="replace")
        rc, text = done.returncode, done.stdout + ("\n--- stderr ---\n" + done.stderr if done.stderr.strip() else "")
    except subprocess.TimeoutExpired as error:
        rc = None
        text = ((error.stdout or b"").decode("utf-8", "replace") if isinstance(error.stdout, bytes) else (error.stdout or ""))
        text += f"\nTIMEOUT after {timeout} s"
    (out_dir / stdout_name).write_text(text)
    return rc, text


def event(rd, kind, **fields):
    with (rd / "events.jsonl").open("a") as stream:
        stream.write(json.dumps({"t": time.strftime("%FT%T"), "event": kind, **fields}) + "\n")
    log(f"{kind} {fields or ''}")


def discover_books(cfg, rd):
    rc, text = run_remote(cfg, ["--script", "ls:books"], 90, rd / "setup", "ls-books.log")
    books = []
    for line in text.splitlines():
        m = re.fullmatch(r"(.+?\.epub)\s+\d+", line.strip(), re.I)
        if m:
            books.append("books/" + m[1])
    return sorted(books)


def pick_round(cfg, state, n):
    """The books of round n: the always-read ones, then `per_round` of the rotating list."""
    chosen = list(cfg["books_always"])
    rotating = [b for b in state["books"] if b not in chosen]
    for j in range(min(cfg["books_per_round"], len(rotating))):
        chosen.append(rotating[((n - 1) * cfg["books_per_round"] + j) % len(rotating)])
    return chosen or state["books"][:1]


def every(n, step):
    return step > 0 and n % step == 0


def cycle_vars(cfg, state, n):
    books = pick_round(cfg, state, n)
    wifi = 0
    if every(n, cfg["wifi_every"]) and state["wifi_rounds"] < cfg["wifi_max"]:
        nets = cfg["wifi_nets"]
        wifi = nets[state["wifi_rounds"] % len(nets)]
    return {
        "cycle": n, "books": "|".join(books), "anchor": cfg.get("anchor") or state["books"][0],
        "turns": cfg["turns"], "turn_wait": cfg["turn_wait"], "wifi": wifi,
        "sleep": 1 if every(n, cfg["sleep_every"]) else 0, "card": 1 if every(n, cfg["card_every"]) else 0,
    }


def first_failure(results):
    for check in results.get("checks", []):
        if check.get("verdict") == "FAIL":
            return check.get("text", "failed check")
    stopped = results.get("stopped")
    return str(stopped) if stopped else None


def metric(results, name):
    m = (results.get("metrics") or {}).get(name)
    return m["last"] if m and m.get("samples") else None


def one_round(cfg, state, rd, n, attempt):
    """Runs round n once. Returns a dict: rc, results (or None), out (dir), text."""
    suffix = "" if attempt == 1 else f"-try{attempt}"
    out = rd / "cycles" / f"{n:05d}{suffix}"
    if out.exists():
        shutil.rmtree(out)
    extra = ["--script-file", str(HERE / "cycle.pps"), "--out", str(out)]
    for key, value in cycle_vars(cfg, state, n).items():
        extra += ["--var", f"{key}={value}"]
    rc, text = run_remote(cfg, extra, cfg["round_timeout"], out, "stdout.log")
    return {"rc": rc, "results": load(out / "results.json"), "out": out, "text": text}


def new_boot(state, results, text, out):
    """A restart happened since the last round: uptime went backwards, or the log shows a kernel start."""
    uptime = metric(results, "clock.uptime_s")
    reboot = False
    if uptime is not None:
        reboot = state["last_uptime_s"] is not None and uptime < state["last_uptime_s"]
        state["last_uptime_s"] = uptime
    try:
        log_text = (out / "device.log").read_text(errors="replace")
    except OSError:
        log_text = ""
    if re.search(r"kernel ready in \d+ ms|rst:0x|ets Jul|ESP-ROM", log_text) and state["cycles_done"] > 0:
        reboot = True
    return reboot


def threshold_failures(row):
    bad = []
    if row["heap_peak_b"] > HEAP_PEAK_MAX:
        bad.append(f"heap peak {row['heap_peak_b']} > {HEAP_PEAK_MAX}")
    if row["stack_unused_b"] != "" and row["stack_unused_b"] < STACK_MIN:
        bad.append(f"stack unused {row['stack_unused_b']} < {STACK_MIN}")
    if row["largest_free_b"] != "" and row["largest_free_b"] < row["next_alloc_b"] + row["allocation_margin_b"]:
        bad.append("largest free block below the next allocation + margin")
    if row["reboots_total"] != row["expected_reboots_total"]:
        bad.append("unexpected reboot")
    if row["result"] != "ok":
        bad.append(row["result"])
    return bad


def append_rows(rd, row, extra):
    with (rd / "samples.jsonl").open("a") as stream:
        stream.write(json.dumps(row) + "\n")
    rows = [json.loads(line) for line in (rd / "samples.jsonl").read_text().splitlines() if line.strip()]
    with (rd / "samples.csv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=COLUMNS)
        writer.writeheader()
        writer.writerows(rows)
    with (rd / "telemetry.jsonl").open("a") as stream:
        stream.write(json.dumps(extra) + "\n")


def append_raw(rd, n, attempt, out, text):
    """raw.log: every round's output, its device log and the clock, under a header the samples' `trace` points at."""
    with (rd / "raw.log").open("a") as stream:
        stream.write(f"=== cycle-{n:05d} attempt {attempt} {time.strftime('%FT%T')} ===\n{text}\n")
        try:
            stream.write(f"--- device.log cycle-{n:05d} ---\n" + (out / "device.log").read_text(errors="replace") + "\n")
        except OSError:
            pass


def sleep_until(deadline, rd):
    while time.time() < deadline and not STOP["now"] and not (rd / "STOP").exists():
        time.sleep(min(2.0, max(0.0, deadline - time.time())))


def stopping(rd):
    return STOP["now"] or (rd / "STOP").exists()


def end(rd, state, reason, failed=False):
    state["ended"] = {"at": time.strftime("%FT%T"), "reason": reason, "failed": failed}
    save(rd / "state.json", state)
    (rd / "STOPPED").write_text(reason + "\n")
    event(rd, "stopped", reason=reason, failed=failed)
    return 1 if failed else 0


def main_loop(rd, cfg, state):
    signal.signal(signal.SIGTERM, lambda *_: STOP.update(now=True))
    signal.signal(signal.SIGINT, lambda *_: STOP.update(now=True))
    (rd / "STOPPED").unlink(missing_ok=True)
    state.pop("ended", None)
    state["resumes"] = state.get("resumes", 0) + 1
    event(rd, "start" if state["resumes"] == 1 else "resume", cycles_done=state["cycles_done"])
    while True:
        elapsed = time.time() - state["start_epoch"]
        if stopping(rd):
            return end(rd, state, "stop file or signal: clean stop")
        if elapsed >= cfg["duration_s"]:
            return end(rd, state, f"duration reached ({int(elapsed)} s)")
        n = state["cycles_done"] + 1
        began = time.time()
        got = None
        for attempt in range(1, cfg["link_retries"] + 2):
            got = one_round(cfg, state, rd, n, attempt)
            append_raw(rd, n, attempt, got["out"], got["text"])
            has_heap = got["results"] is not None and metric(got["results"], "heap.used") is not None
            if has_heap or got["rc"] == 0:
                break
            if not (got["rc"] is None or LINK_WORDS.search(got["text"][-3000:])):
                break
            event(rd, "link-retry", cycle=n, attempt=attempt, rc=got["rc"])
            sleep_until(time.time() + 30 * attempt, rd)
            if stopping(rd):
                return end(rd, state, "stop file or signal: clean stop (during link retry)")
        results = got["results"]
        if results is None or metric(results, "heap.used") is None:
            reason = f"cycle {n}: no heap sample (rc {got['rc']}); the device or the link is gone"
            return end(rd, state, reason, failed=True)
        problem = first_failure(results) if got["rc"] != 0 else None
        reboot = new_boot(state, results, got["text"], got["out"])
        if reboot:
            state["reboots"] += 1
            state["boot"] += 1
        now_elapsed = round(time.time() - state["start_epoch"], 1)
        stack = metric(results, "heap.stack")
        row = {
            "elapsed_s": now_elapsed, "phase": "steady", "cycle": n, "boot_id": f"boot-{state['boot']:03d}",
            "heap_used_b": int(metric(results, "heap.used")), "heap_free_b": int(metric(results, "heap.free")),
            "heap_peak_b": int(metric(results, "heap.peak") or 0),
            "stack_unused_b": int(stack) if stack is not None else "",
            "largest_free_b": int(metric(results, "heap.block")), "next_alloc_b": cfg["next_alloc_b"],
            "allocation_margin_b": cfg["margin_b"], "reboots_total": state["reboots"],
            "expected_reboots_total": state["expected_reboots"], "boot_kind": "", "boot_ms": "",
            "baseline_boot_ms": "", "current_ma": "", "baseline_current_ma": "", "battery_interval_s": "",
            "result": "ok" if not problem and got["rc"] == 0 else f"fail: {problem or 'script rc ' + str(got['rc'])}"[:200],
            "trace": f"raw.log#cycle-{n:05d}",
        }
        planned = cycle_vars(cfg, state, n)
        warns = len(results.get("warnings") or [])
        errors = len(results.get("errors") or [])
        extra = {
            "cycle": n, "elapsed_s": now_elapsed, "round_s": round(time.time() - began, 1), "rc": got["rc"],
            "attempts": attempt, "crashes": metric(results, "crashes.count"), "warns": warns, "errors": errors,
            "uptime_s": metric(results, "clock.uptime_s"), "card_writes": metric(results, "card.writes"),
            "books": planned["books"], "wifi": planned["wifi"], "sleep": planned["sleep"], "reboot_seen": reboot,
            "failed_checks": [c["text"] for c in results.get("checks", []) if c.get("verdict") == "FAIL"][:5],
        }
        if extra["wifi"]:
            state["wifi_rounds"] += 1
        crashes = extra["crashes"]
        if crashes is not None and state["last_crashes"] is not None and crashes > state["last_crashes"]:
            row["result"] = row["result"] if row["result"] != "ok" else f"fail: crashes.txt grew {state['last_crashes']} -> {int(crashes)}"
        state["last_crashes"] = crashes if crashes is not None else state["last_crashes"]
        append_rows(rd, row, extra)
        state["cycles_done"] = n
        save(rd / "state.json", state)
        log(f"cycle {n}: heap {row['heap_used_b']} peak {row['heap_peak_b']} block {row['largest_free_b']} "
            f"stack {row['stack_unused_b']} crashes {crashes} warns {warns} {row['result']}")
        bad = threshold_failures(row)
        if bad and not cfg["keep_going"]:
            return end(rd, state, "stopped on failure, evidence kept: " + "; ".join(bad), failed=True)
        sleep_until(began + cfg["interval_s"], rd)


def parse_duration(args, default=None):
    if args.hours is not None:
        return int(args.hours * 3600)
    if args.minutes is not None:
        return int(args.minutes * 60)
    return default


def add_options(p, resume=False):
    p.add_argument("--hours", type=float)
    p.add_argument("--minutes", type=float)
    g = (lambda v: None) if resume else (lambda v: v)
    p.add_argument("--interval", type=int, default=g(600), help="seconds from the start of one round to the next (at most 900: the validator's gap)")
    p.add_argument("--books", help="books/a.epub|books/b.epub: the rotating list (default: every .epub of books/ on the card)")
    p.add_argument("--books-always", default=g(""), help="read in every round (a CJK book, a picture book, ...)")
    p.add_argument("--books-per-round", type=int, default=g(3))
    p.add_argument("--anchor", help="the checkpoint's book (default: the first book)")
    p.add_argument("--turns", type=int, default=g(30))
    p.add_argument("--turn-wait", type=int, default=g(1200))
    p.add_argument("--wifi-every", type=int, default=g(6), help="a Wi-Fi up/down every N rounds (0 never)")
    p.add_argument("--wifi-max", type=int, default=g(40), help="at most this many in the whole run")
    p.add_argument("--wifi-nets", default=g("1"), help="1 = Hannes, 2 = FRITZ!Box, e.g. 1,1,2")
    p.add_argument("--sleep-every", type=int, default=g(0), help="a power-button sleep/wake every N rounds (0 never)")
    p.add_argument("--card-every", type=int, default=g(1), help="the card file workload every N rounds (0 never)")
    p.add_argument("--next-alloc", type=int, default=g(32768), help="the largest next allocation of the frozen workload, bytes")
    p.add_argument("--margin", type=int, default=g(1024))
    p.add_argument("--round-timeout", type=int, default=g(1500))
    p.add_argument("--link-retries", type=int, default=g(5))
    p.add_argument("--keep-going", action="store_true", default=None if resume else False,
                   help="do not stop at the first failed check / threshold (default: stop, evidence kept)")


def config_from(args, old=None):
    cfg = dict(old or {})
    mapping = {"interval": "interval_s", "books_per_round": "books_per_round", "turns": "turns", "turn_wait": "turn_wait",
               "wifi_every": "wifi_every", "wifi_max": "wifi_max", "sleep_every": "sleep_every", "card_every": "card_every",
               "next_alloc": "next_alloc_b", "margin": "margin_b", "round_timeout": "round_timeout",
               "link_retries": "link_retries", "keep_going": "keep_going", "anchor": "anchor"}
    for arg, key in mapping.items():
        value = getattr(args, arg, None)
        if value is not None:
            cfg[key] = value
    if getattr(args, "books_always", None) is not None:
        cfg["books_always"] = [b for b in args.books_always.split("|") if b]
    if getattr(args, "wifi_nets", None) is not None:
        cfg["wifi_nets"] = [int(v) for v in args.wifi_nets.split(",") if v]
    duration = parse_duration(args)
    if duration is not None:
        cfg["duration_s"] = duration
    return cfg


def cmd_start(args):
    rd = run_dir(args.name)
    if rd.exists():
        sys.exit(f"{rd} exists: `resume` it, or pick another name")
    if args.emulator:
        if not args.library:
            sys.exit("--emulator needs --library (a card folder, written to: use a copy)")
        label = args.label or "emulator"
    else:
        if not args.port:
            sys.exit("--port needed (the X4 is rfc2217://esp-host:4001?ign_set_control)")
        if re.search(r":4000\b", args.port) and os.environ.get("SOAK_ALLOW_4000") != "1":
            sys.exit("port 4000 is the X4 Pro: never without the owner's word (SOAK_ALLOW_4000=1)")
        label = args.label or "x4"
    cfg = config_from(args)
    if "duration_s" not in cfg:
        sys.exit("--hours or --minutes needed")
    if cfg["interval_s"] > 900:
        sys.exit("--interval above 900 s breaks the validator's 15-minute gap rule")
    cfg.update(port=args.port, emulator=args.emulator, library=args.library, label=label,
               books_always=cfg.get("books_always", []))
    rd.mkdir(parents=True)
    state = {"start_epoch": time.time(), "started": time.strftime("%FT%T"), "cycles_done": 0, "boot": 1, "reboots": 0,
             "expected_reboots": 0, "last_uptime_s": None, "last_crashes": None, "wifi_rounds": 0, "books": []}
    if args.books:
        state["books"] = [b for b in args.books.split("|") if b]
    else:
        state["books"] = discover_books(cfg, rd)
    if not state["books"]:
        sys.exit("no books found on the card (books/*.epub): pass --books")
    save(rd / "config.json", cfg)
    save(rd / "state.json", state)
    write_metadata(rd, cfg, args)
    rc, text = run_remote(cfg, ["--script-file", str(HERE / "setup.pps"), "--out", str(rd / "setup" / "out")], 300, rd / "setup", "stdout.log")
    event(rd, "setup", rc=rc)
    if rc not in (0,):
        print(text[-1500:])
        sys.exit(f"setup failed (rc {rc}); see {rd}/setup")
    # The first checkpoint is the run's own start: boot / uptime are known from the setup.
    state["start_epoch"] = time.time()
    save(rd / "state.json", state)
    return main_loop(rd, cfg, state)


def write_metadata(rd, cfg, args):
    template = {
        "run_id": f"{cfg['label']}-{rd.name}", "device": "x4", "device_id": args.device_id or ("emulator" if cfg.get("emulator") else ""),
        "hardware": not cfg.get("emulator"), "source_commit": args.commit or "", "image_sha256": "",
        "compiler": args.compiler or "", "features": [f for f in (args.features or "").split(",") if f],
        "heap_budget_b": 262144, "fixture_id": args.fixture_id or "", "fixture_book_count": 3000,
        "threshold_review": args.threshold_review or "", "battery_instrument": args.battery_instrument or "",
        "card_swap_attended": False, "battery_cut_attended": False, "battery_cut_removed_usb_and_battery": False,
        "raw_log": {"path": "raw.log", "sha256": ""}, "baseline_log": {"path": "baseline.log", "sha256": ""},
        "before_books": {"path": "before-books.jsonl", "sha256": ""}, "after_books": {"path": "after-books.jsonl", "sha256": ""},
    }
    if args.image:
        template["image_sha256"] = hashlib.sha256(Path(args.image).read_bytes()).hexdigest()
    elif args.image_sha256:
        template["image_sha256"] = args.image_sha256
    save(rd / "metadata.json", template)


def cmd_resume(args):
    rd = run_dir(args.name)
    cfg, state = load(rd / "config.json"), load(rd / "state.json")
    if not cfg or not state:
        sys.exit(f"{rd}: nothing to resume")
    cfg = config_from(args, cfg)
    save(rd / "config.json", cfg)
    (rd / "STOP").unlink(missing_ok=True)
    return main_loop(rd, cfg, state)


def cmd_stop(args):
    rd = run_dir(args.name)
    (rd / "STOP").write_text("stop\n")
    print(f"{rd}/STOP written: the run stops after its current round")


def cmd_status(args):
    rd = run_dir(args.name)
    state, cfg = load(rd / "state.json"), load(rd / "config.json")
    if not state:
        sys.exit(f"{rd}: no such run")
    last = None
    try:
        lines = (rd / "samples.jsonl").read_text().splitlines()
        last = json.loads(lines[-1]) if lines else None
    except OSError:
        pass
    print(json.dumps({"run": args.name, "cycles": state["cycles_done"], "elapsed_s": round(time.time() - state["start_epoch"]),
                      "duration_s": cfg["duration_s"], "boot": state["boot"], "reboots": state["reboots"],
                      "ended": state.get("ended"), "last": last}, indent=2))


def cmd_finalize(args):
    """raw.log's hash into metadata.json, then the validator (if its path is known)."""
    rd = run_dir(args.name)
    meta = load(rd / "metadata.json")
    for key, src in (("baseline_log", args.baseline_log), ("before_books", args.before_books), ("after_books", args.after_books)):
        if src:
            target = rd / meta[key]["path"]
            shutil.copyfile(src, target)
    for key in ("raw_log", "baseline_log", "before_books", "after_books"):
        path = rd / meta[key]["path"]
        meta[key]["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else ""
    save(rd / "metadata.json", meta)
    candidates = [args.validator, os.environ.get("SOAK_VALIDATOR"),
                  str(REPO.parent / "paperpaw-notes" / "scripts" / "check-stable-soak.py")]
    validator = next((c for c in candidates if c and Path(c).is_file()), None)
    if not validator:
        print("metadata.json written; no validator found (--validator PATH to paperpaw-notes' scripts/check-stable-soak.py)")
        return 0
    done = subprocess.run([python(), "-I", validator, "--metadata", str(rd / "metadata.json"), "--samples", str(rd / "samples.csv")],
                          capture_output=True, text=True)
    (rd / "validator.json").write_text(done.stdout)
    print(done.stdout)
    return done.returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    start = sub.add_parser("start")
    start.add_argument("name")
    start.add_argument("--port")
    start.add_argument("--emulator")
    start.add_argument("--library")
    start.add_argument("--label", help="x4 (default on a port) or emulator (default with --emulator): part of run_id")
    start.add_argument("--device-id")
    start.add_argument("--commit")
    start.add_argument("--image", help="the flashed image file: its SHA-256 goes into metadata.json")
    start.add_argument("--image-sha256")
    start.add_argument("--compiler")
    start.add_argument("--features", help="comma separated")
    start.add_argument("--fixture-id")
    start.add_argument("--threshold-review")
    start.add_argument("--battery-instrument")
    add_options(start)
    resume = sub.add_parser("resume")
    resume.add_argument("name")
    add_options(resume, resume=True)
    for name in ("stop", "status"):
        sub.add_parser(name).add_argument("name")
    fin = sub.add_parser("finalize")
    fin.add_argument("name")
    fin.add_argument("--validator")
    fin.add_argument("--baseline-log")
    fin.add_argument("--before-books")
    fin.add_argument("--after-books")
    args = parser.parse_args()
    code = {"start": cmd_start, "resume": cmd_resume, "stop": cmd_stop, "status": cmd_status, "finalize": cmd_finalize}[args.command](args)
    sys.exit(code or 0)


if __name__ == "__main__":
    main()
