# Declarative registration for interface 7 (R-DECL, R-DATA): design proposal

Status: PROPOSAL for Kernel Engineer review, then Senior review. Nothing here is implemented,
no schema, codec or host code is changed by this file. Written 2026-11-06 against
`abi7/integration` at `23086bb9`.

Conventions: **Fact** marks something read from the repository, with `path:line`. **Proposal**
marks a decision this document recommends; the owner decides. **Estimate** is a number that was
computed or guessed, not measured on a device. **Unverified** names what I could not check.
Paths are relative to the repository root unless prefixed `ADR34:` (the file
`docs/adr/0034-stable-plugin-boundary.md` on `origin/fw/adr-flat-boundary-228`, line numbers of
that file) or `notes:` (`origin/notes/port-classification-748`, `paperpaw/design/flat-boundary-228/
port-classification-748/README.md`).

## 0. Summary of the proposal

1. Registration is split over **three transports that share one record grammar**:
   (a) **folder files** (plugin.ini, `i18n/*`, resource files), which the kernel's data-plugin loader
   already reads for every plugin folder; (b) the **inline batch** of slot 16 for small declarations
   and for every record that carries a callback token; (c) **record frames in a file** of the plugin
   folder for bulk declarations (settings), reserved for a lazy-retention step and not needed for M1. The wire grammar of `docs/abi/records.md` is unchanged.
2. **Manifest/identity needs no record.** Identity is the folder's `plugin.ini` (host-known, never a
   claim in bytes); target, ABI minor, services and `uses` permissions are signed metadata.
3. **Translations need no record.** They are `i18n/<lang>.ini|.ppcat` in the plugin folder
   (ADR 0033); slot 6 resolves through the kernel's one translator chain.
4. **Settings** get three records (`setting.group`, `setting.spec`, `setting.extra`), carried inline
   when there are few and in a frame file when there are many.
5. **Data resources** (hyphenation, fonts) are **files in the plugin folder named by a record**,
   read by kernel services through the host's own storage. They are not read through slot 18 and not
   embedded in plugin rodata.
6. The host's `REGISTRATIONS = 8` stays the cap of **retained callback registrations**. Declarative
   records are applied to the kernel registries at commit and are **not** retained in that array;
   they get their own bytes budget.

A finding that may make most of Part 4 unnecessary for the G0 plugins is in section 1.5 and open
question Q1.

## 1. Facts this proposal rests on

### 1.1 The wire today

- A batch is a LEB128 count, then records of `tag, body length, bitmap length, bitmap, fields`;
  at most 8 records and 4096 bytes; unknown registration tags give `ERR_UNSUPPORTED`
  (`docs/abi/records.md:7-17`, `:27-28`, `:39-41`). Field types are u32/u64/i32/str/bytes
  (`abi/README.md:21-22`). Only tags 1-3 exist (`docs/abi/records.md:17-21`); tag 4 is
  `command.set_setting` on the commands branch (`wt-abi7-commands/docs/abi/commands.md:19`, local
  worktree at `d96b26cf`, not pushed to `origin` when I looked).
- Initial records have no optional fields: "All declarations and fields start at minor 1. There are no
  optional fields yet." (`docs/abi/records.md:23`). Appending optional fields with defaults is
  allowed only in a later minor (`abi/README.md:21-25`). Hence every record below uses required
  fields with documented sentinels (empty string, zero), not optional ones.
- Statuses: OK 0, UNSUPPORTED -1, INVALID -2, NO_SPACE -3, NO_MEMORY -4, UTF8 -5, STATE -6,
  CANCELLED -7, IO -8, BUSY -9 (`abi/abi.def`, `statuses`).
- Slot 16 `registrations.submit(ctx, batch)` cap 4096; slot 18 `blob.read(ctx, blob, offset, out)`
  returns a copied count, cap 4096; slot 19 `blob.release` (`abi/abi.def` host_entries 16, 18, 19).
  A `blob` is a host-issued handle (`crates/paperpaw-core/src/abi7/handles.rs:12-16`); handles are
  32 slots device-wide, 4 per owner (`handles.rs:6-8`). No declared entry lets a plugin hand bytes to
  the host: slot 18 reads host data into the plugin.

### 1.2 The host staging today

- Registrations are accepted only while a construction scope is active (`make`); a decode, unsupported
  or staging refusal latches and poisons the construction even if the plugin ignores the status
  (`crates/paperpaw-core/src/native_host/registrations.rs:1-6`, `:28-31`, `:102-111`).
- The whole batch is decoded and checked for support before the first record is staged
  (`registrations.rs:39-67`).
- `Construction.records: [Option<T>; REGISTRATIONS]` with `REGISTRATIONS = 8`; the ninth `stage` latches
  `Capacity` (`crates/paperpaw-core/src/native_host.rs:14`, `:149`, `:175-178`). Commit moves that same
  array into the published `RegistrationBatch`, which is what `dispatch(index)` indexes
  (`native_host.rs:195-205`, `:233`, `:258-275`). So today the array is both the staging area and the
  **retained callback registry**; it is sized for token records.
- Interface 6 already merges a plugin's contributions into a staging area and refuses duplicates before
  anything is visible (`crates/paperpaw-core/src/plugins.rs:205-225`).

### 1.3 Settings

- A `SettingSpec` has id, title key, kind (Bool, Int{min,max}, Choice, Text, Language, List, Extended),
  default, group, level, description key, scoped, guarded, kept_on_reset and an optional "extra"
  (unit, editor, step, slider, section, secret, follows, keeps_unknown, shown_if, check fn)
  (`crates/paperpaw-sdk/src/settings.rs:172-199`, `:237-300`).
