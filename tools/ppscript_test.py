"""Tests of the script language (tools/ppscript.py) and of x4-remote.py's end of it, with a stand-in
for the X4: no device, no emulator. Run: python3 -I tools/ppscript_test.py"""

import importlib.util
import json
import sys
import tempfile
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


pp = load("ppscript", "ppscript.py")


class FakeX4:
    """What the engine asks of the X4 class; `primitive` plays the device: a press logs a
    key line and a frame a little later."""

    def __init__(self):
        self.failed = []
        self.logbuf = []
        self.seen = []
        self.screen = ["Home", "Library"]
        self.last_heap = None
        self.last_usage = None
        self.reset_log = []
        self.block = 50000
        self.turn_delay = 0.05
        self.files = {"books": [(False, 1, "a.epub"), (False, 2, "b.epub"), (True, 0, "sub")]}
        self.link_down = 0

    def log(self, text):
        self.logbuf.append((time.monotonic(), text))

    def primitive(self, step):
        self.seen.append(step)
        if self.link_down:
            self.link_down -= 1
            raise pp.LinkError("no answer")
        name = step.split(":")[0]
        if name == "page-forward":
            self.log("INFO - page-forward pressed")
            self.logbuf.append((time.monotonic() + self.turn_delay, "INFO - frame Partial 0,0 480x800: drawn in 100 ms (writing 5 of them), shown in 900 ms, sent again in 5 ms"))
        elif name == "warn":
            self.log("WARN - call 3 ended with 7")
        elif name == "bad":
            raise pp.StepError("bad: ERR no")
        elif name == "run":
            self.log("INFO - core.library: 120 ms")

    def drain(self):
        pass

    def pass_time(self, seconds):
        time.sleep(min(seconds, 0.01))

    def shows(self, text):
        return text in self.screen

    def listing(self, path):
        return self.files[path]

    def heap_values(self):
        self.block -= 100
        self.last_heap = {"used": 1000, "free": 60000, "stack": 3000, "block": self.block, "peak": 2000}
        return self.last_heap

    def usage_totals(self):
        return {"read_kb": 1.0, "written_kb": 0.0}

    def ask(self, command, until, timeout=0):
        if command.startswith("stat "):
            return ["STAT f 3"]
        raise pp.StepError(command)

    def read(self, path):
        return b""

    def hard_reset(self, seconds):
        self.reset_log = [(0.1, "INFO - SPI Speed : 40MHz"), (1.0, "INFO - kernel ready in 800 ms"), (2.5, "INFO - frame Full 0,0 480x800: drawn in 1 ms, shown in 2 ms")]


def engine(x4=None, **kw):
    x4 = x4 or FakeX4()
    e = pp.Engine(x4, x4.primitive, **kw)
    e.say = lambda text: None
    return e, x4


def run(source, inline=False, **kw):
    e, x4 = engine(**kw)
    e.run(pp.parse(source, "t", inline))
    return e, x4


class Parse(unittest.TestCase):
    def test_lines_comments_and_blocks(self):
        got = pp.parse("# c\nback\n\nrepeat:3 {\n  page-forward\n  wait:100\n}\nshot:a.png\n")
        self.assertEqual([s.text for s in got], ["back", "repeat:3", "shot:a.png"])
        self.assertEqual([s.text for s in got[1].children], ["page-forward", "wait:100"])
        self.assertEqual(got[1].line, 4)

    def test_inline_commas_and_escapes(self):
        got = pp.parse("back,repeat:2{page-forward,wait:10},set:x=a\\,b", inline=True)
        self.assertEqual([s.text for s in got], ["back", "repeat:2", "set:x=a,b"])
        self.assertEqual(len(got[1].children), 2)

    def test_commas_are_text_in_files(self):
        self.assertEqual(pp.parse("type:a, b")[0].text, "type:a, b")

    def test_errors(self):
        for source in ("repeat:2 {\nback", "back }", "nothing:1 {\n}"):
            with self.assertRaises(pp.ScriptError):
                pp.parse(source)

    def test_variable_braces_are_not_blocks(self):
        self.assertEqual(pp.parse("echo:${a}")[0].text, "echo:${a}")


