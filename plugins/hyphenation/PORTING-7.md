# Hyphenation on interface 7 (ruling and plan)

**Ruling (#228):** the 13 hyphenation plugins (English and 12 store languages) are
code-free data packs (package format 5: `plugin.ini [hyphenation]` and
`hyphenation/<lang>.pphp`). No native `.ppn` wrapper. The interface-6 code
crates are removed only together with the proof below, never before.

## Proof required before the removal merges

Status: items 1–3 are claimed here and shown by #580's packer output (all 13 folders at 2.0.0, `check` exit 0) and by `tools/plugin-files/test_data.py`; the flasher's default install of English is checked on the web side, and item 4 is Port B's gate. Nothing is measured on a device.

1. **English out of the box:** `hyphenation` is in the default package set
   (`tools/plugin-files/data.toml`, web flasher, update path) on every device.
2. **Store:** all 12 language packs (`hyphenation-<lang>`) are offered as data
   packs.
3. **Update:** the packs keep their ids and go from 1.0.3 to 2.0.0, so a Beta 3
   install is offered and takes the data pack and keeps hyphenating.
4. **App:** `paperpaw-app` test `no_screen_reads_while_it_draws` passes with the
   crates unlinked (STORE_PLUGINS, offered(..), workspace).

## Order

1. This branch: data packaging (`tools/hyphenation-data.py`, `data.toml`),
   checks 1–3 shown with the packer output.
2. Port B's removal (`port-b/hyph-remove-228`) stacked on it: only after 1.
   Check 4 is its gate.
3. Reader hyphenation stays gated on slot 38.