- Memory: specs from a `static` list "take next to no memory, where each added with `add_setting`
  takes about 200 bytes" (`crates/paperpaw-sdk/src/contribution.rs:243-246`); the registry keeps a
  reference to each fixed list and 2 + 2 bytes of index per spec
  (`crates/paperpaw-sdk/src/settings/specs.rs:5-26`). In the flat boundary a plugin's static list
  cannot cross, so every spec becomes an owned copy unless something lazier is built.
- Count (**Estimate**, lexical, tests included, from `grep -c 'SettingSpec::<ctor>('` in
  `plugins/*/src`): 246 constructor calls in 37 of the 70 plugins: widgets 53, mail 25, picture-home 23,
  stats 18, home 13, weather 12, reader 12, vocabulary 9, sleep 9, notes-vault 7, then 5 or fewer; median
  3. Kinds: 100 choice, 53 text, 43 switch, 41 number, 9 `new` (list/language/extended); 94 uses of
  `with_extra`/`with_owned_extra`; 65 `SettingGroupSpec::new`. 246 x ~200 B is about 49 KB of heap if all
  were enabled at once; a typical install enables a fraction. The 81 `number` calls contain no literal
  of 9 or more digits, so an `i32` range holds them (lexical check only).
- Slot 15 `SetSetting`: key namespace (first dot segment) must be the submitting plugin; the setting must
  be registered; bool/int must satisfy its spec; a secret setting returns `ERR_UNSUPPORTED`
  (`wt-abi7-commands/docs/abi/commands.md:49-54`). Slot 5 returns absent/bool/int/text and keeps
  defaults by going through `Settings::get` (`abi/settings.md:3-6`, `:21-24`;
  `crates/paperpaw-core/src/abi7/queries.rs:28-37`).

### 1.4 Translations

- `i18n/<lang>.ini` and compiled `i18n/<lang>.ppcat` are "read by the loader for every folder"
  (`docs/plugin-conventions.md:105`; `crates/paperpaw-core/src/data_plugin.rs:7-9`, `:662-712`);
  `.ppcat` needs `formats = 4` in plugin.ini (`data_plugin.rs:86`, `:677-682`). A code plugin's folder is
  "a folder like a data plugin's" with `i18n/*.ini` (`docs/plugins.md:2029-2037`). **Unverified:** whether
  a folder that holds `code.ppn` is also passed through `data_plugin::load_folder` in the current
  firmware path; the docs say so, I did not trace `plugins.rs`/`builder.rs` for that case.
- ADR 0033: a `.ppcat` is opened with a block table of 8 bytes a block (about 1 KB for 3,400 messages),
  a lookup reads one block of at most 2 KiB, and reads stay under 16 KiB (`MAX_READ`); a corrupt file
  falls back as if missing (`docs/adr/0033-catalogs-read-as-needed.md`, Decision).
- Today all first-party English catalogs total 191,272 bytes, 3,590 messages in 70 plugins (German
  203,991 bytes); the largest are widgets 19,593, vocabulary 16,713, stats 11,744, store 11,447 bytes
  (`wc -c plugins/*/i18n/en.ini`).
- Slot 6 `i18n.copy` calls `frame.translator().tr(key)`: one kernel translator, no per-plugin key
  check, and a missing key yields the key itself (`crates/paperpaw-core/src/abi7/queries.rs:44-65`).

### 1.5 Data plugins and where bytes live

- A **data plugin** is a folder with `plugin.ini` and data files, no code (`docs/plugins.md:10`), and the
  store already ships such plugins (`tools/plugin-files/README.md`, `data.toml` row: "PaperPaw's own data
  plugins (no code)", e.g. the Literata packs). Font faces are declared in `plugin.ini`
  (`[extends]`, `[family]`, `[fonts]`; `data_plugin.rs:12-18`, `:39`). There is **no** hyphenation
  file kind in the data-plugin loader (`data_plugin.rs:1-45`); the 12 language plugins and the base are
  code plugins whose table is `include_bytes!` (`plugins/hyphenation-de/src/lib.rs:15-18`).
- The store ships the 12 hyphenation plugins as code files (`tools/plugin-files/plugins.txt:15-26`).
- **Placement of native code on the X4** (the deciding fact): the `.ppn` holds "the plugin's code and
  read-only data as one block", is "written once into flash" in "the free end of the app partition ...
  about 640 KB after an install by CrossPoint's SD installer", mapped through the MMU, so "its code runs
  from flash and only its data takes RAM"; the X4 Pro reads it into PSRAM; unloading is a restart
  (`docs/adr/0024-native-code-plugins.md:53-57`, `:63-66`, `:80-92`; `docs/plugins.md:1853-1866`).
  So rodata costs no heap on any platform, but it consumes the **scarce, write-once flash region**.
- Sizes. Hyphenation tables (`.pphp`, **measured** from build outputs in
  `scratch/k228snapshot/target/release/build/*/out`, an older snapshot, rebuild to confirm), bytes:
  de 351,145; nb 251,949; nl 108,898; ru 74,905; es 49,109; uk 47,618; en 41,460; sv 38,470; pl 37,441;
  fr 12,362; da 7,886; pt 2,666; it 2,341; total 1,026,250. Header (`crates/paperpaw-hyphen/src/lib.rs:11-26`):
  de has 36,709 patterns in 2,295 index blocks (9,180 index bytes), nb 1,716 blocks, en 309.
  Default font packs (`.ppfp`, same snapshot): literata 643,946; literata-bold 702,764;
  literata-italic 620,281; literata-bold-italic 691,580; paperpaw-sans 651,736; icons 307,035; total
  3,617,342. Source TTFs in `plugins/fonts-default/fonts`: Literata 955,132; Literata-Italic 902,728;
  SourceSans3 646,340; PaperPawIcons 44,344. The Korean full pack on a test card is 39,828,925 bytes.
  Per-plugin `how/` 325-650 bytes, `i18n/` 203-243 bytes per language for the G0 plugins.
- Read in place: a hyphenation table is looked up by binary search over a sparse index without loading
  it (`paperpaw-hyphen/src/lib.rs:1-9`), but `Patterns::new` takes `&[u8]` only (`lib.rs:84`); a font pack
  on storage is read a glyph at a time (`StoredPack`, `crates/paperpaw-sdk/src/fonts.rs:62-80`,
  the `FontFace` doc). **There is no storage-backed `Patterns` today.**
- Boundary rules (ADR34): one interface-7 batch before 1.0, closures never cross, tokens are `u32`
  (`ADR34:257-264`); batches at most 4 KiB, "larger records require a bounded chunk/stream protocol"
  (`ADR34:312-321`); proposed X4 profile of 4,096 B total additional boundary storage with "one
  384-byte inline batch", eight dynamic tokens and four persistent handles per plugin
  (`ADR34:924-945`). The 384 B inline figure is a proposal in the ADR and conflicts with the 4096 cap in
  `abi.def`; see Q6.
- G0 classification: 14 data plugins (12 hyphenation languages, hyphenation base, fonts-default), "blocked
  only by R-DECL and R-DATA"; none of the 14 registers settings (`notes:` Result table; `grep add_settings`
  in the 14 sources is empty).

