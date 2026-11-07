# Interface 7 family FS: folders, file information, moves and free space (draft)

Contract draft for #228 (assignment #1088). Items marked [D] are proposed decisions, [Q] open
questions for Plugin, Core and Senior. Inputs: the storage slots 20-26 and their rules
(`proposals/interface-7-events-storage-services.md` section 2, "the storage contract" below), owner
decisions 124 and 125, `stages-services.md` (service schemas, D4), `registry.md` 2.1 (bare replies
and paging), `screens-input.md` section 13 (features and statuses), the platform `Storage` trait
(`crates/paperpaw-platform/src/storage.rs`) and an audit of the interface-6 storage calls of the
17 first-party plugins that use files beyond their own data folder (section 6). Doc-only; records
follow once approved. Tags are proposed and numbered without gaps at the records step.

## 0. Scope and principles

- [D] **Slots 20-26 stay the file calls.** `storage.size`, `read_at`, `write`, `append`,
  `rename`, `remove` and `sync` keep their contract: path based, no handles, a root argument,
  synchronous, 4 KiB per call. FS adds what plugins do with **folders** and **file information**,
  as one versioned service, `core.files` (D4: no new one-off slots).
- [D] **Only what first-party plugins use.** Every op below is used by at least two plugins today
  (section 6 lists who). Not offered: copying (plugins copy with `read_at` and `append`, as
  interface 6's `files::copy` does; the SDK keeps that helper), `write_at`, `truncate`, local paths,
  watching a folder, file handles.
- [D] **Bounded calls.** Every op does one bounded piece of card work (`bound: card`). Listing is
  paged, and deleting a folder tree is done a piece per call until the reply says it is done; no
  call walks an unbounded tree.
- [D] **The same roots, paths and statuses as slots 20-26**, plus root 2 (D1.3). Root 1 needs the
  signed feature `uses.storage` (decision 125); everything else needs no feature.
- Out of FS: downloads to a file (NET), the library's catalog, books, annotations and their moves
  and deletes (LIB, `library.md`), installing plugins and firmware (DEV), plugin settings (REG and
  slot 15, decision 126).

## 1. Roots and features

### 1.1 Roots

| Root | Name | What it is | Access |
| --- | --- | --- | --- |
| 0 | data | the plugin's data folder `.paperpaw/plugins/<id>/`, `<id>` stamped from the owner | read and write |
| 1 | card | the user's files from the card's root, without `plugins/` and `.paperpaw/` | read and write, with `uses.storage` |
| 2 | bundle | the plugin's own install folder `plugins/<id>/` (files it ships, such as `data/`) | read only |
| 3 and above | reserved | | `ERR_UNSUPPORTED` |

- [D] **Root 1** keeps decision 125 unchanged: the gate is the signed `uses.storage` declaration
  read at load, never a runtime argument; the first path segment may not resolve to `plugins` or
  `.paperpaw`, compared by identity (case, 8.3 aliases), not by spelling; a plugin reaches its own
  data through root 0 only; settings change through slot 15, never by files, so decision 126 does
  not widen root 1. **One change:** without the declaration, a root-1 call answers `ERR_DENIED`
  (the plugin lacks a feature the host offers, `screens-input.md` section 13), no longer
  `ERR_UNSUPPORTED`, on slots 20-26 and every `core.files` op. A host that offers no root 1 (no
  card files to give) does not offer `uses.storage` and answers `ERR_UNSUPPORTED`.
- [D] **Root 2 (new)** is read-only access to the plugin's own install folder, stamped like root 0:
  `storage.size`, `storage.read_at`, and `core.files` ops `list`, `stat`. Every changing call on
  root 2 is `ERR_DENIED`. Reason: plugins ship data files next to their code (vocabulary's
  `data/phone.txt`, reading-goal's `pictures/`, widgets' `data/quotes.*.txt`), which interface 6
  read through the whole-card storage. Another plugin's install folder is not reachable (see
  [Q1] for about's licence texts). Where the plugin has no install folder (linked into the
  firmware), root 2 is an empty folder: `ERR_NOT_FOUND` for every path but `""`.
- [D] **Listings follow the root's rules.** On root 1, `list` of the card root leaves out
  `plugins` and `.paperpaw`, and `stat` of them is `ERR_INVALID`, as every other root-1 call.

### 1.2 Features

| Key | Allows | Plugins (first-party) |
| --- | --- | --- |
| `uses.storage` | root 1 on slots 20-26 and on `core.files` | dictionary, file-manager, files, format-txt, format-xtc, koreader, news, notes-vault, picture-home, read-later-cloud, sleep, transfer, vocabulary, wallabag |

