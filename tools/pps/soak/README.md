# Soak runner (#230): hours of unattended use on the X4, in the `.pps` language

Builds the evidence of the 1.0 steady soak (`paperpaw-notes`: `fixtures/stable-soak/README.md`, validator
`scripts/check-stable-soak.py`, PR #8, landed). **It is a runner, not a result:** nothing here was run on a device
when it was written (emulator only, label `emulator`).

```
~/dt-jobs/soak/run.sh start  x4-72h --port 'rfc2217://esp-host:4001?ign_set_control' --hours 72 \
      --device-id X4-… --commit <40 hex> --image build.bin --compiler 1.99.0 --features x4-classic,remote \
      --books-always 'books/<cjk>.epub|books/<picture-book>.epub' --anchor books/alice-in-wonderland.epub
~/dt-jobs/soak/run.sh status   x4-72h
~/dt-jobs/soak/run.sh stop     x4-72h      # clean stop after the round in progress (same as `touch runs/x4-72h/STOP`, or SIGTERM)
~/dt-jobs/soak/run.sh resume   x4-72h      # continue (more time: --hours 96); elapsed_s keeps counting through the pause
~/dt-jobs/soak/run.sh finalize x4-72h --before-books … --after-books … --baseline-log …    # hashes into metadata.json, runs the validator
```

Start it with `start_job` (it runs for hours). One serial client at a time: between rounds the link is closed, so a
person or the tester can use `--log`/`--screenshot` in the gaps only. Port 4000 (X4 Pro) is refused unless
`SOAK_ALLOW_4000=1` (owner's word). The emulator: `run.sh start NAME --emulator BIN --library CARD-COPY --minutes 20`.
Runs live in `~/dt-jobs/soak/runs/NAME` (`SOAK_RUNS`). `~/dt-jobs/soak/run.sh` is a wrapper of `tools/pps/soak/run.sh`
in a checkout (`~/dt-jobs/soak/repo`: keep it on the branch with the tested `tools/x4-remote.py`).

## A round (`cycle.pps`, every `--interval` s, default 600, at most 900)

`reading` (`--books-per-round` 3 books rotating through the card's `books/*.epub`, plus `--books-always`: **name a CJK book and a
book with pictures there**; `--turns` page turns each, 3 back, Back → Home) · `library` (open, scroll, Home) · `keyboard` (Search
field, type, held Back) · `settings` (three settings changed and set back, the settings screen) · `card` (save, replace,
grow, move, delete of known-content files under `soak/`, MD5 read back each time; never a book) · `wifi` (every
`--wifi-every` rounds, at most `--wifi-max` in the run, `--wifi-nets 1,2` = Hannes / FRITZ!Box, then off) · `sleep` (every
`--sleep-every` rounds, 0 = never) · `checkpoint` (the same anchor book, Home, then `heap`, `crashes`, `clock`).
Rest of the interval: the device idles, untouched. `setup.pps` runs once: it sets the device quiet (Sleep after 0, Switch off
after Never, hints off), prints the old values (kept in `setup/`; **not restored**, test device), reads version/boot.

## What it writes (run folder)

| file | what |
|---|---|
| `samples.csv`, `samples.jsonl` | one `steady` row per round, **the validator's columns exactly**. `elapsed_s` is host wall time since the start (a pause counts), `cycle` the round, `boot_id` `boot-NNN`, heap/peak/largest/stack bytes from the remote's `heap` (peak is never reset), `next_alloc_b`/`allocation_margin_b` from `--next-alloc`/`--margin`, `reboots_total` = restarts seen, `expected_reboots_total` = 0 (the steady soak plans none), `result` `ok` or `fail: …`, `trace` = `raw.log#cycle-NNNNN` |
| `telemetry.jsonl` | per round: crash-file entries, WARN and ERROR counts, uptime, card writes, books, Wi-Fi/sleep done, failed checks, attempts |
| `raw.log` | every round's output and device log under `=== cycle-NNNNN …` headers (hashed by `finalize`) |
| `cycles/NNNNN/` | each round's `results.json`, `report.md`, `device.log`, `stdout.log` |
| `events.jsonl`, `state.json`, `config.json`, `metadata.json` | events (start, resume, link retries, stop), counters, settings, the validator's metadata (`hardware` false with `--emulator`) |
| `STOPPED` | why it ended: duration, stop file, or a failure (then exit code 1) |

A restart (uptime going back, or a kernel start in the log) opens a new `boot_id` and raises `reboots_total`: the validator FAILs
a steady soak with a reboot, and the runner **stops at the first failed round or threshold** (heap peak > 222,822 B, unused
stack < 10 KiB, largest block < next allocation + margin, reboot, crash-file growth, a failed check) with the evidence kept;
`--keep-going` records and continues. A lost link is retried (the tool's own reconnects, then the same round again after 30 s,
60 s … up to `--link-retries`); no heap sample after that ends the run as failed.

## Assumptions and limits (decide before the run)

* `--next-alloc` (default 32,768 B) must be set from the frozen workload's largest next allocation (the validator wants it declared).
* The remote has no true file append: `grow` reads the file and writes it whole again (restarts at 1 MB). Real appends happen in
  plugins' own logs (e.g. the battery log); the kernel's append/cut contract is the owner-attended block below.
* The keyboard, library, Back → Home and sleep UI come from the Beta 4 suite and the remote's button names: **sleep (`power`
  short press, 20 s, `power` again) and the wake are unconfirmed on the X4** (#156: the link can drop in sleep): leave
  `--sleep-every 0` for a first run, then try 1 round in 12. Wi-Fi uses `WIFI_PASSWORD_HANNES` / `WIFI_PASSWORD_FRITZ` from the environment.
* Raw `PP:HEAP` wire lines are not kept: the log has the tool's printed form (`heap: N bytes used …`), the numbers are exact.
* Peak/stack are per boot; a restart restarts them (the validator wants one boot for the whole 72 h).
* The emulator has no stack figure (`stack_unused_b` empty → the validator says INCOMPLETE), no uptime, no radio, no sleep.
* The runner does not build the 3,000-book fixture manifests (`before-books.jsonl`, `after-books.jsonl`), the baseline log or the attended
  flags: those are the owner-attended blocks' evidence; `finalize` takes their files.

## Needs the owner's hands (approved through the Manager only)

Real power cuts (battery out and USB out) for the replace/append/move cuts, card swaps for the FAT ↔ exFAT remount block, battery
current with an instrument, and the 10 cold/warm boots measured from real power/reset: none of it can be driven over the link.
USB-only unplug, software reset and sleep are not power cuts. The soak itself needs nobody; the USB link powers the device,
so it proves nothing about battery draw or sleep current.