## 2. Architecture

### 2.1 Three transports, one grammar (Proposal)

| Transport | Carries | Bounds |
|---|---|---|
| Folder files | identity (`plugin.ini`), translations (`i18n/`), resource bytes (`hyphenation/`, `fonts/`) | existing data-plugin budgets (`data_plugin.rs:66-82`); resource files outside the 96 KiB parsed-data budget |
| Inline batch, slot 16 | token records; the few declarative records; pointers (paths) to resource files | `abi.def`: 8 records, 4096 B per batch, any number of batches during `make` |
| Frame file | the same records, for bulk declarations | a file `declarations.ppr` in the plugin folder, read by the host in frames (2.3) |

**M1 needs only the first two rows.** The frame file is specified so that, if the host later retains
settings lazily (S2 in Part 3), the wire and the schemas do not change; it can be left out of the first
implementation (estimate below counts it separately).

Rationale: records are a good fit for typed, validated, cross-referenced declarations, but a batch is
8 records / 4096 B (and a profile proposal of 384 B), so bulk data does not fit; folder files are already
how data plugins ship translations and fonts and cost the plugin no rodata; and keeping one grammar means
one generated codec to fuzz.

### 2.2 Host staging: split callbacks from declarations (Proposal)

Today `Construction` stages every record into the 8-slot array that later becomes the dispatch registry
(Fact 1.2). Proposal: keep that array for **token records only** (event, draw, and later action/screen
tokens); stage declarative records in a second per-construction list `Declarations`, bounded in bytes and
count, which is **consumed at commit** by the kernel registries (settings, fonts, hyphenation) and
dropped. Consequences:

- `REGISTRATIONS = 8` is untouched and keeps meaning "callbacks retained per plugin"; ADR34's limit of
  eight dynamic tokens per plugin (`ADR34:941`) is the same number. This is a coincidence worth keeping.
- Declarative records cost transient heap during `make` (their decoded form, until commit) and then
  whatever the receiving registry retains; they never sit in the dispatch registry.
- Atomicity is as today: whole batch validated before staging; any refusal latches and `commit` fails with
  `Invalid`; kernel registries are touched only after `make` returned OK and the output validated
  (`native_host.rs:186-205`). Cross-record rules (below) are checked against `Declarations` plus the
  already merged registries at commit, reusing the interface-6 duplicate checks
  (`plugins.rs:205-225`), so a failure leaves no partial plugin visible.

### 2.3 Frame file (Proposal)

`plugins/<id>/declarations.ppr`: a magic `PPRG`, a u32 layout (1), then frames, each `u16 length` + one
**ordinary registration batch** (count, records) of at most 2048 bytes and 8 records, until EOF. The host
reads one frame at a time into a single 2 KiB buffer, decodes with the same `registrations_decode`, checks
the frame, stages its records, and repeats. Properties: no new codec; transient heap is one frame; a
malformed or oversize frame refuses the whole file (`ERR_INVALID`/`ERR_NO_SPACE`) before commit; the file
is announced by one inline record or by plugin.ini (`declarations = declarations.ppr`, so a plugin without
it pays nothing). The 2048-byte frame is my choice: one `setting.spec` with 24 choices can reach about 1.4 KB (Part 4 of
settings, below), so 1024 would be too small and 4096 costs 2 KiB more transient heap (Q6). The file is produced by the author's
build (a `paperpaw-dev`/`paperpaw-sign` step, like `paperpaw-sign catalog` for `.ppcat`), from the same
`SETTINGS` data the SDK macros read.

