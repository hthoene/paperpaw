# Canary results (#244 safety net)

Base: integration e2c186c66 (train 49) + Kernel `registry_lines` b07fe406b. Stage 1 = `-p paperpaw-core --lib`, then `--workspace`; stage 2 = every other CI test set (not `workspace-*`, `bluetooth-*`). `--first-catch`: `.fails` lists the first catching set's failing tests only.

| canary | side | result | tests failing (first catching set) |
|---|---|---|---|
| close-request-order | ? | **NOT CAUGHT** | 0 |
| dims-refresh-region | ? | CAUGHT | 3 |
| failed-plugin-listed | ? | CAUGHT | 3 |
| firmware-answer-source | ? | CAUGHT | 14 |
| guard-event-screens | ? | **NOT CAUGHT** | 0 |
| guard-flag | ? | CAUGHT | 28 |
| guard-let-pass | ? | CAUGHT | 2 |
| guard-shut-out | ? | **NOT CAUGHT** | 0 |
| keymap-layer-order | ? | CAUGHT | 2 |
| keymap-origin | ? | CAUGHT | 6 |
| merge-error-dropped | ? | **NOT CAUGHT** | 0 |
| observer-order | ? | CAUGHT | 3 |
| observer-under-guard | ? | CAUGHT | 1 |
| overlay-draw-base | ? | **NOT CAUGHT** | 0 |
| override-tie | ? | **NOT CAUGHT** | 0 |
| override-under-guard | ? | CAUGHT | 1 |
| passes-through | ? | CAUGHT | 3 |
| password-event | ? | CAUGHT | 2 |
| sealed-override | ? | CAUGHT | 3 |
| secret-flag | ? | CAUGHT | 2 |
| secret-owner | ? | **NOT CAUGHT** | 0 |
| secret-setting-owner | ? | CAUGHT | 2 |
| slot-list-order | ? | CAUGHT | 2 |
| slot-priority | ? | CAUGHT | 322 |
| subscriber-order | ? | CAUGHT | 2 |
| takes-buttons | ? | **NOT CAUGHT** | 0 |
| tap-outside-ignored | ? | CAUGHT | 1 |
| topmost-guard | ? | **NOT CAUGHT** | 0 |

Caught 19 of 28; not caught 9.

## Not caught: what would catch each

Each needs one test (core `kernel_api` or `lib::tests`) that fails on the canary and passes on the real code:

- **override-tie** (`paperpaw-sdk/src/action.rs`): two handlers of one priority put in an action's place; assert the later one runs.
- **guard-event-screens** (`commands.rs`): a guard opens screens, another screen sits above them; assert the guard's event reaches all screens above it.
- **merge-error-dropped** (`plugins.rs`): a plugin whose contributions the merge refuses; assert the registration is an error and nothing of it is kept (all-or-none).
- **topmost-guard** (`screens.rs`): two guards stacked; assert the upper one decides, not the lower.
- **guard-shut-out** (`gestures.rs`): the guard opens a screen above itself; assert that screen gets input.
- **takes-buttons** (`gestures.rs`): a screen that is offered buttons by name; assert it gets them and the key map is not the only route.
- **close-request-order** (`screens.rs`): two screens that record when asked to close; assert the order (lowest first).
- **overlay-draw-base** (`drawing.rs`): a full screen below an overlay; assert drawing starts at the lowest full screen (draw-call order or pixels).
- **secret-owner** (`lib.rs`): two secret screens of two plugins; assert typed text belongs to the upper screen's plugin.

No canary ended in ERROR.
