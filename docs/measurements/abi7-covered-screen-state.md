# Freeing screen state while a screen is covered (#248, design note, no code)

Measured on the flip (`c7ecbd3eb` + #1330), X4, `--heap-report`, reading flow, `help.show` on/off. Peak 182.1 / 165.4 KiB, Beta 3 141.8 / 141.5.

## Why hints on cost 17 KiB more (measured per step, live heap)

Not the help plugin (about 0.4 KiB). With hints on the screen after `core.home` is still `reader.screen` and the library step ends on `help.overlay`: the keys close overlays first, so (inferred from the screens at each step, no stack dump) the stack is **reader, library list, settings list, settings group** at the settings step. With hints off the same keys close each screen: **home, settings**. Live heap after the settings group: 162.0 vs 137.3 KiB. The peak is at the settings group (182.1), not in the library (hints off, 165.4: a transient while the library list is read). Whose code holds it: reader plugin (text window 4.9, layout and word boxes 5.0 KiB), files plugin (list cells 5.6, book lines 4.3 KiB), settings plugin (list view 7.5 KiB of the 15). A real user with a deep stack (reader, library, menu, settings) sits in the same state.

## Mechanism

`Screen::on_hide` exists in the SDK and the kernel calls it when a full screen opens above (`screens.rs`, `hide_from`; not for overlays: they draw the page below). `NativeScreen` does not forward it, and there is no event for a native plugin ("screen covered"). Proposal for Kernel: **one new event record `event.screen_hidden` (now_ms, screen, instance; the size of `screen_closed`, about 290 B framed)** sent from `NativeScreen::on_hide` to the screen's own token, like `screen_shown`. No change to existing records, a lock regeneration only. Plugins that do not read it lose nothing. The reply to `screen_shown` already exists in both plugins: settings has `stale` plus `rebuild` on shown, files has `reread` on every shown.

## What is freed, expected KiB

| Screen (plugin) | Freed when covered | KiB | Comes back by |
|---|---|---|---|
| Reader page (reader, Port A) | text window, layout, word boxes; keeps the position | about 10 | `shown` re-lays out the page (one page: a read from the card and a layout; not measured on the X4, an e-ink refresh follows anyway) |
| Library list (files) | cells, book lines, cover state; keeps focus row and the window's start | about 10 | existing `reread` on shown (a catalog read, no change) |
| Settings list / group (settings) | built rows and groups; keeps the view's position | 7 to 8 each | existing `stale` and `rebuild` on shown, declarations are in RAM |

Estimate: hints-on peak 182 to about 160 KiB (reader and library freed). Hints-off peak unchanged (the library list is on top there). **Not enough alone for Beta 3's 141.8.**

## Risks

- Redraw cost above, and for a short moment the screen below is blank where an overlay draws it through (`shows_through`): overlays do not get `screen_hidden`, so the page under a menu stays.
- A rebuild can fail with no memory while the screen above holds a lot: the screen must then show its error state, not panic (it already handles `Error::NoMemory` on reread).
- Reader: the position must be kept exactly (page start, not a page number); a font or setting change while covered must be applied on return (the reader already re-lays out on `setting-changed`).
- Event order: `screen_hidden` must come before the screen above opens, so its peak is not beside the freed state.

## Separate and cheaper (Files' own code, no ABI)

The library list's read holds two 4 KiB service buffers and a 4 KiB message buffer at once (`SERVICE_BUFFER`; 8.0 + 4.0 KiB at the hints-off peak). Sizing the input to the request and the reply to the op's max_reply saves about 5 to 6 KiB of the 165.4 peak. I would do this one next unless Senior objects.