class Language(unittest.TestCase):
    def test_variables_calc_repeat_for_define(self):
        e, x4 = run("let:n=2\ncalc:m=$n*3+1\nrepeat:$n:k {\n  echo:$k\n}\n"
                    "define:turn {\n  page-forward:$1\n}\ncall:turn:x\ncall:turn:y\n"
                    "for:f in ls:books/*.epub {\n  open:${f}\n  open:${f.stem}\n}\nfor:v in list:a|b {\n  v:$v\n}\n"
                    "for:i in range:1..2 {\n  r:$i\n}\n")
        self.assertEqual(e.vars["m"], 7)
        self.assertEqual(x4.seen, ["page-forward:x", "page-forward:y", "open:books/a.epub", "open:a", "open:books/b.epub", "open:b", "v:a", "v:b", "r:1", "r:2"])
        self.assertEqual(e.exit_code(), 0)

    def test_unknown_variable_stops(self):
        e, _ = run("echo:$nope")
        self.assertEqual(e.exit_code(), 2)
        self.assertIn("no variable", e.stopped["why"])

    def test_if_else(self):
        e, x4 = run("if:shows:Home {\n a\n}\nelse {\n b\n}\nif:shows:Nope {\n c\n}\nelse {\n d\n}\nlet:x=3\nif:$x>=3 {\n e\n}\n")
        self.assertEqual(x4.seen, ["a", "d", "e"])

    def test_measure_and_asserts(self):
        e, x4 = run("measure:turn:n=4:warm=1:settle=0:heap=on {\n page-forward\n}\n"
                    "assert:turn.first_ms.median<=500\nassert:turn.first_ms.max<5000\nassert:turn.frames==1\n"
                    "assert:turn.first_ms<10:too slow\n")
        self.assertEqual(e.metrics["turn.first_ms"].__len__(), 4)
        self.assertAlmostEqual(e.metrics["turn.first_ms"][0], 50, delta=30)
        self.assertEqual([c["verdict"] for c in e.checks], ["PASS", "PASS", "PASS", "FAIL"])
        self.assertEqual(e.exit_code(), 1)
        self.assertEqual(e.metrics["turn.drawn_ms"][0], 100)
        self.assertEqual(e.metrics["turn.heap_block"][-1], 50000 - 1000)

    def test_log_checks(self):
        e, x4 = run("mark\nwarn\nexpect-log:call \\d+ ended\nno-log:frame\nno-warn\nknown-warn:ended with\nno-warn\n")
        self.assertEqual([c["verdict"] for c in e.checks], ["PASS", "PASS", "FAIL", "PASS"])

    def test_until_log_waits(self):
        e, _ = run("until-log:never:50\n")
        self.assertEqual(e.checks[0]["verdict"], "FAIL")

    def test_tests_verdicts_skip_and_step_errors(self):
        e, x4 = run("test:one {\n expect:Home\n}\ntest:two {\n expect:Nope\n}\ntest:three {\n skip:no wifi\n a\n}\ntest:four {\n bad\n a\n}\ntest:five {\n b\n}\n")
        self.assertEqual([(t["name"], t["verdict"]) for t in e.tests],
                         [("one", "PASS"), ("two", "FAIL"), ("three", "NOT TESTABLE"), ("four", "FAIL"), ("five", "PASS")])
        self.assertEqual(x4.seen, ["bad", "b"])
        self.assertIsNone(e.stopped)

    def test_step_error_outside_a_test_stops(self):
        with tempfile.TemporaryDirectory() as d:
            e, x4 = run("a\nbad\nb\n", out=d)
            self.assertEqual(x4.seen, ["a", "bad"])
            self.assertEqual(e.exit_code(), 2)
            self.assertIn("bad", (Path(d) / "STOPPED").read_text())

    def test_try_and_retry(self):
        e, x4 = run("try {\n bad\n}\na\nretry:3 {\n expect:Nope\n}\n")
        self.assertEqual(x4.seen, ["bad", "a"])
        self.assertEqual(len(e.checks), 1)
        self.assertEqual(e.exit_code(), 1)

    def test_retry_passes_once_screen_changes(self):
        e, x4 = engine()
        count = [0]
        shows = x4.shows

        def flaky(text):
            count[0] += 1
            return count[0] >= 3
        x4.shows = flaky
        e.run(pp.parse("retry:5 {\n expect:X\n}\n"))
        self.assertEqual(e.exit_code(), 0)
        self.assertEqual(count[0], 3)

    def test_stop_on_fail(self):
        e, x4 = run("expect:Nope\na\n", stop_on_fail=True)
        self.assertEqual(x4.seen, [])
        self.assertEqual(e.exit_code(), 2)

    def test_link_recovery(self):
        calls = []
        e, x4 = engine(reconnect=lambda: calls.append(1))
        x4.link_down = 1
        e.run(pp.parse("heap\n"))
        # `heap` is a question: asked again once the link is back
        self.assertEqual((calls, x4.seen, e.exit_code()), ([1], ["heap", "heap"], 0))
        calls.clear()
        e, x4 = engine(reconnect=lambda: calls.append(1))
        x4.link_down = 1
        e.run(pp.parse("page-forward\nb\n"))
        self.assertEqual(calls, [1])
        self.assertEqual(x4.seen, ["page-forward", "b"])
        self.assertEqual(e.checks[0]["verdict"], "FAIL")

    def test_link_that_stays_down_stops(self):
        def down():
            raise pp.LinkError("still down")
        e, x4 = engine(reconnect=down, retries=2)
        x4.link_down = 5
        e.run(pp.parse("heap\nb\n"))
        self.assertEqual(e.exit_code(), 2)

    def test_secrets_are_masked(self):
        import io
        import os
        os.environ["TEST_WIFI_PASSWORD"] = "s3cret-pw"
        buffer = io.StringIO()
        e, x4 = engine()
        e.say = lambda text: print(text)
        saved, sys.stdout = sys.stdout, buffer
        try:
            e.run(pp.parse("wifi-connect:Net:${env:TEST_WIFI_PASSWORD}\necho:pw is ${env:TEST_WIFI_PASSWORD}\n"))
        finally:
            sys.stdout = saved
        self.assertNotIn("s3cret-pw", buffer.getvalue())
        self.assertIn("••••", buffer.getvalue())
        self.assertEqual(x4.seen[0], "wifi-connect:Net:s3cret-pw")

    def test_reset_boot_metrics_and_crashes(self):
        e, x4 = run("reset:30\nassert:boot.kernel_ms<=1000\nassert:boot.first_frame_s<=3\nnote:image=18A\n")
        self.assertEqual(e.exit_code(), 0)
        self.assertEqual(e.notes["boot.spi"], "40MHz")

    def test_assert_without_data_fails_on_x4_and_skips_on_emulator(self):
        e, _ = run("assert:nothing.here<=1\n")
        self.assertEqual(e.checks[0]["verdict"], "FAIL")
        e, _ = run("assert:nothing.here<=1\n", target="emu")
        self.assertEqual(e.checks[0]["verdict"], "SKIP")

    def test_include(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "lib.pps").write_text("define:go {\n  go-$1\n}\n")
            (Path(d) / "main.pps").write_text("include:lib.pps\ncall:go:now\n")
            e, x4 = engine()
            e.run(pp.parse((Path(d) / "main.pps").read_text(), str(Path(d) / "main.pps")))
            self.assertEqual(x4.seen, ["go-now"])