Trust: the file is plain data beside the signed `.ppn`; the code is unsandboxed native code, so a forged
file grants nothing the code could not already do (`ADR34:` "No sandbox"; `ADR 0024` Decision, "No
sandbox"). It is nevertheless fully validated, because a corrupt card must not poison the kernel's
registries.

## 3. Part 1: manifest and identity

### Decision (Proposal)

**No manifest record and no new metadata field for identity.** Identity, name, description and version are
the folder's `plugin.ini`. What a plugin may do (`uses`) is declared as signed metadata features.

- *Identity.* The plugin id is the folder name and `plugin.ini` `id` (`docs/plugin-conventions.md:103`, `:119`;
  `data_plugin.rs:617-650` reads it). It is **never a claim in registration bytes**; the host knows the
  submitting plugin, as slot 15 already assumes ("never a claim in the bytes",
  `wt-abi7-commands/docs/abi/commands.md:41-43`).
- *Name, description.* English in `plugin.ini` (`[plugin] name, description`); translated by the keys
  `<id>.plugin.name` / `<id>.plugin.description`, which sit in `i18n/*` (`plugins/hyphenation-de/i18n/en.ini`).
  *How it works* is `how/<lang>.txt`, which the kernel does not read (`plugin-conventions.md:106`).
- *Version.* `plugin.ini` `version`. Interface 6 requires the code's manifest to repeat it
  (`docs/plugins.md:2039-2041`); with a flat entry that duplicate disappears, so the check goes (Q2).
- *Target, ABI, services, features.* Already in `PAPERPAW_METADATA`, which "already contains target
  identity, services, features and entry/export requirements" (`abi/entry.md:19-20`,
  `abi/metadata.md:12-17`). Nothing to duplicate.
- *`uses`* (network, bluetooth, developer; `crates/paperpaw-sdk/src/plugin.rs:76-109`). Proposal: three
  semantic feature keys in the metadata `features` list, `uses.network`, `uses.bluetooth`,
  `uses.developer`, revision 1, minor 1. Reasons: the loader can show and enforce them **before** `make`
  runs and without running code, they are inside the signed envelope (`metadata.md:17`), and the
  metadata already limits features to 32 entries of 128-byte keys. The alternative, plugin.ini
  `uses = network`, is unsigned and editable on the card; it would be acceptable only if `uses` stays a
  consent device rather than a guard (unsandboxed code, ADR 0024), which is the current interface-6
  position ("shown before it is installed, and the kernel refuses what is not declared",
  `plugin.rs:21-23`). Q3.

### Rationale

A manifest record would duplicate `plugin.ini` (which the store, installer and plugin manager read without
loading code: `docs/plugin-conventions.md:64-72`) and would spend one of eight record slots and part of the
transient batch on text that is already on the card. Metadata is the right home for permissions because
it is the only signed, pre-`make` channel.

### Schema, validation, failure, memory

- Records: **none**. Tag 5 is not consumed by this part.
- Metadata: add three feature descriptors to `abi.def.metadata.features` (currently `[]`); the generator
  and lock tooling already own that list (`abi/metadata.md:3-9`, `abi/abi.json` key `metadata.features`). Encoder rejects duplicates; loader refuses
  an unknown feature key before `make` (`metadata.md:19-25`).
- Validation: folder id equals `plugin.ini` id (existing, `plugins.rs:983-984`); `plugin.ini` at most 4 KiB
  (`data_plugin.rs:68`). Failure: plugin not loaded, a warning naming the folder; not an ABI status.
- Memory: unchanged from a data plugin today: `plugin.ini` is read transiently (at most 4 KiB), the
  retained manifest is a few strings (**Estimate** under 200 B).

### Open questions (Part 1)

- Q2: drop the code-vs-`plugin.ini` version cross-check, or add `plugin_version` to the metadata prefix
  (format 3, a lock-format change)? I propose dropping it and checking in `paperpaw-dev package`, which
  can read both files at build time.
- Q3: `uses` as signed metadata features (proposed) or unsigned `plugin.ini`?

## 4. Part 2: translations

### Decision (Proposal)

**No translation records. Catalogs are resources of the plugin folder**, loaded by the existing data loader,
compiled to `.ppcat` and read on demand (ADR 0033). A plugin ships `i18n/en.ppcat` (or `.ini`) and
`i18n/de.ppcat`, with `formats = 4` in `plugin.ini`; the `en` catalog is mandatory, `de` is required by the
repository's own check (`docs/plugin-conventions.md:205-207`, `:272`).

### Rationale

- Per-string records cannot work: 3,590 first-party English messages (Fact 1.4) against 8 records and 4096 B
  per batch; the largest plugin alone has 404 keys (`grep -c = plugins/widgets/i18n/en.ini`).
- Even a **per-catalog** record is unnecessary: the loader already discovers every `i18n/*.ini|ppcat` in the
  folder (`data_plugin.rs:662-712`), picks `.ppcat` over `.ini` of the same stem (`:692-696`) and refuses
  files over budget (`:703-707`).
- Heap: a text catalog is held whole (ADR 0033, Context); all first-party text for one language is about
  191 KB, more than the X4 could hold. A `.ppcat` costs about 8 B per block of at most 32 messages plus a
  bounded read cache (at most 16 KiB, emptied when the screen changes). For the G0 plugins a text catalog is
  203-243 bytes per language (14 plugins x 2 languages is about 6 KB on disk, parsed text roughly the same
  in heap while the language is active), so `.ini` is acceptable there and `.ppcat` is **required** above a
  threshold (Q4; I suggest 2 KiB, which matches no existing rule and is a guess).
- Key lookup stays as it is: `i18n.copy(key)` copies `translator().tr(key)` for the **UI language chain**
  (UI language, then English), across all registered catalogs, first match wins
  (`queries.rs:44-65`). Keys follow `<id>.<what>`; there is no per-plugin enforcement on read, and none is
  proposed (a plugin may read `core.*` keys). Writing catalogs is namespace-checked by the loader's
  existing rules, not by the ABI.

### Schema, validation, failure, memory

- Records: **none**.
- Validation (existing): file size at most 64 KiB for `.ini` (`MAX_FILE`), 512 KiB of text catalogs per
  plugin (`data_plugin.rs:66`, `:79`); `.ppcat` is checked whole at open (places in order, blocks at most
  2 KiB, hashes ascending), a bad one is dropped and the language falls back (ADR 0033).