- [D] `uses.storage` is a signed metadata feature (revision 1), as the DEV features. Root 0 and
  root 2 need nothing. Hosts read it into the per-instance feature bits (#285); the check is the
  shared `feature()` helper.

## 2. The service `core.files` (major 1)

No feature on the ops (root 1 is checked per call, 1.1). Bare replies and paging as
`core.registry` (`registry.md` 2.1): a list writes its rows, then exactly one `service.end`, last;
other ops write exactly their records, no `end`. Input is one record (`input_rows` 1).

| Op | Name | Effect | Input | Reply | Notes |
| --- | --- | --- | --- | --- | --- |
| 1 | `list` | query | `fs_list` | `fs_entry`*, `end` | one folder, paged, `page_rows` 32 |
| 2 | `stat` | query | `fs_path` | `fs_entry` | one file or folder |
| 3 | `space` | query | `fs_path` | `fs_space` | free and total bytes of the root's medium |
| 4 | `make_dir` | mutate | `fs_path` | (none) | creates the folder and missing parents |
| 5 | `remove_tree` | mutate | `fs_path` | `fs_progress` | removes a folder and all in it, a piece per call |
| 6 | `move` | mutate | `fs_move` | (none) | moves or renames a file or folder, also between roots 0 and 1 |

### 2.1 `list`

- [D] `fs_list.path` names a folder (`""` is the root). Rows are the folder's entries in the
  storage's order (no sorting: a plugin sorts what it shows), `.` and `..` never, hidden names
  (`.x`) included (plugins filter them). Each row is an `fs_entry` (2.6).
- [D] **Paging.** `cursor` 0 starts; `service.end.next` is where the next page starts, 0 when the
  folder is done (`flags` bit 0 `more` set when not). The cursor is the storage's place
  (`Storage::list_from`), opaque to the plugin and valid only for the same folder. An entry added
  or removed between pages may be listed twice or not at all; a place that no longer holds starts
  over at a place the host can find, never fails and never shows an entry that is not there
  (the trait's promise). `service.end.total` is 0 (unknown) and `generation` 0: folders have no
  generation.
- [D] A page holds as many whole rows as fit in the op's `max_reply` (4,096 bytes), at most 32. A
  folder of thousands of entries is so read once in all, not once per page (notes-vault indexes up
  to 50,000 notes this way).
- [D] `flags`: none yet; other bits `ERR_UNSUPPORTED`.
- `ERR_NOT_FOUND`: the folder is missing; `ERR_INVALID`: the path names a file.

### 2.2 `stat`

- [D] Answers one `fs_entry` for a file or folder: `name` is the last path segment (`""` for the
  root), `size` the file's bytes (0 for a folder), `modified` where the storage keeps it.
  `ERR_NOT_FOUND` when there is none. It replaces interface 6's `exists`, `size` on folders and
  `modified`.

### 2.3 `space`

- [D] Answers `fs_space` for the medium the root lives on (`path` must be `""`, else
  `ERR_INVALID`). `free` is bytes free for new files **after the host's reserve** (the storage
  contract's D2.6, so a plugin never plans to use the room the kernel keeps for its own saves);
  `total` the medium's size. `flags` bit 0 `free_known`, bit 1 `total_known`: on the X4, free space
  is unknown until the FAT has been counted (G-free-1), and then `free` is 0 with bit 0 clear, never
  a guess. Bit 2 `ignores_case`: names that differ only in case name one entry (FAT), so a
  case-only rename needs care (file-manager).
- Store and transfer check room before an install or an upload with it; a write can still fail
  with `ERR_FULL` (the reserve and other writers apply at the time of writing).

### 2.4 `make_dir`

- [D] Creates the folder and every missing parent; an existing folder is `OK`; an existing file
  at the path or a parent is `ERR_INVALID`. Not on root 2 (`ERR_DENIED`). The root itself (`""`)
  always exists (`OK`). Power cut: some parents may exist, never a half-made folder entry (FAT
  creates each folder whole before linking it, as the platform's `create_dir`).

### 2.5 `remove_tree`

- [D] Removes the folder at `path` and everything in it, at most **64 entries per call**, deepest
  first, and replies `fs_progress`: `removed` this call, `flags` bit 0 `done`. The plugin calls
  again with the same path until `done`; a missing folder is `done` at once with `removed` 0. A
  file at the path is removed as a one-entry tree. Interface 6's `remove_dir_all` becomes a loop in the SDK
  helper, so every call stays bounded.
- [D] Root 0 `""` empties the whole data folder (the folder itself stays). Root 1 `""` is
  `ERR_INVALID` (never the whole card); root 2 is `ERR_DENIED`.
- [D] Power cut or a call that fails part of the way: what was removed is gone, the rest is whole
  (each entry is removed by the storage's own `remove`); calling again goes on. Nothing outside
  `path` is touched. The host never follows a link out of the folder (FAT has none; hosts with
  links remove the link, not its target).

### 2.6 `move`

- [D] Moves or renames the file or folder `from` to `to`, also into another folder, and between
  roots 0 and 1 (both on the device's one storage: a news article written in the data folder moves
  into the user's books; an upload moves from the data folder to the card). Root 1 on either side
  needs `uses.storage`; root 2 is `ERR_DENIED`.
- [D] `flags` bit 0 `replace`: an existing **file** at `to` is replaced. Without it, an existing
  `to` is `ERR_STATE`; an existing **folder** at `to` is always `ERR_STATE` (never merged). Other
  bits `ERR_UNSUPPORTED`. A folder moved into itself or below itself is `ERR_INVALID`. The parent
  folder of `to` must exist (`ERR_NOT_FOUND`; plugins call `make_dir` first). A case-only rename
  (`Book.epub` → `book.epub`) is a rename, not a replace, where the storage ignores case.
- [D] **Power cut:** as `storage.rename`: the new entry is added before the old one is removed, so
  the file or folder is found at both places or at one of them, never at neither. For a folder, the
  host also points its `..` entry at the new parent; a cut between leaves a folder that is whole and
  readable at the new place (the `..` entry of FAT is not used to find files). Core's cut test
  covers a folder move (handoff, section 7).
- [D] A move between two different storages (a host with the data folder on internal memory and
  the user's files on a card) is `ERR_UNSUPPORTED`; the plugin then copies with `read_at` and
  `append`. No first-party host has one today.
- Books moved by a plugin through root 1 are not followed by the library: the library sees the
  change at its next look (as in interface 6, where file-manager tells it through
  `library::tell_moved`). Plugins that move books they know as books use LIB's `move` (journalled,
  `library.md`), which keeps positions and annotations.

### 2.7 Records

| Tag | Record | Fields |
| --- | --- | --- |
| 1142 | `service.fs_path` | `root: u32`, `path: str 512` |
| 1143 | `service.fs_list` | `root: u32`, `path: str 512`, `cursor: u32`, `flags: u32` |
| 1144 | `service.fs_move` | `from_root: u32`, `from: str 512`, `to_root: u32`, `to: str 512`, `flags: u32` |
| 1145 | `service.fs_entry` | `name: str 255`, `flags: u32`, `size: u64`; optional `modified: u64` (ordinal 0) |
| 1146 | `service.fs_space` | `flags: u32`, `free: u64`, `total: u64` |
| 1147 | `service.fs_progress` | `removed: u32`, `flags: u32` |

- `fs_entry.flags`: bit 0 folder; others 0. `size` is 0 for a folder. `modified` is seconds since
  1970-01-01 as the storage keeps it (FAT: the device's local time read as UTC, the platform's
  `DirEntry::modified`); absent where the storage keeps none. A FAT time is never before 1980, so
  no time is negative and `u64` holds every one.
- `name` is one segment of at most 255 bytes (the FAT limit), never containing `/`.
- Paths follow the storage contract (D2.3): relative, UTF-8, at most 512 bytes and 255 per
  segment, no `.`, `..`, `\`, `:`, control characters, empty segments, trailing dots or spaces.

## 3. Statuses

Every op checks, in this order, before it touches anything: the record framing (generic,
`stages-services.md`), the root (`ERR_UNSUPPORTED` for 3 and above, `ERR_DENIED` for root 1
without `uses.storage` and for a change on root 2), the path rules (`ERR_INVALID`, `ERR_UTF8`,
`ERR_NO_SPACE` over a cap), the root-1 exclusions (`ERR_INVALID`), then the op's own rules above. A refused call
changes nothing.

| Condition | Status |
| --- | --- |
| Missing file or folder where one is needed | `ERR_NOT_FOUND` |
| The target exists (`move` without `replace`, a folder at `to`) | `ERR_STATE` |
| Card full or the reserve reached (`make_dir`, `move` that needs a new folder entry) | `ERR_FULL` |
| Card removed, unreadable, or lent to a computer as a USB drive | `ERR_IO` |
| Called outside a callback | `ERR_STATE` (slot rules) |
| Re-entry into `core.files` from its own call | `ERR_BUSY` |

- [D] **No card.** Every root-0, root-1 and root-2 call answers `ERR_IO` while the card is away
  (both live on it on the X4). A plugin that shows the card's state asks `stat` of root 1 `""` (the
  `files` plugin's probe): `OK` is present, `ERR_IO` gone. Interface 6's `StorageError::CARD_GONE`
  maps here.

## 4. Uninstall and the data folder (decision 124)

- [D] Removing a plugin asks the user whether to delete its data folder (root 0); the default is
  to keep it. Kept, it is found again when the plugin is installed again.
- [D] The host deletes only `.paperpaw/plugins/<id>/` resolved from the validated id (non-empty,
  not `.` or `..`), never a path a plugin gave, compared exactly (removing `battery` leaves
  `battery-plus/`); an empty or unresolvable id is a refusal, never the data root. It first renames
  the folder into a kernel-owned `.removing` folder and removes it there, finishing at the next
  start after a cut, so a cut never leaves other plugins' folders touched (Senior reviews the kernel
  PR; Core's handoff, section 7).
- Root 2 (the install folder) goes with the plugin, always.

## 5. Memory

- Host: no heap per call. A path is checked and joined in a stack buffer of at most 640 bytes, as
  slots 20-26. `list` writes rows into `out` as the storage visits entries, at most 4,096 bytes; a
  row that does not fit ends the page there (its place is `next`), so no row is cut and nothing is
  held between calls. `remove_tree` keeps no state between calls: each call finds the deepest
  entries again (at most 64 removals and the folder reads needed to find them).
- Plugin: a page is at most 4 KiB; the SDK decodes it with the generated stream decoder.

## 6. Who uses what (first-party, interface 6 audit)

| Op | Plugins |
| --- | --- |
| `list` | dictionary, file-manager, koreader, notes-vault (with the cursor), picture-home, sleep, store, transfer, update, vocabulary |
| `stat` | most (`exists`, `size`, `modified`): file-manager, files, koreader, notes-vault, picture-home, sleep, vocabulary, wallabag |
| `space` | store, transfer (room checks), file-manager (`ignores_case`) |
| `make_dir` | file-manager, files, koreader, news, notes-overview, notes-vault, read-later-cloud, store, transfer, update, vocabulary, wallabag |
| `remove_tree` | dictionary and format-xtc (caches), file-manager, transfer (user folders), picture-home, update |
| `move` | koreader (`.sdr` folders), update (a fetched folder), file-manager (folders and files), news, read-later-cloud, wallabag, transfer (data folder to card), files, store |
| root 2 | vocabulary (`data/phone.txt`), reading-goal (`pictures/`), widgets (`data/quotes.*.txt`) |

Uses the audit found that are **not FS**, and where they go:

| Interface 6 use | Plugins | Interface 7 |
| --- | --- | --- |
| The kernel's annotation store (`.paperpaw/…/annotations`) | notes-overview, wallabag | LIB annotations (`library.md`) |
| The library's index, `books_in`, `tell_moved`, `changed` | file-manager, files | LIB catalog and `move`/`delete` |
| `.paperpaw/boot.txt` | update | DEV `firmware` (op 3) |
| `.paperpaw/tmp/` | transfer | root 0 |
| `plugins/` (staging, `.next` folders) | update, store, transfer | DEV install commands (the host stages) |
| Downloads written to a file | store, update, wallabag, news | NET (download to a root-0 file) |
| Copying a file | koreader, vocabulary, file-manager | the SDK's `files::copy` over `read_at`/`append` |

- [Q1] **about** shows the licence texts in `plugins/fonts-default/fonts/*.txt`, another plugin's
  install folder, which root 2 does not reach. Proposal: about ships the three texts in its own
  `data/` (root 2), or reads them through REG's plugin description. Plugin decides.
- [Q2] **file-manager** lists and moves books on the card through root 1 and tells the library
  afterwards (interface 6 `tell_moved`). With FS plus LIB, it either moves books through LIB's
  `move` (journalled, keeps positions) or leaves the library to notice. Proposal: LIB `move` for
  files the library knows, root 1 `move` for others. Plugin confirms.

## 7. Handoffs (host side, after approval)

- **Core:** `core.files` backend over the platform `Storage` (ops 1-6), root 2 resolution (stamped
  install folder, read only), `ERR_DENIED` for root 1 without `uses.storage` on slots 20-26 too,
  the root-1 identity exclusions in `list` and every op, the folder-move `..` update and a cut test
  for it, `remove_tree` pieces of 64; the uninstall data removal of section 4 (its own PR, Senior
  reviews).
- **Services:** SDK facade `native::files` (typed `list` pages with the cursor, `stat`, `space`,
  `make_dir`, `remove_tree` looped to done, `move`), the root enum, and `files::copy` over
  `read_at`/`append`.
- **Plugin:** ports use root 2 for shipped data and declare `uses.storage` where they use root 1.
