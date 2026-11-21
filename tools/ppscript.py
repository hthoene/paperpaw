r"""The test script language of tools/x4-remote.py (`.pps` files), one for the X4
and for the emulator: this is its engine (parser, variables, loops, measurements,
checks, reports). The steps that touch the device or the emulator (a key, a
screenshot, a card file) are x4-remote.py's own; what is described in its usage
text and in docs/device-test-scripts.md.

Nothing here talks to a device: the engine is given an object with the X4 class's
methods (`ask`, `pass_time`, `parts`, `logbuf`, ...) and a `primitive(step)`
function for the steps it does not know, so tests run it against a stand-in.
"""

import fnmatch
import hashlib
import json
import math
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path


class ScriptError(Exception):
    """The script is wrong (found before it runs, or by a step that cannot be)."""


class StepError(Exception):
    """A step failed: the device said ERR, a file is missing, an unknown step."""


class LinkError(Exception):
    """The link to the device does not answer (cable, RFC2217 server, device)."""


class Stop(Exception):
    """The script ends here (`abort:`, `on-fail:stop`, a lost link)."""


class StepFailed(Exception):
    """A step failed inside a `test:`: that test fails, the next one runs."""


class SkipTest(Exception):
    """`skip:why` in a `test:` block: not testable here."""


STATS = ("median", "min", "max", "mean", "p90", "n", "last", "sum")
# Metrics only the X4 has: its times (on the emulator they are the host's), its gray passes.
TIMED = {"first_ms", "last_ms", "wall_ms", "drawn_ms", "shown_ms", "action_ms", "kernel_ms", "first_frame_s", "grays",
         "unprompted"}
BLOCKS = {"repeat", "measure", "test", "define", "for", "if", "else", "retry", "try"}
# Steps that only ask: after a lost link they may simply be asked again.
SAFE = {"heap", "describe", "screen", "screens", "ls", "stat", "md5", "cat", "settings", "setting", "usage", "time",
        "version", "plugins", "native", "actions", "boot", "expect", "expect-not", "until", "wait", "crashes"}
SECRET_NAME = re.compile(r"PASS|SECRET|TOKEN|KEY|PIN", re.I)
FRAME = re.compile(r"INFO - frame (?!in grays)(\w+) ")
FRAME_TIMES = re.compile(r"drawn in (\d+) ms.*?shown in (\d+) ms")
KEY_LINE = re.compile(r"INFO - ([\w-]+) pressed")
ACTION_LINE = re.compile(r"INFO - ([\w]+(?:\.[\w-]+)+): (\d+) ms$")
HIGH_WATER = re.compile(r"high-water: stack left (\d+) B.*heap free (\d+) B \(lowest (\d+)\), largest block (\d+) B \(lowest (\d+)\)")
HOSTSTAT = re.compile(r"hoststat [\w ]+: card in site 1: reads (\d+) cmds (\d+) blocks (\d+) ms, writes (\d+) cmds (\d+) blocks (\d+) ms")
WARN_LINE = re.compile(r"^(WARN|WARNING|ERROR|ERR)\b")
ANSI = re.compile(r"\x1b\[[0-9;]*m")


# --- the parser -------------------------------------------------------------------------

class Stmt:
    def __init__(self, text, line, file, children=None):
        self.text = text          # the step, or a block's header (without the brace)
        self.line = line
        self.file = file
        self.children = children  # None for a step, a list for a block
        self.kind = text.split(":", 1)[0].strip()

    def __repr__(self):
        return f"Stmt({self.text!r}, {self.children!r})" if self.children is not None else f"Stmt({self.text!r})"


def parse(text, name="<script>", inline=False):
    """The statements of a script. A step is a line (or, with `inline`, a piece
    between commas: `--script "a,b"`); a block is a header ending in `{`, the
    statements, and a `}`. `#` begins a comment line. `\\,`, `\\{` and `\\}` are the
    characters themselves."""
    pos = 0
    line = 1
    n = len(text)

    def fail(why):
        raise ScriptError(f"{name}:{line}: {why}")

    def statements(depth):
        nonlocal pos, line
        out = []
        while True:
            # Separators and comments before a statement.
            while pos < n:
                c = text[pos]
                if c == "\n":
                    line += 1
                    pos += 1
                elif c in " \t\r" or (c == "," and inline):
                    pos += 1
                elif c == "#":
                    while pos < n and text[pos] != "\n":
                        pos += 1
                else:
                    break
            if pos >= n:
                if depth:
                    fail("a `{` is not closed")
                return out
            if text[pos] == "}":
                if not depth:
                    fail("a `}` without a `{`")
                pos += 1
                return out
            start_line = line
            header = []
            ended = None
            while pos < n:
                c = text[pos]
                if c == "\\" and pos + 1 < n and text[pos + 1] in ",{}":
                    header.append(text[pos + 1])
                    pos += 2
                elif c == "$" and text.startswith("${", pos):
                    close = text.find("}", pos)
                    if close < 0:
                        fail("`${` is not closed")
                    header.append(text[pos:close + 1])
                    pos = close + 1
                elif c == "\n" or (c == "," and inline):
                    ended = c
                    break
                elif c in "{}":
                    ended = c
                    break
                else:
                    header.append(c)
                    pos += 1
            step = "".join(header).strip()
            if ended == "{":
                pos += 1
                head = step.split(":", 1)[0].strip()
                if head not in BLOCKS:
                    fail(f"`{head}` is not a block (blocks: {', '.join(sorted(BLOCKS))})")
                out.append(Stmt(step, start_line, name, statements(depth + 1)))
            elif step:
                out.append(Stmt(step, start_line, name))
            if ended == "}":
                # The `}` closes the block this statement is in; left for the loop above.
                continue

    return statements(0)