- Failure: never a `make` failure. A missing or corrupt catalog is a warning and a fallback to English;
  `i18n.copy` returns the key itself for an absent message (`queries.rs:62`).
- Memory per plugin per active language: the open `.ppcat` (block table, about 1 KB for 3,400 messages), plus
  shared read cache. Inactive languages keep file name and meta only (ADR 0033).

### Open questions (Part 2)

- Q4: threshold above which `.ppcat` is mandatory.
- Q5: slot 6 cannot tell "missing" from "message equals its key". Interface 6 has the same property
  through `tr`. Should slot 6 return a distinct status for a missing key (an interface-7 change that I would
  prefer to batch now, since the batch is open) or stay as is?
- Verify (Unverified in 1.4) that `code.ppn` folders pass through `load_folder`.

## 5. Part 3: setting specs

### Decision (Proposal)

Three records: `setting.group` (tag 5), `setting.spec` (tag 6), `setting.extra` (tag 7). Carried **inline,
over as many `registrations.submit` calls as needed during `make`**; the frame file of 2.3 is an optional
later transport for the same records. Retention is host-internal: **S1 (M1): eager, owned copies** in the
existing `Specs` registry; **S2 (later): lazy**, only if the heap numbers below demand it.

### Rationale

- **Records, not a table resource, for M1.** The wire shape is identical either way, so the choice between
  S1 and S2 is a host decision that does not touch the ABI. Starting with S1 needs no new file format, no
  build step for authors and no change to `Specs`; its cost is the 200 B per spec of Fact 1.3.
- **Numbers.** 37 plugins register settings; 28 of them have 8 or fewer (median 3), 9 have more
  (widgets 53, mail 25, picture-home 23, stats 18, home 13, weather 12, reader 12, vocabulary 9,
  sleep 9). **Estimate** of one `setting.spec` on the wire: name 12 + group 12 + title 28 + help 15 +
  default 16 + choices 30 + five 1-byte lengths + 16 B of fixed scalars + about 6 B framing = about 140 B.
  8 records are about 1.1 KB (inside 4096 B, so the **record count** cap binds before the byte cap);
  widgets' 53 specs are about 7.4 KB, so about 8 submissions. Raising `REGISTRATIONS` to 53+ is **not**
  proposed: that array is the callback registry (2.2) and would grow `Construction` for every plugin.
- **Heap (S1).** All 246 in-tree specs enabled at once would be about 49 KB; 28 small plugins at 3 specs
  would be about 17 KB. The X4 starts with about 125 KiB of heap in use (ADR 0033, Context), so 49 KB is
  the pessimistic case, not the typical one. S2 would keep about 4 B per spec of index (as the fixed
  lists do today, `specs.rs:5-26`) plus a decode cache for the group on screen, at the price of reading the
  frame file on demand and of refactoring `Specs::get(&str) -> &SettingSpec` to return an owned or cached
  spec. I recommend building S1, measuring on the X4 with the largest realistic set, and deciding S2 on the
  measurement (Q7).
- **Why a closed `kind` and not `Extended`.** Extended is used nowhere in `plugins/*/src` (grep: 0), and
  interface 6 treats an unknown extended kind as read-only text (`settings.rs:38-45`). Interface 7 can add a
  kind number in a later minor and refuse unknown kinds now (`ERR_UNSUPPORTED`).
- **Relative names.** `name` is relative to the plugin id; the host forms `<plugin>.<name>`. This makes the
  namespace rule true by construction (no foreign id can be expressed), saves about 8-15 B a spec, and
  matches slot 15, whose namespace is the first dot segment (`commands.md:49`). A plugin that today uses a
  nested id (`weather.now.symbol`) writes `now.symbol`.

### `setting.group` (tag 5)

| # | Field | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `name` | str | 96 | group id relative to the plugin (`page` becomes `weather.page`) |
| 2 | `title` | str | 96 | translation key of the group's name; empty is refused |
| 3 | `order` | i32 | | smallest first; 0 names none |
| 4 | `page` | str | 96 | screen id of the plugin that replaces the generated page, relative; empty none |
| 5 | `parent` | str | 128 | full id of the parent group (own or foreign); empty none |

### `setting.spec` (tag 6)

| # | Field | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `name` | str | 96 | setting id relative to the plugin; full id at most 128 B (slot 15's key cap) |
| 2 | `kind` | u32 | | 0 bool, 1 int, 2 choice, 3 text, 4 language, 5 list; others `ERR_UNSUPPORTED` |
| 3 | `flags` | u32 | | bit0 scoped, bit1 guarded, bit2 kept_on_reset, bit3 secret, bit4 advanced (`Level::Advanced`); unknown bits `ERR_UNSUPPORTED` |
| 4 | `group` | str | 128 | full id of the group; empty is refused |
| 5 | `title` | str | 96 | translation key; empty derives `<plugin>.setting.<last segment of name>` |
| 6 | `help` | str | 96 | translation key; empty means no help (never derived, so a missing key is never silent) |
| 7 | `default` | bytes | 512 | exactly one `settings_value` (layout 1, `abi/settings.md`): bool, int or text, 12-byte header included |
| 8 | `min` | i32 | | int only; zero otherwise |
| 9 | `max` | i32 | | int only; zero otherwise |
| 10 | `choices` | str | 1024 | choice only: 1 to 32 values joined by `\n`; empty otherwise |

`secret` moves from `SettingExtra` (`settings.rs:258-263`) into `flags` because slot 15 must decide on
it without loading an extra. The 24-entry choice lists in the tree (largest static array found, lexical)
fit in 1024 B if keys average under 40 B; that is a guess (Q8).

### `setting.extra` (tag 7)

