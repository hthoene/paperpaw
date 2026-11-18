# Device test scripts (`.pps`)

One script language drives the X4 (over USB, `tools/x4-remote.py`) and the emulator
(`paperpaw-emu --headless --repl`, through the same tool). A test is written and
debugged on the emulator, then run unchanged on the device. It has variables, loops,
named blocks, measurements, numeric checks, log checks, and a report with a
before → now comparison.

```
python3 tools/x4-remote.py --port 'rfc2217://host:4001?ign_set_control' \
    --script-file tools/pps/smoke.pps --out out/run1 --baseline out/run0/results.json
python3 tools/x4-remote.py --emulator target/debug/paperpaw-emu --library card/ \
    --script-file tools/pps/smoke.pps --out out/emu1
python3 tools/x4-remote.py --port COM10 --script "back,wait:1500,page-forward"   # the one-line form
```

`tools/x4-remote.py` prints every step's description in its usage text (the module
docstring); this page is the longer version. Python test of the engine:
`python3 -I tools/ppscript_test.py`.

## Files and steps

A step is a line, `name:arg:arg`. `#` starts a comment line. `include:other.pps` runs
another file (path relative to the including one). In `--script "a,b,c"` steps are told
apart by commas and newlines; in files only by newlines, so a comma is just a comma
(`type:hello, world`). `\,` `\{` `\}` are those characters in either form.

The steps that press, look and read the card are the remote's own (`page-forward`,
`long:back`, `hold:confirm:2000`, `run:core.library`, `slot:core.settings`, `open:books/a.epub`,
`type:text`, `tap:x:y`, `describe`, `screen`, `shot:a.png`, `set:`, `setting:`, `settings:`,
`ls:`, `stat:`, `md5:`, `rm:`, `mkdir:`, `mv:a>b`, `heap`, `usage`, `restart`, …; see the
usage text). On the emulator the card is the `--library` folder. A step the emulator cannot do
(`usage`, `wifi:*`, `plugins`, `time`, …) says `SKIP` there.

## Variables

`let:book=books/a.epub`, `calc:n=$n*2+1`, then `$book` or `${book}`. `${env:NAME}` reads the
environment; a name with PASS, SECRET, TOKEN, KEY or PIN in it is a secret and is blanked
(`••••`) in every output, log and report (`wifi-connect:Hannes:${env:WIFI_PASSWORD_HANNES}`).
`--var name=value` sets one from outside. Predefined: `$target` (`x4` or `emu`), `$out`,
`$date`, `$run`. `$$` is a dollar sign.

## Blocks

A block's header line ends in `{`; `}` closes it (inline, `repeat:3{page-forward,wait:300}`).

| block | meaning |
|---|---|
| `repeat:5 {…}`, `repeat:5:k {…}` | five times; `$i` (or `$k`) counts from 1 |
| `retry:3 {…}` | again while a check in it fails (the failed tries leave no trace) |
| `try {…}` | errors of steps inside are ignored |
| `for:f in ls:books/*.epub {…}` | over files on the card; also `list:a\|b\|c`, `range:1..5`, `local:glob`. `$f`, `${f.name}`, `${f.stem}`, `${f.dir}`, `${f.ext}` |
| `if:COND {…}` then `else {…}` | COND: `shows:Text`, `not-shows:Text`, `focused:Text`, `log:REGEX`, `has:METRIC`, `exists:PATH`, or a comparison `$n>=3` |
| `define:turn {…}`, `call:turn:a:b` | a named block; its arguments are `$1`, `$2`, … |
| `test:04-turns {…}` | a point with its own verdict (below) |
| `measure:turn:n=10 {…}` | the steps n times, measured (below) |

A `test:` ends **PASS**, **FAIL** (a check failed, or a step errored: the next test still runs)
or **NOT TESTABLE** (`skip:why` ran). Steps outside a test that error end the script.

## Measurements

`measure:NAME:n=10:warm=1:settle=4000:gap=0[:heap=on][:usage=on] { steps }` runs the steps
`warm + n` times (the warm-up runs are dropped). After the steps it watches the device log for
`settle` ms. Per run it records (as `NAME.<metric>`):

| metric | what |
|---|---|
| `first_ms`, `last_ms` | the key press log line (else the start of the steps) → the first / last frame line, host time |
| `frames`, `grays` | frame lines, and gray passes (`frame in grays`) |
| `unprompted` | frames more than 3 s after the last key (nobody asked for them) |
| `drawn_ms`, `shown_ms` | from the first frame line |
| `action_ms` | the `run:` action's own log line (`core.library: 120 ms`) |
| `card_reads`, `card_writes`, `card_read_ms`, `card_write_ms` | hoststat lines (builds that have them) |
| `hw_block`, `hw_block_low`, `hw_free`, `hw_free_low`, `hw_stack` | the last high-water line |
| `warns` | WARN/ERROR lines (without `known-warn:` patterns) |
| `wall_ms` | host time of the steps |
| `heap_used`, `heap_free`, `heap_block`, `heap_peak`, `heap_block_drop` | `heap` before/after (`heap=on`) |
| `use_read_kb`, `use_written_kb`, `use_refresh_*`, `use_busy_ms` | `usage` before/after (`usage=on`) |