# --- numbers and text -------------------------------------------------------------------

def number(text):
    try:
        return float(text)
    except (TypeError, ValueError):
        return None


def fmt(value):
    if value is None:
        return "–"
    if isinstance(value, str):
        return value
    if float(value).is_integer():
        return str(int(value))
    return f"{value:.2f}"


def stat(samples, which):
    if not samples:
        return None
    if which == "median":
        return statistics.median(samples)
    if which == "min":
        return min(samples)
    if which == "max":
        return max(samples)
    if which == "mean":
        return statistics.fmean(samples)
    if which == "p90":
        ordered = sorted(samples)
        return ordered[min(len(ordered) - 1, math.ceil(0.9 * len(ordered)) - 1)]
    if which == "n":
        return len(samples)
    if which == "last":
        return samples[-1]
    if which == "sum":
        return sum(samples)
    raise ScriptError(f"no statistic `{which}` (median, min, max, mean, p90, n, last, sum)")


def calc(expression):
    """Integer/float arithmetic of numbers, + - * / ( ) and nothing else."""
    if not re.fullmatch(r"[\d\s+\-*/().]+", expression):
        raise StepError(f"`{expression}` is not arithmetic")
    return eval(expression, {"__builtins__": {}}, {})  # noqa: S307 (only digits and operators got here)


OPS = {
    "<=": lambda a, b: a <= b,
    ">=": lambda a, b: a >= b,
    "==": lambda a, b: a == b,
    "!=": lambda a, b: a != b,
    "<": lambda a, b: a < b,
    ">": lambda a, b: a > b,
}


class Masked:
    """Standard output with the secrets (passwords from the environment) blanked."""

    def __init__(self, inner, secrets):
        self.inner, self.secrets = inner, secrets

    def mask(self, text):
        for secret in self.secrets:
            if secret:
                text = text.replace(secret, "••••")
        return text

    def write(self, text):
        return self.inner.write(self.mask(text))

    def flush(self):
        self.inner.flush()

    def __getattr__(self, name):
        return getattr(self.inner, name)


# --- the engine ---------------------------------------------------------------------------