Refers to a spec by name, may appear anywhere after it in the same construction, at most once per spec.
Used by about 94 of 246 specs (Fact 1.3), so it is optional and kept apart, as in the SDK.

| # | Field | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `name` | str | 96 | the spec's relative name |
| 2 | `unit` | str | 96 | translation key of the unit format; empty none |
| 3 | `section` | str | 96 | translation key of the part of its group; empty none |
| 4 | `follows` | str | 128 | full id of the setting whose value it has until set; empty none |
| 5 | `shown_id` | str | 128 | `shown_if` setting id; empty none |
| 6 | `shown_value` | str | 128 | `shown_if` value as written in `paperpaw.ini`; empty when `shown_id` is empty |
| 7 | `editor_kind` | u32 | | 0 none, 1 screen, 2 slot |
| 8 | `editor` | str | 96 | screen id (relative to the plugin) or slot id; empty iff `editor_kind` is 0 |
| 9 | `step` | i32 | | 0 means 1; positive otherwise |
| 10 | `flags` | u32 | | bit0 slider, bit1 section_of_plugin, bit2 keeps_unknown; unknown bits `ERR_UNSUPPORTED` |
| 11 | `check` | u32 | | callback token of a value check; **must be 0 in M1** (`ERR_UNSUPPORTED`): interface 6's `SettingCheck` is `fn(&Settings, &SettingValue)` (`settings.rs:281-284`), which needs a callback and a settings view that do not exist yet |

### Validation before effects (all `ERR_INVALID` unless stated)

Per record, before staging, without allocating:

1. UTF-8 of every str (`ERR_UTF8`); every cap (`ERR_NO_SPACE`); unknown `kind`/flags (`ERR_UNSUPPORTED`).
2. `name`: dot-separated segments of `[a-z0-9-]`, no empty segment, full id at most 128 B
   (`docs/plugin-conventions.md:121`, "Anything the plugin registers").
3. `default` decodes as exactly one `settings_value`, is not absent, and **its kind matches**: bool for
   `kind 1`, int for 2, text otherwise; then the default passes the same `SettingKind::accepts` the live
   registry uses (int within `min..=max` with `min <= max`; choice default is one of `choices`; language
   tag charset). A default that slot 15 would refuse never registers.
4. `choices` for `kind 2` only: no empty or duplicate value, none longer than 96 B, at most 32.
5. Fields that do not apply to the kind must be zero/empty (so a later minor can give them meaning).
6. `secret` and `scoped` together are refused (a secret has no scopes, as in interface 6; **Unverified**,
   I did not trace the settings code for that rule).

