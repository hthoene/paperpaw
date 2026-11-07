# Interface-7 runtime port

Base: typed UIX/REG facade `d7488aa58`, combined branch `plugin/batch2-ui-1048`.

Completed: typed screen/action/menu/icon declarations with byte parity;
`native_menu` reads all registry menu pages and both current/declared orders,
checks one generation across the snapshot, and stages add/remove/reorder/reset
commands through the typed SDK. Traversal is iterative and allocations fallible.
Existing protection of Settings and the editor remains in place. Version 1.0.7.

Job #3252 passed menu-editor tests (including arbitrary-cycle and reorder
properties), fmt and all-target clippy. These are model/command checks, not
native activation or pixel evidence. Screen callbacks, renderer, add-candidate
metadata and host UIX acceptance are still open; no ready PR claim.

Native packaging must use paperpaw-plugin7.x, `-C jump-tables=no`, and pass the
2 KiB frame scan. Services has the 2,048-byte local buffer in set_menu to audit;
no measured pack refusal is claimed yet. Device gates remain open.

Renderer work in progress: native header/font/clip/canvas drawing and retained
hit geometry are implemented, but not yet accepted against pixel goldens.
Remaining renderer parity: wrapped value rows, drag grips/indicator, popup
region and theme behavior. Soft-focus currently refuses Unsupported because
its shade depends on display gray levels not exposed by the snapshot facade;
this is an explicit incomplete path, not a replacement visual.

REG gap resolved in contract #288 at 482556eeb: op-11 reg_item flags bit 5
marks a user-defined menu, including empty overrides and overrides equal to
defaults. The checked PR #292 getter is now retained in each Menu; Core host population
and the screen callback consumers remain open. Preserve existing
Changed/Reset/Reset all behavior using this flag; never compare row orders.
Native pointer contact forwarding is owned by Kernel/Services/Core; do not
substitute a fabricated id or capture a single contact.

Host acceptance dependencies (Core handoff, 2026-11-07): H1 screen commands
(Core), H2 services/dialogs (Services), H3 compose and H4 input (Input Engineer).
Existing native-screen tests are fixtures, not emulator activation proof.
Integration ceabdc905 and SDK PR #292 at 286a4832826a592663c57501a9cd8cdaaa6edbe7
are merged locally. SDK interface references/blessing for #292 remain pending.
