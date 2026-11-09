# Screen lock on interface 7 (slices 1 and 2a, partly ported)

Status: **not complete.** The interface-6 plugin (the lock screen with its PIN pad,
pattern and password, *Lock now*, the touch lock with its input filter, status item
and menu entries, the events that lock the device) is still in place beside the
native one. It is removed in the PR that finishes the port; until then nothing is
dropped.

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`)

- Declarations: the lock's page of settings (group `lock.group`, order 92), the
  kind of lock (off, PIN, pattern, password; guarded), whether waking up asks for
  it (guarded), and the touch lock's switch. `build.rs` and `native/metadata*` are generated with
  `tools/abi-metadata.py`. The en/de texts are the existing ones.
- The rules (`native_model.rs`, host-free): the interface-6 `code` module and the
  logic of the lock screen made fallible. The code is kept as `kind:salt:hash`,
  the hash a salted SHA-1 taken 2,000 times; checked in constant time; a code kept
  with a salt made from it is to be kept anew with a random one; a damaged code
  locks nothing. The wrong tries (kept on the card as a number, version 1 still
  read) and the wait of 30 s after five; the keypad of a PIN (at most 16 digits);
  the pattern's nine dots, each once; the shortest code (4). Secrets are wiped
  as soon as they are done with (`mem::wipe`: the digests on the way, the hash,
  the PIN's digits when cleared or dropped). Tests (`src/native_parity_tests.rs`)
  compare the hashes with the old code's for random salts and codes, read random
  bytes as tries as the old reader did, and test the rules by property.
  `tests/native_alloc_refusal.rs` refuses each allocation.

## Slice 2a: the screen's steps and layout (`src/native_flow.rs`, `src/native_pad.rs`)

- `native_flow` is the interface-6 `LockScreen` without the kernel: the steps (unlock,
  check the code in use, list the kinds, a new code, the same again), the messages
  (wrong, too short, mismatch), the wrong-tries rules (`Attempts` of slice 1) and what a key, a
  dot, a finger, a typed password, *Back* or a finished wait does. Each input returns a
  short list of **effects** (draw, close, ask for the password, notice, forget or keep the code,
  set the kind, wake at the end of the wait, keep the tries) that the plugin carries out in
  order; nothing here calls the host. A code is wiped as soon as it is done with; every
  allocation is fallible (`tests/native_alloc_refusal.rs` refuses each one).
  `tests/native_flow.rs` has the cases of the interface-6 `locking.rs` that concern the steps.
- `native_pad` is the layout of the interface-6 screen: the clock or header at the top,
  the title, the code's dots and a message, the round keys of the keypad and the nine dots of
  the pattern with their reach, the pattern's *OK* where there is no touch, the password's
  button. Sizes in, rectangles out; it saturates on any screen size.
- Not wired yet: the screen's drawing and input, the events, *Lock now* and the code's
  place in the secrets facade follow in slice 2b, with the touch lock after. The interface-6
  plugin stays until then.

## What the slice loses (each returns in a later slice or with its host piece)

- **Pending host, to come back before activation with parity:** the page's icon
  (`lock`, an icon record: the host refuses icon records until it takes them) and the
  first-run step that offers the lock (`SetupStepDecl`, page = the kind setting,
  order 70; `core.registry` op 22, Host todo 99). The builders exist and the batch
  is as the interface-6 plugin registered them, minus these two records, which are
  left out so that the host admits the rest. Each has an ignored case in
  `tests/acceptance_pending.rs`.
- **The lock screen** (PIN pad, pattern, password, the kind list, drawing) and
  everything that takes input while locked: it needs UI composition (H3), input
  interception (H4), screen commands (H1), the ask service (the password) and the
  setting editor screen (the kind is changed on the lock's own page).
- **Locking**: the started and resume events, *Lock now* (a sealed action; the
  declarations builder has no seal), the question of whether the kind setting
  changed away from the code kept (the setting-changed event).
- **Where the code is kept**: the secrets facade (`lock.code`, not shown, not
  changeable in the list).
- **The touch lock**: the input filter that takes taps and the way out (two taps,
  hold the second), the hint, the status item, the quick-panel and reader menu
  entries, the menu need for touch, the key map (`long.home`) and the actions.
  The way-out state machine (`Escape`) is pure and moves with the filter.
- The sleep screen interplay ("closed while locked it opens again at once").
- "How it works" (`how/`).

Each loss is an ignored case in `tests/acceptance_pending.rs`. The interface-6
tests (`tests/lock.rs`, `tests/locking.rs`, `tests/touch.rs`) stay as the references.
- **Removal checklist:** with interface 6 goes `src/code.rs` (it keeps the code
  through the interface-6 secrets); the native `Pin` zeroes its digits on drop.
