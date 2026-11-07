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
