### Added

- **A test script language for the X4 and the emulator** (#310). `tools/x4-remote.py` runs script
  files (`--script-file test.pps`) with variables, loops, named blocks, test points with verdicts,
  built-in measurements (key to first frame, heap, card writes, gray passes), numeric checks
  (`assert:turn.first_ms.median<=1000`), log checks, a lost-link retry and a report
  (`results.json`, `report.md`, before → now against an earlier run). The emulator speaks the X4's
  remote commands too (`paperpaw-emu --headless --repl`, `x4-remote.py --emulator`), so one file runs
  on both. See `docs/device-test-scripts.md`.