class Reports(unittest.TestCase):
    def test_json_markdown_and_baseline(self):
        with tempfile.TemporaryDirectory() as d:
            first, _ = run("test:turns {\n measure:turn:n=3:settle=0 {\n  page-forward\n }\n}\nheap\n", out=Path(d) / "a")
            data = json.loads((Path(d) / "a" / "results.json").read_text())
            self.assertEqual(data["tests"][0]["verdict"], "PASS")
            self.assertEqual(data["metrics"]["turn.first_ms"]["n"], 3)
            self.assertTrue((Path(d) / "a" / "device.log").read_text().strip())
            second, x4 = engine(out=Path(d) / "b", baseline=data)
            x4.turn_delay = 0.15
            second.run(pp.parse("measure:turn:n=3:settle=0 {\n page-forward\n}\nassert:turn.first_ms<=baseline*1.5:no slower\nassert:turn.first_ms<=baseline+10\nassert:other.x<=baseline\n"))
            verdicts = [c["verdict"] for c in second.checks]
            self.assertEqual(verdicts, ["FAIL", "FAIL", "SKIP"])
            report = (Path(d) / "b" / "report.md").read_text()
            self.assertIn("before (median)", report)
            self.assertIn("turn.first_ms", report)
            self.assertIn("%", report)