class Engine:
    def __init__(self, x4, primitive, target="x4", out=None, baseline=None, variables=None,
                 stop_on_fail=False, retries=2, reconnect=None, flasher=None):
        self.x4 = x4
        self.primitive = primitive
        self.target = target
        self.out = Path(out) if out else None
        self.baseline = baseline          # the results.json of an earlier run, or None
        self.stop_on_fail = stop_on_fail
        self.retries = retries
        self.reconnect = reconnect        # () -> None: opens the link again, or raises LinkError
        self.flasher = flasher            # object with check_chip(chip) and flash(image, chip, options), for `flash:`
        self.vars = {"target": target, "out": str(out or "."), "date": time.strftime("%Y-%m-%d"),
                     "run": time.strftime("%Y%m%d-%H%M%S")}
        self.vars.update(variables or {})
        self.secrets = set()
        self.defs = {}
        self.metrics = {}                 # "turn.first_ms" -> [samples]
        self.notes = {}                   # "boot.spi" -> text
        self.checks = []                  # one dict per check
        self.tests = []
        self.errors = []                  # steps that failed outright
        self.mark_at = 0                  # logbuf index `expect-log` looks from
        self.known_warns = []
        self.stopped = None
        self.depth = 0
        self.started = time.time()
        self.current = []                 # names of the tests being run
        self.ignore_errors = 0
        self.script_name = ""

    # -- output
    def say(self, text):
        print(text, flush=True)

    # -- variables
    def subst(self, text, line=None):
        def one(m):
            name = m.group(1) or m.group(2)
            if name == "$":
                return "$"
            if name.startswith("env:"):
                key = name[4:]
                value = os.environ.get(key)
                if value is None:
                    raise StepError(f"the environment has no {key}")
                if SECRET_NAME.search(key):
                    self.secrets.add(value)
                return value
            if name in self.vars:
                return str(self.vars[name])
            raise StepError(f"no variable `{name}`")

        return re.sub(r"\$(?:\{([^}]+)\}|([A-Za-z_][A-Za-z0-9_]*|\d+|\$))", one, text)

    # -- running
    def run_text(self, text, name="<script>", inline=False):
        self.script_name = self.script_name or name
        return self.run_statements(parse(text, name, inline))

    def run_file(self, path):
        path = Path(path)
        self.script_name = self.script_name or path.name
        return self.run_statements(parse(path.read_text(encoding="utf-8"), str(path)))

    def run(self, statements):
        """The whole script: stops cleanly (STOPPED marker, reports) on a stop."""
        saved = sys.stdout
        sys.stdout = Masked(saved, self.secrets)
        try:
            self.run_statements(statements)
        except Stop as stop:
            self.stopped = {"why": str(stop)}
            self.say(f"STOPPED: {stop}")
        except (StepError, LinkError, ScriptError) as error:
            self.errors.append(str(error))
            self.stopped = {"why": str(error)}
            self.say(f"STOPPED: {error}")
        finally:
            sys.stdout = saved
        self.finish()

    def run_statements(self, statements):
        for statement in statements:
            self.execute(statement)

    def execute(self, st):
        try:
            if st.children is not None:
                self.block(st)
            else:
                self.step(st)
        except (StepError, LinkError) as error:
            if isinstance(error, LinkError):
                if self.recover(st, error):
                    return
            if self.target == "emu" and "not on the emulator" in str(error):
                return self.check("step", True, f"`{st.text}`: not on the emulator", skip=True)
            if self.ignore_errors:
                self.say(f"IGNORED ({st.file}:{st.line} {st.text}): {error}")
                return
            where = f"{st.file}:{st.line}: `{st.text}`: {error}"
            self.errors.append(where)
            self.check("step", False, where)
            if self.current and not isinstance(error, LinkError):
                raise StepFailed(where) from error
            raise Stop(where) from error

    def recover(self, st, error):
        """The link lost during a step: open it again; asking steps are repeated."""
        if not self.reconnect:
            return False
        self.say(f"LINK: {error}; reconnecting")
        for attempt in range(1, self.retries + 1):
            try:
                self.reconnect()
            except LinkError as again:
                self.say(f"LINK: attempt {attempt}: {again}")
                continue
            self.vars["link_retries"] = int(self.vars.get("link_retries", 0)) + 1
            if st.kind in SAFE:
                self.step(st)
                return True
            self.check("link", False, f"link lost during `{st.text}` (line {st.line}); reconnected, the step is not repeated")
            return True
        raise Stop(f"link lost during `{st.text}` and not back after {self.retries} attempts")

    # -- checks and results
    def check(self, kind, ok, text, value=None, limit=None, baseline=None, skip=False):
        verdict = "SKIP" if skip else ("PASS" if ok else "FAIL")
        record = {"test": "/".join(self.current), "kind": kind, "verdict": verdict, "text": text,
                  "value": value, "limit": limit, "baseline": baseline}
        self.checks.append(record)
        self.say(f"{verdict}: {text}")
        if verdict == "FAIL":
            self.x4.failed.append(text)
            if self.stop_on_fail:
                raise Stop(f"check failed: {text}")

    def record(self, name, value):
        if value is None:
            return
        self.metrics.setdefault(name, []).append(float(value))

    def resolve(self, ref):
        """The value of a metric reference `name[.stat]` (default median), or a number."""
        direct = number(ref)
        if direct is not None:
            return direct, ref
        name, which = ref, "median"
        if name not in self.metrics and "." in name and name.rsplit(".", 1)[1] in STATS:
            name, which = name.rsplit(".", 1)
        if name in self.metrics:
            return stat(self.metrics[name], which), f"{name}.{which}"
        return None, f"{name}.{which}"

    def baseline_value(self, name, which):
        if not self.baseline:
            return None
        entry = self.baseline.get("metrics", {}).get(name)
        if not entry:
            return None
        return stat(entry.get("samples", []), which)

    # -- blocks
    def block(self, st):
        kind = st.kind
        rest = st.text.split(":", 1)[1].strip() if ":" in st.text else ""
        rest = self.subst(rest) if kind not in ("define", "if", "else") else rest
        if kind == "define":
            self.defs[rest] = st.children
        elif kind == "repeat":
            count, _, var = rest.partition(":")
            for i in range(1, self.int(count) + 1):
                self.vars[var or "i"] = i
                self.run_statements(st.children)
        elif kind == "retry":
            self.retry(st, self.int(rest))
        elif kind == "try":
            self.ignore_errors += 1
            try:
                self.run_statements(st.children)
            finally:
                self.ignore_errors -= 1
        elif kind == "for":
            self.loop(st, rest)
        elif kind == "if":
            self.last_if = self.condition(self.subst(rest))
            if self.last_if:
                self.run_statements(st.children)
        elif kind == "else":
            if getattr(self, "last_if", None) is False:
                self.run_statements(st.children)
            self.last_if = None
        elif kind == "test":
            self.test(st, rest)
        elif kind == "measure":
            self.measure(st, rest)

    def condition(self, text):
        kind, _, arg = text.partition(":")
        if kind == "shows":
            return self.x4.shows(arg)
        if kind == "not-shows":
            return not self.x4.shows(arg)
        if kind == "focused":
            return self.focused(arg)
        if kind == "log":
            return any(re.search(arg, s) for s in self.log_lines())
        if kind == "has":
            return self.resolve(arg)[0] is not None
        if kind == "exists":
            return self.x4.ask(f"stat {arg}", "STAT")[-1].split()[1] != "-"
        m = re.fullmatch(r"\s*(\S+?)\s*(<=|>=|==|!=|<|>)\s*(\S+)\s*", text)
        if not m:
            raise StepError(f"no condition: {text} (shows:T, not-shows:T, log:RE, has:METRIC, exists:PATH, A<=B)")
        left, op, right = m.groups()
        a, b = self.resolve(left)[0], self.resolve(right)[0]
        if a is None or b is None:
            a, b = left, right
        return OPS[op](a, b)

    def int(self, text):
        value = number(text)
        if value is None:
            raise StepError(f"`{text}` is not a number")
        return int(value)

    def retry(self, st, times):
        for attempt in range(1, times + 1):
            marks = (len(self.checks), len(self.x4.failed), len(self.errors))
            try:
                self.run_statements(st.children)
            except Stop:
                if attempt == times:
                    raise
                self.errors[marks[2]:] = []
            failed = [c for c in self.checks[marks[0]:] if c["verdict"] == "FAIL"]
            if not failed or attempt == times:
                return
            self.say(f"RETRY {attempt}/{times}: {len(failed)} check(s) failed, again")
            del self.checks[marks[0]:]
            del self.x4.failed[marks[1]:]

    def loop(self, st, rest):
        m = re.fullmatch(r"(\w+)\s+in\s+(.*)", rest)
        if not m:
            raise ScriptError(f"{st.file}:{st.line}: for:NAME in SOURCE")
        name, source = m.groups()
        kind, _, arg = source.partition(":")
        if kind == "list":
            values = [v for v in arg.split("|") if v]
        elif kind == "range":
            lo, _, hi = arg.partition("..")
            values = [str(i) for i in range(self.int(lo), self.int(hi) + 1)]
        elif kind == "ls":
            folder, _, pattern = arg.rpartition("/")
            if "*" not in pattern and "?" not in pattern:
                folder, pattern = arg, "*"
            values = [f"{folder}/{n}".lstrip("/") for d, _, n in self.x4.listing(folder) if not d and fnmatch.fnmatch(n, pattern)]
        elif kind == "local":
            import glob
            values = sorted(glob.glob(os.path.expanduser(arg)))
        else:
            raise ScriptError(f"{st.file}:{st.line}: no source `{kind}` (list:a|b, range:1..5, ls:folder/*.epub, local:glob)")
        for value in values:
            self.vars[name] = value
            p = Path(value)
            self.vars[f"{name}.name"], self.vars[f"{name}.stem"] = p.name, p.stem
            self.vars[f"{name}.dir"], self.vars[f"{name}.ext"] = str(p.parent), p.suffix.lstrip(".")
            self.run_statements(st.children)

    def test(self, st, name):
        self.current.append(name)
        first_check, first_error = len(self.checks), len(self.errors)
        started = time.time()
        self.mark_at = len(self.x4.logbuf)
        self.say(f"--- test {'/'.join(self.current)}")
        skipped = None
        try:
            self.run_statements(st.children)
        except SkipTest as skip:
            skipped = str(skip)
        except StepFailed:
            pass
        finally:
            mine = self.checks[first_check:]
            failed = [c for c in mine if c["verdict"] == "FAIL"]
            if skipped is not None:
                verdict = "NOT TESTABLE"
            elif failed or len(self.errors) > first_error:
                verdict = "FAIL"
            else:
                verdict = "PASS"
            self.tests.append({"name": "/".join(self.current), "verdict": verdict, "why": skipped or "",
                               "checks": len(mine), "failed": len(failed), "seconds": round(time.time() - started, 1)})
            self.say(f"TEST {'/'.join(self.current)}: {verdict}" + (f" ({skipped})" if skipped else ""))
            self.current.pop()

    # -- measurements
    def window(self, since, t_ref):
        """The metrics of the log lines since `since` (index into logbuf)."""
        lines = self.x4.logbuf[since:]
        out = {}
        frames = [(t, s) for t, s in lines if FRAME.search(s)]
        out["frames"] = len(frames)
        out["grays"] = sum(1 for _, s in lines if "frame in grays" in s)
        keys = [t for t, s in lines if KEY_LINE.search(s)]
        ref = keys[0] if keys else t_ref
        last_key = keys[-1] if keys else t_ref
        if frames:
            out["first_ms"] = (frames[0][0] - ref) * 1000
            out["last_ms"] = (frames[-1][0] - ref) * 1000
            m = FRAME_TIMES.search(frames[0][1])
            if m:
                out["drawn_ms"], out["shown_ms"] = float(m[1]), float(m[2])
        out["unprompted"] = sum(1 for t, _ in frames if t - last_key > 3.0)
        out["warns"] = sum(1 for _, s in lines if WARN_LINE.match(s) and not self.known(s))
        actions = [int(m[2]) for _, s in lines if (m := ACTION_LINE.search(s))]
        if actions:
            out["action_ms"] = sum(actions)
        water = [m for _, s in lines if (m := HIGH_WATER.search(s))]
        if water:
            out["hw_stack"], out["hw_free"], out["hw_free_low"] = (int(water[-1][i]) for i in (1, 2, 3))
            out["hw_block"], out["hw_block_low"] = int(water[-1][4]), int(water[-1][5])
        cards = [HOSTSTAT.search(s) for _, s in lines]
        cards = [m for m in cards if m]
        if cards:
            out["card_reads"] = sum(int(m[1]) for m in cards)
            out["card_read_ms"] = sum(int(m[3]) for m in cards)
            out["card_writes"] = sum(int(m[4]) for m in cards)
            out["card_write_ms"] = sum(int(m[6]) for m in cards)
        return out

    def known(self, line):
        return any(re.search(p, line) for p in self.known_warns)

    def measure(self, st, rest):
        name, *options = rest.split(":")
        opts = {}
        for item in options:
            key, _, value = item.partition("=")
            opts[key] = value
        n, warm = self.int(opts.get("n", 1)), self.int(opts.get("warm", 0))
        settle = self.int(opts.get("settle", 1500)) / 1000
        gap = self.int(opts.get("gap", 0)) / 1000
        # Asking the device costs it time: off unless wanted (`:heap=on`, `:usage=on`).
        want_usage = opts.get("usage", "off") == "on"
        want_heap = opts.get("heap", "off") == "on"
        if not name:
            raise ScriptError(f"{st.file}:{st.line}: measure:NAME[:n=5][:settle=1500][:warm=1][:gap=0][:usage=on][:heap=on]")
        self.say(f"--- measure {name} (n={n}{', warm=%d' % warm if warm else ''}, settle {int(settle * 1000)} ms)")
        for i in range(warm + n):
            before_heap = self.heap_now() if want_heap else None
            before_use = self.usage_now() if want_usage else None
            self.x4.drain()
            since = len(self.x4.logbuf)
            t0 = time.monotonic()
            self.run_statements(st.children)
            self.x4.pass_time(settle)
            took = (time.monotonic() - t0 - settle) * 1000
            got = self.window(since, t0)
            got["wall_ms"] = took
            after_heap = self.heap_now() if want_heap else None
            after_use = self.usage_now() if want_usage else None
            if after_heap:
                got.update(heap_used=after_heap["used"], heap_free=after_heap["free"], heap_block=after_heap["block"],
                           heap_peak=after_heap["peak"])
                if before_heap:
                    got["heap_block_drop"] = before_heap["block"] - after_heap["block"]
            if after_use and before_use:
                for key in after_use:
                    got[f"use_{key}"] = after_use[key] - before_use[key]
            if i >= warm:
                for key, value in got.items():
                    self.record(f"{name}.{key}", value)
                shown = ", ".join(f"{k} {fmt(v)}" for k, v in got.items() if k in ("first_ms", "last_ms", "frames", "grays", "warns"))
                self.say(f"  run {i - warm + 1}/{n}: {shown}")
            if gap:
                self.x4.pass_time(gap)
        parts = []
        for key in ("first_ms", "last_ms", "frames", "heap_block"):
            samples = self.metrics.get(f"{name}.{key}")
            if samples:
                parts.append(f"{key} median {fmt(stat(samples, 'median'))} (min {fmt(min(samples))}, max {fmt(max(samples))})")
        self.say(f"  {name}: " + "; ".join(parts))

    def heap_now(self):
        try:
            return self.x4.heap_values()
        except (StepError, LinkError):
            return None

    def usage_now(self):
        try:
            return self.x4.usage_totals()
        except (StepError, LinkError):
            return None

    # -- steps
    def step(self, st):
        text = self.subst(st.text)
        name, _, rest = text.partition(":")
        name = name.strip()
        handler = getattr(self, "do_" + name.replace("-", "_"), None)
        if handler:
            return handler(rest, st)
        self.primitive(text)
        # `heap` and `usage` print, and are measured too.
        if name == "heap":
            self.sample_heap()
        elif name == "usage":
            self.sample_usage()

    def sample_heap(self):
        values = self.x4.last_heap
        if values:
            for key in ("used", "free", "block", "peak", "stack"):
                if values.get(key) is not None:
                    self.record(f"heap.{key}", values[key])

    def sample_usage(self):
        values = self.x4.last_usage or {}
        for key, value in values.items():
            self.record(f"usage.{key}", value)

    def do_let(self, rest, st):
        key, eq, value = rest.partition("=")
        if not eq:
            raise ScriptError(f"{st.file}:{st.line}: let:NAME=VALUE")
        self.vars[key.strip()] = value.strip()

    def do_calc(self, rest, st):
        key, eq, expression = rest.partition("=")
        if not eq:
            raise ScriptError(f"{st.file}:{st.line}: calc:NAME=EXPRESSION")
        value = calc(expression)
        self.vars[key.strip()] = int(value) if float(value).is_integer() else value

    def do_echo(self, rest, st):
        self.say(rest)

    def do_section(self, rest, st):
        self.say(f"=== {rest}")

    def do_include(self, rest, st):
        path = Path(rest)
        if not path.is_absolute():
            path = Path(st.file).parent / path
        self.run_file(path)

    def do_call(self, rest, st):
        name, *args = rest.split(":")
        if name not in self.defs:
            raise StepError(f"nothing defined as `{name}`")
        saved = {k: v for k, v in self.vars.items() if re.fullmatch(r"\d+|#", k)}
        for i, value in enumerate(args, 1):
            self.vars[str(i)] = value
        self.vars["#"] = len(args)
        self.depth += 1
        if self.depth > 50:
            raise StepError("calls nested too deep")
        try:
            self.run_statements(self.defs[name])
        finally:
            self.depth -= 1
            for k in [k for k in self.vars if re.fullmatch(r"\d+|#", k)]:
                del self.vars[k]
            self.vars.update(saved)

    def do_mark(self, rest, st):
        self.x4.drain()
        self.mark_at = len(self.x4.logbuf)

    def do_skip(self, rest, st):
        raise SkipTest(rest or "skipped")

    def do_abort(self, rest, st):
        raise Stop(rest or "abort")

    def do_fail(self, rest, st):
        self.check("fail", False, rest or "fail")

    def do_on_fail(self, rest, st):
        if rest not in ("stop", "continue"):
            raise ScriptError(f"{st.file}:{st.line}: on-fail:stop or on-fail:continue")
        self.stop_on_fail = rest == "stop"

    def do_known_warn(self, rest, st):
        self.known_warns.append(rest)

    # screen checks (recorded as checks)
    def split_ms(self, rest, default):
        text, ms = rest, default
        if ":" in rest and rest.rsplit(":", 1)[1].isdigit():
            text, ms = rest.rsplit(":", 1)[0], int(rest.rsplit(":", 1)[1])
        return text, ms

    def do_until(self, rest, st):
        text, ms = self.split_ms(rest, 30000)
        deadline = time.monotonic() + ms / 1000
        while not self.x4.shows(text):
            if time.monotonic() > deadline:
                return self.check("until", False, f"until `{text}`: not shown in {ms} ms")
            self.x4.pass_time(0.5)
        self.say(f"until `{text}`: shown")

    def do_expect(self, rest, st):
        self.check("expect", self.x4.shows(rest), f"expect `{rest}` on the screen")

    def do_expect_not(self, rest, st):
        self.check("expect", not self.x4.shows(rest), f"expect `{rest}` not on the screen")

    def focused(self, text):
        return any("focused" in p[5] and text in p[6] for p in self.x4.parts())

    def do_expect_focus(self, rest, st):
        self.check("expect", self.focused(rest), f"expect `{rest}` focused")

    def reach(self, rest, key, what):
        text, max_presses = self.split_ms(rest, 40)
        for n in range(max_presses + 1):
            if self.focused(text):
                self.say(f"{what} `{text}`: focused after {n} press(es)")
                return True
            if n < max_presses:
                self.primitive(key)
        return self.check("focus", False, f"{what} `{text}`: not focused after {max_presses} presses of {key}")

    def do_focus(self, rest, st):
        """focus:Label[:max]: press `right` until the part with that label is focused."""
        self.reach(rest, "right", "focus")

    def do_select(self, rest, st):
        """select:Label[:max]: focus:Label, then confirm."""
        if self.reach(rest, "right", "select"):
            self.primitive("confirm")

    # log checks
    def log_lines(self):
        self.x4.drain()
        return [s for _, s in self.x4.logbuf[self.mark_at:]]

    def do_expect_log(self, rest, st):
        pattern, ms = self.split_ms(rest, 0)
        if ms:
            return self.wait_log(pattern, ms, "expect-log")
        found = [s for s in self.log_lines() if re.search(pattern, s)]
        self.check("log", bool(found), f"expect-log `{pattern}`" + (f" ({len(found)} lines)" if found else ""))

    def do_until_log(self, rest, st):
        pattern, ms = self.split_ms(rest, 30000)
        self.wait_log(pattern, ms, "until-log")

    def wait_log(self, pattern, ms, kind):
        deadline = time.monotonic() + ms / 1000
        while True:
            if any(re.search(pattern, s) for s in self.log_lines()):
                return self.check("log", True, f"{kind} `{pattern}`")
            if time.monotonic() > deadline:
                return self.check("log", False, f"{kind} `{pattern}`: no such line in {ms} ms")
            self.x4.pass_time(0.2)

    def do_no_log(self, rest, st):
        bad = [s for s in self.log_lines() if re.search(rest, s) and not self.known(s)]
        self.check("log", not bad, f"no-log `{rest}`" + (f": {len(bad)} line(s), first: {bad[0][:120]}" if bad else ""),
                   value=len(bad))

    def do_no_warn(self, rest, st):
        bad = [s for s in self.log_lines() if WARN_LINE.match(s) and not self.known(s)]
        self.check("log", not bad, "no WARN/ERROR lines" + (f": {len(bad)}, first: {bad[0][:120]}" if bad else ""), value=len(bad))

    do_no_warnings = do_no_warn

    def do_record(self, rest, st):
        key, eq, value = rest.partition("=")
        v = number(value)
        if not eq or v is None:
            raise ScriptError(f"{st.file}:{st.line}: record:NAME=NUMBER")
        self.record(key.strip(), v)

    def do_note(self, rest, st):
        key, _, value = rest.partition("=")
        self.notes[key.strip()] = value.strip()

    def do_assert(self, rest, st):
        m = re.fullmatch(r"\s*([^<>=!\s]+)\s*(<=|>=|==|!=|<|>)\s*([^\s:]+)(?::(.*))?", rest)
        if not m:
            raise ScriptError(f"{st.file}:{st.line}: assert:METRIC<=VALUE  (METRIC like turn.first_ms.median, VALUE a number or baseline*1.1)")
        lhs, op, rhs, why = m.groups()
        left, label = self.resolve(lhs)
        limit, base = None, None
        b = re.fullmatch(r"baseline(?:([*+\-])(\d+(?:\.\d+)?))?", rhs)
        if b:
            if number(lhs) is not None:
                raise ScriptError(f"{st.file}:{st.line}: baseline needs a metric on the left")
            name, which = label.rsplit(".", 1)
            base = self.baseline_value(name, which)
            if base is None:
                return self.check("assert", True, f"assert {lhs} {op} {rhs}: no baseline value", skip=True)
            limit = base
            if b[1]:
                limit = {"*": base * float(b[2]), "+": base + float(b[2]), "-": base - float(b[2])}[b[1]]
        else:
            limit = number(rhs)
            if limit is None:
                raise ScriptError(f"{st.file}:{st.line}: `{rhs}` is not a number")
        text = f"assert {lhs} {op} {rhs}" + (f" ({why})" if why else "")
        key = label.rsplit(".", 1)[0].rsplit(".", 1)[-1]
        if self.target == "emu" and key in TIMED:
            return self.check("assert", True, f"{text}: only the X4 has it (times, gray passes): not measurable on the emulator", skip=True)
        if left is None:
            if self.target == "emu":
                return self.check("assert", True, f"{text}: no data on the emulator", skip=True)
            return self.check("assert", False, f"{text}: no data for {label}")
        ok = OPS[op](left, limit)
        shown = f"{fmt(left)} {op} {fmt(limit)}"
        self.check("assert", ok, f"{text}: {shown}", value=left, limit=limit, baseline=base)

    # steps that need the device and results
    def do_reset(self, rest, st):
        seconds = self.int(rest or 40)
        self.x4.hard_reset(seconds)
        got = self.boot_metrics(self.x4.reset_log)
        self.say("boot: " + ", ".join(f"{k} {v}" for k, v in got.items()))

    def boot_metrics(self, lines):
        got = {}
        for t, s in lines:
            m = re.search(r"SPI Speed\s*: (\S+)", s)
            if m:
                self.notes["boot.spi"] = m[1]
                got["spi"] = m[1]
            m = re.search(r"kernel ready in (\d+) ms", s)
            if m:
                self.record("boot.kernel_ms", int(m[1]))
                got["kernel_ms"] = int(m[1])
            if "first_frame_s" not in got and FRAME.search(s):
                got["first_frame_s"] = round(t, 2)
                self.record("boot.first_frame_s", t)
        return got

    def do_clock(self, rest, st):
        """clock: asks the device's clock; records clock.known (1/0) and clock.skew_s (device - host, seconds)."""
        answer = self.x4.ask("time", "TIME")[-1].split()
        known = len(answer) > 1 and answer[1].isdigit()
        self.record("clock.known", 1 if known else 0)
        if known:
            self.record("clock.skew_s", round(int(answer[1]) / 1000 - time.time(), 1))
        if len(answer) > 2 and answer[2].isdigit():
            self.record("clock.uptime_s", int(answer[2]) / 1000)
        self.say("clock: " + (f"{time.strftime('%F %T', time.localtime(int(answer[1]) / 1000))}" if known else "unknown (--:--)"))

    # -- card workloads: files of known content (no real append exists on the remote: `grow` rewrites)
    @staticmethod
    def pattern(size, seed):
        """`size` bytes that depend on `seed` only, so a file can be checked later without keeping it."""
        out, n = bytearray(), 0
        while len(out) < size:
            out += hashlib.sha256(f"{seed}:{n}".encode()).digest()
            n += 1
        return bytes(out[:size])

    def card_file_args(self, rest, st, usage):
        parts = rest.split(":")
        if len(parts) != 3 or number(parts[1]) is None:
            raise ScriptError(f"{st.file}:{st.line}: {usage}")
        return parts[0], self.int(parts[1]), parts[2]

    def card_check(self, path, data):
        got = self.x4.ask(f"md5 {path}", "MD5")[-1].split()
        ok = got[1] == hashlib.md5(data).hexdigest() and int(got[2]) == len(data)
        self.check("card", ok, f"{path}: {len(data)} bytes as written" + ("" if ok else f" (card says MD5 {got[1]}, {got[2]} bytes)"))

    def do_fill(self, rest, st):
        """fill:PATH:SIZE:SEED - save (or replace) a file of SIZE bytes made from SEED; read back by MD5."""
        path, size, seed = self.card_file_args(rest, st, "fill:PATH:SIZE:SEED")
        data = self.pattern(size, seed)
        self.x4.write(path, data)
        self.record("card.writes", 1)
        self.card_check(path, data)

    def do_grow(self, rest, st):
        """grow:PATH:ADD:SEED - the file gets ADD more bytes at its end (it is read and written whole again; missing: created, above 1 MB: starts again)."""
        path, add, seed = self.card_file_args(rest, st, "grow:PATH:ADD:SEED")
        try:
            old = self.x4.read(path)
        except StepError:
            old = b""                 # not there yet: it starts now
        if len(old) > 1_000_000:
            old = b""                 # a soak does not fill the card: start again at 1 MB
        data = old + self.pattern(add, seed)
        self.x4.write(path, data)
        self.record("card.writes", 1)
        self.card_check(path, data)

    def do_check_file(self, rest, st):
        """check-file:PATH:SIZE:SEED - the file on the card is what `fill` made from SEED (SIZE bytes)."""
        path, size, seed = self.card_file_args(rest, st, "check-file:PATH:SIZE:SEED")
        self.card_check(path, self.pattern(size, seed))

    def do_nap(self, rest, st):
        """nap:SECONDS - one forced light sleep (the remote's `nap`), then a new start. Survived when `boot` says
        place 10; 11 means the X4 died in it (#434)."""
        if self.vars["target"] != "x4":
            return self.say("nap: not on the emulator")
        boot = self.x4.back_again(f"nap {self.int(rest or 3)}", "BOOT")
        self.check("nap", boot.split()[2] == "10", f"light sleep survived ({boot})")

    def do_crashes(self, rest, st):
        kind = self.x4.ask(f"stat .paperpaw/crashes.txt", "STAT")[-1].split()[1]
        count = 0
        if kind == "f":
            text = self.x4.read(".paperpaw/crashes.txt").decode("utf-8", "replace")
            for line in text.splitlines():
                if re.match(r"^\S+\t", line) or re.search(r"exception|panicked", line):
                    count += 1
        self.record("crashes.count", count)
        self.say(f"crashes.txt: {count} entries")

    def do_chip(self, rest, st):
        """chip:esp32c3 - stops unless the chip on the link is that one."""
        if not self.flasher:
            raise StepError("no flasher here (--esptool)")
        self.say(self.flasher.check_chip(rest))

    def do_flash(self, rest, st):
        image, *options = rest.split(":")
        opts = dict(o.partition("=")[::2] for o in options)
        chip = opts.get("chip")
        if not chip:
            raise ScriptError(f"{st.file}:{st.line}: flash:IMAGE:chip=esp32c3[:sha256=...][:offset=0x0][:erase=0xd000+0x2000]  (chip= is required: a wrong chip is never flashed)")
        if not self.flasher:
            raise StepError("flashing needs --esptool")
        sha = opts.get("sha256")
        if sha and hashlib.sha256(Path(image).read_bytes()).hexdigest() != sha.lower():
            raise StepError(f"{image}: sha256 differs from {sha}; not flashed")
        self.say(self.flasher.flash(image, chip, opts))

    # -- the end
    def finish(self):
        self.x4.drain()
        if self.out:
            self.out.mkdir(parents=True, exist_ok=True)
            write_reports(self)
            if self.stopped:
                (self.out / "STOPPED").write_text(self.stopped["why"] + "\n", encoding="utf-8")
        failed = [c for c in self.checks if c["verdict"] == "FAIL"]
        skipped = [c for c in self.checks if c["verdict"] == "SKIP"]
        self.say(f"{len(self.checks) - len(failed) - len(skipped)} check(s) passed, {len(failed)} failed"
                 + (f", {len(skipped)} skipped" if skipped else ""))
        for test in self.tests:
            self.say(f"  {test['verdict']:<13} {test['name']}")

    def exit_code(self):
        if self.stopped:
            return 2
        return 1 if any(c["verdict"] == "FAIL" for c in self.checks) or self.errors else 0