Whole batch, before the first record is staged: every record above is valid, and tags are in the
supported set (an unknown tag is `ERR_UNSUPPORTED`, as `registrations.rs:55-67`). Cross-record, at commit
(checked against `Declarations` and the merged registries, `plugins.rs:205-225`): setting ids unique within
the plugin and against every registered setting; group ids likewise; an own-namespace `group` must be a
`setting.group` of this plugin (declared before or after in the same `make`) or already registered; a foreign
group (`core.*` and other plugins') is not required to exist, as `builtin::groups` are kernel constants;
`parent` must not form a cycle; each `setting.extra` names one spec of this plugin; `follows` must name a
setting of the same kind once registered (interface 6 follows one step only, `settings.rs:264-267`);
`editor`/`page` screen ids must be registered by part 5. Limits per plugin (**guesses**): 128 specs, 32
groups, 128 extras.

### Failure semantics

A refusal returns the status above and **poisons construction** (`registrations.rs:102-111`); the plugin is
not loaded and the user sees one warning naming the plugin and the first failing id. Allocation failure while
staging is `ERR_NO_MEMORY`, and also poisons. Over-limit count is `ERR_NO_SPACE`. Nothing reaches the kernel
settings registry before `commit`.

### Consistency with slots 5 and 15

- **Slot 5** returns the value or the spec default of a registered setting through `Settings::get`
  (`abi/settings.md:21-24`); `absent` stays the answer for an **unknown key only**, because a default is
  never absent (rule 3).
- **Slot 15** finds the spec in the same registry: namespace by first segment, kind and range via the same
  `accepts`, `secret` refused with `ERR_UNSUPPORTED` (`commands.md:49-54`). A guarded setting is writable by
  its own plugin by namespace; guarded protects against other plugins, which cannot name it.
- **Secrets.** Slot 5 on a secret key should return `absent`, since secrets live in the secret store
  (`settings.rs:258-263`); the current slot-5 code does not special-case this (`queries.rs:28-37`) and I did
  not check what `Settings::get` returns for one (Q9).
- A setting whose spec arrived in this record is the only kind slot 15 may set; therefore the record is also
  the **permission list** of slot 15.

### Memory cost

Transient: one decoded batch (at most 4096 B of input, borrowed, not copied) plus the staged `Declarations`
(S1: about 200 B per spec until commit, then owned by `Specs`). Retained (S1): about 200 B per spec, 49 KB
for all 246, about 0.6 KB for a plugin with 3. Retained (S2): about 4 B per spec plus a cache (**Estimate**,
not measured). Nothing is retained in the callback registry.

### Open questions (Part 3)

- Q7: S1 or S2, and the per-plugin caps (128/32/128 are guesses).
- Q8: is 1024 B enough for `choices`? Measure the longest real list.
- Q9: slot 5 on a secret key.
- Q10: `Extended` kinds and `check` callbacks: defer until a plugin needs them (none uses Extended today).
- Q11: should `title` default derivation exist at all, or should every title be explicit (about 25 B more per
  spec, but no hidden convention)?

## 6. Part 4: resource records for data plugins (R-DATA)

### Decision (Proposal)

Resources are **files in the plugin folder, named by a record, read by the kernel service that consumes
them through the host's own storage.** Three records: `resource.hyphenation` (tag 8), `resource.font_face`
(tag 9), `resource.font_extension` (tag 10). The plugin never reads these bytes, so **slot 18 is not
used** and no handle is spent.

### Rationale

1. *Slot 18 points the wrong way.* `blob.read` copies host data into the plugin through a host-issued
   handle (Fact 1.1). Here the consumer is a kernel service, not the plugin.
2. *Rodata does not fit and wastes the scarce resource.* Fact 1.5: on the X4 the `.ppn` lives in a write-once
   flash region of about 640 KB after an SD install, shared by all plugins. The 13 hyphenation tables total
   1,026,250 B (de 351,145 and nb 251,949 alone are 603,094 B); the default font packs total 3,617,342 B.
   Nothing but the smallest tables fits, and every install would write megabytes of flash. On the X4 Pro
   and the desktop rodata is not scarce, but one mechanism for all targets is simpler. A record also cannot
   carry a pointer (types are u32/u64/i32/str/bytes; `bytes` is inline and capped at 4096 per batch).
3. *Files match what exists.* `FontFace`, `FontExtension` and `add_translation_file` already name storage
   paths and load on demand (`contribution.rs:403-441`, `fonts.rs:62-80`), data plugins already ship
   `.ppfp` and `.ttf` (`data_plugin.rs:12-18`), and the store already distributes data plugins
   (`tools/plugin-files/README.md`, `data.toml`).
4. *Pull-reader callback (fallback, not proposed).* If a plugin must keep bytes in rodata, an appended
   `PpPlugin` callback `resource_read(state, token, offset, out)` would let the host pull chunks; it keeps
   zero RAM and today's speed but adds a callback, a lease per read and flash consumption. Only if Q13's
   measurement fails.

### Records

`resource.hyphenation` (tag 8)

| # | Field | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `name` | str | 96 | id relative to the plugin (`de` becomes `hyphenation-de.de`) |
| 2 | `language` | str | 35 | BCP 47 tag, `[A-Za-z0-9-]`, not empty |
| 3 | `path` | str | 100 | file in the plugin folder, e.g. `hyphenation/de.pphp` |

`resource.font_face` (tag 9), mirrors `FontFace` plus the optional role binding of `bind_font`

| # | Field | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `name` | str | 96 | relative id (`literata-bold` becomes `fonts-default.literata-bold`) |
| 2 | `path` | str | 100 | `.ppfp` pack or `.ttf`/`.otf`/`.ttc`; the extension selects the reader |
| 3 | `family` | str | 64 | empty means outside any family (the icons) |
| 4 | `style` | u32 | | bit0 bold, bit1 italic; unknown bits `ERR_UNSUPPORTED` |
| 5 | `weight` | u32 | | 0 style default (400/700); 1 to 1000 for a variable font (`wght`); must be 0 for `.ppfp` |
| 6 | `role` | str | 96 | full id of a font role (`builtin::fonts::*`) to bind; empty none |
| 7 | `priority` | i32 | | -100 to 100 (interface 6 `Priority(i16)`, `plugin.rs:135-144`); used only with `role` |

`resource.font_extension` (tag 10), mirrors `add_font_extension`

| # | Field | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `name` | str | 96 | relative id |
| 2 | `path` | str | 100 | a `.ppfp` with characters the extended font lacks |
| 3 | `extends` | str | 128 | full id of the font it extends |

The 100-byte path cap is the existing "whole path at most 100 bytes" rule (`plugin-conventions.md:126`).

### Reference rule for `path`

Relative to `plugins/<id>/`; characters `[A-Za-z0-9._-]` and `/`; no empty, `.`, `..` or dot-leading
segment; at most 3 segments; no leading or trailing `/`. The host joins it to the folder it loaded `make`
from (a host-known path, never one the plugin supplies), so a record cannot name another plugin's file.
Case: `.ppfp`/`.ttf` extension compared case-insensitively, as `has_extension` does for `.ini`
(`data_plugin.rs:665-676`).

### Validation (before effects, then at activation)

At submit (whole batch before staging): UTF-8, caps, tags, path syntax, `language` charset, style/weight
rules, `role` is a syntactically valid id, `priority` range. Failures `ERR_UTF8`, `ERR_NO_SPACE`,
`ERR_UNSUPPORTED` (unknown style bits), otherwise `ERR_INVALID`; **each poisons construction.**

At commit: names unique within the plugin and against registered fonts and hyphenation ids
(`plugins.rs:205-225` already refuses duplicate ids); `font_extension.extends` must name a font that is
registered by now or by this plugin; `role` bindings go through the same priority resolution as today.
Several plugins may register the same `language`; as today (`hyphenation/src/lib.rs:5-6`, "a later
registration for a language replaces an earlier one") the later one wins, which depends on load order (Q15).

At **activation**, by the consuming service, not at `make`: the file exists and its size is within the cap
(**guesses:** 2 MiB for a hyphenation table, 64 MiB for a font file; the largest real pack on a test card is
39,828,925 B); the **format check**: hyphenation magic `PPHP` version 1 (`paperpaw-hyphen/src/lib.rs:37`)
and header offsets inside the file; fonts by their own readers (`.ppfp` pack header, sfnt tables). A failure
here **does not poison anything**: the resource stays inactive, the language or family is treated as
missing, one warning is raised, and the next candidate in the chain is used. Reason: at `make` the card may
not be mounted yet or may be swapped later (ADR 0033 uses the same stance for catalogs), and a flaky read
must not unload a plugin.

The new storage-backed reader accepts **only** `PPHP\x01`. The old `QIHP` magic that `Patterns` still reads
(`lib.rs:39`) is a migration layer, which ADR 0034 forbids for interface 7; tables are rebuilt anyway.

### Memory

- Per record retained by the kernel (**Estimate**): three short strings and a struct, about 80-100 B for a
  hyphenation entry, about 100-120 B for a face (the interface-6 `FontFace` is the same shape). fonts-default
  needs 6 face records (4 Literata faces, Source Sans, icons), so about 700 B; the 13 hyphenation plugins
  about 1.2 KB if all installed.
- Nothing is read at registration. At activation a hyphenation reader needs the header and the sparse
  index: 4 bytes per 16 patterns, **measured from the tables**: de 9,180 B, nb 6,864, nl 3,184, ru 1,756,
  en 1,236, others under 1.2 KB (Fact 1.5). Only the book's language is active, so one index at a time
  (assumption: books are monolingual for hyphenation). Pattern blocks average about 153 B for de
  (351,145 B / 2,295 blocks) and are read on demand through a small cache (size to be set by measurement).
- Fonts: unchanged, `.ppfp` packs are read a glyph at a time and loaded only while their family is chosen
  (`fonts.rs:62-71`); no heap proportional to file size.
- **Gap (Fact 1.5):** `Patterns` takes `&[u8]`; **a storage-backed `Patterns` (positional reads, an index
  copy and a block cache) does not exist** and is a kernel work item outside "codecs + host staging". Loading
  the whole table is only possible for the small ones (en 41 KB, es 49, uk 48, sv 38, pl 37, fr 12, da 8, pt 3,
  it 2) and not for ru (75 KB, marginal), nl 109 KB, nb 252 KB, de 351 KB, whose size exceeds or approaches
  the free heap after a book is open.

### Open questions (Part 4)

- **Q1** Should G0 be ported at all as flat code? The 12 language plugins and the base contain no logic: their
  `register` adds two catalogs and one table (`plugins/hyphenation-de/src/lib.rs:36-42`). As **data plugins**
  (`plugin.ini`, `i18n/*`, `hyphenation/<lang>.pphp` built by a `data.toml` builder) they need no flat
  boundary, no `.ppn` signature, and no flash write; the data-plugin loader would gain one file kind (a
  `[hyphenation]` section, formats 5) that fills the same staged item as `resource.hyphenation`. That makes
  Part 4's records needed only by **code** plugins that also carry resources. I recommend the owner decides
  this before the plugin engineer starts G0.
- **Q12** `fonts-default` is built into the firmware image today (lean packs; `fonts-default/src/lib.rs:90-130`),
  so its bytes are in firmware flash, not in a card folder. A `path` record cannot name them. Either the
  built-in plugin keeps a Rust-side registrar (outside the flat boundary) or the firmware exposes its
  embedded faces under reserved names (a `builtin:` path scheme). Not decided here.
- **Q13** Measure a storage-backed `Patterns` on the X4 (page-turn hyphenation cost with a cold and a warm
  block cache, de and nb as worst cases) before committing to files for hyphenation; fall back to the
  pull-reader callback of rationale 4 if the cost is unacceptable. I have no such measurement.
- **Q14** Caps for hyphenation (2 MiB) and font files (64 MiB) are guesses.
- **Q15** Language collisions between plugins: last-registered wins by load order. Should there be an explicit
  priority (a field like fonts' `priority`)? Not added, because interface 6 has none.

## 7. Part 5: actions, menus, screens, icons (outline only)

Not specified in detail; this only shows that the tag plan and record shapes fit.

- **Tokens.** A record that carries a callback has a `token: u32`, 0 meaning none; the plugin owns the
  token-to-closure table and the host calls `dispatch_event(state, token, input)` (`abi/callbacks.md:3-12`).
  **Static token functions need no dynamic token slot** (`ADR34:941-943`), so 83 first-party `FixedAction`
  tables do not hit `REGISTRATIONS = 8`; only dynamic tokens do, and those stay in the retained array (2.2).
  Unresolved: the host needs a routing table `(plugin, token)` per action, bytes the 8-slot array was not sized
  for (Q16).
- `action.spec` (tag 12): `name` str(96), `title` str(96 key), `description` str(96 key, empty none),
  `keywords` str(96 key, empty none), `hint` u32 (interface 6 `ActionHint`), `flags` u32 (bit0 sealed),
  `token` u32. Declarative-with-token, so the record lands in `Declarations` and a routing entry.
- `menu.entry` (tag 13): `menu` str(128 full id), `action` str(128 full id), `order` i32, `flags` u32. Static
  cases are already expressible without code in `menus.ini` (`data_plugin.rs:25-27`); a record is needed only
  for entries a plugin computes. Candidate to leave unassigned.
- `screen.spec` (tag 14): `name` str(96), `title` str(96 key), `token` u32 (screen factory token, not 0),
  `refresh_on_open` u32 (0 default, then the `RefreshMode` order Mono, Fast, Partial, and the stronger
  ones, `crates/paperpaw-platform/src/display.rs:23-31`), `instances` u32 (interface 6 `Instances`).
  Screen callbacks (draw, keys, gestures) are separate callback-table work, not registration.
- `icon.binding` (tag 15): `item` str(128 full id of an action or setting), `icon` str(64 name). Equivalent to
  `icons.ini` (`data_plugin.rs:33`); candidate to leave to the file.
- Titles are translation keys everywhere (as `SettingSpec::title`); no raw display text crosses.
- Not outlined and held in reserve: widgets, status items/places, setup steps, keymap presets, input filters,
  slots, choices, subscriptions, formats, readers, lookups (EXT/DOC in `notes:`). Tags 16-23 reserved for
  them; 24-31 spare.
