# Bounded X4 textprobe runner (#228 / #716)

The runner implements the three-command stage in owner-review package 735–738.
It is preparation tooling, not execution permission. Normal invocation, `--help`,
imports and tests do not construct serial ports. The worker is an internal child;
never invoke it directly. Only an explicit, separately authorized `--execute`
run may open the fixed TEST-X4 endpoint on port 4000. The Pro is never selected.

```sh
python3 -I tools/x4textprobe_runner.py
python3 -m unittest discover -s tools -p 'test_x4textprobe*.py'
```

The standard-library suite is self-contained. Additional tests exercise the
exact esptool 5.6.0 methods, Linux constructor and isolated configuration when
that optional package is installed; otherwise those methods/subcases report
explicit skips. In the device-testing environment run the same suite
with `/home/agent/esptool-venv/bin/python` to include the exact-SDK regressions.
All fixtures and measurements in tests are invented. Tests never open a serial
port, reset a device or call esptool's hardware entry point. One subprocess test
starts an ordinary Python child to prove that partial loss output kills it.

## Fixed procedure

The candidate is source `3e3b38ff73c83297d7767507696fd0dfa2be1ce8`, application
5,804,272 bytes, SHA-256
`4f3f1daba6a4a2addd1b389ea3742874c7a83979aa5d60fbf876aebfb3f830cb`.
Both this file and the merged reference (attachment 726) are checked before any
child is started. Only the application is written, at `0x10000`. The merged
reference supplies exact bootloader/partition bytes; it is never flashed.

The eventual approved invocation needs `--execute`, an `--approval-ref`,
`--physical-test-x4-confirmed`, verified `--app` and `--merged-reference` paths,
and a **new** `--evidence` directory. The reference records the external owner/
Manager authorization; supplying flags does not grant it. Physical mapping must
be confirmed before active auto-chip identification resets anything.

1. One auto-chip/no-stub ROM-entry reset; require ESP32-C3 and MAC
   `f8:5b:1b:5d:a2:ac` before selecting a C3 loader.
2. Read bootloader, partition table and OTA metadata; require exact reviewed
   bytes, decoded partition MD5/layout, and CRC-valid active `ota_0` VALID.
3. One pinned app-only write and successful transfer verification; one boot
   reset and complete startup evidence. No write to bootloader/table/OTA/NVS.
4. Record old awake settings, set sleep 0 / switch off Never and verify them;
   require the already-off radio, X4 version and expected boot/naps evidence.
5. Exactly `textprobe ascii control cold clean raster`, then `direct`, then
   `adapter`. Parse and inspect every invocation before starting the next one.
   Compare direct/adapter output and allocation metrics; control is not parity.
6. Stop and leave the candidate and awake settings installed. No restoration,
   recovery, retry, new test, radio change or extra boot on success or failure.

Native settings/Wi-Fi/boot queries use their existing wire acknowledgements;
textprobe requires both `PP:TEXTPROBE_END` and `PP:END`. The worker streams **all
raw bytes immediately**, including partial lines and bytes following a marker.
No fatal suffix is discarded by an early successful return.
Only the exact wire name `PP:END` completes transport validation;
`PP:TEXTPROBE_TRANSPORT_END` is invalid and permanently fails the invocation.

## Deadlines and first-error enforcement

The parent uses an absolute monotonic deadline before opening the log or starting
its child: ROM/firmware/boot 30 s each, flash and verification 180 s. If opening a
log exhausts the deadline no child starts. Each worker is an independent process
group. First reported loss, retry/reconnect, reset, protocol error, malformed
sample or exceeded deadline kills and closes that group. No later stage starts;
the runner is permanently stopped. Only the runner's child group is killed.

Raw output is bounded in host memory and written after the child is closed, so a
blocked disk write cannot stall a running hardware worker's deadline. A log
failure stops the procedure before another child starts. Process termination
retains the last received chunk even if its read crossed the deadline; those
late bytes are evidence only and cannot complete validation. Termination
cannot preempt a masked device callback or undo a partial flash. Host scheduling
and kernel termination are not real-time guarantees.

Before importing the SDK, each child pins a private esptool configuration with
only the reviewed retry settings, clears ambient ESPTOOL_* overrides and verifies
the exact selected file/content. CWD/home/custom reset configuration cannot
silently replace it; a missing/tampered pin is rejected before hardware opens.
The first-open guard sets DTR/RTS inactive while closed, including Linux SDK
constructors. A second open is refused even through a different port instance.
Control-line methods and property setters latch failures before SDK handlers
can swallow EINVAL/ENOTTY or retry another OSError. Control OSErrors become
terminal RuntimeErrors, blocking later reads, writes, controls and buffer flushes.
Application sessions suppress RFC2217's initial input purge to preserve pending
startup/error evidence; ROM sessions retain their required protocol flushing.

The child pins esptool 5.6.0. It asserts block/full-image/open/connect attempts 1,
including CLI environment overrides, and uses no fast-reflash, force or recovery
options. It also converts the **first loader protocol FatalError** to a terminal
error: esptool otherwise has a hardcoded five-sync loop and an autodetection
fallback that can reset again. A shared connect latch forbids a second connect;
the RFC2217 wrapper forbids reopening and I/O after a transport exception. This
is process-local instrumentation; installed tools are not edited.

Read/flash children use `--before no-reset --after no-reset-stub`, retaining the
RAM stub. In esptool 5.6.0 `--after no-reset` on a stub instead performs a soft
return to ROM; that extra transition is deliberately avoided. The initial
identity operation uses `--no-stub --after no-reset`. Stub loading/use and the
single final boot reset still require explicit authorization. A conservative
protocol failure stops even where a general esptool workflow would fall back.

## Measurement gates and exclusions

Final policy: **IRQ body >10,000 us stops; >1,000 us is advisory only**. This is
post-return, not an enforced safe masking budget. The parent validates actual
heap totals, absolute peak <=222,822 B, min free >=39,322 B, normal and painted
unused stack >=10,240 B, valid guards, no failed allocations/overflow, finite
nonnegative f32 advance, hashes, scope consistency and strict field/order/end
markers. Dedicated-stack charge stays in absolute totals. Compare paired peak
extra, not raw baseline, and preserve all raw evidence.

No warm, repeat10, bitmap, bidi/fallback maxima or fragmentation command is
admitted. Four stored-control × blind/describing combinations remain unsupported
by the candidate. Paint is touched-depth, not exact min-SP or a 512 B delta proof;
reserve alignment is not renderer request alignment. Semantic hashes, installed
fonts/SD, representative reader history, raw ABI and aggregate M1 remain open.
Physical mapping, live identity/layout/radio, final driver review and explicit
owner/Manager authorization remain prerequisites. Nothing in offline tests is a
device measurement or runtime acceptance result.