# --- reports --------------------------------------------------------------------------------

def warnings_of(lines):
    found = {}
    for _, s in lines:
        if WARN_LINE.match(s):
            key = re.sub(r"\d+", "N", s)[:160]
            found[key] = found.get(key, 0) + 1
    return found


def results(engine):
    metrics = {}
    for name, samples in sorted(engine.metrics.items()):
        metrics[name] = {"samples": samples, **{which: stat(samples, which) for which in STATS}}
    return {
        "run": {"script": engine.script_name, "target": engine.target, "started": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(engine.started)),
                "seconds": round(time.time() - engine.started, 1), "variables": {k: v for k, v in engine.vars.items() if "." not in k and not k.isdigit()
                                                                              and str(v) not in engine.secrets}},
        "tests": engine.tests,
        "checks": engine.checks,
        "metrics": metrics,
        "notes": engine.notes,
        "warnings": warnings_of(engine.x4.logbuf),
        "errors": engine.errors,
        "stopped": engine.stopped,
        "exit_code": engine.exit_code(),
    }


def write_reports(engine):
    data = results(engine)
    out = engine.out
    text = json.dumps(data, indent=1, ensure_ascii=False)
    for secret in engine.secrets:
        text = text.replace(secret, "••••")
    (out / "results.json").write_text(text + "\n", encoding="utf-8")
    log = "".join(f"{t - (engine.x4.logbuf[0][0] if engine.x4.logbuf else 0):9.3f} {s}\n" for t, s in engine.x4.logbuf)
    for secret in engine.secrets:
        log = log.replace(secret, "••••")
    (out / "device.log").write_text(log, encoding="utf-8")
    (out / "report.md").write_text(markdown(data, engine.baseline), encoding="utf-8")