Asking the device costs it time, so `heap` and `usage` are off unless wanted. The step `heap`
records `heap.used/free/block/peak/stack`; `record:name=5` records any number; `crashes` records
`crashes.count`; `reset:30` resets the chip and records `boot.kernel_ms`, `boot.first_frame_s`
(and the note `boot.spi`). Times are the host's clock at the moment the line was read.

## Checks

A failed check makes the exit code 1. A step that errors outright (the device says `ERR`, a file is
missing, a variable is unknown) ends a test; outside a test it ends the script: exit code 2, a
`STOPPED` file in `--out` with the reason.

* On the screen: `expect:Text`, `expect-not:Text`, `until:Text[:ms]`, `expect-focus:Text` (the focused row/button has that label).
  `focus:Text[:max]` presses `right` until the row/button with that label is focused (40 presses at most, else a failed check);
  `select:Text[:max]` does that, then `confirm`.
* On the log (lines since the last `mark`, or the start of the `test:`): `expect-log:REGEX[:ms]`,
  `no-log:REGEX`, `until-log:REGEX[:ms]`, `no-warn`, `known-warn:REGEX` (WARN lines to ignore from now on),
  `mark`. A trailing `:digits` is read as milliseconds.
* Numbers: `assert:METRIC[.STAT] OP NUMBER[:why]`. STAT is `median` (default), `min`, `max`, `mean`,
  `p90`, `n`, `last`, `sum`; OP is `<= >= < > == !=`; the number may be `baseline`, `baseline*1.1`,
  `baseline+50` — the same metric's value in the `--baseline` run (without one the check says SKIP).
  Without data for the metric the assert fails on the X4. On the emulator, asserts on times and gray
  passes (which only the X4 has) say SKIP.
* `fail:why`, `skip:why`, `abort:why`, `on-fail:stop` (stop at the first failed check; also `--stop-on-fail`).

## Output

`--out DIR` writes `results.json` (tests, every check, every metric with all samples and
median/min/max/mean/p90/n, notes, WARN lines, `stopped`), `report.md` (tests, measurements with
before → now against `--baseline`, failed and skipped checks, WARN/ERROR lines with new ones marked),
`device.log` (every log line with its time) and, on a stop, `STOPPED`. Screenshots go where
`shot:` says.

## Link and flashing

A lost link (RFC2217 hiccup, cable) is opened again (`--retries`, default 2): asking steps
(`heap`, `describe`, `ls`, …) are asked again; a key press is not repeated (the check says it was
lost). `chip:esp32c3` stops unless that is the chip on the link; `flash:IMAGE:chip=esp32c3[:sha256=…][:offset=0x0][:erase=0xd000+0x2000]`
reads the chip first, checks the hash, erases the region, writes and verifies. `chip=` is required: a
wrong chip is never written. `--esptool 'COMMAND'` names the esptool call, with `{chip}` and `{port}`
in it (default: `python -m esptool --chip {chip} --port {port} --after no-reset`).

## The emulator end

`paperpaw-emu --headless --repl` reads the remote's commands (`crates/paperpaw-x4/src/remote.rs`,
parsed by it) one per line from standard input and answers with `PP:` lines, as the X4 does, so the
tool's code is the same for both. Every frame the panel shows is told first as a log line
(`INFO - frame Partial 0,0 480x800: emulated`). Time there is emulated: `wait` costs no real time,
and measured milliseconds are the host's. It answers buttons, touch, actions, slots, screens,
events, typing, settings, `describe`, `screen`, `heap` (the emulator's budget), `shot`, `restart`;
card commands are the tool's own on the `--library` folder; the rest is `not on the emulator`.

## The Beta 4 suite

`tools/pps/beta4/` holds one file per area (`01-home` … `11-help-quick-panel`, shared `common.pps`) and `beta4.pps`, which
includes them all: 38 tests, every user flow of Beta 4. Asserts on times, gray passes and the heap count on the X4 only.
Tests that need the X4 (offline store, update download, clock after restart, Switch off after sleeping, a book with a cover)
say NOT TESTABLE on the emulator.

```
tools/pps/beta4/run-emulator.sh target/debug/paperpaw-emu <card template> out/beta4      # a fresh copy of the card each run
python3 tools/x4-remote.py --port 'rfc2217://esp-host:4001?ign_set_control' --script-file tools/pps/beta4/beta4.pps \
    --out out/x4-beta4 --baseline out/earlier/results.json
```

The suite sets Hints off, Sleep after 0 and Switch off after Never (X4), and does not restore them (test device).
