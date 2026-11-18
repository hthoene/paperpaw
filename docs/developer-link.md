# The developer link over USB

A computer on the USB cable of an Xteink device (X4, X3, X4 Pro, X4 Classic) can press keys, take
screenshots and read and write books and plugin files, in every firmware build, once the reader has
allowed it on the device (#12, design `paperpaw-notes/paperpaw/designs/usb-dev-tools.md`, owner
decision: a firmware setting, no store plugin, no interface change). The client is `tools/x4-dev.py`.
The test remote control (`PP:` lines, `tools/x4-remote.py`) stays what it is: test builds only.

## Switching it on

*Settings > Customise > Developer* (with *Developer options* on): **USB developer tools**. Switching the
row on asks on the device (the firmware confirmation screen, focus on *Don't allow*; only the reader's
own press answers it). While it is on, a **DEV** mark shows over every screen. The row is hidden on
devices whose shell does not set the `USB_LINK` capability.

It ends, and the computer is told (`DEV/1 0 closed <why>`), when: the row is switched off (`switched off`);
the device restarts (a new kernel is off: nothing is stored, a file that says "on" is ignored); the device
sleeps or stops (`sleep`); the cable is pulled (`unplugged`); ten minutes pass without a command (`idle`);
ten refused commands come within a minute (`probing`); the computer says `close`.

## Wire

Lines on the serial link beside the log and the cable transfer's `PPT/1` lines: `DEV/1 <n> <command>
[args]`. Answers: `DEV/1 <n> ok [text]` or `DEV/1 <n> err <why>`, with lines before `ok` for commands that
return more (`frame`/`row`, `part`, `entry`, `size`/`data`). While the link is off only `err off` is said,
once a second at most.

| Command | Does | Rule |
|---|---|---|
| `hello`, `ping`, `close` | state; `close` ends the link | |
| `key <name>` | press and release `page-forward page-back up down left right confirm back menu home` (not `power`) | refused (`err question`) while a question or a lock screen is shown |
| `run <action>` | as a button bound to the action | same |
| `shot` | `frame <w> <h> <bits>`, `row <y> <runs>` (count and byte, hex), then `ok` | |
| `describe`, `screen`, `heap` | what the screen shows (secret screens only as dots), its id, the heap | |
| `ls <dir>`, `get <path>`, `md5 <path>` | list (secret names left out), read (≤ 16 MiB), hash | path rules |
| `put <size> <path>` + `data <hex>` lines | a new file (≤ 4 MiB, 512 B a line), written beside its target and moved in when whole | path rules; never over a book |
| `rm <path>` | only a file below `plugins/<id>/` that this link wrote | |

Not offered: setting values, `set`, Wi-Fi, installing, flashing, `power`, any other path.

## Rules and where they are kept

All in the kernel (`crates/paperpaw-core/src/developer_link.rs`, `developer_link_run.rs`); the shell
(`shells/xteink/src/firmware/cable.rs`) only carries lines in and answers out. Tests: `crates/paperpaw-core/src/tests/developer_link.rs`.

1. Off by default; on only after the question, answered by its screen with its random nonce (`developer_answer`).
2. Off after a restart: RAM state; the row is forced off at start (`builder.rs`).
3. `DEV` mark: `drawing.rs`.
4. Ends: see above (`developer_off`, `developer_poll`, `developer_close`, `developer_suspend`).
5. Secrets: `path()` refuses `.paperpaw`, `wifi.ini`, `secret*`, `*.key`, `*.sec`, `*.pem`, `..`, `.`, empty parts, `~` (short names), trailing dot or space, non-ASCII names outside the library.
6. Folders: the library folder, `plugins/<id>/…` (listing `plugins`), `screenshots/` (read only).
7. No code in: no `.ppn`, `.so`, `.bin`, `.elf`, `.ppu`, `plugin-code.txt`; nothing into a code plugin's folder; no setting is writable.
8. Limits: 20 commands a second, one put at a time, five quiet seconds give a put up.
9. One line a command in the device log, never a file's content (`cable.rs`).