def markdown(data, baseline):
    run = data["run"]
    lines = [f"# Test run: {run['script'] or 'script'} on {run['target']}", "",
             f"Started {run['started']}, {run['seconds']} s. " + ("**STOPPED:** " + data["stopped"]["why"] if data["stopped"] else
                                                                  "Exit code " + str(data["exit_code"]) + "."), ""]
    if run["target"] == "emu":
        lines += ["_On the emulator: times are the host's and heap sizes are the emulator's; checks on them are skipped._", ""]
    if data["tests"]:
        lines += ["## Tests", "", "| test | verdict | checks | failed | s |", "|---|---|---|---|---|"]
        for t in data["tests"]:
            lines.append(f"| {t['name']} | {t['verdict']}{' – ' + t['why'] if t['why'] else ''} | {t['checks']} | {t['failed']} | {t['seconds']} |")
        lines.append("")
    base = (baseline or {}).get("metrics", {})
    if data["metrics"]:
        head = "| metric | n | median | min | max |" + (" before (median) | change |" if baseline else "")
        lines += ["## Measurements", "", head, "|---|---|---|---|---|" + ("---|---|" if baseline else "")]
        for name, m in data["metrics"].items():
            row = f"| {name} | {fmt(m['n'])} | {fmt(m['median'])} | {fmt(m['min'])} | {fmt(m['max'])} |"
            if baseline:
                old = stat(base.get(name, {}).get("samples", []), "median")
                if old is None:
                    row += " – | |"
                else:
                    delta = m["median"] - old
                    pct = f" ({delta / old * 100:+.0f} %)" if old else ""
                    row += f" {fmt(old)} | {delta:+.2f}{pct} |"
            lines.append(row)
        lines.append("")
    if data["notes"]:
        lines += ["## Notes", ""] + [f"- {k}: {v}" for k, v in data["notes"].items()] + [""]
    failed = [c for c in data["checks"] if c["verdict"] == "FAIL"]
    lines += ["## Checks", "", f"{len(data['checks'])} checks, {len(failed)} failed.", ""]
    for c in data["checks"]:
        if c["verdict"] != "PASS":
            lines.append(f"- **{c['verdict']}** {c['text']}" + (f" _(in {c['test']})_" if c["test"] else ""))
    lines.append("")
    if data["warnings"]:
        old = set((baseline or {}).get("warnings", {}))
        lines += ["## WARN/ERROR lines in the device log", ""]
        for msg, count in sorted(data["warnings"].items(), key=lambda kv: -kv[1]):
            lines.append(f"- {count}× {msg}" + ("  **NEW**" if baseline and msg not in old else ""))
        lines.append("")
    return "\n".join(lines)