@unittest.skipUnless(importlib.util.find_spec("serial"), "pyserial missing")
class WireClass(unittest.TestCase):
    """x4-remote.py's X4 class against a port that plays the firmware."""

    class Port:
        def __init__(self, answer):
            self.answer, self.out, self.port = answer, bytearray(), "fake"
            self.in_waiting = 0

        def write(self, data):
            for line in data.decode().splitlines():
                self.out += self.answer(line).encode()
            self.in_waiting = len(self.out)

        def read(self, n=1):
            data, self.out = bytes(self.out[:n]), self.out[n:]
            self.in_waiting = len(self.out)
            if not data:
                time.sleep(0.05)
            return data

        def reset_input_buffer(self):
            pass

        def close(self):
            pass

    def test_ask_logs_and_heap(self):
        xr = load("x4_remote_under_test", "x4-remote.py")

        def answer(line):
            if line == "heap":
                return "\x1b[0;32mINFO - frame Partial 0,0 1x1: drawn in 1 ms, shown in 2 ms\x1b[0m\r\nPP:HEAP 1000 2000 300 4000 1500 1800\r\n"
            if line.startswith("hold"):
                return "PP:ERR no such button\r\n"
            return ""

        x4 = xr.X4(self.Port(answer))
        values = x4.heap_values()
        self.assertEqual(values, {"used": 1000, "free": 2000, "stack": 300, "block": 1500, "peak": 1800})
        self.assertEqual(x4.logbuf[0][1], "INFO - frame Partial 0,0 1x1: drawn in 1 ms, shown in 2 ms")
        with self.assertRaises(pp.StepError):
            x4.ask("hold nothing 60", "OK")
        with self.assertRaises(pp.LinkError):
            x4.ask("silence", "OK", 0.3)


@unittest.skipUnless(importlib.util.find_spec("serial"), "pyserial missing")
class Pipeline(unittest.TestCase):
    """The real X4 class and the engine, against a port that answers a press at once and
    shows its frame 0.2 s later."""

    class Port:
        def __init__(self):
            self.queue, self.port = [], "fake"

        @property
        def in_waiting(self):
            return sum(len(d) for t, d in self.queue if t <= time.monotonic())

        def write(self, data):
            now = time.monotonic()
            for line in data.decode().splitlines():
                if line.startswith("hold"):
                    self.queue.append((now, b"INFO - page-forward pressed\r\nPP:OK\r\n"))
                    self.queue.append((now + 0.2, b"INFO - frame Partial 0,0 480x800: drawn in 90 ms (writing 5 of them), shown in 700 ms, sent again in 5 ms\r\n"))
                elif line == "heap":
                    self.queue.append((now, b"PP:HEAP 1000 2000 300 4000 1500 1800\r\n"))

        def read(self, n=1):
            ready = [(t, d) for t, d in self.queue if t <= time.monotonic()]
            if not ready:
                time.sleep(0.02)
                return b""
            self.queue.remove(ready[0])
            return ready[0][1]

        def reset_input_buffer(self):
            self.queue = [(t, d) for t, d in self.queue if t > time.monotonic()]

        def close(self):
            pass

    def test_key_to_frame_is_measured_across_a_wait(self):
        xr = load("x4_remote_pipeline", "x4-remote.py")
        x4 = xr.X4(self.Port())
        e = pp.Engine(x4, lambda step: xr.run(x4, step))
        e.say = lambda text: None
        e.run(pp.parse("measure:turn:n=2:settle=600 {\n page-forward\n}\nassert:turn.first_ms.median<=400\nassert:turn.first_ms.median>=100\nassert:turn.shown_ms==700\n", "t"))
        self.assertEqual([c["verdict"] for c in e.checks], ["PASS", "PASS", "PASS"], e.checks)


if __name__ == "__main__":
    unittest.main()
