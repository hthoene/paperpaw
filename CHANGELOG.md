# Changelog

What changes from one release of PaperPaw to the next. Versions follow
[Semantic Versioning](https://semver.org); before 1.0, a minor version may
change the plugin SDK.

## [Unreleased]

## [0.6.0-beta.4] - 2026-11-20

- Interface 7 SDK: `native::storage::DataStorage`, the plugin's data folder as a
  `Storage`. Reads and appends over 4 KiB are split; a longer write is staged in
  `name.pp-staged`, synced and renamed over the old file, so it is the old file or
  the new, whole. The suffix `.pp-staged` is reserved: `native::storage::write`,
  `append`, `rename` and `remove` refuse (`Invalid`) any name ending in it, in any
  case, so only the staging touches such files.

- Interface 7 draft 7 SDK: typed callbacks and frame-owned drawing, font metrics
  and ink, nested clipping, exact snapshot views, complete message copies,
  settings/grant reads, typed callback subscriptions and
  screen/action/redraw/setting commands.


- Interface 7 draft 5 SDK: bounded callback replies, typed draw frames and input,
  screen/action declarations and events, catalog formatting and digit conversion,
  and distinct permission/timeout errors. Callback input has no fixed glue cap.

### Added

- `paperpaw-sqlite` writes, column names and schema reads allocate fallibly (`Error::OutOfMemory`,
  `try_encode_record`, `columns` returns a `Result`); `Md5::hex_into` and `md5::partial_into`
  write hex digits without allocating (#228).
- Battery is a native interface-7 plugin only: the interface-6 plugin, its view and its Kernel
  tests are removed, the events table and storage goldens run on the native recorder, and the app
  and the store-file build no longer register it until Core publishes native plugins (#228).
- Keyboard 1.1.2: the interface-7 native plugin (the text dialog on `core.keyboard` answering
  through `core.ui`, keys, shift, variants, paste, type on the phone, secret texts, the layouts
  chooser) on `paperpaw-ui-kit`; hardware-typed text, font-coverage filtering and layouts added by
  other plugins wait for host pieces and are named in `plugins/keyboard/PORTING-7.md` (#228).
- Settings 1.0.7: the interface-7 native plugin (main page, group pages, choosing a value, themes,
  search, reset, restart (asks first, declares `uses.power`), number editing with preview) over the registry service; `paperpaw-ui-kit` item
- Native SDK library field reads use lossless pages with a separate book-field
  generation cursor. Typed book-field and badge declarations expose the existing
  library records to providers (#228).

- Home 1.0.5: the host-free model of the pages for interface 7 (the grid of cells and the pages,
  with fallible allocation and refusal sweeps); the screens follow (#228).
- `paperpaw-config`: `Value::try_as_str` (one fallible reservation for escaped values, so an X4
  plugin gets an error instead of an abort) and `Value::is_escaped` (#228).
- Native SDK Bluetooth commands (`CommandSender::bt_scan`, `bt_stop_scan`, `bt_pair`, `bt_connect`,
  `bt_disconnect`, `bt_forget`, `bt_give`, `bt_take_back`, `bt_keys`) and the typed `event.bluetooth`
  (`Event::Bluetooth` with typed kind, device kind and reason; unknown values read as `Other`) for the
  plugin bound to `core.bluetooth` (#228).
- Settings 1.0.6: the interface-7 native plugin (main page, group pages, choosing a value, themes,
  search, reset, number editing with preview) over the registry service; `paperpaw-ui-kit` item
  rows gain − and + buttons and show a title that is already translated as it is (#228).
- Core native callbacks carry authoritative registry owner keys and checked,
  nonzero callback epochs. Failed admission cannot reuse an issued owner key (#228).
- `paperpaw-document`: `OffsetMap::try_checkpoint` (fallible, refuses out-of-order and
  oversized indices) and `OffsetMap::checkpoints`, the rows of `doc_map` (#228).
- Native SDK storage roots `Install` (root 2, the plugin's own install folder, read only)
  and the callback-scoped format roots `Book` (3), `UserCss` (4) and `DocumentCache` (6).
  Empty paths identify the book/CSS files; the host enforces permissions, lifetime and
  cache limits (#228).
- Native SDK `EventKind::Answered` (subscribe to `core.ui` question answers) and
  `Snapshot::display_gray_levels` / `display_is_colour`. Interface draft 22 (`native::DRAFT`)
  refuses plugins built for draft 21 (#228).
- SDK: `native::net` (typed `core.net@1` requests with checked headers, bodies, files and
  limits, answer and body reads, cancel) and the `HttpDone` and `HttpProgress` events with
  `EventKind::HttpDone` and `HttpProgress` (#228).
- Native SDK `native::lookup` (synchronous `core.lookup` asks and provider answers) and
  `LookupProviderDecl`. Interface draft 23 (`native::DRAFT`) refuses plugins built for draft 22
  (#228).

- Reading profiles 1.0.9: the interface-7 native plugin (list, apply, update, rename, remove, next),
  `profiles.ini` versions 1 and 2 kept byte for byte; `paperpaw-ui-kit` gains item lists with
  footers, whole-file storage helpers and a screen registry (#228).

- Buttons and gestures 1.0.8: the interface-7 native plugin (list of inputs, place chooser, action
  picker) on `paperpaw-ui-kit`'s item lists (#228).

- Menu editor 1.0.8: the interface-7 native plugin (menus, entries, entry options, add), built
  on `paperpaw-ui-kit`; dragging an entry and the soft focus style wait for host input and
  display support (#228).

- `paperpaw-ui-kit`: drawing helpers for interface-7 plugin screens (stock UI style,
  header with back and pager, paged list rows with focus and hit-testing, text fitting, catalog
  messages) over the flat native SDK, extracted from Battery and the menu editor (#228).

- Native SDK typed device commands: Wi-Fi radio, network policy and power
  (`CommandSender::wifi_join`, `go_online`, `sleep`, `show_then`, `wear` and others) (#228).
- Native SDK library queries, borrowed book metadata and badges, finished-status
  updates and plugin-owned field writes (#228).
- Native SDK typed device inventory and status reads, including firmware, native
  build, hardware values and memory probes, with validated borrowed reply pages (#228).
- Native SDK widget and status provider adapters decode generated stage requests
  and encode complete replies without heap allocation. Widget targets and status
  values use bounded scratch and refuse oversized replies (#228).
- Core native screen admission uses a shared declaration entry, fallible factories,
  retained owners and declared instance policies. Native screen lifecycle events
  carry nonzero instance ids. Other declaration families remain refused until wired (#228).

- Native SDK screen instance policies, borrowed confirmation button labels and flags,
  dialog label accessors, and an explicit user-defined menu getter (#228).

- Interface-7 generated stage schemas for `core.input-filter`, `core.widget` and
  `core.status` share bounded payload codecs between the host and SDK (#228).
- Native slot 32 rejects overlapping buffers and delegates bounded inputs and
  uninitialized output directly through the callback frame; production broker wiring follows (#228).

- Interface-7 EVT SDK subscriptions and borrowed typed events cover settings,
  book sessions and file edges, plugins, Wi-Fi, power, status and charge. Page
  turns and book edges share `ReadingView` accessors; malformed values are
  refused and reserved state values use the contract fallbacks. Interface draft
  15 (`native::DRAFT`) refuses plugins built for draft 14 (#228).
- `paperpaw-document`: `Block::try_transform` runs a filter on a working copy of a
  block's text, offset map and runs (`TextEdit`: `try_replace`, `try_set_runs`,
  `try_push_run`, kind, alignment, dropping the picture) under a byte limit checked
  before every reservation, and puts it back only on success; out of memory, the
  limit or the filter's own error leave the block unchanged. `Block::replace` shares
  its code and leaves the block unchanged where memory runs out (#228).
- Interface-7 SDK stage declarations, borrowed subscriber calls and lending reply
  cursors expose host-stamped style grants; schema-checked service handles survive
  callbacks and release explicitly; interface draft 14 (`native::DRAFT`), so plugins built for
  draft 13 are refused (#228).
- Interface-7 call-service host slots 33–34 use generated schemas, bounded reply
  scratch and owned service handles; device hosts refuse test-only schemas (#228).
- Interface 7, family 2 (registry, `docs/abi/registry.md`): the `core.registry` service (major 1,
  21 paged read ops over settings, plugins, actions, screens, slots, menus, bindings, inputs, setup
  steps, presets, widgets and status items and lines; rows 1025-1040 and 1059), the registry
  commands 1536-1546 (reset, preview,
  book scope, menus, key and slot bindings, plugin switching, plugin requests and answers,
  presets), events 75-78 (registry and preview changes, plugin requests and answers) and the
  declarations 21-28 (`setup.step`, `slot.spec`, `slot.offer`, `key.bind`, `setting.default`,
  `setting.choice`, `preset.spec`, `preset.value`); the feature `uses.registry`. Commands of the
  growth block are generated from their `abi.def` fields into the command codec. Hosts answer
  `ERR_UNSUPPORTED` until wired. Interface draft 16 (`native::DRAFT`) refuses plugins built for
  draft 15 (#228).
- Interface 7, family 2 UIX (`docs/abi/ui.md`): screen instances with lossless hand-overs
  (`core.ui` ops 16-17), navigation commands 1547-1552 and `screen_policy` 1563, plugin
  inputs (`input.spec` 29), questions and answers with paged host texts (ops 8-11, event 82),
  widgets and status items, lines and places (declarations 224-228, events 84-86, input 193),
  help hints, `screen_ready`, `notify`, speech (1557-1558, 1561-1562, events 87, 89), the
  clipboard (ops 18-19), and host slot 35 `ui.compose`, which answers `ERR_UNSUPPORTED` until
  the host wires it. `event.screen_shown` (38) gains the instance it shows. Service ops name
  their row counts (`ServiceOp::input_rows`, `paged`, `reply_rows`); an input batch above 8
  records is encoded with `records_encode_counted` and decoded by the host with
  `host_records_visit` (op 16: 32 params). Typed SDK facades: `native::ui` (navigation,
  hand-overs, questions and answers with host texts, widgets and status composition, help, speech,
  clipboard) and `native::registry` (registry declarations, commands, paged queries and events),
  without public wire records. Interface draft 17 (`native::DRAFT`) refuses plugins built for draft
  16 (#228).
- Interface 7, family 4 DOC and LIB (`docs/abi/document.md`, `docs/abi/library.md`): the
  `core.document` service (21 ops: open with book options, metadata, sections, lossless block
  fragments, table of contents, XPointers, links, fonts, cover and images by a stable picture
  index, formats and readers), format, reader and lookup declarations with the `core.lookup`
  service, and the `core.library` service behind the new feature `uses.library` (26 ops:
  catalog pages, books, texts, facets, covers, status, journalled delete, move and import,
  positions, annotations with staged transactions and paged receipts, fields, provided
  metadata and book keys); declarations 31 and 229-233, events 90-91, rows 1060-1124 and
  the image commands 1559-1560; `input.pointer` gains the optional `contact` (simultaneous
  touches told apart, interface 6 `PointerEvent::id`), which the SDK's `Input::Pointer` carries
  as `contact` (0 when absent); `screen.spec.flags` bits 6-7 give a screen's `instances` policy
  (one per params, single, many; interface 6 `Instances`), and a confirm dialog's `choices` may
  name its two button words, with flags for a single button and the focus on yes;
  `native::SafeModeReason::PowerEscape` names a start in safe mode after the 8 s Power escape.
  Hosts answer `ERR_UNSUPPORTED` until wired. Interface draft 18 (`native::DRAFT`) refuses
  plugins built for draft 17 (#228).
- Interface 7, family 3 DEV (`docs/abi/device.md`): the `core.device` service (major 1, 15 read
  ops: device, hardware values, firmware, built-in plugins, native build, file checks, memory,
  Wi-Fi, networks, Bluetooth, bonded devices, power, install requests, plugins not installed,
  keys; rows 1125-1141), one command per action 1564-1592 (Wi-Fi, power, installing and removing
  plugins with trust evidence, firmware, file checks, Bluetooth), events 92-98 (scan, join and
  install outcomes, network wanted or idle, firmware progress, Bluetooth) and the features
  `uses.wifi`, `uses.power`, `uses.install`, `uses.firmware` and `uses.bluetooth`. Hosts answer
  `ERR_UNSUPPORTED` until wired. Interface draft 19 (`native::DRAFT`) refuses plugins built for
  draft 18 (#228).
- Interface 7, families FS, NET, SEC, ASY and SOCK (`docs/abi/files.md`, `network.md`,
  `secrets.md`, `work.md`, `sockets.md`): the services `core.files`, `core.net`, `core.secrets`,
  `core.work` and `core.sockets` (major 1, rows 1142-1181), events 99-107 (requests, serving,
  works, sockets) and 108 `event.document_closed`, the features `uses.storage`, `uses.network`
  and `uses.secrets`, an optional `cursor` on `service.end`, and the stage schemas `core.format`
  and `core.lookup` that mirror their service rows. Secret-bearing record fields print redacted.
  `paperpaw_sdk::document` re-exports `TextEdit` and `EditError`; `TextEdit::try_set_text` sets a
  whole text mapped from the block's own source range.
  `service.dev_check` gains the optional `sha256` digest `device.md` already named.
  Hosts answer `ERR_UNSUPPORTED` until wired. Interface draft 20 (`native::DRAFT`) refuses
  plugins built for draft 19 (#228).
- Interface 7, `core.library` op 22 `fields` is paged: it takes `lib_ref` and the new
  `service.lib_field_page` (1182: `cursor`, `generation`), as `docs/abi/library.md` section 4
  says. Interface draft 21 (`native::DRAFT`) refuses plugins built for draft 20 (#228).
- Interface 7, family 1b (core events, `docs/abi/core-events.md`): event records 64
  `event.setting_changed`, 65-67 `event.book_opened`/`book_closed`/`book_finished` (the reading
  position, as `event.page_turned`), 68 `event.book_moved`, 69 `event.book_removed`, 70
  `event.plugins_changed`, 71 `event.wifi_changed`, 72 `event.power_changed` (power mode), 73
  `event.status_changed` and 74 `event.charge_changed` in the generated records codec. Only the
  host raises them (book moves and deletions from its own operations, reading events from its
  reading session); paths are never cut. Hosts admit their registrations once a producer is
  wired; until then `ERR_UNSUPPORTED` (#228).
- Native slot 32 rejects overlapping buffers and delegates bounded inputs and
  uninitialized output directly through the callback frame; production broker wiring follows (#228).
- Native host text ink probes the shaped raster path with the target frame's
  coverage threshold; inkless text reports not found. X4 timing remains gated (#228).
- Interface-7 call-service host slots 33–34 use generated schemas, bounded reply
  scratch and owned service handles; device hosts refuse test-only schemas (#228).
- Interface 7, family 1b (core events, `docs/abi/core-events.md`): event records 64
  `event.setting_changed`, 65-67 `event.book_opened`/`book_closed`/`book_finished` (the reading
  position, as `event.page_turned`), 68 `event.book_moved`, 69 `event.book_removed`, 70
  `event.plugins_changed`, 71 `event.wifi_changed`, 72 `event.power_changed` (power mode), 73
  `event.status_changed` and 74 `event.charge_changed` in the generated records codec. Only the
  host raises them (book moves and deletions from its own operations, reading events from its
  reading session); paths are never cut. Hosts admit their registrations once a producer is
  wired; until then `ERR_UNSUPPORTED` (#228).

- Interface-7 host usage/name/note services and callback-scoped blobs, with checked
  metadata admission, bounded heap snapshots and generation-checked reads/releases (#228).
- Interface 7, family 1a (stages and services, `docs/abi/stages-services.md`): host slots 32
  `stage.run`, 33 `service.open` and 34 `service.call`; records 19 `stage.spec`, 20
  `registration.stage`, 512 `stage.call`, 513 `stage.reply` and the test-only 1024 `service.echo`;
  service schemas in `abi/services/<key>@<major>.def`, validated by `tools/abi-gen.py`, which
  writes the shared op table `services_wire` and pins it in the lock (format 9, append-only ops,
  new schemas at an unreleased minor). Hosts answer `ERR_UNSUPPORTED` and refuse stage
  declarations until they wire them (#228).
- Interface 7: `native_plugin!(P, metadata = M, linked = NAME)` and `native::Linked` for plugins
  linked into the program that hosts them (firmware built-ins): the same entry and metadata under
  a Rust name of their own, without exported symbols, heap word, allocator or panic handler, so
  several link into one program; the host reads the entry with `Linked::entry` (`kernel`) and
  admits it as a loaded plugin (`abi/entry.md`, linked built-ins) (#228).
- Interface 7: command 60 `command.set_item_state` in the generated command codec: a plugin says
  what menus show of its own actions and settings (label and badge as catalog keys with arguments,
  hidden, faint, a switch) when it changes, instead of interface 6's callback asked on every menu;
  with `keep` the host restores it at boot without starting the plugin. Hosts answer
  `ERR_UNSUPPORTED` until their menus take it (#228).
- Graphics: `Framebuffer::probe`, a framebuffer that keeps no pixels but where masks put ink
  (pixels at or above the frame's ink threshold, `Framebuffer::ink_threshold`, unclipped), and
  `InkBounds`; host-only `FontHandle::ink_drawn`
  measures text ink by the drawing path itself (shaping, joining, marks, jamo), for slot 30
  `text.ink` (#228).

- Checked native admission metadata exposes declared service revisions through a
  borrowing iterator, so host query admission uses the validated requirements (#228).
- Interface 7 SDK: typed, borrowed builders for menu entries, icons, device needs,
  sections, hidden entries and item descriptions, including static build-script batches.
- Interface-7 SDK snapshot services with callback-bound blob leases, borrowed plugin usage paging,
  device usage, translated plugin names and work-note readers. Includes a deterministic fake-host
  fixture for Battery port tests; helpers use bounded buffers and allocate nothing (#228).
- `Id::try_parse` and `IdError`: a parse that tells an invalid id (`IdError::Invalid`) from a full
  heap (`IdError::NoMemory`) instead of aborting, so the X4's plugin admission can make new ids
  from declarations and answer `ERR_NO_MEMORY` (#228).
- `Id::try_clone`: a copy of an id that gives `None` where the heap refuses instead of aborting
  (kept ids copy without allocating), for hosts that must survive a full heap (#228).
- Interface 7: the heap word (`abi/heap-word.md`). A plugin's optional read-only word
  `PAPERPAW_HEAP`, which the loader fills before `make`, points at the host's context-free
  `PpHeap` table (alloc, dealloc, realloc, abort). `native_plugin!` emits the word, a global
  allocator over it and a panic handler that reports to the host without allocating, or traps
  where there is no usable word; `heap = none` leaves all three out. The generated C header has
  `PP_DEFINE_HEAP_WORD()` and the checked `pp_heap()`; the X4 linker script keeps the section.
  Hosts and loaders take the word in follow-up work (Core, Firmware) (#228).
- Interface 7: declaration records 13 `menu.entry` (any action or setting, also another plugin's
  or a core setting, into a menu, by order or beside another entry), 14 `icon.spec` (an icon for
  any item, so icon themes work), 15 `menu.need` (offered only on devices with touch, speech,
  Wi-Fi and the like), 16 `menu.section` (a heading), 17 `menu.hide` (takes an entry out, also
  another plugin's) and 18 `menu.describe` (a line under an entry), in the generated records
  codec: interface 6's menu calls as declarations. An unknown place or need is accepted (the entry
  is not shown), so plugins for a later host still load. Hosts check them and answer
  `ERR_UNSUPPORTED` until their menus take them; a plugin's translations stay the
  `i18n/<language>.ini` files of its folder (#228).
- Interface 7: host slots 29-31 (`text.metrics`, `text.ink`, `canvas.clip`) for text placed by its
  line and ink and for nested clipping, commands 56-59 (open screen, run action, redraw, close
  screen) in the generated command codec, and generated i18n format/digits limits. Hosts answer
  `ERR_UNSUPPORTED` until they wire them; the SDK keeps private generated copies of the snapshot,
  settings, colour and command codecs (#228).
- Interface-7 SDK callback host binding and `native_plugin!` entry glue, with checked table admission
  and fallible host allocation. Storage slots 20-26 use caller-owned buffers; event tags 32-37 decode
  into borrowed payloads. These facades do not activate any shell loader (#228).

- Core adapter for shell-owned interface-7 plugins: deliver registered lifecycle, battery and page-turn
  events through the kernel subscription pipeline with bounded encoding and callback-scoped environment
  access. Shells supply the owner bridge and failure logging; loader wiring follows separately (#228).

- Host-side storage adapter for interface-7 slots 20–26 (path rules, free-space reserve, per-plugin
  read/write/sync counting), not wired (#228).
- `paperpaw-sign pack` writes the X4's interface-7 plugin file (format 5, `abi/ppn5.md`) for a plugin linked with
  `paperpaw-plugin7.x` (it defines `PAPERPAW_METADATA`), reads it back as the loader does, and refuses the X4 Pro (target 9),
  an exported symbol besides the two, writable data and more than 8,192 patches (a patch record is three words, sorted by site: `abi/ppn5.md` decisions 4 and 5); `plugin`, `sign-plugin` and `check-plugin`
  read both formats. `samples/abi7-x4-min` is a riscv32imc sample built by it (#228).
- Interface-7 draft: host-table slots 20–26 (`storage.size`, `read_at`, `write`, `append`, `rename`, `remove`,
  `sync`) declared in `abi/abi.def`; the default table answers `ERR_UNSUPPORTED` until they are wired. The
  storage contract records owner decisions 124 (uninstall asks before deleting the data folder) and 125 (root 1
  for the user's files, gated by a signed `uses.storage`) (#228).
- Interface-7 draft: service records 48-52 (`service.usage`, `plugin_usage`, `plugin`, `work_note`, `end`) and the
  four data services of slot 17 in the metadata catalog (semantic revision 0, no call table), with a generated
  codec for one bare record (`record_framed_len`, `record_encode_one`, `record_decode_one`). A plugin cannot submit
  them (#228).
- Desktop emulator: a developer switch, `--abi7-dev-loader` or `PAPERPAW_ABI7_DEV_LOADER=1` (Linux x86-64, off by
  default), loads interface-7 plugin libraries (`plugins/<id>/code-host.so`, told from their bytes) for loader work. What a
  plugin declares is staged but not applied yet, and nothing is drawn; interface-6 plugins are unchanged (#228, #772).
- Interface-7 draft: statuses `ERR_NOT_FOUND` (-10) and `ERR_FULL` (-11), and record codecs generated from
  the fields declared in `abi/abi.def` (optional appended fields, tag ranges per record family) (#228).
- Interface-7 draft: event records 32-37 (`started`, `resume`, `suspend`, `stopping`, `battery_measured`,
  `page_turned`) with host encoders in `native_host::events`; a plugin can register for them (#228).
  An optional scalar field declared with `"default": null` is generated as `Option<T>` (present exactly
  when its bit is set): `page_turned`'s `page` and `pages`, which have no value meaning "not counted".
- `tools/hyphenation-packs`: builds and checks the thirteen hyphenation plugins as data plugins
  (`[hyphenation]` file kind, PPHP version 1 tables); the interface-6 plugins stay until the
  card-backed reader lands (#228).
- `paperpaw_sdk::native::declare` (interface 7, merges with the raise): a builder for the declaration records 5-10 (setting groups, specs and extras, hyphenation, font faces and extensions) that splits them into batches and encodes them with the generated codec, with golden settings batches of the widgets and Battery plugins as fixtures (#228).
- `paperpaw-hyphen`: `check_table`, `Layout`, `TableError` and the limits `MAX_BLOCKS` (4096), `MAX_BLOCK_BYTES` (512) and
  `MAX_EXCEPTION_BYTES` (8 KiB) behind the `kernel` feature (not seen by plugins), the one check of a pattern table that the
  kernel's reader and `paperpaw-dev package` share (#228).
- Interface-7 groundwork: generated typed settings values and scoped settings/translation
  copy-out adapters, with bounded handle and callback admission checks (#228).
  The new host table is not yet activated.
- Host-only pinned plugin owners and transactional registration staging, with
  explicit cleanup ownership on commit failure (#228). Runtime wiring remains inactive.
- Scoped submission of validated host records during plugin construction (#228),
  clearing access on return or unwind and latching validation refusals even when
  ignored by the caller; foreign registration decoding remains unwired.
- Generated optional event/draw callback tails, bounded table readers and callback
  minor requirements for the interface-7 prototype (#228). No live dispatch yet.
- `native::DRAFT`: the draft of the plugin interface, part of `native::FINGERPRINT`, so a plugin built for
  an earlier draft is refused. Freezing the interface (the owner's decision) deletes it (#228).

- USB developer tools (Customise > Developer, "USB developer tools"): a computer on the USB cable can press keys, take screenshots and read and write books and plugin files (`tools/x4-dev.py`, `docs/developer-link.md`). The kernel enforces the rules itself: it is off by default and on only after a question on the device, off again after every restart, sleep, unplugged cable or ten quiet minutes; a `DEV` mark shows while it is on; secrets (`.paperpaw`, Wi-Fi, keys, `secrets*`), code and firmware are never read or written, files are limited to the library, `plugins/<id>/` and `screenshots/`, and commands are rate limited. The row is hidden where USB cannot carry it. No interface change (#12).

- Anki: study the cards due in Anki on your computer on the reader and send your answers back, through the AnkiConnect add-on (a store plugin, #26). Answers are kept while there is no Wi-Fi and go with the next sync.

- Books show real tables: EPUB, HTML, FictionBook and notes-vault Markdown tables are set in columns sized from their content, each cell's text broken in its column, with rules around and between the cells and the header row in bold. A row never splits across pages; a table too wide for the page (columns narrower than two ems) or a row taller than a page is shown as before, its cells in a line set apart with `·`, so no text is cut. Block quotes have a rule beside their lines and notes-vault callouts a box around them. Books are converted again once to show them (#27).

- **RTF documents**, a new store plugin (`format-rtf`, #30): opens `.rtf` files as books, with paragraphs, headings (the outline level or a style named *heading N* or *Title*, with a table of contents), bold, italic, underline and strike-through, lists, simple tables (a row a line, the cells set apart by `|`), `\uN` characters and the Windows-1250, 1251 and 1252 code pages, and JPEG and PNG pictures; title, author and language come from the document. The file is read in pieces by a streaming parser (about 30 KB of memory however long the document is); headers, footers, footnotes, fields' codes, hidden text, unknown destinations and other kinds of pictures are left out. MOBI, DOCX and pictures as books were added before; comic archives in RAR form (CBR) are not planned.

- USB drive mode on the X4 Pro and X4 Classic (Tools > USB drive): the card appears on the computer as a drive (USB mass storage on the chip's OTG core), while the reader unmounts its own file system and shows a notice. Eject the drive on the computer, press Back (Home or Power on the Pro), or pull the cable: the card is mounted again and the library is walked for what was copied. The USB log and remote link are off for the time. Not on the X4 (ESP32-C3 has no OTG core): the entry is hidden there (#32).

- **exFAT memory cards** (#54, experimental). The reader now reads and writes cards formatted as exFAT (how many cards of 64 GB and more are sold), with the same safety as on FAT32: every change to a folder is one block's write, so a power cut leaves the files as they were or as they are now. Mounting a 64 GB card takes four card commands and about 14 KB of the heap. The first time an exFAT card is used the reader says that exFAT support is experimental and that FAT32 is the safest; FAT32 cards work as before. The card's exFAT code is written from hadris (MIT), see About PaperPaw.

- **Choose a setup when you set PaperPaw up** (#72). The setup guide has a new page after Wi-Fi: *Choose a setup* with ready-made PaperPaw templates — *Minimal*, *Reading statistics*, *Language learning* and *Notes* (English and German names and one-line descriptions) — and *Keep the defaults*. Choosing one applies its settings at once, switches on the built-in plugins it needs and fetches the others from the plugin store in the background (the plugins they require come with them). Without a connection the plugins wait, a notice says they will be installed when you are connected, and the store fetches them as soon as Wi-Fi connects (or at its next start online, or when it reaches the library). Plugin store 1.0.15; no interface change. Sharing your own setups comes later.

- Mail (#81): an optional background check (settings "Check for new mail in the background", off by default; every 15, 30 or 60 minutes). It checks the accounts one connection at a time while the reader is awake and online, and the Home widget shows the unread count. A mail's EPUB, PDF, MOBI, AZW3, FB2, TXT, DOCX or CBZ attachment opens as a book from the mail ("Open book"). `tools/pps/mail/` holds an end-to-end script and a fake IMAP/SMTP server for the emulator.

- Plugins on interface 7 can declare secret settings, such as a mail password (#81). The value is kept with the encrypted secrets, never in `paperpaw.ini` or the plain settings: no plugin reads it through the settings, not even its own (it asks for its secret), and no other plugin is told when it changes. A value an older card still keeps in `paperpaw.ini` moves into the secrets at the start (on the X4, which seals only while a radio is on, as soon as Wi-Fi is switched on) and is then taken out of the file. A plugin of the card can declare one too. The settings editor can enter, change or clear it.

- Pictures: three new settings under Reading, Display — Picture contrast, Picture brightness and Picture grays (Atkinson as before, Floyd-Steinberg, ordered). They change how every picture the host draws looks, before it is dithered; the defaults are the look as it was (#82).

- **Pictures full screen, zoom and pan** (#92). Hold Select on a page with a picture (on a touch screen: hold the picture) to see it alone, fit to the whole screen. Left and Right step through the page's pictures. Select zooms to 2x, 3x and back (touch: double tap); zoomed, Left and Right pan sideways and the page buttons (or swipes) move on across and down, four fifths of the screen at a time. Back returns to fit, then to the same page (touch: hold to leave). The picture is decoded once more at the size the zoom needs, never larger than 1,600 pixels; a picture too big for that stays at fit with a short note. The grays follow when the device has rested. Pages without a picture still bookmark on a hold of Select. Turning wide pictures needs a new drawing flag and waits for the next interface (#110).

- Edge light: swipe up or down along the left or right edge of the screen to set the front light, like a slider: the light follows the finger in steps of 5 % and a small notice shows the level (a store plugin, #104). The left edge sets the brightness, the right edge the warmth; each can be swapped or left out, and the strip's width is a setting. Taps in the strip, sideways swipes and swipes anywhere else work as before. For devices with a touch screen and a front light (the X4 Pro). The widget *Front light* shows the level and a tap switches the light off and on again; the action *Front light: on or off* does the same from a button or a menu.

- Pomodoro (store plugin, interface 7): a focus timer with phases, rounds of a set, auto-start, an end signal (notice, flash of the screen or blink of the front light) and a daily goal with the last seven days' counts; a screen under Tools, and three home widgets (timer, start/pause button, today's rounds). The clock redraws once a minute, only its own block, never per second (#107).

- A status bar of the host above any screen (#123): native screens say whether it shows with `screen.spec.flags` bits 11-12 `status_bar` (SDK `ScreenDecl::status_bar(StatusBar)`, changed at run time with `ScreenPolicy::status_bar`); the user or other plugins override that per screen with the kernel setting `core.status-bar.screens` (`<screen id>=shown|hidden` per line) and choose its line with `core.status-bar.line`. The setting *Status bar on all screens* (`core.status-bar.all`, off) is the device default. Home, Picture Home and the reader's page declare it hidden. When the time or the battery changes, only the bar is drawn again.

- **Calendar: your calendars on the reader** (#133). A new store plugin with an agenda of the next days and day, week and month views, and an event shown whole (when, where, which calendar, how it repeats). Sources: CalDAV accounts (Nextcloud, Radicale, Baïkal, Fastmail …; the server's address is enough, every calendar with events is read), iCal addresses (`.ics`, `webcal://`), calendar files on the card and, for birthdays and anniversaries, CardDAV address books; up to six, each marked with a dot, a ring, a gray dot or a square. Events repeat by their rules (daily to yearly, days of the week, counts, end dates, left-out days, changed times), last whole days or by the hour, and are shown on the reader's clock whatever zone they were made in. Five home widgets (*Today*, *Next event*, *Agenda*, *Month*, *Birthdays*) and a sleep screen with today's agenda (*Sleep screen* in the settings). Calendars are read when the calendar is opened, on *Sync now*, and as set when the network is on anyway or when charging begins: never a radio switched on for it. Only the sixty days either side of today are kept, at most 240 events, in a file that is written whole or not at all; the reader only reads calendars, it never changes them. Passwords are kept with the reader's passwords.
- **`paperpaw-caldav`**: events read as they stream (zones, `EXDATE`, changed times), recurrence rules (`RRULE`), a small table of time zones with daylight saving, vCard birthdays and anniversaries, and the `REPORT` bodies for a range of days and for contacts.

- File sync: keeps folders of the card in sync with your own WebDAV server (Nextcloud, ownCloud, a NAS and the like), in both directions or from the server only (a store plugin, #134). Three pairs of folders, each with its own server, user and password (kept among the passwords and keys). A file changed on one side goes to the other; changed on both, both are kept (the card's copy as `name (conflict device date).ext`); deleted on one side and unchanged on the other, it is deleted on the other too (the card's copy goes to a trash of the last 100), else it stays; many deletions at once are held back. Files are written whole or not at all and a cut or a lost network is picked up where it stopped. Syncs on request (Tools, or the widget *Sync now*), with the charger or where Wi-Fi is up anyway; shows progress and a summary, and has the widgets *Last sync* and *Pending changes*.

- Smart home (store plugin, interface 7, #135): values and switches from your own Home Assistant. A dashboard under Tools (temperatures, humidity, energy, presence, doors and windows; lights, switches and scenes switch with the server's answer shown), an entity picker with a search over the server's list, and four home widgets (single value, group, switch, room card). The server's address and the long-lived token are set on one screen; the token is kept among the passwords and keys. Only addresses of the home network are used (plain http there, https elsewhere once allowed). The list of states is saved to the card and read in pieces, so only the chosen entities are held in memory. Fetched when the dashboard opens, on request, at a wake only where Wi-Fi is already up and on a schedule while charging; never by a radio of its own. Works with Home Assistant; nothing else is needed.

- **Tasks: to-do lists, on the reader or synced with your CalDAV server** (#136). A new store plugin: lists of tasks with a due date and a priority, added and changed with the keyboard and ticked off with Select. *Today and overdue* shows what is due across all lists. Four home widgets (*Tasks today*, *Task list*, *Open tasks*, *Next task*); a tap on a task in a widget ticks it off. With a *CalDAV account* (server address, user name, password kept with the reader's passwords) every task list of the server (Nextcloud, Radicale, Baïkal, Fastmail …) is a list on the reader: it syncs when Tasks is opened, on *Sync now* and when the network is on anyway, never on a key press, and what is changed without a network waits and is sent later. A task changed on the reader and elsewhere ends as the server's version with yours kept as a new task beside it; notes, alarms and everything else a task has on the server are kept as they are. The tasks file is written whole or not at all, so a power cut loses nothing. The reader holds up to 200 tasks in 12 lists.
- **`paperpaw-caldav`**: a small crate with the CalDAV and iCalendar code (content lines, to-dos read, written and patched as they stream, multistatus answers, addresses, ETags), for Tasks and the calendar plugin to come (#133).

- Puzzles (store plugin, interface 7): sudoku with puzzles made on the device (exactly one solution, three levels, pencil marks, check, undo, kept between starts, button-only input), crosswords from `.puz` and `ipuz` files in the card's folder `puzzles` (the clue list, typing a word on the keyboard, check, reveal), a small set of chess puzzles (Lichess puzzle database, CC0: pick a piece and its target, hint, next), statistics, and a *Puzzle of the day* (a sudoku, the same all day) with a home widget. A cursor move redraws only the squares that change (#137).

- Picture frame: your own photos as the sleep screen (a new one at each sleep or each day), as a slideshow while plugged in and as the Home widget "Photo of the day", from a folder of the card or from your own WebDAV photo server (a store plugin, #138). Photos are made for e-ink with a contrast setting and fit or crop, decoded by the host in strips; the server's photos are cached on the card within a size cap and fetched only on request, with the charger, or where Wi-Fi is up anyway.

- **Choose what the screen shows when the device is switched off** (#152). The new slot `core.off` holds the off screen, filled by default with the sleep plugin's screen as before. It is drawn once, with a full refresh, then the power goes: for the power menu (reason `asked`) and now also when the time of "Switch off after sleeping" passes on the X4 and X3 (reason `timer`, passed as the parameter `off-reason`); a low or critical battery still cuts the power without drawing. "Show when switched off" in the Sleep screen settings chooses as the sleep screen, the logo and "Switched off", the cat, the cover or your pictures, or a screen another plugin offers; a plugin can also take the slot over, as for `core.sleep`. Interface 7 is unchanged: only new constants (`slots::OFF`, `params::OFF_REASON`, `OFF_ASKED`, `OFF_TIMER`) and a kernel call for shells (`Kernel::switch_off_screen`).

- Bengali (Bangla) and Devanagari (Hindi, Marathi, Nepali, Sanskrit) books read correctly: an Indic shaper in the text family joins consonants into conjuncts, draws reph, half forms and ra-, ya- and ba-phala, and puts vowel signs drawn left (ি ে ৈ, ि) and the split vowels ো ৌ where they belong, a word at a time, with the substitutions and mark anchors of the font (#157). New letters packs `script-bengali` and `script-devanagari` (Noto Serif Bengali and Devanagari) carry the fonts' forms and their tables (Bengali 8.4 KiB, Devanagari 15.9 KiB in a compact version 2 of the table with 2-byte keys and offsets, kept in memory once while text needs them; Devanagari's i is drawn in its usual length), one pack file per weight that extends several fonts (a comma list in `[extends]`, data formats 5), and new language packs `language-bn` and `language-hi` (machine drafts) show the menus in Bengali and Hindi. Line breaking measures a word as wide as it is drawn. A pack's substitution table gains contextual lookups (kinds 4 and 5) without a new version; packs and layouts made before are read as they were. New in `paperpaw-text`: `has_indic` and `shaped_at` (the part of a text drawn as one, for line breaking); interface 7 draft 36 and its fingerprint stay.

- **Amharic books: the letters pack `script-ethiopic`** (#159). Books in Amharic, Tigrinya and the other languages written in Ethiopic letters show on the X4 family with the new store pack *Ethiopic letters*: the syllabary with its punctuation and numerals (U+1200–137F) and the Ethiopic Supplement, Extended and Extended-A blocks, 495 characters in Noto Serif Ethiopic (SIL OFL), seven sizes, a regular and a bold pack that Literata's faces and the interface's font share (2.5 MiB on the card). The reader offers the pack for a book whose text or language is Ethiopic, as it does for Greek or Korean. Lines end after the wordspace ፡ and at spaces, and the full stop ። stays with the word before it (Unicode's line breaking, which decided there already); Ethiopic words are not hyphenated. The letters are not kerned on the X4, as with every pack read from the card.

- Added the Thai UI language pack `language-th` (ไทย; a machine translation, marked as a draft) and the letters pack `script-thai` it needs (Noto Serif Thai and Noto Sans Thai, SIL OFL 1.1) for the store. Vowels and tone marks above and below a letter, also a tone mark over an upper vowel, are placed by the font's anchors (`paperpaw-text` reads Thai's `mark` and `mkmk` lookups as it does Arabic's and Hebrew's). Thai has no spaces, so the pack's texts hold a zero width space after each word, where lines break (`tools/thai-segment.py`): no word list in the firmware (#190).

- **The plugin manager says when a plugin's code was copied to the card** (#244, Decision 205). Such a plugin runs without a question; its note now tells that it was never installed and runs unchecked, and points to the store.

- **Community plugin sources in the plugin store** (#214). *Sources*, the
  last row of the store's start, adds plugin lists of others by their
  address (`https://…/plugins.json`, in the format of PaperPaw's library),
  lists one in place of PaperPaw's and removes it again; PaperPaw's own
  library is always there. Addresses can also be put into
  `.paperpaw/plugins/store/sources.txt` on the card, one on each line. The
  store says before a source is added that PaperPaw does not check it. A
  source signs its list (`plugins.json.sig`: its Ed25519 key and the
  signature over `PaperPaw plugin source\0` and the list's SHA-256, as
  hexadecimal digits); the device keeps the key of the first list it reads
  and from then on refuses a list of that source signed with another key
  or with none, and a code plugin not signed with it. A plugin is updated
  only from the source it was installed from, a plugin's page names its
  source, and a source's list is read up to 512 KiB.

- **Choose the start and the restart screen** (#219). Two new slots: `core.restart`, drawn once as PaperPaw restarts (opened with `off-reason` = `restart`), and `core.start`, the start screen. Plugins do not run yet while the device starts, so the screen bound to `core.start` is drawn ahead as the device switches off or restarts, into `.paperpaw/start.pbm` on the card (a 1-bit PBM of the screen's size), which the X4 shows at its next start in place of "Starting PaperPaw". Both are unbound by default, and the device shows its own screens as before (the picture is removed). "Show when starting" and "Show when restarting" in the Sleep screen settings choose a screen another plugin offers (`setting_choice` on `sleep.start-show`, `sleep.restart-show`); a plugin can also take a slot over. A screen that does not open, or takes longer than 3 s, gives way to the device's own, and so does a missing, damaged or wrongly sized picture. The device restarts or switches off in any case. Interface 7 is unchanged: only new constants (`slots::START`, `slots::RESTART`, `params::OFF_RESTART`).

- **About is a native interface-7 plugin** (#228). The version, source, plugins built in, the interface and draft code plugins are built for, and the licences run on `paperpaw-ui-kit`'s item lists and the typed device reads; the interface-6 page and its app registration are gone. The licences and the plugins-built-in texts open in the reader only with draft 22. Other plugins' lines come through the plugin-defined stage `about.lines` and follow the licences row under a heading. The fingerprint row is replaced by the interface and draft. Rows that cannot work yet refuse as unsupported and never fake.

- **Actions know who ran them** (#228). `Event::ActionRun` says `background` when a rule or
  another plugin ran the action rather than the reader (interface 6 `Env::reader`), so wallabag
  and OPDS can fetch at once for a press and as declared work for a rule, as in Beta 3.
- **The store can name the code's expected digest** (#228). `command.install_author` carries the
  `sha256` that `device.md` 5.2 describes, for the host's install check (an install whose code
  differs ends with reason 18 once the install handler lands). Interface 7 draft 34.

- **A book's annotation file can be read without holding it** (#228).
  `annotations::try_rows` reads the annotations of a book a line at a time,
  each text, note, client key and XPointer through a window of its own, and
  `try_head` its version and generation; neither allocates beyond the
  file's name, and a shortage of memory is `data::Error::OutOfMemory`.

- **`command.apply_preset` works for native plugins** (#228). A preset sets the values it names, and every setting of its kind it leaves out returns to its default: `core.theme` covers the appearance group except the size and contrast of the interface, the logo and the frame of all widgets; `core.widget-look` covers the looks of the widgets there are and of all of them; any other kind covers only what its presets name. A value a preset names outside the set of its kind is left out. The batch is checked whole at submit (unknown preset `ERR_NOT_FOUND`, a foreign guarded setting `ERR_INVALID`, any flag `ERR_UNSUPPORTED` until the preview commands are wired), applied after the callback in submit order, a few settings at a time. Secret settings and grants are never changed by a preset.

- **Bluetooth 1.0.3 as an interface-7 native plugin** (#228). The page of
  bonded devices (switching a remote's or keyboard's keys, forgetting a
  device, giving a sensor to a plugin that uses Bluetooth), adding a device
  with a search and pairing, and *Remotes* (by hand, while reading or
  always) now run on the new plugin interface. The interface-6 plugin is
  gone. The status sign while a device is connected and the *how it works*
  text come back once the SDK and host can show them.

- Native plugins can declare library book fields (`book_field.spec`) and badges (`book_badge.spec`, interface 7): both are admitted into the declaration store (at most 8 fields and 4 badges per plugin, 64 fields and 16 badges on the device, one name once per plugin and kind), listed in load order and dropped with their plugin; the host can tell whether a field id is declared and whether it is kept in the catalog.

- **`native::library::Book::missing`** (#228). A plugin reading a book with library op 3 can tell
  that its file is gone from the card (`lib_book.flags` bit 8) without `uses.storage`, so home's
  recent shelf can leave such books out as Beta 3 did. Interface 7 draft 34.

- **The cable's native server** (#228, #900). A native plugin serves the USB cable with `net_serve` on port 0: the computer's lines reach it through the kernel's cable link, bodies land in its `.received/` folder, answers go back as lines. The queue is bounded, every cable allocation is fallible, an answer waits for room, and a server on the cable needs a device place when served again on a network port. The transfer plugin's cable tests (`plugins/transfer/tests/cable_native.rs`) now run on a hosted X4-like kernel instead of being ignored.

- Interface 7 host slot 42 `canvas.describe` (`docs/abi/screens-input.md` section 6.1): a native
  screen tells its parts (role, label, area, focused, on, disabled) to automation, reading aloud
  and tests, as a legacy screen does; the host also tells every native `text.draw` as text while
  it asks. Nothing is kept or allocated where nobody asks. SDK: `native::canvas::Canvas::describe_part`
  with `PartRole` and `PartFlags` (#243).
- `paperpaw_graphics::Canvas::try_describe`: tells a part within a pass's bounds of parts and label
  bytes and without aborting where memory runs out; slot 42 and the native text auto-record keep at
  most 128 parts and 4 KiB of labels per describe pass (#243).

- **A catalog line can be read without holding it** (#228).
  `catalog::try_view_at`, `try_offset_of`, `try_state` and `try_generation`
  read one book's path and numbers, count its tags and fields, find a
  book's line by its path, and read the catalog's state and generation.
  `catalog::try_text_at` reads a title, author, series, tag or field from
  a given byte on into the caller's buffer. All return
  `data::Error::OutOfMemory` instead of stopping the program when the
  device is short of memory.

- **A page of the catalog's query without an abort on a memory shortage** (#228).
  `catalog::try_query` and `try_query_in` find the same books as `query` and
  `query_in`, in the same order, and reserve the page they hold at once: a
  device short of memory gets `data::Error::OutOfMemory` for the page.

- **Slot 35 `ui.compose` is wired into the core frame** (#228). A plugin's `draw` can now draw a widget instance, status item or status line of another plugin into its own canvas: the handle is resolved in the device's `core.ui` table, the owner's `draw` runs on its own instance one level deeper (a child cannot compose again), and a failing child is told it was dropped while the caller's frame stays. A stack the frame cannot read hides everything below the guard. The X4 still answers `ERR_UNSUPPORTED` until its stack gate passes.

- **Native plugins hear the kernel's events** (#228). `core.tick` (3) and `started`, `resume`, `suspend`, `stopping`, `battery_measured` and `page_turned` (32 to 37) now reach the native plugins that subscribed to them. The kernel only queues them, in the same per-plugin queue as the setting events, so a plugin hears everything in one order; the pump tells them between callbacks, never inside one. Suspend and stopping are also told right after they are raised, from the kernel's top level, before the device sleeps or stops. Tick and resume coalesce into one pending bit each; the others are held within the event store's limits and, if they do not fit, dropped and logged. A full event store logs once per plugin until its queue is empty again; the log text is static, so writing it needs no memory.

- **Covers of DOCX and MOBI books, made in the background** (#228, #245). A book whose format still has to prepare it gets its cover from one background job at a time: the first `cover` of it keeps the document open, the preparing loop converts it a step a pass between inputs, and `event.library_changed` bit 6 tells the screens to ask again. Any other open, sleep and a pulled card end the job first; nothing holds the device awake. A book whose job failed answers "not found" and is not tried again until the next start. A marker file `.paperpaw/cover-job.txt` names the book being tried, so a format that crashes on one book cannot crash the device at every start: the book counts as failed at the next start (after a plain power loss too, for that run only).

- **The desktop shell and the emulator offer `core.document`, `core.library` and `core.files` to the interface-7 plugins they run** (#228, `--features native-linked`, Linux x86-64, off by default), with the signed features `uses.storage` and `uses.library`, over the card itself (a folder, or the files kept in memory). The library serves the catalog reads (its writes answer "not supported" until the production write half exists); a book the library's move job carries is in use for `core.files` and for `provide`. A library wired after `core.files`, even by an earlier wiring, shares that job. On simulated time no book counts as new.

- **The desktop and emulator wire `core.lookup` for linked plugins** (#228). `register` names `HostWiring::lookup()`: a linked plugin that needs `core.lookup` is admitted and its lookups reach the providers other linked plugins declared. Nothing else changes on the desktop; `native-linked` stays off by default.

- **The desktop shell and the emulator can run the first-party interface-7 plugins** (#228, `--features native-linked`, Linux x86-64). The feature registers `NATIVE_PLUGINS` with the desktop's `core.ui` and `core.device` services and drops the interface-6 form of each ported plugin; plugins that need services the desktop does not wire yet stay unregistered and are said on stderr. It is off by default until those services are wired.

- **The desktop shell and the emulator offer `core.net`, `core.sockets` and `core.secrets` to the interface-7 plugins they run** (#228, `--features native-linked`, Linux x86-64, off by default). HTTP goes through the emulator's client with the redirect rules of the network contract (`paperpaw_http::follow`: never down to plain `http`, no body or key headers to another origin, at most 10 hops); a wanted network brings the emulated Wi-Fi up. Sockets are plain TCP and UDP broadcast; TLS sockets use `paperpaw_serve::secure` (rustls with ring, the authorities PaperPaw ships or the pins a plugin gave, no mode that accepts what it is shown) and fail closed: a switch to TLS is refused where clear bytes could pass as secure. Secrets are sealed with AES-256-GCM under a key of the user's own.

- **The desktop emulator keeps the secrets of native plugins sealed** (#228, with `--features native-linked`). Values are sealed with AES-256-GCM under a random key of the user's own in `~/.config/paperpaw-emulator/device.key` (made atomically, 0600, never beside the card's files, refused when its folder or file is open to others or owned by another user, wiped from memory when dropped). Without a key, or when the card cannot be opened, `core.secrets` is not wired; a run on simulated time keeps its secrets in memory and leaves the card's file alone. Declared work (`core.work`) runs on the device's clock and radio.

- **The kernel has a device log for its own diagnostics** (#228). `KernelBuilder::device_log(sink)` takes one function that gets each line (at most 120 bytes, plugin ids and numbers only, never plugin text, paths or secrets); without it the lines are dropped and nothing is formatted. The host services log through `Frame::log`, which renders a line only when a sink exists; the lookup service logs its two lines (unmatched ready command, pending table full, each at most once a minute) there too. The desktop emulator prints the lines to its error stream; the X4 follows with the `log` crate.

- Diagnostics is a native interface-7 plugin only (1.0.9): the test screen and the guided
  device check run on `paperpaw-ui-kit` and the device reads; the report is kept in the
  plugin's data folder (`.paperpaw/plugins/diagnostics/device-check.txt`); the snapshot
  reports the panel's gray levels and colour (#228).

- Questions (interface 7, `core.ui` op 8) now open their dialog in the slot of their kind once the asking callback returned, are cancelled as `not_started` when that callback failed or no screen serves the slot, and their answers reach the asker as `event.answered` (82), one callback each.

- **Dictionary: the interface-7 lookup reads dictionaries** (#228). The StarDict provider finds the dictionaries of the folder through `core.files`, makes their indexes in the background (`core.work`) and answers lookups from them.

- **Dictionary: the interface-7 lookup panel** (#228). A native overlay screen fills the lookup slot, shows what the providers found for a word over the lower part of the screen and scrolls by button or tap.

- **`DirStorage::same_entry`** (#228): the desktop and Android directory storage says two paths are the same entry only where they are the same name (another case of it, where the file system ignores case) in the same folder, so a case-only rename is told from a move onto another file. Nothing is followed at the last name: a hard link, and a symlink and its target, are two entries. Paths it refuses and names that lead nowhere are `Some(false)`; a gone root is `None`; Windows answers `None` until that path has been run there.

- **A document's cache is made ready at its open, kept whole or not at all, and counted** (#228, `docs/abi/document.md` section 2.1 and 3). The format's root 6 is the folder `.paperpaw/cache/<book key>/<format plugin>/<conversion key>/`. Before a document uses it, a folder without the host's stamp `.complete` for exactly this conversion (the format plugin's id and version and the conversion key) is removed (a cut preparation, a new version of the format, other options), a book keeps at most two conversions and the older goes, never one an open document uses (all pinned: the open answers `ERR_BUSY`). The host syncs the card and writes the stamp when the format answers that the document is ready. A write into the cache is counted per book and refused with `ERR_NO_SPACE` over 64 MiB where the device has the X4's two document places (4 GiB elsewhere); a replaced, renamed-over or removed file stops counting.

- **`format.spec` and `reader.spec` are admitted** (#228, `docs/abi/document.md` section 1). A native plugin may declare up to 8 formats (32 on the device) and up to 4 reading apps (16 on the device); the declarations stay in the declaration store with their plugin. Within a plugin each extension is claimed once (`ERR_INVALID`), a reader's `screen` is one of the plugin's own screens, and a ninth format of a plugin or a thirty-third on the device is `ERR_FULL`, a fifth reading app of a plugin `ERR_INVALID` and a seventeenth on the device `ERR_FULL`. The document service reads them from there once its host is wired.

- **`event.document_closed` (108) reaches the opener** (#228, `docs/abi/document.md` 2). When the host ends a document on its own (the format stopped, the card went, the book was moved or removed, its place was needed), the document service keeps one record for the opener and the kernel's pump delivers it, once and never dropped: an open leaves room for the close of every live document, and waits (`ERR_BUSY`) while undelivered events fill it. A plugin registers for 108 like any event, and now also for `event.image_ready` (90) and `event.lookup_ready` (110), whose producers deliver only to subscribers. A document the opener closed itself, or one whose opener stopped, raises nothing.

- **A format's call is for its document: roots 3, 4 and 6 on the storage slots** (#228, `docs/abi/document.md` section 3, Roots). The document service enters the call's scope around every call to a format, so inside it `storage_size` and `storage_read_at` read the book as root 3 (a single-file format: the file as the path `""`; a `folder` or `siblings` format: its folder and below, never the kernel's `plugins` and `.paperpaw`) and the user's CSS file as root 4 (in the open only), both read only (`ERR_DENIED` for a change); root 6 is the document's cache with read, write, rename and remove, and keeps `.complete` and `.host/` for the host (`ERR_DENIED` to change, `ERR_NOT_FOUND` to read). Outside a format call these roots are `ERR_UNSUPPORTED`. The cache folder itself comes with the cache.

- **A book of the opener's own data folder (root 0) is held while its document is open** (#228, `docs/abi/document.md` section 2). Like a root-1 book it is named to the in-use table by its card path (`.paperpaw/plugins/<id>/…`), so a move or removal of the file waits until the document closes, a failed open gives it back, and a refused allocation holds nothing and makes no document.

- **The document service is served for native plugins** (#228, `docs/abi/document.md`). `core.document` opens, closes, lists and chooses among the declared formats (the user's choice comes before `priority`), calls the format through the stage dispatch, and holds a book of the card for a reading app until its document closes. Pictures, the document events, sniffing a file's head, root 5 and root-0 holds follow in later changes.

- **A format's pictures become the opener's image ids** (#228, `docs/abi/document.md` section 5). A `fmt.image` of a format's answer (a block, the cover, a larger picture) is checked against the document's cache folder (root 6: the path rules, the file's header against the width and height the format says) and given an image id of the opener, drawable with `canvas.draw_image` and `image.info`; a picture that fails the check is left out. The ids of a document end with it: by `close`, and with the next call of the opener when the host ended it (the format stopped, the card went, the place was needed). `HostWiring::document_colour` says whether the screen keeps colour, for `doc_image.flags` bit 0.

- **A book a format is still preparing is stepped by the kernel** (#228, `docs/abi/document.md` section 2.1). A format whose `open` answered `state` 1 gets op 3 `prepare` between inputs, one document and one call a pass (one book at a time on the X4, in open order). The answer is checked and recorded, the opener hears of a change with `event.document_changed` (one per document, the latest values), a finished preparation stamps the cache, and a failure (the format's own status, a malformed answer, `ERR_NO_SPACE` which also removes the partial cache) makes the document `failed`. The format runs as its own and is metered to itself.

- **The document service tells the host which book a reader holds, and gives a book to show its place** (#228). Every `open` calls the host's `reader_opened` before the format and every end (close, opener or format stopped, the host closing it, its place taken) calls `reader_closed` once, so the book is not freed under a reader. The host can end one document with reason 2 `the card went` or 3 `moved or removed`. Where the device-wide places are full, an `open` that is not a background build takes the first background document, else the oldest of an opener that shows no screen (`document.md` 2.1); its opener hears reason 4 `needed`, the format gets op 4, and later calls on its id are `ERR_CANCELLED`.

- **The reader report** (#228, interface 7 draft 37). `core.document` op 22 `report` (`service.doc_report`) lets the reading app tell the host where the user reads: position, progress, title, chapter, page counters and "finished". The host raises the book events from it (`event.book_opened`, `event.page_turned`, `event.book_finished`, `event.book_closed`), checks every field and stamps the time, path and reason itself, so no plugin raises them. The events gain the optional `chapter_left`, the pages left in the chapter, for the reading widget's "pages left in the chapter" line.

- **The reader reports its place with `Documents::report`** (#228). The reader tells the host where the user is (core.document op 22) and the host raises the book events; `ReadingView::chapter_left()` reads the pages left in the chapter from the page and book events (interface 7, draft 37).

- **`doc_open` root 5 opens the file a hand-over names** (#228, `docs/abi/document.md` 1). A reader screen whose hand-over carries `root` (0 or 2) and `path` may open that one file of the plugin that made it, read only; another path, no hand-over and `css_root` 5 are `ERR_DENIED`, a maker that is not installed any more or a missing file `ERR_NOT_FOUND`. The file is held under its card path while the document is open.

- **A file no format claims by extension is sniffed** (#228, `docs/abi/document.md` section 2). When no format claims a file's extension, `doc_open` and `format_for` ask the formats that declared `sniffs`, in priority order, with the file's first 512 bytes read through the card storage; the first one to say yes gets the file. A claimed extension never sniffs, and a file that cannot be read or that nobody wants stays unsupported.

- **`event.document_changed` (88) has its record** (#228): `document`, `state` and `progress_ppm`, sent to a document's opener while it is prepared, coalesced per document (`docs/abi/document.md`).

- **The host side of `command.firmware_dual_boot`** (#228). A request in the firmware queue sets `dual-boot` in `paperpaw.ini`'s `[firmware]` section: mode 1 (`off`) always asks (focus on No) and, on a yes, writes the file, removes `.paperpaw/boot.txt` and restarts; mode 0 (`auto`) writes without asking or restarting and counts the setup as declared until the next start. Reasons 16 (file unreadable or over 16 KiB, write failed), 19 (no second app partition) and 21 (an image waits for its restart) end a request that changes nothing. `dev_firmware.flags` tells bit 0 `shared` and bit 1 `ini_off`. The command stays `ERR_UNSUPPORTED` until the host opens it; `FirmwareHost` gets three defaulted methods (`dual_boot_capable`, `dual_boot_declared`, `restart_running`) the X4 shell answers.

- **The emulator serves for native plugins** (#228). A plugin that serves a page through `core.net` (the transfer page, the phone page of the flash cards) now listens on a real port of the computer, on the loopback address, where it was refused before; uploads are written to the card as they come, answers and files are sent back, and the plugin's own limits apply. CI can now test serving.

- **End of book is a native interface-7 plugin** (#228). The screen at the end of a book with the next book, the rating and the finished mark, the two switches, and the finished mark said again when the book closes now run on `paperpaw-ui-kit` and the library service. The interface-6 plugin is gone and the store build offers no file until native plugins are published. Reading the rating back and declaring its field and badge wait on the SDK; the screen's pictures wait on the host.

- **A native plugin can register for the answers of its own requests and for work** (#228). The host admits the registrations of `event.wifi_done` (93), `event.work_due` (103), `event.work_refused` (104) and `event.work_allowed` (105); until now a plugin that named one of them was refused at `make` with `Unsupported`. The work events are delivered as before; the host now delivers `event.wifi_done` too, once per request and only to the plugin that asked.

- **Native plugins get `event.tick` once a minute while the device is awake** (#228). The kernel sends one tick a minute to each plugin registered for it, never asks for an extra wake to do so (a tick goes out where the kernel is ticked anyway, and a device that slept for several minutes gets one when it wakes), and does not filter ticks by screen or widget. A plugin registered only for another event (`started`, `resume`, ...) gets no tick record.

- **Fallible JSON streaming** (#228). `paperpaw_json::stream::Parser::try_feed`,
  `try_finish` and `try_read_file` report refused memory as `OutOfMemory` instead of aborting,
  and never report a cut or partial event, so plugins reading catalogs on the X4 can stop
  cleanly. `feed`, `finish` and `read_file` report the same events as before.

- The books the document service has open count as paths in use: `HostServices::reader_book_opened(path)` and `reader_book_closed(path)` (counted per path), for the document service to call when a document opens and when it ends; its wiring registers the report once (`register_reader_book_reporting`), and `core.files` stays unwired until it has. The library's move job can be registered as a busy source of `core.files` (`register_library_moves`); a path either holds answers `ERR_BUSY`.

- **A format can list the folder of its book** (#228). Inside a call of a
  `folder` or `siblings` format, `core.files` `list` and `stat` now take
  root 3: the book's folder and everything below it, read only, with no
  `uses.storage` needed. A single-file format, or a call outside a format
  call, still gets "unsupported"; changing anything there is "denied".

- `HostWiring::files(storage)`: a shell wires `core.files` by giving the card's storage; core composes the rest. The host's own folders (`plugins`, `.paperpaw`, `.received`) are known by identity as well as spelling, folded as the FAT driver folds names (so `plugıns` and `pluginſ` are refused too); an 8.3 alias is refused (the check over-refuses a user's own tilde name until the storage can answer "same entry"). What the host holds in use answers `ERR_BUSY` through registered sources; the wiring is refused (`ERR_STATE`) until the library move, the network's files and the reader's open book each have one, so that "nothing is busy" is never a wrong answer. A move is the storage's rename.
- What `core.net` and `core.sockets` hold in use (a saving request's target and staged file) is the network source of those busy answers, registered when either is wired: a `move` or `remove_tree` of it answers `ERR_BUSY`.
- Core keeps the ids of the plugins linked into the firmware (`Kernel::register_linked` records one before the plugin is registered and takes it back if that fails), for `core.files` root 2 to treat a built-in plugin's install folder as empty.

- **Firmware commands, events and op 13 on the kernel's firmware table** (#228). `install_firmware`, `firmware_restart` and `check_file` (kind 1) are checked whole at submit (feature, one a batch, flags, key, root and path, the restart's gates) and a request is accepted in the device's table or the batch is refused with nothing kept; a batch that never applies gives its requests back. `KernelBuilder::firmware_host` supplies the device's firmware (without it the commands are unsupported), the pump runs one bounded step per call and tells the plugin `event.firmware_changed` (96) and, once per request and never dropped, `event.install_done` (97); both can be registered for. Op 13 reads a request's state and the outcome kept for it. The host's confirmation screen follows.

- **The firmware confirmation** (#228, device.md 5.0/5.3). An install or a restart into a new firmware is asked in the host's own words (en/de) on a kernel-owned screen no plugin draws, covers or answers: the asking plugin, the image's project, version and signer (a PaperPaw key by name, another key by fingerprint), "older" where allowed, and for a foreign image the four points. Focus starts on No, input within 500 ms of showing is dropped, Back or ten minutes dismiss, sleep dismisses, and it waits while a guard or the lock is up. After a yes to the restart the plugins hear that PaperPaw stops, what they saved is written and the device restarts; a restart that cannot start ends the request with its reason.
- A host screen that guards is now the input target: no plugin filter, key binding or raised input reaches it.

- **The kernel's firmware request table, queue and pump** (#228). Firmware installs and checks wait in a device-wide queue of at most four requests (running one included) that is accepted whole or refused with nothing changed (`ERR_FULL`, `ERR_INVALID`, `ERR_NO_MEMORY`), run one bounded device step at a time, keep their outcomes for `install_done` and op 13, and end with reason 20 when the host stops them. The firmware commands, the events and the confirmation screen follow.

- **Dual boot for native plugins** (#228): `dev_firmware.flags` says whether the setup is declared dual-boot (bit 0) and whether `paperpaw.ini` says `dual-boot = off` (bit 1). `command.firmware_dual_boot` (1595) turns the check off, always confirmed by the host and followed by a restart, or back to `auto`, which takes effect at the next start (`docs/abi/device.md` 5.3), for the update plugin's interface-6 dual-boot notice and button.

- **`FirmwareHost`, the platform's side of checking, installing and restarting into a firmware image** (#228). One bounded step per call (a check that reads the image in pieces and streams the plugins it has built in, a write of one 64 KiB block at a time bound to the confirmed digest, a read-back, a commit, a restart), with reasons by the numbers of `docs/abi/device.md` 5.5; `FakeFirmware` scripts it for tests and the emulator, with a failure at any call of any stage. The X4's implementation and the kernel's use of it follow.

- **The X4's side of firmware check, install and restart** (#228). `paperpaw-x4`'s `Updates` implements the platform's `FirmwareHost`: a check that streams the plugin ids an image has built in, hashes and verifies it (the signature, or another firmware's image as the boot loader would), a write bound to the digest the user confirmed (reason 18 where the image changed), a read-back, and a commit that starts a PaperPaw image on trial and makes another firmware's final. `paperpaw-update` gets `PluginCrate::plugin_id`. Interface 6's flow is as it was.

- **A script for the X4 flash gate** (#228). `tools/flash-gate.sh` builds the
  Beta 3 firmware and a given revision with the same profile and compares what
  the chip's flash holds, against the gate of 60,000 bytes.

- **Fonts browser on interface 7: progress, going online, linked** (#228). A font being downloaded says how far it is (`2 / 5`) in the list and on its page, the pages follow a font that was installed, removed or chosen, an empty list that says why (no connection, not reachable) tries again when tapped or confirmed, and in Offline mode the list and a download ask once whether to go online. The device with buttons gets its own wording of the hints. The fonts of paperpaw.org's library whose packs are over 512 KiB are offered again (a pack may be 4 MiB, a font file 8 MiB, as interface 6 set no cap). Builds that link the store's plugins take its settings from the program's tables.

- **Fonts browser: downloading and removing fonts on interface 7** (#228). The font's page offers Download, Update, Stop and Remove again: the files are downloaded one by one, checked by size and SHA-1, and the host is asked to install or remove the font.

- **format-epub answers `core.format@1` for EPUB books** (#228). `epub_provider()` serves EPUB on the shared provider (sniffed by its `mimetype` entry), with a parity test against the interface-6 reader across chapters: blocks, header fields, links and contents. The parser is unchanged. Embedded fonts, ruby kinds and inline languages follow with draft 29; the allocation-refusal case is ignored until the shared parser is fallible (Core todo 208). Nothing is active until the host calls providers.

- **format-epub answers `core.format@1` for FictionBook and comic book files** (#228). `fb2_provider()` and `cbz_provider()` serve FictionBooks (also zipped) and comic archives on the shared provider, with parity tests against the interface-6 readers (blocks, links, contents, pages, cover, picture). The parsers are unchanged; footnotes stay links of the note kind. The allocation-refusal acceptance cases are ignored until the shared parser is fallible (Core todo 208). Nothing is active until the host calls providers.

- **format-epub sends embedded fonts, ruby kinds and inline languages through `core.format@1`** (#228). Embedded faces are copied in chunks to the document cache (`fonts/<i>`, through a temporary name) and listed with their size; ruby, phonetic and gloss notes keep their kind; a block's inline languages go out as `fmt.inline_lang` rows that its runs name by index. Sniff tests cover a wrong-case, a deflated, a not-first and a too-short `mimetype` head. Nothing is active until the host calls providers.

- **format-epub answers `core.format@1` for Markdown and HTML files** (#228). A shared provider serves any of the crate's document parsers (first Markdown and HTML) in the interface-7 way: blocks come in fragments, with text, runs, source maps, language, direction and writing mode, and bad calls get their refusals. The converter itself is unchanged; footnote and ruby kinds, inline languages and embedded fonts follow with draft 29. Nothing is active until the host calls providers.

- A native plugin declares a file format with `Declarations::format` and
  `FormatDecl` (name, title, extensions, priority, token and the
  `prepared`, `sniffs`, `folder`, `whole_pages` and `siblings` flags) (#228).

- **Command stage for native screens** (#243, H1). The input and event callbacks of a native
  screen now get a command stage in production: what they submit (open, close, back_to, notify,
  settings, item states, labels, widget changes) waits in a bounded screen queue (4 batches) and
  the kernel applies it between callbacks with the same code the tests use. A callback that fails
  applies nothing. `command.notify` is staged (bit 0 `announce`, other bits `ERR_INVALID`); an
  open that names a hand-over stays refused until the hand-over store is wired.

- **The host knows which file a reader's hand-over names** (#228). A reader opened with a hand-over that carries `root` (`0` or `2`) and `path` can be asked, by the host, for that file and for the plugin that made the hand-over, which stays known after the maker stops. The document service uses it for `doc_open` root 5; until it is wired, that root still answers unsupported.

- A native plugin's `help.hint` (a hint for any screen, own or another's) and
  `input.spec` (an input or gesture of its own) records are admitted at
  install and kept in the declaration store, removed with their owner. A hint
  names a screen by full id, the inputs it is for (any, touch, buttons), an
  optional zone that lies on the screen (thousandths, a sum that overflows is
  refused) and the catalog key of its text; a plugin may not repeat a hint
  (same screen, input and text) or an input name. `core.registry` op 14
  (`inputs`) lists the declared inputs and gestures the device meets, after
  the device's own.

- **Decoding hex without an abort** (#228). `paperpaw_digest::hex` gains
  `decode_into` (into the caller's buffer, no allocation), `decode_array`
  (exactly `N` bytes on the stack) and `try_decode` (a vector reserved
  fallibly, so a full heap gives an error). Keys, signatures and digests in
  the SDK, Store and Transfer, and the cable's data lines, use them; a data
  line too long is refused before anything is reserved.

- **Native plugins can subscribe to the hint events 70-74** (#228): `plugins_changed`, `wifi_changed`, `power_changed`, `status_changed` and `charge_changed`. A hint is one pending bit in the plugin's queue, so several raised before it is told are one event, and its record is built from the state at the time it is told. They are told between callbacks with the setting changes, a few to a plugin in a pass.

- A native screen can be declared with its row of button hints hidden (`screen.spec.flags` bit 8 `hints_hidden`, `ScreenDecl::hints_hidden()`), as a code plugin did with `set_screen_hints(.., Hidden)`: the sleep screen shows no hints. Declared only; `command.screen_policy` cannot change it.

- **Home screen on the new plugin interface: the Add gallery** (#228). Adding a widget shows the
  widgets of a kind as they would look, a page at a time, each under its name; keys and taps pick
  one. A widget that cannot be shown that way appears as a row (icon, name, size), and where the
  device cannot show widgets inside other screens yet, as the list of names.

- **Home is complete on interface 7** (#228, #243). The status line above the grid (`home.status-bar`) is drawn in its strip and tapped for an item's action, and the nine first-time help hints of the dashboard are declared natively. PORTING-7 now names only what is the host's (screens and composition on a real host, `event.widget_changed` for the declare SDK, the scripted host's settings store).

- **Home screen on the new plugin interface: arranging with a finger** (#228). While you edit the
  home screen, put a finger on an item and drag it to move it, or drag its corner to size it; a
  frame shows where it will land. Letting go on the same spot opens the item's options as before.

- **Moving status items with a finger** (#228). In the editor of a status line (the footer items of
  the home screen; picture-home uses the same view), the shown items carry a grip: put a finger on
  it, take the item along to its new place (a bar shows where it lands) and lift the finger. The
  hint under the list names the grip.

- **Home screen on the new plugin interface: Settings in an item's options** (#228). The options of
  a widget you are editing offer *Settings* again, where the widget has settings of its own: each
  placement gets settings of its own, which open in the settings.

- **Home screen on the new plugin interface: pages follow the grid** (#228). When you change the
  columns or rows of the home grid, the pages are fitted to it as before: what has no room moves
  to the next page, a page is added where there is none, and nothing is lost.

- **Home screen on the new plugin interface, the continue card and the recent shelf** (#228).
  The card (the book you read last, its cover, title and how far you are) and the shelf of the
  books before it now run as native widgets of the home screen, with the same settings and
  looks. A tap opens the book in your reader. The covers come from the library's cover cache.

- **Home screen on the new plugin interface, the status line** (#228). The status line widget
  (the items you chose for its left, centre and right, from any plugin, or a whole line another
  plugin draws) now runs natively, with the same settings and the same dropping of the least
  important items when the width is short. Items their plugin draws are composed into the line.

- **Core host side of `core.files@1`** (#228). Folders, file information,
  moves and free space (`docs/abi/files.md`): `list`, `stat`, `space`,
  `make_dir`, `remove_tree` and `move` over the callback's storage view, with
  root 0/1/2 rules, paging by a 64-bit cursor, the `ERR_BUSY` rules against
  the library's move job and the host's paths in use, and a family that stays
  `ERR_UNSUPPORTED` until a shell wires its host view. `paperpaw_platform::tree`
  adds `remove_tree_step`, a folder tree removed a bounded piece at a time (at
  most 64 removals and 256 listed entries per call, every visit counted) over
  any `Storage`, without heap, and `kind`; `Storage::remove_listed` removes an
  entry by its place for names no path can hold (FAT overrides it).

- Choice and confirm questions (interface 7, `core.ui` op 8) are answered by the host's own dialogs when no dialog plugin serves the slot: the answer goes through the same store path as a dialog plugin's, *Back* or a closed dialog cancels. Text and folder questions still need a dialog plugin.

- **Interface 7: a host profile for every shell** (#243, owner decision 172). The interface declares five more profiles for linked built-in plugins: Windows x86-64, macOS x86-64 and Apple silicon, Android AArch64 and the web (wasm32). A linked plugin's metadata now names the profile of the target its program is built for (`paperpaw_native::profile::BUILD_PROFILE`), set by the SDK at compile time, so the emulator on Windows and macOS, the Android app and the web emulator can host the built-in native plugins. No card or store file loads on those targets.

- **Plugins can declare setup steps** (#228). A native plugin's `setup.step` records are admitted instead of refused as unsupported. A step's name must be its own: a plugin whose step is named like one of its screens, actions, settings or menu sections is refused, so one state push never sets two things. Steps go away with their plugin. The setup list and a step's help text (registry operations 15 and 22) follow when their contract is merged.

- **Plugins can declare stages and subscribe to them** (#228). A native plugin's `stage.spec` and `registration.stage` records are admitted instead of refused as unsupported. A plugin names each stage once, subscribes a token to a stage once, and has at most eight subscriptions (a ninth is refused as full); a subscription to its own stage is refused. A subscription to a stage nobody has installed is kept and stays inactive. Stages and subscriptions go away with their plugin.

- **`HostWiring::library` wires `core.library` for a shell** (#228): the reads of the library's catalog on the card (state, book, text, key, position, annotations, anchors) with the facts the card cannot tell (`CatalogEnv`: whether the walk is making the catalog, today's date, a file's format). Every write op still answers `ERR_UNSUPPORTED` until the production write half exists, and a name below the card's top is one of the host's folders by spelling or, for an 8.3 alias, exactly as `core.files` decides it; where `core.files` is wired too, a book the library's move job carries answers `provide` with `ERR_BUSY`.

- **A shell can wire `core.secrets` and `core.work`** (#228). `HostWiring::secrets(SecretsInputs)` takes the storage of the secrets file, the shell's `Sealer` (a shell without a key does not wire `core.secrets`: `keep` through a sealer that does not seal answers `ERR_STATE` and writes nothing, decision 140) and an optional random source, and gives back `WiredHosts::secrets()` for the start-up upgrade and for forgetting a deleted plugin's secrets. `HostWiring::work(WorkInputs)` takes the shell's clock and radio (`WorkDevice`); core composes the rest of the scheduler's view of the device from the kernel (power level, stretch, real-time screens), the owner directory and the network policy. Both are fallible and refused twice (`ERR_STATE`).

- `core.net`: the shell can report a failed certificate or TLS handshake as `HttpReason::Tls`; the plugin gets `event.http_done` with reason 3 (`tls`, `network.md` 4.1).

- **Named icons in the native SDK** (#228). `IconGlyph::named(host, name)` wraps host
  slot 40 `icon.glyph`: the character of an icon name in the bound icon font, drawn
  with `text.draw` through a `role:icons` font. Malformed names are refused before
  the host is asked, and `NotFound` means the plugin shows text instead.

- **A plugin's own pictures now run** (#228). `command.image_prepare` and `command.image_release` are admitted in the command stage (root 1 needs `uses.storage`, at most the plugin's room) and applied after the callback; the kernel decodes **one request a turn** between callbacks and tells the plugin `event.image_ready` (never inside a callback). The cache folder is swept once before the first decode. A request the table cannot take for now is kept (at most 16) and applied at a later pass. A held-back command is tried again after 2 s (three tries, never an immediate pass) and then fails to its plugin with `event.image_ready` status 5.

- **Image services for interface 7** (#228). The host now serves
  `image.info`, `canvas.draw_image`, `image_prepare` and
  `text.hyphenate`: pictures are drawn in bands, a failed read or a short
  cache file is reported instead of drawing half a picture, and
  prepared images are decoded one request per main-loop turn.

- **`image_prepare` reads the plugin's install folder, and a missing file is status 6** (#228). Root 2 (the plugin's own install folder, read only, no feature) is accepted and resolves like `core.files` does (`plugins/<plugin>/<path>`); a built-in plugin's folder is empty, so its paths are not found. `event.image_ready.status` 6 (`not found`) now tells a missing source file on every root, where a missing file was status 4 (card failure) before; other card failures stay 4.

- **Native plugins can filter input** (#228). Plugins that subscribe to the stage `core.input-filter` now see every key and touch before screens, gestures and key maps, and answer pass, take, run, cancel or replace (ui.md 4.1). Power is never filtered, and a filter that fails or answers badly passes the input. While a screen guards the device or a secret is typed, only the filters of the guard's or the screen's plugin run. A filter can only turn a key into a button the device or platform knows; any other name leaves the original key as it was. The interface-6 filters keep running after the native ones. A plugin that brings a callback depth counter other than the device's is refused at installation. A display of no width or height no longer panics when a filter replaces a pointer.

- **A native input filter can raise inputs and make gestures** (#228). A filter that answers `raise` with an input or gesture its plugin declared (`input.spec`) makes the layers shown run what they bind to it, as if it came from the device; a name the plugin did not declare leaves the key as it was. A filter that replaces an input with a gesture (`input.gesture`: tap, long press or a swipe) gives the screen on top that gesture at a point on the display.

- **Native input filters see the gestures the kernel recognizes** (#228). A tap, long press or swipe goes to the filters of `core.input-filter` as an `input.gesture` record before the screens: a filter may pass it, replace it by another gesture, take or cancel it, run an action, or raise an input of its plugin's. A gesture a filter made, and an input a filter raised, do not go through the filters again. A filter whose call fails at three inputs in a row is left out (and logged) until the declarations change, instead of being called and the list read again at every key.

- **A plugin's right to install without asking is the user's setting** (#228). The host reads `core.grant.install.<plugin>` (off by default) just before it steps a plugin's install, so a grant given or revoked counts from the next step on. A plugin cannot write the setting, reads only its own, and a plugin that is not running holds no grant.

- **`command.set_item_state` (command 60) is stored for native plugins** (#228). The host keeps the last pushed state per plugin and item (flags, label and badge keys with their arguments), within the contract's limits and with fallible allocation, and applies a batch after the callback; a default state removes the push, an update or unload of the plugin clears it. `core.registry` menu rows (op 12) and setup steps (op 15) show the pushed state: hidden, faint, switch, and the label and badge in the user's language. The note in `registry.md` fixes the order of op 13 with an empty scope: layers by id, inputs by name.
  Memory (X4): item states are charged at what the heap holds (a 24-byte entry, its text allocation's header and the plugin's slot) within the contract's 6 KiB. Room reserved at submit and not yet used adds at most about 0.8 KiB (16 entries for each of the two callbacks a nesting allows) plus a plugin slot each (up to 80 bytes); it goes back when the callback ends.

- A native plugin's `key.bind` records are admitted at install and bound in the
  key map (the keyboard's held Back ends it): on a screen's layer or `global`,
  optionally while a setting has a value (a setting nobody declares keeps the
  binding off, as in interface 6), built before the device's and the
  user's bindings so theirs win, and removed with the plugin. A binding the
  key map cannot hold (an input that is no input, a zone of its own) refuses the
  plugin whole with `ERR_INVALID`.

- **Keyboard on interface 7: layouts other plugins add** (#228). A plugin that brings a keyboard layout subscribes to the new stage `keyboard.layouts` and answers with its layouts (name, code, letter rows, variants), by the same rules as before; the keyboard and its layout chooser offer them.

- **Keyboard on interface 7: typed text and drawable layouts** (#228). The keyboard takes text from a hardware keyboard or a remote control again (it is added to the field; *done* confirms), and leaves out the layouts, and the variants of added layouts, that the fonts cannot draw.

- **Annotations in the native library SDK** (#228). `native::library::Library`
  reads a book's annotations in borrowed pages with their own cursor and counts,
  and reads the complete text or note of one annotation in bounded UTF-8
  chunks, so nothing is cut short.

- **Positions through `core.library` on the card** (#228). Op 14 `set_position` writes the book's position file, the SDK's own `[position]` form (so an interface 6 reader reads it), atomically, with the book's fingerprint kept; the host adds `insert` (when not 0) and `sync = 1` (a sync write that the user did not confirm), and op 13 `position` now reads both back, `from_sync` included.

- **A book's file that is gone from the card is told in the library's book
  answer** (#228). `core.library` op 3 sets flag bit 8 `missing` after one
  lookup of the file, so a plugin needs no storage access of its own to tell
  a gone book; a card that fails is an error, never "missing".

- **`core.library` `cover` (op 7) in the production library host** (#228, #245). The first call for a book opens it through `core.document` as the caller (root 1, no `uses.storage` needed), asks the format for its cover (DOC op 15) and closes the document again; the picture is the caller's own image id, kept after the document ends, and the same book's cover has one id. Later calls find the picture in the book's document cache and decode nothing. Nothing is held between calls; a full document table or a format not running yet answer `ERR_BUSY` and raise `event.library_changed` bit 6 so the caller asks again, a book without a cover or whose format is still preparing (DOCX, MOBI before their first read) `ERR_NOT_FOUND`.

- The native SDK reads library facets (authors, series, tags, first letters,
  folders) through `Library::facets` and receives the library-changed hint as
  the typed `Event::LibraryChanged` and `EventKind::LibraryChanged` (#228).

- **Library reads can be refused for lack of memory instead of aborting** (#228).
  `library::try_book`, `try_book_sized`, `try_status`, `try_key`, `key_into`,
  `catalog::try_changes`, `try_count`, `try_path_at` and `Format::try_load`
  return `data::Error::OutOfMemory` when the device is short of memory.
  `library::book` and `library::key` keep working as before (a shortage
  still stops the program, never answers "no such book").

- **Whether a book was read for its title and cover can be asked without
  reading its details** (#228). `library::try_indexed` scans the details
  file a line at a time and holds none of it, and returns
  `data::Error::OutOfMemory` instead of stopping the program when the
  device is short of memory.

- The library's journal holds the two other steps that change the user's files, a delete (up to 7 books, one entry) and an import (`library.md` 6): the entry is on the card, synced, before any file changes; the files go or arrive at once; the catalog's part, what plugins keep, the document caches and the events follow in bounded steps, and a start after a power cut finishes or drops the job by what the card shows. One journal entry at a time: a move, a delete and an import answer busy while another is open. The catalog's own parts (LIB ops 10 and 12) follow with the library service.

- The library follows a move of the user's files: the catalog carry (every book's details, place in the lists and plugins' book data to its new key, folder reader choices, folder settings, the book-moved events) runs as bounded steps of the main loop over the kernel's `Env`, with the one move job and its busy source made when `core.files` is wired. A root-1 `move` of `core.files` is that job. Third slice of the production library moves; the kernel's service pump still has to call `pump_library_moves`. The walk over a moved folder is resumable: a step costs what its (at most 16) books cost, not the whole folder.

- The card's side of the library's moves: `MoveHost` over the shell's storage with the journal, the shell's identity of entries (`EntryIdentity`, a host-only seam: the shared `Storage` trait is unchanged; where the shell gives none every move is refused as unsupported), `Storage::same_entry` (a storage that cannot tell refuses the move) and `Storage::rename`. What follows a move in the catalog is the `MoveCarry` the library host composes. Second slice of the production library moves.

- The library's move journal on the card (`.paperpaw/library/journal`): one checksummed record, written to a temporary file, synced and renamed over the journal, so a power cut leaves the old state or the new one; a record that cannot be read is an error and is never overwritten. First slice of the production library moves.

- **`core.library` op 24 `provide` is kept on the card** (#228). The catalog's host side keeps what a source (calibre, a catalog) provides for a book in the book's details, where the book's own file names nothing (the file is read anew, as in interface 6): title, author, series, number, and one list of tags that the last provider replaces. A failure of the memory or the card is the answer, never a silent OK. `paperpaw_sdk::library::try_provide` and `ProvidedText` (kernel only) are the fallible form of `library::provide` the adapter uses. The rest of the write half still answers `ERR_UNSUPPORTED` until the adapters are wired.

- **`core.library` op 2 `query` on the production host** (#228). The books a plugin lists (the home shelf, the recent books, the files list) are found on the card: show, words, author, series, tag, folder and day filters, the five orders (a tie is the path), descending, and paging by cursor under the catalog generation. The catalog is read a line at a time and nothing the size of the result is held, so the home screen no longer gets "unsupported" for its start-up reads. Not yet: the book badges and the archive setting's value are the shells' (`lib_entry.badges` is empty).

- **The library service reads the catalog on the card** (#228). `core.library` answers state, book, text, position, annotations, anchors and the book key over `paperpaw_sdk::library`'s fallible reads: a short heap is an error, never an abort, a book's change log is read once and its line in one scan, and a book's file is looked up for the `missing` flag only for the one book asked. Ops 2 (query, facets) and 22 (fields) still answer `ERR_UNSUPPORTED` in production for now; their host sides follow.

- **`core.library` ops 6 `next_in_series` and 22 `fields` on the production host** (#228). `next_in_series` answers the lowest unread book of the book's series (the given book when it is that one), one catalog scan that holds a number and an offset. `fields` lists a book's fields of every plugin by plugin id then name, paged under a field generation that changes with the fields; the title of a row is the field's name until the shell supplies the translated one (`CatalogEnv::field_title`).

- **Fallible library moves and removals** (#228). `library::try_moved` and `library::try_removed` carry along or forget what the library knows of a book (its details, its cover and its place among the recently opened and the favourites) and return the first card error instead of dropping it. What is already done counts as done, so a host step that fails can run again and finish.

- **The library's state names the progress of a first walk** (#228). While the
  catalog is made where there was none, the kernel reads how far the walk is
  (`Walk::progress_ppm`) for `core.library` op 1; its books are those found so
  far, and the share is only a rough bar. `catalog::is_growing` tells whether
  such a walk is under way.

- **`lookup.spec` is admitted** (#228, `docs/abi/document.md` section 7). A native plugin may declare up to 8 lookup providers (a ninth is `ERR_INVALID` for the batch) and the device holds 32 (the batch that would pass it is `ERR_FULL`); a name is unique within its plugin (`ERR_INVALID`), the same name in two plugins is allowed. The declarations stay in the declaration store with their plugin, in load order, for the lookup service once its host is wired. A provider's name is at most 64 bytes (what `lookup_entry.provider` holds), and a plugin refused at a lookup cap shows `reg_plugin.problem` 18 (too many lookup providers) with the cap (8 or 32) as the argument. A plugin's providers are called in the order it declared them.

- **`core.lookup` is served by the host** (#228, `docs/abi/document.md` section 7). The providers the plugins declared (`lookup.spec`) are read from the declarations, left out when they are the asker's own, need a network that is not connected or do not take the ask's language (an empty list or an empty ask takes all, otherwise the tag or its base, any case), and the one the cursor names is called in its own instance; its answer is the page, the host setting `more`. A provider without an entry, or one that fails or answers badly, is an empty page, so the providers after it are still asked. A shell wires it with `HostWiring::lookup()`. Providers that answer "pending" are not served by this host yet (no pending asks are kept). The cable link does not count as a network (it serves a computer, not the internet).

- **Pending lookup asks** (#228). The lookup service keeps, in a fixed
  table that allocates nothing, which asker waits for which provider's answer
  when a provider answers `pending`, and ends each ask on the provider's ready
  command, after 60 seconds, or when either plugin stops. The table is 16
  entries on the X4 and 32 elsewhere; a full table answers the empty entry.

- **Interface 7, lookup: answers that come later and entries that open more** (#244, dictionary parity). A lookup entry (`lookup_entry`, the stage reply `lookup.entry`) gains `opens`, the screen that shows more of it (a whole article; Beta 3's *Open Wikipedia ›*), which the asker opens with a hand-over of the word, the language and the entry's text. The SDK's `lookup::Entry` gains `pending()` for a provider that fetches and answers after `lookup_ready`, and `opens()`; `ProviderEntry::opens()` reads it. The SDK's docs no longer say that a provider that fetches is refused.

- **Asynchronous lookups in the native SDK** (#228). A lookup provider that
  answered `pending` calls `CommandSender::lookup_ready(text, language)` when
  its result is kept; an asker subscribed to `EventKind::LookupReady` gets
  `Event::LookupReady { now_ms, provider }` and asks again.

- **Mail on interface 7: writing and sending** (#228). The compose window (sender, To, Cc, Subject and message, each typed on the keyboard) keeps a draft on the card, replies start from the mail read (to the sender or to all), and Send checks the addresses and sends through the account's SMTP server.

- **Mail on interface 7: the connection to the servers and the account** (#228). The plugin connects to IMAP and SMTP servers over the host's sockets (TLS from the start or STARTTLS, a pinned key, the trust question's data), reads the account from the settings and the password from the secrets for each connection, and wipes what carried it. The screens that start a connection follow.

- **Mail on interface 7, first slice** (#228). Mail's three accounts and their settings (provider, address, password, servers, ports, security, pinned keys, how many mails to fetch, mark as read, signature) are declared to the host as interface 7 asks, with the password kept as a secret. The mail codecs (base64, quoted-printable, encoded words, character sets), the header, address, date and subject readers, and the credentials buffer now run without panics and report out-of-memory instead of aborting; bytes from a server are bounded, and the password buffer wipes itself, never grows and shows nothing in debug output. Reading and sending mail (`html`, `imap`, `smtp`) stay on the old interface until the sockets, secrets and background-work services are there.

- **Mail on interface 7, second slice** (#228). The reader that turns an HTML mail into text runs without panics and reports out-of-memory instead of aborting. It makes the same text as before, but a mail can no longer crash it with a non-ASCII character inside a style or script block, a long comment no longer hides the rest of the mail, and a mail's text is held to 24 KiB.

- **Mail on interface 7, third slice** (#228). The readers of IMAP responses and SMTP replies, and the builders of the login, address and data lines, run without panics and report out-of-memory instead of aborting. They give the same answers as before; a server's line is held to 8 KiB, the socket buffer to 16 KiB, a line to 256 tokens and a capability or flag list to 64 words, a fetch line that ends in a space no longer loses its items, and the login line with the password wipes itself from memory.

- **Mail on interface 7: the list screens** (#228). *Mail* shows the folders of every account with how many mails wait, and a folder shows its mails newest first; both check for mail from their first row over the host's sockets, say where the connection is and how it went, and ask once whether to trust a server whose certificate no authority vouches for. Reading and writing a mail follow.

- **Mail on interface 7: the reader** (#228). A mail opens from a folder's list with its sender, copies, date and attachments above the text, fetched the first time and kept on the card, scrolled by key, tap or swipe, with *Reply*, *Reply all* and *Star* below. Writing and replying follow.

- **Mail on interface 7, fourth slice** (#228). The reader of a message (headers, MIME parts, quoted-printable, base64, character sets, HTML, attachments) and what Mail keeps (the list of a folder, its counts, the folders, the messages read) are rewritten so that they give the same text and the same files as before, hold only a little memory at a time, and never abort when memory runs out. They are not yet used by the plugin that runs; the connection and the screens follow.

- **Mail on interface 7, fifth slice** (#228). The IMAP and SMTP conversations with a mail server (connect, secure with TLS or STARTTLS, log in, read the folders, the headers and a message, set a flag, send a message) are rewritten so that they say the same things to the server and leave the same files, never keep the password in a copy that is not wiped, close the connection on a server's answer that makes no sense, and tell the reader when memory runs out instead of stopping. They are not yet connected to the network by the plugin that runs; the connection and the screens follow.

- **Menu: the reader's bar on interface 7** (#228). The menu opened with `mode=bar` (Select or Menu on a book) shows at the top as in Beta 3: the book's title, its chapter and the page, then the entries as icons with short labels in even rows; the buttons move round them (mirrored right to left), a tap on the title runs the book's information, actions close the bar and run, switches and choices of two change in place, submenus open as a list over it.

- **Menus and icons from built-in plugins** (#228). A plugin can now declare menu entries, needs, sections, hides, descriptions and icons. They are checked when the plugin loads (a plugin's own items must exist), kept with the plugin and removed when it unloads.

- **Titles and authors of books in native formats, read in the background** (#228, #245). After a catalog walk, the host reads the title, first author, series and subjects of each book of a native format plugin whose details are still unread, one book at a time through its format plugin, without converting it (a DOCX's or MOBI's details come from what its format reads at open; a format that cannot answer them unconverted gives none), and keeps them as the book's details; the next walk lists the book with them. It runs only after the start, when no book is open for reading and no cover job runs (the two share one background slot, its crash marker `.paperpaw/cover-job.txt` and its rules); opening a book or sleep ends it, a cover the library needs takes its place (the book is read later), and nothing holds the device awake: it goes on by timed wakes at the cover job's pace and within its awake-time limit per book (one limit for both jobs). A book the format fails is skipped until the next start. The catalog is walked again once when the job reaches the end of it (and at most every two minutes while it runs), only if a book was read.
- **Library covers and details of DOCX and MOBI books without a conversion** (#245): the host's background job opens such a book with the host-only `details` flag; it is never converted for a cover or its details, its cache is neither stamped nor removed by that open, and any other open of the book is a fresh one that converts as usual. A plugin's open with the flag is refused.

- **Native plugins can bring fonts and hyphenation tables** (#228). A native plugin's `resource.font_face`, `resource.font_extension` and `resource.hyphenation` records are published with the rest of what it declares, all or nothing, and go with it when it is unloaded: a font id that is taken, a language named twice, or no heap for them refuses the whole plugin, never on a full heap, and nothing is left behind. The files are named relative to the plugin's folder, which the loader passes (`Kernel::register_native_in`); a plugin built into the firmware has none and is refused if it declares such records. A face with no family is bound to roles and never offered for text (an icon font). A face whose weight does not match its style bits is used with the bold bit alone and noted in the log.

- **A native plugin can declare settings, setting groups and actions** (#228). The kernel installs them with the plugin's screens in one step: if any record is refused, nothing is published. When the plugin unloads, its actions, settings and groups go too (the values the reader chose stay for its next load).

- Native plugins can now use the navigation commands of interface 7 on the kernel's own screen stack: `close` of an instance (own screens and other plugins' overlays), `back_to`, `set_key_layer`, `show_hints` and `raise_input`. A guard's rules (only its plugin closes or raises past it) and the 8-hop chain limit apply; the root screen is never closed. `screen_policy` stays unsupported.

- **A native plugin that uses the network gets its network settings** (#228). Every admitted native plugin with the feature `uses.network` gets `core.network.<id>` and `core.saving.<id>`, the same specs, ids and defaults as an interface-6 plugin, with the reader's kept values applied, and the network policy of `core.net` and `core.sockets` follows them. A setting that cannot be defined is a warning, never a refusal of the plugin.

- **A native plugin's `command.open` applies, in all four modes, and hands over** (#228). A native plugin opens a screen by id (mode 0), shows the screen bound to a slot (mode 1), replaces its own top screen (mode 2) or opens a screen for a slot (mode 3), as `docs/abi/ui.md` says. A hand-over the open names is checked and claimed when the plugin submits, belongs to the instance when the open applies (the instance's old one is freed, an open of none clears it) and is reported in `screen_opened` with its params and the length of its text, so the reader's selected text reaches the screen as host text and not as a param. A refused or stale open frees its claim, and a callback that fails frees what it claimed. The host's production wiring of `core.ui` (todo 119) is still to come.

- Native plugins have a real on/off state again, as in Beta 3: `Kernel::native_config` reads `[plugins] <id> = on|off` from `paperpaw.ini` once and `Kernel::native_decide` decides per plugin (off wins, then the shell's overrides, then the shipped default; safe mode holds plugins back as in Beta 3) before the shell hosts it, and `Kernel::list_native` lists every native plugin (running, off or failed) so the plugin manager shows it, saves a switch for the next start and drops it on removal. A plugin that is off costs no instance and no declarations.

- **Four more first-party plugins are linked native plugins** (#228, #243). With `native-linked` the list `paperpaw_app::NATIVE_PLUGINS` gains, with the new feature `native-linked-store` (which the emulator names), aids, mail, notes-vault and widgets; their interface-6 forms are not installed then. `native_default_on` is their shipped state and the kernel's `native_enabled` decides with the configuration (mail and notes-vault off until `[plugins] id = on`, an `off` wins, none where the store installs them as files); an off plugin is listed for the plugin manager and not hosted, and `paperpaw_app::native_version` gives the version of each. Off by default, as `native-linked` is. The formats (txt, xtc, docx, images, mobi), sleep, home, reader, dictionary, store, wifi, transfer, fonts-browser, vocabulary, news and picture-home stay interface 6 until the native reader exists and their ports are finished.

- Input to native screens (interface 7): while a screen guards the device, a key binding outside its own key layer runs only if it names a `core.*` action or the guard's own action; others are ignored and logged once.

- Native plugins can change a screen's policy at run time with `command.screen_policy`: `guards`, `secret` and `holds_edges` on their own screens. `guards` can be turned on only for their own top screen. The kernel applies the change before the callback's other commands, and it stays when the screen is made anew.

- **A native plugin's `slot.offer` reaches the slot registry** (#228). A screen a native plugin offers for a slot (`core.translate`, `core.lookup`, a slot of its own) is now bound like any other offer: `Env::has_slot` is true and `slot_screen`/`slot_screens` name it, so a reader that hides a feature until a plugin offers its slot shows it. The offers go with the plugin when it is unloaded, are admitted whole or not at all (no room refuses the plugin, never on a full heap), and where the heap has no room to update the list of bound slots the list stays and a warning says so.

- **Sockets in the native SDK** (#228). `native::sockets` wraps `core.sockets`
  major 1: `Sockets::{open, release, connect, send, read_bytes, read_file,
  data, start_tls, peer, close, broadcast}`, the `Connect`, `Tls`, `Pin`, `FileRead` and
  `Broadcast` builders (checked as the host checks them, before any call), a
  `Reader` that keeps surplus bytes and reads lines as UTF-8 (and makes
  `start_tls` refuse with `State` while it holds any), an `Outbox` that queues
  a long message piece by piece on `Sent`, and the `Socket` and `Broadcast`
  events (`EventKind::Socket`, `EventKind::Broadcast`, `Event::Socket`,
  `Event::Broadcast`). Borrowed buffers, no allocation, no socket data in
  `Debug`.

- Native widget/status declarations use the shared retained declaration store and
  full-width host owner identities. Callback frames expose the directory, bounded
  owner/token status calls and a fixed kernel stack snapshot. UI cleanup retries
  borrowed stores after callbacks return; X4 nested UI activation remains gated (#228).

- **Deferred and background work in the native SDK** (#228).
  `native::work::Work` declares, cancels and lists a plugin's work on
  `core.work`: `WorkSpec` says when it comes due (after, within, every, an
  exact time or a moment such as going online or charging) and whether it needs
  the network or only runs when that is cheap. `retry_delay_ms` gives the
  growing wait after a failure. Everything is checked before the host call and
  nothing is allocated. Receiving the due, refused and allowed events follows.

- **Production `core.net` host for interface 7** (#228). The core now
  serves HTTP and the local server from the storage and the clock it is
  given: it checks the save target and the staged file against the files
  the card is using, keeps free room back for the system, writes a saved
  answer to a staged file and renames it only when whole, and cleans up
  `.received/` when a plugin stops. A shell wires it with
  `HostWiring::net` and only supplies the transport (`ShellHttp`) and the
  network and power state (`ShellNetwork`).

- The network manager now hears what `core.net` and `core.sockets` want: `event.network_wanted` once when a request, server or socket waits for a network that is not up, `event.network_idle` after the last of them is gone, and the flags a shell reads (`Env::wifi`: `wanted`, `busy`) include them. A plugin whose network is off, held or refused by the offline mode creates no demand (and the shell is no longer told of one for a request or socket that is refused at once). The work scheduler's one-shot wish for a network (`Kernel::native_work_wish`, called by the work pump) stands for 30 s and lapses when a network is up; the kernel wakes at its end and not before.

- **News on interface 7: the list of feeds and the library** (#228). The feeds are listed on a screen of their own (*Fetch the news*, the feeds, *Add a feed*): a tap opens a feed's book or fetches a feed that has none, a hold offers to open, fetch or remove it, and the address of a new feed is typed on the keyboard. A fetched feed is put in the library as a book in the folder *News* with the library's own import. The headlines widget and the removal of the interface-6 News follow. After an update from Beta 3 each feed is fetched once more and its new book is saved beside the old one (the old book was not imported by this plugin), which may be deleted by hand.

- **The registry's menu list tells which menus the user redefined** (#228). `core.registry` op 11 (menus) will set `flags` bit 5 for a menu the user has a `command.set_menu` definition for, also an empty one or one equal to the declared rows, and clears it after a reset. The host side is in place and tested for all four states; the list of the user's own menus is read without allocating.

- **Picture home draws its status bar and takes a finger's drag** (#228). The bar is drawn in the strip the page keeps free: its plate in the five looks, full or a compact pill at the left, middle or right, the items of any plugin, a tap on an item that does something, shown always, never or after a press (the first press of a hidden bar only shows it; the row of hints shows with it). While editing, an item or its corner is dragged with a finger. The dashboard's focus, grid and arrange model and the status line's layout and drawing now live in `paperpaw-ui-kit`, shared with home.

- Native installer plugins can answer plugin requests (`command.answer_plugin`): only a plugin that declared `uses.install` may, the requester alone hears `event.plugin_answered`, the first answer for a plugin wins, "installed" is checked by the host, and a request is closed when it is fully answered, when no installer is left, or when its requester stops. A request naming `core` is refused.

- The check of a plugin source folder (`check_file` kind 0, `docs/abi/device.md` 5.1) as a bounded, stepwise job of the host: the folder walked in pieces (id, name and version from `plugin.ini`, the sum of the sizes of the files an install copies), `code.ppn` (format 5) checked as the loader checks it (header, room for code, digest, Ed25519 signature, metadata record against what the device offers), and the trust flags from Beta 3's pin files, read a line at a time (the pinned key, its recovery key, revocations that hold). It changes nothing on the card, refuses the reserved `core` and the plugins built in, and a pin file it cannot read is a card error, never "no pin". First slice of the host side of plugin install (B2c-1); the queue wiring follows with Core's plugin seam.

- `install_plugin` (`docs/abi/device.md` 5.2) reaches the plugin install job through the firmware queue: the command with its `install_author` and `install_rotation` evidence (checked whole at submit: flags, the request each belongs to, one author, at most six rotations, keys and signatures none or whole), accepted like any firmware request, run one job at a time. The job's question waits in the driver until the host's answer (`plugin_question`, `answer_plugin`); the host's trusted-installer grant is a hook of its own (`set_install_grants`) and is the only way to skip it. An install that the user confirmed keeps running when the plugin that asked stops, and one past its journal is stepped on to its end; `install_done` carries the installed plugin's id and `flags` (bit 1: the code changes at the next start). Third slice of the host side of plugin install (B2c-2b); the confirmation's words and the grant's setting are Core's, the removal and `trust_revoked` follow (B2c-3).
- The confirmation of a plugin install or removal is shown on the host's own guarding screen, the one of the firmware install (en and de words: who asked, name, version and id, the signer or the key's fingerprint, "not from the store", an older version, what the plugin uses; characters that could forge a line are replaced). Only the screen answers, through its nonce: No ends the install with reason 2; Back, ten minutes, sleep, a screen that cannot open and a device without a random source end it as dismissed (reason 3), never as a yes; the asking plugin stopping closes the screen. A job that waits for the answer is no work for the main loop.

- The install of a plugin source folder (`install_plugin`, `docs/abi/device.md` 5.2, 5.4) as a bounded, stepwise job of the host: the folder copied into the stage `plugins/.<id>.part` (outside every root a plugin reaches) in pieces of 4 KiB, the stage checked again as a check does, the trust rules of the store moved into the host (pinned key, rotations signed by the pinned recovery key, new id with the catalog's author, sideload, revoked key, digest) and applied to the stage, the older/same-version gate, the host confirmation as plain data (with the trusted-installer grant skipping only the prompt, never for a sideload or a new `uses.*` feature), and the journalled swap (`plugins/.journal`, `.old-<id>`) whose start-up replay finishes it, so a cut leaves the old plugin with its pins or the new one with its pins, never a mix. Pins and author keys keep Beta 3's on-card files. Second slice of the host side of plugin install (B2c-2); the queue wiring and the evidence pool follow with Core's plugin seam.

- **Plugin manager says why a plugin passed the book-field, badge or lookup-provider limit** (#228). A plugin whose batch passed the limit of book fields (8 a plugin, 64 on the device), book badges (4 or 16) or lookup providers (8 or 32) shows "Not loaded: it passes the limit of {cap} …" in English and German, with the limit the host reports; no limit or an unknown number still says "Could not be loaded".

- **The plugin manager says why a plugin with too many formats did not load** (#228). A plugin the device refuses because it passes a limit of book formats (per plugin or on the device) or of reading apps shows that limit instead of "Could not be loaded".

- **Plugin manager 1.0.6 as an interface-7 native plugin** (#228). The list
  of plugins with their state and notes, switching on and off, the places to
  get more, *Screens* with its question when a plugin offers a screen for a
  place, and the notices after a start in safe mode or with plugins not
  installed now run on the new plugin interface. The interface-6 plugin is
  gone. How a plugin works, the places it is needed for, where it came from,
  *Restart now* and the reading-app row come back once the SDK and host can
  provide them.

- **Plugin jobs in the firmware queue** (#228). The kernel's job queue now holds plugin jobs (a plugin check, later an install and a removal) next to firmware ones: they share the request limits (four device-wide, two a plugin) and the outcomes of `install_done` and op 13, and run one at a time, never inside a callback. `check_file` of kind 0 (feature `uses.install`) is accepted at submit once the shell gives the device's plugin host (`KernelBuilder::plugin_host`), else it stays unsupported. A plugin check's facts wait in its plugin's one result slot for op 6 (the same slot as a firmware check's; op 16 answers not-found after a plugin check), and a stop or a plugin that stops ends its jobs like a firmware job's. The id `core` is "taken" (reason 15) again after the SDK's reserved-name rule.

- The removal of an installed plugin (`remove_plugin`, `docs/abi/device.md` 5.2, 5.4; decision 124) as a job of the host: `core` and the plugins built into the image are refused (reason 15), one question with three answers (remove and keep the data, remove with the data, cancel; the trusted-installer grant never skips it), then a journalled removal (`plugins/.journal`, record kind 1) that closes the plugin's instances first, moves `plugins/<id>` and, if chosen, `.paperpaw/plugins/<id>` into `.removing-<id>` folders by the card's journalled move (the ids are checked names compared whole, never a path a plugin gave: removing `battery` leaves `battery-plus`), has the host forget the plugin's secrets and its grants (`core.grant.<kind>.<id>`, so a reinstall does not regain a trusted installer) with its data, after the moves and before the journal goes (a cut there keeps the journal: the start's replay stops at that step and the kernel's first pass finishes it with the host, a host that cannot forget keeps it for the next start), then drops the journal and removes the moved folders in bounded pieces (a large data folder never holds the device in one call; what a cut leaves, the start's sweep finishes piece by piece). A cut at any point is finished by the same steps at the next start: the plugin is whole, or removed with its data only if chosen. A removal refuses (reason 1) while an earlier one's `.removing-<id>` folder is still there, before any journal is written. Pins stay, so a reinstall still comes from the pinned author.
- At start, before plugins are found, a plugin install or removal a cut left is finished, and a journal that can never be finished (a state no step makes) or cannot be read is set aside as `plugins/.journal.bad` with a line in the log instead of blocking every later install with reason 1. Leftover `.removing-<id>` folders and a half-written journal are swept, at most eight a start; a stage `.<id>.part` never is (the shell hands stages over to be installed after the start; the next install of the id removes its own), nor an `.old-<id>` folder, which can be the only copy.
- `remove_plugin` reaches that job through the firmware queue (one command a batch, the request and a name needed, `flags` reserved: anything else is `ERR_INVALID`), and the removal's question is shown on the host's guarding screen with three buttons — remove and keep the data, remove with the data, cancel — with the focus on cancel. Back, ten minutes, sleep and a screen that cannot open end it as dismissed (reason 3), never as a yes. The plugin leaves the list (event 70) once its instances are closed, before its folder is moved.
- `install_author.sha256` of a catalog's install is passed to the install (none or exactly 32 bytes; another length is `ERR_INVALID`), so other code than the catalog's ends the install with reason 18. Evidence that follows another device command instead of its `install_plugin` is refused at validation with `ERR_INVALID`, not later with the batch half taken. The install question's text is built with fallible allocation; with no memory for it the question is dismissed, not asked.
- `trust_revoked` (`docs/abi/device.md` 3, 5.1): at most eight a batch (a ninth, a key that is not 32 bytes, a signature that is not 64 or an empty plugin is `ERR_INVALID`, the whole batch refused), needing `uses.install` like the other install commands. The device keeps the revocations that hold (signed by the revoked key itself or the plugin's pinned recovery key, about the key pinned as its author) at the next pump with no plugin job running, never takes one off, and drops the others; nothing answers it. At most 16 wait for the pump; a revocation that finds no room is dropped and counted (the library lists it again).

- Native plugins can ask for plugins that are not on the device (`command.request_plugins`): the host holds at most four open requests (two per plugin), tells installers that subscribed with `event.plugins_requested`, and answers `failed` at once when no installer can answer.

- **A book's reading position can be read without risk to memory** (#228).
  `positions::try_load` reads where a book was left and returns
  `data::Error::OutOfMemory` instead of stopping the program when the
  device is short of memory.

- Power is a native interface-7 plugin only (1.1.2): the interface-6 plugin and its Kernel
  tests are removed; the mode choice, the power menu and the sleep-screen-first ordering move
  to a host-free model with tests, the DEV commands (sleep, power off, restart, wear) have
  typed SDK methods, and the dialog's answer reaches the plugin without a subscription (#228).

- The native SDK receives the radio, firmware and install events as typed
  `Event::WifiScanned`, `Event::WifiDone` (with `WifiFailure`),
  `Event::NetworkWanted`, `Event::NetworkIdle`, `Event::FirmwareChanged` and
  `Event::InstallDone`, with matching `EventKind` subscriptions (#228).

- **Reader: font size, justification and chapter actions of the interface-7 reading screen** (#228). Larger and smaller text, switching justification, and going to the next or previous chapter work again.

- **Reader on interface 7: read aloud** (#228). "Read aloud" hands the page to the device's voice sentence by sentence in the book's language and at the speed set, underlines the sentence being read (setting *Underline the sentence*), turns the pages (setting *Turn pages*) and stops after the time of the sleep timer. A tap or a button pauses it and "Read aloud" goes on from the same sentence; it also ends when the book is closed, another screen covers the book or the device sleeps. It needs the host's speech service.
- **SDK: `EventKind::Spoken`** (#228). A plugin can subscribe `event.spoken` (the host's answer to `command.speak`), which goes only to subscribers.

- **Reader: host-free model of the annotations screen** (#228). The Markdown export, the notes folder, the file name and the list rows, with allocation fallible; the screen follows with the native reader.

- Reader (interface 7): the annotations screen (`reader.annotations`) lists the bookmarks and highlights of the book on show from `core.library`, goes to the one chosen, and exports them as Markdown (written in pieces into the `Notes` folder and moved into place, then the stage `reader.notes-exported`).

- **Reader: the block assembler of the interface-7 view** (#228). Joins the fragments of a block read from the document service into the block the layout lays out, checked, allocation fallible; the view follows.

- The reader's report to the host now carries the title of the chapter of the page on show and the pages left in that chapter (once the pages are counted), for the interface-7 reader.

- **Reader on interface 7: underline, overline, strike-through and table rules** (#228). A run is drawn with the lines its style asks for (one, two, dotted or wavy), as in Beta 3. Pictures and table rows follow.

- **Reader: the document adapter of the interface-7 view** (#228). The layout reads the open book through the document service, a block at a time, with the fragments joined and a changed block read again.

- **Reader: the interface-7 entry** (#228). What the reader declares (settings, six screens and their slots, actions, menu entries, key bindings and three stages (the events wait for the host)) is built into the plugin image and submitted by a native entry; the screens follow, and the plugin is not active in the app until they do.

- **The reader turns pages by itself on interface 7** (#228). "Turn pages automatically" (menu *More*, or its key) starts a timer on the host's work service that turns the page every "Turn pages every" seconds (5 to 600, as before), sets the count anew when you turn a page, and stops at the end of the book, with the menu or a tap in the middle, with any other action, when another screen opens over the book, when it closes, and when the device sleeps. A device without the work service says so and starts nothing.

- The native reader hears `event.library_changed`: when another plugin changes the highlights or notes of the book on the page, they show at once, not at the next open (#257).

- The native reader draws the book's highlights on the page (in the look of the `core.highlight` settings) and marks the match of a search hit you go to (`core.search-hit`), as the interface-6 reader did (#257).

- **Reader: the stage `reader.notes-exported`** (#228). After the reader wrote a book's notes as Markdown it runs this stage (the file's root, path and title) for notes apps and syncs to take the file further; it replaces the event `core.notes-exported`.

- The native reader's selection bar can highlight the chosen words (and take the highlight away) and write a note on them (*Highlight*, *Remove highlight*, *Note*, *Edit note*), kept in the library as before (#257).

- **Reader: a laid-out page drawn by the interface-7 view** (#228). The lines of a page are set word by word in the role of each run through one function that reaches the host.

- **Reader on interface 7: pictures and table rows** (#228). A picture block is drawn scaled into the page with the host's picture drawing; a table row keeps its cells and column weights, so its cells are set and its rules drawn as in Beta 3.

- **The native reader opens a book's information, the font and layout panel, and sets or removes a bookmark** (#243). Book information and the quick settings hand over to the slots they always did; a bookmark is kept in the library like any annotation and goes with all the other bookmarks of the page. The ribbon on a bookmarked page and the corner tap follow with the reading screen's drawing code.

- **Reader on interface 7: the page is laid out again** when a reading setting changes or the screen turns (#228). The place is kept; the pages before it are found anew.

- **Reader: the interface-7 reading screen tells the host where it is** (#228). On open and at every page turn the reader reports the place, the progress and the title, and that the book is read to the end, so the host raises the book events (opened, page turned, finished, closed).

- **Reader: the interface-7 reading screen opens at the place saved last time and saves it** (#228). Without a place in the hand-over the book opens where it was left, and the place and its progress are written when pages are turned (at most every 5 seconds) and when the screen closes.

- **Reader: the reading screen of the interface-7 reader** (#228). Opens the book named by the hand-over through the document service, lays out the page with the reading settings, draws it and turns it with the page actions; the saved place, progress, menus and selection follow.

- **Reader: host-free model of the search screen** (#228). The text looked for, the snippet round a hit and its cut to the row, the hits kept and the background scan over the book's blocks, with allocation fallible; the screen follows with the native reader.

- Reader (interface 7): the search screen (`reader.search-results`) asks for the text with a question, reads the book in the background (`core.work`) a few blocks at a time while the hits fill the list, and goes to the place of the hit chosen (no match mark yet).

- **The native reader finds a book's chapters and can skim** (#228). For a book whose table of contents has at most one entry, the reader scans the blocks for headings in small background steps and keeps the result in its data folder, with a "Finding the chapters" text that says how far it is (English and German). The skim screen (a bar across the book with its chapters, and buttons for the chapter before and after, where I was, and a page number) is ported to the interface-7 reader as an overlay at the bottom; carrying out its moves in the reading screen follows.

- **The interface-7 reader has its status lines** (#228). The line above and the line below the text switch between the lines you chose (items, progress bar, empty) with a tap or the *Next status line* menu entries, and the choice is kept as before. The reader declares its places, its title, chapter, page and progress items, its progress and empty lines, and the status settings (text size, edge, lines, parts, battery percent). A place's height depends only on the status text size, so switching lines never changes the pages. `paperpaw-ui-kit`'s status line can now show a plugin's own items.

- **Reader: host-free model of the table-of-contents screen** (#228). Entries made from headings, the kept copy, the focus on the current chapter and the place parameters, with allocation fallible; the screen follows with the native reader.

- **Reader: the table of contents of the interface-7 reading screen** (#228). The contents action lists the book's chapters with the focus on the one being read; choosing one goes there.

- **Reader: the typesetter of the interface-7 view** (#228). The page layout measures text with the four reading fonts of the host (regular, bold, italic, bold italic), with the widths kept in a small table.

- **Reading goal's sleep screen on interface 7** (#228). *Today's reading cat* is offered to the sleep screen's slot below the sleep plugin's own screen and is one of the choices of the sleep screen setting: the cat of today's mood large in the middle, the minutes and the goal below it.

- **Record tags can be checked without decoding** (#228). The generated
  records codec gains `record_tag_known(tag)`, so a plugin or the host that
  decodes records by type keeps the statuses of the `Record` enum path (an
  unknown tag is `ERR_UNSUPPORTED`, a known but unexpected one
  `ERR_INVALID`) without linking every decoder.

- **The host admits plugins' registry declarations** (#228). Slot specs and
  offers, key bindings, setting defaults and extra choices for other plugins'
  settings, and presets with their values are now kept with the plugin's other
  declarations instead of refusing the whole batch. A bad record still refuses
  its whole batch, an offer must name a screen the same plugin declared, and
  all of a plugin's registry declarations go when it is unloaded.

- **Registry: plugins a store installed say so** (#244). `install_plugin` takes `flags` bit 2
  "from a catalog" (a store's install, code or data), which marks the version in the host's record
  (interface 6's `plugin-origin.txt`); an install without it forgets the id's store versions. A
  device that kept no record takes the plugins installed for the store's once, as interface 6's
  store did: not at start, but before the first plugin install or removal commits or the first
  plugin rows. `core.registry` ops 5 and 6 set `reg_plugin.flags` bit 5 where the version
  installed, or the one waiting, is in that record, and write that version as the store named it.
  The native store offers updates only for these.

- **`Kernel::register_linked`** (#228). A plugin linked into the program is
  checked (entry, metadata view, target profile, versions and requirements
  against the shell's card-plugin host, passed in), hosted and registered like a loaded one.
  An entry or metadata the kernel refuses, an id that is not a plugin id and
  an id already registered are refused before the plugin's factory runs.

- **`event.registry_changed` bit 0 for pushed item states** (#228). When a plugin's `command.set_item_state` batch changes what the host holds (a new, changed or cleared state), or an unloaded plugin's states end, the host raises bit 0 of `event.registry_changed` once per drain; pending bits are OR-ed per plugin until delivered, so none is lost. The same state pushed again raises nothing. The room a callback reserves for item states is now given back by a guard on every path, a failed or refused callback included.

- **A native plugin can register for `event.registry_changed` (75) and `event.preview_changed` (76)** (#228). Both are told as hints, once however often they were raised before the plugin could be called. `registry_changed` carries the `what` bits raised since the last one, OR-ed together; a change of the plugin set raises all of them, and a menu's item state raises bit 0. `preview_changed` is raised when a settings preview starts or ends. Until now a plugin that named either was refused at `make` with `Unsupported`. Bit 31 (`dropped`) goes to the plugin whose own preset was dropped when applied, and to no other.

- **`core.registry` answers its first ops** (#228). The host side of
  `core.registry` reads the plugin list (ops 5 and 6, the host first), the
  actions, screens, setting groups, slots and slot offers (ops 4, 7, 8, 9, 10)
  and the inputs a device and its plugins have (op 14), paged as the contract
  says (up to 32 whole rows and one `end`, a generation that never is 0, a
  stale one is `ERR_STATE`). Settings, presets, menus, bindings, setup, widgets,
  status and `needs` follow in later slices.

- **`core.registry` lists menus, key bindings, setup steps, widgets and status**
  (#228). The host side of `core.registry` now answers `menus` (every menu id,
  `flags` bit 5 where the user has a definition), `menu` (the rows the host
  would show now: the user's own definition, else the declared rows without
  hidden ones and ones the device cannot offer; `all` adds the hidden and the
  removed rows, `declared` ignores the user's definition; checked, enabled,
  switch, label and badge from the item's state), `bindings` (by layer or for
  all, with where each comes from), `setup` (every step, with `flags` bit 0 where it applies now),
  `widgets` (with kind, icon, cells and whether a place that shows now lists
  them) and `status` (status items and lines natively declared; empty where
  none is). A scope that names nothing is an empty page, as for the other
  lists. Settings, presets, values and `needs` follow.

- **`core.registry` answers the settings and presets ops** (#228). The host
  side of `core.registry` lists the settings by group or plugin with their
  kind, flags, source and value (op 1), gives one setting with its editor
  fields and help text (op 2), lists a choice setting's own and added choices
  (op 3), and lists the presets, one preset and the values a preset or a
  widget placement sets (ops 16, 17 and 18). A value is shown to a plugin
  exactly when slot 5 would give it; otherwise the row is empty and flagged
  hidden. A secret is never shown, only that it has a value, and the grants
  only to the settings screen.

- **On the X4 Pro and X4 Classic a plugin call is refused where the stack band cannot guard it** (#228). A call that starts at an interrupt level of 1 or more, or on the second core, is not run and is logged once; the band's interrupt cannot fire there.

- CI scans the X4 Pro's and the X4 Classic's firmware (`ppn5`) with `paperpaw-sign frames-s3` against `shells/xteink/frame-budget-s3.txt` and `frame-budget-s3-classic.txt`: every host entry a plugin can call and what it reaches stays under the 2 KiB stack band, and every call the scan cannot follow (ROM routines, calls through a vtable or a function pointer, jump tables) is listed one by one with its reason. The S3 build for that step uses legacy symbol mangling; only the names differ from the release's build.
- `frames-s3` budget lines name a function by its path or its raw symbol; the scan names a function by its symbol when another function of the image has the same path, and a line whose path matches more than one function reached is an error.

- The S3 firmware (X4 Pro and X4 Classic, `ppn5`) exports its host table as a named read-only static, `PAPERPAW_HOST_TABLE_ANCHOR` (about 150 bytes of flash), so `paperpaw-sign frames-s3` can read the table's function pointers from the ELF and use them as the roots of its scan. The table plugins are handed is unchanged.

- **Target 9 for format-5 plugin files** (#228, decision 142). `paperpaw-sign` packs linked Xtensa plugins (X4 Pro, X4 Classic) with a frame scan of the windowed ABI (frames under 2,048 B, no `call0`), `paperpaw-native` reads the target-9 header profile, and the X4 Pro and X4 Classic firmware has a PSRAM placement path. The path stays inert until the stack gate has a design for the S3: the loader refuses every format-5 file there with `Unsupported`.

- **The X4 Pro and X4 Classic watch the end of the main stack** (#228). Where interface-7 plugins are built in, a 2 KiB band above a 14 KiB reserve at the stack's end is watched by the chip's debug assistant: a plugin (or firmware code a plugin called) that overflows the stack ends in the crash slot blaming that plugin, not in a failure somewhere else later. Code plugins run only where the band armed at start (and again after a light sleep). The 16 KiB come out of the internal heap on those two devices.

- Safe mode says so: where no plugin provides a screen for a slot (an X4
  with native plugins only, started without them), the placeholder shows
  "Safe mode" and why the plugins are off (a failed start, or a key held
  at power-on) and how to start with them again, in English and German.
  Outside safe mode it still says "Nothing here yet".

- A native screen can say it is best in grays (`screen.spec.flags` bit 9 `grays`, `ScreenDecl::grays()`; at run time `command.screen_policy` bit 9, `ScreenPolicy::grays()`), as a code plugin did with `Screen::grays`: a page with a picture, or smooth text, is shown in the X4's four grays when the screen is on top with nothing over it. The reader sets it page by page.

- `event.screen_ready` (87) and `command.screen_ready` (1557) for native plugins (interface 7): a plugin can register for 87 and is told, once for each screen instance, after the instance's first draw; a screen declared `waits_ready` says so itself with the command (its own top instance, else `ERR_STATE`; a repeat raises nothing). An instance closed before it was drawn is never told.

- **The scripted host answers questions and pages lists** (#228). In
  `paperpaw-scripted-host` (test support), `ScriptedHost::dialogs()` is a
  scripted `core.ui` for questions: request ids, the request text in parts,
  the caps of a text and of a secret, answers by kind and the
  `event.answered` batch for the asker; `ScriptedHost::registry()` is a
  scripted `core.registry` that pages its rows by cursor and generation like
  the host; `sweep_released` runs a refusal sweep and fails a path that leaves
  a heap block or a service handle held.
  The questions check an `ask` as the host does (kind, slot, flag bits,
  choices per kind), cancel a request whose asking callback failed or left
  its initial text uncommitted (`not_started`), keep `typing` text of a
  secret question for the commands check and cap folder answers at 512
  bytes; the dialog's own side (op 2, op 11, instances, nesting, device
  limits) is documented as not modelled.

- **The scripted test host answers `i18n.copy`, `i18n.format` and `i18n.digits`** (#228). A native plugin's callbacks that build texts no longer end in `ERR_UNSUPPORTED` in tests: feed the host the plugin's catalogs (`ScriptedHost::add_catalog`, `add_catalogs_in` for its `i18n` folder) and a language (`set_language`), and the slots answer with the shell's own code (`native_host::formatting::{copy_with, format_with, digits_with}`), so keys, fallback to English, plurals, placeholders and digits give the real results. The three slots can be refused and counted like the others.

- **A scripted host for native plugin tests** (#228). The new, unpublished
  `paperpaw-scripted-host` crate makes a plugin from its entry with a host
  table it answers itself, runs its event, input and draw callbacks, and
  shows what the plugin did: the kernel's own command staging (limits, drop
  on error, no stage in a draw), scripted `core.*` service answers,
  `settings.read` and `env.snapshot` values, `Answered` events, and refusals
  (`ERR_FULL`, `ERR_BUSY`, `ERR_NO_MEMORY` ...) at the n-th call of a slot with
  a sweep over every call. `CALLBACK_COMMANDS` and `CALLBACK_COMMAND_BYTES`
  are public so tests can name the stage's limits.
  Statuses follow the shell: input above 1 and event results above 0 are
  `ERR_INVALID`, events get no reply buffer, an unset setting reads as the
  encoded `Absent` record, `service.open` of an unknown key is
  `ERR_NOT_FOUND` (one handle per key, else `ERR_FULL`) and `service.call`
  checks the caller's buffer against the op's reply bound before the
  service runs.

- **`CommandSender::check_file`** (#228). A native plugin with `uses.install` or `uses.firmware`
  asks the host to check a plugin or a firmware image before the user is asked to install it
  (`CheckKind::{Plugin, Firmware}`); the check ends with `install_done` kind 3 and its evidence is
  read with `Device::check`.

- **A document client in the native SDK** (#228). `native::document::Documents` opens
  `core.document` and opens a book through its format (`open_book`, with `OpenBook`:
  root, format, flags, picture sides, user CSS and the format's options), reads its
  preparation state (`state`, `DocState`, `Preparation`), closes it (`close`) and
  converts a position to a KOReader XPointer and back (`xpointer`, `locate`,
  `DocPosition`).

- **Which format reads a file, in the native SDK** (#228). `Documents::format_for` (DOC op 20: by
  extension, or by sniffing the file's first bytes) answers a `Format`, and `Documents::formats`
  (op 19) lists every format in pages (`FormatPage`, `FormatCursor`), for a file manager's Open and
  Open with.

- **Reading apps in the native SDK** (#228). `Documents::readers` (DOC op 21) lists
  the reading apps in pages (`ReaderPage`, `Reader`, `ReaderCursor`), for "Open with"
  and the default reader.

- **Section lengths of an open document** (#228). `Documents::sections` reads `core.document` op 5 in bounded pages (`DocPage<SectionEntry>`): each `SectionEntry` has the section's `index`, its `length` in source units and `whole_page_picture`, so a reader can compute progress where the format has no progress of its own. The whole page is validated before any row is read, like the table of contents.

- **The reader's side of `core.document`: blocks, table of contents and links** (#228). `Documents::block_at` reads the block at or after a position as validated fragments of typed records (`BlockFragment`, `BlockRecord`, `BlockHead` and the rest), with the continuation cursor for a block that goes on and the next block's offset; `toc` and `links` list entries and links in bounded pages (`DocPage`, `TocEntry`, `Link`), and `toc_target` and `link_target` resolve an entry or an internal link to a `DocPosition`. All are borrowed from the caller's buffers and allocate nothing; a malformed reply is refused whole.

- **Inked text in the native SDK** (#228). `Font::draw_inked` draws text through host slot 41 with an `Ink`: glyphs widened by `extra` pixels, an optional 16-step tone curve and the `sharp` mode, as the per-book font weight, smooth-text and text-contrast settings of Beta 3 need.

- **`Params::try_set` and `Params::try_clone` report a memory shortage** (#228). They copy with the allocation checked and return `TryReserveError` instead of aborting; on an error the parameters are as they were. Code that builds the parameters of a screen or command on a small heap uses them instead of `set`, `with` and `clone`.

- **Favourites and book metadata in the native SDK** (#228). `Library::set_favourite` (LIB op 8,
  status bit 1) mirrors `set_finished`. `Library::book_text` with `BookTextQuery` reads a book's
  whole title, author, series, tags and plugin fields in parts (LIB op 4). `Documents::metadata`
  and `Documents::text` with `DocTextQuery` read a document's title, language, series,
  direction, the counts of authors and subjects, and every text whole, description included
  (DOC ops 3 and 4).

- **`FileCheck::leaves_out`** (#228). A finished firmware check says whether the checked image leaves out plugins built into the running one (`dev_check.flags` bit 5), so the update view can skip `Device::leaves_out` when nothing is left out.

- **`CommandSender::install_firmware` and `firmware_restart`** (#228). A native plugin with
  `uses.firmware` asks the host to install a firmware image from its data or the user's files
  (`FirmwareOptions`: allow an older version, a foreign image, the signing key of an image signed by
  another key). The host always asks the user and ends the request with `install_done` (kind 1).
  `firmware_restart` starts the image the plugin's own install left waiting; the host asks again,
  and declining keeps the image installed for the next start.

- **Reading and status font roles in the native SDK** (#228). `native::fonts::FontRole` gains `ReadingBold`, `ReadingItalic`, `ReadingBoldItalic` and `Status`, which acquire `role:reading-bold`, `role:reading-italic`, `role:reading-bold-italic` and `role:status` as `docs/abi/screens-input.md` section 4 contracts; the host falls back to the reading font (bold italic to bold) where none is bound.

- **The block stream of a format provider in the native SDK** (#228).
  `FormatCall::reply_block` writes a document-model `Block` as the record list of
  `core.format` op 8 (`BlockReply`): header, text parts, runs, offset maps, picture,
  table columns and cells, annotations and inline languages, in fragments that end
  between records with the block's generation. Nothing allocates.

- **Page replies of a format provider in the native SDK** (#228). `FormatCall::rows`
  writes the sections, table-of-contents, links and fonts pages of `core.format` row
  by row into the caller's output (`FormatRows`, `PageEnd`, `LinkKind`), always
  leaving room for the closing record, and `FormatCall::reply_image` answers cover
  and image requests. Nothing allocates.

- **Format provider requests and replies in the native SDK** (#228). A plugin that
  reads a file format through `core.format` decodes a stage request with
  `native::format::FormatCall::decode` and encodes borrowed metadata, text,
  preparation state and scalar replies into caller-owned output, without heap
  allocation. The `format.spec` declaration is `Declarations::format`.

- **SDK: `native::text::hyphenate`** (#257). Native plugins ask the host where a word may break (host slot 38 `text.hyphenate`, `docs/abi/document.md` 6.1), with the tables of the installed hyphenation plugins, so the native reader can hyphenate again. Allocation-free, fallible; break places are checked before they are returned.

- **Native SDK: the UI language tag** (#228). `native::i18n::language` reads the complete UI
  language tag from the `core.language` setting into a buffer the plugin owns, without
  allocating.

- **Pictures in the native SDK** (#228). `native::image`: `ImageId`, `ImageInfo::read`
  (`image.info`), `Canvas::draw_image` with `ImageStyle` (`canvas.draw_image`),
  `CommandSender::image_prepare` (`PrepareImage`, `PrepareRoot`) and `image_release`,
  and `Event::ImageReady` (`ImageReady`, `ImageStatus`) with
  `EventKind::ImageReady`.

- **`CommandSender::install_plugin` and `remove_plugin`** (#228). A native plugin with `uses.install`
  asks the host to install a downloaded plugin (`InstallRoot::Data` or `Files`, update and
  older-version flags) or to remove an installed one; the host asks the user (decisions 124 and 132)
  and ends the request with `install_done`.

- **A plugin sends the catalog's trust evidence with an install** (#228). `CommandSender::install_plugin_with` sends the author and recovery keys, the expected code digest (`Evidence::sha256`) and the rotation statements in the same batch as the install; `CommandSender::trust_revoked` hands the host revocations to keep. The host still applies every trust rule itself. An install carries at most 6 rotations (a batch holds 8 commands); revocations are sent 8 to a batch.

- **Annotation anchors and per-book folders in the native SDK** (#228).
  `Library::anchors` (LIB op 16: `AnchorPage`, `Anchor`, with the conversion and the
  XPointers of annotations) and `Declarations::book_data` (`BookDataKind`) for the
  `book_data.spec` declaration.

- **Book covers in the native SDK** (#228). `Library::cover` (LIB op 7) returns a
  `Cover`: the image id of the book's cover and its size, to draw with
  `Canvas::draw_image`.

- **Staged annotation edits in the native SDK** (#228). `Library::{edit_begin,
  edit_add, edit_commit, edit_abort, edit_receipt}` with `Transaction`,
  `AnnotationEdit` (add, upsert, replace, remove), `Committed` and the receipt pages
  (`ReceiptPage`, `Receipt`, `ReceiptAction`): long texts, notes and XPointers are sent in parts,
  every limit is checked before the host is called.

- **Deleting and importing books in the native SDK** (#228). `Library::can_delete`
  and `Library::delete` (up to 7 paths, `DeleteReport` with one `DeleteRow` per path,
  `with_notes`) and `Library::import` (`Import`, `Existing`) with the contract's name
  checks made before the host is called.

- **`Library::move_to`** (#228): a plugin with `uses.library` and `uses.storage` moves a book to another library path; its position, annotations, fields and book data follow. `Library::can_delete`, `delete` and `import` were already there.

- **Book key, fingerprint and reading position in the native SDK** (#228).
  `Library::book_key` (op 25) reads a book's key, KOReader fingerprint and size
  (`BookKey`), and `Library::position` / `Library::set_position` (ops 13, 14) read and
  write its reading position (`Position`, `PositionWrite`) with the sync and
  confirmed flags, so kosync can match other devices and keep a local place over a
  sync one.

- **Provided metadata in the native SDK** (#228). `Library::provide` (LIB op 24, `uses.library`) with
  `Provided` (title, author, series, number, tags) lets a plugin such as calibre fill what a book's
  file does not name; only the fields set are provided.

- **Native SDK: typed folder and file service** (#228). `native::files::Files` lists folders
  page by page, reads file information and free space, makes folders, moves entries and
  removes a folder tree step by step, all into buffers the plugin owns (`core.files@1`).

- **Serving HTTP from the native SDK** (#228). `Net::serve` (`Server::new(token, port)` or
  `Server::cable(token)`, `max_body`), `Net::stop_serving` and `Net::respond` (`Response`: status,
  content type, up to 8 headers, an inline body or a file). The server's state and the requests it
  gets arrive as `event.server` and `event.served_request`. The SDK checks the port, the body cap,
  the status range, the sizes and counts, one of body or file, and a header's name (an HTTP token),
  value (no control bytes) and the host-owned names (`Host`, `Connection`, `Content-Length`,
  `Content-Type`); the host checks the rest (who may serve, how many servers, the request's owner).

- **`PrepareRoot::Bundle`** (#228). `image_prepare` also reads the plugin's own install folder
  (root 2, read only, no feature), for the pictures a plugin ships. A plugin built into the image
  has none there and the host answers `ImageStatus::NotFound`, a new status (`event.image_ready`
  status 6: no such file at that root and path; an older SDK reads it as `Damaged`).

- **Server events in the native SDK** (#228). `Event::Server` (`ServerEvent`,
  `ServerState`: started with its address, failed or stopped with a reason) and
  `Event::ServedRequest` (`ServedRequest`: method, target and the body length when
  the request has a body), with `EventKind::{Server, ServedRequest}` and the
  `kinds::{Server, ServedRequest}` markers.

- **`Device::leaves_out` and `CommandSender::set_item_state`** (#228). `leaves_out` lists the built-in plugins the last finished firmware check's image leaves out (device op 16), 32 rows a page. `set_item_state` with `ItemState` (hidden, faint, a switch, keep, a catalog label and badge with `formatting::Arg` arguments) says what a menu shows of one of the plugin's own items (command 60). The update plugin's "update without them" choice and its New mark use them.

- **Fallible web helpers in the SDK** (#228). `web::try_clean_name`, `try_file_name`,
  `try_percent_decode` and `try_base_url` do what `clean_name`, `file_name`, `percent_decode` and
  `base_url` do, and return an error where memory is refused instead of aborting, so native
  plugins can name files and addresses on the small heap. The old functions are thin wrappers and
  give what they gave.

- **`RegistryWidget::previews()` and `Registry::widgets_previewing`** (#228). The registry reports which settings group a widget previews (`widget.spec.previews`), and `widgets_previewing(group, ...)` lists the widgets that preview a group, in the host's order (a settings page shows the last). `widgets` still leaves them out.

- **Plugin secrets in the native SDK** (#228). `native::secrets::Secrets`
  gives typed owner-only reads, paged editor listings, save and remove calls
  and random bytes with a strength flag. Replies borrow the caller's buffers,
  values and random bytes print redacted, and secret writes clear their encoded
  input on success and refusal. A public volatile `secrets::wipe` clears a
  buffer after use. The facade allocates nothing.

- **One hint for readers that cannot save passwords** (#228). The text `core.secrets-unsupported` (English and German) tells the owner to reinstall PaperPaw once with the web flasher; plugins use it as they are ported, when saving answers "unsupported".

- `core.net` and `core.sockets` events now reach plugins: the kernel delivers each running plugin's queued events one callback at a time between callbacks (4 per service per pass, in the service's order), frees what the callback could read when it returns, and wakes at the first real deadline of the services (`timeout_ms`, `idle_ms`, `wait_ms`) instead of polling. A plugin can now subscribe with `registration.event` to events 99–102, 106 and 107. A closing screen ends the servers and sockets it started. The work events 103–105 and the run-end hook wait for a work pump.
- `NetReports::redirected_to_https`: a shell that follows an `http` to `https` redirect closes the plain connection and reports it. The request goes back to its place in the queue, keeps its start time, and starts again (the shell resumes the request it parked) only once it holds the device's one TLS session; a session kept by a socket ends it with `timeout`.
- Hardening after review: the pump's event buffer is made once and kept, and a refused allocation waits 2 s instead of re-running at once; a stopped plugin's save claims end without allocating; a staged file kept after a failed rename is tried again at the next claim of its target and when the plugin starts (at most 8 are kept); an https request or socket that cannot start leaves the TLS queue; `core.net` and `core.sockets` naming two different network pools is refused (`ERR_INVALID`) instead of the first one winning.
- `core.net` saves: the staged file is created empty before the request starts (bytes a power cut left at that name no longer end up in front of the new answer); an address the host refuses after the shell is listening makes it stop listening (`unlisten`); `ShellHttp::abort` must have closed the staged file before it returns.
- `core.net` and `core.sockets` follow the user's network settings, decided in core, not by the shell: a plugin whose network is off gets `network_off` (13) with no retry, the offline mode ends a request, socket or server before it starts with `offline` (11) and a retry, and a plugin held back in saving gets `saving` (12) for its background use, as in interface 6. Turning a plugin's network off ends what it has open at the next pass (requests are aborted, sockets closed, servers and broadcasts stopped). The `power_end` hook, `NetEnd` and `NetPowerEnd` are gone from the shell's `ShellNetwork`: a shell no longer answers for policy. The policy is closed (every plugin's network off) until the kernel has filled it and whenever it cannot be made, is made again the moment a setting, the power mode or *Go online* changes and at the end of every call, and a plugin whose id cannot be had (not known, or the table in use) is not let out.

- Native plugins can relabel, icon or hide the hint of a button while their screen shows (`command.set_hint`, 1594). A label goes with its screen instance, at most 8 actions per instance (a ninth is `ERR_NO_SPACE`); the actions that are the user's way out (back, the menu, settings, sleep, power off, restart) are never hidden (`ERR_DENIED`). A device without a row of hints accepts the command and does nothing.

- **`event.setting_changed` reaches native plugins** (#228). A plugin that registers for event 64 is told of every kept change of a setting (a set, a reset, a preset, a write into a book's scope), with the old value as far as it could read the key with `settings.read`; a secret of another plugin and a grant are not announced. A scope switch, an import or an overflow arrives as one event with an empty key ("read again what you keep"). Each subscriber has its own bounded queue, told between callbacks, a few events to a plugin in a pass. Heap: at most 16 edges and 4 KiB per subscribed plugin and 8 KiB for all plugins together, plus one record of up to 1 KiB while it is told.

- **Settings on interface 7: the page of a widget placement** (#228). The settings page of one placement of a widget (a second clock, say) shows the widget's settings with that placement's values; changing or resetting them affects that placement only.

- **The layout and font pages show a sample of text.** The Settings page for the reading layout shows a few lines in the chosen font, size and spacing above its rows, so a change can be seen before leaving the page. If the device cannot draw the sample, the page shows its rows as before.

- **Settings on interface 7: the widget look** (#228). The appearance page offers the looks of widgets again (frames, room, sizes, colours), chosen as the themes are; a theme leaves the frame of all widgets alone.

- `paperpaw-sign frames-s3` budgets take `allow form <function> <form> <why>` for the privileged forms firmware code uses (`rsil`, `waiti`, `wsr`, `xsr`, `wsr.ps`, `xsr.ps`, `l32e`, `s32e`). Each is named in the scan with what it writes, fails unless the budget allows it for that function, and one that writes `a1`, or a `wsr`/`xsr` of `WINDOWBASE` or `WINDOWSTART`, is still refused as a change of the stack pointer (only `allow sp` passes it). `PS` is its own form. `rotw`, the `rf*` returns, MAC16 and the `ee.*` forms stay refused.

- `paperpaw-sign frames-s3 <elf> --budget <file>` reads the call graph of an S3 firmware from the budget's roots and function-pointer tables, holds every function it reaches to a frame under 2,048 B, and refuses `call0`/`call4`/`callx0`/`callx4` and any write to `a1` other than `entry`. Calls it cannot follow (through a register, into ROM or a blob, to code with no symbol or size) fail unless an `allow` line names them, and an `allow` line that matches nothing fails.

- **`core.sockets` host, first slice** (#228). The core now keeps the socket
  table of the service: plain TCP sockets with `connect`, `send`, `read` (bytes,
  also exact), `data` and `close`, the event `event.socket`, two sockets per
  plugin and per X4, and the buffers (send queue and read part) charged to the
  same network pool as `core.net`. Sockets end with the screen or work run they
  were opened in, after the idle time, when the network goes and when the
  plugin stops. TLS, reads to a file and UDP broadcast answer
  `ERR_UNSUPPORTED` until their slices land; a shell still has to wire the
  transport.

- **Reads to a file** (`read` mode 1): the bytes go to the staged file of the
  target and are renamed over it only when all `len` bytes arrived; then the
  plugin is told `saved`. The target and its staged file are in use meanwhile,
  the card rules and the free-space reserve apply, and anything that ends the
  socket first (the peer, a full card, the idle time, `close`, the screen, the
  run, the plugin stopping) removes the staged file and leaves the old file.
  An `exact` read is all or nothing: the peer closing early ends the socket
  with `closed_early`. Core now combines the network demand of `core.net` and
  the sockets, so the shell hears one edge per change.

- **Broadcast** (`broadcast`, `event.broadcast`): one datagram to the
  network's broadcast address on up to eight ports, the answers for `wait_ms`,
  then `done` (the eighth answer ends it early). It counts as one of the
  plugin's sockets, charges eight answers' room to the network pool, and ends
  with the screen, the run, the network and the plugin. An answer over 1,024
  bytes, or from an address over 64 bytes or not UTF-8, is dropped whole and
  does not count.

- **`core.sockets` can be wired by a shell** (#228). `HostWiring::sockets` takes
  `SockInputs` (the card, a `SockTransport` for TCP and UDP, optionally a
  `SockTls`, the same `ShellNetwork` as `core.net`, and `SockLimits`) and gives
  back `SockReports`, the calls the shell's loop makes for each outcome
  (connected, bytes, closed, failed, answers, `secured`, `untrusted`, TLS
  failures, time passed, network lost). Core does the rest: the limits, the one
  network pool shared with `core.net`, reads to a file, the events. A shell
  without TLS leaves `connect` with `tls`, `start_tls` and `peer`
  `ERR_UNSUPPORTED`, so nothing a plugin asked to be secure is ever sent in the
  clear.
- **One TLS session lease for HTTPS and sockets.** A device has 1 to 4 TLS
  sessions (the X4: 1, `NetLimits::tls_sessions` and `SockLimits::tls_sessions`).
  An `https` request and a secure socket take one in the order they ask and
  give it back on every way they end; a request or socket at the head of the
  queue that ends leaves it, so nobody waits behind it.

- **TLS for `core.sockets`, core side** (#228). `connect` with the `tls` flag
  and pins, `start_tls` and `peer` now work on a shell that provides the TLS
  trait; without it they stay `ERR_UNSUPPORTED`. The device's one TLS session
  is a lease in the network pool that `core.net` and sockets take first come,
  first served, one holder at a time. A secure socket waits for it up to
  `timeout_ms` (`busy`), a secure socket idle for 5 s while another waits ends
  `session_needed`, `untrusted` and the `tls`, `name` and `expired` failures
  come from the shell's reports, and every end path gives the session and the
  lease back before the plugin is told.

- **`event.stack_changed` (81) is raised** (#228): a plugin that subscribed to it
  hears a hint after the screen stack changed (a screen opened, closed or came
  to the top), at most one pending per plugin, between callbacks. It carries
  the time only; the plugin reads the stack with `core.ui` op 1 (`ui.md` 5.1). A plugin that
  starts subscribing is not told of earlier changes.

- **Native plugins can run a stage they own** (#228). `stage.run` (slot 32) from a native plugin now reaches its subscribers: the owner asks one subscriber per cursor, in the stage's order, and gets one `stage.reply` back. The list is frozen for the owner's callback, so a subscriber that stops or crashes meanwhile answers `ERR_CANCELLED` and nobody is skipped. A subscriber that fails, answers too much or answers with something that is no record batch costs only its own reply a status. On the X4 the call answers `ERR_UNSUPPORTED` until its stack gate passes. Plugins not started yet and the user's per-stage switch are not part of this yet.

- **Per-stage subscriber switch** (#228, decision 131). The host setting `core.stage.<stage id>.<plugin id>` turns one plugin off for one stage: it is then not in the list `stage.run` goes through and does not count toward the 32 active subscribers, from the next callback of the stage's owner. It is on unless set to no. The setting has no screen or definition yet.

- Tools and tests: `Kernel::stage_text_question` asks a text question as a plugin's `ui.ask` does, so the emulator's `open:keyboard.screen`, the flows and `paperpaw-dev`'s visual checks open the native keyboard (which needs a question); with the interface-6 keyboard the screen opens as before.

- **Statistics on interface 7: sync with KOReader's WebDAV folder** (#228). The folder, user and password, *Sync now* (download, merge, upload only if nobody changed the file) and *Take over from KOReader*, as before; the overview shows the sync row again.

- **Statistics on interface 7: the widget *Daily goal*** (#228). Today's reading as a ring (whole, or a gauge open at the bottom) with the minutes inside and the streak or the time left below; without a goal it asks for one and a tap opens the settings where it is set.

- **Statistics on interface 7: the widget *Reading streak*** (#228). The days read in a row, the longest streak and the last 7 or 14 days as marks, as before.

- **Statistics on interface 7: the widget *Reading calendar*** (#228). This month as a calendar or the last weeks as a heat map, each day shaded by how long you read, with the streak and the time read below.

- **Statistics on interface 7: the widgets *Read today* and *Reading per day*** (#228). They draw on the home screen from the statistics' files, open the statistics on a tap and show a new day without a page turn; the daily goal is a setting again, and setting it for the first time puts *Read today* on the first home page.

- **Statistics on interface 7: the widget *Books this year* and the status items "Time left in chapter" and "Time left in book"** (#228). The widget counts the books read to the end this year towards a yearly goal (a bar, a ring or a dot a book), says how the year goes and names the book finished last; a tap opens the books. Books without a finishing date count if the library has them read to the end (the plugin now asks for the library, optionally). The two status items give the pages still to read at your reading speed.

- **`Storage::same_entry`** (#228): whether two paths name the same entry however each is spelt (`None` where the storage cannot tell, the default). The memory card answers by the entries' clusters, the mounted storage asks the side both paths are on. `core.files` uses it to refuse an alias of a protected folder by identity, not spelling.

- **Store on interface 7: the download and the install** (#228). The files of a plugin come one after the other into a staging folder, each checked against the size and digest the library lists, then the device installs them; what the store installed is kept as before, and the queue goes on with the next plugin. Not wired to the screens yet.

- **The store offers the plugins a plugin needs** (#244). A plugin that names others in `requires` has them offered after its install, with their sizes: *Download* or *Not now*, as in Beta 3.

- **Discover fonts is back on the Text page of the reading settings** (#244). It opens the fonts browser's list, or says why it cannot (not installed, switched off, waiting for a restart, could not start) and offers the one thing to do about it, in English and German.

- Native plugins can ask which part of a text a font draws (host slot 39, `text.covers`): the byte length of the longest start the font and its fallbacks have glyphs for, so a keyboard can hide layouts the device fonts cannot draw. Text over 1,024 bytes is `ERR_INVALID`, text that is not UTF-8 is `ERR_UTF8`, a stale font handle is `ERR_STATE`, and an icon role with no icon font bound answers 0.
- Text typed from outside (a shell's remote control or a hardware keyboard, `Kernel::type_text`) reaches a native keyboard screen as `event.text_typed` (109), only while that screen is on top and is the one offered on slot `core.keyboard`. Text over 1,024 bytes comes in several events cut between characters, `done` only on the last; more than 64 KiB in one call is dropped whole.

- **Glyph coverage and typed text in the native SDK** (#228). `Font::covers(text)`
  wraps host slot 39 `text.covers` (the byte length of the longest start of the
  text the font draws), and `Event::TextTyped { now_ms, text, done }` (text as `TypedText`, whose `Debug` shows
  only its length) decodes
  `event.text_typed` (109), text from a hardware keyboard or a shell's remote
  control, for screens on the `core.keyboard` slot.

- **The host answers `text.draw_inked`** (#228). Slot 41 draws for native plugins: no canvas is `ERR_STATE`, then a stale font handle, then the arguments (a tone of 0 or 16 bytes, no unknown flag bits). A widening above 16 is clamped to 16, `sharp` applies to the chosen curve, and a refused argument draws nothing; as with `text.draw`, an advance error is reported after drawing.

- **Inked text for native plugins** (#228). Host slot 41 `text.draw_inked` draws text as `text.draw` does, with glyphs widened by up to 16 pixels (more is clamped) and an optional 16-step tone curve (or without grays at the screen's own threshold). This gives readers the per-book font weight, smooth-text and text-contrast settings, and lets widgets draw bold runs as Beta 3 did. Hosts answer it as unsupported until it is wired.

- **Theming (#114, build-out): the interface kit's shared parts can be styled by a theme.** New appearance settings, each leaving the look as it is until a theme sets it: `core.ui-shadow` (a hard shadow under dialogs and chips; the bottom sheet has no free edge and takes none), `core.ui-frame` (the plate of sheets, dialogs and chips: none, line, card, dark), `core.ui-density` (compact, regular, roomy margins and gaps), `core.ui-button` (outlined, filled, flat buttons) and the light bar for `core.ui-focus = core.focus.soft` on panels of more than two grays (a frame on two). A theme also colours the panel, the chosen row's bar, filled buttons, outlines, the chosen chip and the dark header bar through new names in `core.ui-palette`. The SDK gains the setting constants only (`api.txt` and `shape.txt`, lines added, none changed). The sample theme `theme-slate` sets them all. Texts in English and German.
- **ui-kit: `StyleCache`.** A screen can keep its `Style` and clear it on `setting_changed` and when it is shown; nothing in the kit holds one for a screen (a read is 12 host calls, about one per screen rebuild, measured on the X4 profile).

- **Wi-Fi transfer on interface 7: plugins, typing on a phone and the offers** (#228). A plugin sent from the computer
  is put on the device file by file, each checked against its SHA-256, and installed once the device has asked you; the
  page's hint for a book sent outside the library folder knows the formats the device reads. The phone's keyboard
  (*Phone* in the keyboard) serves a small page for typing and sends the text back. In the offline mode the screen offers
  to go online first, and *Without a router* opens the device's own network with a code to join it, closed after ten
  minutes without a request.

- **Translation on interface 7: the language setting** (#228). The plugin now declares the setting *Translate into* (the reader's own language or one of the 32 DeepL languages) in the reading settings, as the interface-6 plugin did.

- **Translation on interface 7, first slice** (#228). The translation panel is declared to the host as interface 7 asks, and the parts of translating that need no host — which language DeepL is asked for, the request with the key kept in a buffer that wipes itself, and reading the translation from the answer — now run without panics, report out-of-memory instead of aborting, and read answers up to 24 KiB. Asking DeepL and showing the panel stay on the old interface until the network, secrets and screen services are there.

- **Translation on interface 7, second slice** (#228). Asking DeepL over the network service and keeping the API key with the secrets are ready on the new interface: the request goes out as before (the key only in the `Authorization` header, texts up to 3,000 characters whole, longer forms sent from a file), the answer is read in pieces up to 24 KiB, and a host that cannot save passwords is told apart so the key can still be used once. The panel itself stays on the old interface for now.

- **Translation on interface 7, third slice** (#228). The translation panel now runs on the new interface: it reads the selected text, asks DeepL, shows the translation over the lower part of the page (scrolled by the page buttons or taps, closed with Select), asks for the API key on the keyboard (typed masked, kept with the passwords, used at once), offers once to go online in offline mode, and says "This device cannot save passwords yet" where the key cannot be kept. The language setting and the entry in the reader's menu stay on the old interface until the host and the contract take them.

- **Translation on interface 7: the offer for the reader's *Translate* slot** (#228). The plugin now offers its panel for the reader's slot `core.translate`, so the reader can find it once it reads the new offers. *Go online* in the panel lasts 10 minutes, the most the host allows; press *Translate* again after that.

- `core.ui` ops 18 `clipboard_write` and 19 `clipboard_read` (interface 7): the kernel's one clipboard as text 0 of at most 4,096 bytes on every device, written in parts and replacing the old text only when committed, read in parts by any plugin. A committed write is copied out to the system clipboard where the shell has one, and interface-6 plugins read the same text.

- `core.ui` op 3 `hints` (interface 7): one `ui_hint` row for each `help.hint` declared for the screen asked about, in plugin order and then declaration order, with the input style, zone and text key as declared and the declaring plugin as owner. A screen nobody declared a hint for gives an empty page.

- **Chips for native screens** (#228). The plugin ui-kit has a row of chips, one of them chosen, as the
  library showed its filters on large screens in Beta 3: each chip as wide as its word, a gap apart, onto the
  next row where one does not fit, the chosen one filled and the focused one framed; left and right move the
  focus, a tap chooses.

- The UI kit tells what it draws to whoever asks (`canvas.describe`): list, item and window rows, headings, dialogs, sheets, chips and the keyboard's field and keys. A draw asks once at its start and queues nothing where nobody does, so automation (`tap-on:`), reading aloud and tests see the native screens' parts. A row can say it is a switch (`RowItem::on`): the menu's switch settings are told as on or off.

- **A list can keep room above its first row.** A screen can reserve a strip at the top of a list for its own picture, such as a sample of text; the rows scroll and focus below it.

- **UI kit: wiping a list** (#228). `ItemList::wipe` zeroes the text of every item and empties the list, for a screen that showed a secret such as a password drawn for the user to read.

- **A list row can be small print or plain text, and an empty list can have a picture.** A small row shows its title and icon in the small size; a plain row is read-only text in the full ink; a head taller than the page is cut so the first row stays on it.

- **UI kit: the model of the status items editor** (#228). `paperpaw_ui_kit::status_items::Chooser`
  keeps what a setting that lists status items or lines holds and answers the rows to draw, what a
  tap, a drag or the buttons (pick up, move, put down) make of it; items of plugins that are not
  there stay where they were. It is the behaviour of the interface-6 items screen, checked against
  it on every small setting, and refuses cleanly when memory is short. Plugins name their own
  screen built on it as the editor of such a setting.

- **UI kit: the screen of the status items editor** (#228). `paperpaw_ui_kit::status_items::view`
  draws the editor on top of the chooser: the shown items in order, then the others, a hint line and
  the item being moved. Buttons work as on interface 6 (Menu picks up, direction and page buttons
  move, Confirm, Back or Menu put down); on touch, a long press picks an item up and a tap puts it
  down, since the kit has no drag. Two new texts, `core.status-items.hint` and
  `core.status-items.moving`, in English and German.

- **UI kit: a list over a row source** (#228). `WindowList` shows a long list (a word list of thousands) from a `RowSource` and builds only the rows of the page shown, each with a title, a value and a note line below.

- `core.ui` op 1 `stack` lists the screen stack, top first, 8 rows a page: instance, screen, slot, owner and flags (overlay, guards, covered, secret). While a screen guards the device, other plugins see only the topmost guard and are told rows were left out.

- **Widgets and status lines of other plugins** (#228). Screens can now make and drop widget instances and ask for the values and drawn lines of other plugins' status items through the `core.ui` service. Private ones stay hidden behind a lock screen, and a plugin never calls itself back. The X4 answers status requests as unsupported until its stack check passes.

- **Update without the plugins the new firmware leaves out** (#228). When a firmware update leaves out
  plugins that are built into PaperPaw now, the update page names them and offers *Update without them*
  (for an update on the card, from the device's own check of the file) or *Download without them* (for an
  update the server named, from the plugins its answer lists), next to *Not now* to wait. No built-in
  plugin is dropped without being named; an update that waits for plugins and is chosen anyway names
  everything that is gone after it. The *New* mark is back on Update in the settings while an update the
  server named is kept, and goes when it is downloaded or no longer newer. An update whose list of left-out plugins the
  device cannot give yet (a check still running, or replaced by a newer one) is checked again (at once when a newer check replaced the result, as none of ours runs then; after the running check ends when one is running), and one it cannot give at all is shown as one that cannot
  be installed, never offered without its plugins named.

- **Vocabulary on interface 7: the cards on the SD card** (#228). *Cards on the SD card* in the flash cards reads the CSV, TSV and Anki files of the folder into decks, asks where a deck of the file's name is already there, saves a deck to its file (keeping the file that was there as a backup), and switches the removal of the cards a file lacks. The progress shows as a percentage; opening the flash cards still updates from new and changed files when the setting says so.

- **Vocabulary on interface 7: the flash cards' first screen as a view** (#228). The rows of the screen that lists *Review*, *Edit with your phone*, *Cards on the SD card* and the decks are made in both languages with the same texts as before, and when memory runs out they report it instead of stopping. The screen itself is not yet shown by the plugin that runs; its registration and the screens it opens follow.

- **Vocabulary's list on interface 7: Export, Go to the book and the review's help** (#228). *Export* saves all words as `Notes/Vocabulary.tsv` for flash card apps; holding a word whose book is on the card offers *Go to the book* (opens it at the word) beside *Delete*; the review's four help hints are declared again.

- **Vocabulary on interface 7: Edit with your phone** (#2861, #257). The flash cards' *Edit with your phone* row is back on the native plugin: it serves the page for decks and cards from the device, shows its address as text and as a QR code, and guards every request with a key made for the screen. A wrong key is refused (and too many wrong ones lock it out), a request that is malformed or too long gets a clean refusal, and a device without a network says so.

- **Vocabulary's flash card as a sleep screen on interface 7** (#228). *A flash card due* is offered again as a choice of the sleep screen (front of the next card and how many are due), and on waking the review opens with that card, or where *Review on waking* is on.

- **Vocabulary's home widgets on interface 7** (#228). *Word of the day*, *Words saved* and *Cards due* are made by the interface-7 plugin with the settings and texts of Beta 3, and a word looked up redraws them at once. Where the device does not know the day, the word of the day changes each time the widget is made.

- **Vocabulary on interface 7: words looked up reach it through the reader's stage** (#228). Vocabulary subscribes to `reader.word-looked-up@1`, keeps the word with its sentence, book and place as the settings say, and tells the reader if it could not be kept. The reader sends it once its native selection flow is in.

- **Wallabag fetches at a good moment when a rule starts it** (#228). *Fetch articles* fetches at once when you run it; when a rule (or another plugin) runs it, the fetch waits until the device is awake and the network is there, within an hour, as in Beta 3.

- A widget's owner can say an instance shows something new with `command.widget_changed` (its own instances only), and the plugin whose screen made the instance gets `event.widget_changed` (86) between callbacks, once however often it was said, while the instance lives.

- **A widget's maker hears of its drop** (#228). When the screen that showed a widget closes, the plugin that made it gets `event.widget_dropped` at the widget's token, once: the entry ends at the first delivery attempt whatever came of it, and at once when the plugin cannot be called (stopped, unloaded, switched off), so a dead plugin never takes up room in the widget table.

- **A tap forwarded to a widget reaches its owner** (#228). `core.ui` op 6 queued taps and long presses that nothing delivered, so it answered unsupported. The kernel now delivers them between callbacks, a few a pass, to the owner's widget `input` callback with its command stage, applies the guard rule again at the time of delivery, and never delivers to a dropped instance or an owner that cannot be called. Op 6 is served from the first pass on. The widget-drop events were already delivered.

- **`core.ui` widget ops 7, 12 and 13 are served, op 6 is built** (#228). A screen that shows a widget instance it made measures it (`widget_measure`: height and least height for a width), tells it its place (`widget_place`) and reads its targets (`widget_targets`, at most 32, reading order) with one `core.widget` stage call to the owner each, and a tap or long press (`widget_input`) is queued for the owner's `input` callback (8 waiting, 2 on the X4); op 6 stays `ERR_UNSUPPORTED` until Core's dispatcher drains the queue. Only the instance's maker may; an own widget (instance 0, or an instance of the caller's own widget) is `ERR_BUSY`, a private or guarded-below instance `ERR_DENIED` under another plugin's guard, depth 1 only, `ERR_UNSUPPORTED` on the X4 until its stack gate passes. An owner that fails or answers badly counts as no answer: height 0, no targets, the caller goes on.

- **Widget pack on the new plugin interface, battery** (#228). The battery
  widget (lying, standing or as a ring, the bolt while charging, shaded when
  low) now runs as a native widget in the widget pack.

- **Widget pack on the new plugin interface, highlight** (#228). The widget
  that shows one of your highlights (of the books read lately or of all,
  another each day or each time it shows), with the note and the book below,
  now runs as a native widget in the widget pack; a tap opens the book at the
  highlight.

- **Widget pack on the new plugin interface, library in numbers** (#228). The
  widget that counts the books, those read, being read and not read yet, and
  shows the share read as a bar, now runs as a native widget in the widget
  pack. It counts with the library's own totals instead of looking at the
  books opened lately, so it is exact for the whole library.

- **Widget pack on the new plugin interface, time family** (#228). The analogue
  clock, word clock, tiles, world clock, calendar, year, countdown and moon
  widgets now run as native widgets in the widget pack, on the shared widget
  toolkit. The words and books families and the removal of the old pack follow.

- **Widget pack on the new plugin interface, first slice** (#228). The day
  and date widget (weekday, number and sheet styles) now runs as a native
  plugin. `paperpaw-ui-kit` gains `widget`, the toolkit for widget plugins:
  settings read when drawing, the shared frame, paper, ink, corners, line,
  shadow and padding, fitted and wrapped text, and stacks. The other widgets
  follow.

- **Widget pack on the new plugin interface, words family** (#228). The
  greeting, note and quote widgets now run as native widgets in the widget
  pack. The quote widget reads the shipped quote files and the reader's
  `quotes.txt` a line at a time and follows the interface language. The to-do
  list, books family and the removal of the old pack follow.

- **Wikipedia in the dictionary panel again** (#244, native). The native Wikipedia plugin is a lookup provider once more: the panel shows *Looking it up …*, then the article's summary with a line to open the whole of it, as in Beta 3. When the network is not reached, the panel says so.

- **Work events in the native SDK** (#228). `native::work` decodes
  `event.work_due` (`WorkDue`: key, token, `Why`), `event.work_refused`
  (`WorkRefused`: `Refusal::NetworkOff`) and `event.work_allowed`, as
  `Event::{WorkDue, WorkRefused, WorkAllowed}` and
  `EventKind::{WorkDue, WorkRefused, WorkAllowed}`.

- **Declared work runs** (#228). The kernel now carries `core.work` to its plugins: when work comes due the plugin gets `event.work_due` (103) in a run, `event.work_refused` (104) when the user turned its network off, and `event.work_allowed` (105) when a hold ended. Wake, online and charging moments come from the device. A run's requests and sockets keep it open until their end events were delivered, those events come in the run, and at the longest time (10 minutes) the host ends them with reason 14 (requests) or 27 (sockets); a plugin that never takes its events loses the run 30 seconds later.

- X4: a firmware built with `native-on` hosts the linked native plugins for
  real (`register_native_plugins`, offering exactly the services it wired),
  in place of their interface-6 forms. The test image (`test-image`) also logs
  the stack's and the heap's high-water marks every 30 s and keeps the lows
  through a reset; the `release` feature refuses it at compile time and a check
  refuses it in the release build. A start that fails twice in a row, and a band
  lost after a sleep, go on without native plugins.

- **The X4 firmware wires `core.work`, `core.document`, `core.library` and `core.files`** (#228, #243). The loader wires them with the network (`core.work` over the bridge's clock and network) and over the card (the books a plugin reads, the catalog, the card's files), and offers exactly those four to the plugins it admits, with the features `uses.storage` and `uses.library`. Nothing runs yet: interface-7 plugins stay held back by the stack gate. `core.secrets` follows with the device key's sealer (#411). A plugin that is given `uses.storage` can read and write the whole card (its root), as the owner decided (Decision 125): only signed plugins are given it. The services are offered only when their wiring succeeded at that start.

- **X4 `core.device` host, first part** (#228): `paperpaw_x4::device_host::X4Device` answers the device (op 1), the named readings (op 2), the plugins built into the image (op 4: the plugin id of each crate the image lists, as an update's check names them) and the physical keys (op 15). The other parts, and wiring it into the firmware, follow; the firmware does not offer `core.device` yet.

- **X4 `core.device` host, second part** (#228): `X4Device` also answers the running firmware (op 3: version, project, chip, room, undone; the install state is the core's), what code plugins stand on (op 5), memory (op 7, one real probe per 250 ms, cached answers say so) and the plugins a start could not install (op 14). Not wired into the firmware yet.

- **X4 `core.device` host, third part** (#228): `X4Device` also answers the Wi-Fi radio (op 8), the networks the last scan found (op 9: the strongest 32, none known until the secrets service is wired on the X4) and the power level and mode the kernel reports (op 12). Not wired into the firmware yet.

- **X4 `core.device` host, fourth part** (#228): `X4Device` also answers the Bluetooth radio (op 10) and the bonded devices (op 11: at most 16, in the firmware's order) in builds that have a radio; builds without one answer neither. Not wired into the firmware yet.

- **`core.device` on the X4, wired** (#228): interface-7 builds make the device once at start (`shells/xteink/src/device_host.rs`: identity and `caps` per device, the controls the kernel is built with, the start, the running firmware, what code plugins stand on, the plugins not installed, the memory probe), wire it behind the host table with `core.ui`, tell it the code room when the loader is done, and offer `core.device` to admission. Nothing runs on the X4 until the stack gate lifts; the Wi-Fi, power, Bluetooth and USB readings follow in the next part.

- **`core.device` on the X4 follows the kernel** (#228): the networks a scan found (op 9), the USB readings and the front-light percent are told to the device where the firmware already learns of them (a scan, the status, the light), so plugins read them and nothing polls. Wi-Fi, Bluetooth, the bonded devices and the power (ops 8, 10, 11, 12) are answered by the kernel from its own models, so they are never a copy in step; the device is told once at start which radios the kernel has, and the Wi-Fi and Bluetooth `caps` bits are set with that. `Kernel::wifi_model` and `Kernel::bluetooth_model` give a shell what plugins see of both.

- The X4 wires `core.sockets` for interface-7 plugins over its own TCP, TLS and UDP code: plain and TLS sockets (the device's TLS session is shared with HTTPS and interface-6 sockets) and broadcasts with answers from address and port. Each report to core is followed by the kernel's tick; the counts of dropped and ignored reports are logged once the sockets are idle. `core.net` and network demand follow.
- The X4 wires `core.net` for interface-7 plugins: the HTTP client (HTTP and HTTPS over the same requests as interface 6, an answer saved by appending to core's staged file, a redirect from `http` to `https` parks the request until core starts it again). Plugin and cable servers are not served yet.

- **XML: entity decoding that reports low memory** (#228). `paperpaw_xml::try_decode` and
  `Attrs::try_get` decode character and entity references with one fallible reservation of at
  most the raw text's length, so a feed full of entities (OPDS) gets an error on a full heap
  instead of an abort. `decode` and `Attrs::get` keep their behaviour.

- Code plugins: the check that refuses a host plugin library with a way to run code while it loads now also reads macOS (Mach-O) and Windows (PE) libraries, not only Linux ones (#233). A Windows library must have no entry point at all; TLS callbacks, delay-load imports and libraries outside the system's and the C runtime are refused, as are initialisers, Objective-C `+load` and libraries besides `libSystem` on macOS. The web emulator loads no code plugins, so it needs no such check.

- Simple home (a store plugin, #236): a calm alternative start screen for those who need only the core. A large clock, a card for the book you are reading (title, author, how far), cards for the library and the store, and a dock of three icons; a quick panel of big round switches (night mode, Wi-Fi, sleep and more, as the device has them) with a bar for the front light, where there is one, and a bar for the text size; and the library as a list of cards, a page at a time. Choose it for the home screen and the panel in Settings. Cards, dock icons (any action of any plugin) and the panel's switches are three menus you edit in *Edit menus*, as you edit the main menu; under *Simple home* you choose whether the date and the battery show. Moving the focus draws only the two parts it touches, and the minute's tick only the clock. Buttons and touch (a tap on a bar sets it).

- **`EventKind::WidgetChanged`** (#243). A native plugin can subscribe to `event.widget_changed` (86) in its declarations, so a widget's owner refreshes its own view when an instance says its content changed.

- **Native pictures can keep their transparency** (#243). `PrepareImage::see_through()` (`image_prepare` flags bit 2) makes a PNG or GIF in gray with how opaque each pixel is. Drawn, it shows what is below through it, as the Beta 3 sleep picture showed the page. A JPEG has no transparency and is made as before.

- **Native plugins can read aloud** (#243). `command.speak` and `command.stop_speaking` run on the same speech the shells give interface-6 plugins, and `event.spoken` tells the plugin that asked, and no other, how its text ended: spoken, replaced or stopped, no voice for the language, or no speech on this device (a device without speech says so at once). A plugin stops its own speech only.

- **`event.document_changed` in the SDK** (#243, #2443): `EventKind::DocumentChanged` subscribes an opener to the host's hint that a document it opened moved in its preparation (`document.md` 2.1), and `Event::DocumentChanged` (`kinds::DocumentChanged`) reads its document, state and progress. A state the SDK does not know reads as failed, as `DocState::preparation` does.

- **`Declarations::reader` in the SDK** (#243): a native plugin declares a reading app (`ReaderDecl`: name, its own screen, the formats it reads; `reader.spec`), as interface 6 `Registrar::add_reader` did.

- **`event.document_closed` in the native SDK** (#243). A plugin that opens documents
  subscribes with `EventKind::DocumentClosed` and gets
  `Event::DocumentClosed { now_ms, document, reason }` when the host ends one of its
  documents: the format stopped, the card went, the book was moved or removed, or the
  place went to a book to show (`DocClosedReason`). Before, it learnt this only from
  `Cancelled` on its next call.

- **`paperpaw-dev build` builds a plugin with code in its own repository into the file a device loads** (#244): `paperpaw-dev build` for the Xteink X4 and X3, `--device x4-pro` for the X4 Pro and X4 Classic. It writes the records of what the code calls, says what is missing of the compiler and how to install it, builds for the chip, has `paperpaw-sign` pack the program and leaves the plugin's folder as it goes onto a card in `dist/<device>/<id>` (its `plugin.ini` and words with `code.ppn`); a packer that is not installed is named with the command that installs it. The code is not signed. `new --code` now writes a repository that builds for both chips (the second chip's build configuration and record), and its `CODE.md` says how.

- **`paperpaw-dev build --device emulator` builds a plugin with code for the emulator** (#244): on Linux (x86-64) it links the plugin's program as the library the emulator loads and leaves `dist/emulator`, a library with the plugin in it; `paperpaw-emu --library dist/emulator --trust-code` starts it as an X4 with the X4's memory. `paperpaw-dev metadata` writes a third record for it, `native/metadata.host.bin`, and `new --code` writes a plugin that uses it; a plugin made before adds the three lines that choose the record in `src/main.rs`. `CODE.md` says how to try a plugin in the emulator and what the packer's two refusals mean. The emulator now gives a plugin library its heap (the heap word `PAPERPAW_HEAP`, filled before the plugin starts): before, a library that allocated ended its start with "no memory".

- **`paperpaw-dev metadata` writes a code plugin's record of what it calls on the device** (#244): `native/metadata.bin` (X4, X3) and `native/metadata.s3.bin` (X4 Pro, X4 Classic) from `native/metadata.json`, outside PaperPaw's repository. Until now only Python tools of the repository could make them, so a plugin that called one more function than the template could not be built elsewhere. The bytes are those the Python tools write: a test compares both records of every plugin of PaperPaw's own. `--check` says whether they are current; a name the interface does not have is refused with the name. The tool carries a table of the interface's schema (`tools/abi-metadata-table.py` writes it).

- **`paperpaw-dev new --github <id> --code` makes a plugin with code for plugin interface 7** (#244). Beside the repository of a data plugin it writes the crate (`native_plugin!`, a screen, an action in the tools menu), the X4's build configuration with the release's linker script, the record of the device's functions the code calls, and its words; `CODE.md` says how to build it (`cargo x4`). `--sdk <path>` builds against a PaperPaw checkout instead of the published SDK. Before, `--code` only said that there was no template.
- **SDK: `DeclareError::to_error()` gives the `native::Error` a plugin's `make` returns** (#244) and keeps the cause: the host's or encoder's status, no memory, or `Invalid` for an item refused before it was sent. The template declares in a function of its own with plain `?` and uses it. `new --code` writes `--sdk` as an absolute path (Cargo reads it from the new repository's folder) and refuses an id that begins with a digit, which Cargo does not take as a package's name.

- **`font.acquire` takes `data:<path>`** (#244). A font pack or font file in the plugin's own data folder is opened when acquired (a pack is read where it is) and counts with the eight font handles; the SDK's `native::fonts::Font::acquire_data(host, path)` builds the key.

- **Pictures: the recipe picture-home needs** (#244, `docs/abi/document.md` section 5). `image_prepare` can turn a JPEG as its EXIF orientation says (`PrepareImage::turn()`, the size reported is the size after the turn) and map every gray through the plugin's own 256-byte table while the picture is decoded (`.tone(&table)`); `event.image_ready` carries the mean brightness of an 8 by 8 grid over the picture (`ImageReady::luma()`), and `draw_image` can stretch to the rectangle (`ImageStyle::stretch()`) or draw black and white dots (`.dots()`) or the panel's grays (`.grays()`). A quarter turn is done in tiles of at most 32 KiB on the X4.

- **`paperpaw_sdk::data::plugin_dir` and `paperpaw_sdk::numbers::parse_f32`** (#244): a plugin's data folder and the SDK's small parser of floats, which were reachable only through `Env::plugin_dir` and the events' `ParamValue`.

- **Annotation transactions on the device host** (#245, `docs/abi/library.md` 2.2). `core.library` ops 17 to 21 and 26 (`edit_begin`, `edit_add`, `edit_text`, `edit_commit`, `edit_abort`, `edit_receipt`) work on the card: the staged edits go to a temporary file, the commit rewrites the book's annotation file as version 3 (ids, `next-id`, generation, client keys) through a temporary file renamed over the old one, and the receipt of the ids is kept until the plugin's next `edit_begin`, 10 minutes or its stop. A power cut leaves the old file whole. The native reader's bookmarks, highlights and notes use them.

- **`event.library_changed` reaches the plugins that subscribed** (#245). `core.library` tells them what changed (books, status, positions, annotations, fields); the changes raised between two passes arrive as one event, and the plugin that changed something hears it too. A plugin may register for it now (the native `files` plugin was refused at admission before).

- **A plugin can move a book in the library on the device** (#245). `core.library` `move` (op 11) hands the move to the card's journalled move job: the file, its details, its position, its annotations and the plugins' book data follow it. A name that a document, the reader, a transfer or the move job holds answers `ERR_BUSY` and nothing is written.

- **A one-time notice when memory is short** (#248). On the X4 family, when the largest free block is under 50 KB with plugins from the store loaded, measured once the start has settled (15 s) with Home on top, no book open (nor a cover or a background job's) and Wi-Fi and Bluetooth off (with Bluetooth on the notice does not come), the home screen shows one notice: "With these plugins memory is short. Pictures and covers may be missing, and books open more slowly. Switch one off under Plugins." with *Plugins* (opens the plugin list) and *Not now*. It comes once for each set of running plugins (again only when the set changes, also after a restart: the card keeps eight digits in `paperpaw/low-memory.txt`), only over the home screen, never over the first-run setup, a book, a dialog or the sleep screen, nor in a call that sleeps, switches off or restarts the device or shows grays. There is no limit on plugins.

- **"Not enough memory for this" when a screen cannot open for a lack of memory** (#248). The user saw nothing before (the log said it): a notice made then would meet the same lack. The kernel now makes this notice ahead, at start and again when the language or any interface setting (`core.ui-*`) changes, and shows it without allocating, in place of a notice that shows when a native screen (a parked plugin's among them) cannot open for a lack of memory. It comes back for the next time when it ends. English and German.

- **Per-plugin heap budget and block owners** (#248). A plugin's block of 256 bytes or more carries
  a header (previous, next, owner index; 12 bytes on the X4) so that `dealloc` and `realloc` follow
  the block's own owner in constant time and the teardown sweep can list what a plugin still owns;
  a smaller block has no header and is charged by its size, rounded up to 8 bytes, to the plugin
  whose code runs (hundreds of small blocks would cost the X4 kilobytes of header). The owner pays
  for what it holds, alignment padding included. `HostWiring::plugin_heap(bytes)` sets the ceiling a
  shell gives each plugin (`PLUGIN_HEAP_SMALL` 48 KiB on the X4, `PLUGIN_HEAP_LARGE` 4 MiB;
  defaults follow the device's memory class). The limit is a ceiling, not a reservation, and may
  change after the device measurement.

- **`event.screen_hidden`** (#248). A native screen is told when a full screen opens above it, so it can let go of what it keeps only to draw itself and build it again on its next `screen_shown`. Plugins that do not read it lose nothing.

- **A screen is told when a draw only counts its pictures** (#250). To decide whether a page is shown in grays the host draws the screen once aside. That draw's frame now carries `id` bit 1 (`DrawFrame::pictures_only()`): a native screen may leave out its text and status lines there, so the reader's sixth draw of a page turn need not cost a whole draw. A screen that ignores the bit draws and is counted as before.

- Native plugins: `command.redraw_area` (SDK `CommandSender::queue_redraw_area`) draws again only a rectangle of the plugin's screen, as interface 6's `Cx::request_redraw_area`. Each rectangle is drawn and weighed on its own, so pictures filled in after a page's text refresh as small areas and do not count as a page turn towards the periodic full refresh. Rectangles asked for together with a whole redraw are drawn in the frame after it, so a reader shows a page's text first and its pictures right after (#252).

- Native plugins: `Documents::ask_image` (core.document op 16) asks the format again for one picture of an open document by its index, for a picture block that came without an image id (none was free, or the picture was not decoded yet) or whose id has ended (#252).

- **Full refresh in menus** (#255, lab). A new advanced Display setting, `core.ui-full-refresh-interval`, counts the refreshes of the user interface (fast ones and those smaller than a page: focus moves, menus, settings, notices) apart from page turns and flashes the whole screen at the n-th, so ghosting no longer builds up where no page is turned. 0 (the default) keeps Beta 3's behaviour; page turns keep their own count, and a full refresh starts both counts anew. While the reading screen is on top, nothing counts there: a clock or progress line never leads to an extra flash.

- **The home status line's *Line* setting has its picker again** (#257). `home.status.line` opens home's native `status-items` screen in a lines mode, as Beta 3 opened `core.status-lines`: it lists the status lines the registry offers, each tap shows or hides one (the first shown that is there is drawn, else the items), the order is changed as for items, *Back* leaves, and the kernel's line of items is still found by its old id `reader.status-line.items`. The three item lists of the status line widget now open the same screen instead of being refused.

- **Menu: tiles on interface 7** (#257). With the menu style *Tiles*, a menu of its own whose every entry has an icon (the main menu) opens as in Beta 3: the reader's bar without its title and page lines, the entries as icons with short labels in even rows at the top of the screen. The buttons go round them (mirrored right to left), *Select* or a tap chooses, the menu button, *Back* or a tap outside close; actions close the tiles and run, switches and choices of two change in place, submenus open as a list over them. A submenu, a menu with an entry without an icon or the *List* style shows the list as before.

- **A book's own settings for native readers** (#257). `CommandSender::set_scope(Some(book path))`
  now works: while the reading screen is open, settings declared `scoped` (font size, margins
  and the like) take the book's own value where "Only this book" is on, and the global one
  otherwise. The host keeps them where Beta 3's reader did (`books/<book key>/settings.ini`
  in the plugin's data), so the settings saved per book in Beta 3 carry over, and moves and
  removes them with the book. A `book_data` folder name of 16 hex digits (a book key's form)
  is now refused.

- The quick panel shows a number that asks for a slider (the front light, the light colour) as one on a touch screen, as Beta 3 did: `−`, a track with a knob and the value, and `+`. A tap on the track sets the nearest step. Without a touch screen the number keeps its `−` and `+`.

- **The reader's stage for marks in the text, `reader.decorations@1`** (#257): its schema and codec, for Kernel review. Plugins (vocabulary first) mark words of a page and hear taps on them, as Beta 3's decorators did; the reader asks and draws them once the schema is approved.

- **Marks in the text from other plugins** (#257): the native reader asks each page of the plugins that subscribe to `reader.decorations@1` (a vocabulary list, say) and draws the backgrounds, lines and frames they answer; a tap on a mark that takes taps goes to the plugin whose mark it is, as Beta 3's decorators did. Not on the X4 until the device runs stages.

- **The native reader declares its first run** (#257). On the native interface the reading screen
  has Beta 3's nine help hints again (the three tap zones, the quick panel and night mode by touch,
  and the buttons), the first-run setup asks for the text size as Beta 3 did, and the sample of
  text above the reading settings is a native widget, set with the reading settings as they are
  chosen. The hints do not yet wait for the book to show, and the row of button hints does not yet
  show only on a press: the native interface cannot declare either.

- **Reader: the reading aids' rules in the native reader** (#257). The native reader asks the `reader.filter` stage when a book opens and again when another plugin's setting changes, checks each answer once and applies bold word beginnings, replaced words and the pictures rule (shown, captioned or left out) to each block before it is laid out, as Beta 3 did. Pages laid out under other rules are counted anew. It keeps only the answers' bytes (at most 3,840 each) while a book is open. Devices that run no stages yet (the X4 until its stack gate) read the book unfiltered.

- **The native reader asks for the letters a book needs** (#257). As in Beta 3, a book in Greek, Cyrillic,
  Arabic, Hebrew or Korean letters that the reading font cannot draw asks the store for the pack of those
  letters when it opens, once for each book; a pack that could not be had is asked for again at the next
  opening, and the book is laid out again once its letters are installed (Korean's rare syllables are asked
  for only then, where the book has them).

- **Settings that ask for a slider say so in the registry** (#257). `reg_setting.flags` bit 11 is set for an int setting whose extra asks for a slider (the kernel's margins and sizes among them), so the native settings and menus show Beta 3's slider; switches need nothing new (bit 6 of `reg_menu_row`).

- **`reg_setting.flags` bit 11 `slider`** (#257, interface 7 contract). The registry tells a settings list which int settings show as a slider (their `setting.extra` slider flag), so the native settings and menu panels draw them as Beta 3 did; how Boolean rows show as switches is spelled out. Documentation only: no record, lock or SDK change.

- **Overlays can fade the page behind them again** (#257). A native overlay screen (a menu, a panel) may be declared with `ScreenDecl::dims_below()` (`screen.spec.flags` bit 10): the host washes out what lies below it while it shows, as Beta 3 did. Without the flag nothing changes.

- **A native plugin's screen can relabel a button in the row of button hints** (#257). `CommandSender::set_hint` with a `native::ui::ButtonHint` sends the existing `command.set_hint`: the plugin's own words (*Try again*), an icon, no hint, or the action's usual name again, while its own top screen shows. No change to the interface: the command was there, the SDK had no method for it.

- **The sleep screen's own logo picture is native** (#257). On interface 7, the message shows the picture `core.logo` names (a JPEG, PNG or GIF on the card, such as a data plugin's `plugins/<folder>/logo.png`) above *Sleeping*, fitted into a square a third of the screen across, as in Beta 3; a picture that cannot be read, or a card the reader gave no consent for, shows PaperPaw's logo.

- **The sleep screen's picker of the picture over the page is native** (#257). On interface 7, *Picture over the page* and *Widget over the page* open the picker as in Beta 3: the pictures folder, the plugins' pictures (presets) and every widget, the chosen one marked; choosing sets it and closes, and why the picture did not show at the last sleep is noted below.

- Vocabulary: the words you are learning are marked in books again (and a tap on one looks it up), now through the reader's new marks stage; their look is set in the reading settings as before.

- Reading apps: where two or more reading apps are installed, Settings → Reading chooses the default one and one for each format, and a book's menu in the library offers *Open with …* (this time), *Always open with …* (kept for the book) and *For this folder …* (kept for the books in its folder) (#260). The choices are kept in one small file on the card, read once at the start. With one reading app, as today, none of it shows.

- **Choose a font for each widget** (#262): every widget on the home and sleep screen can use its
  own font, including fonts from your SD card. Each widget's settings have *Font* (as all
  widgets, as the menus, as the book, or a font family); *Font of widgets* in the home screen's
  advanced settings sets it for all widgets that have none of their own. Text larger than a card
  font has (on the X4 they have 20 to 36 px, so the large digits of a clock) stays in the menus'
  font, and at most three families beside the menus' and the book's are open for widgets at once.
- **Plugins: the font key `widget:<widget id>`** (#262, `docs/abi/screens-input.md` section 4): the
  host gives every widget a plugin declares, that has a settings group of its id, the setting
  `<widget id>.font`, and resolves the key to the font chosen. A widget drawn with the UI kit's
  `widget::Cx` is set in it without a change (`widget::cx::widget_font` for one that is not). No
  record, slot or lock change.

- Reading aids is a store plugin file again (`tools/plugin-files/plugins.txt`): a release builds `aids` for the X4, the X4 Pro and the X4 Classic as an interface-7 file (format 5) and the store offers it with the other store plugins. How the store code plugins of the X4 family are published is in `docs/releases.md`.

- **Interface-7 plugin files for the X4 Pro and X4 Classic** (#263). `tools/plugin-files7/build.sh x4-pro` and `x4-classic` pack every plugin as a target-9 file (format 5, Xtensa) and check it; the release, the store workflow and CI build, check and sign the S3 files beside the X4's. Each plugin's S3 metadata is generated (`tools/plugin-files7/s3-metadata.py`).

- Show a one-time notice on Home after an update when the saved Literata font needs its store pack, with a store shortcut when Wi-Fi is configured. Keep the saved font choice (#258, #265).

- Linux emulator (`--trust-code`): the store installs code plugins as real downloads, like a device does. The emulator has its own catalogue name, `emulator-linux-x86_64`, and a plugin's library (`code-host.so`) carries its author's signature after it (`paperpaw-sign sign-plugin` signs one); it is checked, asked about and installed into the plugin's folder, so its bundled pictures load from there. Code plugins built into the emulator stay as they were.

- **Page layers for plugins: translations, glosses and drawing on the reader's pages** (#284,
  #261). Two new stages of the reader take plugins in again that Beta 3 had as text filters and
  page layers. With `reader.text@1` a plugin sees the text of each paragraph before it is laid
  out, once while the book is open, and may ask for room above and below its lines, replace
  words as shown, set glosses, ruby or phonetics over or under words (a sentence's translation
  runs under the sentence, line after line) and insert small blocks after the paragraph. With
  `reader.layer@1` it is shown each laid-out page once, with the places of its lines and words,
  and answers boxes, lines and text the reader draws over the page whenever it is drawn. Page
  turns ask nothing they asked before; without such a plugin nothing changes. The SDK has them
  as `native::page_layers` (`TextAsk`, `TextAnswer`, `LayerAsk`, `LayerAnswer`), and
  `Stage::run_from` asks one subscriber of a stage again. The sample `samples/interlinear`
  shows a translation line under each sentence of a book from a text file beside it
  (`docs/abi/page-layers.md`). Interface 7 is unchanged: plugins built before run as they did.
- **Building blocks for read-along, windows at a word and split pages** (#284, #377, #378,
  #379). A page layer may follow reading aloud: it is asked again for each sentence, with the
  words being read (`LayerAnswer::uses_reading_aloud`, `LayerPage::spoken`), and marks them as
  it likes. It may give the book's text only a part of the page (`LayerAnswer::region`, in
  thousandths of the text area) and draw a second text, notes or a picture in the rest; its
  lines say where in the book they are, so the plugin follows page turns. It may be shown taps
  (on its part of the page only, or all) and key actions before the reader acts on them
  (`LayerAnswer::input`, `LayerAsk::Tap`, `LayerAsk::Key`), take them (`LayerAnswer::taken`) and
  draw its layer anew, the page drawn again in one partial refresh. The lookup slots get
  the looked-up words' box on the screen (`anchor`), so a dictionary window can open next to
  the word.

- The reader offers the hyphenation pack of a book's language from the plugin store. A book in Danish, German, Spanish, French, Italian, Norwegian, Dutch, Polish, Portuguese, Russian, Swedish or Ukrainian (or English, if its pack was removed), opened with *Hyphenate words* on and no table for its language on the device, asks the store for that language's pack (`hyphenation-de` and the like), once for each book: the store shows what it is and what it takes and downloads it only if you say *Download*. Nothing is installed on its own (German stays optional). Once it is on the card the book is set with breaks at line ends, without a restart. The question the store asks for a book now reads "This book needs more to be shown as it should (letters or hyphenation patterns)".
- The thirteen hyphenation packs of interface 7 (`tools/plugin-files/data.toml`, format 5, version 2.0.0) are what a release's store index lists, in the group *Reading*; the emulator's store test lists and installs the German one from a local library and sets a German page with it.

- **A test script language for the X4 and the emulator** (#310). `tools/x4-remote.py` runs script
  files (`--script-file test.pps`) with variables, loops, named blocks, test points with verdicts,
  built-in measurements (key to first frame, heap, card writes, gray passes), numeric checks
  (`assert:turn.first_ms.median<=1000`), log checks, a lost-link retry and a report
  (`results.json`, `report.md`, before → now against an earlier run). The emulator speaks the X4's
  remote commands too (`paperpaw-emu --headless --repl`, `x4-remote.py --emulator`), so one file runs
  on both. See `docs/device-test-scripts.md`.

- **Better comics** (#312). Comic book archives (.cbz) show their series and number in the library from the book's ComicInfo.xml (title, series, number, writer, language), and read manga from right to left: with `Manga` set to `YesAndRightToLeft` in the book, or with the new setting "Comic pages run" (Reading, Behaviour: as the book says, left to right, right to left), the page turns are mirrored, the left side and key go on. A new setting "Panel by panel" (off to begin with) shows one panel of a page at a time, as large as the screen allows, and the page turns step through the panels in reading order (right to left for manga); a page without panels is shown whole.

- Thai books break their lines between words (#313): the packs of `script-thai` carry a table of 4,203 Thai words (the 4,000 most frequent of the Thai National Corpus, PyThaiNLP's list, CC0 1.0, and the interface's, less the long ones that shorter words make up; `tools/thai-dictionary.py`, slot 6 of a pack), which `paperpaw-text` reads the first time it breaks Thai text into lines (16.0 KiB, held while the pack is open, let go when memory is short). A line breaks after the longest word that another word follows; letters no word explains break between clusters, never between a letter and its vowels or tone marks. The reader, the library and the screens break Thai this way; without the pack, Thai breaks as before (at spaces and zero width spaces).

- Khmer, Lao and Tibetan books (#314): the store's `script-khmer`, `script-lao` and `script-tibetan` bring their letters from Noto Serif Khmer, Lao and Tibetan, shaped as Bengali and Devanagari are. Khmer's consonants subscript under coeng, ro and the vowel signs drawn left of their consonant move before it, its split vowels are drawn in two parts; Lao's vowels and tone marks stack above and below their letter; Tibetan's consonants stack. Khmer and Lao are written without spaces: their packs carry the 3,000 words of ICU's word lists (Unicode License V3) most frequent in their Wikipedias (`tools/word-table.py`), and lines break between them as Thai's do; Tibetan breaks after each syllable's tsheg. Noto Serif Tibetan's stacks are too many for a pack read from a card to hold: its packs keep the 600 clusters most frequent in the Tibetan Wikipedia shaped (`tools/tibetan-clusters.py`; 99.8 % of its text), a rarer one drawn as the longest of those it starts with. The reader offers the packs for books in these scripts.

- New language packs `language-mr` (Marathi) and `language-ne` (Nepali) show the menus and settings in Marathi and Nepali (#317). They are machine drafts of all messages, translated in context; they use the Devanagari letters pack `script-devanagari` like `language-hi` and need no shaping work of their own. Nothing else changes.

- **A template for a community plugin source** (#318).
  `templates/plugin-source` is a repository to copy: an example plugin with
  code for plugin interface 7, and a GitHub Actions workflow that builds it
  for the Xteink X4 and X3, the X4 Pro and X4 Classic and the desktop
  emulator, signs the builds and the list with the source's key (the
  repository secret `PAPERPAW_SOURCE_KEY`) and publishes the source with
  GitHub Pages. Its README says how to make the key and how readers add the
  source on their device (*Plugin store*, *Sources*).
- **`paperpaw-sign pack-source`, `sign-source` and `check-source`** (#318).
  `pack-source` makes a source from the folders of its plugins and their
  signed builds: `plugins.json`, `plugins.json.sig`, a page and the files of
  each plugin, as any web server serves them. It refuses what a device would
  not install, such as a build signed with another key than the list.
  `sign-source` signs a list made another way; `check-source` checks a
  list's signature as a device does and prints the key's fingerprint.

- Interface 7 (additive, `since` 1): `command.show_again` (1598; SDK `CommandSender::show_again`, `cancel_show_again`) lets the sleep screen ask to be shown again while the device sleeps, once or every `every_ms` (at least a minute), and `dev_device.caps` bit 15 `refresh_asleep` (SDK `DeviceCapability::RefreshAsleep`; kernel `Capability::REFRESH_ASLEEP`) says where that works: the X4 and X3 draw the screen again from their light sleep's wake (a partial refresh, never earlier than 5 minutes after the last, never at 10 % battery or less, no radio); the X4 Pro and Classic, and the emulator's other devices, do not. The emulator's X4 and X3 profiles do it on their timer. The sleep plugin's *Redraw while asleep* (off, 15, 30 or 60 minutes; off by default) uses it to keep a clock, calendar or weather widget right, with the cost named in its help (about 0.03 mAh a redraw, an estimate) (#319).
- Interface 7 (additive): input-filter verdict 6 `preview` (`core.input-filter@1` revision 1, optional `key` and `value` of `filter.verdict`; SDK `settings::encode_value`) takes the input and shows a setting's value without a card write until the capture ends; a filter keeps it by answering the up with `run`. The Edge light plugin uses it for a stepless edge swipe: the light follows the finger and one setting write happens when it lifts (#104).

- **`paperpaw-dev sweep`** (#343). One fast pass over every screen the app has, taken from the registry, none left out by a list: each is shown once on the X4 in English and in German on a device of its own, as the buttons lead to it from the home screen, else opened as a plugin would open it. A screen that does not show, draws nothing or fails a call of its plugin while it opens, a part past the screen, text cut without an ellipsis, a translation key shown as it is, German showing the English text of a key it lacks, and (where every way was searched) a screen no way leads to with the buttons are errors that name the screen, and the run fails. It is a command of its own, not part of `check`.

- **The Boox Picco, experimental** (#368). A firmware for Onyx's Boox Picco
  comes with each release: `paperpaw-picco-<version>-install.bin`, flashed
  over USB into the stock firmware's app partition, which leaves Onyx's boot
  loader and recovery as they are. It is built from what CrossPoint Reader
  and the FreeInk SDK found out about the device and has **not been tried
  on a Picco**: it may show nothing or not start. Save the Picco's own
  flash first, the only way back to its stock firmware
  (`shells/xteink/README.md`). It is an X4 Pro with another board: touch,
  the front light, two page buttons, Wi-Fi and Bluetooth; no update from
  the device itself (one app partition), and the store's plugins are the
  X4 Pro's. The emulator has it as the profile `boox-picco`.

- **Plugins can shape how pictures look: the picture stage `core.picture`** (#82, #369). A plugin
  that subscribes to the new stage (at most two on a device) is asked once for each picture the
  host prepares, with where it is shown, its size and a histogram of its grays, and may answer a
  tone curve of 256 grays and whether it is drawn in dots or in grays. It sees no pixels and is
  never asked at a draw, so page turns cost nothing more. The SDK has it as
  `native::image::picture` (`Asked`, `Tone`, `Place`). The host asks for every picture it
  draws by an image id and says where it is shown: a picture on a book's page (with the book's
  path), a page of a comic, a cover, a small picture, a picture of the sleep screen, or a
  picture a plugin prepared for itself. A comic's panels take their page's answer. Without a
  subscriber nothing is read and no plugin runs (`docs/abi/document.md` 5).
  Appended to interface 7; plugins built before run unchanged.

- **A part of a picture can be drawn: `canvas.draw_image_part`** (#312, #92, #369). Interface 7
  has one more host slot, 44: it draws a rectangle of a decoded picture scaled into a rectangle
  and reads only that part from the card (SDK: `Canvas::draw_image_part` with `ImagePart`). A
  format can also answer a picture as a part of a cache file (`fmt.image.part`, stage
  `core.format` revision 1), which the host draws for every reader.
  Appended to interface 7; plugins built before run unchanged.
- **Comics panel by panel write no copies to the card** (#312, #369). A panel is now a part of the
  page's one decoded picture, drawn enlarged by the host; before, every panel shown was written
  as an enlarged picture of the page's size (about 384 KB a panel on the X4). Turning to the next
  panel is quicker and wears the card less.

- **Turn wide pictures** (#92, #369). A new reader setting (Reading behaviour, off to begin with):
  a wide picture shown full screen (hold Select on a page with a picture) is turned a quarter so
  that it fills a tall screen. The host turns the picture once, when it is asked for, not at every
  draw: `Documents::ask_image_turned` (op 16 `flags` bit 0) and `PrepareImage::turned`
  (`image_prepare` bit 3) in the SDK.
  Appended to interface 7 (flag bits and an optional field); plugins built before run
  unchanged.

- Native reading apps: `command.pass_on` (SDK `CommandSender::pass_on`) passes a book the app cannot open after all to the next reading app that reads it, in the order of #260 and then any other, leaving out the ones that passed it on; with none left the reader closes with a notice. The reader passes on a book no format of it reads (#369 G, #260).

- `net_request.flags` bit 1 `foreground` (SDK `Request::foreground`): a download the user waits for gets the X4's larger receive window and write chunks whatever its file cap, honoured only from a screen of the plugin that shows. The store asks for it (#369 H, #216).

- `event.display_changed` (112, SDK `Event::DisplayChanged`): the display's new size and ppi after a turn or a resized window, for native plugins that subscribe (#369 K, #88).

- The environment snapshot grows by an extension: `hints_shown` (SDK `Snapshot::hints_shown`) says whether the host's row of hints was on screen when the screen was last drawn, so a screen whose first press only shows its bars no longer keeps a timer of its own. Picture Home's bar uses it. Older plugins read the longer snapshot unchanged, and one that asks for exactly the 52-byte first layout gets that prefix without the extension: extensions are appended and only written when the buffer has room for them; only a buffer below 52 bytes is refused (#369 L, #205).

- `doc_link.kind` 3 `file` (SDK `LinkKind::File` in documents and formats, `FormatProvider::with_file_links`): a link to another file on the card. Wikilinks of a notes vault to its other notes are such links and the reader opens them (#369 M, #87).

- `lib_state.state` 3 `failed` (SDK `CatalogPhase::Failed`): the catalog is there but cannot be read; the library says so instead of showing no books (#369 N, #145).

- **Read-along: mark the sentence being read aloud** (#377). The sample plugin `samples/read-along` follows reading aloud on the reader's page layer (#284): for each sentence read it draws a frame around the words being read on each line of the sentence, or a thick line under them (setting *Mark the sentence read aloud*), and the page is drawn again in one fast refresh. Nothing is drawn or asked while no book is read aloud, and with *Not marked* the plugin does not follow reading aloud at all. A word-by-word mark needs the host to say where in a sentence the voice is, which interface 7 has not (the host's speech reports a sentence at its end); interface 7 is unchanged.

- **The dictionary, Wikipedia and translation windows open next to the word** (#378). When a word is looked
  up in the book, the window with its entry goes right below the word, or above it where there
  is no room below, and never covers it, instead of always rising from the bottom of the
  screen. A word picked in the lower half of the page gets the window above it. The page stays
  as it was; closing the window brings it back. *Online ›* in the dictionary opens Wikipedia in
  the same place. The translation window does the same where it is opened with a place (the
  param `anchor` of the hand-over, as for the lookups). Where there is no place (a lookup from a
  list) the window is the sheet at the bottom as before.
- **Translate in the reader's selection bar** (#378). The bar of chosen words has *Translate* after
  *Look up* where the translation plugin is installed and on; it opens the translation of the
  chosen words in a window next to them (and the key action *Translate* does the same). Where the
  plugin is off there is no button.

- Gloss (a store plugin, #379): small text above the words of a book, for those who learn a language: how a word is read (furigana, pinyin, a pronunciation) or what it means in a word or two. The words come from a plain list on the card, `.paperpaw/plugins/gloss/<book>.txt` for one book or `<language>.txt` (`de.txt`) for every book in a language: a line each, the word, a tab, the small text. The list is read once when the book opens, so turning pages reads nothing. *Settings → Reading aids* sets the size of the small text. It uses the page layers of interface 7 and is off until you switch it on.

- Tap zones for reading (#381): in Settings › Controls, on touch devices (the X4 Pro), choose where a tap turns the page or opens the menu: thirds with the menu in the middle (as before), halves with a menu strip at the top, an L-shape for one hand (the right and bottom edges go on), or top and bottom; *Swap left and right taps* mirrors any of them. Your own button and gesture changes still win.

- 22 new UI language packs show PaperPaw's menus and settings in Catalan (`language-ca`), Croatian (`language-hr`), Serbian in Cyrillic (`language-sr`), Slovenian (`language-sl`), Lithuanian (`language-lt`), Latvian (`language-lv`), Estonian (`language-et`), Malay (`language-ms`), Filipino (`language-fil`), Swahili (`language-sw`), Afrikaans (`language-af`), Bosnian (`language-bs`), Macedonian (`language-mk`), Belarusian (`language-be`), Albanian (`language-sq`), Icelandic (`language-is`), Basque (`language-eu`), Galician (`language-gl`), Welsh (`language-cy`), Irish (`language-ga`), Kazakh (`language-kk`) and Central Kurdish, Sorani (`language-ckb`, right to left) (#384). They are machine drafts of all messages, translated in context, each with its own plural rule, date and number formats and a word clock in the language. The Cyrillic ones need the Cyrillic letters pack, Sorani the Arabic letters pack; the others use the letters PaperPaw has built in. Nothing else changes.

- The Georgian letters pack (`script-georgian`, Noto Serif Georgian, with the Mtavruli capitals upper case makes) and the Armenian letters pack (`script-armenian`, Noto Serif Armenian) draw books in these scripts (#385).
- 2 more UI language packs show PaperPaw's menus and settings in Georgian (`language-ka`) and Armenian (`language-hy`) (#385). They are machine drafts of all messages, translated in context, with the language's date and number formats and a word clock in the language; each needs its letters pack. Nothing else changes.

- Letters packs for Gujarati (`script-gujarati`), Gurmukhi (`script-gurmukhi`, Punjabi), Odia (`script-oriya`), Tamil (`script-tamil`), Telugu (`script-telugu`), Kannada (`script-kannada`), Malayalam (`script-malayalam`) and Sinhala (`script-sinhala`), from their Noto Serif faces: their conjuncts, reph, forms below, after and before the letter, vowel signs drawn left and split ones are set as HarfBuzz sets them (#385).
- 8 more UI language packs show PaperPaw's menus and settings in Gujarati (`language-gu`), Punjabi (`language-pa`), Odia (`language-or`), Tamil (`language-ta`), Telugu (`language-te`), Kannada (`language-kn`), Malayalam (`language-ml`) and Sinhala (`language-si`) (#385). They are machine drafts of all messages, translated in context, with the language's date and number formats; each needs its letters pack. The word clock speaks the language; its letter grid stays English's, as Hindi's does.

- The Latin Extended letters pack (`script-latin-extended`) draws the letters Azerbaijani (ə), Hausa (ɓ ɗ ƙ ƴ) and Yoruba (ẹ ọ ṣ, ǹ ḿ and the combining tone marks) add to the Latin alphabet: from Literata and Source Sans 3, with what those lack from Noto Serif and Noto Sans (#385).
- 3 more UI language packs show PaperPaw's menus and settings in Azerbaijani (`language-az`), Hausa (`language-ha`) and Yoruba (`language-yo`) (#385). They are machine drafts of all messages, translated in context, with the language's date and number formats and a word clock in the language; each needs the Latin Extended letters pack. Nothing else changes.

- **`Show::Opened` for library queries** (#393). A query can list only the books opened at least once (being read or finished), as a list of the books read last needs them, without reading every other book's row. A host before it refuses the query with `Error::Invalid`.

- 7 more UI language packs show PaperPaw's menus and settings in Amharic (`language-am`), Javanese (`language-jv`), Somali (`language-so`), Uzbek (`language-uz`), Zulu (`language-zu`), Kyrgyz (`language-ky`) and Pashto (`language-ps`, right to left) (#394). They are machine drafts of all messages, translated in context, with the language's date and number formats and a word clock in the language. Amharic needs the Ethiopic letters pack, Kyrgyz the Cyrillic letters pack, Pashto the Arabic letters pack; the others use the letters PaperPaw has built in. Nothing else changes.

- A UI language pack shows PaperPaw's menus and settings in Mongolian (`language-mn`, in Cyrillic letters, as Mongolia writes it; it needs the Cyrillic letters pack) (#397). It is a machine draft of all messages, translated in context, with Mongolian date and number formats and a word clock in the language. Nothing else changes.

- Burmese letters (`script-myanmar`, Noto Serif Myanmar with its words for breaking lines) and the interface in Burmese (`language-my`, a machine draft; it needs the letters) are in the store (#401, #397). Burmese words are shaped as HarfBuzz shapes them (2,000 of 2,000 of the most frequent).

- Picture books and math formulas in EPUB books: the new store plugin *EPUB: picture books and math* shows fixed-layout EPUBs (picture books, comics) page by page, each picture whole on its page, in the book's own order and direction, and reads MathML formulas as plain text (fractions as a/b, powers as x², roots as √(x), sums with their limits; a formula on its own line stands as its own centered line). Once installed it opens your EPUB books instead of the built-in reader (#409). The built-in EPUB reader is unchanged and costs no more flash or memory.

- **Card plugins on the X4 family are offered the services the built-in plugins have** (#2850). A plugin file with screens and settings of its own (`core.ui`, `core.registry`) is no longer refused for a service the firmware did not list; `core.net`, `core.sockets`, `core.work`, `core.document`, `core.library`, `core.files` and `core.secrets` are offered as far as the start wired them, and a plugin gets only the ones its metadata declares. Of the consent features a card plugin may use `uses.network`, `uses.storage` and `uses.library`; `uses.secrets` (which reaches other plugins' secrets), `uses.registry`, `uses.power`, `uses.wifi`, `uses.install`, `uses.firmware` and `uses.bluetooth` stay with the built-in plugins, and a card plugin that asks for one does not get it. `core.secrets` is wired before the first card plugin is admitted, not after. A card plugin whose folder is named like a plugin built into the firmware (`wifi`, `settings`, …) is refused with a notice in the log: the id keys secrets and files. Such a folder loads nothing, not even its translations, and the plugin list keeps the built-in plugin's own row (off and uninstall there act on the built-in plugin, never on the folder).

- **Card plugins that declare screens, settings and actions load on the X4 family** (#2850). A plugin file of interface 7 is hosted for the kernel to admit with everything it declares, all or none, as on the desktop; before, the X4 refused every plugin that declared anything. Each entry into a card plugin's code names it to safe mode and checks the stack's end afterwards; a refused entry (the band is not armed) runs nothing. The firmware now hands plugins its heap table, so plugins built with the SDK's default heap word are no longer refused.
- Plugin images are placed once per start: after the start's last one nothing is placed or taken back, so a hosted image stays where it is until a restart.

- Added the UI language packs Czech, Danish, Finnish, French, Hungarian, Indonesian, Italian, Norwegian Bokmål, Dutch, Polish, Brazilian Portuguese, Slovak, Swedish and Turkish as data plugins (`language-<code>`) for the store, and completed Spanish and Arabic against the current English texts, including the kernel's settings and install dialogs. The packs are machine translations, marked as drafts in their descriptions (#3535).

- Added the UI language packs Romanian, Greek, Bulgarian, Ukrainian, Russian, Hebrew, Persian and Urdu as data plugins (`language-<code>`) for the store. The Greek, Bulgarian, Ukrainian, Russian, Hebrew, Persian and Urdu packs need the matching letters pack (Greek, Cyrillic, Hebrew or Arabic letters); Hebrew, Persian and Urdu run right to left. The packs are machine translations, marked as drafts in their descriptions (#3563).

- Store: joplin, browser, read-later-cloud, readwise, wallabag and mail are back in the interface-7 publish set (#3597).

- Added the Vietnamese letters pack (`script-vietnamese`: Literata and Source Sans 3 faces for ơ, ư and U+1EA0–1EF9, new character set `vietnamese`) and the Vietnamese UI language pack (`language-vi`, a machine translation, a draft) for the store (#3597).

- Added the letters packs `script-chinese-simplified` (the 6,763 ideographs of GB 2312), `script-chinese-traditional` (the 5,401 frequently used characters of Big5) and `script-japanese` (kana and 2,998 kanji) in Noto Serif (SIL OFL), in four sizes, the new character set `kana` and the lists `tools/fontgen/sets/{hanzi-gb2312,hanzi-big5,kanji-japanese}.txt`; and the UI language packs `language-zh-hans`, `language-zh-hant`, `language-ja` and `language-ko` (machine translations, drafts) for the store (#3645, #173, #197, #180, #184).

- Language packs: `tools/glyph-coverage.py` (run by `language-pack.py check` and the quick check) checks that the interface font plus the required script pack can draw every message. Romanian now requires Latin Extended letters (ș ț added to that pack); arrows, № and foreign-script language names that drew as boxes are reworded in fi, hu, tr, bg, uk, ru, zu.

### Changed

- Interface-7 slot 6 `i18n.copy` returns `ERR_NOT_FOUND` for a missing translation key,
  leaving output unchanged, instead of copying the key (D5, #228).

- Background-work notes are cut to 256 UTF-8 bytes at a character boundary when stored;
  interface-6 `env.work().note()` readers see the same bounded text (#228).

- **Plugin interface 7, draft 1** (#228, early raise on `abi7/integration`): `native::INTERFACE` is 7 and
  every plugin with code needs a build for it; plugins built for interface 6 are refused as built for an
  earlier interface. Until it is frozen, a change plugins rely on takes the next draft, named in
  `abi/interface-7/blessings.txt`; CI (`tools/abi-reference-check.py`, replacing `abi-frozen6-check.py`)
  refuses an unnamed one against the PR's base. The temporary partition of the interface-7 host table
  from the frozen interface-6 files is retired.
- `DATA_FORMATS` is 5: data plugins may name hyphenation tables in `[hyphenation]` (`hyphenation/*.pphp`),
  so the 13 hyphenation plugins install.
- **Plugin interface 7, draft 2** (#228, `abi/interface-7/raise.md` row 2): hyphenation from data plugins is
  used on a page. `Hyphenation::get` returns a `Hyphenator` (`with_minimums`, `hyphenate`) instead of
  `&Patterns`: a table compiled into a plugin, or a data plugin's table read from the card a few blocks at a
  time, which comes first. At most two card tables are open, within 48 KiB of heap together, checked
  before anything is taken (about 20 KB for `de`, the largest); only a table idle for about a page of words
  is closed for another, so two languages in use never close each other at every word. The kernel opens
  them (an opener it sets with the storage), so plugins built without the `kernel` feature hyphenate with
  them too, and closes them when a book is closed, before sleep and before Wi-Fi, network, store-install
  and firmware work (`Hyphenation::close_tables`). A read that fails closes the table and the next word
  opens it once more, a second failure gives the language up until the card is mounted anew. `PageType::patterns` returns the `Hyphenator`. The kernel keeps the
  data plugins' tables in the SDK's `Hyphenation` (the greatest plugin id has a language).
  `paperpaw-hyphen` (kernel): `StoredPatterns::open_within` refuses a table over a heap budget
  (`OpenError::OverBudget`), `StoredPatterns::heap_bound` bounds it from its header.

- **One look across plugins: the same progress bar, cover placeholder, buttons, skim slider with chapter ticks and focus frames on every screen** (#15, owner OK, Decision 255). The reading bar on the home card, the library and the update screen is one bar (an outline with the part read inside it, rounded as the theme's corners are). A book without a cover shows one placeholder (a frame with the title, or its first letter where the frame is small), in the library, on the home shelf and in the shelf widgets. Buttons of the file manager, setup, the lock screen, flash cards and the self-check follow the theme's button look. The reading skim bar is a slider like the one in the quick panel, with the chapters as ticks. The chosen cell of the library's cover grid, of the lock keypad and of the store's letter list is the theme's focus, and the frame round the chosen widget, card or menu button is the theme's ring. Bottom panels (the skim bar, footnotes, lookups) share one edge. The file manager's note and the store's hint look like the notes under other lists. On themes of pills the keyboard's text box has round ends.

- **Fewer card commands for folders and reads** (#54). A new folder is cleared in one command instead of one for each block, and a block of a file read twice is not asked of the card again: the Library's first start and walk of 14 books sends about a fifth fewer commands.

- The WebDAV address, account and listing code that Picture frame had is now shared in the UI kit (`paperpaw_ui_kit::dav`) with File sync.

- The README says the Linux emulator and tools need glibc 2.35 or newer (Debian 12, Ubuntu 22.04); the release builds them on Ubuntu 22.04 and `tools/check-glibc.sh` fails the build when they ask for more (#165, #125).

- The store plugin update workflow can publish a batch of interface-7 code plugins from the current base against the released firmware (`plugins`: ids, `changed` or `all`), and `tools/plugin-files/store_consistency.py` refuses a changed plugin without a version bump and a plugin or file whose ABI the released firmware would refuse. It publishes nothing; how it is run is in `docs/releases.md`.

- **Large scripts (Korean, Chinese, Japanese) read their letters from the card with fewer reads and less memory churn** (#203). A letters pack of thousands of characters can now store each glyph in a cell of one size (`paperpaw-fontgen` does it for packs of 1,024 characters or more): the card gives a glyph's shape and bitmap in one read, not two (a Chinese page turn in the emulator: 136 reads, from 319). While such a pack is open, the glyph cache is 24 KiB on the X4 (it stays 12 KiB for Latin and the alphabets, which cost no memory more), and a glyph the cache lets go hands its memory to the next one read, so that a full cache allocates nothing. Packs made before read as they did, in two reads. No change for Latin books.

- Faster downloads of plugins and language packs over Wi-Fi (#216): the radio stays out of power saving for two seconds after the last request, so the next file of a plugin or language pack no longer waits for the router's beacon; over TLS the connection's bytes pass up to 16 KB at a time instead of 2 KB for each pass of the main loop; a large download (32 KB or more) gets a larger receive window (24 KB on the X4, 32 KB on the X4 Pro and Classic) and larger card writes (8 or 16 KB), only while it runs and only where the memory keeps a free block of 40 KB; consecutive requests to the same server reuse one connection (kept for three seconds), so each file no longer pays for a new TCP and TLS handshake; and a download is written to its `.part` file with plain appends and one sync before it takes its name, with no temporary-file write for the first chunk. Nothing changes for plugins.

- `tools/plugin-files/published.py` downloads the published files through GitHub's API with the job's token (`GH_TOKEN` or `GITHUB_TOKEN`) when there is one, so releases of a private fork or a test repository can read their own earlier assets; without a token it asks the public address as before.

- Wi-Fi: after a search the Wi-Fi page and the list of networks draw only the rows that say something else (a signal that moved, a network that is kept now) and refresh just those; nothing is drawn where no visible row changed. Before, every such result drew and refreshed the whole list (a broad frame of about 1.6 s on the X4). The whole list is still drawn when the number of networks changes. (#223)

- **Shorter rotation chains per install** (#228). A store install carries at most 6 key
  rotations (Beta 3: 8), so the install, its author and its rotations fit one 8-record batch; a
  catalog entry with a longer chain ends with reason 6 (key changed). At most 8 revocations go in
  one batch (was 16), more go in later batches.

- **The reading aids on interface 7** (#228). The plugin has a native form, which declares the three
  settings for the native plugin interface; the reader applies the rules once it calls the plugin's stage.

- **The reading aids answer the reader's rules stage** (#228). On the native interface the plugin
  subscribes to `reader.filter` and answers with the rules of its three settings (bold word
  beginnings, replaced words, pictures kept, captioned or left out; the caption word of the device's language). A configuration the answer cannot hold is refused, not cut short. The reader applies the
  rules once its native screens ask the stage.

- **Reading aids are interface 7 only** (#228). The interface-6 plugin and its text filters (bold word beginnings, replaced words, pictures) are removed from the app and the plugin file build; the native plugin declares the same three settings and answers the reader's `reader.filter` stage. The rules apply again once the reader asks the stage (`BLOCKED READER-filter-fire`).

- **`book_data.spec` names are one folder name** (#228). A plugin's book-data name is 1 to 96 bytes of `a-z`, `0-9` and `-`, and a plugin names a folder once whatever its kind; anything else (empty, too long, an upper-case letter, `/`, `.`, `..`) is refused at admission with `ERR_INVALID`.

- A native plugin can register for `event.book_opened`, `event.book_closed` and
  `event.book_finished` (65 to 67), so Rules and Setup load. Nothing is
  delivered until the reader port produces them (`core-events.md` 3.1).

- A native plugin can register for `event.book_moved` and `event.book_removed`
  (68 and 69) as well as the other book events, so a plugin that subscribes to
  them (the reader) is not refused. Nothing is delivered until the library
  moves and removals raise them (`core-events.md` 3.3).
- A native plugin can register for `event.stack_changed` (81, a hint) too, so
  a plugin that closes what a covering screen hides (help) is not refused. The
  hint is raised when the stack path reports it (`ui.md` 5.1).

- **The browser on interface 7, last part** (#228). Books go to the library, fonts to the fonts
  folder and other files to `Downloads`, each under a free name, and a saved book can be read at
  once. Books keep the server's file name until the document service tells their author and title.

- **The browser runs on interface 7 only** (#228). The interface-6 browser is gone: the start screen, the pages kept for reading, the bookmarks and history, and the downloads (books to the library, fonts to the fonts folder, anything else to `Downloads`) are the native plugin's. The texts are the same in English and German. The browser is no longer built into the app or offered as an interface-6 plugin file; it is switched on in the plugins like the other native plugins.

- **The browser on interface 7, second part** (#228). The start screen with the address, the
  bookmarks and the pages read lately, the choices on a held entry, loading an address and showing a
  page in the reader are native now. Downloads of books, fonts and other files follow.

- **The browser's rules and lists on interface 7, first part** (#228). Addresses and searches, the
  kind of an answer, a page's title and kept copy, and the bookmarks and the history are in place
  without the host and refuse cleanly when memory is short; an address no request can carry is
  turned away at once. The screens follow.

- **The German texts of the plugins built in are one catalog, kept in the program** (#228, #248). The English catalogs of the first-party plugins were already indexed together at build time; the German ones now are too, in a table of their own (the builder takes one `BuiltLanguage` table for each language). Starting in German no longer makes a catalog for each plugin, and the texts with escape sequences are no longer copied onto the heap. A source a table cannot take (a key another has, a direction or digits of its own, another name) stays a catalog of its own, as before.
- **paperpaw-i18n:** new `BuiltLanguage` (language, name, catalogs), `BuiltLanguageTable` and `Translator::built_language`; `BuiltCatalogs` and `BuiltTable`'s fields are unchanged, and `BuiltTable::add` now also leaves out a catalog with another plural rule or a direction, script, digits or calendar of its own (additive; used by the app's build script and the core builder, no plugin).

- **The wireless library (Calibre) runs on interface 7** (#228). While its screen is open the device finds Calibre in the network (or connects to the address you set) and Calibre lists, sends and deletes books as with a cable; books are put in the library folder through the library, and a book Calibre sent before is replaced while one you put there yourself never is. The password is kept among the secrets. The old interface-6 plugin is removed. What Calibre knows about a received book (title, author, series, tags) is not taken over until the library offers it.

- **`Hooks::call_begin` can refuse a call** (#228). The shell's hook before plugin code runs (`make`, a callback, the destructor) now answers whether the call may run: `false` means the plugin is not run and no `call_end` follows. A refused `make` is `Error::Refused`, a refused callback a busy answer (nothing of it changed, the caller may try again), and a refused destructor keeps the owner as a leak and tells `Hooks::leaked`; none of them panics. The X4 uses it to refuse plugin code at an interrupt level that must never run it (`abi/heap-word.md`); the shells that never refuse answer `true`.

- A native plugin's service call no longer needs a second buffer on the host: the reply is built
  straight in the plugin's output buffer (up to 4 KiB less heap at the peak of every call on the
  X4). After a service's own failure the reply room of that buffer is cleared; a refused call still
  leaves it untouched, and an input that overlaps the output is refused (`ERR_INVALID`).

- **A callback may stage 64 commands** (#228). The per-callback command limit
  of the native command path is the named `CALLBACK_COMMANDS` (64, was 8), so a
  profile apply fits one callback; the 4,096-byte budget of copied keys and text
  and the wire batch cap are unchanged. The 65th command, and bytes over the
  budget, are `ERR_NO_SPACE` with nothing more staged; a callback that returns
  an error still drops the whole stage.

- The host's two confirmations, a question of kind 2 and the firmware confirmation, now draw, scroll and move their focus through one shared box (`dialogs::ConfirmBox`); what their buttons mean stays with each screen. The firmware confirmation's focus changes redraw only the buttons concerned, as the question's do.

- **Interface 7 draft 28: the contract for images, hyphenation, covers, icons,
  setup steps, book badges and lookups that fetch** (#228). Plugins can read
  picture sizes and draw pictures, ask for hyphenation points, cover text and
  named icons through new host slots; setup steps and book badges have their
  host records; a lookup provider that must go online answers "pending" and
  tells the reader when its answer is ready instead of blocking it. Staged
  downloads now use `.pp-staged` names, so a user's own `.part` file is never
  overwritten. `EventKind::Answered` is gone: answers to a plugin's own
  questions arrive without subscribing.
- Interface 7 host slots 36 `image.info` and 37 `canvas.draw_image`
  (`docs/abi/document.md` section 5) and `doc_open` root 5, a file handed over by
  the plugin that opened the reader (section 2.1). Hosts answer `ERR_UNSUPPORTED`
  until wired (#228).
- Interface 7 host slot 38 `text.hyphenate` (`docs/abi/document.md` section 6.1):
  the host hyphenates a word from the card tables of `[hyphenation]` data
  plugins; the reader holds no table (#228).

- **Interface 7 draft 29: widgets keep their least size, screens label their
  buttons, and the X4 Pro and X4 Classic get their plugin profile** (#228).
  A widget declares the least columns and rows it is shown in and whether it
  stays square (`WidgetDecl::min_cells`, `square`), and home screens read them
  (`RegistryWidget::cells`, `min_cells`, `square`), as interface 6's size hint
  did. A screen relabels or hides the button hints under it
  (`command.set_hint`, interface 6's `Screen::hint`). Setting choices hold up
  to 64 values, the environment snapshot carries the user's UTC offset, font
  faces carry their weight, and `Native::minor` says which interface-7 minor
  the device loads, so a store can tell which plugins run on it.
- Interface 7 Xtensa profile `esp32s3-xtensa-lx7-ilp32-windowed-le`, target 9
  of format 5, for the X4 Pro and X4 Classic (`abi/entry.md`), and the S3's
  stack band design (`abi/heap-word.md` section 6) (#228).
- Contract text: books open at a place, block direction, writing mode,
  language and annotations in documents, and library paths that never reach
  the plugins' or the kernel's folders (#228).

- **Interface 7 draft 30: screens know what the user used last, typed text
  stays private, and pictures too large for good are told apart** (#228).
  `Event::ScreenShown` says whether buttons, touch or a pen were used last, so
  a screen shows a focus at once only for buttons. Typed text from a keyboard
  or a remote (`Event::TextTyped`) never shows in debug output. The image
  decoders answer `ImageError::TooLarge` for a picture that can never fit, and
  keep `OutOfMemory` for memory that is short now, so a reader keeps the
  alternative text for good only for the first. `ImageFile::try_open` and
  `Decoded::try_open` open a decoded picture without aborting when memory is
  short. Lookup providers that went online tell the reader when their answer
  is ready (`Event::LookupReady`, `CommandSender::lookup_ready`), and
  `Font::covers` says whether a font has the glyphs for a text. Each stage
  codec has a `ReplyWriter` and an `InputWriter` that write a batch record by
  record, so a format plugin answers 64 rows without a slice of them on the
  stack.

- **Plugin interface 7, draft 31** (#228): one bless for the approved SDK pieces of the carrier: server events, pictures (`image_prepare`, `ImageReady`, `Canvas::draw_image`), book covers, the `core.document` client (opening books, readers for "Open with"), annotation anchors and edit transactions, book data folders, format blocks, deleting and importing books, firmware installs from plugins (`install_firmware`, `firmware_restart`) plugin servers (`Net::serve`, `respond`), file checks before an install (`check_file`), and `Storage::same_entry`. Plugins built for draft 30 must be rebuilt.

- **Plugin interface 7, draft 32** (#228): one bless for the approved SDK pieces of the carrier: what a firmware update leaves out (`core.device` op 16), widgets that preview a group of settings found through the registry (`previews`), the formats the reader knows (`Documents::formats`, `format_for`), metadata other plugins provide for books (`Library::provide`, as Beta 3: the book's own metadata wins where it names a field), pictures from a plugin's own install folder and a "not found" status for missing pictures, and replies written straight into the host's buffer (`Reply::write_with`). Plugins built for draft 31 must be rebuilt.

- **The cover job goes slower on low power and stops after 10 minutes per book** (#228, #245). Between two steps of the background cover job the device waits 1 s in the power modes Saving and Offline (100 ms at Normal). A book's job may run 10 minutes in all per start of the device (time before a displacement or a sleep counts); after that the book gets no cover until the next start.

- **The cover job waits while the library list is open** (#228, #248). The background conversion of a DOCX or MOBI book for its cover takes no conversion step while a screen of the files plugin is on the stack, and for 2 s after, so its memory peak no longer comes beside the list (a book that is ready is not held). The time it waits does not count toward the book's 10 minutes.

- **A plugin refused at a declaration cap says which in its `reg_plugin` row** (#228, `docs/abi/registry.md`). When a plugin's declarations are refused at the format or reader cap (8 or 32 formats, 4 or 16 readers), the plugin is listed as failed with `problem` 14 (too many formats) or 15 (too many readers) and `problem_arg` the cap it hit, so a plugin manager can say why. The cap, not the status, decides the value (a plugin's fifth reader is `ERR_INVALID`, the others `ERR_FULL`). The host keeps it for sixteen plugins at most and forgets it when the plugin is tried again; a refusal it cannot keep shows the general failure as before.

- **Looking a declaration up by name in a plugin's image is about 45 % cheaper** (#248, #228). A plugin whose declarations stay in its image (no index kept on the heap) was read once per batch for every lookup, and every record of the kind was decoded to compare its name. A lookup now reads the image once, walks each batch's frames once and decodes only the records whose bytes hold the name. Opening the settings group of the stats plugin (93 widget settings) costs 3.05 instead of 5.57 thousand million instructions (emulator only, development build); the answers are the same. A record that cannot hold the name is no longer decoded, so a decoding error in it is not reported by a lookup of another name (it still is where the records are listed).

- Native plugins whose declarations are all read from their read-only
  image (linked plugins and mapped card modules) keep no per-plugin index of
  their records any more: their records of a kind are found by walking the
  image's batches, which saves about 6.6 KiB of heap on an X4 with the
  linked plugins (#248). Records are visited in the order the plugin
  declared them, for every plugin; lists built from them (e.g. the widgets a
  place can show) follow that order instead of a name-hash order.

- **English hyphenation is put in place wherever it is missing.** Hyphenation comes from data plugins only (the English one is `plugins/hyphenation/`); the web flasher puts it on a new card, but an update of Beta 3 (English built in) leaves none, and the emulator, the Android app and the web page had no card to bring it. All of them now put the pack there once, from the program itself, at the first start that finds none (`paperpaw-default-packs`): the X4 family after an update or in any way other than the flasher, the desktop emulator in its library folder, Android in its app folder, the web page in its files. A pack that is already there (the flasher's, a newer one) is kept as it is, and one a reader removes stays removed. A power cut while copying leaves no half plugin and the next start finishes.

- **The emulator's linked-plugin registration is split** (#228). `register` in `shells/desktop/src/linked.rs` was over clippy's 100-line limit with `native-linked`; the part that says which services are wired is its own function. No change of behaviour.

- **The desktop emulator offers `uses.wifi` to linked native plugins** (#228). On a profile whose device has Wi-Fi (such as the X4 profile), the pretend radio answers the radio commands, so the `wifi`, `transfer` and `diagnostics` plugins are admitted; profiles without a radio still refuse a plugin that needs it.

- The library reads books' details on its own only while home or the library is shown and no download or install runs, so another screen's memory peak (the store's, say) never meets it (#248).

- Less memory while the library reads a book's details on its own: the host holds a reply buffer sized to what the format answers (a book's state, its details) instead of 4 KiB (#248).

- **`paperpaw-dev` lists the widgets of native plugins.** Its gallery, lint and text checks used the interface-6 widget list, which no longer holds first-party widgets. They now ask `Widgets::everywhere()`. Measuring a widget's grid cell still used an interface-6 probe widget; four tool tests wait for a native probe.

- `core.device` answers Wi-Fi (op 8), Bluetooth (op 10), the bonded devices (op 11) and the power level and mode (op 12) from the kernel's own models on every device, so a plugin that reads right after another changed a radio sees the new state. Each plugin lists only the bonded devices given to it; the Bluetooth manager lists all of them.

- **Diagnostics keeps its big buffers off the stack** (#228). The device reads and the step results of the device check now live in memory the plugin asks the host for, and the heavy parts of showing a screen are separate calls, so the plugin's event handler needs well under the X4's stack allowance (it was over it, and the plugin could not be packed). The check behaves as before.

- **Language packs Arabic 1.0.4 and Spanish 1.0.2** (#228). Their copy of the diagnostics
  text names the new report path.

- **The native dictionary waits for lookups that answer later, and runs as a native plugin with the native reader** (#228). Where a lookup service answers later (it must fetch), the panel says "Looking up …" as Beta 3 did, shows the answer when it is ready on the page it showed, and stops waiting after 20 seconds. Builds that link the native reader list the native dictionary in place of its interface-6 form, with its settings kept in the program.

- **The dictionary is a native plugin only.** The old interface-6 form of the dictionary plugin is removed; the native plugin looks words up as before.

- **The dictionary starts its move to interface 7** (#228). A native lookup provider and the folder setting sit beside the old code; reading the dictionaries, the definitions panel and background indexing follow in later slices.

- **Digests wipe what they held of a password** (#228). `Md5`, `Sha1` and `Sha256` from
  `paperpaw-digest` (also `paperpaw_sdk::digest`) wipe their state when they go, also after
  `finish` and `hex`, and the block copies and message words of each step, so a password hashed
  for a sync key is not left in their memory.

- **A bad `doc_toc` or `doc_link` row is dropped, not the page** (#228). A format's table-of-contents row with a level over 15 or an unknown flag, or a link row with a kind over 2 or `end < start`, is left out of the page the opener gets, like a picture that fails its check; the other rows are delivered and the paging goes on. It no longer fails the page or counts against the document. A broken stream (rows out of order, a record that does not decode) still fails the page.

- A book whose format plugin sleeps opens as "preparing" and the format is woken to open it, instead of the reader being told "busy"; a format that cannot be woken, or finds no memory after its longest wait, fails the book with the usual error notice (#248).

- **A book, a CSS file or a file to sniff on the card's top never names the kernel's or the plugins' folders** (#228, `docs/abi/document.md` section 1). On root 1 a path whose first segment is `plugins`, `.paperpaw` or `paperpaw.ini` (in any letter case) is `ERR_INVALID` for `doc_open`'s `path` and `css_path` and for `format_for`, the same rule as the file service.

- A book's cover, a preparing step and a format's close lend the format a reply room the size of its answer (a few hundred bytes), not 4 KiB, so less memory is held while a format reads a book (#248).

- The log line for a native plugin call the host dropped now names the plugin (`native: linked: <id>: call <tag> ended with <status>` on the X4, `native plugin <id>: …` in the emulator). The shell's report function gets the plugin id as a borrowed `&str`, so nothing is allocated (#228, #250).

- **The emulator's plugins from the card are offered what the device offers** (#228). A plugin file on the card is checked against the services and features the emulated device has wired, the same rule as the firmware's card path, so a plugin that needs the network, the card's files or the library loads here when it loads on the device, and is refused where it would be refused there.

- **A plugin reads only the events it handles** (#228, draft 26). `Plugin::event`
  receives `events::Incoming` (the event framed, its body not read) instead of a decoded
  `Event`. `decode_as::<events::kinds::Tick>()?` reads one kind and links only its
  decoder (`Ok(None)` for any other, body unread), `is::<K>()` asks without reading,
  and `decode()` reads any kind as `Event::decode` does (which stays). The 18
  first-party native plugins read their kinds only; `paperpaw_ui_kit::read_event!`
  lists them. End of book 91,716 → 84,704 bytes, Battery 82,944 → 76,468, Profiles
  123,928 → 116,900, Keys 129,436 → 121,636, Settings 156,532 → 149,872, Stats
  59,584 → 55,600. A malformed body of a kind a plugin does not read is no longer an
  error of that callback. `tools/plugin-symbols.sh --events` proves which decoders a
  plugin links, fails on a stripped file, and `tools/flash-gate.sh --plugin-elf` runs it.
  The `core.files` client of the SDK reads and writes its rows through the typed
  codec as the other facades do (no change for plugins; the codec gate holds).

- **File manager on interface 7, native shell** (#228). Browsing the card, renaming, moving, copying, deleting and making folders, with the same checks as before (nothing replaced, a folder never moved into itself, PaperPaw's own folder left alone). Until the library service is there, a moved or renamed book keeps its reading place and notes under its old name, and the file manager says so.

- **The file manager runs on interface 7** (#228). A book now opens from the file manager in the reading app you chose, or in another one that reads its format ("Open with"), and deleting a book through it deletes what is kept for it (place, bookmarks, notes) as well. Browsing, renaming, moving, copying, deleting and new folders are as before; the library follows moves and renames by itself. The per-folder "Reading app" choice is gone: the reading app is the one you bound in the settings. The old interface-6 plugin is removed.

- **The library runs on interface 7, and the old interface-6 plugin is removed** (#228). Books now show their covers in the list, in the grid and on the card of the book you read last, and the book information leads with the cover. Holding a book offers to open it, mark it finished or unread, see its information, move it to the archive (under a free name) or delete it with what is kept for it, after asking. While the library is looking for books it says how many it has found so far, with a rough bar of how far it has got, and an empty list says why: preparing, the folder could not be read, the memory card is gone, or every book is in the archive. A book's row shows how far it is read as a bar and the badges plugins put on it; the shelves count the favourites and each list at the row's end; the library menu offers the order, the view, the search, the shelves, the library folder and the settings; the library follows changes of the catalog by itself; on the shelves, "Find a book" lists the first letters of the titles with their counts, and a letter opens the library by title at the books that start with it. The folder chooser of the folder settings is part of the library, as it was.
- **Not yet back in the native library** (#228). Adding a book to the favourites, "Look for new books", the home screen's swipe down to search, filter chips on large screens, gray covers apart from black and white, and the tags, language and description in the book information. `plugins/files/PORTING-7.md` names what each one waits for.

- **Files starts its move to interface 7** (#228). A native version of the library list, the
  shelf lists and the shelves screen now sits beside the old code; authors, series, tags,
  folders, covers and the book sheet follow in later slices.

- **More tests for `core.files` root 3** (#228): a change op outside a format call, and a trailing `/` in a book's path or a request path. No behaviour change.

- **The interface-7 library lists authors, series, tags and folders, searches, shows the next book of each series begun, and shows book information** (#228). The shelves page offers them; each list shows its names with how many books they have and opens the books under a name. Search asks for words with the keyboard. Covers, the book sheet and finding by letter follow, with the removal of the interface-6 library.

- **The firmware and plugin-job driver is made on demand** (#228, #248, #2654). The request table and install machine are made at the first firmware or plugin command and let go once nothing runs, asks or waits; what plugins may still read (the outcomes op 13 answers, a check's result) rests with the device's firmware host. A device that installs plugins no longer pays about 2 KiB for them at idle. A device that can boot two firmwares keeps its driver made.

- **`paperpaw.ini` is closed to plugins on root 1** (#228), like `plugins/` and `.paperpaw/`: its `[firmware]` section guards a dual-boot setup, so only the host's confirmed command changes it (`docs/abi/files.md` 1.1).

- **The plugin list reads linked plugins' manifests from the program** (#248). Interface 7's one shape change for this (owner's Decision 168): `PluginInfo::manifest` is a `ManifestRef`, a manifest kept in the program or a copy of its own, and it reads as a `Manifest` as before (`info.manifest.id`). `ManifestRef::try_new` keeps the one in the program (`Manifest::fixed`, with `uses` and `how` usable in `static` items) only where every field equals the plugin's own, else makes a copy, and gives the manifest back where the heap has no room for it (the kernel then leaves the plugin out of the list, with a warning). A linked plugin's row takes 40 bytes, not 104 (emulator); an interface-6 plugin's row, with its copy, some 48 bytes more until those plugins go (#244).
- **Native actions keep their id and title in the program** (#248). Each linked plugin's build script makes a table of the id and title of every screen and action it declares (`paperpaw_sdk::native::declare::fixed_titles_source`, `NATIVE_TITLES`, entries of `paperpaw_sdk::TitleEntry`). The kernel keeps a native action whose id and title equal an entry as that entry (`paperpaw_sdk::Titled`), any other as a copy. `NativeAction::new` takes a `Titled`. An action's row takes 32 bytes, not 56 (emulator).
- **The kernel's reading setting groups are kept in the program** (#248). The ten parts of the reading settings and the space widget's group are `static`, as the kernel's other groups are, not made onto the heap at the start. About 0.9 KiB less (emulator).
- **Native screens keep their id and title in the program** (#248). The kernel's screens registry keeps interface-7 plugins' screens in a list of their own, each a `Titled` (a linked plugin's entry of `NATIVE_TITLES`, or a copy), apart from interface-6 plugins' screens. `ScreenList::new` takes that list too; `ScreenList::iter` still gives the screens plugins added in the order of their ids, then those kept in the program.

- Tests: with the linked reading plugins (the flip, #243) the sleep parity cases of the seven formats run, and the standby cycles require every plugin that may sleep, formats included; CI and `check-local.sh` run them with the reader and the formats linked (todo 566). The tests of the native reader (open last book, pictures in their own frame) and the network settings of native plugins run with the flip.
- Tests: with the native reader linked, the interface-6 page-layer, text-filter and decorator tests are skipped until interface 6 goes (#244), and the table of contents' partial redraw and the Wi-Fi rejoin of `--wifi online` are skipped with their open bugs named; the partial-redraw harness moves the native host's work clock, so the table of contents finds its chapters.

- **The fonts browser reads its catalog without aborting when memory is short** (#228). The JSON
  reader's own allocations now refuse cleanly too, so a short heap ends the read with an error
  instead of stopping the plugin.

- **Fonts browser linked as a native plugin** (#228). Builds that link the store's plugins (`native-linked-store`) list the native fonts browser in `NATIVE_PLUGINS` in place of its interface-6 form.

- **The fonts browser's catalog and install plan on interface 7, first part** (#228). The
  reading of the font catalog, the files a font needs and the plugin's declarations are in place
  without the host and refuse cleanly when memory is short; the screens follow.

- **The fonts browser has an interface-7 native entry** (#228). `fonts-browser` declares its screens, its
  catalog setting and its menu entries, and shows the list of fonts (the catalog kept in the data folder
  and fetched anew with `core.net`) and a font's page where a font can be read with or used for the
  status lines. Downloading, updating and removing a font follow with the host's install commands.

- **The fonts browser is interface 7 only** (#228). The interface-6 plugin (list, font page, installer, samples) and its plugin file are removed; the native plugin is what is left, with the same catalog, downloads, texts and settings. It runs in the app once the host activates native plugins.

- **The fonts browser shows a font's sentence in the font's own face** (#228). On a font's page the sample sentence is drawn in the font itself, from a small sample pack fetched from the library; without one the sentence stays in the menu font.

- Reading needs a little less memory: while a book's format plugin works, the host keeps only the call it sent, not room for the longest call (#248).

- **Comic book archives run on interface 7 only** (#244). The interface-6 `format-cbz` plugin is gone: a comic book archive (`.cbz`) opens through the native plugin, with the same content. It is no longer registered by the app; until native plugins are switched on in the shells, these files do not open as books.

- **Word documents run on interface 7 only** (#228). The interface-6 `format-docx` is gone: the plugin that opened `.docx` files in the app, its cache in the plugin's folder and its plugin file. Word documents are opened by the native format plugin, which converts them into the cache the host keeps; the pages, contents, links and pictures are the same, the texts are unchanged in English and German. A document without a title is named by its file, as the library names every book. The format is no longer built into the app or offered as an interface-6 plugin file: it comes with the native plugins when they are switched on.

- **Word documents are an interface-7 plugin** (#228). `format-docx` declares its format (`docx`,
  converted into the cache on the first open) and answers the host's `core.format` calls from a
  native entry, with the same pages, contents, links and pictures as before.

- **Word documents get the interface-7 format provider** (#228). `format-docx` answers the host's
  `core.format` calls (open, prepare, text, contents, links, pictures) with the same blocks as
  before, and its converter no longer stops on a full heap: a refused allocation makes the host try
  again instead of leaving a document without its styles or its links.

- **The EPUB, HTML, FictionBook, Markdown and comic book plugins are five crates now** (#228). Their shared conversion lives in `paperpaw-epub-family`, and each plugin (`format-epub`, `format-html`, `format-fb2`, `format-markdown`, `format-cbz`) has its own interface-7 entry, ready to be packed as a native plugin file. Nothing changes for readers yet: the interface-6 plugins stay registered until the per-format removals.

- **EPUB books run on interface 7 only** (#244). The interface-6 `format-epub` plugin is gone: an EPUB book (`.epub`) opens through the native plugin, with the same content. It is no longer registered by the app; until native plugins are switched on in the shells, these files do not open as books.

- **FictionBooks run on interface 7 only** (#244). The interface-6 `format-fb2` plugin is gone: a FictionBook (`.fb2`, `.fb2.zip`) opens through the native plugin, with the same content. It is no longer registered by the app; until native plugins are switched on in the shells, these files do not open as books.

- **HTML files run on interface 7 only** (#244). The interface-6 `format-html` plugin is gone: an `.html`, `.htm` or `.xhtml` file opens through the native plugin, with the same reading matter, links and pictures. It is no longer registered by the app; until native plugins are switched on in the shells, these files do not open as books.

- **Pictures as books run on interface 7 only** (#228). The interface-6 pictures plugin is gone: a JPEG, PNG or GIF opens as a book through the native `format-images` plugin, with the pictures of its folder after it as pages, and what is decoded is kept in the host's document cache instead of a folder of the plugin. It is no longer built into the app or offered as an interface-6 plugin file; until native plugins are switched on in the shells, pictures do not open as books.

- **Pictures as books are an interface-7 plugin** (#228). `format-images` declares its format
  (`jpg`, `jpeg`, `png`, `gif`; the pictures of the folder follow the opened one as pages) and
  answers the host's `core.format` calls from a native entry, with the same pages as before.

- **Markdown files run on interface 7 only** (#244). The interface-6 `format-markdown` plugin is gone: a Markdown file (`.md`, `.markdown`) opens through the native plugin, with the same content. It is no longer registered by the app; until native plugins are switched on in the shells, these files do not open as books.

- **Mobipocket books run on interface 7 only** (#228). The interface-6 `format-mobi` plugin is gone: a `.mobi`, `.azw` or `.azw3` file opens through the native plugin, with the same blocks, contents, links and pictures, and the converted book is kept in the host's document cache instead of a folder of the plugin. It is no longer registered by the app or offered as a store plugin there; until native plugins are switched on in the shells, these files do not open as books.

- **Mobipocket books have an interface-7 entry** (#228). `format-mobi` declares its format (`mobi`, `azw`, `azw3`, with a sniff and the first-open conversion) and answers the host's `core.format` calls from a native entry, with the same blocks as before. The interface-6 form stays until the host activates native plugins.

- **Mobipocket books get their interface-7 format provider** (#228). The
  `format-mobi` plugin can now answer the host's `core.format` calls (open,
  preparing the book in steps, metadata, text blocks, contents, links, covers and
  pictures). Its reading of a book no longer aborts when memory is short, and a
  card that fails in the middle of a book is now an error instead of the book's
  end.

- Less memory while the host reads a book on its own (book details, covers, preparing): a format plugin answers straight into the host's buffer, without a second copy of its reply (#248).

- **Plain text reads with less memory** (#248). `format-txt` reserves the decoded text at the size it
  takes (not three times the file bytes), writes the records of a block straight into the reply (no
  64-record list), and no longer reads 4 KiB ahead: the held window is about 9 KiB, not 13 KiB.
  Reading a TXT book on the X4 emulator takes 4.0 KiB less live and 10.3 KiB less at the peak.

- **Plain text is a native plugin only.** The old interface-6 form of the plain text format plugin is removed; the native plugin opens `.txt` files as before.
- **Android and web builds cannot open books for now.** They link the app without the native reader, and the interface-6 reader and plain-text format were the only ones they had (EPUB, FB2, HTML, Markdown and comics were already native only). Until those shells run the native reader (owner decision 185, temporary), these builds start without a reader. The X4 family and the desktop emulator are not affected.

- **Plain text is an interface-7 plugin** (#228). `format-txt` declares its format (`txt`, `text`)
  and answers the host's `core.format` calls from a native entry, with the same blocks as before.
  `paperpaw_sdk::native::reply::Reply::write_with` is public so a provider can encode its answer
  straight into the host's reply buffer.

- **Plain text books get their interface-7 format provider** (#228). The
  `format-txt` plugin can now answer the host's `core.format` calls (open,
  prepare, metadata, sections, blocks, progress) from the book and the
  document cache, in fragments for long paragraphs. It is not active yet:
  the host switches to it with the other interface-7 plugins.

- **A format declaration is encoded by its own record type** (#228). `Declarations::format`
  hands the typed `FormatSpec` row to the encoder instead of the generic record enum,
  which linked the encoder of every record into a plugin that declares a format. Nothing
  a plugin calls changes.

- **XTC and picture books get their interface-7 format providers** (#228).
  The `format-xtc` and `format-images` plugins can now answer the host's
  `core.format` calls (open, metadata, pages, contents, covers and pictures).
  They are not active yet: the host switches to them with the other
  interface-7 plugins.

- **XTC and XTCH books run on interface 7 only** (#228). The interface-6 `format-xtc` is gone: the plugin that opened the Xteink readers' pre-rendered books in the app, and its page cache in the plugin's folder. XTC and XTCH books are opened by the native format plugin, whose pages are decoded into the cache the host keeps; the pages, chapters, cover and metadata are the same, the texts are unchanged in English and German. A book without a title in its metadata is named by its file, as the library names every book. The format is no longer built into the app: it comes with the native plugins when they are switched on.

- **XTC and XTCH books are an interface-7 plugin** (#228). `format-xtc` declares its format (`xtc`,
  `xtch`, recognised by their first bytes too) and answers the host's `core.format` calls from a
  native entry, with the same pages, chapters and covers as before.

- **A book format may sleep** (#248, D2). Formats are no longer kept awake by the kernel's park checks: a format the shell flags to sleep is parked after its start and made at the next safe point once a book needs it. The document service keeps such an open preparing instead of answering busy (Host, #2717). No format is flagged yet.

- **The book formats sleep** (#248, Decision 165). The seven linked formats (TXT, EPUB, HTML, Markdown, FB2, CBZ, XTC) are flagged to sleep: each is parked until a book of it opens and again on standby, where the kernel's checks allow it. They are native plugins only where the build activates them (#243). A parked format a document needs is made in the same pass (the service pump runs up to three rounds); a call to a parked plugin asks it to start; a pending open reports no book flags until its format answers; a re-park whose retirement was refused at first keeps the formats listed on its retry; no cover job starts on the way into standby. A parked format that finds no memory, where no idle plugin can be parked again to make room, fails the open at once with the no-room notice, as awake (queued actions keep their wait). The reader now counts the pages of a book that opens ready at once, and its count waits while another screen covers the book.

- **The GIF decoder refuses what the host refuses** (#228). `paperpaw-image`
  refuses a picture larger than its screen and a screen over 8 megapixels
  itself (before, only the host checked, so a plugin's own decode could still
  take frames up to 64 megapixels), and keeps its colour tables on the heap so
  its stack frames stay small on the X4.

- **Desktop native plugins are admitted by the kernel** (#228). The desktop loader hands every interface-7 library to `Kernel` through `CodeLoader::load_native` and `NativeAdmission::admit`, once per plugin before its first frame; the old flat-plugin path for interface-7 libraries is gone. A refused declaration lists the plugin as failed and nothing of it is mapped. A library that declares or registers anything is admitted (an events-only plugin loads); a library with nothing at all is invalid. All plugins admitted by one loader share one dispatch budget.

- **Opening a screen with parameters takes no extra memory** (#228). The
  handover of up to 32 parameters is encoded straight into the plugin's own
  buffer instead of through a 3,328-byte temporary list, which was most of
  the 4 KiB the X4 may add. A test counts zero allocated bytes.

- Less memory in use from the start: the plugin heap's table of owners holds exactly the plugins taken in, without room kept for more (#248).

- **The host side of the plugin heap word, first part** (#228). The host has a `PpHeap` table (`handed_heap::<S>()`) over the same allocator as the heap slots, and every entry from the host into a plugin's code (its factory, destructor and callbacks) runs it as the "frame owner" and gives the previous owner back on every way out. Nothing charges a plugin's allocations yet and no shell passes the table to a loader.

- Home: a widget that declares how many cells it takes (its size, least size and square flag) gets that size when it is added or placed, without being made and measured, as before; resizing keeps the least rows it declared.
- Home subscribes to the minute tick and draws the dashboard anew while it is on top (the status line and the clock), as on interface 6.

- **Home screen: the dashboard on interface 7, viewing** (#228). The page of tools and widgets in
  its grid, the footer's actions and dots, the page buttons, the button focus and taps that run a
  tool or reach a widget. Editing, *Add*, the status line, icons and the home screen's own widgets
  follow.

- **Home screen: *Add* and the editing switch on interface 7** (#228). The pencil (a long press
  too) starts editing and *Done* (or *Back*) ends it; *Add* opens the two-step screen (the kinds,
  then the things of one; *Add a page*, *Remove this page*, *Reset landscape layout*) and writes
  the pages, the placement of a widget added and the values a removed page's widgets set. Moving,
  sizing and removing items, the gallery of widgets as they would show, the ready-made layouts and
  icons follow.

- **Home screen: arranging items with the buttons on interface 7** (#228). While editing, an item
  offers *Move*, *Change the size*, *Remove*; the buttons move or size it a cell at a time and
  *Select* sets it down. The finger's drag and the options' *Settings* entry follow.

- **Home screen: its settings, screens and actions declared for interface 7, second slice** (#228).
  The pages, grid and footer settings, the screens, the `core.home` offer, the actions, the swipe up
  and the pages as a place of widgets are declared for the native plugin interface; the dashboard
  and the built-in widgets follow.

- **Home screen: the tools drawer on interface 7** (#228). *All tools* opens the grid of every tool
  with its icon and name; buttons and taps move between them and run one, a swipe down goes back.
  The focus shows at once on a screen that is shown after buttons were used, not after a touch.

- **Home screen: icons on interface 7** (#228). Tools show their icon above their name (light on a
  dark ground where their plugin shows them on, faint where not enabled), names of tools in a row line
  up, and the footer's actions and the pencil show their icons. Where no icon font is there the names
  and the word on the pencil stay.

- **Home screen: the footer items' editor is a screen of its own** (#228). `footer.items` names the
  screen `status-items`, built on the UI kit's status items editor, as its editor, instead of the
  host's status items screen; the dashboard and its callbacks follow with the native entry.

- **Home screen: the native entry, first part** (#228). The footer items' editor runs as an
  interface-7 screen, *All tools* and *Edit home screen* open their screens, and the declarations
  are built into the plugin image. The dashboard, *Add* and the tools drawer follow.

- **The home screen is a native plugin only.** The old interface-6 form of the home screen plugin is removed; the native one shows the pages, the tools, the widgets and the editing as before. Android and web builds, which have no native home yet, start without a home screen for now (owner decision 185, temporary).

- **Home (interface 7): a widget that changed is redrawn at once** (#243). Home subscribes to `event.widget_changed`, so a widget's new content shows on the home screen without waiting for the next minute tick.

- Home: the date and the clock widgets are native on interface 7, with the same settings, groups and looks (alignment, frame, padding, paper, ink, corners, frame line, shadow, size, the clock's date). The clock splits the time from "PM" also for languages with their own digits.

- **The host reads and writes records by type** (#228). Service, event, input
  and library ops decode the record types they expect instead of the
  enum-wide `Record` decoder, and encode with the typed encoders, so the X4
  image no longer links `record_decode`, `Record::write`, `Record::body_len`
  or `Record::tag`. Statuses are those of the enum path; the one intended
  change is in `events::decode`: a non-event record with a malformed body
  gives `ERR_UNSUPPORTED`. In release builds a service reply is checked
  by the framing of every row and its typed end row (the rows of `core.lookup`,
  whose provider is a plugin, by their types); debug and test builds read
  every row by its type.

- **Less host memory for each hosted plugin** (#248). A plugin's host context no longer keeps its own pointer to the shared host table or a second view of its resources (its callback's blobs expire through the context's own), and the identity its dispatch and its parking recipe keep holds the device pool, not an owner's view of it. Nothing changes in behaviour.

- Less memory in use while reading: the library keeps no room for annotation edits while none is being made, and the table of formats holds exactly the formats there are (#248).

- **Hyphenation plugins are data only** (#228). The English pack and the twelve language packs no longer have an interface-6 code plugin; they are data packs (`hyphenation/<lang>.pphp`) the reader reads from the card. Hyphenation now comes only from the installed data packs: English is no longer built in, so a device without the English pack does not hyphenate English until it is installed. The packs keep their ids and are offered as updates.

- **The hyphenation plugins are data plugins in the release** (#228). English and the twelve store languages are built by `tools/hyphenation-packs` into code-free packages (`plugin.ini` with `[hyphenation]`, `hyphenation/<lang>.pphp`, format 5) and listed in `tools/plugin-files/data.toml` at 2.0.0, newer than Beta 3's 1.0.x. They leave `tools/plugin-files/plugins.txt`, so each id has one catalogue entry. English stays linked into the app until the interface-6 crates are removed; its default install by the flasher comes with the web side. The interface-6 hyphenation crates stay until their removal follows.

- **Beta 3's hyphenation plugins move to the data packs without a second install path** (#228). A `hyphenation-<lang>` folder of Beta 3 (interface 6, a `code.ppn`) is replaced as a whole by the data pack of the same id through the one plugin install (`install_staged` for the flasher's bundle, the store's install otherwise), which keeps the old folder aside until the pack is in place and puts it back at the next start where the power went first. Tests cut the power at every write of the swap, and show a Beta 3 folder without a pack untouched and listed as built for an older interface (the language shows "needs update"; nothing is deleted or written).

- **Prepared pictures are bounded before they are decoded** (#228). A
  source over 8 Mpx is refused from its header, a prepared picture is at
  most 1,600 pixels on its longest side on every device, the image
  command paths count against the callback's 4,096-byte command budget,
  and the boot sweep of the picture cache reads its folder with the
  bounded listing. `ui::Picture` is `#[non_exhaustive]` and has
  `Picture::new`.

- **A plugin install holds less while it runs** (#228, #248). The copy's walk and file list go before the stage check, and the 4 KiB buffer goes after the trust rules (nothing later reads through it), so neither is held while the user decides or through the swap. The first firmware or plugin request no longer holds room for four requests: each reserves exactly its own.

- **The interface-6 form of sixteen first-party plugins is gone** (#244). Settings, Wi-Fi, Transfer, Keyboard, Aids, Fonts browser, Widgets, Notes vault, Mail, News, Picture home, Menu, and the formats MOBI, XTC, Word, Pictures, EPUB, HTML, FB2, Markdown and CBZ are native (interface 7) plugins only: their `Plugin` implementations, the old screens and the tests that covered only those are removed, the app no longer registers them and `tools/plugin-files` no longer builds files for them. The native plugins, their screens, texts and settings are unchanged. Reader, Home, plain text, Vocabulary, Dictionary, Sleep and Store keep their interface-6 form for now.

- **Joplin on interface 7, first slice** (#228). The set-up screen, its settings, actions and icons are
  declared for the native plugin interface, and the sync targets' addresses, Joplin's ids and items
  (notebook and note properties) and the reading of a book's exported notes are rewritten to refuse
  instead of aborting when memory runs short. Notes are sent as before; the native plugin takes over
  once the host runs native plugins.

- **Joplin runs on interface 7** (#228). The Joplin plugin is now a native plugin only: a book's exported notes go to Joplin Cloud, Joplin Server or a WebDAV folder over the network service, the account is set up on a screen of rows with the password kept with the other passwords and keys, and what cannot be sent waits and is sent again when the network is there (declared work), with the same texts in English and German. The sending checks the target as before (its format, no end-to-end encryption, no lock held by a client upgrading it) and writes a note over its earlier one only if it was not edited in Joplin. A sending that runs out of memory ends with a notice instead of aborting. The plugin hears the reader's export through the reader's stage `reader.notes-exported`.

- A global `key.bind` of a native plugin that would replace or unbind the last
  global input to the main menu is dropped from the key map (`registry.md`
  section 4.2); the row stays the plugin's and is removed with it. Held Power
  has no input name, so no binding can take it.

- **Keyboard: layout ids without commas** (#228). A comma in the id of an added layout is left out, so the stored list of chosen layouts cannot split it.

- **The keyboard is interface 7 only** (#228). The interface-6 keyboard plugin and its suites are removed; the native plugin is what is left, with the same keys, layouts, settings and texts. It runs in the app once the host activates native plugins.

- **Keys on the new plugin interface: refreshes when the registry changes** (#228). The plugin now
  runs in the host's real registered scope with its registry-change subscription admitted (#916),
  so its rows follow changes of the plugins, menus and presets without reopening the screen.

- **KOReader exchange and screen lock start their move to interface 7** (#228). KOReader's file reading and writing, and the screen lock's rules (the code kept as a salted hash, the wait after five wrong tries, the keypad and the pattern), written so that a refused allocation never aborts and a secret is wiped when done, exist natively beside the old code, with the lock's settings page; the exchange, the lock screen and the touch lock follow when the host pieces they need arrive.

- **KOReader's exchange of bookmarks and highlights runs on interface 7** (#228). When a book opens and when its highlights or bookmarks change, the plugin reads KOReader's file beside the book, takes over what is new there through the library's annotation facade (positions as XPointers through the document facade), and leaves what is new here for KOReader. The same rules as before: removals on one side reach the other, a copy of what is removed is kept aside, and where KOReader's file has none any more the reader is asked in the book. A file that cannot be read or understood changes nothing; a refused allocation stops the exchange instead of aborting. The old interface-6 plugin is gone.

- **Progress sync starts its move to interface 7** (#228). Its settings and the logic that decides what a fetched position means, writes the progress and reads the server's answer now exist natively beside the old code; the sync itself, the account screen and the overlay follow when the network host and secrets reach plugins.

- **Progress sync is now an interface-7 plugin** (#228). The account screen (user name, password, server, *Sign in*, *Create account*, *Sign out*), *Sync progress*, the key kept among the passwords and keys, and the offer to go where another device is run through the network, secrets and work services; the interface-6 plugin and its store file are gone. Until the library and document services can name a book and a place, no position is fetched or sent.

- **Network services made at their first call** (#248). `core.net` and `core.sockets` are wired
  at start but their devices are made when a plugin first calls them, fallibly: a first call
  without room is `ERR_NO_MEMORY` and the next one tries again. The shell's reports before that
  answer as an empty device does. A device whose plugins never use the network keeps about
  0.7 KiB more free.

- **Library moves and removals go on past errors no retry cures** (#228). `library::try_moved` and `library::try_removed` still stop at the first error a retry may cure (a card error, a full card, no memory) and return it. An error no retry changes (a path the card refuses, damaged or too large data) no longer stops the other parts: the cover, the recently opened and the favourites are still carried along or forgotten, and that error is returned at the end.

- **`StorageError::OutOfMemory`** (#228). A read that the heap could not hold
  used to be reported as `TooLarge`, which means "never works"; it is now
  `OutOfMemory` ("try again"). This is a new variant in a full interface draft.

- **`core.library` op 22 `fields` pages** (#228). A book's fields come in
  pages of whole rows under the book's field generation, as annotations do:
  a later page with a stale generation is `ERR_STATE`, and a book with many
  fields no longer needs them to fit one reply.

- **The library's move job runs in the kernel's main loop** (#228). A move of the
  user's files (root 1) is carried by one bounded step a pass of the service pump,
  outside callbacks and with the kernel's own catalog on the same card; the
  library's book-moved events and the settings that follow run right after the
  pump. A step the card or memory refused waits two seconds (twice as long after
  each refusal in a row, up to a minute) and the job is kept; an interrupted job is finished at the first pass after a start. A move the user begins while no job is open (the card refused the journal at the start) does not wait for an earlier back-off.

- **LIB op 24 `provide` is checked as the contract says** (#228). `flags` follow the record's field order (bit 0 title, 1 author, 2 series, 3 number, 4 tags) and must name at least one field, none above bit 4. A provided text that is blank (Unicode White_Space, as `str::trim`), a set bit with no value, a tags bit with no tag, a blank or empty tag line, and any control character (`char::is_control`: U+0000-U+001F, U+007F, U+0080-U+009F) or U+2028 or U+2029 in a title, author, series, number or tag are `ERR_INVALID`; more than 32 tags stay `ERR_NO_SPACE`. Nothing is escaped.

- Library query: each book a query keeps costs one allocation and 32 bytes less (sort text and path in one buffer, 32-bit line offset), and the recently-opened list is reserved for the books it names (#228).

- **The built-in plugins keep their setting groups as a table in the program too** (#228, #248). A linked plugin's build script makes a table of its setting groups next to its settings table, and the kernel keeps a group that equals an entry as a reference to it instead of a copy on the heap (`Kernel::register_linked` takes the plugin's tables, `paperpaw_app::fixed_tables`).

- **A built-in plugin's id takes no heap** (#248, D4). `Kernel::register_linked` takes the id as the firmware keeps it (`&'static str`), and its context, the owner directory and the linked set borrow that text instead of copying it, and a command or a callback's stage takes its owner id without a copy. The declaration store keeps an image-only owner's batch starts and kinds in one allocation. Loaded (card) plugins are unchanged.
- **A built-in plugin's declarations take its id from the firmware too** (#248, D4). The owner of a linked plugin's declarations is the checked `&'static str`, not a copy parsed per plugin (four heap copies of the id before), and the id is no longer copied once more just to check it, so a full heap no longer reports a valid id as invalid. Shells that built ids at run time must now keep them for the whole run (`&'static str`); every shell in the tree links its plugins at build time.

- **The built-in plugins that declare settings keep them as a table in the program too** (#228, #248). Keyboard, lock, help, OPDS, power, Bluetooth, menu, settings, file manager and aids take their settings from a table their build script makes of the same declarations (as the widget pack does), instead of a copy on the heap for each: about 7 KiB less heap on the X4 profile in the emulator.

- **The text of "Literata, larger sizes" says what it needs.** Literata must be chosen as the reading font for its larger sizes to show.

- **The screen lock's steps move to interface 7** (#228). What a PIN key, a pattern dot, a typed password or *Back* does on the lock screen, the wrong-tries wait and the screen's layout now exist natively beside the old code, without a refused allocation ever aborting; the lock screen itself, *Lock now* and the touch lock follow.

- **The screen lock is a native interface-7 plugin** (#228). The lock screen (PIN, pattern, password, setting up the code, the kinds list), locking at start and when the device wakes, *Lock now*, the wait after five wrong tries and the code kept among the secrets work as before, with every allocation fallible, the codes wiped when done with and no code ever printed; the interface-6 plugin is gone. The touch lock, the setting's immediate revert and the end of the wait without an input wait for host pieces (see `plugins/lock/PORTING-7.md`).

- **A lookup provider without an entry no longer ends the search** (#228). The
  broker of `core.lookup` turns a provider's "not found" into an empty entry
  with its name, so the providers after it are still asked; only running out
  of providers ends the asker's loop.

- **Mail runs on interface 7 only** (#228). The interface-6 mail is gone: the folders, the list of mails, the reader, the compose window, the five home widgets (unread, latest, starred, mailbox, write a mail) and the IMAP and SMTP sessions are the native plugin's. The texts are the same in English and German. Mail is no longer built into the app or offered as an interface-6 plugin file; no build offers it until the host switches the native plugins on, as for the other ports. Choosing a provider does not fill in the servers yet (it waits for the host's setting-changed event); the servers are typed in.

- **Menu editor on the new plugin interface: refreshes when the registry changes** (#228). The plugin now
  runs in the host's real registered scope with its registry-change subscription admitted (#916),
  so its rows follow changes of the plugins, menus and presets without reopening the screen.

- **Menu on the new plugin interface: refreshes when the registry changes** (#228). The plugin now
  runs in the host's real registered scope with its registry-change subscription admitted (#916),
  so its rows follow changes of the plugins, menus and presets without reopening the screen.

- **Menu 1.0.6: the interface-7 native plugin, first slice** (#228). The menu list is a sheet over
  the screen (rows from the registry, submenus, settings changed in place, paging); `paperpaw-ui-kit`
  gains `ItemList::draw_sheet`, `sheet::geometry` and faint rows. Tiles, the bar and the quick panel
  follow.

- **Menu: the quick panel's layout on interface 7** (#228). Which controls a menu becomes
  (switches and buttons several to a row, numbers with `−` / `+`, choices as segments) and where
  they go (bottom or top, in pages) is ported without the host, with allocation-refusal tests. The
  panel screen follows.

- **Menu: the quick panel on interface 7** (#228). The panel (`core.panel`: switches, buttons,
  `−` / `+` numbers, choices as segments, pages, hanging from the bottom or the top) is a native
  screen with its help hints. Sliders and icons follow.

- **Catalog text buffers are made when first needed** (#228). A native plugin's `Messages` no longer fills a 4 KiB and a 1 KiB buffer when it is made, which cut the heap peak at the X4's start by about 5 KiB.

- An interface-7 plugin's action is kept without a handler of its own: `paperpaw_sdk::NativeAction`
  (id, title, plugin and token, with `Actions::insert_native` and `Action::native`) in place of an
  `ActionSpec` with a boxed handler; the kernel queues `event.action_run` where the action runs,
  with a warning where its plugin is gone or disabled. A native screen keeps its id once. Less
  heap per native action and screen on the X4 (#248).
- `Kernel::owned_native_texts` lists interface-7 plugins' ids and titles kept as heap copies, with
  `paperpaw_sdk::Id::kept` and `Str::kept` (kernel feature); a `paperpaw-app` test holds every
  linked plugin to none. Power's id table now also names its actions and screens (#248).

- CI and `tools/check-local.sh` (new step `test-native`) run the app's `flows`, `sleep_parity` and `sleep_cycles` with `--features native-linked` (#248): the workspace test run leaves the feature off, so they had never run there. `check-local` runs the step when the core, the app, the native layer or a plugin changed, or with `--all-linked`.

- **A screen that could not be opened says so** (#228). When a native plugin's screen cannot be made (the plugin stopped, an argument was refused, …), the user sees the notice "This screen could not be opened." besides the diagnostic; the words never name the screen or its parameters, and after a lack of memory it is skipped (the log still says it), since the notice screen would meet the same trouble.

- **A native plugin's event and draw registrations are admitted with its declarations** (#228). An event registration needs a tag the host encodes and each tag only once; a draw needs the token of a screen or widget the plugin declared and each token only once, also across batches. A registration that does not fit refuses the whole plugin with nothing published, never on a full heap, and teardown drops the registrations before the owner stops. Events are not delivered to native plugins yet.

- Every interface-7 plugin's dispatch shares one set of the kernel's handles (meter, stack rows,
  declaration directory …) instead of a copy each: 40 bytes less per native plugin on the
  desktop, about 0.9 KiB with 24 linked plugins (#248).

- **News on interface 7, third slice** (#228). A fetched feed is made into the page of its book in small pieces over the storage calls (at most 4 MiB of feed, the page written in calls of 4 KiB), refusing instead of aborting when memory runs short; a feed without articles leaves no page. The page and the result are the same as before. The plugin still runs as before until the native plugin takes over.

- **News on interface 7, second slice** (#228). A feed (RSS or Atom) is made into a page to read
  by a converter that reads the XML with the SDK's fallible reader and keeps only one article in
  memory at a time, so a refused allocation ends the fetch with an error and the old book stays,
  instead of aborting. The page is the same, byte for byte, as before. The native plugin takes over
  once the host runs native plugins.

- **News on interface 7, first slice** (#228). The setting for the articles a feed keeps, the list of
  feeds, its actions, the place in the tools and the icons are declared for the native plugin
  interface, and the feeds kept, the headlines, the feeds' dates and the text helpers are rewritten
  to refuse instead of aborting when memory runs short, with the read caps of before (64 KiB feeds,
  32 KiB headlines). Feeds are fetched as before; the native plugin takes over once the host runs
  native plugins.

- **News on interface 7, fourth slice** (#228). The feeds are fetched one after the other over the network service, each answer saved to its own file (at most 4 MiB) and made into its page; a feed that fails does not stop the others and a feed removed meanwhile is left out. The action *Fetch the news* declares the background work for when it is cheap, as before. The books themselves are put in the library with the next slice, so the plugin is not yet offered; it still runs as before until the native plugin takes over.

- **News as an interface-7 plugin file** (#228). The news plugin is built as a native plugin file for the X4 (81,412 bytes), so the packer and the flash gate see it. The page of a feed is read through a buffer from the heap instead of one on the stack (the X4 refuses a frame of 2 KiB or more). The plugin has no list screen yet and still runs as before until the native plugin takes over.

- **All notes on interface 7 is complete** (#228). A note opens its book at the note's place through the reader bound to `core.reader` (the book's path, the section and the offset), and the interface-6 plugin, its registration and its store file are gone. Until the reader port reads the place, the book opens at its usual place.

- **All notes on interface 7, native screens** (#228). The list of books with notes and a book's
  notes have a native form, read through the library service, with the same texts. A book's
  notes are shown 100 to a page, with *Next notes* and *Previous notes* rows. The export to
  `My Clippings.txt` works through the library and files services (note texts are read whole up to
  8 KiB each, and times are in UTC until the time zone is available). The opening of a note at its
  place waits for the reader; that row says it is not available. The native plugin takes over once
  the host runs native plugins.

- **All notes on interface 7, library scan** (#228). Which books have notes is found through the
  library service: the catalog is passed a page at a time and each book's highlights and bookmarks
  are counted, keeping only the 200 books of the list however many books there are. A library that
  changes during the scan is read again; a reply that does not add up ends the scan with an error.
  The notes are listed as before; the native plugin takes over once the host runs native plugins.

- Notes vault: the first part of its interface-7 port: its settings declared natively, and the readers of lines, notes, tags, lists and the vault's index made fallible and free of panics on hostile files. The vault itself keeps working as before.

- **Notes vault: Markdown notes are a format for interface 7** (#228). Opening a note takes no more
  memory than the device has: a note read on a full heap is answered "out of memory" and tried again
  later, never stops the device. The notes' formats (front matter, wiki links, embeds, tables, callouts)
  read as before.

- **The notes vault runs on interface 7 only** (#228). The interface-6 notes vault is gone: the vault's page, the lists of notes and tags, the index, the Markdown reading and the six home widgets (recent notes, pinned note, note of the day, random note, note tags, search) are the native plugin's. The texts are the same in English and German. The notes vault is no longer built into the app or offered as an interface-6 plugin file; no build offers it until the host switches the native plugins on, as for the other ports.

- Notes vault: the vault's page, the lists of notes (folders, latest changes, a tag, a search) and the tags are native screens on interface 7, with the Tools entry that opens the page. The index is made in the background by the work service in slices while the page is open. The Markdown format and the home widgets follow.

- **The catalogs (OPDS) and news plugins keep their state until it is needed** (#248). Idle, each holds only a pointer; the state is made by the first screen, sync, fetch or widget event, so a device that never opens them keeps about 1.1 KiB more heap free.

- **OPDS: *Sync catalogs* by who asks** (#228). Run by you (a menu, a key), the sync starts at once; run by a rule or another plugin, it waits for a good moment within the hour, as in Beta 3.

- **Catalogs (OPDS) run on interface 7** (#228). The list of catalogs and the editor of one, browsing a catalog (OPDS 1 and 2, WebDAV folders, search, more pages, back through the feeds), downloading its books into the library, covers in the lists and the sync of marked catalogs all run on the new plugin interface, and the old interface-6 plugin is removed. A catalog's password is kept among the passwords and keys; a password an older version kept in the list moves there. Books go into the catalog's folder below the library folder, under a name of their own where one is taken. The sync is no longer offered to go online first, and the action *Sync catalogs* now always runs at a good moment within the hour (the list's own row and a catalog's option still sync at once). A book's title and author are not handed to the library until it offers to take them.

- **No kernel *Open with…* in the book menu with the linked native reader** (#228, #257). With `native-linked-reading` the kernel no longer registers the book menu's *Open with…*: its resolver is interface 6's, which interface 7 does not carry (`docs/abi/document.md` section 1). The book menu has no *Open with…* in that build until a native one exists. The file manager's one-time *Open with…* stays, and builds with interface 6's readers keep the entry.

- **A started plugin gets its services' recovery** (#228). When the kernel takes a native plugin with `uses.network` in, each wired service is told (`ServiceEvents::owner_started`): `core.net` empties the plugin's `.received/` folder and retries a staged file kept after a failed rename, `core.sockets` retries its kept staged files. Before, only the next claim or the plugin's stop did.

- **Smaller declaration rows for linked plugins** (#248, D5). A plugin whose declaration batches lie close together in its image keeps one start, one kind mask and a 2-byte offset per batch instead of 12 bytes per batch (about 2 KiB less heap at start on the emulator). A plugin with one batch, or batches far apart, keeps one row per batch as before.

- **Parked native plugins, kernel side** (#248, #2638). A native plugin's lease holds its live instance or, for a parked plugin, the recipe that makes it again at a safe point (`ERR_BUSY` and a start on the next pass elsewhere, a back-off after no memory, withdrawal at the pass boundary on a refused batch). The setting and stack-event pumps make a parked subscriber before telling it and keep its event where they cannot. Nothing is parked yet.
- **Linked plugins parked at their admission** (#248, #2638). `Kernel::register_linked_parked` parks a linked plugin right after its own `make` where that `make` only registered, it holds nothing of the host, its declarations lie in the program's image and no other plugin's callback can reach it; its declarations are published and its instance dropped. Its first use hosts it again under its old owner seat; another batch withdraws it. No shell calls it yet.
- **Nothing is lost for a parked plugin that cannot be made yet** (#248, #2638). An action waits and reaches it once it is made; its screen is made as it opens, and without room the open fails as any plugin's does without memory; its retry is a wake deadline, so an idle device does not wait for a key press; after six tries without memory its waiting actions are dropped, each logged.

- **A native plugin's pending declarations cost a flat amount** (#228). The
  host keeps one sorted list per plugin instead of a table per declaration
  kind, so the memory each registered plugin holds no longer grows with every
  new kind. Admission and limits are unchanged.

- **Picture home screen: *Add* on interface 7** (#228). Editing the pages has its *Add* button
  again; it opens the two-step screen (the kinds, then the things of one; *Add a page*, *Remove
  this page*) and writes the pages, the placement of a widget added and the values a removed
  page's widgets set. The widgets offered are the registry's list without the ones that only
  preview a settings group. The picture behind the page, the bar's status frame and hints, and
  the finger's drag follow.

- **Picture home screen: the dashboard on interface 7** (#228). The pages of tools and widgets in
  their grid on a plain background, with the dots and the pencil below, the page buttons, the button
  focus and taps that run a tool or reach a widget. Editing works with the buttons: the pencil or a
  long press starts it, an item's options offer *Move*, *Change the size* and *Remove*, and the page
  is written when an item is set down. *Add*, the picture behind the page, the status bar and the
  hints row, icons and the finger's drag follow.

- **Picture home screen: the picture and the bar's decisions for interface 7** (#228). How the chosen
  picture is fitted, placed, toned and kept, what a JPEG says about its size and turn, which files
  of the folder are pictures, and where the status bar sits are decided without the host, as the
  current screen decides them, with allocation that can be refused; the screens follow.

- **Picture home screen: its declarations on interface 7, first slice** (#228). The group and its
  28 settings (picture, bars, grid, pages), the screens, the `core.home` offer, the pages as a place
  of widgets and the bar's place of status items are declared and checked against the interface-6
  registration. The screens themselves follow.

- **Picture home: the status bar's items are edited on a screen of its own** (#228). The three item
  settings name the screen `status-items`, built on the UI kit's status items editor, as their
  editor, instead of the host's status items screen; the dashboard and its callbacks follow with
  the native entry.

- **Picture home runs as a native interface-7 plugin where the store's plugins are linked** (#228). It ships switched off, as in Beta 3; switched on, it takes the home screen. Its settings are kept in the program.

- **Picture home screen: the native entry, first part** (#228). The picture chooser and the editor of
  the status bar's items run as interface-7 screens, and the declarations are built into the plugin
  image. The dashboard, *Add* and the picture's decode follow.

- **Picture home screen: the picture behind the page on interface 7** (#228). The dashboard asks
  the host to prepare the chosen picture (the folder and the picture, the fit, the place, the zoom and
  how much paler it is), says *Preparing the picture* while that takes place, draws it first behind the
  page (not while editing) and gives it back when the screen closes. A picture that cannot be made
  leaves the page plain and says why. The turn of a phone's photo, brightness and contrast, dots by
  force, stretching and the bar's ink over the picture are not done by the host and wait; the bar's
  status frame, the hints and the finger's drag follow.

- Fifteen first-party plugins with fixed declaration tables (bluetooth, file-manager, help, home, keyboard, lock, menu, opds, power, reader, settings, aids, news, vocabulary, widgets) also keep the table of the ids they declare in the program, so the kernel no longer copies them onto the heap at the start (about 5 KiB less at idle on the X4).

- **Plugin manager (interface 7): smaller reads at the X4 start and in the list** (#248). The registry and device queries take their request from a 256-byte stack array instead of a 4 KiB heap block, the plugin rows are reserved for the registry's count at once, and the start's notices free their read buffer before they ask for a name.

- **Plugin manager (interface 7): lower start heap on the X4** (#248). The start reads only the names its notices need from the registry, and the *Screens* offers share one pair of read buffers, so the store flow peaks about 8 KiB lower.

- **Nine more linked plugins keep their settings, setting groups and ids in the program** (#248). About, files, mail, notes vault, plugin manager, setup, transfer, update and Wi-Fi get their tables made by their build scripts from the very declarations they submit, as menu does, so the kernel no longer copies each setting, group and id of them onto the heap at start. No behaviour change.

- **Built-in plugins may sleep until first use** (#248, D1). A shell names which linked plugins may sleep (`paperpaw_app::SLEEPS`: about, file-manager, files, keyboard, menu, opds, settings; every other plugin stays awake), and the kernel parks them after their start where its checks allow it. What all sleeping plugins keep of the shell is shared once (`ParkShell`) instead of copied per plugin: `Kernel::register_linked_parked` takes it, and `register_native_plugins` takes the shell's `sleep: fn(&str) -> bool` (e.g. `paperpaw_app::sleeps`) and makes the `ParkShell` itself. A flagged plugin without room for what sleeping keeps is registered awake. A test (`sleep_parity`) runs each flagged plugin's screens and actions asleep and awake and compares screens, replies and saved files.

- **Shorter help for "Sleep after" and "Record battery use"** (#228). Both explanations now fit
  their two lines at the largest text size on the X4 Pro, so they show whole (en and de); so does
  the help of "Plugins may keep awake" on the Boox.

- **Readwise Reader on interface 7, first slice** (#228). The set-up screen, its setting, actions and
  icons are declared for the native plugin interface, and the article ids, the books' start, the lists
  of articles fetched and read, and the copying of an article's text out of Readwise's answer are
  rewritten to refuse instead of aborting when memory runs short. Articles are fetched as before; the
  native plugin takes over once the host runs native plugins.

- **Read-later Cloud and Joplin on interface 7, second slice** (#228). Read-later Cloud's list of
  articles and its answer about one article are read with the SDK's fallible JSON parser, so a
  refused allocation ends the reading with an error instead of aborting; its article lists and
  answers keep the read caps of interface 6 (64 KiB, 16 KiB, 512 KiB, 4 MiB), and its article
  copier is spent after a refusal, so the fetch ends. Joplin reads an item's properties from the last
  2,048 bytes of its file and a notes file's title from the first 1,024, as before, in fallible
  memory. Articles and notes are sent as before; the native plugins take over once the host runs
  native plugins.

- **Readwise Reader runs on interface 7** (#228). The Readwise Reader plugin is now a native plugin only: the articles come from Readwise Reader over the network service and go into the library as books (the folder *Reader*), the access token is asked for on the keyboard and kept with the other passwords and keys, articles read to the end are archived with the next fetch and their books taken away unless they carry your highlights, notes or bookmarks, and a fetch a rule runs is declared work. The texts are the same in English and German, with two new ones (not enough memory; the token could not be kept). A fetch that runs out of memory ends with a notice instead of aborting.

- **The reader on interface 7, first part** (#228). Its settings, screens, actions, menus, icons,
  device needs and key bindings are declared for the new plugin interface, and the book read last
  and the rules of reading aloud are in place without the host and refuse cleanly when memory is
  short. The pages and screens follow.

- **The reader has a stage for text filters** (#228). `reader.filter` lets a plugin such as Aids give rules (bold, replacements, pictures) that the reader applies to each block before layout; it replaces the interface-6 text filters.

- **The reader is a native plugin only.** The old interface-6 form of the reader plugin is removed; the native reader opens and shows books as before.

- **The reader on interface 7: the page count** (#228). The pages of the open book are counted in the
  background, a few at a time, into a small file kept per book and layout (8 bytes a page, as before),
  and the host's reading report carries the page and the number of pages once the count is complete.
  Another layout counts again; the file of the layout left is removed. The native reader takes over
  once the host runs native plugins.

- **Reader: the saved place is also written after a page has rested for two seconds, and when the device sleeps or stops** (#228). A reader no longer loses its place when it is put to sleep with the last pages turned.

- **The native reader can choose words, follow links and show footnotes** (#228). A long press (or the select action) chooses words with a bar to look them up or copy; links are followed with a *Back* button, and a note shows over the page where the reader is set to show them so.

- **Reader: book text is inked by the weight, contrast and sharpness settings** (#228). The interface-7 page drawing sends the font weight, the text contrast, the sharpness, a curve of one's own and smooth text to the host with each run, as Beta 3 did.

- **The reading screen's settings moved to their own file** (#228). `native_view.rs` keeps under
  the length limit; the reading settings and layout key are in `native_view/setup.rs`. No change
  in behaviour.

- **The reader has a stage for a word looked up** (#228). `reader.word-looked-up` carries the word, its sentence, the book's file and title, and the place in the book, for a vocabulary list to subscribe to; it replaces the interface-6 event.

- **Reading goal on interface 7 is complete** (#228). The interface-6 plugin, its registration and its store file are gone. The cat's picture comes from one's own folder; the cats installed with the plugin and the sleep screen's *Today's reading cat* follow once the host can read the plugin's install folder and the sleep plugin offers other plugins' modes. The statistics answer the stage `reading-goal.read-today`.

- **Reading goal on interface 7, first part** (#228). The settings (when the cat is content and happy, and the folder of your own pictures), writing today's reading to the card when a book closes or the device sleeps or stops, and asking the reading statistics for the figure. The cat on the home screen and the sleep screen follow with images.

- **Reading goal on interface 7: the widget** (#228). The *Reading cat* widget is declared natively with its settings, shows today's mood in words with the minutes read and the goal, is refreshed each minute from the statistics, and a tap opens the goal or offers to switch the statistics on or get them from the store. The cat's pictures and the sleep screen follow with images.

- **Readwise on interface 7, first slice** (#228). Reading a book's exported notes and writing a
  highlight for Readwise are rewritten to refuse instead of aborting when memory runs short, for the
  native plugin interface. Highlights are sent as before; the native plugin takes over once the host
  runs native plugins.

- **Readwise runs on interface 7** (#228). The Readwise plugin is now a native plugin only: the highlights of an exported book go to Readwise over the network service, the access token is asked for on the keyboard and kept with the other passwords and keys, and what cannot be sent waits and is sent again when the network is there (declared work, within half an hour), with the same texts in English and German. A highlight that does not fit in memory ends that sending with a notice instead of aborting, and the request body is written a highlight at a time. The plugin hears the reader's export through the reader's stage `reader.notes-exported`.

- **Buttons and gestures, reading profiles and the menu editor are interface 7 only** (#228).
  The interface-6 versions of these three plugins are removed; the native plugins are
  what is left. They run in the app once the host activates native plugins.

- **A native plugin may register up to 32 callbacks** (#228). The cap on event subscriptions plus draw callbacks, counted over the whole construction, grows from 8 to 32; beyond it the batch is still refused with `ERR_NO_SPACE`. The records are kept in a list grown per batch (`ERR_NO_MEMORY` if it cannot grow, nothing staged), so a plugin costs only the heap of what it registered instead of eight fixed slots.

- **Document rooms given back after the details run; `core.work` made at its first call** (#228,
  #248). When the host's details run ends and no document is open, the document table's room,
  the reader's books and the places of the format calls are freed; the next open reserves them
  again, fallibly (an open without room is `ERR_NO_MEMORY` and the job waits). The work
  scheduler and its view are made at the first `core.work` call, in the host's fallible shared
  pointer.

- **The plugin id `core` is the host's own** (#228). A card plugin's `plugin.ini`, a plugin
  bundle, the store's and transfer's installers and the registration of linked or native plugins
  refuse it, so no plugin's action can pass for a host `core.*` action while the lock is up.
  Ids such as `core-tools` stay free. A card manifest's id may also be at most 64 bytes.

- Rules 1.0.8: the interface-7 native plugin (the engine on the host's events, the list with pause,
  conditions and removal, adding a rule step by step); `rules.txt` kept byte for byte, lines this
  version cannot read are kept. Time of day and "every so many minutes" follow the minute tick; the
  interface-6 plugin, its app wiring and its store file are removed (#228).
- Hints 1.0.4: the interface-7 native plugin (hints over a screen, show again, the settings) on
  `paperpaw-ui-kit`'s new `Dialog`; the button-hint labels while the hints are open wait for a native
  hint-row API; the interface-6 plugin and its app wiring are removed (#228).

- **Rules on the new plugin interface: constructed in the host's real scope** (#228). The host's
  limit of callback registrations per plugin is 32 now, so the rules plugin's nine event
  subscriptions are admitted and it is constructed there without refusal.

- The S3 frame scan (`paperpaw-sign frames-s3`) now also follows the code behind trait objects:
  `root trait <path>` and `root impl <type>` budget lines root every impl method of a trait or a type
  (the card's `Storage`, the font packs, `Display`/`Write`, the allocator), so a frame of 2,048 B or
  more, or a call it cannot follow, in `FatStorage`, `Metered`, `Recalled` and the rest is found.
  An `allow form` line names the special register it writes (`wsr.scompare1`); a write of `LITBASE`
  is refused. Both budgets (`frame-budget-s3.txt`, `-classic.txt`) cover the larger set, each line
  with its reason.

- The S3 frame scan can hold every function of a plugin family to the 2,048 B frame limit (`frames-in` line), not only the ones a root reaches; CI runs it as a second, report-only scan of the S3 image with the linked plugins' file. The update and reader plugins' handlers are no longer inlined into one 2 KB frame.

- `paperpaw-sign pack` for the ESP32-S3 (target 9): a plugin may now have a few
  words in a literal pool that nothing reads, the toolchain's dead constants
  (at most 64 bytes, only when no relocation, stored address, call, jump or
  branch reaches them); the packer says which. The packer also skips hidden
  global symbols in its export check. Every other uncovered byte between
  functions is still refused.

- **Plugin library entries read interface-7 files** (#228, decision 142). `tools/plugin-files/catalogue.py` writes `abi = "7.<needs_minor>"` for a format-5 file (header bytes 8..12, decimal) where a format-4 file keeps its fingerprint, checks the file's chip against its device, and names `tools/plugin-files7` as the source of such a build. `tools/plugin-files7/build.sh` builds for the X4 Pro and X4 Classic too (compile only: it reports "pending" until the target-9 metadata profile exists), and CI runs it.

- **The screen queue makes room as it is needed** (#248). A screen callback reserves room for one batch per callback running, not for all four at once, and the room goes back on standby or memory pressure when nothing waits. About 1 KiB less peak heap on the X4 reference set; a refused reservation reports no memory and holds nothing.

- **Bold runs draw through the regular font** (#228). `Paint::Bold` in the UI kit widens the regular font by `max(1, size / 20)` pixels, as Beta 3 did, so a plugin no longer acquires a bold font: `FontRole::UiBold` and `FontRole::Mono` are removed (the host never had `role:ui-bold` or `role:mono`), and `replay_with` and `replay_all` give way to `replay` and `replay_icons`.

- **Smaller plugin files** (#228). The event, draw and input entry points of
  a plugin are thin shims over code shared by all plugins, and the UI kit and
  the config parser slice text with `.get(..)`, so the `char`/`str` `Debug` and
  Unicode tables are no longer linked. Battery 89,640 → 82,944 bytes, End of
  book 244,620 → 228,912, Profiles 130,956 → 123,928. Behaviour is unchanged.

- **Plugins link only the records they use** (#228). The SDK's library, device,
  network and lookup calls now decode and encode the record types they name
  instead of going through the generic record enum, which pulled every record
  type of the wire into every plugin. A plugin file that reads the library
  (End of book) is 137 KB smaller (235,828 → 98,624 bytes). Nothing a plugin
  calls changes. `tools/plugin-symbols.sh` and a source check keep it so.

- **`core.net` `serve` works from any callback of a plugin that has a screen showing** (#228). Outside a screen's own callback (an `event.wifi_changed`, a service event) the server is tied to the plugin's topmost screen, if it is showing and not covered, and stops with it as before. Without such a screen, and in a background work run, `serve` still answers `ERR_STATE`. A callback of a covered own screen gets its server on the topmost screen too; when the work scheduler cannot be asked, the call fails closed with `ERR_STATE`.

- `core.net` servers: the plugin is told `event.server` `started` (with the address) only when the shell reports that the listener opened (`NetReports::server_started`), not when its `listen` returns. A listener that cannot open is reported with `NetReports::server_failed`: the plugin gets `failed` with reason `refused` and the server slot is free again.
- `core.net` has a device-wide cap of servers open at once over all plugins (`SERVERS`, from the resource profile: 1 on the X4, 2 on the X4 Pro and Classic, 8 elsewhere; `NetLimits::servers`). A `serve` past it, or past the plugin's two, answers `ERR_BUSY` at once and opens nothing; the place is free again after the server stops, fails or ends.

- **The host's handle table grows as handles are opened** (#248). It grew by eight slots at a time, so the first service a plugin opened held room for seven more handles (about 1 KiB on the emulator); it now grows by four (one at a time moved the table for every handle opened at start), and a growth without room is refused with no memory while the handles already open stay.

- `command.set_hint` labels are charged to the submitting plugin (`action +
  words + 4` bytes a label, summed over its open screens) against a budget of
  4,704 bytes on the small-heap class and 18,816 bytes elsewhere. A label over
  it is `ERR_NO_MEMORY` and the earlier labels stay. The charge is released
  when a label is replaced or cleared, when its screen closes and when the
  plugin stops. The budget is read in one place, to switch to the plugin's own
  heap budget once the frame-owner charging of `abi/heap-word.md` exists in core.

- **Settings: constructed in the host with all its event subscriptions** (#228). With the host admitting `registry_changed` and `preview_changed`, the native Settings plugin is now built in the kernel's registered scope by a test (it was refused as unsupported before).

- **Opening a settings group asks the registry for far fewer pages** (#243). The native settings page asked for the whole list of settings twice for a group (once for its rows, once to see which parts hold something), and a third time to find the group of a setting to focus. It now asks for the group's own settings, and for each part only until one is found; a setting to focus or to choose from is one call. With 426 settings on a real device this is about 3 pages instead of 16 to 24; the screens are the same.

- **Settings of plugins loaded from the card are described on demand** (#248). The kernel
  kept a full copy of every setting such a plugin declares (titles, help texts, choice lists,
  ids) on the heap for as long as the plugin was installed. It now keeps each setting's
  value, default and a few flags, and reads the rest from the plugin's own data when a
  screen, a search or a change asks for it; what a round of the kernel read is let go at its
  end. Plugins built into the firmware keep their tables and pay nothing. A plugin may
  describe at most 16,384 settings this way.
- **At most 16,384 settings built at run time** (#248), half of the 32,768 before: the
  places of settings now also tell those a plugin describes. The next one is refused as
  before ("no place left for another setting"). Inside the SDK only: no change to
  `abi.def` or `abi.lock`.
- **No room is told as no room** (#248). Where the heap has no room to read a setting's
  description, `command.set_setting` and the `core.registry` setting queries answer
  `ERR_NO_MEMORY` instead of calling the setting invalid or not found, the change is refused
  with a warning and the value stays as it was. Whether a setting is guarded or a secret
  never depends on free memory.

- **The native settings read the registry a page at a time** (#228). Opening Settings no longer builds the list of every setting at once, which cut the peak heap of opening it on the X4 by about 94 KiB; vectors in plugin lists grow in small steps instead of doubling.

- **The interface-6 settings plugin is gone** (#228). `SettingsPlugin`, its pages, chooser, search, themes and passwords code, and the kernel suites that pinned it are deleted; the app no longer registers it. Settings is the native interface-7 plugin alone (`NATIVE_ENTRY`), whose screens, texts and declarations are unchanged.

- **Settings: passwords and keys tested** (#228). The questions and answers of the passwords-and-keys screens (remove, type anew) now run in a test over the scripted host.

- **Settings on interface 7: passwords and keys** (#228). The list of the passwords and keys plugins keep, typing one anew and removing one (asked first) work on the native settings, over the secrets service; the value is never shown. A password or key that could not be saved now says so.

- Setup 1.0.4: the interface-7 native plugin (the first-run guide with dots, a card per answer, a
  sample of the text size; steps from the registry; skip with a question); its marks (`done`,
  `trial`) are kept as before. The interface-6 plugin, its app wiring and its three desktop
  goldens are removed (#228). One accepted difference: the guide notes the first book opened in its
  own data (no library query); a book read while the setup was disabled offers the open-a-book step
  again, which can be skipped.

- **One shared sorter instead of dozens of copies** (#228). Sorting by title, key or rank no longer compiles a separate copy of the standard sort for every list type. The new `paperpaw-sort` crate (also `paperpaw_sdk::sort`) is stable and never panics; short lists (up to 12 items) are sorted in place without any memory, and longer ones fall back to an in-place merge when memory is short, so a sort never fails on a full heap. The X4 image shrinks by about 127 KB; sort order is unchanged.

- **Native plugin screens** (#228). The X4 firmware (with interface-7 plugins, `ppn5`) and the desktop emulator (developer switch) now wire the `core.ui` services beside `text.hyphenate`, sized for the device by its id, so plugins' screen-stack services are served there. Android runs no code plugins and needs none.

- `paperpaw-sign frames-s3` follows a `callx8`/`callx12` whose register an `l32r` loaded (the S3's long call: 71,308 of the 78,606 calls in an X4 Pro image) as the direct call to the literal's address, so the scan reaches the code behind the host entries instead of stopping at the first call. The analysis is a forward pass over the function (an `l32r` makes its register hold its literal; a write, a call from its rotation up, `entry` or a window register takes it back; paths that disagree lose it) and fails closed, leaving the calls indirect and listed, at a branch form it does not model, a branch into the middle of an instruction, a hardware loop, a `jx` in a function with no address to jump to, an address the image points at that is no instruction's start, or a call the flow never reaches. Every address inside the function that a word of the image points at is an entry with nothing known. Literals are read from sections the program cannot write. A `rotw` is judged as a change of the stack pointer (`allow sp`) instead of an unclassified form. One problem line per caller and callee, not per call site.

- **CI holds the firmware's size** (#258). The firmware job builds each device's install image as the release does and fails when it is over its line in `shells/xteink/size-budget.txt`, or when the image or the line is missing (`tools/size-report.py`).

- **The firmware is 100 to 146 KB smaller** (#258). The parts that run seldom (the app's pages for settings, store, Wi-Fi, Bluetooth and updates, the built-in plugins behind them) are now built for size, as the kernel already was; opening a book, turning a page, drawing and reading the card are built as before. The released image no longer holds the paths of the build machine: a crash report names `crates/…/file.rs` and the line as before. `shells/xteink/size-budget.txt` holds each device's image size for a check in CI.

- **A plugin that may sleep is parked again** (#248, D3, Decision 166). On standby every flagged plugin that runs idle is parked again, and when waking a plugin finds no memory one idle plugin is parked at the next safe point; there is no idle timer. Idle means: no callback runs, none of its screens is on the stack, no action, event, hand-over or staged batch waits, and it holds no blob and nothing in any host service (a document, also one its format reads, a transaction, an image, a UI request or widget, a network request or server, a socket, declared work). It is made again at its next use; its handles go with the old instance (50 standby cycles leave heap and handles unchanged), its library receipts stay, and a format plugin's formats stay listed (asking whether a format plugin holds anything no longer takes its formats out of the format index). A plugin's slot is now borrowed only through checked borrows (a failed one answers busy).

- **Another plugin's sleep mode shows through the slot `core.sleep`** (#228). When `sleep.show` names a
  screen another plugin offers for the sleep slot (reading-goal's), `sleep` binds the slot to it; for its own modes,
  an empty value or a mode nobody offers any more it binds the slot back to the sleep screen, which falls back to the
  cover or the cat as before. Binding the slot needs the optional `uses.registry` permission.

- **The built-in sleeping cat takes 14.9 KB less of the firmware** (#258). The picture is stored in 4-bit grays (60,900 instead of 75,778 bytes); it has 16 grays, so it decodes to the very same pixels as before.

- **Sleep screen: its decisions and declarations on interface 7, first slice** (#228). What the
  screen shows, which picture of the folder is next and where a picture over the page goes are in
  place without the host and refuse cleanly when memory is short; the settings, screens and the
  `core.sleep` offer are declared. The screen itself follows.

- **The sleep screen draws its pictures natively** (#228). The sleeping cat, the logo above the
  message, the cover of the book read last, the next picture of the folder and a picture over the
  page are now decoded and drawn by the host through the plugin's image requests, as the
  interface-6 screen drew them. Until a picture is ready the message shows and the screen
  refreshes when it comes. The folder and the card pictures need the optional storage consent;
  without it the cat shows. A cover that is still being made also shows the cat.

- **Sleep runs as a native interface-7 plugin where a build links native plugins** (#228). Builds with `native-linked` list the native sleep screen (the cover, the cat, pictures, widgets, the message and logo, the picker) in place of its interface-6 form, with its settings kept in the program as the other linked plugins' are. Other builds are unchanged.

- **The sleep screen is a native plugin only.** The old interface-6 form of the sleep screen plugin is removed; the native plugin shows the same screens and settings as before.

- **The sleep screen has an interface-7 native entry** (#228). `sleep` shows its screen (the message,
  the widgets you chose, a widget over the page, the page itself) from a native entry, closes when the device wakes
  and takes the presses while asleep as before. The cat, the cover and the pictures of the folder follow with the
  host's image drawing.

- **`sleep` binds `core.sleep` as Kernel ruled** (#228). At start and when the plugins change, a stored foreign mode binds the slot unless the reader has bound it, and nothing is unbound; on a change of `sleep.show` (or a reset) it binds an offered mode and unbinds only a slot bound to another plugin's sleep mode. A screen the reader bound stays across restarts.

- **Smaller kernel rows for each plugin** (#248). The rows the kernel keeps for every linked plugin and its screens take less memory: a plugin whose declarations are read from the program keeps no room for an index, the set of events a plugin subscribed to holds the 128 event tags there are (not 256), a screen's row reads how many instances it may have from its flags, and a plugin's row keeps a waiting replacement as the bytes of its folder. About 2 KiB less at the start and while reading (emulator); nothing a plugin sees changes.

- **Saved files and reads to a file lose nothing at a failed commit.** One
  path (`save_files`) serves `core.net` and `core.sockets`: the staged
  `.pp-staged` file is flushed before it is renamed over the target, and a
  rename that fails after the old file is gone leaves the staged file in place
  as the only copy (the next save to that path replaces it). A read to a file is
  exclusive: no other writer, and no second read or request of the same plugin,
  may hold the target or its staged file.
- **A plugin's own storage writes cannot meet the host's temporary file.**
  `write`, `append`, `rename` and `remove` of a file that `core.net` or
  `core.sockets` holds (the target or its `.pp-staged` file) answer `ERR_BUSY`.
  Names that FAT takes for one are one in this check beyond ASCII (`É` and `é`).

- **Long lists are sorted in place, and the ui-kit sort is the shared one** (#228). The shared sorter held `4 * n` bytes for the positions of a list (40 KB for 10,000 entries, three times that with cached keys); from 2,048 entries on it sorts in place and allocates nothing (about 2.5 to 3 times slower there), so a long list never needs a large block on a full heap. `paperpaw_ui_kit::mem::stable_sort_by` is no longer a quadratic insertion sort but the shared sort, with the same order. The Bluetooth device list places each device found instead of sorting the whole list again.

- A linked native plugin's declared titles, descriptions and choices are read from the program
  instead of being copied to the heap, and its ids are taken from a table its build makes
  (`fixed_ids_source`, `FixedTables::with_ids`) where it has one (#248).

- **Statistics screens on interface 7** (#228). The overview, the books, a book's numbers and the calendar are ported: the calendar chooses a day by tap or key and turns the month by arrow, swipe or page key.

- **Statistics on interface 7 are complete** (#228). The interface-6 plugin, its registration and its store file are gone; the native plugin is the whole plugin: recording, the overview, calendar and books screens, the widgets, the status items and the WebDAV sync, as before.

- **Statistics screens on interface 7: bounded memory** (#228). The books screen lists the 200 books read last (most recent first) and cuts long titles and author lines to 80 bytes; the calendar keeps only each book's id and a cut title. Both declare the `core.ui` service and the host calls it needs.

- **Statistics screens on interface 7: less memory again** (#228). The books screen keeps a slim row per book (id, last open, the cut title and authors) instead of the whole book, and the calendar stops reading the list of books once it has the titles of the day's books.

- **Statistics screens on interface 7: no whole list of books in memory** (#228). The books, book, overview and calendar screens now read `books.ini` one book at a time instead of loading it whole, so opening them takes the same memory for a library of ten thousand books as for ten. The books screen still lists the 200 books read last; the calendar looks up the titles of the day's books as the file is read.

- **Plugin interface 7, draft 33** (#228): storages tell an entry by its own identity (`Storage::entry_identity`: the first cluster on a memory card, the file id in a computer's folder), so the library can tell after a power cut whether a book's move was done; storages that wrap another (the card swap, the emulator's card and traces, the X4's trace, Android's private files) now pass this and `same_entry` on, and the SDK's `MemoryStorage` keeps an identity per file and folder, so moves work in the in-memory emulators too. Plugins built for draft 32 must be rebuilt.

- Opening and reading books needs 4 KiB less memory: a plugin's file read goes straight into its own buffer instead of through a copy (#248).

- **Plugin store (interface 7): the device's facts and the library fetch** (#244). The store reads the device and what is installed from the host, asks the library for its list and a plugin's page, and keeps the index; the screens, downloads and installs follow.

- **A chain of plugins that need each other is a question for each step** (#244). The store offers what a plugin needs after the install of the one before.

- **A disabled row reads on the X4's black-and-white screen** (#244). The facts of a plugin's page (version, size, needs) were drawn in a grey that is paper on 1-bit screens; they are black there now.

- **The store page shows the whole summary, "Needs … free", and the store says "no connection" when its list never comes** (#244). Before, the list stayed "Loading plugins…" when the answer came late and was a refusal.

- **The plugin store's index** (#228). The store keeps the library's plugins in an index on the card as before, now written and read by the new interface-7 code: the same files and format, a list of any length with little memory, and a refused allocation or a card that fails ends in an error instead of a crash.

- **The Plugin store no longer fetches Literata's bold and italic at the first start** (#258, Decision 186). The lean firmware has no Literata of its own any more; `literata-styles` is in the store like any other plugin.

- **The plugin store starts its move to interface 7** (#228). Its two settings and the rules that read an untrusted plugin library (the index line of a plugin, the reading of a plugin's keys from the library's JSON with long texts cut, the files, builds, key rotations and screenshots of a plugin's page and which plugins fit this device, read as the library's text streams in, what a build needs of the firmware (its `abi` as a fingerprint or as `7.<minor>`), the library's address, revoked keys), now written so that a refused allocation never aborts, exist natively beside the old code; the list, the install and the screens follow when the network host and the install design for native plugins arrive. A line of the plugin index with a name, id, author, licence, version or SDK over 80 bytes (a summary over 300) is no longer read: the store never writes one.

- **Store on interface 7: who installed a plugin comes from the device** (#228, #244). The store's installs say they come from its catalog, and it reads from the registry which versions it put in place and as which release (betas included), so it offers updates only for those, as in Beta 3. It no longer keeps a record of its own; plugins it installed before keep their updates.
- **SDK: `Evidence::from_catalog`** (#244) marks a plugin install as a store's (`install_plugin` flags bit 2).

- **A plugin's page in the store looks as in Beta 3 again.** What the plugin does reads in the text's size under its name in the title, and the facts (who made it, its version, what it needs) follow in small print, each with its icon. The empty store and an empty page show a picture over what they say.

- **The plugin store's page and install messages** (#228). What a plugin's page offers (install, update, remove, switch on, read how it works) and the words for why an install failed are now worked out by new interface-7 code with the same rules as before, and the store says why in German and English for the reasons the device now reports (key withdrawn, unknown key, newer PaperPaw needed, version already there, card full, firmware restart first).

- **The plugin store's queue and requests** (#228). The queue of plugins to fetch (*Update all*, plugins other plugins ask for, the first-run offer) and the requests of other plugins are kept on the card as before, now by the new interface-7 code: a queue or request left by Beta 3 is read as it is, and a refused allocation or a bad file ends in an error instead of a crash.

- The plugin store's screens (the start, the views and letters, the source, a plugin's page and pictures, the progress of a fetch, the dialog of plugins other plugins ask for, the first-run offer) are ported to interface 7 beside the interface-6 ones, with the same texts in English and German.

- The plugin store's native entry now opens its screens and carries out what they ask: fetching the library, downloading and installing, updating, removing, the queue and its notices, and the pictures of a page, ported to interface 7 beside the interface-6 code.
- Plugins other plugins ask for (the reader's and others') are offered to the reader in a dialog and the one who asked is told for each plugin whether it was installed, declined, failed or not found, ported to interface 7.

- Home, reader, news and vocabulary have their settings and setting groups as tables kept in the program, like the other linked plugins, ready for the moment they are linked (#248). No change to the plugin files, the interface or what a setting is.

- **A dark frame on a dark page is a card** (it was the page's own paper, so the card was not to be seen).
- **Fixed (#114): the strip between a header and the list under it showed the screen's clear colour, not the theme's paper.** In a theme whose paper is not white (or a dark one) the strip was a white band; lists, windows and item lists now paint the page up to the header. With the default look nothing changes.
- **A theme's light `focus`, `control` and `chip` colours are left out on a panel of two grays** (they would draw as paper, an invisible bar or button); the chosen row's frame there is the `outline` colour, else the ink.
- **A card plate gets its edge where the theme coloured `outline`** (a card the colour of the page had no visible edge at its top and left, only the shadow).
- **Content on a plate takes the ink that reads on it** (a light card in a dark theme gets dark text; the dialog's gray text follows a dark plate).

- **Theming (#114): help, the mail reader, the OPDS browser and home draw their text and paper in the theme's inks** instead of a literal black and white. With the default palette nothing changes.

- **Theming (#114, steps 0 and 1): the interface kit draws its widget frames in the theme's inks, with no pixel changed.** The frames of home-screen widgets (dark, line, card, inverted plate, shadow) took a literal black or white; they now use the theme's text and paper inks, so a dark theme is no longer inverted on widget plates. With the default palette the colours are the same. A test draws every frame in a light and a dark theme and checks that only the theme's inks come out.
- **Fixed (#114): a theme that chooses the soft focus (Pebble does) no longer leaves every native screen empty.** The interface kit refused that choice and failed to draw; it now shows the chosen row as a frame until the kit draws the light bar. **Fixed:** the chosen row's bar was a literal black while its text is the theme's paper ink, so in a dark theme the text vanished on it; the bar is now the theme's text ink.

- **The USB cable transfer runs as a native interface-7 screen** (#228, #244). When a computer talks over the cable, PaperPaw opens the plugin's own *USB transfer* screen (slot `core.cable`), which serves the cable and asks before anything is written: the question names the plugin and the books offered, an answer counts for that offer only, and *Back* or five minutes without a request ends it. A plugin sent over the cable is installed in one step as before, but PaperPaw's own install confirmation comes first and the answer reaches the computer once it is done. `GET /api/plugins` over the cable is cut to fit one answer and says `next` where more follow (`?skip=`), and `hello` has no `fingerprint` any more (it is `null`: interface 7 builds are told apart by the interface and draft).

- **Transfer on interface 7, first slice** (#228). The transfer, phone keyboard and cable screens, the
  action in the tools menu and the two slots are declared for the native plugin interface, and the
  address shown, the book-name check and the cable offer are rewritten to refuse instead of aborting
  when memory runs short. Transfer works as before; the native plugin takes over once the host runs
  native plugins.

- **Wi-Fi transfer moves and renames files on interface 7** (#228). The page offers *Move* and *Rename* again; a book keeps its place, bookmarks and notes when it is moved or renamed, a book deleted from the page is deleted with what is kept for it, and a book sent into the library folder is in the library at once.

- **The transfer plugin is native only** (#228, #244). The interface-6 transfer (the web page, the phone's keyboard, the installs from a computer and the cable) is removed; the interface-7 plugin does all of it. Nothing changes for the reader once native plugins are on.

- **Wi-Fi transfer on interface 7: the page and its address** (#228). The transfer plugin has its native
  entry: while its screen is open the device serves the web page to the local network, shows the address
  as text and as a QR code with a key drawn anew each time, and answers the page: the files list, a
  download, an upload to a free name when the card has room, a new folder and a delete. A few wrong keys
  lock the page for a minute. Closing the screen stops it. Moving and renaming files and the USB cable
  wait for what they need from the library and the host; the plugin is not offered as a file until they
  are native.

- **The transfer plugin is a native interface-7 plugin, complete** (#228). It serves the Wi-Fi page (files, moving and renaming through the library, installing plugins, the phone's keyboard, going online, the reader's own network) and the USB cable screen, and is linked under `native-linked`; the interface-6 form stays until native plugins are activated.

- **Translation on interface 7 is complete** (#228). The interface-6 plugin, its registration and its store file are gone; the native plugin is the whole plugin. The panel, the DeepL request, the key and the language setting are as before.

- **Native SDK: plugins link only the record codecs they use** (#1156, #228). The generated
  records codec has a typed form (`WireEncode`, `WireRecord`, `RawRecord`, the `_of` encoders,
  `RecordStream::feed_raw`) and every SDK facade encodes and decodes by record type. Battery's
  X4 plugin file shrinks from 239 KB to 90 KB; the wire format is unchanged. Interface draft 24
  (`native::DRAFT`) refuses plugins built for draft 23.

- **Cleanup of widgets and status values** (#228). When a screen closes, the widget instances it made are freed. A status handle ends with the callback that got it. The cached status values end on the status-changed event and each minute, and a lock screen starting drops the cached private ones.

- The clipboard (`core.ui` op 18) reserves room for a text written in parts a few times (doubling up to the 4,096-byte cap), not once a part; the copy out to the system clipboard is its own step, tested for the log line when it finds no memory.

- **The screen stack service reads its state from the running call** (#228). The `core.ui` service no longer needs a separate host object from the shell; one call in `shell_wiring` (`HostWiring::ui`) wires it.

- **Open screens no longer hold spare room** (#248). `ui_kit::Screens` grew its lists to four rows at
  the first screen; with a reader's view of about 900 bytes that was 3.6 KiB held for one book. It now
  grows exactly (`mem::push`), so a plugin with one open screen holds one row.

- **UI kit: list over a row source** (#228). The page arithmetic of `WindowList` is its own tested function; the docs say a chosen row must be checked against a fresh `len()`.

- The `core.ui` limits and admission rules for the small heap (the X4 profile)
  now follow the memory class a device declares (`MemoryClass::Small`) instead
  of a list of device ids. `HostWiring::ui` takes the `MemoryClass`. A device
  that declares `Large` (the X4 Pro and X4 Classic) keeps the large limits.

- The store of dialog and screen hand-overs grows by exactly one entry and gives its list back when the last one is freed, so it holds no spare room while reading (#248).

- **Update on interface 7, first slice** (#228). The update page, its settings, actions, icons and
  first-run step are declared for the native plugin interface, and the weekly look's timing, the
  counting of devices and the reading of versions are rewritten without allocation. Updates work as
  before; the native plugin takes over once the host runs native plugins.

- **Update is an interface-7 plugin only** (#228). The old form of the update page is gone: the app links
  the native update plugin and no other, and the programs that do not run native plugins have no
  *Updates* row until they do. What the old page did that the native one does not yet, until the device
  gives the plugin what it needs: the dual-boot notice with its two buttons, plugins built in now that
  come back as files after an update, the fetch of plugins that went on after a restart, the page opening
  at once for a check run from an action, and another firmware's image at the top of the card. The files of the old fetch queue are no longer read.

- **Update on interface 7: the page for an update file on the card** (#228). The update plugin has its
  native entry: the device checks each update file in the card's `updates` folder, and the page offers
  the newest one that fits with its version, who signed it (PaperPaw, or your own key shown by its
  fingerprint, which installs only with that key), and whether it is newer, older or the same (an
  older or the same version installs only when you choose it, told so); another firmware's image is
  offered as such, never blind. It sends the install to the device (which checks the file again and
  always asks you first), shows how far it is, restarts when the update is in place, says after the
  restart how it went and removes the file. A newer update on the card is offered once at start.
  The plugins an update leaves out and the dual-boot notice are not native yet; the plugin is not
  offered as a file until they are.

- **Update on interface 7: searching online and downloading** (#228). The update page's *Search online*
  button asks the update server (the address in the developers' settings, betas as you chose) and
  offers a newer PaperPaw with its size; *Download* saves it into the card's `updates` folder with its
  progress, checks its size and hands it to the page that installs it. The code plugins you have
  installed and the server has builds of for the update are fetched after it, and the device asks you
  before each is put in place. The weekly look for updates (when you turned it on) and the check a rule
  asks for run as declared work, at a cheap moment: when Wi-Fi is on anyway or the device charges.
  Not native yet: the plugins an update leaves out, an update that waits for plugins, the dual-boot
  notice and the badge on the update entry.

- **Update on interface 7: an update that waits for plugins** (#228). When the update server names a newer
  PaperPaw but plugins you use have no version for it yet, the update page says which plugins it waits for
  (and which can never come along), and offers *Keep waiting*, *Update anyway* (the offer then says what
  stops working) and *Check again*. PaperPaw asks the server once a day at a cheap moment; if you have not
  opened the update page within a week, it opens once by itself. The file `waiting.txt` is the same as
  before. An update you download anyway is not asked about again.

- **Vocabulary flash cards on interface 7** (#228). *Flash cards* is back in the Tools menu with the decks and the cards due, and the review: show the answer, then Again, Hard, Good or Easy, each with when it brings the card back. A card answered Again comes back in the same review. The phone page, the memory-card folder, the word list, the sleep card and the widget follow.

- **Without the native plugins, flash cards are no longer offered as a sleep screen.** The interface-6 sleep screen is gone, so the choice had nothing to attach to.

- **Vocabulary on interface 7, first part** (#228). The settings for keeping and marking words and for the flash cards, and putting the word list and the decks right at start after a power cut. The screens, the review and the rest follow.

- **Vocabulary word list on interface 7** (#228). *Vocabulary* is back in the Tools menu: the words you looked up, newest first with their sentences, one page at a time, and holding a word asks whether to delete it. Looking a word up again, going to it in its book and *Export* follow with their parts.

- **Wallabag on interface 7, first slice** (#228). The set-up screen, its settings, actions and icons
  are declared for the native plugin interface, and the article ids, the lists of articles fetched
  and read, and the test for an article read to the end are rewritten to refuse instead of aborting
  when memory runs short. Articles are fetched as before; the native plugin takes over once the host
  runs native plugins.

- **Wallabag on interface 7, second slice** (#228). The list of unread articles and an article's
  archived flag are read with the SDK's fallible JSON parser, so a refused allocation ends the
  reading with an error instead of aborting; the lists of articles fetched and read keep the read
  caps of interface 6 (64 KiB and 16 KiB) and say so when a file is over them. Articles are fetched
  as before; the native plugin takes over once the host runs native plugins.

- **Wallabag is counted as a finished interface-7 port** (#228). Its plugin file for the X4 (about 148 KB) is built with the others and counts against the Beta 3 file in the flash gate; nothing changes for the reader.

- **Wallabag is an interface-7 plugin** (#228). The set-up screen (server, user name, password, API client's ID and secret), the three settings and the fetch are native: the token, the archiving of articles read to the end, the list of unread articles and the books go through the network and library services, and the books are put into the library with `Library::import` and taken away with `Library::delete` (a book with the reader's marks stays). The password, the client's secret and the token are zeroed wherever they are copied. The interface-6 plugin is gone; Wallabag is back in the app with the host's activation of native plugins.

- **Weather on interface 7, first slice** (#228). The settings group, the place and its screen, the
  units, how often to fetch, the actions and the icons are declared for the native plugin
  interface, and the numbers, dates, coordinates, addresses, the weather kept in its file (read to
  4 KiB as before), the conditions, the units and the timing are rewritten to refuse instead of
  aborting when memory runs short. The weather is fetched and shown as before; the native plugin
  takes over once the host runs native plugins.

- **Weather on interface 7, second slice** (#228). The forecast and the places a search finds are
  read with the SDK's fallible JSON parser, so a refused allocation ends the reading with an error
  instead of aborting, and the weekday of a date near the ends of the range no longer overflows.
  A place's name, region and country are cut at 120 and 80 bytes. The weather is fetched and shown as before; the native plugin takes over once the host runs
  native plugins.

- **Weather runs on interface 7** (#228). The weather plugin is now a native plugin only: the two widgets (*Weather now* and *Forecast*), the place screen (search by name, the places found, *No place*, the offer to go online in the offline mode), fetching from Open-Meteo over the network service, and the timer that looks again (the work service: only while the radio is on anyway, after the interval set, at the wake and once online) behave as before, with the same texts in English and German. Out-of-memory ends a step instead of aborting, an answer is read in pieces so it is never held whole, and the plugin no longer keeps interface-6 code beside the native one.

- **Widget owners hear of dropped instances** (#228). When a widget instance goes (its maker dropped it, its screen closed or its maker stopped), the plugin that declared the widget gets `event.widget_dropped` (85) at its widget's token, so it can free what it kept for the instance. A drop waits until the first delivery attempt is over, whatever its outcome, and is dropped at once when its owner stops; live instances and undelivered drops share the widget cap, and at it no new instance is made (`ERR_FULL`).

- A test pins that the SDK's widget list never sets bit 0 of the registry query flags, so the home widget picker (which reads it) offers no preview widgets.

- **Widgets: the logo widget on interface 7** (#228). The logo (PaperPaw's own or the picture set in `core.logo`) is decoded and drawn by the host; the name beside it, its size, font and look are as before. PaperPaw's own logo is the picture installed with the plugin.

- **The widget pack is native only** (#228): the interface-6 pack, its tests and its place in the app and in the
  plugin files are gone; every widget runs on the new plugin interface.

- **The "Next up" widget runs on the new plugin interface** (#228): the next book of the series, the newest
  new book or one by chance, with its cover. Which books are new is now what the library says.

- **Widgets: no writable static data** (#228). The quote widget's count of showings is held by the plugin, not in a global, so the widget pack can be packed as a native plugin file. Nothing changes on the screen.

- **Widgets: the picture widget on interface 7** (#228). The folder is listed with the files service and the chosen picture is decoded and drawn by the host (filled or whole, its name below); the picture of the day, the next on a tap and the folder chooser work as before.

- **The "Book progress" and "Now reading" widgets run on the new plugin interface** (#228): the ring,
  cover and bar of the book being read, and its page, chapter and time left. The pages left in the
  chapter are not shown for now (the reading position does not carry them yet).

- **The "Favourites" and "Series" widgets run on the new plugin interface** (#228): the same covers,
  labels, marks and taps. Up to 8 covers show at once (the host keeps that many pictures for a plugin);
  further books show as frames with their titles.

- **The to-do widget on the new plugin interface** (#228). It shows the open
  items of `todo.txt` with boxes as before; a tap on an item ticks it off or on,
  and a failed tick now says so. The list's own screen follows in a later
  change; until then the heading and the empty list open nothing.

- **The to-do list's own screen on the new plugin interface** (#228). *Add* first, then the
  items; a tap ticks an item off or on, a hold or the menu button offers tick, edit, move and
  remove. The heading, "more" and an empty list of the to-do widget open it.

- The Wi-Fi plugin's reads of the radio and of the kept networks take smaller input and reply buffers where the service allows it, which lowers its peak on the X4.

- **Wi-Fi on interface 7, fifth part** (#228). The networks put on the card in `wifi.ini` are read once at a start, kept among the passwords and the file removed, as before. A file that cannot be kept stays for the next start, and a device that keeps no passwords says so.

- **Wi-Fi: the setup with a phone serves its page on interface 7** (#228). The device opens its own
  network, serves the setup page on port 80 and answers the phone through the host's server; a device
  that serves another page already says so, and closing the screen stops serving and leaves the own
  network. The join code and the page's address show as QR codes under the rows; a short memory no
  longer stops the device for them.

- **Wi-Fi: the phone setup's decisions and pages for interface 7** (#228). When the device opens its
  own network, which page the phone gets, what a form sent from it means, when the device leaves for
  the network chosen and when the setup ends by itself are decided without the host, as the current
  screen decides them, and the passwords typed on the page are wiped; serving the page follows.

- **The Wi-Fi plugin's radio policy on interface 7, first part** (#228). The decisions of when the
  radio is on and which network is joined, and the plugin's declarations, are in place without the
  host and refuse cleanly when memory is short; a join that another request replaced no longer
  counts as a failure. The pages follow.

- **The Wi-Fi plugin is linked as a native interface-7 plugin where the app is built with `native-linked`** (#244). The page, the networks, the phone setup and the radio's rules are the ones already ported; the interface-6 form stays in the default build until native plugins are activated. The on/off and settings rows show as words (On/Off) where the kit has no switch row yet.

- **Wi-Fi: the radio's events on interface 7** (#228). The Wi-Fi plugin now brings the radio on when a plugin wants the network and joins the network joined before, looks again after a scan, hears a join that failed (a wrong password stops being pending, the next network is tried or the failure told), switches the radio off after its idle time (as a timer through the device's work service) and at once when home or a book shows, and says so when no network is coming. The phone setup and the typed password's keeping once online follow with their parts.

- **Wi-Fi on interface 7, fourth part** (#228). The Wi-Fi page, the networks in reach and the setup
  with a phone are native screens: choose a network (a kept one joins at once, a secured one asks for
  its password), forget a kept one with a long press, and the password is kept once the device is in
  the network. Where the device cannot keep passwords the user is told, and the network still works.
  The password of the device's own network is only ever made from a fully random draw.

- **Wi-Fi on interface 7: the hint for a device that cannot keep passwords** (#228). The Wi-Fi plugin says it with the shared text `core.secrets-unsupported` instead of a text of its own.

- **Wi-Fi on interface 7, third part** (#228). The networks joined before are kept among the
  plugin's secrets, and the rules of the device's own network for a phone (its password, the page's
  token and the code that joins it) are in place without the host and refuse cleanly when memory is
  short. The passwords drawn and kept are wiped after use.

- **Wikipedia names its source and licence in About again** (#228). The native plugin answers the About page's stage with the line for the articles' licence (CC BY-SA 4.0), in English and German.

- **The Wikipedia lookup is a native plugin on interface 7** (#228). The summary screen fetches the
  article's summary through the network service, shows it with its source and licence, and fills the
  slot for the online lookup. The interface-6 plugin is removed. Wikipedia is not yet listed beside the
  dictionaries in the lookup panel (the host cannot answer a source that must fetch yet).

- **Wikipedia offers to go online** (#228). When the device is offline and a summary cannot be asked for, the screen offers to go online (confirm or a tap) instead of only saying that a network connection is needed.

- **One shared wipe for passwords and typed secrets** (#228). `paperpaw_ui_kit::mem::wipe` and
  `wipe_text` overwrite a buffer in a way the compiler cannot drop; the keyboard and the Wi-Fi
  policy use them instead of a copy each.

- **`core.work` `declare` needs a `work_due` subscription** (#228). A plugin that did not subscribe to `event.work_due` (103) gets `ERR_STATE` from `declare` (after the form checks), as `core.net` asks for `http_done`: work it could not be told of is never kept.

- **Highlighting with the buttons is a few key presses** (#235, Decision 221). Hold Left starts choosing text; Left and Right move by word, the page buttons by line, holding Left or Right goes to the first or last word of the page, holding a page button by paragraph. Select opens the highlight bar for the word (*Highlight* is in focus: Select again highlights); holding Select widens the word to its sentence and its paragraph, then Left and Right move the end, holding them the start. A selection goes on over page turns (up to five pages; *Highlight* and *Note* take any length, *Look up* and *Copy* only one page) and shows its own legend at the bottom. On the X4 Pro a tap on a word still opens the bar, the page may be turned while it is open, and a tap on the new page moves the end. Reader 1.0.19.

- Progress sync (kosync) now also continues a comic (CBZ) in Readest and back: a book of whole pages sends its 1-based page number as the progress, as KOReader and Readest do for a fixed layout, and a page number received opens exactly that page. Books are matched with Readest by the same partial MD5 and EPUB positions by the same XPointers, so those already worked (#239).

- **The install image is checked against PaperPaw's own app partition, not CrossPoint's slot** (Decision 179). From Beta 4 PaperPaw installs by flashing; the native images are larger than CrossPoint's 6,553,600-byte app slots, so CI no longer fails a build for that (it says how far over the slot the image is) and fails one that no longer fits PaperPaw's own 8,323,072-byte partition. A device that keeps CrossPoint's layout still gets the update page's notice before a download that cannot fit.

- **Notices wait their turn** (#243). A notice shows one at a time; those that come meanwhile (or while a screen guards the device) wait in order, at most 4 (2 on the X4), the oldest dropped. A background work run may now send `command.notify` with the `announce` bit (device messages and rules the user set up); without it the command is still refused for a run.

- Tooling: interface 7 is one interface until 1.0. A pull request that changes the plugin interface
  carries its regenerated lock and reference files; the draft steps, blessing lines and baseline
  replacements are gone. The append-only checks come back once a minor is released (#243).

- **The X4 Pro and the X4 Classic host the native plugins like the X4** (#243). Their frame budgets cover the linked plugins, and `tools/check-x4-start-stack.py --s3` checks the S3's start paths.

- **The X4 and X3 run the built-in plugins as interface-7 native plugins** (#243, Beta 4). The firmware links and hosts them by default and loads plugin files of the new format. The install image is about 1.3 MB larger. The X4 Pro and X4 Classic keep the interface-6 plugins for now.

- **Three app tests that need the native Settings, home and reader say so.** They run with `native-linked-reading` (the native test step, now with the `bands` test too) and are skipped, with the reason, in a build without it.

- **The default fonts are a table in the program, not an interface-6 plugin.** `fonts-default` (1.0.3) implements no `Plugin` any more: `DEFAULT_FONTS`, `DEFAULT_FONT_PACKS` and `LEAN_FONT_PACKS` are tables (`paperpaw_sdk::BuiltInFonts`, kernel feature), and a shell or test adds one with `KernelBuilder::built_in_fonts(&TABLE)` where it wrote `.plugin(DefaultFonts)`. The same fonts, roles, families and names are registered in the same order, and the list of plugins shows "Default fonts" as before. The startup screen makes the UI font alone from the table. With this no plugin under `plugins/` implements the interface-6 `Plugin` trait (#244).

- **The `frame-test` sample is gone (#244, Decision 204).** It was a screen of the old Rust plugin API, built in only with the emulator's and the X4 test builds' `frame-test` feature, both removed with it.

- **The sample plugin with code is the plugin `paperpaw-dev new --code` makes** (#244): it now shows a setting (the name to greet) and an event (the books opened since the start, counted), as the old `hello-plugin` sample did for the kernel's own plugin interface. The guide *Writing plugins* starts from it: `new --code`, edit, `build`, run in the emulator. The emulator feature `hello` is gone.

- For developers: what the kernel's own parts, the shells and the tools register with is the kernel's own type now (`paperpaw_core::Items`); the SDK no longer has `Registrar` or the old `Plugin` trait. Nothing a device does changes, and plugin files built before load as before (the interface fingerprint is the same).

- Kernel: its own contributions, plugins of data, the built-in fonts and the Pebble theme register through functions over `Items`; a plugin with the old `Plugin` trait is taken through an adapter. Nothing a user or a plugin sees changes (#244).

- Kernel: `KernelBuilder::items(manifest, |items| ..)` takes contributions as a closure, so a test fixture no longer needs a type with `impl Plugin`; `Items` is the name its calls go through (#244).

- **The kernel's `Items` loses the nine calls nothing made** (#244): `add_extended`, `add_keymap_when`, `add_book_badge`, `add_hyphenation`, `add_lookup_provider`, `add_reader`, `add_gatt_service`, `add_status_place`, `add_widget_category`. The SDK's Bluetooth module drops its example on the old route (Senior R1 on D4b).

- **The kernel's screen stack holds its own kind of screen (#244, slice D5).** The stack holds the kernel-private `Layer`; interface-7 plugins' screens and the kernel's question dialogs are layers, and a screen of the old SDK `Screen` trait is held through an adapter until the last one is gone. Nothing a user sees changes.

- **The build of plugin files of the format before interface 7 is gone** (#244). `samples/hello-native`, the test plugin built apart (`crates/paperpaw-core/tests/native-plugin`), the reproducible-build check, the "same in any folder" check and the old-plugin check (`tools/table-compat.sh`) with their CI steps. `paperpaw-dev new --github <id> --code` says that the tool does not yet support plugins with code for the new plugin interface (data plugins are supported), and the author release workflow releases a plugin without code only. Devices refuse such files since the old loader went; nothing a reader has changes.
- **Known gap: no template for community plugins with code on interface 7** (#244). `paperpaw-dev new` and the author release workflow scaffold, build and release plugins without code only. A scaffold and a release workflow for plugins with code on the new interface are not written yet.
- **`paperpaw-sign` makes and reads plugin files of interface 7 only** (#244). `pack` no longer makes a file of the format before 5, the `fingerprint` command is gone, and `plugin`, `sign-plugin` and `check-plugin` name an older file as that in one line. `check-publish` reads a release's builds as files of format 5 (header, digest, words to relocate, signature); `tools/plugin-files/check_files.sh` takes `none` as its fourth word only.

- **Plugin files of the format before interface 7 are no longer loaded anywhere** (#244). The kernel's `CodeLoader::load` is gone: a shell's loader hosts interface-7 files (`load_native`), and for any other file the kernel itself says why it does not run. A plugin file of Beta 3 on a card is listed as *built for an older PaperPaw* (switched on, not loaded, never taken for damaged, its file left alone), tested with a file Beta 3 shipped. The X4 firmware loses the old load path, its placement in flash and PSRAM and the function table those files called; such a file is no longer code to start again for after an update.
- **The emulator hosts interface-7 libraries with `--trust-code` alone** (#244; Linux x86-64). `--abi7-dev-loader` is no longer needed for that. A library built with `plugin!` (interface 6) is listed as built for an older PaperPaw and nothing of it is loaded; on other computers a card's code is listed as not supported.

- **SDK: the code for plugin files of the format before interface 7 is gone** (#244). `paperpaw_sdk::plugin!`, `native::take` and the hidden module `native::support` (what the macro expanded to), and `Native::fits` and `Native::problem`, which judged such a file's header against this build. In `paperpaw-native`, `Relocator`, `check_relocations` and `Header::check_file` leave the public surface (they are built for the crate's tests only). Nothing in PaperPaw called them since the old loader went; a plugin is made with `native_plugin!`. A Beta 3 `code.ppn` is still named as made for an older PaperPaw (`Header::read`, `Header::read_older`, `Native::problem_of_file`).
- **`paperpaw-sign plugin` fails on a plugin file of a format before 5** (#244). It used to name the file as made for an older PaperPaw in one line and succeed; it now refuses it with that line as its error, as `sign-plugin` and `check-plugin` do.

- **The kernel's old Rust plugin route is gone (#244, Decision 206).** `paperpaw_core::Plugin`, `KernelBuilder::plugin` and `plugin_off` and the kernel's `register_plugin` are removed: what registers without a plugin file is a manifest and a function over `Items` (`KernelBuilder::items`, `items_off`). The unused adapter `FlatPlugin` (`Instance::into_plugin`) goes with its tests; what they pinned of the production route is tested there. The `interlinear` sample and the two ignored app tests on page layers (`interlinear.rs`, `layers.rs`) are removed; page layers come back after Beta 4 (#284). The web emulator's `set_plugins` is removed: the web module runs no plugin with code, and `check` with a code plugin's id is refused. `frame-test` stays, registered through `items`.

- **The old SDK screen API is gone (#244, Decisions 194/206).** The kernel's own screens, its stock screens (the choice and yes-or-no question every plugin asks, the status items chooser, the widget list editor) and the shells' and tools' screens are now the kernel's layers. The SDK's kernel-only `Screen` trait, `ScreenSpec`, `FixedScreen`, `ScreenFactory`, `call_since`, `revision_holding`, `Env::make_screen`, the screen host (`host::ScreenHost`) and `chrome::ChromeSpec` are removed. Chrome bars are added with the kernel's `ChromeSpec`, and `chrome::Edge` is now `ui::look::Edge`. Interface 7 is unchanged: `api.txt` and the FINGERPRINT are the same.

- **Picture home (interface 7): the picture's recipe** (#244). The picture behind the dashboard is turned as a phone's EXIF orientation says, goes through the *Brightness* and *Contrast* curve, is drawn in dots or in every gray when *Render* says so, and *Stretch to the screen* now stretches instead of cropping. A status bar without a plate takes white or black ink from how bright the picture is behind it. Many tiles and a replaced file of the same name stay named in `PORTING-7.md`.

- **A release's table for plugins is the interface and its minor** (#244, #275). `paperpaw-<device>-<version>-abi.txt` is two lines now, `interface: 7` and `minor: N` (the newest minor of the interface a plugin file may need to run on that firmware), read from the firmware's ELF file (`PAPERPAW_NATIVE_ABI`, version 2; `paperpaw-sign abi`). The fingerprint line and the `api:`, `families:` and `min:` lines of the plugin files before interface 7 are gone: an interface-7 plugin file has no fingerprint, and the release no longer checks one. Instead the release checks each plugin it builds for the store against the firmware (`paperpaw-sign fits`: its chip, its interface, no newer minor), where a sample of the old format was packed before, which proved nothing for interface 7.

- **SDK: fifteen more modules show a plugin only what interface-7 code uses** (#244). Without the `kernel` feature `annotations`, `builtin`, `decorations`, `filters`, `help`, `install`, `library`, `logo`, `lookup`, `readers`, `reading`, `status`, `time`, `ui` and `widgets` are facades of 60 items in all (the calendar functions and time zones of `time`, the ids of `builtin`, `widgets::grid`'s geometry, `reading::WordBox`, `install::Package` and the like); the old widget tree, the catalog and the rest are the crate's own there. With the feature nothing changes. The check that builds every plugin's library alone now also fails where a build has the SDK's `kernel` feature on without being listed, and takes each crate's features from `cargo metadata`.
- **SDK: `native::events::PowerChange::level()` returns `native::device::PowerLevel`** (#244), the type
  `PowerInfo::level()` already gives. It returned a type behind the `kernel` feature, which a plugin
  could not name, so it could not tell "saving" from "offline". `tools/interface.py` now fails when a
  public signature of the SDK names a type its listing does not declare, beyond the eight known ones.

- **SDK: the old plugin API is public only with the `kernel` feature** (#244). `Plugin`, `Registrar`, `Screen`, `Cx`, `Env`, `Command`, `Commands`, the events, actions, menus, fonts, formats, plugins and status stores, and the modules `ask`, `bluetooth`, `chrome`, `collections`, `contribution`, `developer`, `event`, `host`, `layers`, `power`, `presets`, `render`, `replies`, `services`, `setup`, `sound`, `voices`, `words` and `work` are the crate's own in a build without it, so a plugin cannot name them; the kernel, the app and the shells build with the feature and see everything as before. A plugin is written against `paperpaw_sdk::native`. Still public beside it: ids, storage, the setting specs, `MenuNeed`, `Handled`, and the modules interface-7 plugins use (their functions that take an `Env` or give a `Command` follow in a second step). CI builds every plugin's library and the UI kit alone (`tools/ci/plugin-libs-alone.sh`), since the workspace's build turns the feature on for everyone.

- **The shared function tables are gone** (#244). Interface 7 never used them: a plugin file carries its own copies. Removed: the crate `paperpaw-table` with `shared!`, `shared.list`, `table.sig` and `table.body`, the SDK's `shared` module (kernel feature), `tools/shared-table.py` with its tests and CI step, and the "shared functions" line of the X4 remote control's description. The about 160 functions that were declared with `shared!` are plain functions with the same bodies. `Native::family_revs` and `family_min` stay as unused fields, all 0, so that the fingerprint does not change. A change to the body of such a function no longer needs `table.body` written anew.

- **Emulator scripts can take the card out** (#244). The steps `card-out` and `card-in` of `--script`: while the card is out, everything asked of the storage fails as on a card that went. For tests only, `Kernel::show_firmware_confirm` (feature `testing` of `paperpaw-core`, never in a firmware) shows the firmware confirmation without a firmware host.

- **Code plugins copied to the card run without being installed (Decision 205).** A plugin folder an author copies to the card runs its code at the next start, unsigned and without a question; the plugin manager can tell it apart (`reg_plugin.flags` bit 6, "not signed"). It does not run for an id that has an author's key pinned (a plugin once installed signed), nor when it declares installing plugins or firmware (`uses.install`, `uses.firmware`): those still need an install. An installed plugin still runs only the file installed.

- The interface-6 Wi-Fi plugin is removed; the native plugin is the whole Wi-Fi plugin (held until the host activates native plugins).

- DOCX and MOBI answer the library's book details without converting the book. An open with the host's `details` flag (bit 3) is ready at once and answers the title, authors, language and subjects from the document's properties (DOCX) or its header (MOBI), and the cover from the book (MOBI); nothing is converted or written, the other asks are refused, and a normal open prepares the book as before. Index builds (`background`, bit 2) convert as usual.

- **A store plugin's texts no longer sit in memory whole** (#248). A code plugin's `i18n/*.ini` catalogs were read into the heap in every language, the one in use or not: some 27 KB for `stats`, which on the X4 split the largest free block (65 KB → 39 KB at Home, 54 KB → 27 KB with a book open) and brought the "memory is short" notice with that one plugin. The larger text catalogs of a code plugin are now compiled once into `.paperpaw/cache/catalogs/` and read from the card as they are needed, about a kilobyte of memory a language; a changed text makes a new copy and removes the old.

- **Covers and conversions wait for a quiet second** (#248). The library's covers are made, converted and started only once the user has not pressed a key for a second (and a second after the start), and a cover that is asked for sooner is answered "not now" without opening the book in the list's frame, so the zip and XML of a book (some 20 KiB of memory) do not come beside the list or an overlay the user is working in (the first open after the quiet time still happens in the list's frame). The screens ask again when it is quiet.

- **A linked plugin's declared services are no longer copied** (#248). The host borrows the list from the plugin's checked metadata, which is the program's read-only data, instead of copying keys and entries onto the heap for every linked plugin. Loaded card plugins still get a copy.

- **A Word document converts in less memory** (#248). The cache's blocks were collected in a buffer that grew by doubling (old and new copy held together), and could wait for the card with several blocks in it. They are now sent to the card before a block would not fit with them, and each gets its room once, to the byte: the hostile Word document of the X4 memory test peaks at 211.8 KiB instead of 222.2. What is written is unchanged.

- **A book's cover for the library is made without building the book** (#248). The library's cover open is a background open now (`document.md` 2.1), and a format skips what only a shown book needs: the EPUB family's open no longer builds the contents, the style cache and the first chapter for a cover (about 10 KiB less at the peak, once per book). A normal open still finds everything out. With both X4 document places held by other openers' documents the cover open is `ERR_FULL` (the library says busy) instead of taking a place.

- Native plugins: a plugin's pending-event queue is freed as soon as its events are told, so an idle device holds no event queue (about 2 KiB less heap with all plugins linked). Delivery order and limits are unchanged.

- **The first-run hints open in less memory** (#248). Opening the hints over a screen held the buffers of its hand-over while it read the hints with a second pair, 16 KiB at once; the first pair is now gone before the hints are read, and the requests of the screen list, the hints and the hand-over take 256 bytes instead of 4 KiB. What shows is unchanged.

- **Hints switched off no longer take 4 KiB as screens change** (#248). The hints read the screen list on every change, with a 4 KiB reply buffer, even when they were switched off; now they read it only while hints are on or still open. What shows is unchanged.

- **The home screen holds no page while another screen is over it** (#248). The home screen's page (its cells, rows, texts and the widgets made for them) is let go when the screen stack changes and home is no longer on top (a book is read, a settings page is open) and is laid out again when home is shown, as it was at every show.

- **Less heap held by the native host while it is idle** (#248). The event pump keeps a buffer of 256 bytes, not 1,280, and makes the larger one only for the pass that has an event of that size; the places of the `core.format` calls (1 KiB) are made at the first call; the lookup and `core.ui` services are boxed where they are wired; and `Kernel::start` gives back the room the registries and the host's tables kept while the native plugins were hosted one by one (the plugin list, the menus, the subscriptions, the entries, owner rows, setting queues and linked ids).

- **A frame on the X4 takes about 11.6 KB less heap while it is drawn** (#248). The firmware keeps the
  bands of a frame packed, to send them to the panel again after the refresh. It packed each band
  into a second band's worth of memory first and held that until the frame was done; a band is now
  counted and then packed straight into memory of its packed size. What the panel is sent is the
  same.

- **Built-in key maps are no longer copied onto the heap** (#248). Presets, plugins' key maps and the device's key map are kept in the program, so their layer names, button names and action ids are now borrowed instead of parsed into heap copies. The user's and the shell's bindings are still copied.

- **The document and lookup services keep no room until they are used** (#248). The document table, its event queue and the lists of books and unfinished formats are reserved at the first document opened (the list of ended images grows as documents end, as it did), and the lookup service's table of pending asks is made by the first ask that is kept (or the first ready command); a device that never opens a document or waits for a provider holds none of it.

- **Transfer and update keep less on the heap while idle** (#248). The transfer plugin's cable screen and update's download, check and offer state are made when they are first needed instead of living inside the plugin's own block from the start (about 0.8 KiB less at idle on the X4). `ui_kit::mem::boxed` is the fallible `Box::new` for plugins that want the same.

- Library list covers no longer decode a picture inside the list's frame: a cover not made yet answers busy, the host makes it in the background (a ready book at once, beside a conversion the list holds back) and the list is told to refresh (#248, pictures-as-book peak).
- The native formats' books are known to the library through one stand-in format instead of one per format, which lowers the start peak (#248).

- **More stack room on the X4 Pro** (#248). A plugin's batch of commands was checked on a frame of 2,016 bytes, 32 short of the S3's limit; the rarely used parts (Wi-Fi reservations, firmware and plugin installs) now have frames of their own, so the batch takes 224 bytes and the largest S3 frame is 1,824. Nothing a plugin sees changes.

- **The reader reads the screen stack with a small input buffer** (#248). Checking which screen is
  on top before reading aloud took two 4 KiB buffers; the query's input now takes 256 B.

- **Asking the registry for one setting no longer reads every declared extra of the device** (#248, #228). Whether another setting's showing depends on the one asked for was answered by decoding every `setting.extra` record of every native plugin. For plugins whose declarations stay in their image, only the records whose bytes hold the setting's id are decoded now, and each image is read once.

- **Opening Settings holds fewer buffers at once** (#248). The check for passwords and keys now runs through the registry reader's own buffers and before the list of groups is built, instead of allocating a second 5.4 KiB pair beside them; the list of secrets asks for a 256-byte query buffer, not 1,280.

- **The sleep screen asks the host in less memory** (#248). Reading the screen list and its open parameters, and letting its widgets go, took a 4 KiB request buffer each time; the requests take at most 256 bytes, so they now take 256. What shows is unchanged.

- Emulator: the secrets sealer (1.0 KiB) and the lists of services and features offered to the linked plugins (0.6 KiB) are in static memory, not on the heap, so the start holds 1.6 KiB less (#248, P3).

- **A page of a book takes about 8 KiB less memory to draw.** The status lines under and over the text held two reply buffers of 4 KiB and a third of 4 KiB for their request while the registry's answer was read. They now make one reply buffer (a second only for more than eight items) and a request buffer of the size of its items, after the registry's answer is read. On the X4 a drawn page peaks 16.9 KiB above the heap in use in the emulator, now 9.0 KiB. The pages and status lines are pixel for pixel the same.

- Opening a text book reads its 16 KiB head in one pass of 8 KiB pieces (encoding, Project Gutenberg header, the text's look) instead of holding it whole: the library's details open of a text no longer sets the start memory peak, with about as many card reads as before (#248). A Project Gutenberg header line of 8 KiB or more is now left out (no real header has one).

- **Lists let their message buffer go before they paint their rows** (#248). `ItemList` kept the
  4 KiB catalog buffer of its header while drawing every row; it is now freed once the header is
  made, 4 KiB less at the peak of drawing a list (on the X4, the store's install).

- **Catalog texts take small buffers first** (#248). `Messages` started with a 4 KiB message
  buffer and a 1 KiB argument buffer; they now start at 512 and 256 bytes and grow only for a
  longer text: a copied text is asked again once with 4 KiB, a formatted text or digits once with
  the length the host reports, arguments once with 1 KiB. Most screens hold about 4 KiB less
  while they make their texts.

- The Vocabulary plugin makes its state (open screens, widgets, the question asked) when it is first used, so it holds only a pointer on the heap at idle.

- The vocabulary review screen asks the screen stack with a 256-byte request buffer instead of 4 KiB (the reply buffer stays at 4 KiB), 3.75 KiB less heap while it checks that it is on top.

- **The widget pack keeps its state until something needs it** (#248). What the pack holds (its widgets, the to-do list, the cached quotes, numbers, books, pictures and the reading recorder, about 1 KiB) is made by the first widget, screen, question, answer or book event, not when the pack is made; a tick, a dropped widget, a changed setting or the end of a session find nothing to do without it. Behaviour is the same.

- **Wi-Fi reads the screen stack with a small input buffer** (#248). Checking whether home or a
  book is on top took two 4 KiB buffers; the query's input now takes 256 B, 3.75 KiB less at the
  peak of a store install.

- X4: the lists of services and consent features offered to the linked plugins are kept in the program's read-only data for each combination of what the start wired, not made on the heap at the start (#248, P3).

- **A deflated ZIP entry of up to 32 KiB is inflated into a window of exactly its size** (#248), no longer the next power of two: an EPUB's package document or chapter opened for its details or cover takes less heap (Alice's OPF window 8 → its size). Longer entries wrap through the 32 KiB history as before. An entry that inflates past its stated size is still refused.

- `tools/check-local.sh` has a `test-linked` step: the tests of each selected package that has a `linked` feature run with `--features linked` (the plugins' hosted tests are built nowhere else), and `--all-linked` does it for every such package, which the train smoke uses.

- **Large files written by appends take their clusters in runs** (#250). A file that has grown to 16 KB takes up to 64 KB of clusters at a time (a run, where they are free in a row), and gives back what it did not use when it is written back. The FAT is written once for the run instead of twice for each cluster: a 4 KB append onto a card with 4 KB clusters (a decoded picture's cache is written like this) took five write commands and now takes about 1.3 (counted on a simulated card; the cold open of a picture book on the X4 spends about 2 s of its first page on them, an estimate). If the X4 loses power while a file is being written, the clusters of the run stay linked after the last one in use; they read as no data and are taken by the next append or freed when the file is removed. A run lies in one block of the FAT, so that a cut leaves no more taken clusters linked from nowhere than one at a time did.

- **Less stack where a plugin lists or reads the card** (#250). The card's folders are looked up in a loop instead of one frame per folder level, and the one-time look for a half-finished move after the start takes a fraction of the frame it did, so the deepest route from a plugin call to a block read on the X4 is about 1.5 KB shorter (more where the folder is deeper). Nothing on the card is written differently.

- **A book's first opening makes no empty cache files** (#250). Each chapter converted for the first time had its seven cache files made empty before any content, each a whole write to the card (21 writes for the three chapters a first opening converts). A file is now made by the first bytes appended to it, and one without content is not there. What an interrupted conversion left is removed before the chapter is converted again, and the end mark is still written last.
- **An EPUB's file list is read in pieces, not entry by entry** (#250). Indexing the archive took two reads per file in it (358 for a book of 179 files), each a call to the host; the list is now read 2 KiB at a time, and a file is looked up in one read instead of two.

- **A JPEG takes about half the computing to decode** (#250). A cover or photograph shown at more than half its size is decoded at full size, and most of that time went into the general sums of the inverse transform and into a call for every code and byte of the data. The transform now takes a third of the multiplications for the same numbers, and the bits are looked at where they are asked for. For a book's 896 × 1400 cover shown at 512 × 800 the decoder runs 123 million instructions instead of 262 million (measured on the emulator's build, not on a device); every pixel is as before.

- **Checking a file path is cheaper** (#250). Every storage call checks its path, and the layers it passes check it again: about 185 checks for one page turned in the reader, each reading the path four times over. The path is now read once, byte by byte, with the same answer for every path.

- **Reading a setting is cheaper for native plugins** (#250). For every setting a plugin read, the host walked all settings kept as they are to tell whether the one asked for is a secret; it now finds it by the settings' index. The reader reads its settings many times per page turn, so this was a large part of a page turn on the X4.

- **The reader's status lines ask the registry once per page** (#250). A status line drawn band by band asked the registry for its items at every draw. `paperpaw_ui_kit::status_line::draw_kept` keeps the answer in a `Memo` of the caller's between draws; the reader forgets it when its screen is shown again and at an input.

- **The X4's nested-callback stack gate is open by default** (#250). `stage.run`, `ui.compose`, the widget ops 7 and 12-15 and op 14 now run another plugin's callback inside the caller's on the X4 too, so a Home page with widgets of other plugins (battery, progress, news ...), the status items and the aids' text rules work on the X4 as on the emulator. Measured on the real X4 (test image, gate open): lowest stack left 14,720 B in the main menu, 15,012 B on Home, Settings and the reader (limit 8,888 B). The core feature `x4-nested` is replaced by `x4-gate-closed`, which closes the gate again; off by default. The X3's firmware builds with it closed until an X3 has measured the gate.

- **German is a language pack on the X4 family (#251, Decision 155).** The X4, X4 Pro, X4 Classic and X3 firmware no longer has German's texts built in: it carries German as a compressed language pack and writes it to the memory card as the plugin `language-de` where German is the language. A device set to German that updates from Beta 3 (flashing or in-app update) shows German right away at the first start, without Wi-Fi and without any step from the user; choosing German on the first-start screen or in Settings does the same, once. After that the pack is an ordinary language plugin (the store can update it, the plugin manager removes it). The pack is also published in the store as `language-de` (German, user interface), made of every plugin's German texts. The emulator, the desktop and the Android app keep German built in.

- **Pages with pictures show their text first in the native reader** (#252). When a book opens on a page with pictures, or such a page is turned to, its text is drawn at once with each picture's place kept empty at its exact size; the pictures follow in the next frame, refreshing only their boxes, and the text does not move.

- Passwords and keys on an Xteink device (X4, X3, X4 Pro, X4 Classic) are now encrypted on the card (AES-256-GCM) with a key that stays in the device. Values saved by Beta 3 are encrypted the first time the device starts with Wi-Fi or Bluetooth on. A card from another device opens none of them: PaperPaw says so once and asks for them again.

- **The host tests now catch a route that goes too deep for the X4's stack** (#256). A new test crate (`tools/stack-budget`) runs the FAT calls of a plugin's files host, the bounded tree removal, the picture-folder provider on a FAT card and the files service's listing on a thread with the device's stack and measures how deep each goes; a change that deepens one of them past its ceiling fails the normal test run instead of showing as a boot loop on the device. No change for users.

- **The built-in UI font and icons take about 254 KB less flash** (Decision 197). The two packs the firmware carries (PaperPaw Sans and PaperPaw Icons) now store their glyph bitmaps packed (runs of empty and full pixels as a run length, rows without padding), and the text drawing decodes a glyph a row at a time. The pictures are pixel for pixel the same; no screen changes. Packs on the card and the reading fonts keep their format and still load.

- **For plugin authors: a single file that is no book moves on the user's files without the library** (#257, `docs/abi/files.md` 2.6). A `move` or `storage.rename` on root 1 of one file that no format reads (by its name, under the old or the new one) and that the library keeps no details for is the card's rename and nothing else: no book-moved event and no `event.library_changed` is sent, and nothing kept by the file's path (book keys) is carried. A plugin that keeps data by the path of such a file moves it itself. Folders, books and files the library keeps details for (a book whose format plugin is off) move as before, with the library's job.

- **TXT books: the text read around the page is never held twice** (#248, #257). Where the plain-text reader needs a larger piece of the book, it lets go of the old piece and reads it again instead of copying it, so the reader's chapter scan stays within Beta 3's memory on normal books.

- **The next page of a long list of authors, series or tags no longer starts from the top of the catalog** (#257). The host keeps where the last page of names ended (one place, at most 0.7 KiB) and goes on from there: a page after a page reads the catalog twice, however far down it is. Loading all 700 authors of a 1,000-book library 32 at a time took about 320 reads of the catalog and takes 46 now (counted in a host test, not timed on a device). The answers are the same; the place is dropped when the catalog, the list, its filters or the day change, and is not used while a book is being deleted.

- **Picture home screen: icons, widget updates, tiles and a replaced picture on interface 7**
  (#257). Tools show their icon above their name (light on a dark ground where their plugin shows them
  on, faint where not enabled); the pencil, *Done* and *Add* show their icons, and over the picture the
  pencil and the page dots sit on paper again, as in Beta 3. Where no icon font is there the names and
  words stay. A widget that shows something new is drawn again in its place without waiting for the
  page to be laid out. A tiled picture repeats up to 256 times (32 in a row) instead of 64, so small
  patterns tile again; a smaller tile is still not drawn. A picture replaced on the card by another file
  of the same name is made anew the next time the home screen shows.

- The settings' *Search* looks like the field it opens (outlined row), as in Beta 3.

- The X4 firmware no longer carries the translations of plugins it does not have built in
  (News, Vocabulary, the DOCX, MOBI and picture formats): about 42 KB less flash (#258).

- The translations of the plugins built into the firmware take less flash: the program keeps a
  hash of each text's key instead of the key itself, once for English and German together, and
  none of the files' comments (#258). Texts read the same; a plugin's own translation files and
  the languages on the card work as before, and a text a plugin on the card says anew still
  wins. The build fails where two keys of these catalogs would share a hash.
  `paperpaw_i18n::BuiltMessages` (made by `BuiltMessagesTable` in a build script, handed to
  `Translator::built_messages`) is the new form; `paperpaw_app::NATIVE_MESSAGES` holds the
  linked plugins' catalogs, `Kernel::add_built_messages` adds them
  (`paperpaw_app::add_native_catalogs` does it).

- **On the Xteink readers book text is now set in the menus' font (PaperPaw Sans) unless you
  choose another font; Literata is a font of the card** (#258). The firmware no longer carries
  Literata: 257 KB less flash on the X4. The plugin *Literata* (`literata-styles`, before
  *Literata bold and italic*) now brings the regular face too, version 1.1.0: with it on the
  card, choose Literata in Settings, under Reading, as Font. Until then Literata is not
  offered, and a card with the plugin of before keeps its bold and italic closed rather than
  mixing them into the menus' font. The plugin installed while the reader runs offers
  Literata at once; book text changes to it when you choose it, or at the next start where
  it was chosen before. Other devices keep Literata as the book's font.
- **Bold and italic book text comes from the family of the regular book text only.** A bold
  or italic face a plugin proposes, or of a chosen family, is used where regular book text
  is in the same family; else the regular face stands in, as where no bold is bound. A
  binding you make yourself under `[fonts]` is taken as given.
- **Bold book text in a font without a bold face is drawn bolder** (faux bold): the regular
  face with every letter one pixel wider for every 24 px of its size, in the same room, so
  headings and bold words stand out on a reader that has no Literata on its card.
- **A font family is offered for reading and for the status lines only where it has a
  regular face that is there.** A family of bold and italic alone is no longer listed.
- **A font chosen stays chosen while the card cannot say whether its file is there** (busy,
  a read fault); only a file that is gone gives the choice up, also for a font in use whose
  plugin is removed. Literata chosen before, on a card without the plugin, stays in
  `paperpaw.ini` for the start that finds it, and the log names the font and the missing
  plugin rather than an invalid value.
- **A pack that extends a font (a script's letters, larger sizes) is open only while that
  font is**, so packs made for Literata take no memory while another font sets the book.
- **paperpaw-plugin-fonts-default:** new `LeanFontPacks` (the fonts of a lean build, in any
  build) and `LITERATA_ON_CARD`; a lean build registers `fonts-default.literata` as a face in
  that file and binds `fonts-default.source-sans` to the reading role. `paperpaw-app`'s
  `Fonts::Packs` registers the lean set. **paperpaw-sdk** (kernel): new `Fonts::family_there`.

- Lists that are sorted in place (book keys, a ZIP's index, declarations, the status line) share
  one compiled sort instead of a copy of the standard library's per element type: less flash
  on the X4 (#258). `paperpaw_sort` (and `paperpaw_sdk::sort`) gains `sort_unstable`,
  `sort_unstable_by` and `sort_unstable_by_key`: in place, no memory at any length, equal items
  may change places. They take several times as long as the standard library's on long lists
  of numbers (10,000 keys: under a millisecond on a PC), so measure where one is sorted often.

- **The firmware no longer carries the names of all icons** (#258): 58 KB less. The SDK finds an
  icon by the hash of its name. `icons::char` answers as before for every icon's name; a name that
  is no icon's is taken for one where its hash happens to be an icon's (about one in a million).
  The names stay in the SDK for tools and tests (`icons::names()`); nothing in the firmware calls
  it.
- **The firmware no longer carries the standard library's reader and writer of decimal numbers**
  (#258): about 36 KB less. The SDK writes an `f32` with the digits `Display` writes and reads a
  text of up to 19 digits to the float the standard library reads (of a longer one, its first 19).
  JSON numbers are read to the same `f64` as before. A new check (`tools/firmware-symbols.sh`) fails
  the firmware build when that code, or the icons' names, come back.

- **A widget's font is the first setting on its Settings page** (#262): it was the last row, after
  the widget's own settings, pages of button presses away on the X4.

- **Seven kernel functions use less stack, for the x4-pro and x4-classic** (#263). The frames that
  call into a plugin, set the bars up, start the catalog's job, read a JSON file and convert an
  HTML file were over the 2,048 bytes a host frame may have on those devices; each is now under
  it, with nothing changed in what they do. A JSON file is read 1,536 bytes at a time instead
  of 2,048.

- **Home with covers (and the drawer, settings, Library covers and store pages) shows at once in black and white, its grays follow by themselves** (#267, Decision 196). Before, the device first drew the whole screen aside to count its pictures and then in grays, which took about 5 s on the X4 after "Back" from the book. Now the first frame is black and white with the covers dithered, and when the screen has pictures one gray picture follows after 3 seconds in which no key was pressed and nothing else was drawn; a key press while the wait runs begins it again (a focus step too); a screen that already had its grays and gets a key stays as it is drawn until its next own redraw (Home's minute), which is black and white first, and the grays follow 3 seconds after that, and nothing is drawn over a dialog. A screen that had its grays and draws itself again with no key since (Home each minute) keeps them. With "Bilder in Graustufen" off there is no gray picture. Book pages with pictures and the screen a device goes to sleep on keep their grays at once, as before.

- **Home asks the library for its books once for each frame, not once for each band of it** (#267). On the X4 a library query is some twenty card reads, and the first frame of Home after "Back" asked five times (the card and the shelf, band after band) and made 1.3 s of its 2.4 s of host calls. The card and the shelf now keep what the library listed for the bands of one frame and ask again for the next frame and after every event, tap or tick; a cover that is ready stays known when the catalog's generation changes (a book was read) and the tick finds the books changed.

- **Drawing a full frame takes fewer passes on the X4** (#267). Where the heap has room for it, the frame is drawn in two bands instead of five, which cuts the host calls of Home's return from a book by about a third. The shell's panel writes are unchanged.

- The home screen draws faster: it keeps the look it draws in (and the lines of its tools' names) with its page, and each of its widgets keeps its look, instead of reading about 400 settings again at every return; a return from a book makes about half as many host calls.

- **Back to Home from a cover page of a book is quicker on the X4** (#267). After grays the panel had to be driven whole by a half refresh (1.7 s) that began only once Home was drawn (1.5 s). The X4 now cleans the panel to white while Home is drawn and shows it with a fast refresh (0.5 s); this holds for the SSD1677 panel only and for the home screen only.

- **Back to Home is faster** (#267). The home screen keeps the page it laid out while a book or another screen is over it and shows it again at once, where it laid out every widget anew at every return (about 2.7 s of 4.4 s on the X4). It is laid out again only if a setting or a plugin changed meanwhile, the screen was turned, or the recent shelf came or went; what a book changed (the book on the card, its progress, the recent books) shows as before. The page costs about 1.3 KiB of memory while a book is read.

- **Home no longer hyphenates the names of its tools** (#267). On the X4 each question to the hyphenation table was a host call that read the table from the card (13 ms, 21 of them in the frame that returns to Home); a name that no line holds now ends in "…" instead of breaking inside a word. The tools drawer still hyphenates.

- **Home's first frame after "Back" makes 10 service calls where it made 18** (#267, emulator, card and a shelf of six books; the X4 made 19 to 20, about 37 ms each). The continue card, the recent shelf and the check that tells whether the shelf had a book share one library listing for a frame (one query instead of three), a book whose file the library found on the card stays so across the catalog's generations (no `library.book` call after each book read), and a book without a cover is not asked about again, and the card and the shelf share what the host said of covers. A cover that is ready stays known when the tick asks about covers that were still being made.

- On a big screen on its side, the home screen keeps the book card at its natural height and shows the tools on the same page; Beta 3 let the card fill the page and started the tools on page 2.

- Book pages and the screen the device sleeps on are no longer drawn twice where no picture can be on them: the kernel counted the pictures with one more draw of the whole screen before every large frame, also on screens without any. It now skips that count when no screen shown holds a picture of its own; screens with pictures still get their grays as before (#267).

- Tests only: the fonts-default tests check the lean fonts build (the X4 firmware's font set, `PAPERPAW_FONT_LEAN=1`) by what it really has, the UI font and the icons, instead of Literata, and CI now runs them in its feature test step. `a_widget_placed_twice_has_two_looks` uses a widget of the native home, so it passes without `native-linked-store` too.

- Tooling: `versions.py --published releases.json` compares the folder of each data plugin with the
  `<id>-<version>.zip` a release published (GitHub's list of the releases), and fails when the bytes differ under
  the same version, whatever a `Plugin-unchanged:` trailer says; the message names the next patch version (#275).
  `literata-large` 1.1.0 changed its store text without a new version and was not caught by the commit check.
- Tooling: the release page's text is `docs/release-notes/<version>.md` (`tools/check-release-notes.py`: UTF-8, not
  blank, at most 125,000 characters, GitHub's limit). A tag release stops in its first job without it and again in the
  last one; a dry run warns and shows the CHANGELOG section (#275).

- **The lock's help says plainly that it keeps out casual use only** (#276). The code and the count of wrong tries are kept on the SD card, so whoever takes the card out, edits it or swaps it gets past the lock. The help (English and German) and the security model now say so.

- **Only a plugin that declares `uses.registry` switches plugins on and off** (#279). `command.set_plugin_enabled` was accepted from every code plugin; the host now refuses it with `ERR_DENIED` (the whole batch) unless the plugin was admitted with the feature `uses.registry`, as `docs/abi/registry.md` 4.2 says. A plugin file on the card is never given that feature, so a community code plugin cannot switch other plugins off. The Store, *Manage plugins* and *Reading goal* declare it (Store 1.0.14, Manage plugins 1.0.9, Reading goal 1.0.10). Where *Reading goal* is a plugin file on the card (X4, X4 Pro), its choice reads *Open Manage plugins* (*Plugins verwalten öffnen*) and opens it, which has the switch for *Reading statistics*; where it is linked (emulator, Android) it stays *Switch them on*.
- **The switch answers with what went wrong** (#279). Switching off a plugin the device cannot be without (the only one for Home, the reader, the library, the menu, the settings, the questions or the fonts) is `ERR_DENIED`, and a plugin the device does not list is `ERR_NOT_FOUND`; both were accepted and only logged. `core` and a plugin switching itself off stay `ERR_DENIED`.

- **The X4 keeps the time through a restart** (#280). Until now the X4
  showed `--:--` and no date after every restart (an update, a crash, a
  wake from the deep sleep on USB) until Wi-Fi set the clock again, and
  reading statistics waited for it. The X4 now keeps the last time it was
  set in the chip's RTC and goes on from it: Home shows the time at once
  and the reading is dated. The kept time is approximate (the RTC's clock
  is a few per cent off) and Wi-Fi sets it exactly at its next chance.
  After a real power-off (battery out, or the switch-off without USB) the
  RTC is gone too: the time stays unknown (`--:--`) until it is set.
  Time read is counted by the monotonic timer, not by the clock.

- **`check-publish` refuses a plugin with code that turns on the SDK's `kernel` feature** (#282). Directly in its `Cargo.toml` or through a crate it depends on: the message names the path, such as "plugin clock turns on paperpaw-sdk/kernel via fonts-default". The first-party plugins that need it are exempt by an explicit list (`tools/publish/src/kernel.rs`). Plugin interface 7 does not change.

- **The S3 stack scan no longer misses a jump table silently** (#283). A function with a `jx` (a switch) now needs a jump table that one of its own `l32r` literals gives, of any length; a function where the scan cannot tie a table that way fails unless the budget names it with `allow jx <function> <why>` after someone has read it. Build tooling only; interface 7 is unchanged.

- **A new canvas slot draws the stippled separators in one call** (#291, owner Decision 210). Interface 7 has one more host slot, 43 `canvas.fill_pattern` (a checker of pixels in a rectangle, clipped like a fill; `screens-input.md` 6.2), and the SDK a `Canvas::fill_checker`. The faint row and header separators of black and white lists were one host call for each pixel (about 200 for a separator, 836 for the first frame of Settings > Manage plugins); they are one call each now, the same pixels. A host without the slot answers `ERR_UNSUPPORTED` and the lists draw the pixels one by one as before.

- **Kernel: the old in-process registries nothing filled are gone (#293, step S1).** Text, block, input and refresh filters, page layers and frames, lookup providers, voices, reading apps, book fields and badges, action overrides and observers, plugin inputs and gestures, points, screen hints, menu descriptions, GATT services, UI looks and render hooks of the old plugin API had no producer left (interface-7 plugins declare theirs as records). Their `Items` calls, `Contribution` kinds, `Env` parts, kernel state and SDK modules (`layers`, `voices`, `render`) are removed; what reading, refreshing, speaking and opening books do is unchanged.

- **SDK: kernel-side ui kit items nothing calls are gone (#293, step S4a).** Since the old plugin API's callers left (S1–S3), parts of the kernel-only `ui` kit (number editor, address view, offline offer buttons, unused heights and draw helpers) had no caller in any crate or feature; they are removed. Interface 7 and api.txt unchanged.

- **The choice and the yes or no question are the keyboard plugin's (#293, step S5a).** The keyboard plugin, already the dialog plugin of `core.keyboard`, now also draws `core.choice` (a sheet at the bottom) and `core.confirm` (a box in the middle) the way native screens draw, for every plugin's question and for the kernel's own (clean-up, Bluetooth pairing, the reading app, widget lists). The kernel has no dialogs of its own any more: its questions are `core.ui` requests like a plugin's, and their answers come back as the replies they named. Interface 7 and api.txt unchanged. A plugin that alone serves `core.choice` or `core.confirm` cannot be switched off (`RequiredFor` names both slots), and the keyboard plugin is essential: one crash holds it back for one start only, never saved off.

- **The editor of a list of widgets is the sleep plugin's; the kernel's own editors are gone (#293, step S5b).** The sleep screen's widgets are edited on the sleep plugin's screen `sleep.edit-widgets` (slot `core.edit-widgets`), drawn as native screens draw, with the same rows, options and grips. The library folder's editor is the folder chooser's slot itself, and the kernel's unused status items and lines choosers are removed. Interface 7 and api.txt unchanged.

- **Kernel: the built-ins' event handlers, job steps and data-plugin actions are plain records (#293, step S2).** A subscription is an event and a function of the kernel (`Hear`), a job's step a function too (`Stepper`, the kernel's `WorkSpec`), an action's handler a function (`FixedHandler`); a data plugin's action keeps its steps for the kernel to run. No boxed closure is made for any of them: what the core plugin's handlers kept between calls (the catalog's job, the reading recorded) is the kernel's own state. The SDK's `EventHandler`, `ActionHandler`, `work::WorkSpec` and its `Subscription`/`Work` contributions are gone (kernel side only; interface 7 unchanged).

- **The host's own screens are rewritten without the SDK kit (#293, step S5c).** The kernel's stack now holds a plugin's native screen or one of the host's own five: the firmware confirmation, the placeholder (all a safe-mode start shows), the update finishing, the notice where no plugin shows notices, and what holds a screen's place while it is made anew. The kernel draws them itself from its fonts, settings and texts; they look as before, with the same texts, keys and timing. The developer firmware's waveform lab (feature `waveform-lab`) is removed. Interface 7 and api.txt unchanged. The one-time notices at start (the Literata hint, low memory) are host screens on the same box as the firmware confirmation.

- **The kernel's screens are a plugin's native screen or the host's own; the old screen layer is gone (#293, step S6a).** Bars along an edge (`ChromeSpec`), a list and its detail beside each other (the `panes` feature and its action *Other pane*) and the items only interface-6 screens had (revisions, raw buttons, squeezing, taps outside passed through, double taps of their own, a direction of their own) go with it. A guard below a screen it shuts out no longer lets an action through unasked: what another plugin runs then opens below the guard. On Android the voice for reading aloud is chosen from the reader's *More* menu (*Voice*), which opens the system's speech settings. The emulator's tool loses its `ui-kit` gallery. Tests' screens are native screens whose plugin is the test (`paperpaw-core` feature `testing`). Interface 7 and api.txt unchanged.

- **Kernel: status items and lines, item states, setup steps, widgets, decorators and formats hold plain functions (#293, step S3).** What they compute is a function kept in the program (`StatusFn`, `ItemStateFn`, `AppliesFn`, `WidgetFactory`, `DecorateFn`, `FormatOpener` …), no boxed closure each; a data plugin's marks keep their rules as data (`Decorate::Patterns`), a drawn widget its functions (`widgets::Make`). The unused `BookBadge` is gone (kernel side only; interface 7 unchanged).

- The home screen's widgets (the date, the clock, the book card and the recent shelf) measure, wrap and paint themselves once for the bands of a frame, not once for each band: the Back to Home from a book makes about a third fewer host calls on the X4's banded display.

- Back to Home from a book makes fewer service calls: the home screen no longer asks the screen stack when it was just shown, and its status line keeps what it asked the registry and the settings until a setting or the registry changes (the first frame of a return made nine calls with a full shelf, five now).

- The first Back to the home screen from a book opened at the start is quicker: the page, each of its widgets and its status line read the interface's look settings once between them (nine times before), the library is listed once for the measure and the frame, and the name of Settings comes from the catalog, not from a page of service calls.

- Back from a screen with a large picture (a cover page) to the home screen is no longer a full refresh (1.7 s on the X4, a flash): it is refreshed fast as any frame, and the grays pass after it cleans what ghosting is left (Decision 212). Turning pages on the same screen still refreshes in full after a large picture.
- The home screen's card and shelf keep the lines they composed (measured and wrapped) across returns, for as long as the books, their progress, their covers and the look are the same: the return from a drawer makes about a third fewer host calls (195 to 134 in the emulator), and the first return after a book with an unchanged card draws its lines again without measuring them. The wifi plugin follows the radio's change events and no longer reads the radio at each change of the screen stack while it is off.
- The home screen shown again with a cover ready draws the covers' places first and the covers in the frame right after it (Decision 213): the first frame spends no time on decoding them (about 270 ms on the X4, not measured), the covers follow about 0.5 s later. `COVERS_IN_FOLLOWUP` in Home's `native_plugin.rs` is the one-line switch.

- **The S3 band's `allow indirect-in` ceilings carry a headroom** (#296). The line holds the count of the last CI scan; the scan adds 16 for `paperpaw_plugin_*` and 8 for any other crate, and prints the exact count per crate, so branches counted apart no longer fail the train by a few calls.

- **SDK: the old plugin API's helpers no caller reaches are gone (#300, Decision 209, step S6b-1).** Functions of the old in-process API that take `Env` or `Cx` or make `Command`s and that neither the kernel, a shell, a tool nor a plugin calls any more are removed with what only they used (about 5,700 lines: the old widget grid view, the sorted-library view, list, slider, settings-row and field drawing helpers, Bluetooth, reader and status command builders, the `hears!` macro, `FixedSubscription`). Eight of the nine public lines naming `Env` leave `api.txt` (`Decoration::of_role`, `files::shown_size`, `numbers::decimal_mark`, `UnixTime::now`, `web::user_agent_of`, `What::title`, `Geometry::size_of`); the interface-7 fingerprint is unchanged (no layout changed). The old-API library benchmark `tools/library-bench` goes with the view it measured. What the kernel draws and does is unchanged.

- **SDK: the old plugin API's kit no caller reaches is gone (#300, Decision 209, step S6b-2a).** A second sweep removes what the kernel's own code no longer reaches of the old in-process kit, also where it took no `Env` (widgets such as the date and time pickers, sliders, text fields, chips, sections and reorder rows, the library-service indirection and the old library views), with the tests that only tested it (about 22,000 lines). No plugin could stand in for the kernel's library any more, so the namespace exception for it is gone. What plugins see of the SDK (`api.txt`) and the interface-7 fingerprint are unchanged; what the kernel draws and does is unchanged.

- **Kernel: what the kernel's own code asks for is its own `Ask`, and what registers is staged directly (#300, Decision 209, step S7, the last of the rewrite).** The SDK's `Command`, `Commands` and the old screen `Output` move into the kernel as `paperpaw_core::{Ask, Asks, Output}`; native plugins keep sending their interface-7 command records, which the kernel applies as before. SDK helpers that made a `Command` return what it carried (`bluetooth::*` and its builders a `BtRequest`, `developer::open` a `DevRequest`, `Sound::answer`, `Speech::answer` and `library::changed_book` the reply event and its params; `Event::emit` is gone), `presets::commands` becomes `presets::writes`, and `Editor::open` is removed (its two `api.txt` lines with it; no plugin could call it). The kernel's registrar `Items` and its list of `Contribution`s are gone: a registering function is given the plugin's `Staging` (`paperpaw_core::Staging`, same calls), and each call stages straight into the list of its kind, checked as it is added (namespace, once per kind, key maps and translations parsed), merged whole or not at all. The interface-7 fingerprint is unchanged.

- **Kernel: the old kit the kernel still used now lives in the kernel (#300, Decision 209, step S6b-2c).** Actions, dialogs, decorations, help, install records, the library catalog, logo, menus, readers, setup steps, status items, time, the UI helpers and widgets that take the kernel's state are in `paperpaw_core::kit` and read it through `Place`; the kernel no longer makes an SDK `Env` anywhere. Names plugins see stay the SDK's (the kernel re-exports them). The SDK's own copies go in the next step. The SDK's kernel-only `presets::widget_look_settings`/`widget_look_changes` take a predicate for widget ids in place of the old `Widgets` registry. What the kernel draws and does is unchanged.

- **Kernel: handlers and screens run with the kernel's own `Place` and `Turn` (#300, Decision 209, step S6b-2b).** The kernel's handlers, native and host screens, dispatch, the test fakes and the shells' and tools' kernel users read the kernel's shared state through `paperpaw_core::Place` (the time of the event and the part of the screen they have) and queue what they ask for through `paperpaw_core::Turn`, in place of the SDK's old `Env` and `Cx`. The old kit helpers still in the SDK get the old `Env` made from the `Place` where the kernel calls them; they move into the kernel in the next step. Unrelated local types named `Env`/`Cx` in the kernel are renamed (`Within`, `Facts`, `Subscribed`, `OwnedHost`). What the kernel draws and does is unchanged.

- **SDK: the old `Env`, `EnvParts`, `Cx`, `ScreenMaker`, `ScreenList` and the kit that used them are gone (#300, Decision 209, step S6b-2d).** The kernel runs on its own kit (`paperpaw_core::kit`, `Place`, `Turn`) since S6b-2c, so the SDK's copies of actions, dialogs, decorations, help, install records, the library catalog, logo, menus, readers, setup steps, status items, time, the UI helpers and widgets that took an `Env` are removed (about 18,000 lines), with the SDK reading helpers that could only be made from an `Env` (`PageType::new`, `ReadingFaces::new`, `draw_image`). Plugins see no change but two lines of `api.txt`: `widgets::grid::Geometry::new`, which took an `Env` no plugin can name, and the SDK's `role_settings!`, which called a decoration helper that moved to the kernel (the kernel has its own `paperpaw_core::role_settings!`). The kernel's old `Widget` trait (widgets made by a function of their own) is gone too; the dev tool's probe widgets are drawn widgets. The tests of the moved kit run in the kernel on a kernel with nothing registered (`KernelBuilder::build_bare`, `Kernel::with_place` with the `testing` feature). Unrelated local types named `Env` are renamed (`paperpaw-serve`: `TlsContext`; `paperpaw-sign`: `ScanScope`).

- **The update page shows a progress bar while it downloads or installs an update** (#302). Under the title ("Installing the update") a bar fills, with the share in words ("25 %") below it, in English and German. The bar moves in steps of five percent and only its own strip of the screen is redrawn, with a partial refresh: the page no longer flashes every few percent, and these redraws no longer count as page turns towards the full refresh.

- The emulator's X4, X4 Pro, X4 Classic and X3 profiles no longer answer
  `text.ink`, as the firmware does not, so they draw what the device
  draws (their screens in the snapshots moved with it).

- **Neighbouring changed card blocks are written in one command** (#304). The block cache writes changed blocks that were changed one after the other and lie one after the other on the card (up to 8) in a single write command; the order that keeps the card safe after a power cut is kept.

- **A key ends the gray pass, and every screen after grays is drawn while the panel is cleaned** (#304). A key pressed while a picture screen is drawn in grays no longer waits for the 1.3 s of drawing and the 1.1 s refresh: the pass stops, and the next screen is drawn whole. After grays the next screen (any, not only Home) is drawn while the panel is cleaned to white, and shown with a fast refresh.

- **A smaller firmware: identical functions folded and TLS trimmed** (#304). The X4's linker folds functions with identical code. mbedTLS drops TLS 1.3, SHA-1, the CBC suites, RSA key exchange and the P-521 curve: HTTPS keeps TLS 1.2 with ECDHE and AES-GCM or ChaCha20-Poly1305 (X25519, P-256, P-384), which every server the firmware knows negotiates; a server that offers only TLS 1.3 or only CBC suites no longer connects.

- **The boot reads the flash at 80 MHz on the X4, the X4 Pro and the X4 Classic** (#304). The X4's flash chip (Puya PY25Q128HA, rated 104 MHz for DIO reads) is now clocked at 80 MHz by the boot loader instead of 40 MHz, which shortens the start; the X4 Pro and the X4 Classic verify and boot at 80 MHz too (the app loads 430 to 460 ms earlier). The X3 keeps 40 MHz until its flash chip is known.

- **More free heap and fewer big one-piece blocks on the X4** (#304). The Wi-Fi send and receive code (16.7 KB) runs from flash instead of RAM, and the heap takes the RAM (16 KB). The small heap region is filled first at start-up, so the large one stays whole for big temporaries. An HTTP answer of known length is taken at that length, not at 32 KiB; a small PNG inflates into a window of its own size; a progressive JPEG probes its headroom in 8 KiB pieces, not one 48 KiB block; and a TLS session is opened only where a 20 KiB block is free.

- **Updates can be downloaded packed: a third smaller** (#304). Each release now also offers `-update-packed.bin`: the same firmware deflated (about 3.8 MB instead of 6.0 MB for the X4). The device inflates it while it writes it into the other slot; the signature (Ed25519) covers the packed file and so fixes the image it unpacks to, and a damaged file fails as any other does. Beta 3 and older do not read the format and refuse the file; the plain `-update.bin` stays for them. `paperpaw-sign pack-update` makes the file, `paperpaw-sign show` reads both.

- **Built-in texts no longer cost a card read for every installed store plugin** (#305). The text catalogs of store plugins on the card are asked before the built-in texts, and such a catalog could only say "not mine" by reading a block from the card: after every change of screen, each built-in text cost one card read per installed store plugin (5 ms a text with three of them). A compiled catalog now says the first and the last of its keys, and a text outside them is not looked for on the card. The catalogs in the card's cache are compiled anew once.

- **Asking whether a folder is there no longer reads the whole folder** (#305). Several checks listed a folder from its start to its end only to learn that it exists; on a card that is one read of the whole folder, longer the more books it holds. They now stop at the first entry.

- **The card's free space is no longer counted again at every start** (#305). After each start the reader read the card's whole allocation table to learn how much space is free: 6.9 MB and several seconds of card activity on a 16 GB card. The count is now kept on the card (FAT32) once it is known and taken from there at the next start. It is taken back on the card before the table is next changed, so after a power cut, on a card marked as not closed properly, or after a computer changed the card, the table is counted as before. Cards written by PaperPaw are no longer left marked as not closed properly.

- **Finding the page number reads the card far less** (#305). The reader found a position's page by reading the page list 8 bytes at a time, about a dozen small reads for every lookup and several lookups a page turn. It now reads the entries near the page at once and answers the following page turns from them.

- **Saving the reading place writes one block instead of a whole file** (#305). Each page turn that saved the place wrote the place file anew through a temporary file and two renames, 13 to 25 block writes on the card, after searching the library list for the book. The file is now two slots of one block each: a save writes the slot that does not hold the newest place, where it is, and a book that is on the card is no longer looked up in the library list first. After a power cut the card holds the place saved before or the new one. Place files written by earlier versions, or by a reader itself, are read as before and get their slots with the next save; a place with a long XPointer is still written as a whole file.
- **Files can be written in place on the card** (#305). `Storage::write_at` now works on the memory card for bytes within a file or at its end; it said "unsupported" there before.

- **Less card work at every start** (#305). A plugin's folder is listed once instead of being asked for some 16 file names one by one; a plugin's text catalogs are checked in pieces of 4 KB instead of one block at a time; the note of the room for code plugins is written only when it changed; and the check for files under PaperPaw's old name is one listing of the card's top folder instead of four lookups, twice.

- **Waking from a light sleep no longer walks the library again** (#305). After every wake the library was walked anew, in case books were copied onto the card while the device slept. A card that was taken out loses its power and is noticed as a swapped card; one that still answers is as the sleep left it, so the X4 now walks the library again only after a start or a swapped card. A walk that was under way goes on where it was.

- **The linked plugins are hosted as one batch at start-up** (#306). Each plugin's admission used to rebuild the key map (which reads the user's configuration again), the font choices and the fonts in use, the slot list and the widgets' font settings, so the 28 plugins repeated them up to 28 times (1,841 ms in all on the X4). They are now done once, after the last plugin; the plugin lists are sized once before the first, and the `[plugins]` section of the configuration is read once for all the on/off decisions, not twice for each plugin. The start log names where the time of hosting goes (admit, key map, fonts, widget fonts).

- **Text is set faster** (#306). The font of a role (menus, book text, status)
  is looked up once and remembered until the fonts change, and a text
  draw no longer looks its font up twice. Every text measure and draw
  is cheaper, so pages and Home draw sooner.

- **The kernel's Bluetooth model is built only for devices that have Bluetooth** (#306). The `bluetooth` feature of `paperpaw-core` (on for the X4 Pro's `bluetooth` build, the desktop, Android and web shells) carries the radio model; the X4 and the X4 Classic, which have no Bluetooth, build without it and are 17–22 KB smaller. Plugins' Bluetooth types are unchanged; on such a device a Bluetooth command is refused as "off", as before.

- **The kernel's and the X4's own texts are carried without their keys** (#306). The kernel's catalogs (menus, hints, settings, host prompts) and the X4 shell's are built into the program the way the linked plugins' are: a hash of each key and the messages, English and German in full, instead of the `.ini` sources with every key spelled out. About 30 KB less flash on the X4 family; what the reader sees is unchanged. `KernelBuilder::built_messages` takes the tables; a program built without `paperpaw-app` (the kernel's own tests, the `testing` feature) still carries the sources.

- **Library lists open faster** (#306). A list of the library no longer
  looks at each book's details file again on every call: whether a book is
  indexed is remembered until the library changes.

- **The library answers faster** (#306). A library call reads the log of
  status changes once instead of once per scan and per row, and the
  number of books is counted once for a state of the catalog instead of
  on every call, so Home and the Library open sooner.

- **The release checklist says who gets the packed update file** (#306). `RELEASING.md` now says the update server offers the packed update URL only to devices that report Beta 4 or later, and the plain update file to every other.

- **Reading a setting is cheaper** (#306). Where a setting is kept is
  looked up once and remembered, and the secret check no longer looks it
  up a second time, so the many setting reads of a screen cost much less.

- **Four small memory items at start and on gray frames** (#306). The gray frame is drawn in the memory of the band frame and the band is made again in it, so a gray pass allocates nothing and leaves no hole; `paperpaw.ini` is kept as read, not as a second copy beside it; the kernel's lists give back their reserve through the helper that never aborts; the hyphenation index of a table on the card is kept in 16-bit steps (about 40 % less: 3.5 KB for German).

- **The firmware is about 90 KB smaller (X4 Classic 93 KB, X4 86 KB): records are encoded and decoded from tables** (#307). Every record a plugin and PaperPaw exchange (service calls, events, declarations, the stages of the format, widget, status, lookup, input filter and book badge plugins) had its own generated decoder, sizer and writer, in PaperPaw and again in the plugin kit. Now a record is a table of its fields, and one decoder, one sizer and one writer walk it. The bytes on the wire are the same, byte for byte, so plugins built before keep working unchanged, and the plugin interface did not change. A call costs no more than before: decoding got faster (text that is plain ASCII, as ids and paths are, is checked a word at a time), encoding a little slower, together slightly less work per record. A plugin's own stage schemas (`tools/stage-codec.py`) keep their generated code: with a handful of records it is smaller than a table walker.

- **Pictures turn gray only after 30 s of rest** (#308, Decision 216). A
  screen with pictures (Home with its covers) is shown in black and white
  first; the grays now follow once there has been no key or touch for 30 s, not
  3 s. The gray pass ends in a 1.7 s clean to white on the X4, which the next
  move paid; moving around on Home no longer meets it. Every key press begins
  the 30 s again, and a pass that has started is still stopped by a key.

- **The S3 band's per-function `allow indirect` counts are ceilings** (#309). A line allows its count plus 2 calls through a register (LLVM's layout moves a site or two between builds), fewer calls pass, and a stale `allow indirect` or `allow call` line is a warning that the scan prints, no longer an error. Frame sizes, new unlisted sites and every other `allow` line stay exact.
- **The frame-lifetime probe derives the plain types it needs** from the module's own `paperpaw_sdk::` / `paperpaw_native::` paths, so a new memo or cache type needs no line in `tools/tests/test_abi_frame_lifetimes.py`.

- The substitutions of a script pack skip marks where the font's lookups say so (OpenType's lookup flags: ignoring marks, mark filtering sets, mark attachment classes), as sets listed at the table's end; packs made before read as they did.

- The S3 band scan (`paperpaw-sign frames-s3`) takes a site that LLVM's inlining moved (a call through a register in a function with no line of its own, or past its line, or a call out of the image of a caller no `allow call` line names) from the spare of the same crate's lines: an `allow indirect` line's count less what its functions have now, or an unused `allow call` line of the same callee. It says so in a warning; the sum per crate never exceeds the reviewed counts, and what the spare does not cover fails as before. A stale `allow call` or `allow indirect` line is kept as that spare, no longer advised for removal. `tools/check-s3-budget.sh` runs the same three checks as CI's job (the scan, the scan with the linked plugins, the start-stack check).

- The `filter.verdict` record of `core.input-filter` gained two appended fields, so `FilterVerdict::new` takes `key` and `value` (empty for the older verdicts).

- All UI language packs say "Not signed: runs without a check." on the row of a copied code plugin in their own language, short enough for the X4 row (#321).

- The plugin store's lists (`store.view`) no longer read the card while they draw: the rows of the page shown (about 1.5 KiB) are made when the list opens or its page, order or data changes. The store now also says how it works (Plugin manager, How it works), in English and German.

- The plugin store's list (`store.view`) is about twice as fast to draw: the one-line note of each row is cut to fit once, when the page of rows is made, and no longer again in every band of every frame (a frame estimated at 1.9 s on an X4 now about 0.9 s). Lists with notes (`WindowList`) no longer measure a cut note a second time.

- **A focus move on Home no longer reads the covers from the card** (#329).
  Every frame that drew a cover's rows again read its whole decoded file
  (57 KB), scaled and dithered it: some 100 ms a cover on the X4, at every
  move of the focus, three covers on a Home with a book on the card and two
  on the shelf. Now what a draw put into a black and white frame is kept
  while Home is shown, a bit a pixel (3 KB a cover, 12 KB at most, small
  pictures only), and the same draw again takes its rows from there: the same
  pixels, no card. The room is taken between frames, and a screen opened over
  Home gives it back before it takes its own.

- The Plugin store's setup pages (*Choose a setup*, *Add more*) and every list with notes draw faster: a note that fits one line is measured once, not word by word, in every band of a frame (frames estimated at about 0.99 s and 0.70 s on an X4 are now about 0.85 s and 0.57 s). The dev tool's `perf` no longer reports a request the store list leaves under way when it closes as a leak (the +628 B per opening was that request's state on the emulator's heap, ended later).

- The overview of every tool (*All tools*, `home.drawer`) draws about thirty times faster: the tools' names, icons and states are read once when it opens and their lines fitted once, no longer again in every band of every frame (each band read Home's settings and plugin list again and asked the hyphenation of every word, which reads the language's table from the card on an X4; a frame estimated at 8.7 s on an X4 now about 0.3 s).

- Lists draw faster on the X4 in every plugin: a frame is drawn in bands, and each band used to read the look from the settings, wrap the note under the list and lay out the header again. The first band of a frame now does it once and the other bands of that frame use it (the work of a list's draw is down by about a sixth in the dev tool's measurement). Nothing is kept between frames, and the lists carry less in each plugin's frame than before.

- Hyphenation keeps the answers of the last 64 words asked of each open language table (1 KiB a table, dropped with it): a word asked again, such as a name fitted in every band of a frame or a list drawn again, no longer reads the table from the card. 20 words asked 50 times read the card 363 times instead of 8,163.

- **Pictures are dithered and drawn about three times faster** (#338). The
  error diffusion of a picture (the Atkinson dither of covers, the sleep
  screen's cat, reader pictures) and the storing of its gray rows in the frame
  took some 120 instructions a pixel each; they now take about 40 each: no
  bounds checks or saturation in the diffusion, and the frame packs whole bytes
  of pixels instead of reading and writing each one. The pictures are
  unchanged, pixel for pixel.

- Dependencies: one semver-compatible `cargo update` of every lock file (no esp-*/embassy crate moved, interface 7 unchanged) (#340).

- Statistics, battery and notes-overview, where linked (the emulator's store set), keep their settings, setting groups, ids and titles in tables built into the program instead of copies on the heap: about 6 KiB less heap with all three switched on (statistics 5.4 KiB, battery 0.4 KiB, notes-overview 0.2 KiB). No change on a device that loads them as plugin files.

- **More memory while reading a book opened from the library** (#344). The kernel now tells a native screen when a full screen covers it (`event.screen_hidden`), and the library list gives its covers back while a book is open over it. It asks for them again when it shows, as it did before.

- **`paperpaw-dev reach` says what it knows about each screen** (#351). A screen the search for the ways the buttons lead does not find was a warning "could not be reached", mostly wrongly: the search had ended at its bounds. Now it is an error (`reach.unreachable`) only where the search was complete, and "not decided" (`reach.undecided`, a warning that says where the search ended) otherwise; every screen is looked for, also those that need parameters. Screens the device shows by itself or that the checks' device cannot come by (the kernel's notice, the low-memory notice, the page while an update is finished, the cable's screen, the first-run setup's pages, the Bluetooth search) are declared with their reasons and noted (`reach.exempt`). Where the first search leaves screens, a second one goes on: a walk through the places of the app (a screen with its parts and where the focus is) that keeps where each press led, searches each place once and keeps the settings as at the start; it reaches the reader and five more screens of PaperPaw's own app. `sweep` uses the same search.

- The "How it works" texts of the plugins built into the firmware are kept compressed and unpacked one at a time when they are shown: the firmware carries about 13 KB less for them, and what you read is the same (#352).

- **`paperpaw-dev reach` decides every screen of PaperPaw's own app** (#358). The search for the ways the buttons lead cannot go through an app of this size to its end, so 17 screens stayed "not decided" (`reach.undecided`). Now a screen the search does not come by has a declared way in (`tools/dev/src/reach/ways.rs`): the steps a reader goes there with from the home screen and what they need, such as a text typed into a search, a book with a footnote, a theme installed, a word chosen on a page. The tool replays each way on a device of its own on every run, checks closing and the focus there as on any screen it reached (`reach.declared`), and fails where the steps no longer end on the screen (`reach.way-lost`). Eleven screens are decided so: the reader's contents, notes, skim screen, search results and footnote, the dictionary's lookup, the settings' search and theme chooser, the library's book information and shelves, and the store's letters. Six are declared with the reason the checks' device cannot come by them (no Bluetooth radio, no firmware to update, a fetch that is over at once, a list of fonts that does not load there, #366) or that nothing leads to them yet (the to-do list, #364; the drawer of all tools with buttons alone, #365). With every screen decided, `reach.undecided` is an error where the whole app is looked at, and CI's check of the screens fails on it; for a plugin of others it stays a warning.

- `paperpaw-dev build` makes the same plugin file from the same source in any folder and on any computer, so the plugin store can rebuild a community plugin of plugin interface 7 and compare: the paths in a build are written as `/plugin` (the plugin's folder) and `/cargo` (Cargo's home), for the readers and for the emulator's library. `paperpaw-dev new --code --sdk <checkout>` names the SDK through a link in the plugin's folder (`.paperpaw`, ignored by git and left out of the workspace) instead of by its path on the computer, which Cargo made a part of the build.
- `paperpaw-dev check <file>.ppn` reads a plugin file of plugin interface 7 (file format 5) as a reader of its chip does and shows what it is made for (the chip, `7.<minor>`), its sizes and who signed it. It said "the device refuses it: a plugin file of a newer format" for every such file, also in the store's check of a submission.

- File sync: the help text (en + de) now explains how to sync with a Syncthing folder through a WebDAV server on the PC or NAS (rclone serve webdav) in four steps; the reader itself has no Syncthing (#380).

- Devanagari and Bengali books: a nukta written as its own character moves with its letter, so more words are set as their fonts draw them (#385).

- Simple home (#386): the quick panel opens and redraws much faster (estimated well under half the time on an X4): its switches and bars are read and laid out once when it opens, not for every frame, and the icons are looked up once. The names of the books in the library start at the same margin as the lists elsewhere. Shorter descriptions for the date and battery settings.

- Secure connections (X4, X4 Classic): the firmware has about twenty root certificates built in (PaperPaw's own services, GitHub, the services of the plugins and the largest hosts) instead of Mozilla's whole list, which makes the firmware about 45 KB smaller (#387). The full list is the new store plugin *All certificates* (off at first, signed by PaperPaw, read from the card only when a server's root is not built in). A server under another root fails with "Certificate not known. Install 'All certificates' from the store."

- **Smaller firmware image** (#388). The text wrapper of the screens was compiled
  once for every place that calls it (24 copies); it is one body now. The X4's
  image is 34.9 KB smaller, the X4 Classic's 38.2 KB. Nothing changes on screen.

- **Start, opening a book and Back to Home write less to the card** (#389).
  A small file that already holds what is written is left alone; on the X4 a
  rewrite is 17 to 36 card commands of 5.6 ms (the book list, the page list and
  the reading widget's file were rewritten as they were). The start log now says
  where the time to the first frame goes.

- **Text draws faster** (#393). A word's pixels are now handed to the screen a row at a
  time and gathered eight to a byte, not one by one. A text draw also
  checks its font once, not twice. On the sample page a word takes about
  25 % fewer instructions to draw.
  The look of the text does not change.

- **Less memory held at Home after a start** (#393). The plugins that may sleep and were woken only by the start's work (a format reading the library's book details or covers, the file browser told of a setting) sleep again once that work is over, instead of keeping their state until the device goes to standby.
- **Linked plugins' settings matched faster at the start** (#393). Matching a declared setting with the plugin's built-in table now compares the setting's id first, before any list of choices.

- **Fewer host calls for each page turn** (#393). The reader now takes a
  reading font from the device only when a screen first draws with it,
  not all four at each band of the page. A page turn makes about 45 fewer
  calls (475 instead of 517 on the sample page).

- **The X4 starts about a second sooner** (#393). The starting picture's 1.7 s
  refresh is no longer waited for before the plugins are hosted and the
  kernel starts: that work runs while the panel is busy, and only the first
  Home frame waits for it.

- **Home comes up sooner after a start** (#393). Drawing Home asked the plugin registry for its widgets and tool menus, and every row of a menu or a widget list read all the plugins' declarations again; it now reads them once for each question. Home's recent books ask the library only for the books opened before, not for a page of 32 books with a card read each.

- Internal: the line breaker for Thai, Lao, Khmer and Burmese and the Burmese character classes look their letters up in tables instead of long `match`es (same results, less code the frame budget has to follow; #396).

- Script packs keep a substitution that draws each of many letters with the same glyphs around it (Noto Serif Myanmar puts a spacing glyph before 300 letters, 13 times) as one short entry instead of one per letter: Burmese's full table shrinks from 127 KiB to 31 KiB (#398). Packs made before read as before. The Burmese pack itself still waits: see #397.

- **Page turns measure and hyphenate far less** (#399). The reader forgot the kerning of about a third of the letter pairs it had learned and a good half of the words it had asked the hyphenation table about from one page to the next, and asked the font and the card again at every turn. The table of kerned pairs now holds 256 entries in sets of two (1,024 bytes, as before) and the break places of 256 words in sets of two (2,056 bytes, 8 fewer than before). On the emulator a turn of a Latin book now averages about 45 text measures (65 before) and 7 hyphenation lookups (12 before).

- **A page turn no longer lays the next page out while the key is handled** (#399). The reader lays out the page after the one on show in the quiet time after a turn (300 ms later, while the screen refreshes) and keeps it for the next forward turn, which then only takes it. The measures, hyphenation lookups and card reads of the layout leave the key's own handling: on the emulator the key alone makes about 15 text measures and no hyphenation lookups on average (about 45 and 7 with the layout in it). The kept page takes about 6 KiB of heap (300 bytes a line) while the reader is shown, and is let go of when another screen covers it.

- **The reader keeps 1.5 KB less memory for the words of the page on show** (#400): the table of word boxes is made for the page's words at once instead of by doubling (a page of 45 words held room for 64).

- Reading in scripts written without spaces takes less memory: a paragraph keeps room for the places it may break at, not one for every letter (24 KiB for a long Burmese, Khmer or Thai paragraph, now 4 KiB) (#401). A page's list of lines and each line's list of faces are as long as they are. On the X4 a Burmese book now peaks at 211 KiB (249 KiB with its whole table before), Khmer at 199 KiB (221), Hindi at 208 KiB (215).
- Script packs keep only the forms text of their letters can come to: Burmese's substitutions take 20.3 KiB (31.4), Devanagari's 14.8 KiB (16.6), Sinhala's 2.9 KiB (5.1); text is shaped as before (#401). Packs made before read as before.

- **Fewer card reads per page** (#403). The EPUB, HTML and FB2 formats read up to 2 KB of a
  chapter's converted text at once and serve the blocks after it from memory. Before,
  each block was read from the card by itself. On the X4 every read goes through so much
  code that the flash cache misses most of it, about 1.7 ms each. Alice in Wonderland is
  now read through with 246 card reads instead of one per block (866). The kernel also
  looks up what it has already read in one step instead of comparing long paths.

- **Faster start into the book read last** (#403). The first time a book is opened, the
  reader checks whether its first 80 blocks need letters the reading font lacks. It now
  keeps the result in the book's folder, so later openings and every start skip the
  check. On the X4 each of those block reads took about 10 ms before the first page.
  The list of page counts is no longer rewritten when the book is already its newest
  entry. On the X4 profile of the emulator, a start into the last book now asks for
  3 blocks before the first frame instead of 56. It reads the card 82 times instead of
  154 and writes nothing to it.

- **Faster start to the home screen** (#403). Each built-in plugin's declarations are now
  checked once when the plugin is taken in, not again at every later step of that
  same step. A menu row finds its icon and its submenu with one lookup instead of
  reading every icon and every menu row. On the X4 profile of the emulator, taking in
  the built-in plugins takes about 19 % fewer instructions and the first home frame
  about 26 % fewer. The memory used stays the same.
- The X4's start log splits the kernel's start into its parts (before the start event,
  the start event and its commands, settling).

- **Hyphenation of German, Norwegian and Dutch reads the card about half as often** (#404). The three large pattern tables (`de`, `nb`, `nl`) kept nothing to tell that a word's one- and two-letter keys are no pattern, so every start of a word read a block from the card (25 to 30 reads a word). They now keep their patterns of one or two letters (up to 640, 4,096 bytes in all) and answer such keys from memory: 14, 13 and 11 reads a word (30, 28 and 25 before), with the same break places. The heap of an open table rises by 3.1 to 3.3 KB (`de` 16,376 to 19,456 bytes, `nb` 12,535 to 15,735, `nl` 7,306 to 10,574; their cache is 4 blocks, not 8); the other ten tables are unchanged. Opening a table is one pass over its patterns instead of two. Old hyphenation packs load unchanged.

- **Native cable transfer is switched on** (#900). The X4 and the emulator wire `core.net` with the cable, so a computer that speaks over the USB cable reaches the transfer plugin (interface 7); before, the plugin answered nothing and the cable was refused with 503.

- **An update that does not fit the device's app slot is no longer downloaded** (Decision 179). On a device with the small CrossPoint layout (slots of 6.25 MB) the update page compares the offer's size with the slot before it starts the download and says so, instead of loading about 7 MB and refusing it at the install. The notice (English and German) says the update does not fit this device's storage layout and to install PaperPaw once with the flasher at paperpaw.org/flash; updates work on the device after that. The device's own refusal before the first write is unchanged.

- **The S3 band names four registry lookups of the kernel env change** (#300, #3479): `resolve_slot`, `Screens::find`, `find_native` and `find_fixed` each have one call through a register, and the frame-lifetime probe declares the kernel's `Place<'a>`.

- **A release's version is set by one command** (#3528). `python3 tools/bump-version.py 0.6.0-beta.N` rewrites the workspace version, the internal requirements, the docs and store texts that name the release, the SDK's user agent and the Android version name and code; the lockfiles are refreshed with `cargo metadata`. The Release workflow's comments name the 81 assets of a firmware release.
- **The store byte check no longer fails against a release before interface 7** (#3528). `versions.py --release --bytes` built the baseline release with the interface-7 script that release does not have; it now says the files cannot be compared (another firmware) and leaves it to the version rules.
- **`paperpaw-core` and three plugins no longer wait for each other when the crates are published** (#3528). Core's tests use `battery`, `widgets` and `fonts-default`, which depend on core: the dev-dependencies are by path alone now, so `cargo publish --workspace` finds an order (the dry run stopped at `paperpaw-plugin-battery`).
- **`paperpaw-x4` can be published** (#3528). Its tests use `paperpaw-default-packs`, a crate that is not published; the workspace names it by path alone now (as `paperpaw-fat`), so the package no longer asks the registry for it.
- **`paperpaw-plugin-stats` can be packaged** (#3528). It read a source file of Reading goal's folder, which a crates.io package does not hold (`cargo publish` verify failed: "couldn't find file `src/../../reading-goal/src/read_today.rs`"); it has its own copy now, and a test keeps the two equal.

- **USB developer tools row: a shorter description** (#12), so it fits its two lines in English and German.
- **The store plugin fits the S3's frame limit again** (#214): the page it installs from and the dialog buffers live on the heap.
- **Translations past the read limit are read in a small window**, not by a 2 KiB block, so the peak of a book in Thai or Hindi stays where it was.

- Every first-party plugin shows a "How it works" text in the plugin manager, in English and German: it was missing for the plugins whose text was not wired as a translation (Statistics, Battery, Notes overview and the others), and Wi-Fi has one now (#3837).

- **Emulator memory tests count the reader's covered page** (#4003). Since the reader draws its page when a question or the store is over it (#359), the status-line frame test allows 13 KiB above live (12.7 measured) and the store tests 87 % of the heap (the store's emulator peak is 170 KiB, was 162.5).

### Fixed

- `paperpaw-sqlite` never copies over a database: a storage that cannot move files is refused
  (the old file kept); it copies through one 4 KiB page and syncs before the move (#228).
- *Discover fonts* in the reading settings, where *More fonts* does not run, says why and offers the one thing to
  do (it is not installed: open its page; it is switched off: switch it on; it waits for a restart: restart),
  instead of only opening the store's page of the plugin. A plugin's page opened without a connection says
  "No connection", not "Plugin not found", and carries the plugin's name (#240).

- Mail's unread-count Home widget drew nothing.
- The desktop emulator and its card folder: listing a file, or going below one, answers as on the card (the wrong kind of path), so the library takes an imported file there.

- Widgets of plugins (weather, Pomodoro and the others whose widgets the plugin draws itself) lower on a page than the first band of the screen were not drawn: the kit's band test took the widget's own top off the rows it compares. Their band is now the clip as the host gives it (#107).

- A large clock in Persian or Urdu digits is drawn at one size: where the interface's font is scalable (Android, the emulator) and the digits come from the Arabic letters pack, which has no such large size, the colon is no longer drawn far larger than the digits beside it; the whole time is set at the pack's largest size (#131). Numbers in Latin digits and all text of letters are drawn as before.

- **Calendar** (#133): a calendar file of the card is read with the radio off (only a run that reaches over the network asks for it); the sleep screen keeps a margin from the screen's edge. **Manage plugins** reads up to 64 pages of the registry, so the emulator, which links about eighty plugins, no longer drops its last ones (Calendar, Tasks, Smart home …) from the list.

- **A memory card with a loose contact no longer keeps the device awake and reading the library over and over** (#151). A card that only failed for a moment and never left keeps its mount, with everything not yet written to it. A card that lost its power and is back is mounted anew, and where it is the same card as it was left (its serial number and its FS information sector, FAT32) the library is not read again and the time to sleep is not started anew; the same card found again and again is mounted anew less and less often (every 3, 10, 30, then 60 seconds). Another card, or a card a computer has written to, is read anew at once, as before.

- **A memory card can be swapped as often as you like while PaperPaw runs** (#151). Each new mount of a card taken out and put back kept about 13 KB of the old one's memory, so PaperPaw mounted a swapped card only once per start and asked for a restart after a second swap. The card's storage now owns its file system and gives everything of the old mount back (its open files, folders and caches), so every swap, also one while the device sleeps, is mounted anew with the memory of the first mount. What PaperPaw remembered of the old card's books is forgotten as well.

- Plugin store: a plugin's page now shows what the plugin does, not only its short line. The page reads the library's description (it was sent under another name than the store looked for, so it never showed), shows its first paragraph under the short line, and *How it works* opens the whole text. Store 1.0.15, no interface change. (#212)

- **The native About page opens on a host without firmware facts** (#243, #228). Where the host has no backend for the device's firmware, built-in plugins or native facts (the desktop shell answers *unsupported*), the page now opens without those rows instead of failing its first callback; the lines other plugins add are likewise optional.

- What plugins keep for a book (`book_data.spec` folders and the kernel's own: `library.md` 1 and 4) follows a library move or removal by bounded, repeatable host steps instead of the SDK's unchecked calls, also in the `core` plugin's `book_moved`/`book_removed` handlers (interface 6, behaviour changes): entries are found a batch of 64 at a time (a book with per-page files moves whole) and renamed with fallible allocation (no memory is a status, not an abort); a move never replaces what the new key already has, files or folders; an entry the card refuses for good (damaged name, tree too deep) stays for the clean-up of books no longer found and the others still move, while a transient card error is the step's status and is repeated; a removal is bounded pieces (`remove_tree_step`, at most 64 pieces in an event handler, the rest left for the clean-up) instead of one unbounded `remove_dir_all`. A cut at any card action is finished by repeating the step.

- The host's cache of a document (`.paperpaw/cache/<book key>/`) is removed with the old key of a book that is moved, replaced or deleted (its folder is named by the book key, so it is never renamed; the new key's is made at the next open): its stamps first, so a removal that is cut or ends early leaves no cache that looks whole, then the tree in bounded pieces. One budget of 8 pieces covers a whole step of a library move; a cache too big for it holds the move's phase until it is gone. A card status that is for good (a name the card cannot make) no longer holds a move for ever.

- **A network call without room no longer ends the firmware on devices with the cable** (#228). The first network call of a plugin makes the network service. On a profile with the cable server this took a second piece of memory that could not be refused; when there was none, the firmware stopped instead of answering "no memory". The cable's link is part of the service's own block now, and a call without room answers `ERR_NO_MEMORY` as on every other profile.

- **A received upload is on the card before the plugin hears of it** (#228). The cable's last body part syncs the card first; on FAT an appended file's entry was not written yet, so a book sent over the cable could not be imported (`ERR_UNSUPPORTED`), and a power cut could lose a body already answered for. A card that cannot sync refuses the upload with 500.
- **A question Back closed is answered before anything else** (#228). Answers that wait when a service pass starts reach their askers before requests and events are served, so a computer's next request no longer finds a cancelled question still open.
- **The cable's first line makes `core.net`** (#228, #248). With `core.net` made at its first call, a shell that wires it with the cable gets the device made when the computer's first line comes; the service pump's passes never make it.

- **A plugin called by another plugin is metered as itself** (#228). While a native plugin answers a directed stage call (the document and lookup services), the usage meter charges the callee, and the caller's time stops until the call returns. A refused call charges nobody.

- **Sleep can show a preset picture from a code plugin's folder again** (#228, #243). Root 1 now also shows the install folder `plugins/<id>/` of a running code plugin, read only and behind `uses.storage`, as it does for a running data plugin (`files.md` 1.1). This covers `core.files` `stat` and `list`, and `image_prepare`. The plugin's code files (`code.ppn`, `code.so` and the other code file names, also under an 8.3 name) and the host's `.install-marks` are never shown; everything else there is readable by every plugin with `uses.storage`, so private state belongs in the plugin's data folder, and every change there is `ERR_DENIED`. Beta 3's preset pictures such as `plugins/<id>/owl.png` show again instead of the fallback.

- **Data plugins install on devices that load no plugin code** (#244, #228). On Boox/Android, desktop and web, an install of a data plugin (a font, a dictionary: `plugin.ini` and files, no code) was refused with `ERR_UNSUPPORTED`, because only the ESP chips had a plugin host. A shell can now set a data-only plugin host (`KernelBuilder::data_plugin_host`, `Kernel::set_data_plugin_host`) on the firmware queue: plugin jobs install, check and remove data plugins, and a plugin with a code file ends with reason 19 (`ERR_UNSUPPORTED`), as code for another kind of device does.
- **An installed data plugin loads at once** (#244, #228). After `install_plugin` of a data plugin (flags without "at next start"), the kernel neither loaded it nor raised `plugins_changed` until a restart, so fonts-browser kept showing "Download". As interface 6 did, the new version now loads before `install_done` is told (an update replaces the old one) and event 70 is raised.

- **A data plugin whose install finishes during a sleep or a plugin's stop loads at once too** (#244, #228). When the device went to sleep, or the plugin that asked stopped, while an install swapped the new version in, the swap finished in the stop's bounded steps but the plugin was loaded only at the next start, with the old version still loaded over the new files. It now loads there as after any other install.

- **Pictures from a data plugin's folder** (#228). Settings and presets can name a file of a data plugin (the sleep picture `plugins/owls/owl.png`, a logo, a picture folder); native sleep could not show them because the card's `plugins/` is hidden from plugins. The running data plugins' folders are now readable, read only, through the card root (`core.files` `stat`/`list` and picture preparation, with the file permission as before); code plugins' folders stay hidden and nothing under `plugins/` can be changed.

- **A broken `plugin.ini` no longer keeps English hyphenation away for good** (#228, follow-up of #737): the default pack is kept, and marked as dealt with, only where the plugin found on the card is one the loader takes as hyphenation (`paperpaw_core::hyphenation_tables`, the loader's own rule: it parses, is made for formats 5 or later and names a table that is in its folder; a missing `id` is the folder's name). A `plugin.ini` that is no such plugin is removed, the card synced, and the pack written over it (a failed write takes back only the files this start made, never the reader's other tables); a file that cannot be judged is left as it is and looked at again at the next start. The build of the package now runs the package check, so a bad embedded pack fails the build.

- **Setting defaults keep an override stack** (#228). When a plugin that set
  another plugin's setting default is removed, the default set by the plugin
  below it comes back, not the original one; the original returns when the
  last one goes.

- Less memory at the start with a large library: the host reads native books' details (an EPUB's title and author) only once the device has been left alone for a second after the start or an input, and not while the library is first walked, so an EPUB's open no longer adds to the start's peak (#248).

- **`paperpaw-dev` shows the native home screen and settings.** The tool built the device model without the native plugins, so its kernel had no home screen and the checks that place a widget on it (the gallery, the screen and widget sessions, the performance probe) failed. It now links them, as the app does by default.

- Interface 7: the dialog of a question (the keyboard for a text) opens with the hand-over the host makes for it, holding `core.request` and `core.kind` (`docs/abi/ui.md` 3.2). Nothing made it, so a native keyboard opened with none and refused (`Invalid`): no text could be asked for on the native plugins.
- Interface 7: the request of a question is open for its dialog from the start of the open, so the keyboard reads its question while it opens (it was `State` until the open returned).

- **A document cache that is removed half way no longer looks whole** (#228, `docs/abi/document.md` 2.1). Removing a conversion's cache folder takes its `.complete` stamp away first, so a removal that is cut or fails (or one that goes on in the next call) can never leave a folder whose stamp says it is finished with its pages gone.

- Removing the host's cache of a document no longer cuts pages from under a stamp: a cache whose stamp the card refuses to remove for good, or that has more conversions per plugin than are looked into, is left whole for the clean-up of books no longer found; and a removal step that took nothing but moved on goes on.

- **`event.document_changed` and `event.document_closed` reach the opener** (#243). They were handed over as a bare record, which a callback refuses before the plugin sees it; they are the one-record batch every event is. A book prepared at its first open no longer stays at "Opening the book for the first time".

- **A document image from a format's `fmt.image` record** now carries "not decoded yet" as `doc_image.flags` bit 1 (#228); bit 0 there is colour, as in `image.info`: the cache file's header says colour and the screen keeps colour (never set on a gray screen).

- **A book whose format cannot be called now no longer holds up the others while they prepare** (#228). Where several documents prepare in turns (not the X4), a step that has to wait still passes the turn on, so a book whose format plugin is not started yet does not starve the rest. A kernel test now drives the preparing loop through the kernel's own pump with a format plugin in the instance registry.

- **Opening and reading HTML and Markdown (the shared reading code EPUB conversion also uses) no longer aborts on a full heap** (#228). The shared reading code answers a refused allocation with an out-of-memory refusal instead of aborting; successful reads are unchanged. A sweep test refuses each allocation in turn.

- **A long paragraph of a Word document is cut at a space, not in a word** (#228). The cut at a space after 3,000 bytes never came, because the limit for text without spaces (4,000) was below it: every paragraph over 4,000 bytes was cut at exactly that byte, in the middle of a word, and the reader showed a break there. The cut now falls at the next space after 3,000 bytes (text with no space is still cut at 4,000). Books converted after this update have their positions inside such a paragraph shifted by a few characters (the cut moves): a saved position there may sit a few characters off. Books already converted keep their cache until it is made again.

- **Emulator: downloads of a scripted run land on the card** (#228). On simulated time (tests, `--script`) the emulator handed `core.net` and `core.sockets` the secrets' store in memory, so a native plugin never found a file it had downloaded (the fonts browser's list said the library did not answer). They save on the card now, where the plugins read them.

- **The emulator installs plugins from the store again** (#228, #257). The emulated X4, X4 Pro and X4 Classic take plugin installs, checks and removals as the device does: the kernel gets a firmware host with no firmware to install (plugin jobs share its queue) and the plugin host of the device's chip, and linked plugins are offered `uses.install`. A plugin file is checked against the device's loader and the services the emulator wires; an installed code plugin is the device's from its next start (the emulator runs no device file). `Kernel::set_plugin_host` tells the kernel after the build, for a shell that knows what it offers only once its services are wired.

- **A book with a picture before its first text opens in the native reader** (#243). The EPUB family decodes a picture when a page shows it, so the first block of a book that starts with a picture (a title page, a cover page) named a file that was not there yet; the host left the picture out and then refused the picture block, and with it the whole book ("could not be opened"). The picture is now decoded before its block is sent. A picture that cannot be served at all goes out as a paragraph with the block's text, so one broken picture costs only itself, never the book.
- **A heading or paragraph with several languages no longer stops a book from opening** (#243). The EPUB family sent a block's inline-language records before its text; the host checks the order (header, text, runs, maps, picture, columns, cells, rubies, languages) and refused the block, and with it the book. The languages now go last.

- **A native plugin's failed event callback is no longer silent** (#228, #243). When a plugin's own event callback (`Started` included) returned a negative status, the host showed it nowhere, which is why an undersized reply buffer in the reader's `Started` went unseen. The kernel now keeps a note in a table of a fixed size, one for each plugin and event, and says it as one warning (plugin, event, status) when the shell takes its warnings; six pairs are kept and any more are counted. Nothing is allocated where the callback fails, and what happens to the plugin is unchanged. A plugin that starts again is said again. A plugin's own `ERR_BUSY` and `ERR_STATE` are not said (the host's refusals use them too), and a screen's callbacks, which go through the screen queue, are not noted yet.

- **The folder-tree removal of the FAT storage takes less stack** (#228). On the
  X4 Pro (ESP32-S3) `remove_tree_step` needed 2,928 bytes of stack, over the
  2,048 bytes anything a plugin can reach may take. The walk now keeps each
  step in its own function and changes its folder frames in place instead of
  passing them around; the largest frame of the scan is now 1,712 bytes. What
  the removal does, its limits and its behaviour at a power cut are unchanged,
  and it takes no more heap.

- EPUB 1.0.5: zipped FictionBooks preserve out-of-memory and storage errors
  instead of reporting a malformed book. Unsupported or damaged unrelated ZIP
  members still allow a readable FictionBook to open (#228).

- **Leaving the file manager no longer queues a redraw of the screen it closes.** *Back* in the top folder redrew the folder it was about to close, and the kernel dropped that redraw with a warning ("its screen is covered"). The browser now draws anew only when going up a folder changed what it shows.

- File manager: when the card service runs out of memory, the line above the buttons says so ("There is not enough memory for that") instead of "That did not work".

- **A book opened for the first time is ready sooner.** The cache of an EPUB, FictionBook, HTML or Markdown book is written in pieces of 2 KiB (1 KiB for the contents and indexes) instead of 256 B to 1 KiB, and a UTF-16 text file in steps of 2 KiB instead of 1 KiB. Each write is a call to the card (about 27 ms on the X4), so the first open of Alice makes 279 instead of 659. The cache files are byte for byte the same.

- Format providers (XTC, Word, Mobipocket, picture folders and the EPUB family: EPUB, CBZ, FB2, HTML, Markdown): the end of a paged list (sections, contents, links, fonts) now carries the last-page bit on the final page and a generation that is never 0, as `document.md` requires. The host refused every such list, so a book could not be opened.

- **A pulled card is also noticed on the file read last** (#228). A folder
  volume (desktop emulator card, Android shared storage and SD cards) that
  is gone says "card gone" on the next read of the book that was open, and so
  does a volume whose access was revoked, where it answered "unsupported".

- **Choosing fonts while running no longer aborts on a short heap** (#228). After a plugin brings fonts and when plugins change, the choices of reading font, status font, key map preset, page frame, voice and reading apps, and the values plugins add to them, are offered, and the fonts chosen are opened, with every allocation checked; where there is no room the choices and fonts in use stay and a warning says so (the next refresh tries again). A native plugin's folder written with a trailing `/` is the same folder, an empty one is none, and a font file the device cannot read proposes no font for its role.

- **Fonts from the internet: *Your own fonts* works in the native plugin** (#228). It opens the part of the help about font files of your own in the reader again, in English or German, as in Beta 3, instead of refusing. The page is written to the plugin's data folder and handed to the reader; the help text is not copied into memory for it.

- **Fonts browser on interface 7: a font's page opens on the X4** (#228, #244). The list handed the whole font over to its page, more than the 1,024 bytes a screen may be handed where the library lists the font's packs; now it hands over the font's id and the page finds the font in the catalog it keeps.

- **A Word document with a link address that has a multi-byte letter among its first characters**
  (#228) no longer stops the reader with a panic while its links are read.

- **Opening and reading an EPUB, a FictionBook or a comic book no longer aborts on a full heap** (#228). The package, navigation, metadata, cover and cache code of these formats answers a refused allocation with an out-of-memory refusal instead of aborting, as Markdown and HTML do; successful reads are unchanged. A sweep test refuses each allocation of an opened and fully read book in turn.

- **The hosted construction tests of the plain text and XTC formats** (#228) pin that the host admits the format declaration (they still said it was refused) and run with `--features linked`; the X4 plugin files are 55,984 and 49,408 bytes.

- **Text and XTC books open in the native reader** (#243). Formats TXT and XTC are linked native plugins now where the reader is (`native-linked-reading`), so `core.document` finds a format for them; their interface-6 forms are left out of the same builds. TXT's lists (sections, contents, links, fonts) end with the last-page flag and a generation, as the host checks them: before, the host refused them and the book would not open. XTC needs Services' list-end PR (`services/fmt-list-end-243`) for its own lists: merge both together.

- **EPUB, HTML, FB2, Markdown, CBZ, DOCX, images and MOBI books open in the native reader** (#243). The format plugins are linked native plugins now where the reader is (`native-linked-reading`; DOCX, images and MOBI where `native-linked-store` links the store's plugins), so `core.document` finds a format for them instead of answering "not found"; their interface-6 forms are left out of the same builds.

- **No plugin can write a user grant** (#228). A command that sets, resets or previews a `core.grant.*` setting is refused as denied, whole batch with it, for every plugin including the one the grant names and the settings editor, whether or not such a setting is registered. Before, an unregistered grant key was refused as invalid and a registered one was treated as an ordinary setting.

- **Decoding a picture on a nearly full heap no longer aborts** (#228). The decoded-picture
  writer of `paperpaw-image` writes its 12-byte header from the stack, and a row longer than
  the room it took at the start is written as it is instead of growing that room.

- **A file a plugin hands to the reader opens as the handed file** (#228). The reader read a hand-over with `root` `0` as its own data folder and `2` as a library book; it now opens `0` (the maker's data folder) and `2` (its install folder) as `doc_open` root 5, the read-only file the hand-over names (`document.md` 1). *Your own fonts* and the licences page of About can open their text in the reader this way (the plugin side follows in their own PRs).

- **Search in a book a plugin handed to the reader** (#228). The reader opens its search screen from the answer to the question for the text, an event callback in which no screen instance runs, so the hand-over of the handed file (`root` `5`) was refused. In an event's callback the host now takes the topmost screen of the calling plugin that holds a file of that path (same path, same maker, read only), so a search in a plugin's own text works as in Beta 3.

- **The table of contents of a book a plugin handed to the reader** (#228). The reader's table-of-contents screen opens the same file again, but its hand-over (`root` `5`) was refused, so a text such as the licences page had no table of contents (Beta 3 had one). A reader instance that holds a handed file now hands it on with `root` `5` and the same path: the host gives the new screen the same file of the same plugin, and nothing else (another path or an instance without a handed file is refused). The maker stopping still ends the document.

- An `open` a native plugin stages with a hand-over (the reader's open-last, home's open book, the selected text of a translation) is no longer refused: the stages of the screen and event callbacks take the hand-over store and the callback's stamp, and the hand-overs a callback made that no open claimed are freed when it returns.

- Hints: the plugin no longer registers for `event.screen_shown`, which the host refuses, so it can be installed; it follows other screens through `event.screen_ready` and `event.stack_changed`.

- The Add a widget screen of the native home draws again: its title was translated twice, which the host refused as an unknown message.

- **The tools drawer and the home screen's tools are told as buttons by their whole name, and the drawer is mirrored where the interface runs right to left.** The drawer told its tools as plain text, with no button and no focus, and the home screen split a long tool name ("Plugin store") into a button and a loose word, so a screen reader and the tests could not find them. Each tool is now one button named by its whole name, with the focus on the one that has it. In a right-to-left language (Arabic) the drawer starts at the right, a tap finds the tool where it is drawn, and *left* goes on to the next tool, as in Beta 3.
- **The home screen's editing works by touch and by buttons in the places Beta 3 had it.** The options of an item (move, size, remove) were told as plain text, and a finger could not tap them: the dialog took the touch without making a tap of it. A tap on nothing while arranging opened the main menu. Choosing a widget or tool from a kind's list in *Add a widget* left the list of kinds open instead of going back to the page.
- **A swipe while arranging the home screen keeps the page, as in Beta 3, and a tool beside a widget is told by its own name.** A swipe turned the page under the item being arranged. On a page with a widget (the book card) in front of the tools, a tool was told by the name of another one.
- **The home screen's book card and recent shelf ask the host about a cover once, not at every draw.** The X4 draws a frame in bands, and each band asked the host to look at the book's files again (its size, its cache) for the cover. The answer is kept until the books change or the cover was still being made, and asked for anew if the host ended the picture.

- **The home screen draws on the X4 when a page carries a widget of another plugin.** Where the device will not let the home screen place such a widget yet, the widget is left out and the rest of the page draws, instead of the whole page staying blank.

- **Home screen on the new plugin interface: its own widgets show** (#228). The date, the clock,
  the continue-reading card, the recent shelf and the status line are drawn on the home screen and
  in the *Add* gallery again, and a tap on a book of the card or the shelf opens it.

- **Home: a tool's name is no longer cut inside a word** (#257). As in Beta 3, a name a little too wide is drawn a little smaller, a longer one breaks between its words or after a hyphen it holds ("Notizen-" / "Tresor"), and a single word too long for a line ends in "…" instead of breaking as "Bibliothe" / "k".
- **Home: Settings has its gear again in the tools drawer** (#257). Home adds the Settings tool itself when no menu lists it, and gave it no icon, so its cell was blank above the name.
- **Home: German tool names are hyphenated again** (#257). A word no line holds breaks where the interface language's hyphenation allows ("Einstel-" / "lungen"), as in Beta 3, instead of ending in "…".

- **The clock, date and battery show in native status lines again** (#257, #228). The host's status values (`core.ui` op 14) only knew items that native plugins declare, so the kernel's own items came back empty: the reader's status line, home's top row and picture-home's bar showed no time or battery. The host now answers them itself, without calling any plugin: the time and dates as text, as Beta 3 wrote them, and the battery as Beta 3 drew it, symbol and percentage, through the interface-6 renderer. The registry's status listing (op 21) names them with Beta 3's priorities, so lines lay them out and the item choosers offer them. On the X4, until its nested-call gate opens, these items now show while other plugins' items stay empty (before, op 14 refused altogether).

- **Hosted construction tests of fonts-browser and update** (#244). They still expected the host to refuse the plugins' `install_done` / `firmware_changed` registrations; the host admits them, so the tests now assert that construction succeeds.

- **An icon is no longer read out as a word.** The icon of a row (or of a button) was told to the screen reader as a line of text, or as part of the row's name ("Search" and a symbol). Icons are pictures and are left out.

- **Picture requests that fail always get an answer** (#228). A `command.image_prepare` the table refuses for good at apply (the submit already said yes) is told to the plugin as `event.image_ready` status 1; failures that wait for their telling give way oldest first, and the requests a plugin has held back count against its room, so a plugin cannot flood them. The service pump no longer runs a waiting batch inside a callback. Known limit: with more than eight failing requests across plugins at once, the oldest failure is dropped and its plugin is not told of that request.

- **The widget pack no longer fails to start with `NoSpace`** (#228). The index budget for declarations a plugin borrows from its image is 12 KiB (was 8 KiB): the pack declares 407 entries in 65 batches, and the batch and name vectors double, so the 65th batch could not be reserved. Four `native_image_declarations_tests` that failed with `Factory(-3)` pass again.

- **Decoding with transparency or colour, and screenshots, no longer abort on a full heap**
  (#228). The alpha and colour picture writers write their headers from the stack, and the
  PNG writer takes one fallible reservation per row (and one for its header) instead of
  growing buffers infallibly.

- **A converted-book index with billions of blocks can no longer overflow an offset** (#257). Two reads of the index still multiplied a block number by the entry size in 32 bits before widening it. They now widen first, so a number out of range reads nothing instead of panicking (debug) or reading the wrong place (release).

- **Input filters: bad answers about a held key or finger are counted** (#228).
  When a filter answered with an unknown verdict or an invalid replacement
  for the rest of a key or finger another filter had taken, the answer was
  dropped without a trace; it is now counted and logged like a bad answer to
  the first press, and the input is still taken.

- A native plugin's event and action callbacks can set the state of its items (`command.set_item_state`): the stage of those callbacks admits it as a screen's does, and the state is stored when the kernel runs the batch. The update plugin's "open" item was refused (`ERR_UNSUPPORTED`) at start.

- **JPEG and PNG decoding no longer take 4 KB of stack each** (#228). A JPEG's quantization and Huffman tables, and a PNG's palette tables and read buffer, are kept on the heap instead of in the stack frame of `paperpaw_image::jpeg::decode` (3,808 B before, 224 B now) and `paperpaw_image::png::decode` (4,080 B before, 1,056 B now), as the X4 plugin packer refuses a frame of 2,048 B or more. The heap they take is freed with the decode: 1.8 KB for a JPEG, 1 KB for a PNG and 1.5 KB more for a palette image (2.5 KB at most at once, 1 KB for the tables' source); a refused allocation is `OutOfMemory`. Decoded pixels are unchanged.

- **A JPEG's component list, a progressive scan's member list and the default colour and see-through row conversions no longer abort on a full heap** (#228). Decoding a baseline or progressive JPEG, or handing a sink colour or see-through rows it leaves to the default, on a heap with no room now ends with an out-of-memory refusal, as the rest of the decoders do. A sweep test refuses each allocation of real JPEGs and PNGs in turn.

- **A configuration value for a native plugin's setting is applied** (#228). The kernel reads the configuration file (and the shell's `--set` values) when it is built, before a native plugin is hosted, so a value for a setting that plugin declares was warned about as "unknown setting" and never set. Such values are kept and set as the plugin is published; the warning is taken back, and a setting no plugin declares is still warned about. A secret setting's value is moved to the secrets as at the build. Values are kept only until the start, so a plugin installed after it does not get them; at most 64 are kept.

- **A library query leaves out the archive folder again** (#228). `core.library` reads the files plugin's `files.archive` setting when a query asks to leave the archive out (`lib_query.flags` bit 1) and hands it to the library host, as Beta 3's book lists did; a query that does not ask never reads the setting.

- **The library is collected with native formats only** (#243, #245). With interface 7 the library stayed at "Preparing the library … 0 books": no native view asked for the catalog, and the walk only knew interface 6 formats. Now the first read of the library (`core.library` state or query) since the start or a wake asks for the catalog once, as Beta 3's library view did when it showed. The files of native format plugins (their declared extensions) are books of the walk and of the book check too, together with the interface 6 formats. A format plugin that stopped (after a fault) but is still installed keeps its books in the library; only one removed or turned off takes them out, and a list the device had no memory for is made again at the next pass, not at the next plugin change.

- **A library commit ends the caller's transaction whatever it answers** (#228). `edit_commit` on one's own transaction used to leave it open (until the 10-minute expiry) when parts were missing or the reply did not fit; it now ends it, as the SDK expects, and a later commit or abort on the id answers `ERR_NOT_FOUND`.

- `core.library` op 7 `cover` on the production LibraryHost: the screen's long side comes from the `documents` wiring, in whichever wiring call that comes; before, every cover call was refused as unsupported (#228).

- **A book's details are written without an infallible allocation** (#228). `paperpaw_sdk::library::save` (and what the library keeps itself, e.g. the `provide` of a source) makes the text of the details file with `try_reserve`, counts it against the 16 KiB limit before making it, and answers `OutOfMemory` or `TooLarge` instead of aborting; the text is the same as before.

- **The library's lists by author, by series and by tag show their names again** (#257). On interface 7 the host did not implement `core.library`'s `facets` (op 5), so *By author*, *By series* and *By tag* opened an empty, dead list, the shelves showed no counts, and the letters and folders lists had nothing to read either. The host now lists authors, series, tags, first letters and folders of the books a query finds, each with its number of books and, for authors, series and letters, where its books begin in the list by that order, in pages by cursor (`docs/abi/library.md` 2.1; the names' order is written down there now). Memory of one call does not grow with the library: a window of 32 names and at most 4 KiB for counting them (about 11 KiB at most for a page of authors, measured in the host tests); a library with more than 512 names of a kind is counted in more scans of the catalog. No change to the interface's records.

- **Indexing a book and setting a book's field no longer allocate infallibly** (#228). The metadata the indexer reads from a book is copied with `try_reserve` before anything is changed, the marker text of its details is built without `format!`, and `paperpaw_sdk::library::set_field` copies its id and value fallibly and writes through the same fallible details writer as `save`. With no memory a book is not written down (it is read again at the next walk) and a field is as it was, instead of the device stopping; the catalog line of a book left that way is made again at the next walk.

- **Bookmarks, highlights and the reading position are kept in a library's first session** (#228, #257). The native reader keeps them through the library service, which looked the book up in the catalog, and the catalog is made only once a view of the library asks for it. So a book opened before then could keep no mark ("could not be kept"). Its own data (annotations, their texts and anchors, the position) is kept by its path, as in Beta 3, when the card holds the file and the catalog does not know it yet; what the catalog answers (status, fields, details) still needs it.

- A library move no longer swallows a card write error while it carries a book's details, cover, recent and favourites place: the step fails with the card's status and is repeated (it went on, ended normally and left them under the old path). The replaced book's discard likewise (`library::try_moved` / `try_removed`).

- File manager's id table now also names its action and screen ids, so a linked build keeps
  none of them as heap copies (#248).

- **Linked built-in plugins keep their declarations and settings in the program, not on the heap** (#228, #248). A linked plugin is now hosted with the program's read-only data as its image (`Kernel::register_linked`, `paperpaw_app::register_native_plugins`), so its declarations are borrowed from it as a loaded plugin's are: the widget pack (131 settings, past the 128-copy limit) registers, and the copies of the declarations are gone. The widget pack's settings are also kept as a table in the program, made by its build script from the same declarations and used only where it equals what the plugin declares (Beta 3's way, about 4 bytes of heap for each instead of about 190); any other setting is copied as before. The list of declarations a plugin admits holds one entry for the table's settings and boxes the large ones. X4 firmware still passes no image until its read-only range is given.

- **A linked native plugin's texts are found** (#228). A plugin built into the program has no plugin file to carry its translation files, so nobody added them: its titles and labels (the keyboard's, the menu's) were not found, and the screen failed. `paperpaw_app::add_native_catalogs` adds the `i18n` files of every linked plugin to the translator before the start (`register_native_plugins` does it itself); `Kernel::add_catalog` and `Kernel::reserve_catalogs` are the kernel's two calls.

- **Lists and the keyboard on interface 7: a step of the focus redraws what it leaves and what it reaches, not the screen** (#228). The table of contents, *Add widget* and every other list of a native plugin, and the on-screen keyboard, asked for the whole screen at each step; now they ask for the row (or key) the focus leaves and the one it reaches, as Beta 3 did, and the whole screen only where the step turns the page.

- **A damaged file of wrong tries counts as all of them used** (#257). When the lock started and the file that keeps the count of wrong codes was cut, damaged or too long (a power cut while it was written), it was read as none. It now counts as five, so the next try waits half a minute and the count is forgotten after it. Where there is no file at all, the count is still none.

- **The lock says when it cannot keep the wrong tries** (#257). A card that refuses the count of wrong codes (full, read only) used to be ignored without a word, so the count was lost at the next start. It is now said (in English and German) at the first wrong try the card refuses, and not again until a write went through. The attempt itself is still not refused: a card that cannot be written must not lock the owner out.

- **An unreadable count of wrong tries no longer counts as none** (#257). When the lock started and the card could not tell how many wrong codes were kept, the count began at zero. It now begins as used up, so the next try waits half a minute, after which the count is forgotten as usual.

- **Interface 7, lookup: a provider's `lookup_ready` reaches the asker** (#244). The host staged no `command.lookup_ready` (`ERR_UNSUPPORTED`) and kept no pending ask on a device, so an ask answered `pending` could only end by its deadline. The command is now checked, applied after the callback (a work run's callback may send it), and each subscribed asker gets `event.lookup_ready` from the event pump; the pending asks of a plugin that stops end with it.

- **Emulator: a native plugin's HTTP answer no longer waits forever** (#2795). After the desktop
  shell tells core of an answer (`core.net`) or a socket report (`core.sockets`), it ticks the
  kernel once, as the X4's shell does: the kernel's pass is what tells the plugin. A download whose
  screen waited only for its next answer stayed at "1 / 4".
- **A plugin that asks for pictures without subscribing to `event.image_ready` is warned** (#2795):
  its answers are dropped as the contract says, now with one runtime warning naming it.

- A mail's HTML text that is cut at the length limit now ends as a plain prefix of the whole text, as the text of other mails does.

- **`MemoryStorage::list` no longer aborts when memory runs out** (#228). The SDK's in-memory storage for plugin tests answers `StorageError::OutOfMemory` when the heap refuses, before anything is listed, and lists a folder without subfolders without allocating, so out-of-memory tests fail on the code they test.

- **A menu row for a choice setting says its value in words** (#257). The native menu showed the choice's key (`Power mode, power.mode.auto`, `files.sort.recent`); it now shows the translated text (`Power mode, Auto`, `Recently read`; in German as well), as the interface-6 menu did.

- **A menu entry that says it is on is told as a switch that is on.** A plugin's menu entry with a "checked" state showed "On" as its value, but a screen reader and the accessibility text did not know it was on; the row now says it, as in Beta 3.

- Natively declared menu rows stand among the other rows by their order, as in Beta 3, instead of after all of them: the menu lists Edit home screen before Settings again.

- **Menu: the reader's font and layout panel opens again** (#257). A choice whose value has no text of its own (said as it is, as in rows) made the whole native panel fail, so "Font" in the reader showed nothing.
- **Menu: numbers in the reader's panel say their unit** (#257). The panel showed "9 core.unit.pt" instead of "9 pt", and its round `−` and `+` buttons showed "…" because the label had almost no room.

- **Native screens: every button has a name** (#243). The settings' stepper buttons are told as "−" and "+", and the native home's footer buttons by their names (the pencil as *Edit* or *Done*, *Add* as "+", an action by its name, or as *Action* where its plugin gave it none), as interface 6 told them, so reading aloud and automation find them. ui-kit gains `paint::describe_named` for parts that show an icon or a sign and no words.

- Interface 7: an action a native plugin declares (Transfer, About, Bluetooth …) now does what the plugin asks, such as opening its screen, in the call that ran it. The host ran the action's callback without a command stage, so the plugin's request was refused and nothing opened from a button, a rule or a script's `run:`.

- **A book read with the native reader is listed under "Continue reading" again, and its progress and end are kept** (#228, #243). The kernel kept the books opened last, how far each is read and whether it was read to the end only when the interface-6 reader's events came; the native reader's reports raised their events for native plugins alone. Each event of the reading session is now raised on the kernel's side too, as the interface-6 event of the same name (not told to the native plugins a second time), with the progress rounded as before: the library writes as often as in Beta 3 (a changed percentage, the book opened, closed or finished). A book closed for sleep and opened again after it writes nothing, as in Beta 3.

- **A linked native plugin can add a choice to another plugin's setting** (#228). A `setting.choice` record was refused at admission with `ERR_UNSUPPORTED`, so a plugin that declared one (vocabulary's sleep card for the sleep screen) was not registered at all. The host now offers it as interface 6's `add_choice` does; one for a setting that does not exist is dropped as before.

- Placed setting reads apply grant and secret visibility to the canonical setting
  identity and reject nested placements. Deferred UI cleanup retains all closed
  instances for retry, and weak directory teardown preserves memory safety (#228).

- **A plugin check says which interface revision a file needs** (#228). `dev_check.needs_draft` (op 6) is the header's `needs_minor` for a plugin file the loader refuses as needing a newer minor (reason 10), and 0 for every other result, as `device.md` 5.1 says; a check no longer reports a file that the loader will refuse at the next start without the number the store needs to say why.

- A plugin's callback reached from inside another plugin's (a stage subscriber, a compose child's draw) now runs on the usage meter as itself, so its time, storage and network are charged to it and the secrets and Bluetooth views follow, and the caller runs again after it.

- The setting `core.network-manager` now exists and names the Wi-Fi plugin by default, so native Wi-Fi hears `network_wanted` and `network_idle` on a fresh device, as in Beta 3. It can be changed (empty: no manager) and reset in the settings.

- **A choice offered for a native plugin's setting waits for it** (#228, #244). An interface-6 plugin's `add_choice` for a setting a native plugin declares (the vocabulary's sleep card for native sleep's `sleep.show`) was dropped as "no such setting", since native plugins declare their settings after the kernel is built. Until the start such a choice waits and is added when the setting is published; one for a setting nobody declared is dropped at the start, with its log line.

- Pictures that a format has not decoded yet (DOCX, MOBI) are no longer left out: the host passes
  them on without an id, at the size they will have, and the reader asks for them by index; only
  their path and a size of 1 to 1,600 pixels are checked until then (#243).

- A picture block whose picture is missing or fails the image check no longer closes the book:
  the block keeps its alt text and the host notes it once a document, as `document.md` section 5
  says (an EPUB failed to open after three such blocks) (#243).

- Fix blank standalone picture books in the native reader: request a whole-page redraw together with picture areas so the host schedules the follow-up frame that draws them.

- **Picture home on interface 7: small patterns tile** (#228). A tiled picture is drawn however often it repeats, as on interface 6 (it stopped at 256 tiles, 32 in a row: patterns under about 64 pixels stayed plain); only a tile under 8 pixels a side is not drawn.

- **Picture home on interface 7: tool names fit as in Beta 3** (#257). A name a little too wide shrinks onto one line; else it breaks between words or after a written hyphen, and where no line holds a word it is hyphenated in the interface language ("Einstel-" / "lungen"); "…" only where nothing fits.

- **`tools/plugin-files` no longer lists News and Picture home.** Their interface-6 form is gone, so the build had no binary for them and the firmware-build check "the store's plugins fit the firmware" failed on a missing `news` file. Their store files come with the interface-7 store build. The path test uses a plugin that is still listed.

- **A plugin is not loaded without its row in the plugin list** (#248). The kernel copies a plugin's manifest for the list before anything of the plugin is registered; where there is no room for that copy, the plugin is not loaded (with a warning), instead of running unlisted, which could have let a second copy load or an update install afresh.

- **A plugin-defined stage takes several input records** (#228). The host cannot
  see a stage's own `input_rows`, so the stage broker's input check for
  plugin-defined stages now accepts any canonical batch of up to 64 records
  within `max_input`; 65 records, trailing bytes and truncated frames are
  refused.

- **The power plugin's event handler no longer takes over 2 KiB of the X4's stack** (#228). It decoded the eight kinds of event it handles into one array, a frame of 2,064 bytes (the stack's band is 2 KiB high); it now tries one kind after the other, and the frame is 144 bytes. A check now makes sure no linked plugin's entry frame reaches 2,047 bytes.

- **Reading profiles skip a stale choice** (#228). A profile that keeps a choice the setting
  no longer offers (a font that was removed) applies the rest of its values instead of
  failing, as before. A profile that does not fit the command budget applies nothing.

- **Security:** `paperpaw.ini` at the card's top is out of reach of plugins through every interface-7 route into root 1: `core.files` (list hides it, stat, make_dir, remove_tree and both ends of a move refuse it), the library's paths and imports, the network and sockets saves, and the image commands refuse it in any case and as the FAT driver folds names (`paperpaw.ını`), with the same error as `plugins/` and `.paperpaw/`, and the host's identity check names it with the other host entries. A plugin with `uses.storage` could otherwise have written `dual-boot = off` there and lost the user's second firmware at the next update. The kernel's own reads of the file are unchanged. All routes share one helper (`owner_paths::protected_first`).

- **A plugin's configured settings no longer cost heap through the whole start** (#248, #228). When a native plugin registers, the device defaults, the choices other plugins offer and the values of `settings.ini` for its settings are applied, which reads each such setting's description from the plugin. Those descriptions stayed on the heap until the end of the round, and at the start every plugin registers within one: about 0.36 KiB per configured line, up to about 23 KiB at the registration peak. They are let go at the end of each plugin's publication now.

- Draw native quick-panel choice values at the same text size as their row labels, matching Beta 3.

- **A book whose file is gone from the card says so.** The reader shows "The book's file is no longer there." again, as in Beta 3, instead of calling the file damaged.

- **The reader on interface 7 follows a moved book** (#228). When the book read last is moved, the last
  book kept for the next start follows it, as in Beta 3.
- A last book that is gone from the card is forgotten at the start, which stays at home.

- **The status line no longer reads the page index while it draws** (#228). The index remembers only its last answer, and the reader asked it for the chapter's start and end after the page itself, so the next draw looked the page up again, from the file. What the status lines ask of the index (the chapter's start and end, the page) is now found when the page is turned or the count ends, the page's own last.

- **Hanging punctuation and Korean word breaks work in the reader** (#257). Both settings were shown in Settings but the reader never passed them to the line breaker. Hanging punctuation now hangs quotes and dashes into the margin, and Korean books break between words only where chosen; a change lays the page out again.

- **Select opens the reader's menu again on interface 7** (#243). On the reading screen, Select or
  Menu with nothing chosen opens `core.reader-menu` in the menu slot, as Beta 3 did, with the book's
  title, chapter and page for the bar, and without search, reading aloud and quick settings for a
  book of page pictures.

- **The page of a book shows no row of button hints.** The row ("Back, Select, ‹ ›") under the text was drawn on devices with buttons while reading, as it never was in Beta 3; the page of the book is declared without it again.

- **The reader on interface 7 opens the last book at the start** (#228). With `reader.open-last` on, the
  book read last opens again when the device starts, if it is still on the card; the last book is
  remembered when a library book opens, as in Beta 3. Before, the native reader had no start handler and
  the start stopped at the home screen.

- **The page numbers follow the reading font and the hyphenation.** The count of a book's pages was kept without them, so after another font was chosen (Literata from the store, for one) the old count was used and the page numbers were wrong. Each font and hyphenation has its count, as in Beta 3; the few counts kept stay bounded.
- **A reading font with a very long name no longer stops the book's page.** The reader falls back to the default font where the name does not fit its buffer.

- **Turning a page costs about half what it did.** The reader keeps its reading settings and the look of its status lines, and asks the host again only after a setting changed or the screen turned, instead of reading some forty settings several times for every page turn. A band of the screen draws only the lines that reach into it.

- **The reader skips its text in the draw that only counts pictures.** The host draws a page once more aside to find out whether its pictures call for grays and shows nothing of it; the reader draws only the pictures there, not the words, marks and status lines. The same pictures are drawn as before. A setting changed also makes the status lines read their settings again.

- Upgrading from Beta 3 keeps your reading places and bookmarks with the new reader: a book's place and bookmarks that the old reader kept in its own folder are moved to where the new one reads them the first time the book is opened.

- **"Publisher's formatting" works in the reader** (#257). The reader opened every book with the publisher's styles whatever the setting said, so turning it off did nothing. Books now open with or without the styles as set, and changing the setting opens the open book again at its place, as in Beta 3.

- **Reader: small capitals are drawn again** (#257). Text a book sets in small capitals ("The reader", "9 a.m.") was drawn as written in the native reader; its lower-case letters are capitals at 0.78 of the size again, measured and drawn alike, as in Beta 3.

- **Reader: the status line's chapter item says the chapter again** (#257). The native reader said the book's title on every page; it now says the chapter the page is in, and the book's title only before the first chapter or in a book without contents, as Beta 3 did.

- **A picture a text-size change brings onto the page is shown.** After a setting that lays the page out again (text size, margins, a text filter, the screen turned), the reader asks for the pictures of the page as it does when a page is turned to, so a picture not decoded yet is no longer left as an empty box until the page is turned away and back.

- **The registry lists every key-binding layer and walks slot offers without allocating** (#228). `core.registry` op 13 with an empty scope lists each layer that has a binding (not only `global`, the slots and the screens), and ops 10 and 9 read the slot offers through `Env::each_slot_screen`, so neither allocates.

- A plugin removal with its data whose secrets the host cannot forget no longer keeps its journal, and with it every later install, waiting for ever: after three starts in a row the removal is finished without forgetting. Its id is kept in `plugins/.unforgotten` (at most 8): the host tries again at every start, a plugin of that id is not installed until it has forgotten the secrets, and the user is told, naming the plugin (en/de). The grants of a removed plugin are all forgotten, whatever came of its secrets, not the first eight.

- **Switches look like switches again.** Settings, the menus and the Wi-Fi page draw a switch as a pill with a knob, as Beta 3 did, not as the word "On" or "Off". A row that is a switch is told to the screen reader as one.
- **Rows have icons again.** The menus (main menu, Tools), the search field and the settings pages that other plugins add show an icon at the start of the row. The settings' groups follow when the registry hands their icons on (Kernel todo 649).

- Restore every faint row separator on native list screens: use Beta 3's fixed alternating-pixel pattern on black-and-white panels and solid tints on grayscale and color panels. Respect the separator setting and keep headings and focused rows unlined.

- **`Ui::widget_make` reads the host's answer for the caller's own widget** (#228). The host answers op 4 for an own widget with the own marker and instance 0, like op 15; the SDK refused it as `Invalid`. `widget_make` now returns `MadeWidget::Own` or `MadeWidget::Widget(WidgetHandle)`, an own marker with an instance or an instance without the marker is `Invalid`, and `WidgetHandle::is_own` is gone: a handle is always one the host made. The home, picture-home and sleep plugins follow.

- **Passwords are wiped from memory sooner** (#228). The sync account's password key and Calibre's password hash are cleared as soon as they have been made, and the Calibre connection details no longer show the password when printed for diagnosis.

- **Wi-Fi and mail passwords keep working on a device that seals secrets** (#228). Where `core.secrets` seals values (the X4's device key), the kernel's interface-6 code now reads sealed lines and writes every save sealed, so a password kept by a native plugin and one kept by an interface-6 plugin share the file without losing each other's values. A save that cannot be sealed (on the X4 the radio must be on for a fresh nonce) is not written and not marked saved; nothing is ever written in the veiled form on such a device.

- A shell that wires `core.secrets` now tells the kernel (`Kernel::wire_secrets`), so interface 6's passwords are sealed even when no native plugin runs. A password with no owner in its id (no `.`) is refused at save instead of blocking later saves; an old such line is kept as it was.

- **Service-event callbacks get a command stage** (#228). The events the host raises (`image_ready`, work 103 to 105, `core.net` and `core.sockets` events) run with the stage every callback but `draw` has (`screens-input.md` 2.1): what a plugin submits there is kept when the callback returns `0` or `1` and run by the kernel before the next callback is called; an error drops the whole batch. In the callback of a work run (quiet) only the listed commands are staged (redraw of the plugin's own screens, settings, item states, widget values, pictures, network); any other is refused at submit with `ERR_STATE`, and so is a `core.ui` dialog (op 8).

- The settings screens no longer list a front-light setting twice on devices whose own front-light settings are the shell's: a setting that needs hardware the device lacks is not shown.

- **A settings page starts on its first setting, not on an action another plugin added above it.** The reading text page opened with the focus on *Discover fonts*; it now starts on *Font*, as before, and the action stays one step up. The same holds for any page where a plugin adds an action above the settings.

- **Opening Settings no longer slows with the square of the number of settings** (#228, #248). The settings list finds which settings another's showing depends on in one pass per list, not one pass over every setting per row; on the X4 emulator this was about three quarters of a Settings open. The flag each row carries is unchanged.

- **Up raises a number and down lowers it again in the Settings.** The buttons were the other way round while a number was changed; they now work as in Beta 3 (right and up raise it, left and down lower it).

- Settings, sleep, home and picture home (interface 7): a value is read into a buffer of 4,096 bytes, the most `settings.read` takes; the 4,160 bytes asked before were refused as invalid, so the settings screen could not open.

- **Changing a setting kept in the program no longer doubles the settings list** (#248). The first change to such a setting copies it to the heap; the list of copies is kept exactly sized, and that copy now takes room for one more instead of doubling the list (on the X4 profile with the statistics plugin's settings copied, a 35 KiB peak at start). Where the heap has no room for it, the change is refused like one with no place left.

- **A hosted plugin keeps no second copy of its registrations** (#228). The shell reads the records through the committed batch instead of a copy kept beside it, which saves about 768 bytes per plugin on the X4 heap. The registrations and ticks a plugin gets are unchanged.

- **The native sleep screen falls back to the logo or the cat when its widgets fail** (#228, #244). As in Beta 3, the message shows with the logo above it when the widgets chosen come up empty or cannot be laid out, and the cat shows when the widget over the page cannot be made and there is no cover. The widgets are made at the first draw, which can ask for no picture, so the native screen now asks for that picture when it opens.

- **The shared sorter no longer panics on a comparison that is no total order** (#228). Lists of 13 to 2,048 items were ordered through their positions by the standard library's unstable sort, which may panic on a comparison that contradicts itself (a plugin can hand `stable_sort_by` one); an in-place heapsort now does it, with the same result for any valid comparison, no extra memory and about 1.2 to 3 times the time of the standard sort on 2,000 entries. Every path gives some order of the same items for such a comparison, and tests cover them all.

- Native plugins hear the device's start at once: an event raised after a call's pump (the kernel's
  `started`, raised as the commands of the start run) now asks for the next call now, not at the
  next deadline, so *Open the last book at the start* no longer waits for the first key press (#243).

- The statistics now receive the events of their screens, actions and requests on interface 7 (they were not decoded, so a real host would have ignored them).

- Setting a daily goal the first time no longer loses the offer of *Read today* when the change of the home page is refused: the offer is remembered only after it is staged.

- **Native status lines show again where the kernel's battery is listed** (#243, #228). The registry's status listing (op 21) marked the kernel's battery items as drawn by their owner, a mark the contract keeps for whole status lines (`registry.md` section 3). The SDK refuses such a page, so every status line that reads its items from the registry (the reader's line, home's top row, picture-home's bar) and the item choosers failed and stayed empty. The kernel's items are now listed as any item; that the battery is drawn is still said by its value (`core.ui` op 14), as before.

- **The status line is back on the home screen.** The time and the battery above the grid drew nothing, because the toolkit asked the host for a setting with too small a reply. It asks with the size the host wants.

- The library's log of reading states is folded into the catalog again once it is long when the states come from a native plugin, as they were from the kernel's own notes, so a library read does not hold memory for a long log (#248).

- **Native plugins reach the user's files and their own shipped files through the storage slots** (#228, #257). Slots 20-26 now serve root 1, the card's user files, as `files.md` 1.1 says: open with the signed `uses.storage` feature, `ERR_DENIED` without it, `ERR_UNSUPPORTED` where the host offers no card files. `plugins/`, `.paperpaw/` and `paperpaw.ini` stay out (any case, 8.3 aliases), and a change of a path the host holds (the open book, a download, the library's move job) is `ERR_BUSY`, as on `core.files`. Slots 20 and 21 also read root 2, the plugin's own install folder (such as vocabulary's `data/phone.txt`); every change there is `ERR_DENIED`, and a plugin built into the firmware has an empty one. Before, every call on these roots was refused, so vocabulary's card files read as "This file could not be read".

- **The store's download screen ends at 100 %, not at 99 %** (#244). Where PaperPaw takes the end of an install, the screen showed "99 %" until a key was pressed; it shows the end and the next key closes it.

- **The native plugin store restarts PaperPaw again** (#244). *Restart now* and *Switch on and restart* were refused by PaperPaw because the store did not name the permission they need; it does now.
- **The first-run offer opens as it did in Beta 3** (#244). Opened with nothing to add, it shows its *Done* row and stays, instead of closing at once.
- **Installing from the native plugin store works** (#244). The download could not start (it said "could not be downloaded") because the store could not make its staging folder; and after a declined install the plugin's page stayed on *Getting ready…*. Both are fixed.
- **Choosing *Download* in the store's question for plugins another plugin needs closes the question** (#244). It stayed open under the download, because the screen of the download was opened while the question was being closed.

- **Tools** (#228). `tools/plugin-files/catalogue.py` refuses a plugin file shorter than its header (a format-4 file of 12 to 15 bytes raised a raw `struct.error`) with a clean message. `tools/flash-gate.sh` refuses a `CARGO_HOME` with a space (its path remaps would be split) and removes the records of its own killed runs' worktrees, no one else's. `tools/plugin-files/build.sh` refuses to build when the checkout, target or cargo home path has a space (the files would depend on where they were built); `PAPERPAW_ALLOW_UNREMAPPED=1` allows a trial build.

- **Store baseline builds** (#228). `tools/plugin-files/versions.py` no longer passes `PAPERPAW_ALLOW_UNREMAPPED` on to `build.sh`, so the files it hashes against the release's are always built with the paths remapped. Tests also refuse a spaced checkout and a spaced `CARGO_HOME` in both plugin-file builds.

- **Plugin-file builds** (#228). `PAPERPAW_ALLOW_UNREMAPPED=1`, the override for a trial build in a path with a space, is never read for a build with `--key` or `--fingerprint` (release, CI, store), so it cannot leak into a published file. `tools/plugin-files7/build.sh` refuses a spaced path as `tools/plugin-files/build.sh` does, with the same override. A test runs both scripts against a stand-in `cargo`.

- Restore the native Wi-Fi transfer screen's missing network icon and place its "Without a router" action in a bottom button, with touch and button input.

- The bounded removal of a tree (`remove_tree_step`) no longer stops with `NotFound`, leaving files behind, on a storage whose folders are only their files (the in-memory store of the host and of tests): a folder whose last file was just removed counts as empty and the walk goes on in its parent.

- **The status line of a text book shows its progress again** (#257). The text format ended its list of sections with a cursor the reader refuses, so the reader found no progress and left "0 %" out of the status line. The list now ends where it should.

- **A home screen item whose cells another item took is no longer dropped.** When two items of a page were written with places that overlap (by hand, or after the grid changed), the second was left out of the layout, and an edit then saved the page without it. It now goes to the nearest free cells, as in Beta 3.

- **Right-to-left screens on interface 7** (#228). The plugin ui-kit knows the direction of the interface
  language (from `core.language`) and mirrors what it draws, as Beta 3 did for Arabic: the header with its
  back arrow and pager, list rows (title at the right, value at the left), stepper rows and notes, the
  sheet's pager, dialogs (buttons from the right, left and right swapped) and the chips. Screens that draw
  parts of their own take the kit's `Style::flip` helpers; until they do, those parts stay left to right.

- **A pulled SD card or a revoked share no longer makes every book look
  deleted** (#228). A folder volume whose mount point is gone (the desktop
  emulator's card folder, Android's shared storage and SD cards) now answers
  "card gone" on every operation, where it answered "not found". The library
  reports a read error for its books, not "missing", and a write no longer
  makes the mount point again on the wrong disk.

- **Vocabulary no longer adds an empty line to a deck after a card error** (#248). When the last byte of a deck file could not be read, the next card was written after an extra blank line; a byte that cannot be read now counts as the end of a line, as before the host cleared failed reads.

- **Vocabulary answers a page with nothing to mark properly** (#257). With the marks off, on a page without a listed word, and for a tap on another plugin's mark, the plugin sent no bytes at all, which the host refuses as no answer: a refused call was logged for the plugin on every page. It now sends an empty batch.

- **Vocabulary on interface 7: the list of words reads nothing while it draws** (#228). The words of the page shown are read when the list opens, the focus moves or the words change, and the draw takes them from memory: the X4 draws a frame in bands, and each band read `words.txt` again.

- **Words of your vocabulary list are marked in books again** (#257). The vocabulary plugin read the thickness of the mark into a buffer too small for it, so it never answered the reader and no word was marked. The buffer is now large enough.

- **Flash cards: a deck or card that is made or changed is not reported as failed when only its index could not follow.** Out of memory while the index was brought along left the deck or card done but answered an error, so a retry (the phone page's "try again") made it twice. The index is made anew when it is needed.

- **A book opened again, and every page turn, read the card a third as often.** A block of an EPUB, FictionBook or HTML book was read from the cache with three reads (index, header, text); now the record is read whole with one, and the index 32 entries at a time. A page of 25 blocks takes 25 reads instead of 81, and the whole of Alice 1,009 instead of 2,706. The files are unchanged.
- **The format plugin no longer reads the next block to tell whether a block is the last.** Each block asked of an EPUB, FictionBook or HTML book read and decoded its successor as well (a picture too); it now compares the block's end with the length of the section.
- **A book opened again no longer reads the directory of its zip.** The EPUB's archive is opened where it is first needed (converting a chapter, decoding a picture, following a link), not when the book opens: a warm open of Alice saves 51 reads, of Die Verwandlung 25. A zip that cannot be read fails with a message at that point, while the book reads on from its cache.

- **The weather widgets are offered in the Add picker again** (#228). *Weather now* and *Forecast* were declared with a preview of their settings group, which hides a widget from the picker; they are declared without one, as in Beta 3.

- **A widget's owner hears of the instances other plugins make** (#228, #244). The host never sent `event.widget_made`: an owner did not know an instance another plugin made of its widget and answered "not found" when it was measured or drawn, so picture-home's clock and "Start reading" cells, for example, stayed blank. Op 4 now lists a notice for the owner: the owner is told between callbacks before any drop of that instance, or inside the maker's callback right before it is first measured, placed, asked for targets or drawn. Each notice is told once. An owner that has not started yet keeps its notice, is asked to start and is told once it runs. A notice goes with its owner when the owner stops, and with its instance when the instance is dropped before the owner was told, in which case the drop is not told either. One notice waits per live instance at most; each is a single allocation of at most 256 bytes, freed once it is told.

- **The statistics widgets and the reading cat are offered in the Add picker again** (#228). They were declared as previews of their settings groups, which the Add picker leaves out; they are declared as plain widgets, as in Beta 3.

- **Widget pack widgets are offered in the Add picker again** (#228). The ported
  widgets were declared as previews of their settings groups, which the Add
  picker leaves out; they are now declared as plain widgets, as in Beta 3.

- **The to-do list's screen reacts again** (#228). The widget pack did not read the action that opens the list or the events of its screen (opened, shown, closed), so the list never opened or showed its items. It now lists those four events.

- The Wi-Fi settings row reads as in Beta 3 again (`Wi-Fi, Connected to <network>`, `Wi-Fi, Off`): it is told as a switch and no longer adds the word *On* before where the radio is.
- The Wi-Fi status item answers the host's status ask: its name shows in the status bar while a network is joined (before, the ask ended with an error in the background, at every refresh).
- The power plugin reads the device's power policy and identity into a reply buffer of the size the host requires (4,096 bytes); with 512 the host refused the call with `NoSpace` at start and for the status item.

- A request, socket or broadcast that a background work run opens now keeps the run going until the plugin has been told of its end, its last event delivered and the callback returned (`work.md` 2.3), and gives it back exactly once on every way it ends (at once when the plugin stopped before it was told): answered, cancelled, the plugin's network turned off, the run's 10-minute limit, the plugin stopping, or a transport that cannot start it. `core.net` and `core.sockets` tell the scheduler through `resource_opened` / `resource_closed`; a run that cannot be kept (`ERR_BUSY`) refuses the request or socket and holds nothing. A close that finds the scheduler in use is kept (room for 8, no allocation) and asked again at the next pass; one past that room is counted with the other skipped updates.

- **Installing, updating and removing plugins, and updating the firmware from the reader, work on the X4 family** (#273). The firmware gave the kernel no plugin or firmware host and offered no install or firmware features, so the store's install, update and remove, the plugin check and the update plugin's firmware install were all refused on the X4, X3, X4 Pro and X4 Classic, while the emulator accepted them. The firmware now gives the kernel the update machinery it already had (and, where the device has no second app partition, a host that refuses firmware jobs as "not here"), checks a plugin against exactly what its loader admits, and restarts after an installed update the way every other restart goes. No plugin job runs while an update is on trial or in safe mode (no code plugins load then).
- **A kept update restarts to host the linked plugins it did not host on trial**, and the kernel's firmware write is refused while a trial runs, so a failed trial keeps the firmware to go back to (#273).

- X4: registering a native plugin stays within the 2 KiB stack band its code
  runs over. `bind_native` builds the plugin's dispatch in a small generic
  step and stages and publishes the rest in outlined steps that share one
  borrowed staging list, and a declaration batch is checked one record at a
  time instead of decoding all eight on the stack (the batch is decoded
  twice: once whole, once record by record). On the X4 the largest of
  these frames is now 1,136 B (was 2,944 B for `PendingDeclarations::stage`
  and up to 3,984 B for `bind_native`); statuses and their order are unchanged:
  a decoding error anywhere in a batch still comes before any record's own
  status.

- **A second start of the firmware download no longer ends in a bare "Error"** (#232). While an update downloads, pressing the download again (the page opened again, an old offer) joins the running download and shows its progress bar, in English and German; a check of the card that ends meanwhile no longer offers the update again. The bar itself (steps of five percent, only its own strip redrawn) came with #302 and is also shown when the page is opened again during the download.

- **The touch lock works again on the X4 Pro, and it can always be unlocked without touch** (#234). *Lock touch* is back in the quick panel and the reader's menu (it did nothing on interface 7), takes every finger and pen, shows a lock in the status lines and closes the panel it was chosen in, so the page is back and the buttons turn pages. While touch is locked, holding the power button unlocks it at once (no menu to tap), and so does holding the home key; tapping twice and holding the second tap still works. The notice that says how to unlock now names the way that works (English and German; other languages say it in English until translated).

- The row of button hints no longer says *Select* where the button does something else or nothing: the store's source screen and the vocabulary's phone page hide the buttons that do nothing, the empty store list says *Try again* and the flash-card review says *Show answer*, then the answer in focus (#235).

- **Select text from a menu, the range legend and the page-limit notice** (#235). *More > Select text* starts choosing text again: a menu list opened over the reader's bar closed only itself, so the bar stayed and the action never reached the reader. The legend of a widened selection is on two lines and fits the X4 in English and German. The page-limit notice (a selection can take in at most 5 pages) is shown in the legend's strip, so Back takes it away with the selection; so are *This is the start (end) of the book*.

- **A power cut no longer costs card space** (#238, #269). When the reader lost power while it was writing or removing a file, parts of the card could stay marked as used although no file held them, so the free space shrank a little with every such cut until a computer checked the card. A file that is being written now holds its space from the first moment, and where the card must be changed in two steps the reader notes the step on the card first and finishes it at the next start. This needs no search of the card: a start reads nothing more unless a change was cut off. It holds for FAT32 cards (every card of 4 GB or more as sold); the few smaller FAT16 cards keep the small gaps they had.
- **Removing a large file writes the card far less often** (#238). Its space is now given back a block of the card's table at a time instead of one piece at a time, so removing a file of many megabytes takes a handful of writes where it took hundreds.

- Native plugins: `event.book_moved` is now delivered to subscribed plugins when the library moves a book (old and new path), so the reader's last book follows the move. A path over the record's 512 bytes drops the event and logs it once instead of telling a cut path.

- The first frame after the start (the home, drawn before the plugin that opens the last book hears `event.started`) no longer counts as a page turned towards *Full refresh*, so the fourth page of a book opened at the start is turned partially, as in Beta 3 (#243, todo 588).

- **Settings: a number's unit is said again** (#243). With the native settings a number with a unit showed its key, as in "Full refresh, 6 core.unit.pages"; it now says "6 pages" ("6 Seiten"), with the plural form for the number as in Beta 3.

- **The emulator's tests and the screen tools run without the first-time hints again** (#243). With the help plugin native, a fresh library showed the hints over the first screen, and they took the next button (power, the menu), as on interface 6; the tests and tools set `help.show=false` again, as they did then.

- **Moving the focus on the home screen draws only the two cells again** (#243), as on interface 6, not the whole page: a step takes a small part of a frame on the X4. The cells also say which one has the focus, for automation and reading aloud.

- **The tools list names Settings in the reader's language** (#243). Where no menu lists Settings, the native home screen added it under the core's catalog key (`core.action.settings`) instead of its word; it now shows the core's own name for it, "Settings" or "Einstellungen".

- **A native screen opened with params by anything but a native plugin gets them** (#243). A shell's script, an interface-6 plugin or the kernel opening a native screen with params (for example the choice of how the screen is turned, `settings.choose` with `id=core.rotation`) handed nothing over, so the screen opened empty. The host now hands the params over itself (op 16's caps; keys a plugin may not name, such as `core.*`, and a key given again are left out, the first kept; `text` is also the hand-over's text and counts against the texts' memory while the screen is open, as Beta 3 kept it; a text the device has no room for, on the X4 past about 2 KiB, is left out and the params kept; under the plugins' limits, so the host's own dialogs always have their room), freed when the screen closes; a native plugin's own `open` is unchanged.

- Native plugins: saving the reading place through the library (`set_position`) no longer answers "unsupported" after the place was written, so the reader no longer says the place could not be saved on page turns.

- An essential built-in native plugin (plugin manager, settings, menu) that crashed is held back for that start only and stays switched on in `paperpaw.ini` (`NativeDecision::CrashedOnce`): switched off for good it could be turned on again only by editing the file on a PC. Any other crashed plugin, built-in or from the card, is switched off for good.

- Native plugins' power, Wi-Fi and firmware commands now reach the device: every callback's command stage is admitted with the plugin's features, the kernel's Wi-Fi request table and, where wired, the firmware table, and the kernel applies them in their place among the batch's other commands. Before, they were refused as unsupported, so the native Wi-Fi page could not open (#2384).

- The device's setting defaults (e.g. `keyboard.size` of the X4 and X4 Pro) now reach the settings a native plugin declares: they were dropped with an "unknown setting" warning because the kernel is built before the plugin is hosted. A default for a setting nobody has declared yet is kept and set as the plugin is published; a value the user set still wins.

- **A native screen under an overlay is drawn** (#243). The page below the reader's menu bar, a panel,
  a lookup or a dialog drew blank: the host drew a native screen only when it was on top. As
  `ui.md` §1.2 says, a screen with only overlays above it is drawn as if on top, the overlays after it;
  input still reaches the top screen only. Under another plugin's overlay a screen that is secret now
  (a run-time policy too) or on a stack with a guard (a lock's PIN dialog included) stays blank.

- **The picture home screen names Settings in the reader's language** (#243). Where no menu lists Settings, its tools list showed the core's catalog key (`core.action.settings`); it now shows the core's own name for it, "Settings" or "Einstellungen".

- **The kernel's placeholder and safe-mode screen are drawn whole on devices that draw in bands** (#243). The screen shown for a slot without a plugin, which is also all a safe-mode start shows on the X4, took the band being drawn for the screen: its frame and text were squeezed into the first band and the safe-mode text was repeated in each. It now lays itself out in the screen's room.

- A native screen that says the policy it already has (the reader does at every show) no longer draws the screen a second time: the host redraws only for a policy that changes (#243, Senior R1).

- **Reading aloud speaks in the native reader** (#243, #2426). The reader knew the page on show by the place it was asked for, the sentences by where the page's layout starts; where they differ (a text file whose book starts after a header, a page after a section's end) reading aloud said it failed and spoke nothing. The page on show now goes by its layout's start when reading aloud starts and after each sentence.
- **Read aloud right after the start** (#243). *Read aloud* pressed while the book read last is still opening is kept and reads that book once it is on show, as in Beta 3, which opened the book before any input. Another book, another screen or action first, a failed open or more than 10 seconds drop it.

- **Read aloud at the start reads the book reopened in the native reader again** (#243). *Read aloud* asked at the start of the device could reach the reader before the start itself, before the book read last was asked to open; it was dropped, so nothing was said. It is now kept while no book is on show and read for the book reopened. *Read aloud* asked while a book opened for the first time is still being prepared starts as soon as the book is open and its reading screen is on top (a menu over it keeps it until the reading screen shows again).

- **A forward turn on the last page of a book stays there** (#243). The native reader turned on to an empty page past the book's end; it now lays out the page after first and keeps the last one when nothing follows.

- **A book without room to open says so in the native reader** (#243). When there was not enough memory to open a book, the reader ended the screen's opening with an error and kept no view, so the screen stayed empty. It now shows "no memory" in a few words, as Beta 3 did, and the run goes on.

- **Books opened for the first time open in the native reader** (#243, #2443). A book whose format prepares it first (DOCX, MOBI) was closed at once while the reader said "Opening the book for the first time", so its preparation never ran and the book never opened. The reader now keeps the document while it is prepared, opens the book when the host says it is ready, says it could not when it failed, and gives the document back once, when the screen closes, also when the reader leaves while it is still prepared. A reading screen shown again asks the state itself, so a lost hint never leaves it waiting.

- **A book opened while the native reader shows one opens** (#243). The reader opened the second book without giving back the first one's document, so the host refused it and the reader said the book could not be opened; everything after landed on that message (no bookmarks, no page turns). The same book now stays open where it was; another book opens after the reading screen below gives its document back, and that screen opens its book again when it is on top again. At the start, the book read last is not opened again over a book already on show.
- **Bookmarks, notes and the reading place are saved by the native reader** (#243). Its metadata lacked the `uses.library` feature, so the host refused every library call and the reader said memory may be full.
- **The native reader saves where you are, and pages turn by themselves again** (#243). Its reply buffers were smaller than the host requires for the call (`set_position` 4,096 bytes; the timer's `declare`/`cancel` 64), so the host refused them before they ran: every book reopened at its start ("Where you are in the book could not be saved"), and the timer of the pages that turn by themselves was never set.

- The native reader says its pages are best in grays where smooth text is on (screen policy bit 9, set when the screen is shown and when the setting changes), so a panel that shows more grays with a refresh of its own shows them as in Beta 3 (#243, todo 588).

- **The native reader is known as a reading app** (#243). It declares `reader.app` with its reading screen, so the host counts the books it opens: reading progress, "Continue reading" and the book events work again.

- **Read aloud speaks a book without a language in the reader's language in the native reader** (#243). A book that names no language (a text file) was read aloud without a language tag, so the device's default voice was used. As in Beta 3, the book now takes the language of the user interface, so a German reader hears a German voice.

- **A page under an overlay can redraw** (#243). `command.redraw` and `command.redraw_area` of a
  native screen that has only overlays above it (the reader's page under its menu bar, a picture
  finishing) were refused or dropped, and a redraw hit the top screen's region. They now act on
  that screen's own region, with the overlays drawn again over it (a dimming one too); a full
  screen or a guard above it still gives `ERR_STATE`, and a queued redraw is dropped then.

- **The book read last opens in the first frame** (#243). The start drew the home screen first and the book one pass later, a flash and an extra full-screen refresh that Beta 3 did not have. The start now settles its first passes before it draws, so the book is the first frame (home only where there is no last book).

- **The native transfer plugin starts on a desktop** (#243). It no longer requires `uses.install`, which only a shell that can install plugins offers; without it Wi-Fi and cable transfers of books and files work as before and a plugin sent from the computer is refused (*not allowed*), as in Beta 3 where the page offers no plugin install.

- **The native update plugin starts on a desktop** (#243). It no longer requires `uses.firmware`, which only a device whose firmware PaperPaw is offers; without it the plugin stays silent and hides its menu items, as in Beta 3.

- **The web emulator starts without the first-time hints again** (#243). With the help plugin native, the hints opened over the first screen of the web emulator; it starts with `help.show = false` again, as on interface 6.

- **A card folder left by Beta 3 never stands in for a plugin that is built in now** (#244). A code plugin's folder on the card whose id is a built-in plugin's (`widgets`, `news`, `aids`, … on the X4, where Beta 3 had them on the card) was looked at by its file first: a Beta 3 file was listed as *built for an older PaperPaw* and the folder's old translations were loaded; switched off, never installed, or on a start without code plugins it was loaded as a plugin switched off; safe mode switched its id off, which is the built-in plugin's switch. The kernel now refuses such a folder for its id before anything else (`KernelBuilder::built_in_ids`, which the X4 firmware sets): nothing of it is loaded, the folder is left alone, and the row is the built-in plugin's.
- **Code fetched for a firmware update is installed where the firmware starts again for it** (#244). The X4 firmware asked its loader what it reads to decide the restart once an update is kept, but the kernel read it from the plugin jobs' host, which that firmware does not set: a plugin fetched for the update would have been started again for and then left where it was. Both now ask the loader (`CodeLoader::reads`). No release fetched interface-7 plugins for an update yet.

- **`paperpaw-dev build` checks what it is given before it builds** (#244): a package name that is no plugin id is refused before any folder is named or removed by it; a `paperpaw-sign` of another PaperPaw release is refused with how to install the right one (`paperpaw-sign --version` is new); and `CARGO_ENCODED_RUSTFLAGS` no longer reaches the plugin's build.

- **`paperpaw-dev metadata` refuses a list that is cut off or broken** (#244): a `native/metadata.json` that is not one complete JSON value ended in a shorter record without a word; now the command says so and fails.

- **`paperpaw-dev package` takes a plugin library with a heap word, as the emulator does** (#244): the check and the emulator's loader read one list of what a library may export (`PAPERPAW_ENTRY`, `PAPERPAW_METADATA`, `PAPERPAW_HEAP`). A heap word that overlaps the entry record or the metadata is refused.

- **The native Wi-Fi plugin can be made** (#244). The host refused a registration for `event.wifi_scanned` (92), `event.network_wanted` (94) and `event.network_idle` (95), so the plugin failed at `make` with Factory(-1). They register now, with `event.plugins_requested` (77) and `event.plugin_answered` (78) of the registry; nothing raises 92, 94 and 95 until the radio's own events are wired.

- The hosted construction tests of the DOCX, MOBI and image format plugins expect construction to succeed now that the host admits format declarations (the MOBI one also compiles under the `linked` feature again).

- **A list of installed code that cannot be read loads no code plugin.** Before, every installed plugin was then taken for one never installed and ran whatever file lay in its folder, unchecked (Decision 205). A card without the list is as before: nothing installed.

- **A start once an update is kept sees interface-7 card plugins** (#244). An update on trial starts without code plugins; once it is kept the firmware asks whether a start with them would load any, and starts again if so. That question (`paperpaw_core::code_to_load`, now with what the loader reads of interface-7 files) and the list of plugins fetched for an update only knew plugin files of the format before interface 7, so a card whose code plugins are all interface 7 stayed without them until some later start.
- **The web flasher's bundle takes interface-7 code plugins** (#244). A store code plugin chosen in the web flasher together with the firmware was refused as damaged when its file is interface 7 (`paperpaw_app::flasher::prepare` read the older header only). A code file of either format is now checked whole (`paperpaw_sdk::native::check_code`, kernel feature). Data plugins were not affected.

- **Native plugins hear the radio's events** (#244). `event.wifi_scanned` (92, with the number of networks found) reaches every plugin that subscribed to it; `event.network_wanted` (94) and `event.network_idle` (95) reach the network manager (`core.network-manager`) alone, and nobody where none is chosen. Each is a coalesced hint.

- The Mobipocket format, Mail and the online catalogues (OPDS) pack as plugin files for the X4 again: each had a function with a stack frame of 2 KiB or more, which the X4 refuses. A picture's header is read into memory asked for, not onto the stack, and the sorts of mail rows and catalogue entries use the shared sorter (the same order).

- A delete or import of the library no longer stalls on a part the card refuses for good (a damaged recent-books or favourites list): that part is left and never repeated, the rest of the book is still carried and the job ends; a fault that may pass is still retried. The delete's entry now also records which books are done with, so a restart after a cut never looks again at a path whose file is gone, even when the card reuses the identity (FAT clusters).

- **The busy mark cannot stay on the X4's screen when memory runs short** (#248). After slow work the panel's two picture memories are filled again with what was shown; where the little memory that takes (9.6 KB) is refused, the shell now shows the next frame whole (as after a paced frame) instead of only logging that the mark was not kept, which could leave a ghost of the mark at the next quick refresh.

- **A list of settings builds only the rows it answers** (#248, #228). The registry service answers a list of settings in pages of 32 rows. For each page it built the row of every setting in the list twice, once to count it and once to reach the page: its title looked up in the catalog, its value read, what its plugin declares of it found in the plugin's image. With some hundred settings and the settings screen's fifty pages per opening, that was most of the time a settings page took to open. A setting is now counted and passed by its id and group alone, and its row is built once, where a page takes it.

- Calibre wireless: the session's step lists are joined with room made fallibly, so a refused allocation is `NoMemory` and no longer aborts the plugin.

- **A page's picture is read once for the frame that draws it** (#250). On devices that draw a frame in bands (the X4 family), every band dithered a plugin's picture from its first row again, reading and working through all rows above it: a cover over four bands was read 2.5 times, about 4 s on the X4 for a full-page cover, on every visit of that page. The frame now keeps the dithering between its bands (4 bytes a column, at most 4 KiB, freed when the frame ends), so each band goes on where the one above ended. The pixels are the same; a band drawn alone or again starts from the first row as before.

- **A cover or picture finds room more often when memory is short** (#250). Unpacking a compressed picture asks for its largest piece of memory (32 KiB) first; asked for last, it could be refused although enough was free, and the picture's box (on a cover page, the whole page) stayed blank until the book was opened again. A picture that still finds no room no longer leaves an empty `image.tmp` in the book's cache folder, and no such file is made and removed where the copy could not find the memory either.

- **A book's pictures are decoded no larger than the screen again** (#250). A reader that names no picture size got pictures of up to 1,600 pixels a side, where Beta 3 decoded them for the screen: a cover of 896 × 1,400 pixels was written to the card as 1.25 MB on the X4 while the book opened, and read back at that size for every page that shows it. The size is now the screen's long side (800 pixels on the X4 family, at most 1,600 on larger screens), so that cover is 512 × 800 and 0.41 MB. It is the long side, so turning the screen converts nothing again. Books opened before are converted once more at their next open; a reader that names a size gets that size as before.

- **A book's pictures are decoded when a page shows them, not when the book is read** (#250). Opening a book for the first time and counting its pages read every block, and each picture block decoded its picture and wrote it to the card, shown or not: a first open of a picture-heavy EPUB took most of its time there, and the page count went on decoding in the background. A picture that is not decoded yet now goes out at the size it will have, so the pages come out the same, and it is decoded when the reader asks for it for the page on show. Caches made before stay valid; books of whole pages (comics) are read as before.

- **The reading calendar shows its day numbers on the X4** (#250). Where the host gives no ink for a text (the X4 until `text.ink` is wired there), the statistics calendar took its widest number as never fitting and drew every day as a dot. The number's height is now the line's ascent there.

- **Less X4 stack under a native plugin's event** (#250). The kernel's event delivery keeps the interface-6 handlers, the guard's screens, the book moves, the work moments and the screens' contexts in frames of their own, so they are off the stack while a native plugin hears the event, the route of a nested plugin callback (`stage.run`, `ui.compose`) on the X4.

- **Less X4 stack under a native plugin's callback during a tick** (#250). The kernel's tick, its
  end-of-call work, the dialog pump and the command loop keep their locals in frames of their own,
  so they are off the stack while a plugin's callback runs below them (the route of a nested
  plugin callback on the X4): about 3.9 KB less from `Kernel::tick` to the plugin call.

- **The *Pictures* reading aid (caption or hide) now takes effect on the X4** (#250, #257). With the gate closed the reader never asked the aids for their rules, so a book's pictures stayed whatever the setting said; with *Caption* a picture is now a centred `[Picture: description]` line, with *Hide* it is left out, as in Beta 3.
- **The reading settings' pages show the sample text above their rows on the X4** (the *Preview*, as in Beta 3).

- **Plugin screens are drawn on the X4 again** (#250). On the X4 the home screen, the menu, the settings, the reader's contents, the store and the battery screen stayed white with the native plugins: the X4's host does not answer `text.ink` yet, and `Font::ink` gave that as an error, which ended the whole draw of every screen that centres text or an icon by its ink. `Font::ink` now gives `None` there, as for text without ink, and the text is placed by the font's metrics.

- **The X4's start and main loop need less stack** (#250). A shell can build the kernel on the heap (`KernelBuilder::build_boxed`), so no copy of it (about 6 KB there) is on the stack while the start reads the card. The bounded steps of a plugin install that a stop asks for run at the end of the kernel's pass, before its effects, instead of under the command that stopped it. A signed file's check keeps its state on the heap, and a few dispatch steps run in calls of their own. Where the heap has no room for the kernel, `build_boxed` returns `KernelError::NoMemory` instead of aborting. A signed plugin file whose check finds no memory is reported as a memory problem (`Problem::Other(1)`, `Refusal::Other(1)`), not as a bad signature, and the start keeps the plugin's install marks for the next start.

- **A picture asked for larger than 1,600 pixels is made at 1,600** (#252). `core.document` op 16 (`doc_image_ask`) clamps a `side` above 1,600 instead of refusing it, as `document.md` section 5 and `image_prepare` already do.

- **A page's large pictures, drawn right after its text, no longer count as a page turned**
  (#252). Their frame could be the one made the periodic full refresh: the whole screen
  flashed for the picture, and again on the next page turn. Now it refreshes the pictures'
  box only, however large, as small pictures always did.

- **Pictures that were not ready when a page was laid out now show in the native reader** (#252). Before the page is drawn, the reader asks again for each picture that has no image yet or lost it, up to eight per page, so DOCX and MOBI pictures and pictures dropped from memory appear instead of an empty box or alt text.

- **Recently opened books show at once after a start, also when the library's list was made before they came** (#253). A book opened lately that the list of the library did not know yet (it came after the list was made) showed only when the walk at the start ended. It is now added to the end of the list at the start, a line each, before the walk: the list is not written anew, and a start where nothing is missing reads none of it. A cut off while it is added leaves the list readable.

- The reader's list of bookmarks and highlights shows a long text whole, wrapped as in Beta 3, instead of cutting it with "…".

- The flash cards' card-files screen (the update's result, a save's result) is drawn again on the native plugin: it was blank, and the update never stepped, because the screen was never noted as the one on top and the update's work step was declared with no room for the host's reply.
- The update from the card's files takes its steps together (up to 32 a turn, 32 KiB of files read at most) instead of one a second, so a long update no longer takes minutes.

- After the library looked for books, the titles and authors of EPUB, DOCX and MOBI books are read right away on an idle device. Before, reading began only with the next key press or, on the X4, with the next battery reading up to a minute later, and not at all if the device went to sleep first (#257).
- While the titles and authors of books are read in the background, the device rests between the steps again. Before, it could stay busy without a pause for as long as a book was being prepared, which cost battery (#257).

- A dialog's buttons are stacked when a label would be cut in the room the button leaves it: the first-start hints on the X4 showed *No mor…* instead of *No more hints*, in English and German.

- Dictionary: the lookup panel offers *Online ›* in its header again (tap it, or press the menu button) where a screen for online lookups, such as Wikipedia's, is installed: it closes the panel and opens that screen for the same word and language.

- Dictionary: the lookup panel says "Preparing … N %" while the indexes of a new dictionary are made, then shows the word; a dictionary that could not be indexed is not tried again at every start until its files change, and the indexes of dictionaries removed from the folder are cleaned up.

- Dictionary: the lookup panel shows the definitions of your own StarDict dictionaries whole (up to what a lookup reads, 32 KiB), not cut at 2,048 bytes, and says "No dictionaries in …" where the folder holds none.

- Emulator (`--wifi online`): after Wi-Fi is switched off and on again, the native Wi-Fi plugin finds the workshop's network again and rejoins it, as the interface-6 radio did.

- **A flash card list saves again** (#257). Saving a list on the card (Vocabulary's *SD card* lists) ended with "The file could not be written": the save renames two files in a row, and the first rename started the library's move job, which made the second one wait. The host now moves a single file that is no book (by its name: no format reads it, nor its `.bak`) with the card's plain rename, without the library's journal and job; folders and books move as before. No interface change.

- The focus on a tall row (a title over two lines, a note) is a rounded rectangle again, as in Beta 3, instead of a pill whose ends cut into the text.

- The first-time hint over Home and the "Show hints again" message can be closed with a button again (Select, Back, Left, or holding Select). They swallowed every button press before the device could turn it into its action, so on a new card only a tap closed the hint, and the message could only be left with the power button.

- **Notices show with native plugins** (#257). Messages such as "Saved" or an error (`command.notify`, the kernel's own) vanished when the native menu replaced the interface-6 one, because only the latter filled the notice slot. The host now has its own notice screen, `core.notice-box` (Beta 3's look: a dark rounded box above the bottom, at most three lines, gone after `core.notice-seconds`), bound to `core.notice` as a fallback; a plugin's notice screen still wins.

- **Native screens are not real-time screens** (#257, #2671). A native screen takes every key only so that its plugin gets them; the kernel took that for a real-time screen. While one was on top, background work without a fixed time (news, wallabag and readwise syncs) was held, holding *Back* for 2 s or a finger in the top left corner left the screen, and the "hold Back to leave" notice showed. One rule now decides it: raw keys and not native. Holding *Back* for 2 s or a finger in the corner therefore no longer leaves a native screen; a plain *Back* (the plugin closes its screen) and holding Power for 8 s stay the ways out.

- **Mark vocabulary is back in the reader's More menu** (#257). The switch that marks the words you are learning in books (Beta 3's last row before Settings) was missing; the vocabulary plugin puts it there again.

- **A number changed with the buttons shows its value at once again** (#257). The host refused
  the preview commands of interface-7 plugins (`command.preview_setting`, `command.preview_end`),
  so the Settings screen showed the new value only once it was chosen. They are now accepted as
  `docs/abi/registry.md` 4.4 describes; a preview also ends when the device goes to sleep.

- **The quick panel's button labels are no longer cut** (#257). A label a little wider than its button, such as "Refresh the screen" on the X4 or "On its other side" on larger screens, now has room for the whole text: the button keeps less margin at its sides, and where the font can be drawn smaller, the label is drawn smaller as in Beta 3, before anything is cut with "…".

- **The native reader shows a page's bookmark again** (#257). A page with a bookmark has Beta 3's black ribbon at the top right of the text, and with *Bookmark corner* on, a tap on the top right corner sets or removes it. The book's bookmarks are read once when it opens, not at every turn.

- **A colour screen shows a book's pictures and cover in colour again** (#257). The native reader opened books without asking for colour pictures, so the covers of the Kobo colour profile came out in grays; it now asks when the display shows colour.

- **Reader: a book that opens on a picture draws its page once** (#257). The native reader drew the opening page a second time before its picture, and asked for the picture's box twice, so the cover came in an extra full-screen refresh. Now the page is drawn once and only the picture's box follows, as on a page turn.

- **Go to page and Go to a percentage work in the native reader again** (#257). The number is typed on the keyboard as in Beta 3: a page from 1 to the pages counted so far (the prompt says how many, and whether they are still being counted), or a percentage of the book, where 100 % is the last page. The page left can be turned back to.

- Words are broken at the end of a line again where Settings, Reading has hyphenation on (the default), with the fewest letters kept before and after a break as set there, as in Beta 3. The break places of the words met are kept for the book (about 1 KiB while it is open) so a page is not slower. A book in a language with no table is set whole. English is hyphenated on a fresh install; German needs its pack from the store.

- Letter spacing, kerning and ligatures (Settings, Display) work again in the reader: the book is laid out and drawn with them, as in Beta 3. Kerning and ligatures are on by default, so lines break as they did in Beta 3. The font is asked for the pairs it kerns once for each book (about 7 KiB while it is open, 2 KiB more for a book with more than 96 different characters); if there is no room the reader goes on without the table, only slower. Up to 96 letters are kerned at once; in a book of another script that starts in Latin (a Cyrillic novel with a Latin title page) the new letters are kerned from the third page on, and in a script with more letters than that (Vietnamese) the rarest are set without kerning. Page counts made before are laid again once. The tables are let go while another screen covers the book (the store over a book) and made again on the next page.

- **Reader: "could not be saved" only says the memory may be full when it is** (#257). Opening the last book at the start, before the library had listed it, showed "Where you are in the book could not be saved. The memory may be full." The reader now tries such a place again at the next save without a notice, and says why a place was not saved: a lack of memory, a full memory card, or just that it was not saved. A bookmark, highlight or note that could not be saved no longer blames the memory.

- **The native reader sets paragraphs as Beta 3 did** (#257). Paragraph indent and its size, the space between paragraphs, the lines kept together at the top and the bottom of a page, balanced line breaking and the switches for the book's own styles (alignment, indents, sizes, spacing, fonts, margins, line height) are followed again; a change lays the page out anew and counts its pages again.

- **Reader: the table of contents of a book without one lists its chapters again** (#257). Where a book's own table has at most one entry, the native reader finds the headings in the book, shows how far it is, keeps them for the next time, and lists them, as Beta 3 did. A very long table keeps to a fixed amount of memory: the list then shows its first chapters and says that the rest is left out.

- Turning a page in the reader asks the font for far fewer letter widths: they are kept for the whole book (about 2 KiB while it is open) instead of being asked again for every page. In the emulator's 30 page turns of a sample book the font was asked 9 times a turn on average (41 at most), against 137 (224 at most). If there is no room for the table the reader goes on without it.

- **A native plugin's `setting.extra` reaches the settings page** (todo 541, #257). `core.registry` read only a setting spec's own extra, and a native plugin's extra stays in the declaration store. Its int settings therefore lost the slider flag, and op 2 lost the unit, step, section, follows, shown-if and editor. These are now read from the store when a row is asked for, and nothing extra stays resident.

- **A native plugin's "shown if" works on the settings page** (#257). `core.registry` ignored a native plugin's declared `shown_if`, so a dependent setting always showed (`reg_setting` bit 9), and the governing setting lacked bit 10, so the page did not re-read when it changed. Sleep and picture-home declare such settings. Both bits now come from the declaration store, as in Beta 3.

- **Removing a widget or a page forgets what it set, and "Reset to defaults" works for
  interface-7 plugins** (#257). The host refused `command.reset_setting`. It now resets one
  setting, what a placement set, a group, or the submitter's own settings, as
  `docs/abi/registry.md` 4.1 describes. The two resets that need the reader's consent (another
  plugin's settings, every setting) are refused with `ERR_UNSUPPORTED` until the host can ask.

- The flash cards' menu rows show their long titles and notes whole, as Beta 3 did: a title wraps to two lines and a note to four, instead of being cut with "…". Lists have a new `whole` row flag for it.

- **Rules for a time of day follow the device's time zone again** (#257). With the time zone on "automatic" (the default), the native rules plugin counted the time in UTC, so a rule for 7:00 ran at the wrong hour outside UTC; it now uses the system's zone, as Beta 3 did.

- After the language is switched on an open Settings page, all rows say their words in the new language at once, not only after the page is opened again.

- The transfer screen says *No Wi-Fi* when there is no network, instead of *could not start*: the host refused the screen's request for its access key, which was asked for in too small a buffer.

- **Native clocks follow the device's time zone again** (#257). With the time zone on "automatic" (the default), the clocks of the native home, widgets, lock screen and weather showed UTC, two hours off on a device at +2; they now use the system's zone, as Beta 3 did.

- **The editor of widget lists names the widgets of interface-7 plugins** (#257). The sleep
  screen's widget list showed raw ids (`home.clock`) and offered only spaces to add, since the
  kernel's editor knew the widgets of interface-6 plugins only. It now reads the names and kinds
  of the widgets native plugins declare. No interface change (`shape.txt`: one private field).

- Wi-Fi: the status line shows the Wi-Fi sign again while a network is joined (it showed the word "Wi-Fi"), and nothing otherwise.

- **A book title in letters the font lacks no longer shows as question marks on Home** (#257). Without the pack of letters (Cyrillic on the X4 before the script pack is installed) the continue card, the recent shelf and the cover frames show the title with a "…" for each run of missing letters, never "??????"; with the pack the title is drawn as before.
- **The update server is a row of the Updates settings again** (#257). *Update server* was in the developers' settings, shown only with the developer switch on; it is now in the Updates group, as in the update page's rows, on devices that update their firmware.
- **Hyphenation costs far fewer card reads per page turn** (#257). A small hyphenation table (English and the other tables of up to 8,192 patterns) keeps a 4.9 KiB filter of its patterns, built when the table is opened, so a word's lookups no longer read a block of the card for each key that is no pattern; the reader keeps the break places of 128 words instead of 64 (2 KiB, was 1 KiB). Reading Alice with hyphenation on, the emulator counts 63 block reads per page turn instead of 328.

- `paperpaw-dev lint` compares the translations of the plugins built into the app again and
  writes their texts in the pseudo-language, outside this repository too: their messages by
  key are built into the tool (#258).

- Store: while a plugin downloads, its page shows how many of its files are here ("2 / 4 files", "2 / 4 Dateien") instead of file counts written as bytes ("2 B / 4 B") (#263).

- **Highlights stay on their own page after a turn back** (#265). After a turn
  back (or a jump) the reader marked the page, and chose its words, by the
  words of the page it had left: a highlight was missing on its own page, could
  stand on another page over other text, and a word chosen there was not the
  word shown. The page's words are now made again for every page shown.
- **Short highlights of an earlier version are drawn again** (#265). A highlight
  of a word or two ("Om", "the") that a Beta 3 card or another conversion of the
  book wrote was never drawn. It is drawn where it covers exactly its words.
- **The selection's actions do what they say** (#265). `reader.selection.highlight`,
  `.note`, `.look-up`, `.copy`, `.extend` and `.cancel` (a key bound to one, a
  remote) were answered and did nothing; they now do what the bar's button does.
- **A highlight is dots again on black and white screens** (#265). Its light
  gray was drawn as rows that ran through the letters; it is a pattern of dots as
  in Beta 3, as dark as before. A host without the pattern fill draws rows.

- **Bookmarks and highlights of an earlier version are shown again** (#265). A card written by Beta 3 (or by another conversion of the book) keeps its bookmarks and highlights: the reader draws a highlight where the page has its words, even when the place is a character or two off or a word is broken at a line's end, and a bookmark where its place is. A mark whose text it cannot find is not drawn, but stays in the notes list and in its file. Nothing is written to the card to do this.

- **Soft hyphens and a few characters at a page's end no longer hide a mark** (#265). Where the other conversion hyphenated a word and this one did not (or the other way round), the words still match; a bookmark or highlight whose text starts in the last few characters of a page is found by those.

- **A book keeps its reading place after an update** (#265). A place saved for another conversion of the book, for example by Beta 3 or with another picture size, was dropped and the book opened at its start. The reader now takes the place if its progress still fits the stored one, else the place that progress is; a place it cannot check opens at the start. Nothing is written to the card when the book opens.

- **The power menu does what it says again** (#266). Choosing *Sleep*, *Switch off* or *Restart* in the menu the held power button opens did nothing: the power plugin ignored the dialog's answer (it came under a token the plugin did not look at), and *Switch off* then failed on its second opening of the interface service. Sleep and restart work after the first fix, switch off after the second. The power settings open after the menu was used. The emulator's X4 profiles offer *Switch off* (and show it as a switch-off) as the real X4 does.

- An action sent by a remote command or a test script (not a key press) closes a dialog before the plugin hears the dialog's answer, as a key press does (#266). Before, a screen the plugin opened from the answer was closed in the dialog's place and the dialog stayed on top.

- **Home button navigation** (#267). Widgets outside the changed drawing band are skipped, avoiding repeated layout and host calls during selection moves.
- **Home on short landscape layouts** (#267). The empty Continue card keeps its message inside its cell, so drawing in bands matches a whole-frame draw.

- Show Home book covers when they become ready, without waiting for the next minute tick (#267).

- **Home's first frame after "Back" from a book shows that book under "Continue reading"** (#267). Where the library had no catalog yet (the book was opened before Home ever showed), Home first said "Start reading" and showed the book only once the walk of the library had begun, a moment later. The book closed now stands for the catalog until the walk, which still shows the books as it finds them.

- On a screen that turns, the home screen shows the page that has the first item of the page before (Beta 3 did), not the same page number.

- A question a plugin asks from an action run by the shell (`Kernel::run_action`) now opens its dialog at once, not only at the next unrelated wake-up (#267).

- **Plugin icons** (#268). Restored the Battery entry and icon in Tools and the screen lock icon references lost during the interface-7 port.

- **Card plugin translations** (#268). Include source catalogs in card packages so Battery and other installed plugins show translated labels in English and German.

- **The groups in Settings show their icons again** (#268). Since the native Settings, every group row was drawn without an icon: the registry's reply for setting groups carried none. `service.reg_group` gains the optional `icon` (the name in the icon font, at most 64 bytes; appended, so a reader that does not know it skips it), `RegistryGroup::icon()` reads it, and Settings shows it. A group nobody declared an icon for gets `puzzle`, as in Beta 3.

- **Highlights are drawn on the page again** (#270). The reader read the look of a highlight (and of the search hit) from the start of the host's reply instead of from the value in it, found no look, and drew no highlight at all. It now reads the value.

- **A packed update checks and installs on the X4 with Wi-Fi on** (#273). Unpacking takes about 52 KB of the heap at its start (the window, the decoder, two buffers), which the Wi-Fi radio's buffers and idle plugins can hold, so "Search online, download, install" ended with "could not be installed". The check now asks the main loop for room when it finds none: idle plugins are parked, the radio goes down (the file is downloaded already), and the check begins once more. Wi-Fi stays off afterwards until it is switched on again. Where there is still no room the update says it could not be checked or installed and to restart the reader if it is short of memory (the kernel keeps its own reason for this, none is added to interface 7). The heap is logged where a check begins and where room is made.

- A plugin folder on the card with data only (no code) that uses the id of a plugin built into the firmware still loads, its data merged with the built-in plugin's, and the plugin list shows the one row of the built-in plugin for it; before, the list kept the folder's row and had none for the built-in plugin (#274).

- **"Literata, larger sizes" is version 1.1.1** (#275). Its description changed in 1.1.0 (it says Literata must be the reading font) without a new version, so the file in the store differed from the one published in Beta 1 under the same number. The version now tells the two apart; the fonts are unchanged.

- **The X4 Pro and the X4 Classic image has one data segment again** (#275). The plugin list after the application descriptor is padded so that no gap is left before the read-only data, and the link fails if one is. With the gap the image had two data segments, of which the bootloader maps the last only, and the firmware read its own plugin list from the wrong flash page. `paperpaw-sign check-image` and the new `check-app` fail an image with two, and CI runs the second on every device's image. The X4 Pro is held to its own slot, not CrossPoint's (Decision 184).

- Reading-progress sync (kosync, KOReader sync) works for a book the Library has not listed yet, such as one opened before the first look at the Library or copied on since: the library answers its book key and fingerprint from the file.

- **A plugin's own pictures show again** (#277). A plugin installed from the store that asked for a picture it ships (reading goal's cats) was told the card failed: the picture pump did not know the plugin's install folder (root 2 of `command.image_prepare`). It now reads `plugins/<id>/<path>` for the plugin that asks, and only for that one; a plugin built into the firmware has no such folder, so its request ends as "not found".

- **The desktop emulator has the store's Beta 3 plugins again** (#277). Statistics, Battery, Keys, Reading goal, All notes, Weather, KOReader, Reading profiles, Customise menus, Rules, Wireless library, End of book, Device check and Translate were not in the emulator and its store did not list them. They are built into the emulator now, shipped off: *Switch on* in the store (it applies after a restart) is the install, switching off in the plugin manager the removal. The X4 family is not affected: it installs the files.

- Restore KOReader progress sync through the native library and document services (#277).
- Locate remote positions by XPointer or fall back to their percentage, including formats without XPointers (#277).
- A book opened at a place from another screen (sync, notes, vocabulary) keeps that place even when no page is turned (#277).

- **Settings and the setup show a language by its name again** (#277). Settings > System > Language and the language row of "Set up PaperPaw" showed the code (`en`, `de`) where Beta 3 showed `English` and `Deutsch`. They show each language's own name again, an installed language pack's included, and offer the languages to choose from.

- Fixed light menu and list headers on monochrome displays: their inset separator is visible again, as in Beta 3.

- The Linux emulator and tools of a release start on Debian 12 and Ubuntu 22.04 again: they are built for glibc 2.35 (they asked for 2.39), and the release checks it.

- **The *Lock touch* switch is no longer shown on devices without a touch screen** (#277). The X4 family showed it on the lock's settings page although it does nothing there. The settings page now also honours the device needs that natively declared plugins give a setting, as it did for the kernel's own plugins.

- **"Look for new books" is back** (#277, #286, Beta 3). The library's menu offers it again: it makes the catalog anew, so books copied onto the card while PaperPaw runs show up without a restart. Pressing it while the search runs joins that search instead of starting it over and says it is still looking.

- **A folder of pictures is one book again** (#277). Opening a picture from the library opens it with the pictures after it in its folder as the pages, in name order, as in Beta 3; before, each picture was a book of one page. The picture format reads its book's folder through the files service, a page of names at a time. `Files::list` and `Files::stat` take the book root (`Root::Book`, wire root 3) as `docs/abi/files.md` section 1.1 allows: inside a `core.format` call of a folder or siblings document a format lists the folder and stats its entries; the host answers Unsupported outside one, and every changing call still refuses the book root before any call.

- **A plugin can switch another plugin on or off again** (#277). The command `command.set_plugin_enabled` was refused with "unsupported" by the host, so the store's *Switch on* for a built-in plugin (and the first-run offer) saved nothing. The host now stages it for the next start as the settings editor does; `core` and a plugin switching itself off are refused with `ERR_DENIED` (`docs/abi/registry.md` 4.2).

- Fixed Appearance > List sections > Cards: native lists again group their rows on rounded cards, with inset focus and open edges where a page divides a section, as in Beta 3.

- **List values in Settings** (#277). Every list setting, including keyboard layouts, resolves catalogue keys to translated names and shows “None” for an empty list in English and German. Items without a catalogue entry keep their internal identifier, so some widget and status lists still mix names and identifiers: sleep widgets can show “home.date, Continue reading”, and a Home footer can show “Time, reader.status.page”.

- Restore battery to the store catalogue for interface-7 X4 plugin files (#277).

- Restore calibre to the store catalogue for interface-7 X4 plugin files (#277).

- Restore diagnostics to the store catalogue for interface-7 X4 plugin files (#277).

- Restore end-of-book to the store catalogue for interface-7 X4 plugin files (#277).

- Restore keys to the store catalogue for interface-7 X4 plugin files (#277).

- Restore koreader to the store catalogue for interface-7 X4 plugin files (#277).

- Restore menu-editor to the store catalogue for interface-7 X4 plugin files (#277).

- Restore notes-overview to the store catalogue for interface-7 X4 plugin files (#277).

- Restore notes-vault to the store catalogue for interface-7 X4 plugin files (#277).

- Restore profiles to the store catalogue for interface-7 X4 plugin files (#277).

- Restore reading-goal to the store catalogue for interface-7 X4 plugin files (#277).
- Request the installed cat pictures for Reading goal widgets and sleep screens through the bundle image source (#277).

- Restore rules to the store catalogue for interface-7 X4 plugin files (#277).

- Restore stats to the store catalogue for interface-7 X4 plugin files (#277).
- Fix the statistics stage reply so Reading goal receives today's reading time and an explicit unknown result when the date is unavailable (#277).

- Restore translate to the store catalogue for interface-7 X4 plugin files (#277).
- Add Wikipedia to the store catalogue for interface-7 X4 plugin files (#277).

- Restore weather to the store catalogue for interface-7 X4 plugin files (#277).

- **The action `vocabulary.scan-card` is back** (#277). Key maps and rules of Beta 3 may name it: it opens the screen that reads every file of the cards folder on the SD card anew. It has no menu entry, as in Beta 3; the files are still reached from *Flash cards*.

- **A store download no longer takes the last of the memory on the X4** (#287). With a book and the home screen used and Wi-Fi on, the secure connection of a download took all but 4 KiB of the heap: every screen of the store refused to draw (the page stayed blank) and only a restart gave the memory back. A secure connection is now only opened with 80 KiB of heap free; where there is less, the download ends and the store says "not enough memory, restart the device and try again" (en and de).
- **Settings > Manage plugins no longer stops the X4 with the watchdog when Wi-Fi is on** (#288). Every frame of a list measured the wrapped notes of all rows (about 70 plugins, one measure for each word with the whole line again, 2,700 calls and 6 to 14 seconds on the X4). A text is wrapped by measuring a word at a time where it is far from the edge of the line, the whole line only near it. A list keeps the heights of its rows from one frame to the next (moving the focus, turning a page) while no row changes, so the measuring is done once for a content, not once for every frame.

- **A store download on the X4 can open its secure connection after reading and the home screen** (#287). Before the connection of a request from a plugin opens, and where the heap is short for it, the device now closes the hyphenation tables and puts the plugins that sleep when idle to sleep (as at standby), and the received bytes waiting for the secure connection take 2 KiB less. Where the memory is still not enough, the store says so ("Not enough memory", with the way out) instead of "No connection": on the list and on the install, whose page no longer stays on "Downloading" for ever when the download ends (the plugin's own name was lost when it was cleaned up, so the page never heard of the end). A secure connection that runs out of memory in its handshake is told as memory too, not as "secure connection failed".

- **A watchdog or brownout restart on the X4 now leaves an entry in `crashes.txt`** (#289). Only a panic did before, so a hang that the watchdog ended left no trace. The next start writes the chip's reset reason (RTC or timer-group watchdog, super watchdog, brownout) with the place the firmware last noted being at and the light sleeps of the run.

- Reinstalling a plugin whose saved passwords could not be erased when it was removed no longer fails with "id taken": the install question now says the old data is still on the card and is erased first (en/de), and the trusted-installer grant never skips it. On yes the old secrets are erased before the plugin is put in place (if they still cannot be, nothing is installed, reason 16); declined or dismissed, the plugin is not installed, so it never starts with the old secrets.

- **Settings > Manage plugins opens in about a third of the time on the X4** (#291). With about 70 plugins the first frame made some 6,200 host calls (4,300 of them text measures: every band of the screen measured the wrapped note of every row, a word at a time). The height of a note is now one measure (it fits a line or it takes two), the heights are kept for the bands of a frame, and a row outside the band being drawn is not laid out: 1,830 calls, 280 measures.

- **The Home heap test no longer fails in parallel runs** (#292). `repeated_returns_do_not_grow_the_heap` counts the process's heap, which the other tests of `home_return` moved while running in parallel; the heap test now runs alone while the others share a lock. Test only.

- **The Library's first frame is much cheaper** (#297). A name too long for its row was cut one letter at a time, each letter a text measure the host answers, and every band of the frame laid out every row of the page. The cut is now found by halving, and a band lays out only the rows it draws: 100 books with long titles took 2,600 host slots for the first frame in the emulator (X4 profile) and take 796.
- **A Beta 3 plugin that is not in the list of installed code now says "Built for an older PaperPaw: update it in the store"** (#265) instead of "Not installed from the store or the web flasher".

- **The first book walk writes less to the card** (#297). A book of a format plugin (EPUB and the others) had a stand-in details file written when the walk first saw it, and the host's details job wrote the real one soon after: two writes of a book, each many card commands. Only the job writes now (14 books on a FAT card in the emulator: 515 write commands for the start and the walk, 684 before; the Library's first open reads 20 and writes none). The day a book came is taken from the day its details were read, so a book the job has not read for a day or more is listed as new on the later day.

- **The Home clock and date sat too low on the X4, and the Home card was
  pushed down with them** (#303). Where the host does not tell the ink of
  text, the clock took the digits for as tall as the size says, but the
  X4's fonts stop growing at their largest size, so the block was about
  130 px too high. It now takes the size the font really draws.

- **Text on the X4's keyboard and plugin screens sat too low** (#303). The
  Phone button, the field's labels, the keys and the titles of plugin
  screens were drawn 7 to 10 px too low on the X4, so the Phone label lost
  its lower edge. They now stand in the middle of their place as in the
  emulator.

- **The grays of the covers come back after every rest of 30 s** (#308). After
  one gray pass, a move on Home drew the covers in black and white again and
  nothing brought the grays back. Now the pass runs again 30 s after the last
  key, once, and not while the covers are gray and nothing changed.

- **Home no longer draws its whole page again a few seconds after you moved
  the focus** (#308). Each minute Home laid the page out anew and redrew all of
  it (1.8 s on the X4, a key press waiting for it) whether anything had
  changed. Now a minute that shows nothing new draws nothing, and the clock,
  the date or the status line that changed draw only their own place.

- **A book's cover no longer vanishes from Home for good after you read it a
  little** (#308). Home asked the library for the cover with the list it had
  read a moment before; when the book was read in between (its progress
  changed as the reader closed), the library said the list was out of date and
  Home took that for "this book has no cover", kept it for the whole session
  and never asked again. Now an answer that says nothing of the book (list out of
  date, no room, card hiccup) shows the card or the shelf without the cover and
  asks again at the next minute, with the list read anew.
- **Home no longer loses its whole card when the heap is low** (#308). The
  library's cover call needs about 30 KiB while it makes a cover; where there
  was no room the refusal ended Home's draw (`home: call 10 ended with -4`, six
  times with the store and Wi-Fi open) and left the card and the shelf blank.
  Now Home draws them without the cover and tries the cover again at the next
  minute.

- **Home asks the library again after a start without room for the books** (#308).
  A key press right after a start whose book list was refused for lack of
  memory is no focus step: the list is read again, so "Continue reading"
  shows and opens.

- **Moving the focus between the widgets on Home is faster on the X4** (#308).
  A press that only moves the focus no longer makes Home ask the library for
  its books again and compose the Continue card and the shelf again at the next
  draw: they are drawn from what was read when Home opened, and only the two
  widgets the focus left and came to are drawn again.

- **Moving the focus on Home is faster on the X4 again** (#308). A frame is
  drawn in bands of rows, and every text, line and cover of a widget was sent
  to the host in each band it touched, also where it lay in another one. Now
  each is sent only in the band it lies in: a move between the Continue card
  and the shelf draws each cover once and needs about a third fewer host calls.
  The raw key before a focus press no longer makes Home ask the library for
  its books again at the next draw.

- **The X4 no longer loses a third of its largest block of memory to the way a
  frame is drawn, and with it its fast frames** (#308). A frame of many bands is
  drawn in a taller band for the moment (#267). When the frame was done the
  usual band was asked for while the taller one's block was still held, and on
  the X4's heap (the first block that fits, lowest first) it could come to lie
  right behind it: the 24 KB then given back were a hole under the band, 35 KB
  of the largest block gone until the next restart. Whether it happened hung on
  how the start's allocations fell; since the reader's update of #235 it did at
  every start: the largest block on Home was 52 KB for 75 KB, under the 61 KB a
  taller band needs, so every larger frame was drawn in all its bands (a move
  on Home 1.2 s for 0.8 s). Now the taller band's block is given back first.
- **A largest block a little short of the room for a band of half the screen
  still draws a frame in fewer passes** (#308): the band is as tall as the
  block allows, not the usual one.

- **A saved page turn on the X4 writes one block, not twelve** (#308). The
  storage wrappers of the X4 (its trace), the desktop trace, the Android
  volumes and a mounted storage did not pass an in-place write on, so the
  position file's new two-slot save (#305) fell back to writing the whole file
  anew on every save.
- **Held Back in the keyboard finishes and closes it** (#308). The action of a
  native plugin could not stage `close` (`ERR_UNSUPPORTED`, the warning "keyboard:
  call 1 ended with -1"), so the keyboard stayed open after its answer. An action
  has the screen stack, as a screen's callback has. The warning "native redraw
  dropped: its screen is covered" is gone: a redraw for a screen that is covered
  is dropped as before, it is no fault.

- **Letter spacing, kerning and ligatures show at once** (#311). Changing one of them while a book is open lays the page out again, as hyphenation and the other reading settings already did, instead of leaving the old page until the book was reopened.

- **A large picture's page peaks 3 KiB lower** (#316). Shrinking a picture no longer keeps a table with an entry for each pixel of a row (3.2 KiB for a 1600 pixel wide picture): the pixel each one goes to is stepped along the row.

- **Large pictures in books use less memory** (#316). A large PNG in a deflated entry of a book (a 1600 x 1000 picture) no longer holds two inflate windows at once: it is copied to a file first and decoded from there, one window at a time, so its page peaks at 214 KiB instead of 260 KiB on the X4. Long JPEGs in deflated entries go the same way (213 to 185 KiB).

- **Community plugin sources are read by a store packed as a plugin file** (#320, #214): such a store checks a source's signature with an Ed25519 of its own whose largest stack frame is about 1 KiB, in place of the kernel's, whose frame of about 4 KiB no plugin file may have. It refuses and takes exactly what the kernel's does (a forged or changed list, a signature that is not canonical, a key of a small order). The store file for the X4 Pro and X4 Classic took no source's signature before, and the one for the X4 was refused by the packer; the firmware's linked store is as before.

- **The plugin list says a plugin copied to the card runs** (#321). Its row read "Not installed: its code was…" (cut off) although it runs; it now reads "Not signed: runs without a check." (en + de).

- **Reader: pages left in a chapter that begins a file** (#334). A chapter
  that begins a file is on the first page of that file, so the status lines
  no longer count one page too many for the chapter before it, and the first
  chapter of a book is found on the first page. Found in a fork by Alexandre
  Cormier ([alexcormier/paperpaw@958537c](https://github.com/alexcormier/paperpaw/commit/958537c4a2751a7dbf75175b51511790a6cd22b4)).

- **KOSync: the account key leaves no copy in memory** (#334). The key
  written into a request body or read from the account file was left in
  freed memory when the text had to grow or carried escapes; it is now
  made in one block and wiped.

- `paperpaw-dev perf` over all plugins measures every screen: the firmware confirmation, which guards the device, stayed open after it was measured, and every later screen opened below it unseen until the screen stack was full (about 70 screens were left out without an error). A screen that still cannot be measured is now an error that names it and the screen shown instead, and the run ends with a failure.

- **The sleep screen no longer reads its picture back from the card** (#337).
  Making a gray picture ready (the sleeping cat, the logo, a cover, a picture
  of a folder) read the finished picture again row by row from the card for its
  brightness map, about 290 card calls for the cat. The map is now summed while
  the picture is decoded, so the sleep screen asks the card about 23 times
  before its first frame instead of 293.

- `paperpaw-dev perf` over all plugins ends without an error again: the Wi-Fi plugin's list of networks goes back while the radio is off (as on a device, where the page only offers it with the radio on), and the emulator starts with the radio off, so it was reported as not measured. A screen that closes at once is now tried once more on a second emulated device with the radio on and joined, and only an error if it still closes there.

- **The sleep screen in cover mode shows the book's cover, not the cat** (#346).
  When the cover of the book read last was not made at the screen's size yet,
  the screen showed the cat. It now uses the cover Home shows, and swaps to the
  full-size one when the host has made it.

- **The sleep screen in cover mode shows the cover of the book read last, also
  right after a restart** (#346). The screen asked the library for the books in
  falling "recently opened" order, which puts the books never opened first, so
  it found no book read and showed the cat. The host noted a cover as made per
  book, not per size, so the screen-sized cover was decoded inside the sleep
  screen's call where only the list's was in the cache. And after a start
  straight into the reader the host answered "not now" for the book's cover
  while the device went to sleep; the book being read now gets its list-sized
  cover at once (from the cache, or decoded where the heap has the room).

- **The sleep screen in cover mode shows the book's cover on the X4, not the cat**
  (#346). The screen-sized cover is decoded inside the call and the X4's heap
  does not hold that; the screen gave up on "no memory" instead of asking for
  the list's cover Home shows. It asks for that one now.

- **Largest text: screens that were cut or drawn over themselves** (#348).
  At the largest text size the keyboard's letters were higher than their
  keys on the X4 Pro and the Boox Go 7 and *123* wider than its key; the
  text of *Wi-Fi transfer* without a network ran under the button
  *Without a router*; the words of an empty list (*News* without feeds)
  ran past the bottom of the screen; the store's two texts lay over each
  other while the list loads (German); and in the quick panel a long
  title lay under its value. Letters are now as large as their key
  holds, texts keep to the lines their room holds and end in "…", and
  the panel's title ends before the value. At the usual text size nothing
  changes.
- **`paperpaw-dev visual`, `reach`, `gallery` and `snapshots` look at
  every screen again** (#347). After the firmware confirmation was shown
  it stayed open, every later screen opened below it and was left out
  without a word: the checks, CI's too, saw 5 of 77 screens. Going back
  to the home screen now closes a screen that guards the device too. The
  focus check passes over rows that do nothing, and a screen that hides
  the row of hints is checked against the whole display.

- Settings > Sleep screen > Widgets: a widget of a plugin that is not installed (the *Read today* of Statistics, in the default list) shows "Not installed: <plugin>" instead of its raw key; it can still be removed (#349).

- **A screen whose plugin refuses the open no longer stays open and blank** (#350). Where a plugin's `screen_opened` ends with an error (a screen opened by another plugin with wrong parameters), the kernel closes the screen again and shows the notice "This screen could not be opened." instead of leaving an empty screen on the stack that every draw failed on.

- Another plugin's screen chosen as the sleep screen (the flash card in *Sleep screen*) is shown when the device sleeps: the host now takes the sleep plugin's `bind_slot` command, a choice added by a plugin that registers after the sleep plugin is no longer dropped from `paperpaw.ini` at start, and the vocabulary's sleep card notes that it is shown (#354).

- The periodic full refresh no longer counts the first frame of another screen (Home after Back, a book opened) as a page turn: Back→Home flashed (1.7 s instead of 0.7 s) at about every third return; now only real page turns make the full refresh due (#355).

- **"How it works" in the plugin manager can be reached with the buttons** (#357).
  Holding Select (or pressing the menu button) on a plugin in Manage plugins now
  opens its text in the reader, as a long touch does; before, the buttons opened
  the quick panel instead and the touch hold only said the text could not be
  shown. A plugin without such a text says so.

- **A book opened with a dialog over it no longer fails its draws** (#359). When another screen (a dialog) opens over the reading screen in the round the book opens, the host draws the page before it tells the reader it is shown, and the reader's draw ended with `ERR_STATE` ("call 10 ended with -6") until the dialog closed. The reader now draws its newest screen then.

- At the largest text size the keyboard no longer leaves a cut-off strip of the screen below (its header, e.g. "Settings 1/6") above it: it leaves free a whole header of the menus' look, not only a status line (#362).

- The to-do widget opens its list when pressed (heading, "more" or an empty list), so a first item can be added from Home (#364).
- Fonts: a catalog with no font this device can take shows the "unreachable" message instead of "Loading fonts…" for good; the emulator's fixture catalog lists `plugin.ini` again (#366).

- A stray `--help/` folder (91 hyphenation pack files) at the repository root is gone: `tools/hyphenation-data.py`, `literata-packs.py` and `script-pack.py` took `--help` for their output folder and now print their usage instead; the quick check fails when a tracked path starts with `-` (#367).
- The plugin store's publisher no longer lists `android` as a device for plugins: Android loads no code plugins (#237).

- **The reading-goal cat shows when the plugin is installed from the store** (#370). *Reading goal* asked for its picture in the widget's measure, where commands are refused, so the request was lost and only the minutes showed. It now asks when the widget is made (Reading goal 1.0.11). The minute tick that retries such requests never came where nothing else woke the kernel (the emulator, and an idle X4): the kernel now also sends it at the shell's once-a-minute status wake, as `docs/abi/records.md` says, and the emulator tells its status once a minute like the X4's firmware.

- `paperpaw-emu --registry` lists the widgets that native plugins declare (the home screen's, the widget pack's, news and vocabulary: 34 instead of the two the kernel brings itself), so tools that read the file (the handbook, the Workshop's data check) no longer call them nonexistent. The file keeps its version 2 and its shape (#372).

- Reader: *Turn wide pictures* turns a wide picture in the full-screen view
  again. The host looked the picture up outside the reader's own pictures and
  answered "not found", so it was always shown as it is (#374).

- **Switching a plugin on or off in Manage plugins no longer freezes the X4** (#375).
  Settings built its main page twice behind the plugin list, once for every
  plugin switched, which took about 16 seconds; with more plugins on the card the
  watchdog restarted the device. Settings now builds a page only when it is
  shown, so a switch takes about as long as opening the list. Long host work for
  a plugin also keeps the watchdog fed as a safety net.

- The UI language packs for Amharic, Belarusian, Central Kurdish, Kazakh, Kyrgyz, Macedonian, Pashto and Serbian build again: their longest help texts (calendar, vocabulary and a few others) were over the 4,096 bytes a compiled catalog holds per message and are condensed, all their content kept; `tools/language-pack.py check` now reports such messages (#385).

- The update check on the X4 no longer fails at once with "the request failed": it asked for an answer of up to 40 KiB, more than the X4 allows a memory body (16 KiB), so the request was refused before it was sent; it now asks for 16 KiB (#395).
- A failed secure connection on the X4 is now reported as such (certificate or TLS failure, `tls`) instead of "could not read or write". The catalog (OPDS) screen then says "Certificate not known. Install 'All certificates' from the store." instead of "could not be reached" (#395).
- A network request the host refuses leaves one line in the device log (`net: request of … refused (code)`) (#395).

- Gujarati, Gurmukhi, Odia, Tamil, Telugu, Kannada, Malayalam and Sinhala words are measured as they are drawn, with their conjuncts, so lines in menus and books are as wide as they look (#397).

- **The newer X3 shows PaperPaw** (#408). X3s made from about July 2026
  have a different display controller (UC8279d). PaperPaw drove it as the
  older one, so every refresh timed out after 10 seconds and the screen
  stayed blank. It now sets that controller up and uses its own
  waveforms. Older X3s are unchanged.

- Secure connections after reading a book in a script such as Devanagari: the font pack kept its substitution table (about 15 KB) and the advances of its sizes in memory through every other screen, so on the X3 and X4 the plugin store could fail with "Not enough memory for a secure connection". Before a secure connection (and where a screen asks for room) the packs now let go of them; they are read again from the card when the next page needs them (#410).

- **X4 Classic: home screen, plugins and books work again** (#415). The stack-guard's self-check expected the chip's monitor registers to read back the full 32-bit addresses, but the ESP32-S3 keeps only 26 address bits and 16-byte steps. The guard never armed on the Classic, so no plugin ran and Home stayed empty. The check now expects what the chip holds, and the log names the reason when the guard does not arm.

- The emulator serves hyphenation to the built-in plugins when no code plugins are trusted: the German pack on the card now changes how the tools drawer breaks its names, as on the device (the kernel takes a hyphenation sink from the shell without a code loader, `KernelBuilder::hyphenation_sink`).

- **The EPUB sleep-parity case runs again** (#243, todo 617). A parked plugin made by an input asks for one more pass at once and the pass after it sets the next wake-up; the test now folds that tail pass into the one before it and counts it (awake and asleep may differ by at most 2 per step), where it compared the first effect only. No change to the firmware.

- The quick panel's sliders can be dragged again, as in Beta 3: a finger moves the knob as far as it moves itself, the number shows at once and the light follows it without being kept, and lifting the finger keeps it. A tap on the track sets the value there. Screen readers and tests find the slider with its value.

- **A damaged converted-book index can no longer overflow a read** (#667). Reading the index of a converted chapter multiplied and added block numbers without checking; a block number at the end of the range could panic in a debug build and read the wrong place in a release build. The numbers are now checked and a number out of range reads nothing.

- **No room while a picture is made never aborts or marks it** (#1799). Naming a picture's cache file in `prepare_image`, `prepare_image_larger` and the EPUB cover, and in a FictionBook's cover, no longer allocates without a way to refuse, so a short heap is an answer, not an abort. The EPUB cover answers `OutOfMemory` when it found no room to look up the decoded picture, instead of no cover. A FictionBook cover that found no room to be looked up is no longer marked undecodable (which hid it until its book's cache went).

- **The emulator registers its built-in plugins beside an installed interface-7 card plugin** (#2850). With `--abi7-dev-loader --trust-code` and one such plugin installed, every built-in plugin failed with status -6, because the card plugins' loader and the built-in registration each made their own table of handles and dispatch budget, and a kernel takes one of each. They now share one per kernel.

- `tools/language-pack.py` now also reads the kernel's `i18n/host` and `i18n/settings` folders and the app's `i18n/pebble`, which a language pack lacked until now.

- Progress sync (kosync) on interface 7: its linked host tests run again (the menu fixture lacked the core texts), the plugin is version 1.0.11, and it is complete: the book fingerprint, XPointer and locate go through the library and document services, so sending and receiving progress with KOReader servers works.

- **The base passes its checks again** (#3673): `turn_lab.rs` is rustfmt-clean, and the S3 band names the indirect calls of the glyph cache's `render_reusing` and `Lru::shrink_to` (#203).

- With a language pack from the card (every language but English and German), some texts of a long screen stayed English, for example the *Search* field, *Advanced settings*, *Reset all*, *Restart* and *Off* on Settings: past a limit the kernel no longer read texts from the pack. It reads them from the card again now.

- The base builds green again: the sleep settings' help texts (what the screen shows when switched off, at start, at restart) fit their two-line rows.
- The picture downscaler keeps its (now unused, empty) `columns` field so the plugin interface's layout stays as draft 36 has it.

- **The hints note no longer hides a hint under its box at the largest text** (#350, #347). Where the note's box covers the text of a hint, that text is now said in the box instead of being drawn under it.

- Language packs: the exFAT card hint ("exFAT support is experimental") and the plugin manager's "How it works" messages are translated in all languages.

## [0.6.0-beta.3] - 2026-11-05

### Added

- **Plugin interface 7, draft 5** (#228, #241, the one breaking batch before 1.0; contract in
  `docs/abi/screens-input.md`):
  - **Callbacks:** `dispatch_event` gets a reply buffer and returns the reply length or a status, and
    `draw` gets its `draw.frame` (target rectangle, clip, purpose).
  - **Screens and input:** screens and actions are declarations, plugins get screen and input events
    through the new `input` callback (handled or not), and the i18n slots 27/28 add formatting and digits.
  - **Tag space:** every record family has room to grow, and records are classified by exact tag only.
  - **Pages:** page turns carry an `insert` ordinal, so pages with inserted content keep their own positions
    and progress counts text only.
  - **Statuses:** `ERR_DENIED` and `ERR_TIMEOUT` are new.
  - **Metadata:** a plugin may mark a feature it needs as required, and may require up to 32 services.
  - **User grants:** grants to plugins (style overrides, installs without asking) are guarded settings no
    plugin can change.
  - Plugins must be rebuilt for draft 5. A host cannot tell drafts apart (the metadata carries no
    draft), so a plugin built for an earlier draft must not be installed; none was distributed.
  - A feature a plugin marks optional that the host lacks, or has only at an older revision, is left
    out; only a required one refuses the load. A host serves every older revision of a feature.
- **The Wi-Fi page: the networks have a page of their own** (#161). The Wi-Fi page is short whatever is in the air:
  the switch with where it is, a row *Networks ›* (the network the device is in and how many are in reach; it opens
  the list), the setup with a phone and the three settings. While no network is kept, *Set up with a phone* comes
  first, below the switch. Back on the list returns to the Wi-Fi page; if the radio goes off meanwhile it does
  too. Opening the list does not scan again (the Wi-Fi page has just); its last row looks again. Both pages measure
  their rows again only when what they say changes, and an event of the radio that changes nothing they show (a
  signal that moved a little) draws nothing: each frame costs the X4 a screen refresh, and a key pressed meanwhile
  waits for it (#160, partial: the cause of the slow Up and Down is not settled).

- **Spanish user interface.** Kernel and plugin menus in Spanish, as the data plugin `language-es` from the store.
  Machine-translated and checked with `tools/language-pack.py check es`; corrections from Spanish readers are welcome.
  New or untranslated messages fall back to English. Spanish dates, shorter labels and consistent *Elegir* are
  included (#170).
- **Colours of widgets** (#206, SDK). Each widget has a paper (`<widget>.paper`: normal, inverted, light gray, dark
  gray, transparent; on colour panels also any tone or a code such as `#3b6fd4`) and a text colour (`<widget>.ink`,
  automatic: black or white, whichever reads on the paper), and the kernel's `core.widgets.paper` and
  `core.widgets.ink` set them for every widget that has none of its own. `widgets::widget_look` gives the colours and
  `widgets::draw_frame` paints the paper, so widgets that draw in `look.ink` on `look.paper` follow without a change;
  `widgets::colour_spec`, `paper`, `ink` and the settings' builders are new. `GridView::draw_on` takes what lies
  under the items (`widgets::Plate`: white as before, nothing over a picture, a colour), so widgets can sit on a
  picture or a dark page. Additive: plugin interface 6 stays, plugins built before draw as they did; their widgets
  follow the colours, those of all widgets too, once rebuilt with this SDK and run on firmware with the shared
  widget family described below. Transparent widgets take the page to be light: on a dark picture, choose a light text colour.
- **Shape of widgets** (#206, SDK). Each widget has corners (`<widget>.corners`: automatic, square, slightly
  rounded, round, soft), a frame line (`<widget>.frame-line`: automatic, thin, medium, thick) and a solid shadow
  below and to the right of its card or paper (`<widget>.drop-shadow`; dark gray below a black plate, which a black
  one would not stand out from, dithered where the panel has few grays). The kernel's `core.widgets.corners`,
  `.frame-line`, `.shadow`, `.padding` and `.gap` set them for every widget that has none of its own; "Widget look"
  presets cover them. `widgets::draw_frame` draws them, so widgets need no change; a widget that draws all of itself
  stays as it is. `widgets::widget_look`, `draw_frame`, `frame_height` and `draw_caption` now come from the
  firmware (shared-table family `widgets`, `Family::WIDGETS` = 6, revision 1; `Family::CORE` is deprecated), so
  each widget plugin is 3.5 to 5.8 KB smaller on the X4 and its widgets follow the firmware's look. Plugins built
  with this SDK that have widgets need this firmware: beta.2 refuses them cleanly ("needs a newer PaperPaw") and
  the store does not offer them to it; plugins without widgets are unaffected. Plugin interface 6 stays.
- **Picture home screen** (#205), a store plugin (`picture-home`, group Look). Your own
  picture behind the widgets, from a folder on the card: fill, whole picture, stretch, original size or tile, in one
  of nine places, zoomed and made paler; prepared once into a screen-sized gray file and drawn from it, a few rows at
  a time. Pages of widgets and tools placed anywhere in the SDK's grid, on plates over the picture. The status bar is
  yours to configure: on, off or automatic (a button press or a tap shows it for a few seconds, and that press does
  nothing else), top or bottom, full width or a compact pill at the left, middle or right, on a light, inverted, light
  grey or dark grey plate or on the picture (black or white text, whichever reads). The row naming the buttons (X4)
  can be on, off or automatic. While the plugin is on it is the home screen; switched off, the built-in home screen
  is back with its own pages. Brightness and contrast controls adjust the background while keeping black and white;
  changing them prepares the picture once again.
- **Colour rows in every first-party widget** (#206): each widget of the widget pack, the home screen, statistics,
  weather, reading goal, mail, news, notes vault and vocabulary has *Colours* (its paper) and *Text* beside its frame
  and spacing. The picture home screen's *Widgets sit on* sets the plate under the widgets: white, light gray or none.
- **Shape rows in every first-party widget** (#206): the same widgets have *Corners*, *Frame line* and *Shadow* beside
  their colours, and the home screen and the picture home screen place their widgets with the gap the reader set for
  all widgets (small, normal, large).

### Changed

- **Release page: plugin files out of the firmware release** (#89, #171). A firmware release now holds 73 files; every plugin
  file and every plugin or data folder's ZIP goes to one store release of the year (`plugins-2026`, a pre-release that is
  added to and never replaced; only versions not published before; a numbered successor when the projected count exceeds
  800 assets). The release uploads them before it creates the firmware release, so the catalogue never names a file
  that is not there. The workflow
  *Store plugin update* now takes several plugins at once, and builds from `main` against the base release's fingerprint;
  both workflows have a `dry_run`. Releases already out are untouched.

- **"More plugins" is now "Plugin store"** (#213; de: "Plugin-Store"). The name of the store on the device, its title
  and the help texts that send you to it.

### Fixed

- Interface-7 page-turn delivery keeps the location when optional page counters are stale or
  unparsable, marking unavailable counters absent. Callback enumeration excludes declarations,
  which shells must apply separately before activation (#228).

- **Dual boot: no false positive on plain devices, and a way out** (#224). The dualbooter's `x4boot` tag counts only
  as a word of its own (not inside `x4bootloader` or a path); the bootloaders PaperPaw ships for X4, X3, X4 Pro and
  X4 Classic, CrossPoint's and others are scanned in a test and none is taken for a dual-boot one. Where a device is
  taken for one wrongly, the notice on the Updates page has a button, *This is not a dual-boot setup* (de: *Kein
  Dual-Boot-Gerät*): after a question that warns an update can overwrite a second firmware, it writes `dual-boot =
  off` in `paperpaw.ini` and restarts, and updates work again. Where the file says off, *Detect dual boot again*
  writes `auto`. Update plugin 1.0.6.
- **Dual boot: no write to the OTA data before the setup is known** (#222). Where the bootloader carries a
  dual-boot tag (`x4boot` or `paperpaw-dualboot:1`), an update PaperPaw put on trial itself, or one that failed its
  trial, is no longer marked (or gone back from) before the card is read: nothing is written to the OTA data at all,
  so the dualbooter's choice of slot stays. A setup declared only by `dual-boot = on` in `paperpaw.ini` cannot be
  known before the card is read; there a trial PaperPaw itself began is still marked at once, as before.
- **X4 beside the X4 dualbooter** (#167). The X4 finds the slot it runs from by its own code, so a dualbooter's
  bootloader no longer hides the OTA state. PaperPaw recognises the dualbooter's bootloader by its `x4boot` tag
  (besides `paperpaw-dualboot:1` and `dual-boot = on` in `paperpaw.ini`), decides at every start, writes
  `.paperpaw/boot.txt`, and the Updates page says that the device holds two firmwares and installs nothing
  here, also where it has no firmware information. Update plugin 1.0.5.

- **Card language packs keep translating later commands** (#208). The kernel reclaims a nearly full message
  cache between commands, so a preceding callback cannot exhaust translations for later notices and dialog labels.
  Smaller caches stay resident; the 16 KiB budget and plugin interface stay unchanged.

- **Arranging the home screen no longer turns the page or opens the tools by mistake** (#215). While the page is arranged, a
  swipe or tap that hits nothing (a finger straying off a widget) does nothing, instead of turning the page or opening
  the menu. *Done* and *Add* are buttons at least two rows wide, with the word *Done*, instead of a small tick that taps
  missed and that landed on the page.

- **The store's lists are not left empty when the list arrives** (#169). A group (e.g. Languages) opened while a fresh list of
  plugins was still being fetched showed its pages with every row blank, only the lines between them: the fresh list took
  the place of the one the open list had picked its plugins from. The open list now picks again from the fresh one.
- A rotated picture-home photo is prepared only for the part the screen shows, avoiding a full-image transpose
  at high zoom (#205).

- Adding a UI language pack no longer changes the shared language builder or makes CI ask for version
  bumps of other languages (#172). Language metadata comes from its catalog header, and store texts and
  dependencies from its data-plugin entry. Existing language packs build to the same bytes.

- **The plugin manager reads only the rows it shows** (#208). It translated the description of every plugin to find how high the
  rows must be, which filled the 16 KiB a language on the card may read in one go; later texts of that call came out in
  English. Now only the rows of the page shown are read, and the rows get higher, never lower, when a page needs it.
- **The store shows every plugin after an update, without a restart** (#164). The first start after an update runs on trial
  and loads no code plugins, on purpose, and the kernel then did not know what the firmware takes: the store asked for no
  builds and showed only data plugins (Look, Languages) until the next cold start, which a Pro on light sleep may not have
  for days. Nothing changes during the trial (no code plugin is offered or installed, so a bad update still goes back and
  the plugins before stay as they were), but once the update is kept, its mark written for good (about 30 seconds on), the
  kernel tells plugins what the firmware takes: the store, opened or opened again, lists and installs the builds that fit
  (*Restart now* as for any code plugin); a mark that could not be written keeps it as before.
- **Widget plates follow rounded corners** (#206). A grid plate no longer sticks out past a widget's corners or
  fills the space beside its shadow. Frameless paper follows explicitly chosen corners; Automatic stays square.
  Empty widgets have no grid plate outside editing mode.
- **X4 Pro: the charging bolt on battery, and a sleep that never switched off** (#209). The Pro took "a computer sends USB
  frames" for charging: the bolt, the kernel's low-battery rules, its install check and the sleeping reader's *Switch off
  after sleeping* all went by it, so a Pro on battery could show the bolt, run no idle naps and stay asleep all night.
  Charging is now the charging pin alone (the X3: its gauge and the frames, as before); the frames only keep an awake
  reader from napping while a computer is on the cable, and count only when the frame number moves on like a bus. The
  Pro's charging pin is high on a cable and low on battery (measured on a Pro; the first port had it the other way round). A reader asleep on a cable with a full cell is switched off after its time like one on a
  battery; while it charges it stays asleep, which the setting's help text now says (en, de). The diagnostics page
  shows the charging pin's level and whether USB frames come on the Pro and the X3 (diagnostics 1.0.7). The X4 is as before.
- **The empty start card reads on a dark card** (#207). Its words were black on the black card of a dark frame (and on
  an inverted paper); the home screen draws them in the widget's own ink now, as its status line does. The pack's
  world clock, library numbers and weather forecast chose their lines and shades by the frame being dark; they follow
  the paper's lightness now, so a dark gray or inverted paper works too.
- **A book opened before its font pack came no longer keeps a wrong page count** (#163). A Hebrew book opened before the
  Hebrew letters were installed was counted with those letters measured as `?`: 2 pages for a book of one. The count was
  kept under a name that did not change when the pack arrived, so the book showed `1 / 2` and no turn went to the
  second page. The pages' key now covers the fonts and packs there are (a pack that comes or goes makes the pages be
  counted again, also while the book is open), and a turn that finds no page though the count says more throws the
  count away and makes it again. Reader 1.0.8.
- **The button bar speaks a store language from the first frame** (#204). A screen that read much of a language pack as
  it opened (the plugins list reads every plugin's name and description) filled what is read of the pack at once, so
  the bar's labels fell back to English (*Back*, *Select*) until the next redraw. The kernel now forgets the messages
  read before it looks up the bar's labels when they near that limit, as it already did at the end of each call.

## [0.6.0-beta.2] - 2026-11-03

**Plugins built for 0.6.0-beta.1 keep working; rebuild only to use the new items.** (Plugin interface 6 is unchanged; the SDK changes only additively: `SCREEN_REVISION` 3, `events::MENU_PAGE`, `Cx::is_on_top`, panes, `ScreenClass`, `StorageError::CARD_GONE`, `ui::SearchField`.)

### In plain words

This is a beta. Keep a copy of your books. The details for plugin authors follow in the sections below.

- **Plugins built for 0.6.0-beta.1 keep working** (plugin interface 6 stays). Rebuild a plugin only to use the new items in this release.
- **The first update from 0.6.0-beta.1 has no early rollback marker.** A bootloader with rollback and the web flasher still recover a device that hangs on that first start; later updates can be rolled back early.
- **PaperPaw beside another firmware** (#95). A tool that puts two firmwares on one X4 can tell PaperPaw so (`dual-boot = on` in `paperpaw.ini`, or a marker in the tool's bootloader). PaperPaw then never writes the other partition or the update data. Not declared: nothing changes.
- **A memory card taken out and put back** (#145). The library no longer says "Preparing the library …" for ever: it says at once that the folder cannot be read, shows the books again when the card is back, and asks for a restart where the X4 had to give the card up. No first-run hints over a card that cannot be read.
- **X4 Pro: a real sleep before it switches off** (#153), and a shorter wait to wake on the X4 and X3 (the light sleep's watchdog counts in the sleep).
- **Switch off leaves a picture** (#146): *Switch off* in the power menu shows the sleep picture first, so the device no longer looks frozen.
- **One search field everywhere** (#113): the library, settings, the store and the button settings use the same field.
- **The first walk of a big library reads no covers** (#398), **a list beside its detail on wide screens** (#88) and **the reader's menu shows the page and the pages** (#117).
- **Less waiting** (#118): the busy mark shows less often, and settings screens open faster.
- **The store** shows all the plugins of this PaperPaw, lists a *Languages* group (#148), installs the plugins a plugin requires (#97) and shows the download for a book that needs more letters (#147). The update check says how long the device is in use (#143) and tells when some plugins did not come along (#141).
- **Stay awake on USB** (#96): a developer setting, off by default, so a computer can keep the link for remote tests.
- **The device check** steers its first pages with the buttons (#121). The Screen page may still react slowly to the arrow keys on the X4.

### Added

- Storage: `StorageError::CARD_GONE` (code 1 of `StorageError::Other`, #145): the memory card was taken out and
  cannot be read again in this run (put back more often than a run allows, or holding nothing PaperPaw reads);
  nothing is read or written until PaperPaw starts again. The X4 firmware's storage says it once it gives a card up,
  and the library then asks for a restart (*Restart PaperPaw*) instead of saying it tries again by itself. New
  constant, no change of the interface.
- Screens: `Screen::times_under_overlays` (`SCREEN_REVISION` 3, #117). A screen that says so has its time
  (`Cx::wake_at`) while overlays alone cover it (a menu bar, a panel), as it still shows below them: for work that goes
  on there, as the reader counting its pages. No by default: the time waits until it is on top again, as before.
  Called through `call_since`, so screens built before are asked nothing. New item at the end of a growing trait, no
  change of what plugins built before see.
- Events: `events::MENU_PAGE` (`MenuPage { menu, page }`, #117): whoever opened the menu slot as a bar with a page
  text tells the open bar its new text; the menu plugin's bar shows it. New constant and type.
- **A list and the item chosen in it beside each other on wide screens** (#88). Where the screen is expanded (a
  tablet, a phone on its side; never the X4, which is compact in both orientations), a screen that says
  `Pane::DETAIL` shows beside the full screen below it: the list at the start (on the right in a right-to-left
  interface), at least 60 mm or 38 % wide, a line between them, bars and the row of hints over both, each laid out
  for its part through `Screen::on_resize`. A tap goes to the part it lands in and keys follow it; the new action
  `core.pane.other` moves them to the other part. A detail the list opens takes the place of the one shown, so
  going through a list piles nothing up; one the detail opens shows alone over both, and *Back* brings the two back.
  Free additions, no change of the interface: `ui::ScreenClass` (compact below `ui::COMPACT_BELOW_MM`, 100 mm,
  expanded from `ui::EXPANDED_FROM_MM`, 140 mm, by the width scaled by the reader's interface text size against
  its default), `Env::screen_class` and `Env::landscape` (of the screen's own part), `events::DISPLAY_CHANGED`
  (`DisplayChanged`: the display's width, height, class and whether it is on its side, sent once after a resize
  laid the screens out), `actions::PANE_OTHER` and `Command::extended`, which plugins had no way to make. Screens
  are asked `pane` once, when made. A list kept by `on_resize` while a turn puts its detail over it is told
  `on_hide`. The kernel's new `panes` feature (on by default) builds it; the Xteink firmware (X4, X4 Pro, X4
  Classic, X3), never wide, leaves it out and saves its code. There a detail is always a full screen and
  `core.pane.other` is not offered; the SDK is the same either way. Shells with the app's default features
  (`paperpaw-app/panes`) have it. A finger on the line between the parts goes to the nearer one; a change of the
  interface text size that makes the screen expanded or no longer so lays the two out again. A class a later
  kernel adds reads as `Expanded` in `DisplayChanged`, so an older plugin still hears the event.
- Devices (#95): PaperPaw beside another firmware (a tool that puts two firmwares on one X4) can be told so, by
  `dual-boot = on` in a `[firmware]` section of `paperpaw.ini` or by a marker in the tool's bootloader
  (`paperpaw-dualboot:1`, see `docs/dual-boot.md`). It then never writes the other partition or the OTA data: no
  updates, no trial, no way back to another firmware; the *Updates* page says why. Not declared, nothing changes. A
  `[firmware]` section no longer warns as an unknown plugin, and `firmware` can no longer be a plugin's id (as
  `plugins`, `fonts`, `menus` and `slots` cannot). An update PaperPaw installed itself is still marked as on trial at once, as before; what a tool put in the other
  slot, and the undoing of an update, wait until the card and `paperpaw.ini` are read. The bootloader's 32 KB and the ini are read only where a start would write the OTA data, after a declared start, or before an install; a normal start reads neither. No change for plugins.
- **One search field everywhere** (#113). The SDK draws a search field as one part, `ui::SearchField`: a rounded
  outline round the search icon and a gray hint (or the words searched for), the focus over it when focused, the
  icon at the right where the interface runs right to left, in the theme's inks. The library's *Find* screen, the
  settings, the store and the actions list of the button settings use it, so their searches look the same; the
  settings' search is now a field at the top of the list, no longer a row like the others. For plugin authors:
  `SearchField::new(hint).text(words).focused(focused).draw(ui, rect)`, `SearchField::height(style)`; a new type,
  so plugins built before are not affected.
- Devices (#96): a developer setting *Stay awake on USB* (X4, X4 Pro, X4 Classic, X3; off by default). While the cable is in,
  the sleep after *Sleep after* does not come, and *Sleep* shows the sleep picture but does not sleep, so the computer
  keeps the link for remote tests; the next press or command wakes it, and a cable pulled meanwhile puts the device
  to sleep (two seconds after, so a wiggled plug does not). On the X4 the cable is read at its pin; on the X4 Pro, X4
  Classic and X3 "on USB" means a computer talking to it or charging, so on a wall charger with a full battery the
  setting does not hold. No change for plugins.

### Changed

- **X4 Pro: a real sleep before it switches off** (#153). Sleep (the power button, the power menu, the idle time) used to
  switch the Pro off at once, as the X4 did before 0.5. Now the Pro sleeps lightly as the X4 does: the screen stays, a press of
  the power button wakes it at once, and it switches off after the time of the new setting *Switch off after sleeping* (30
  minutes by default; *At once* gives the old behaviour; *Never* stays asleep). The front light is off while it sleeps and comes
  back as set when it wakes. Every 10 minutes it wakes for a moment to look at the battery (3 % or less: off at once). The
  X4 Classic keeps its deep sleep. Not measured on a device yet.
- **The update check says how long the device is in use** (#143): next to what started the check, it sends `age`,
  `new` (under seven days since the first check), `week` (up to thirty days) or `month`, so that the store's
  statistics can count only devices in use for a while. The device keeps the date of its first check on the card
  (`first-check.txt`, never sent); without a clock set, no age is sent.
- **Settings screens skip a pass that counted pictures** (#118). On a black-and-white panel every whole-screen frame
  drew the screen once more, aside, to count its pictures; the settings screens have none, so they now say so
  (`Screen::pictures_in_grays`) and the pass is gone (about 4% of the work of opening a group and going back, on the
  host). Not the whole lag: the first open of a group in a session still shows the busy mark.
- **Opening a settings group does less work** (#118). The page asked the settings about every registered setting several times
  (making the setting's group each time); it now asks once. The key handler of Select falls by about a third on the host.
- The plugin interface's gate (ADR 0026, #94): `interface.txt` keeps the fingerprint the bless was made with, and
  a bless of a change plugins rely on is refused unless the interface rises above the one that fingerprint was made
  at, whatever number `interface.txt` says; layouts of what crosses that changed while the shapes did not are such a
  change. The first bless of the interface-6 round kept interface 5 that way and its fingerprint with it (#363). The
  fingerprint is as it was (X4: `00063e32940609b9`).
- The first walk of new books reads no covers (#398): a book's cover is read from it when a view first shows it (the
  library's list and grid, its book sheet, the home screen's books, the sleep screen), one book at a time, the books
  shown first, none begun while keys come (a pause of 0.6 s after the last); a book never shown never costs its cover.
  Until then the book shows as one without a cover does (title and author). Plugins see such a book not yet indexed
  (`Book::indexed` off, its title and author there) and read its cover with `library::index`, as for any book not
  indexed. On the bench's card the first walk of 1,000 books that all have covers writes 26,952 blocks instead of
  139,557 and reads 75,974 instead of 152,975, its peak 26.2 KiB instead of 70.0 KiB.
- **Less waiting for the busy mark** (#118). Going back no longer shows the busy mark the first time (the screen
  below is there already); what was timed is kept across starts in `.paperpaw/core/timing.txt` (written when the
  device sleeps or stops, only where it changed what is slow), so on the X4, where every wake is a start, a screen
  that opened quickly before is not marked again; and the mark shows only for work that took as long as the
  refresh showing it (500 ms, `KernelBuilder::busy_mark_after`, 400 before). A damaged or older file holds no
  times. On the X4 a needless mark cost ~330 ms.
- **The store installs what a plugin requires** (#97): after the store installs a plugin whose `plugin.ini` names
  `requires` (the Arabic, Persian and Urdu menus need their letters, Korean extra needs Korean), the plugins the
  device lacks are offered in the store's dialog with their sizes (*Download* or *Not now*). The plugin stays
  installed either way. A chain asks step by step; what is on the device or not in the library is passed over.

### Fixed

- **X4, X3: the 15-minute watchdog of a light sleep counts in the sleep** (#153). The reset that ends a sleep which never
  ends was paused in light sleep after the first sleep (the library's enable call set the pause again). It is now cleared
  after every arming.
- **The reader's menu shows the page and the pages** (#117). Opened right after a book opens, before its pages were
  counted, the menu bar showed a percentage ("0 %") for as long as it was open, as the count waited for it to close.
  The count now goes on below the menu, and the bar shows "2 / 185" once it ends.
- Tap zones (thirds, corners, the zones of key maps) are measured on the part of the screen the screen has, as
  the finger's point always was: on touch devices with the row of button hints (the X4 Pro, the emulator) the
  borders of the lower zones lay about 3 mm off, by part of the row's height (#88).
- A damaged folder on the memory card (entries with names no file system gives, as a reset can leave) can be removed
  again, and the folders above it (#388, #99): the folder is cut off where its parent names it, and nothing in it is
  written, followed or freed, the entries that look sane included (its space may hold another file's data; the space
  comes back when the card is checked on a computer). Before, removing the folder, or a book's cache in it, failed
  with "invalid path". Folders of names PaperPaw does not take (made elsewhere: `con`) are removed with all they hold.
- A book whose cache folder cannot be cleared (a damaged folder, #99) is never read from: the book shows its details,
  and opening it says the card could not be written, instead of reading another book's chapters as its own.
- **The library no longer says "Preparing the library …" for ever** (#145). If the kernel makes no catalog or
  sorted books for 90 seconds (a card taken out and put back while PaperPaw ran, sorted books that cannot be
  read), the library says that the folder cannot be read, and asks the kernel once more. Files plugin 1.0.5.
- **A memory card taken out with the library open is said at once** (#145). Turning a page then said "Preparing the
  library …" until the card was back: the kernel makes nothing of a card it cannot read, so no word of it came. The
  library now says that the folder cannot be read as soon as it finds the card cannot be read (*Restart PaperPaw*
  where the X4 gave the card up for the run), and shows the books again once the card is back. Files plugin 1.0.6.
- **No first-run hints over a card that cannot be read** (#145). With the card taken out, the hints counted as never
  shown and opened over the library as on a new card; they now count as shown where what was shown cannot be read.
  Only a card that has no record of them shows them. Hints plugin 1.0.3.
- **Device check, first page** (#121): the Screen page opened with the focus on *Pass*, the answer it suggests without measuring anything, so the first arrow went round to *Flip pattern* and a first Select passed the page unseen. It now opens on *Flip pattern*, the first tool, as pages with tools should, and *Pass* stays the answer shown filled. Also, a key on that page took seconds, or showed nothing: the page counted as best in grays, which on the X4 makes every change, a moved focus too, a refresh of all the screen in its four grays, and after it a small change is not shown at all until a bigger one comes. The page now shows its stripes in grays when it opens and after *Full refresh*; moving the focus or flipping the pattern refreshes only what changed, in black and white.
- **Device check, Buttons page** (#121): the page took every button from the moment it opened, so on a device of buttons alone the arrows, Select and Back did nothing and there was no way to move on or leave. It now opens with *Start* like the other pages' controls: the arrows, Select and Back work as everywhere until *Start* is chosen; then every button counts as the one asked for, as before.
- Setup: the 'Look for updates' step shows its label next to the switch again (#132).
- Updates: when some plugins did not come along with an update, one short notice says so (not one for each), and they are fetched again at the next update checks (#141).
- **"Switch off" in the power menu leaves a picture on the screen** (#146). It switched off with the last page still showing, so the
  device looked frozen. Switching off and sleeping from the power menu now show the sleep screen first, as the power
  button's short press does. Where the sleep screen is set to show the page as it is, or the page with a picture, switching
  off shows the logo with "Switched off" and how to switch on instead (English and German).
- **The plugin store shows all the plugins of this PaperPaw** (#148). It now tells the plugin library which PaperPaw runs
  (`abi`, the fingerprint) in its list and page requests; without it the library answered with the builds of PaperPaw
  0.5, which this one drops, so only a few code plugins were listed. A plugin built for an older interface than the
  device runs is offered as an update where the library has a build for this one, whatever the version says; one the
  library has no build of yet is listed as "Not available yet", not left out. Store 1.0.5.
- **Language packs have a group of their own** (#148): *Languages* (*Sprachen*), before *System*. Readers that do not
  know a group show it under *System*; a group this store does not know can no longer cause an error.
- **A book that needs more letters shows the download** (#147). After *Download* a screen shows what is fetched, a bar
  with the percent and the bytes so far, and how it ends: it closes and says *Ready* (the book shows its letters, no
  restart: the packs of letters are data plugins that work at once), or it says what did not come (no connection, no
  room) with *Try again* and *Close*, or *Restart now* / *Later* for a pack that needs one. Leaving the screen lets the
  download go on. At the largest text the hint keeps all its lines and the percent stays clear of it. Store 1.0.5 (the same as #148).

## [0.6.0-beta.1] - 2026-11-03

**Plugins of this release must be built from `965d866b` or later** (plugin interface 6, #363; `965d866b` raises the shared interface family `ui` to revision 2, #119: the firmware refuses plugins of revision 1, built before). 0.6.0-beta.1 takes
the interface-6 round too (TLS sockets, screens that fit another size, libraries of any size), so plugins change
once: from interface 4 of 0.5.0-beta.12 to interface 6. Interface 6 was never released; plugins built for it (the
earlier release candidate's store files, test cards and Workshop builds) are refused as built for an older
interface, and are thrown away.

Every plugin is built anew for interface 6, so the split of the library's folders (#280) needs no rule of its own any more: a build
from before the split is refused with the rest.

### In plain words

This is a beta. Keep a copy of your books. The details for plugin authors follow in the sections below.

- **Per-book data lies elsewhere now** (#280). What PaperPaw keeps for each book (its details, its small cover, the
  caches of the format plugins) is spread over 256 subfolders by the first two digits of the book's key, no longer
  in one folder of thousands. A big library opens much faster on a slow card. After the update PaperPaw moves the
  old data once, a few books at a time at calm moments; each book's first opening takes a little longer, as its
  cache is made again. **Plugins that read the library have to be built again for it**: calibre, end-of-book,
  file-manager, files, the format plugins (EPUB, FB2, HTML, Markdown, CBZ, DOCX, MOBI, TXT, XTC, images), home,
  notes-overview, opds, sleep, stats, transfer and widgets. PaperPaw's own are rebuilt for this release; a plugin
  from someone else that reads the library needs its author's new build, and until then shows books without their
  details.
- **The room hint after an SD install.** After an installation from the SD card (as from CrossPoint) there is less
  room for plugins than a full flash gives. The store and the plugin manager now say how much room is left and
  that *A full install via USB (paperpaw.org/flash) gives about …* more; on a fully flashed device they say nothing.
- **Neutral plugin names** (#90). Plugins that work with a third-party product have a generic name; the product is
  named only in the description, as what they work with: *Wireless library* (calibre), *Book notes sync* (Joplin),
  *Highlights sharing* (KOReader), *Highlights export (cloud)* (Readwise), *Read-later articles (own server)*
  (wallabag), *Read-later articles (cloud)* (new id `read-later-cloud`, off at first), *Encyclopedia lookup*
  (Wikipedia), *DOCX documents*, *MOBI books*. The ids of the plugins already published stay.
- **Arabic user interface** (#33). The screens run right to left in Arabic: the switch, the rows, the keyboard, the
  library, the reader's menu, the sliders and the taps and swipes at the sides. The translation comes as the data
  plugin `language-ar` from the store. Nobody on the team reads Arabic: the rules are tested and the pictures
  checked, how it looks to a reader and how it feels on a device are not. Tell us what is wrong.
- **A wider keyboard** (#91). The keys reach nearer to the sides of the screen; on small screens they get nearly the
  whole width.
- **The landscape home screen** (#88). Turned on its side, the home screen keeps the start card beside the clock. On a wide
  screen it shows the same items with more columns, each page filled before the next, and cards no wider than about
  110 mm; arranging it on its side keeps that layout for that orientation, and *Reset landscape layout* (in Add)
  goes back. The SDK gets `grid::refit` for it.
- **A power menu** (#78). Hold the power button: *Sleep*, *Switch off*, *Restart* and, on touch devices, *Lock touch*
  or *Unlock touch*. A short press still sleeps.
- **All six language packs are in the store**: Arabic, Hebrew, Greek, Cyrillic, Korean (2,350 syllables) and
  *Korean, more syllables* (about 30 MB on the card). The Arabic, Hebrew and Korean packs bring their keyboards
  (Hangul letters join to syllables as you type).
- Also new: *Literata bold and italic* come as a small pack from the store (531 KB less firmware, more room for
  plugins), *All notes*, *End of book*, *Pictures* and the formats MOBI and DOCX, flash cards from the card and from
  Anki decks, your own fonts, hanging punctuation, drop capitals, lists that hang under their marker, line breaking for
  Korean, Chinese and Japanese. Book text in Arabic and Hebrew is set right to left.
- **Libraries of any size** (#79). The X4 no longer stops at 2,000 books: the library lists every book, paged from a sorted
  list PaperPaw keeps on the card (a few kilobytes of memory however many books; 10,000 books were tested on a
  computer). Right after the catalog is made anew the list says *Preparing the library …* for a moment. The shelves
  (authors, series, tags, and now folders) show 200 names at a time and go on when the focus moves past the last. A start
  after nothing changed writes next to nothing to the card, and big folders are read about twice, not once per piece.
  The first walk of new books writes about a sixth of what it did (#98), because an EPUB is opened for its details only.
- **Read while the library is collected** (#108). The first time the library is collected (thousands of books copied
  onto a new card), the books found so far are listed after the first 20, then each time they are twice as many, and
  every two minutes at least: they can be opened and read at once (the collecting waits meanwhile and goes on when the
  book is closed). The count says *1,240 books so far …* until all are found.
- **The quick panel starts at the top edge** (#120). It no longer leaves a band above it, which showed what was
  below: over a book the first line of the page, cut. Over the home screen it now covers the status line while it is
  open, as it covers the rest.
- **Look for new books** (#109). An empty library has a button that looks for books again, and the library's menu has
  the same entry: for books copied onto the card while PaperPaw ran. While it looks, the library says it collects the
  books and shows them as they are found ("N books so far"), as on the first start; pressing it again lets the search
  go on. **After swapping the SD card, restart the reader**: PaperPaw reads the card it started with.
- **Arrow keys stay on the page** (#112), a new setting under *Controls*, off by default. On, the arrow keys go
  round on the page shown: back on its first entry goes to its last, on from its last to its first (on a short last
  page among the entries it has); only the page buttons turn pages. In grids (covers, the home screen's tools and
  widgets), up and down go round in the column: up from the top row to the lowest entry below, down from the bottom
  row to the top. Off, as before, they go round the whole list.
  The library's Find screen follows it too: up and down (and the page buttons) go round the search and the letters in
  the column the focus left.
  For the lists of PaperPaw's own plugins; store plugins follow once they are built again.
- **Sharpness** (#122), a new reading setting under *Reading > Display*, next to the text contrast: *Soft*, *Normal*
  (the default, drawn as before) or *Sharp*. Soft draws letters a step lighter (smooth where the screen shows grays),
  sharp a step darker and without grays on every screen; each book may have its own. A tone curve set by a theme
  (`core.text-curve`) still takes its place.
- **Sharper letters** (#122). The font packs of the default fonts are built with their letters fitted to the pixel grid
  (x-height, baseline and stems on whole pixels), so text looks even and crisp on the e-ink screen. Letter widths and
  line metrics are as before, so a book breaks into the same pages. Packs on the card (Literata styles, scripts) are
  not changed.
- **Secure connections for plugins** (#81). Plugins can open HTTPS, IMAP and SMTP connections: the X4 does the encryption
  itself (one connection at a time), the emulator and the Android app use rustls. A server whose certificate no authority
  vouches for is shown and trusted only after you say so, and then only that certificate or key. The X4 just after a
  cold start does not know the time yet and says so. The first plugins that use it are *Mail* (IMAP/SMTP with a
  password or app password, up to three accounts, home widgets) and *Notes vault* (below), both from the store.
- **Background connections** (#81). A connection that background work opens (a mail fetch) stays open until that work
  ends; it no longer closes when the screen that happened to be showing closes.
- **Notes link to notes in the reader** (#87). Markdown notes open with `[[wikilinks]]`, links between note files and
  headings (*Back* returns to the note before), embedded pictures, hidden front matter, callouts and tags. The *Notes
  vault* plugin (from the store) adds a vault view of a notes folder and six home widgets. It works with Obsidian
  vaults and does not change them.
- **Screens that follow the turn** (#88). Screens lay themselves out again when the screen turns or changes size,
  keeping the focus, instead of staying as they were at the start.
- **Smaller menu text by default** (#119). The interface text (menus, lists, settings) starts at 8 pt, on every device
  (it was 9 pt). Anyone who set another size keeps it (Settings, Look); the reading size is unchanged.
- **Search finds tags, and stays small** (#111). The library search now matches tags too, as its question says
  ("Title, author, series or tag"). A search that finds over 500 books lists the first 500 by title and says
  "First 500 of N books"; narrow it with another word. The Find screen starts on the search field, and Up from the
  first row of letters goes back to it.
- **The keyboard's Right key** (#111). With buttons, Right on *Phone* (the field's last button) goes straight to the
  first key.
- **Plugins go from interface 4 to 6 in one step.** Every plugin with code needs a new build for this release (PaperPaw's
  own are rebuilt); a plugin from someone else needs its author's.
- **Updating:** books, reading positions and settings are kept. Every book is laid out again once, the first time it
  is opened. Plugins with code need a build for plugin interface 6; the store offers PaperPaw's own.
- **Not in this release:** exFAT cards. The X4 Pro, X4 Classic and X3 are built and checked by the tests on a
  computer; the owner tests the X4 and X4 Pro by hand.

### Added

- Settings: `builtin::settings::TEXT_SHARPNESS` (`core.text-sharpness`) and `TEXT_SHARPNESS_LEVELS` (soft, normal,
  sharp; #122), registered by the kernel with the reading text's look, each book may have its own. The SDK's reading
  faces move the contrast of `TEXT_CONTRAST` a step lighter (soft) or darker and without grays (sharp); normal and a
  value it does not know draw as before, and `TEXT_CURVE` takes its place. New constants, no change of what plugins
  built before see.
- Text: `paperpaw_text::Hinting` (none, light, normal, mono; `ALL`, `parse`, `name`) and, with the `outline` feature,
  `OutlineFont::rasterize_chars(chars, size, hinting)` and `build_pack_hinted(font, sizes, chars, hinting)` (#122): packs
  made on a computer with their letters fitted to the pixel grid; advances, kerning and line metrics as unhinted, so
  books lay out the same. `build_pack` and `rasterize` stay unhinted. `paperpaw-fontgen --hinting <none|light|normal|mono>`
  (unhinted unless asked); the default fonts are built light (`PAPERPAW_FONT_HINT` to compare). New items, no change of
  what plugins built before see.
- UI: focus moves that follow *Arrow keys stay on the page* (`builtin::settings::ARROWS_STAY_ON_PAGE`,
  `core.arrows-stay-on-page`, #112): `ListView::navigate_in` and `move_focus_in`, `Rows::step_in`,
  `Sections::step_in` and `ui::step_focus_in` (for a focus of a screen's own over pages of `page` things) take the
  `Env` and go round within the page where the reader chose so, else as `navigate`, `move_focus`, `step` and
  `step_focus`, which stay as they were. In a grid `move_focus_in` takes a step of a row (as many as it has columns)
  round within the page too, in the same column. New functions and a new constant, no change of what plugins built before
  see; the SDK's lists and PaperPaw's plugins use them.
- UI: `UiStyle::flip` and `UiStyle::flip_x` are public (#33): a screen lays a row or a grid out once from the left
  and mirrors each part where the interface runs right to left, as the SDK's own parts do. New functions, no
  change of what plugins built before see; their api.txt lines are part of this release's interface 6. The library's rows and
  covers, the reader's menu bar and the home screen's drawer mirror with them now, no copies of their own.
- Text: `TextInk::RIGHT_TO_LEFT`, a flag of the reserved ones: the lines a handle draws run right to left
  (`TextInk::is_right_to_left`). The kernel sets it for the interface's font where its language runs right to left.
- **Text of several lines** (#81). The keyboard can ask for a message of several lines (`ask::text(…).multiline()`):
  it has an *Enter* key, the field shows the end of the text on as many wrapped lines as fit above the keys, and a
  text from a phone or the clipboard keeps its line breaks. (A plain question still gets one line.)
- **Mail (#81):** a plugin that reads and sends e-mail over IMAP and SMTP with a password or an app password, no
  browser login. Up to three accounts under Settings, Mail, with presets for ten providers that fill in the
  servers; the folders with how many mails wait, a folder's latest mails, a mail (text, or the text of a simple
  HTML mail; attachments are named, not fetched), *Reply*, *Reply all*, *Star*, and writing a mail on the keyboard.
  It connects, fetches and closes (the X4 holds one secure connection), keeps what it fetched on the card, and asks
  once before trusting a server no authority vouches for. Five home widgets: unread mail, latest mail, starred
  mail, one mailbox and a shortcut to write. Needs the TLS sockets of plugin interface 6; background
  fetching and a Sent copy are not in this first version.
- **Notes vault (#87):** a plugin that reads a folder of Markdown notes (works with Obsidian vaults) without
  changing it: the vault's folders as they are on the card, its latest changes, its tags and a search in the
  notes' titles, each list shown a hundred rows at a time so that a vault of thousands of notes costs a window of
  memory (5,000 notes tested), from an index kept on the card and made in the background. Six home widgets: recent
  notes, a pinned note, the note of the day, a random note, the tags and a search, each with its own settings
  (how many, which folder). Notes open in the reader: `[[wikilinks]]` (`[[note|words]]`, `[[note#heading]]`) and
  links to other note files open that note by its name, anywhere in the vault, at its heading, and *Back* returns
  to the note before; embedded pictures (`![[picture.png]]`) show, front matter is hidden (its tags are shown),
  callouts (`> [!note] Title`) are quotes with their title, `#tags` stay as written and tables are rows of cells.
  The Markdown reader's HTML anchors the headings for those links.
- **Plugin interface 6:** `library::sorted` (#79): every book of the library sorted by title on the card
  (`.paperpaw/library/sorted-<hash>.idx`, one for each archive folder a view leaves out, two at most), paged by a view
  in a few kilobytes, so the X4 lists every book instead of 2,000 at most. `Sorted::open` (`None` for a file of
  another library version or archive, cut short or damaged: a view asks for it with `sorted::request` and says the
  library is being prepared), `len`/`page`/`find` for a `View` (a filter, the books read last first or by title,
  series stacked; as `Index::order` and `Index::stacked` list them), `next_in_series`, `refresh`, `hide`/`hide_all`;
  the event `SORTED_WANTED`. The kernel makes the file (kernel feature: `sorted::Build`, a bounded merge sort of runs
  of 512 books, written beside it and put in place whole; `sorted::wanted`, `asked`, `kept_archives`, `clean_up`).
- **Plugin interface 6:** `Storage::list_from` lists a folder a piece at a time, each piece going on
  where the last one stopped (#79): a folder of thousands of entries is read about twice in all, not once a
  piece. Storages that cannot go on from a place count, as `paperpaw_platform::list_counted` does (the item's
  default), so every storage keeps working; the memory card's (FAT) goes on from where the entry begins and
  finds a place taken by another entry by its count. The catalog lists the library's folders so: the walk after
  a start reads a folder of 4,000 books (1,254 blocks) in 2,535 blocks instead of 66,460 (bench; the whole walk
  4,367 instead of 68,291).
- **Plugin interface 6:** TLS for plugins' sockets (#81), as a mail plugin needs it (IMAP, SMTP):
  `Command::connect_tls` is secure from the start, `Command::start_tls` switches an open socket (STARTTLS) once
  everything the server sent was read, else it fails and closes. Nothing is sent or read until it is secure: a send or
  read before fails `NotReady` and closes the socket. `Tls` sets it up: the authorities PaperPaw ships (`Tls::new`),
  or up to `MAX_PINS` pinned certificates (`Tls::pinned`, `CertificateHash` as browsers and `openssl` write it) or
  keys (`Tls::pinned_keys`, `KeyHash` of the SubjectPublicKeyInfo: renewals of the same key stay trusted), the name
  checked, the time to connect, the time idle before it closes. `SocketNotice::Secured` and `Untrusted` (reserved
  before) carry the server's certificate and key, `Untrusted` why (`Distrust`); `Secured::time_checked` is false
  where the device did not know the time yet, so the certificate's dates were not checked (an X4 just after a cold
  start). `SocketNotice::Failed` says why it failed (`SocketError`; `Other(0)` from older kernels).
  `SocketRead::Bytes` reads bytes as they come (`SocketNotice::Bytes`), for 8-bit protocol lines that are no
  UTF-8. `Capabilities::tls_sessions` says how many TLS
  connections fit at once, HTTPS among them; with none (the browser's emulator) TLS sockets fail at once,
  `SocketError::Unsupported`.
- **Sockets of background work are the run's** (#81, owner decision 52): a socket, plain or TLS, that a work's event
  handlers or a job's step open no longer closes with the screen that was showing then (a run is no screen's: a
  mail fetch begun while a book was open lost its connection when the book closed). It closes when the run ends:
  the job done or failed, the work run again (an event's run ends with the next), taken back or refused, or its
  plugin gone. Only the work's own plugin's sockets count; one connected from a socket's event is the screens'
  (several connections are a job's steps). A secure one ends when idle: `idle_ms(0)` is refused for it
  (`SocketError::Options`); `MAX_TLS_IDLE_MS` is 30 minutes (was 10), so that waiting for new mail (IMAP IDLE)
  keeps it open. Sockets a plugin opens for the reader still close with their screen.
- **Plugin interface 6:** screens fit another size in place (#88). `Screen::on_resize` lays a screen out
  anew when its part of the screen changes size (turned, a window resized, the row of hints): answering
  `Handled::Yes` it keeps everything (a book open, a list scrolled, a connection); by default it is made anew as
  before. The old screen is now dropped before the new one is made, so a turn never holds both in memory.
  `Screen::pane` says `Pane::DETAIL` for a screen that shows one thing of the list below it, to show beside it on
  wide screens (the placement comes later); `Pane` is a number with constants, as `Instances`, and a value a kernel
  does not know shows as `Pane::FULL`. `Screen` grows at its end from now on: `Screen::revision` (never
  implemented by plugins) says which items a screen has, and newer ones are called only through
  `call_since`, so later screen items need no raise.
- **X4, X4 Pro and X4 Classic: TLS for plugins' sockets** (#81): a socket made secure from the start or switched
  to it (STARTTLS) is an mbedTLS session over the same connection HTTPS uses, the authorities of the firmware
  (Mozilla's) or up to four pinned certificates or keys (the server's own, by the SHA-256 of its DER or of its
  public key), the name asked for (SNI) and checked, the dates checked once the time is known (`Secured` says
  whether they were). The X4 holds one TLS session at a time and the X4 Pro and
  Classic two, shared with HTTPS and served in the order asked: a socket that waits longer than its timeout fails
  `Busy`. Nothing is sent or read before `Secured`; a switch to TLS over bytes that came after the server's
  go-ahead fails (`NotReady`, socket closed), as they might be injected.
- **Emulator on Linux, macOS and Windows, and the Android app: TLS for plugins' sockets** (#81, owner decision 52):
  rustls with ring, over the same sockets, with the same rules as the X4's. The authorities are Mozilla's list
  (`webpki-roots`, the same as the X4's, not the computer's store, so a plugin behaves alike everywhere), or up to
  four pinned certificates or keys; the name is checked for the authorities only, the dates (where the clock
  is believed, i.e. set after 2025) for both; there is no way to trust what is merely shown. A server's own
  certificate, a certificate authority's that it made itself, ends `Untrusted` (`UnknownIssuer`), so a plugin can
  offer to pin its key. Up to eight sessions at once, a socket waits for one until its timeout; a write that moves nothing for 30 s ends the socket, `Timeout`, so a server that never reads cannot hold a session. The Mail plugin
  checks and sends mail against a real IMAP and SMTP server this way. The browser's emulator has no sockets and
  holds no TLS session.
- Screens mirror in a user-interface language that runs right to left (#33): the switch, value rows and their
  chevrons, ticked rows, the number buttons, text fields, dialog buttons, list rows' icons, chips, the date
  and time pickers, headings and the reader's menu bar start at the right, and left/right follow them. The
  shared UI functions changed for this: `switch_rect` is the one whose partner stays a copy in plugins
  (ADR 0026 case 3), which the interface raise of this release makes moot, as every plugin is rebuilt.
- A book in Greek, Cyrillic, Arabic or Hebrew whose letters the reading font lacks names the pack that brings
  them, once for each script (to become one dialog with *Download* once for each book).
- Keyboards for Arabic, Persian, Urdu and Hebrew come with their letters packs (`script-arabic`,
  `script-hebrew`, 1.1.0), and a two-set Korean keyboard with the Korean pack: letters typed join to syllables
  (jamo), *Delete* takes back one jamo and *Shift* types the doubled letters (#61, keyboard 1.0.2).
- SDK: `library::kept_path(folder, prefix, book)`: where a plugin keeps what it keeps for a book in a folder of
  book data, in the part of the folder of the key's first two digits (`<folder>/<2 hex>/<prefix><key>`), so that
  no folder holds thousands of entries. The format plugins (EPUB, FB2, HTML, Markdown, CBZ, DOCX, MOBI, TXT, XTC,
  images) keep their caches so.
- SDK (document): a table cell can span rows (`TableCell::rows`) and say where its lines sit
  (`TableCell::vertical`, `VerticalAlign`); `TableRow`, `TableCell` and `MAX_COLUMNS` are in `paperpaw_sdk::document`
  for format plugins.
- SDK: `Command::RequestPlugins` (`Command::request_plugins`): any plugin can ask for plugins by id (e.g. those
  a template names); those not on the device go to the store as `events::PLUGINS_REQUESTED`
  (`PluginsRequested`, at most `MAX_REQUESTED_PLUGINS`), which asks the reader before it downloads any. What
  came of it reaches the plugin that asked as `events::PLUGINS_ANSWERED` (`PluginsAnswered`: installed,
  missing, declined, an error to show), from the store, or from the kernel where all were there already or
  no plugin installs plugins.
- SDK: `Settings::default_from` names the plugin whose default a setting has in place of its own (a template's
  `settings.ini`, a theme's), so a settings page can say where a value comes from.
- Font packs (`.ppfp`) can lie loose in a shell's font folder, without a `plugin.ini`: each is a face of the
  family its file name gives (`Literata-BoldItalic.ppfp`, `Bookerly.ppfp`). A device without scalable fonts
  leaves OpenType/TrueType files there alone (no warning for each); hidden files (`._…`) are passed over.
- SDK: `Fonts::missing_letters` (and `FontSet::missing_letters`) counts the letters drawn that no font has, so
  a screen can tell that a page lacked letters (e.g. to offer the font pack with them).
- SDK (document, layout VERSION 11): a run's style can carry the book's letter spacing
  (`TextStyle::letter_spacing`, in fortieths of an em, `LETTER_SPACING_STEPS`), a line over the text
  (`TextStyle::OVERLINE`) and how its lines are drawn (`TextStyle::line_style`, `LineStyle`: one, two, dotted,
  wavy); the reading faces measure, lay out and draw them. A drop capital's line is marked
  (`LineBidi::DROP_CAP`).
- Numbers in messages are written with the digits a language's catalog names (`[meta] digits`, e.g. `arabext`
  for Persian, `arab` for Arabic-Indic). SDK: `Translator::digits`, `Translator::number`,
  `Translator::localize_digits` for numbers a plugin writes itself.
  What the SDK writes for showing uses them too: the clock (`time::clock_of`, `clock_text`),
  `time::duration_text`, the battery percent in the status line, a number setting's value
  (`ui::setting_value`) and the date and time pickers. New: `files::shown_size(env, bytes)`, a size
  with the language's decimal mark and digits (`files::size` stays Latin, with the mark given).
- SDK: `Slider::mirrored` for a right-to-left interface: the lowest value at the right, the track filled from
  there, `−` at the right and `+` at the left (`Slider::parts`), the left button moving it up
  (`Slider::key_step`); `SliderPart::mirrored` tells a theme's painter.
- SDK, reserved for features known to come, so they need no new plugin interface (no effect yet):
  `Command::PlaySound` (`sound::Sound`, reply `replies::Played`, answered at once, not played) and
  `Command::StopSound`; `Capability::CHORDS` (key maps may bind `chord.<button>_<button>`, read and kept),
  `Capability::PAGE_TURNER`, `Capability::HOTSPOT`; `paperpaw_table::Family::CORE` for the firmware's own
  `core`; a spare word (zero) in `Manifest`, `ScreenSpec`, `FixedScreen`, `WidgetSpec`, `FixedWidget` and
  `StatusItemSpec` for flags later releases give a meaning with setters of their own.
- SDK: room for later questions without a new plugin interface: `KeyBindings::extended`,
  `ScreenMaker::extended` and `Typesetter::extended` (with `paperpaw_layout::TypesetKey`) answer `None`
  by default; none is asked yet (`docs/sdk-design.md`, rule 17).
- In a right-to-left interface (Arabic), taps at the sides and swipes go the other way on every screen, as
  they do in a book whose pages go right to left. SDK: `Screen::direction` (a `lang::Direction`) replaces
  `Screen::mirrored`: `Auto` by default (as the interface goes), `Ltr` or `Rtl` for a screen with a way of
  its own; the reader gives its book's, so a left-to-right book keeps its sides in Arabic. **Breaking** for
  screens that implemented `mirrored`.
- Korean written in jamo (Unicode's decomposed form, NFD, as some books and Mac file names have it) shows as
  its syllables wherever a font holds them, in books and in the interface, instead of loose jamo or question
  marks; letters so written no longer count as missing. `paperpaw_text::has_jamo` tells such text (#61).
- SDK: `CellGrid::mirrored` for a right-to-left interface: rows of cells start at the right, taps find them
  there, and the left button goes on to the next cell.
- SDK: rows moved by dragging have their grip at the left in a right-to-left interface.
  `Reorder::grip` and `Reorder::content` take the `UiStyle`, and `Reorder::draw` takes `&mut self`
  (it notes the direction for the pointer samples that follow): **breaking** for plugins that move rows.
- A language can come compiled from the card and be read as it is needed (ADR 0033): a data plugin's
  `i18n/<lang>.ppcat` (made with `paperpaw-sign catalog <lang>.ini <lang>.ppcat`, data formats 4) keeps its
  messages on the card, about a kilobyte of memory for 3,400 messages, and the messages of the screen shown;
  an Arabic interface of all plugins (some 210 KB) fits the X4. A pulled card or a damaged file falls back to
  English. SDK: `Catalog::compile`, `Catalog::open`, `CatalogFile`, `Translator::forget_read`,
  `Registrar::add_translation_file`; plugin interface 6 is written anew for it (every plugin is built again),
  `DATA_FORMATS` is 4.
- When the library's folder cannot be read (a failing card, one the device cannot use), the library says
  *Couldn’t read the books folder* instead of collecting books for ever or saying there are none; PaperPaw tries
  again by itself. SDK: `events::CATALOG_FAILED`.
- SDK: `WifiState::Online` says the name the device answers to in the network (`name`, e.g. `paperpaw.local`,
  where the shell gives one), and `WifiState::reach` gives the name or else the address for a page to show.
- SDK: `HttpRequest::max_file` caps what one download (`save_to`) may write; a longer answer fails and nothing
  is kept, so a plugin's download cannot fill the card (at most 64 MiB, as before).
- The store and the plugin manager name the web flasher for more plugin room only where the X4's shell says a
  full flash gives more (a smaller layout than PaperPaw's own, as after an SD install from CrossPoint), no longer
  guessed from the room's size. SDK: `CodeRoom::larger`, `CodeRoom::in_slot` (kernel feature).
- *Automatic* power wears *Saving* while the system's own battery saver is on (where the shell reports it). SDK:
  `Capabilities::power` (`PowerControls`: what the shell changes by the level, so settings show only what
  applies), `KernelBuilder::power`, `Battery::saver`.
- SDK: `PluginUsage::shown_ms`, `read`, `written` and `awake_ms` are `u32` (they stop at their largest value):
  a row is kept for every plugin, about 1.3 KiB less on the X4.
- SDK: `Str`, text kept in the program or owned in two words (a `Cow<'static, str>` takes three);
  `Manifest::name`/`description` and `SettingSpec::title`/`description` are `Str`, as the kernel keeps them for
  every plugin and setting (16 bytes less per plugin on 64-bit, 8 on the X4).
- SDK: widgets kept in the program, `FixedWidget` and `Registrar::add_widgets` (a `static` list, two words of
  memory for the list where each `WidgetSpec` takes about a hundred bytes); `Drawn::new`, `shows` and `targets`
  are `const`. Home, stats, vocabulary and six of the widgets plugin's widgets use them.
- SDK: subscriptions kept in the program, `FixedSubscription`, `Registrar::add_subscriptions` and `hears!` (a
  typed handler, as `Registrar::on` takes it, in a `static` list: one entry of the kernel's for the list);
  `Event::hear`, `Event::id_static`. The update plugin's seven subscriptions use them.
- SDK: `Document::prepare_image_larger` decodes a picture anew, larger than for its page (its longer side up to
  1600 pixels), for a zoom or a picture on the whole screen; EPUB books do, other formats say they cannot.
- A plugin on the card can name the plugins it needs: `requires` in `plugin.ini` (data formats 4), e.g. a setup
  template. SDK: `requires::parse`, `requires::of`, `requires::missing`, `Requirement`.
- SDK: hooks for showing the card to a computer as a USB drive (the X4 Pro's hardware can):
  `Capability::USB_DRIVE`, `Command::UsbDrive`, `events::USB_DRIVE_ENDED`; kernel: `Effects::usb_drive`,
  `Kernel::usb_drive_ended`, `KernelBuilder::capability`. No shell offers it yet.
- In *Offline* a plugin can offer *Go online* outside its network screens too (a widget's *Update now*): SDK
  `Command::GoOnlineFor` (the network for a while, at most `power::MAX_ONLINE_FOR`, ten minutes; held work runs
  as in Saving), taken only from the reader's input.
- Author keys: a recovery key chosen in advance moves a plugin to a new key by a statement the device checks
  itself (`paperpaw-sign rotation`; SDK `native::keys`: `remember_first_install`, `rotation_message`,
  `accept_rotation`; `install::Marks::recovery`), and the code of an author whose key the store revoked does not
  run (`PluginProblem::Revoked`, `keys::keep_revoked`, which only adds). A revocation is a statement
  (`keys::Revocation`, `paperpaw-sign revocation`) signed by the revoked key itself or by the plugin's recovery
  key, about the key pinned as its author on the device; any other is left out, and the list is checked again where
  it is read, so no taken-over library can revoke another author's key (installed plugins are trusted: they have the card). A revocation stops that plugin
  alone (`keys::is_revoked(storage, plugin)`), not others the same key signed. The recovery key is pinned only with
  the first install, and code installed before a rotation does not run until an update by the new key: install marks
  never move a known author to another key, and name code as installed only where the author pinned then signed it. The store
  does not offer either yet; it calls `keys::keep_revoked` with its library's revocations and `keys::accept_rotation`
  with a statement that came with an update (both plugin-facing; the recovery key arrives with the install marks;
  `keys::check_rotation` says beforehand, without writing, whether a rotation would be taken).
- Reserved for devices and plugins to come, so they need no new interface: a pen above the screen
  (`PointerPhase::Hover`, which the kernel passes to no screen yet) and how it leans (`PointerEvent::tilt_x`,
  `tilt_y`, `with_tilt`; `Capability::PEN_HOVER`, `PEN_TILT`); TLS sockets (`SocketNotice::Secured`, `Untrusted`
  with the certificate's SHA-256; `socket_params::SECURED`, `UNTRUSTED`, `CERTIFICATE`), whose commands will come
  as `Command::Extended` (#81). The layout digest now also holds where `PointerEvent`'s fields lie (its size hid
  the new ones), so the plugin fingerprint changed: build plugins from the baseline below or later.
- Layout `VERSION` 11: books cached before are laid out anew once, so drop capitals and the new list markers show
  in them too. SDK, for real tables: `BlockKind::TableRow`, `Block::table` (`TableRow`, `TableCell`, `MAX_COLUMNS`:
  a row's cells, the table's column weights, header, first and last row) and `Page::rules` (`PageRule`: thin lines
  such as a table's borders). Formats and the layout do not fill them yet: rows are still set as one line.
- The start asks for the plugin list and the translations' list once at their size (SDK, kernel feature:
  `Plugins::reserve`, `Translator::reserve`), so they do not double while plugins register.
- The English catalogs of the plugins built in are one catalog, indexed together when PaperPaw is built: the
  translator keeps a table of their sources rather than a catalog for each. SDK: `BuiltCatalogs`, `BuiltTable`,
  `Translator::built_catalogs`; kernel: `KernelBuilder::built_catalogs`; `paperpaw_app::BUILT_CATALOGS`.
- A power menu: hold the power button and a small menu opens with *Sleep*, *Switch off* (where PaperPaw can switch
  the device off), *Restart* and, on touch devices, *Lock touch* or *Unlock touch* (power 1.1.0). A short press
  still sleeps, now when the button is let go. The touch lock no longer binds the held power button itself (so the
  menu can open while touch is locked); it keeps the held home key and the two taps with a hold, and its hint says
  to choose *Unlock touch* in the power menu (lock 1.0.3). No SDK change.
- *Lock after waking up* (the screen lock's setting, on by default) is no longer an advanced setting: off, the lock
  comes only after a real start (power on or restart), not after sleep.
- *All notes*, a new plugin (Tools): the books that have bookmarks or highlights with how many, a book's notes
  (a note opens the book there), and *Export as My Clippings.txt*, all notes in one file in Kindle's form
  (UTF-8 with a byte order mark, `Exports/My Clippings.txt` beside the library) that notes apps read. A book's
  notes file is found by the hash of its path, so listing needs no book to be opened. It needs no SDK change.
- *MOBI books* (`format-mobi`), a new format plugin: opens `.mobi` and `.azw` books without DRM (PalmDOC or uncompressed
  text, Windows-1252 or UTF-8). It shows the title, authors, language, subjects, the cover, headings with a table of
  contents, bold and italic, centred and right-aligned text, lists and pictures (GIF, JPEG, PNG, decoded when
  their page shows). It reads the book in pieces, so a long book needs little memory. Books with DRM, with
  Huffman/CDIC compression or in the KF8-only form (`.azw3`) are refused with a clear message. It needs no SDK change.
- *Read-later articles (cloud)* (`read-later-cloud`, #90), a new plugin (off at first): fetches the articles saved in the Readwise Reader service (from *Later* or
  the *Inbox*, a setting) as HTML books into the folder `Reader`, with their text (no pictures), ten new ones
  at a time. It asks for the Readwise access token once; articles read to the end are archived in Reader with
  the next fetch and their books removed (not those with highlights, notes or bookmarks). It keeps within
  Reader's limit of twenty list requests a minute and stops where Reader asks to wait. It needs no SDK change.
- *DOCX documents*, a new store plugin (`format-docx`): opens `.docx` files as books, with headings and a table
  of contents, lists (bulleted, numbered, nested), tables (a row a line), bold, italic, underline, links (also
  within the document) and pictures (JPEG, PNG, GIF), and the title, author and language of the document. The
  text is read in pieces from the archive and written once as blocks, so a long document needs little memory;
  pictures are taken out when their page shows. Footnotes, headers and footers, and comments are not shown.
  It needs no SDK change and is not part of the firmware, so only readers who want it pay for it.
- *Pictures*, a new store plugin (`format-images`): a JPEG, PNG or GIF opens from the library as a book, one
  picture to a page, fit to the screen; the pictures after it in its folder (in the order of their names, 2
  before 10) are the pages after it, up to 300. Each picture is decoded when its page shows, so one is in
  memory at a time; a file that is no picture or too large shows its name. It needs no SDK change and is not
  part of the firmware.
- *Plugin layout and best practices* (`docs/plugin-conventions.md`): the folders of a plugin, names, where pictures and other assets go (on the card, not in the code), translations, data a plugin writes, and the budgets. `paperpaw-dev package` warns where a data plugin's folder or a code plugin's source (`paperpaw-dev package path/to/crate`) does not keep them. It needs no SDK change.
- *End of book*, a new plugin: when the last page of a book is reached, a screen offers the next book of its
  series (or the library), a rating of one to five (`4 / 5`, kept as a book field and shown as a badge on the
  book's row and on its information page) and whether the book counts as finished. Two settings in the reading
  group: *Show a screen at the end of a book* and *Mark the book as finished* (both on). It needs no SDK change.
- Flash cards from the SD card (vocabulary 1.1.0): every CSV or TSV file in `/Flashcards/` is a deck named after
  the file, and *Cards on the SD card* updates the decks from their files (cards match by an id column where the
  file has a `#paperpaw:` line, else by their front; cards deleted from the file stay unless the deck's *Remove
  missing cards* switch is on), or saves a deck back to its file (written to a `.part` file, the old one kept as
  `_<name>.bak.<ext>`, a change made on the computer in between merged first). Decks also come in through *Edit
  with your phone* (CSV, TSV, TXT or Anki `.apkg`), which the page, the help and the empty state now explain, and
  with errors that name the problem: a wrong file type, a file that is not UTF-8 or has no cards, a full deck.
  Files over 4 MiB or 20,000 lines are refused and a card holds at most 500 characters. No SDK change.
- Flash cards from Anki decks on the card (#49, part 3): an Anki deck package (`.apkg`, exported with *Support older
  Anki versions*) in `/Flashcards/` is a deck like a CSV or TSV file: the notes' first three fields (front, back,
  example) become cards as plain text, without markup and sounds, and an update merges them as it does a text file's
  lines. Scheduling and media are not read (ticket #26, Anki's own sync, is not part of this). The deck's database is
  read page by page, so the heap needs no more than for a text file. A deck of the newest Anki format, one with
  more than 4,000 files, a database over 16 MiB or more than 20,000 notes is told, not read. An Anki file is only
  read: *Save to card* never writes over it. No SDK change.
- A release's `paperpaw-<device>-<version>-abi.txt` says the firmware's table for plugins after its
  fingerprint: `interface:`, `api:` (the table's size), `families:` and `min:` (each family's revision and the
  oldest it runs), read from the firmware's ELF file (`PAPERPAW_NATIVE_ABI`, `paperpaw-sign abi`). The store
  and the web flasher use them to refuse a plugin that needs more than a firmware has; the first line is the
  fingerprint as before.
- Your own fonts (#59): *Your own fonts*, beside *More fonts* where the reading font and the status lines' font
  are chosen, shows how to read with a font file of one's own, per device, in English and German (it works
  without a network). `paperpaw-fontgen --family "My Font" --regular a.ttf [--bold b.ttf] [--italic i.ttf]
  [--bold-italic bi.ttf] <folder>` makes the whole font plugin for the X4 and X4 Pro, which read font packs
  only: `<folder>/font-my-font` with its `plugin.ini` and a pack per face, to copy into the card's `plugins`
  folder; it prints how large each pack is and how many of the characters asked for the font has. No SDK change.
- A Greek letters pack, `script-greek` (#60): Literata's Greek, modern and with the older accents (Greek
  Extended), for books whose Greek shows as `?` on a device whose fonts hold Latin only. It is a data plugin
  that `tools/script-pack.py greek` builds, with no code and no interface change. Its entry in
  `tools/plugin-files/data.toml` is live, so the release builds it for the store (off until chosen), with the Arabic, Hebrew
  and Cyrillic packs (`script-arabic`, `script-hebrew`, `script-cyrillic`): letters for books whose scripts the built-in fonts lack.
- A Cyrillic letters pack, `script-cyrillic` (#60), the same way for Russian, Ukrainian, Bulgarian, Serbian and
  other Cyrillic books (about 1.0 MiB on the card), off until chosen.
- A fix to one of PaperPaw's own data plugins (Literata's sizes, the
  hyphenation data; `tools/plugin-files/data.toml`) reaches readers alone
  between releases, as for the plugins with code: tagged `<id>-v<version>`,
  a pre-release with its own catalogue (`store-plugin.yml`).
- `paperpaw-sqlite`: `Database::scan_bounded` reads a table like `scan`, but a row whose payload is more than a
  given number of bytes comes as `Row::Oversized(size)`: nothing of it is allocated and its overflow pages are not
  read. One huge row (a picture kept in a note) then no longer fails the whole table for want of memory. Not part
  of the plugin interface.
- The store lists a built-in plugin that is switched off (Android builds the store's plugins in, off until wanted)
  with *Switch on*, which does what the plugin manager does; nothing is downloaded for it.
- Each release has `paperpaw-fontgen`, which makes bitmap font packs (and, with `--family`, a font plugin to copy to the
  card), built for Windows (x64), macOS (Apple silicon and Intel) and Linux (x64): `paperpaw-fontgen-<version>-<computer>.zip`,
  with its SHA-256 and a signature of its own, like the emulators.
- *Pictures in books* (Settings › Reading › Reading aids, in the `aids` plugin): show them as the book has them
  (as before), put a short line such as `[Picture: Rings of light]` in their place (the picture's description,
  centred, italic), or leave them out. No picture is decoded for a picture not shown. Comics and other books
  made of pictures (CBZ, XTC) are not changed. It needs no SDK change.
- A Korean letters pack, `script-korean` (#61): the 2,350 Hangul syllables of KS X 1001, which ordinary modern
  text keeps to, with CJK punctuation, compatibility jamo and full-width forms, drawn from Nanum Myeongjo (SIL OFL
  1.1; the pack is not named after it, as its licence reserves the name). One face in seven sizes, 8.2 MiB on the
  card, for books whose Korean shows as `?` on a device whose fonts hold Latin only. It is a data plugin that
  `tools/script-pack.py korean` builds from the repository, with no code and no interface change. It is listed in
  `tools/plugin-files/data.toml` for the store (publishing waits for the release); its keyboard
  layout (two-set Hangul, `tools/plugin-files/keyboards/korean.txt`) comes with it.
- A second Korean pack, `script-korean-extra` (#61): the other 8,822 of the 11,172 Hangul syllables (rare ones outside
  KS X 1001), about 30 MiB on the card and no flash. It only adds letters to `script-korean`, so it needs no interface
  change; `tools/script-pack.py korean-extra` builds it (`tools/fontgen/sets/hangul-rest.txt`). It is listed in
  `tools/plugin-files/data.toml` like the Korean pack, and the reader offers it only for a book that uses such
  syllables, with its size.
- The Arabic user interface (#33): `language-ar`, a data plugin of one catalog (`i18n/ar.ppcat`, ADR 0033) with all
  3,197 messages of the kernel and the plugins translated (`tools/plugin-files/languages/ar.ini`; `tools/language-pack.py`
  checks it and builds the plugin). The language packs `script-arabic`, `script-hebrew`, `script-greek`,
  `script-cyrillic`, `script-korean` and `script-korean-extra` are listed for the store; nothing is published before
  the release.
- Author keys (#20), store side: a plugin's recovery key, named by the library, is pinned with its author's key at
  the first install; an update signed by another key is taken if the library carries rotations (`rotations`, in
  order) the recovery key signed, which the kernel checks, and refused (`KeyChanged`) otherwise. The library's
  `revoked_keys` (`{plugin, key, signature}`) go to the kernel, which keeps those that hold; a plugin it stopped says
  so on its store page, with *Remove*.
- The transfer screens show the device's name in the network (`paperpaw.local`) beside its address where the shell
  gives one; the QR code keeps the numeric address.
- Downloads of the store, the update, the news and font lists and the read-later cloud plugin are limited to the size the library
  names (`max_file`), so a manipulated server cannot make a plugin read an unbounded body.
- The icon font has `alert-triangle` (+320 bytes), and a check (`tools/checks`, icons) fails where a plugin names a
  Tabler icon the font lacks; the *Unreviewed* line of the store had been drawn without its icon.
- A data plugin's entry in `tools/plugin-files/data.toml` may have `how` and `de.how`: its *How it works* text.

### Changed

- The Android app icon takes the website's colours: a dark green sheet with a paper-coloured paw.
- **Greek, Cyrillic and Korean titles in the library's list** (#387): the screens' texts are drawn in Source Sans 3,
  which has none of these scripts' letters on the X4, and the Hebrew and Arabic packs brought a face for it while
  these three did not, so a book's title showed as boxes in the list. `script-greek` and `script-cyrillic` 1.0.1
  bring Source Sans 3's letters (+0.6 and +0.25 MiB on the card), `script-korean` 1.1.2 a second Hangul face for
  the screens (+1.9 MiB). A store update, no firmware change.
- The first walk of new books writes about a sixth of what it did (#98): an EPUB is opened for its details only (title,
  authors, series, cover) and its package goes into the reader's cache the first time the book is read, instead of
  a folder and its files written for every book the library finds. On the bench's card (FAT32, clusters of 32 KiB)
  the first walk of 1,000 books writes 26,952 blocks instead of 154,558 and reads 71,974 instead of 153,425; the
  X4 test of 3,000 small EPUBs (about 100 books a minute) is to be measured again. Covers are still made for the
  library during the walk, in the book's cache folder with a stamp of the book: a book replaced on the card, or a
  folder a cut-off removal left behind, is cleared when the book opens.
- **The first walk shows the books found so far** (#108): where there is no catalog yet, the kernel's walk adds the
  books found to `catalog.txt` at checkpoints (20 books, then each time twice as many, at least every two minutes once
  50 more were found), whole lines a 2 KB piece a step, sorts them for the views and sends `CATALOG_CHANGED`; the
  walk then goes on. The catalog it ends with is the one it would have put in place, byte for byte; cut off (the
  power gone), the catalog left lists the books found, and the next walk completes it. The kernel's library answers
  `Library::extended` with the key `core.library-walking` (`walking` = `true` while the catalog is made anew); the
  library's count says *so far* then. No change of interface 6.
- **Look for new books** (#109): the library (`files`) has the action `files.look-again` (in its menu, and as a
  button on an empty library), which sends `library::request_catalog_anew()` (`request_catalog()` while a walk that
  shows its books as it goes is under way, so it is not started over). A walk over a catalog without books removes
  it first and goes as a first walk does (#108): its books are put into a catalog at checkpoints.
- **Packs name the letters they need** (#97): the Arabic interface (`language-ar` 1.0.1) says `requires = script-arabic`
  and the rare Hangul syllables (`script-korean-extra` 1.0.1) `requires = script-korean` in their `plugin.ini`
  (formats 4), and their descriptions and *How it works* say it in words. Nothing fetches the letters by itself yet:
  the store reads `requires` in a later release; until then the letters pack is installed by hand.
- *Right to left: the overview of every tool and the keyboard's header* (#33): where the interface runs right to
  left the overview's grid starts at the right (the first tool at the top right, the left button on to the next
  tool), and the keyboard's title is at the right, the hint of how typing ends and the field's *Phone* and
  *Paste* buttons at the left, typed text from the right.
- *Right to left: the library and the reader's menu* (#33): where the interface runs right to left a book's cover is
  at the right of its row with the title, author and progress running from the right, the row above the books starts
  at the right (the count and the search at the left), the continue-reading card and the grid of covers start at the
  right (the left button on to the next cover), and the reader's menu shows the book's title and chapter at the
  right and the page's percentage at the left.
- The library lists every book on the X4 too (#79), not 2,000 at most: the list pages the books sorted on the card
  (`library::sorted`) and holds a few kilobytes (10,000 books: 3.7 KB held, 8 KB at most while opened or paged, on
  the host; the index of 2,000 took 48 KB). Its order, filters, stacks of series as covers, the next book of each
  series and where the focus goes are as before. Right after the catalog was made anew the list says *Preparing
  the library …* until the kernel sorted the books (a moment); the index it kept while a book covered it
  (`.paperpaw/plugins/files/index.bin`) is removed. On the X4 the catalog is now walked and caught up while the
  library shows too (the reason it waited, the index's memory, is gone); still never while a book is read.
- The library's shelves hold a bounded part of the memory however large the library (#79): the shelves in the
  list show the first 200 series and folders on the X4 (2,000 on others) with an *All series* and *All folders*
  row after them, and count the authors up to 2,048 (*more than 2048 authors*); the screens of authors, series,
  tags and (new) folders hold 200 names at a time, the next taken when the focus moves on past the last. At
  10,000 books of 5,000 authors and 2,500 series, showing the shelves took 1.26 MB on the host (about 600 KB on
  the X4, more than its heap) and now takes 52 KB; the authors' screen 214 KB, now 25 KB. Each name is counted
  exactly; the authors' screen showed the first 2,000 met before, now every author, in order. The list keeps its
  index when the catalog is as it was.
- A catalog walk that finds the library as it was writes nothing (#79): it compares what it makes with the old
  catalog and writes a new one only from where they part (the lines before copied a piece a step), keeps the
  catalog and its generation where they never do, and keeps a folder listed in one piece in memory instead of
  in `walk.list`. The walk after every start at 10,000 books writes 975 blocks instead of 69,744 (bench); after
  10 books came, 11,350 instead of 71,096.
- `shape.txt` no longer holds what only the kernel builds (the SDK's `kernel` feature, ADR 0020): no plugin
  has it, so its types change with the firmware alone (ADR 0026, `tools/interface.py`).
- *Keyboard: wider keys* (#91): the keys' side margin is a quarter of the margin other screens keep (at least
  4 pixels), so on small screens the keys get nearly the whole width. Every layout, tapped or chosen with buttons;
  the text field keeps its margin, the key order and the focus are as before. Round keys are as high as wide, so
  the keyboard gets a little higher; it still stops at half the screen where the size allows more.
- Arabic in the interface (#33): labels, rows, buttons, dialogs and whatever else draws with `FontHandle::draw`,
  `draw_toned` or `measure` set Arabic letters joined, in the order they are shown, as the reader does (`draw_in`), and
  `FontHandle::wrap` breaks them as wide as drawn. A line of both directions starts at the right with the interface's
  font in a language right to left (`Wi-Fi متصل`: `Wi-Fi` read first), as its first strong letter goes with other
  fonts. Text left to right alone is drawn as before. Plugins built before take it up from the firmware (shared text
  functions); plugins with their own copies need a rebuild.
- *Store: a plugin rebuilt for a newer PaperPaw* is not offered to a device whose firmware lacks what the build
  needs (a newer table or revision of a family of shared functions, as the library lists it in `interface`,
  `needs_api` and `families` of the build). The page says *Needs a newer PaperPaw* in place of *Install* or *Update*,
  and what is installed stays as it is; before, such a build was offered and the firmware refused it at the next
  start. A library that lists none of it is read as before.
- Large libraries: the books' details and covers made small are kept in 256 folders by the first two digits of their
  keys (`.paperpaw/library/details/<h>/<key>.ini`, `thumbs/<h>/…`) instead of one folder of thousands, which a
  memory card searched one entry after the other for each name (the first catalog of 5,000 books grew with the
  square of their number). What earlier versions kept in `books/` and `covers/` is read there until the kernel
  moves it, once, a few at a time at calm moments after the start (each move whole or not made, cut off or not);
  the old folders go once empty. Folders of book data (`add_book_data`, `add_book_cache`) may hold their entries
  in the same subfolders (`<folder>/<2 hex>/<prefix><key>`, `library::kept_path`): the kernel moves, removes and
  cleans up both, and with the move takes away once what folders of caches hold in the folder itself (made again
  from the book when needed: each book's first opening after the update takes a little longer). On FAT
  (the X4's card), a folder of 256 entries or more read whole is looked through from where the last name was
  found (for up to 8 such folders at a time), so names taken out of it in its order take a few blocks each, deep
  in a folder of thousands too (where it was in the folder's chain of clusters is kept: the FAT is not read along
  the chain from its start again for each name);
  rust-fatfs, as patched, moves and removes an entry found without looking for it again. EPUB books look for one's
  own stylesheets (`styles.css`, `<book>.css`) only when a chapter is first needed, not when opened: the catalog,
  which opens every book for its details, no longer searches each book's folder for files mostly not there. With
  10,000 books the first catalog reads 2.4 million blocks of the card instead of 160 million. Plugins that read the
  library's details must be built from this change on (they read `details/`).
- *Literata's bold and italic* are no longer in the firmware of the X4, X3, X4 Pro and X4 Classic (531 KB less, more
  room for updates and plugins) but a small font pack on the card, the plugin *Literata bold and italic*
  (`literata-styles`, 0.5 MB, 0 flash). The web flasher puts it on the card with the firmware; after an update
  or an install from CrossPoint, the store fetches it once by itself as soon as there is a network (the one download
  it makes unasked; not in Offline mode, three tries at most), says "Added" once, and else shows a short hint. Until
  the faces are there, bold and italic words are set in the regular face. `literata-large` (36 to 48 px) now
  extends these faces. A face that a plugin binds to a role is now loaded also when the reading font is the
  default one (it was, before, only once its family was chosen).
- The names of all Tabler icons take 35 KB less firmware: a name that begins the next one (`a-b` in `a-b-2`)
  is kept only in it, and each name's place, length and character take 5 bytes instead of 8
  (`tools/icons/subset.py`). `icons::names()` and `icons::char` give the same as before.
- New reading setting "Hanging punctuation" (`core.hanging-punctuation`, off by default, in the layout settings): an
  opening quote that starts a line, and a stop, comma, hyphen or closing quote that ends a justified one, are set
  just outside the text, so its edges look straight (optical margin alignment). Lines break as before, so pages hold
  the same text and kept page indexes stay valid; only left-to-right lines hang. The layout does what the reserved
  `ScriptOptions::HANGING_PUNCTUATION` bit names; no SDK change.
- Drop capitals: a book's large first letter (an EPUB's letter floated left at 1.5 times the text size or more, or
  CSS `initial-letter`) is set over the paragraph's first two to five lines, which are set in beside it. Only
  paragraphs that run left to right and start with a letter (after an opening quote or two); a paragraph of fewer
  lines gets a smaller capital. Only where the font can be drawn that large: fonts of fixed sizes (the X4's font
  packs) show the paragraph as before. The layout does what the reserved `Block::drop_cap` names (no SDK change).
  EPUB chapters converted before keep no capitals until they are converted again. The paragraph's own size is no
  longer taken from such a letter (it was set as large as the letter before). Builds without scalable fonts (the
  X4 and X4 Pro firmware) leave the drop-capital layout out (`paperpaw-layout` feature `drop-caps`, which the SDK's
  `outline` turns on); they lay such paragraphs out as before, as they did anyway.
- Lists in EPUB, HTML and Markdown books: an item's lines hang after its marker (the second line starts under the
  text, not under the number), and an item that starts with a paragraph has its marker on that paragraph's line.
  CSS `list-style-type` (and the `list-style` shorthand) and the old `type` and `value` attributes are read: disc,
  circle, square, decimal, lower and upper alpha (or latin), lower and upper Roman, none; other values keep the
  list's own markers. The marker is followed by a no-break space. Fonts without the circle or the square (the X4's
  font packs) show a bullet. Chapters converted before keep their old markers until they are converted again.
- Line breaking follows Unicode (UAX #14) where the text needs it: Korean breaks between syllables, Chinese and
  Japanese between characters, never before a closing mark (。」) or after an opening one (「), and U+200B (the zero
  width space) is a place to break; it and the word joiner U+2060 are now zero width and not drawn (they took
  the width of a missing letter before). A line that breaks by word (`ScriptOptions::KEEP_ALL`), small kana that
  may start a line (`LINE_BREAK_LOOSE`) and breaks between any letters (`BREAK_ALL`, `LINE_BREAK_ANYWHERE`) are
  options of the layout. Layout version 10: every saved page index is made again once after the update (reading
  positions are kept). Text in Latin, Cyrillic, Greek, Arabic, Hebrew and Indic scripts is laid out as before,
  except around U+200B and the word joiner, emoji, and fullwidth characters. The line-break table is 4.8 KB of
  firmware (Unicode 17.0.0, `tools/linebreak-table.py`). Arabic shaping does not change: U+200B and U+2060..U+2064
  join nothing across them, as before.
- The slider of the book text's size (settings and quick panel) has the sizes that look different as its positions,
  evenly spaced; before, its track was the whole range, so the sizes a font pack skips made the knob move one step
  here and two there.
- Settings: the separate *Update* entry in the settings menu is gone; the *Updates* group shows the update page
  first, with its state (*New*). A group page shows a plugin's actions before its settings.
- New reading setting "Korean line breaks" (`core.korean-breaks`, `settings::KOREAN_BREAKS`): by syllable (default, as
  printed books) or by word. It applies to books in Korean only.
- How an X4, X3, X4 Pro or X4 Classic starts, runs the trial of and
  installs an update moved from the firmware into `paperpaw-x4`
  (`updates`), which the firmware runs as before: CI now tests the whole
  update path on a computer, the update plugin on the kernel installing
  online onto a flash with the tables PaperPaw ships, with the release
  before's real update file and the firmware built in the run, then once
  more to a higher version; and a damaged update, a download cut short,
  one too large and a trial that fails, none of which may take the
  firmware (`crates/paperpaw-x4/tests/update_path.rs`, CI job
  `update-path`).
- X4 and X3: the power button (and *Sleep after*) now put the reader to a real sleep: the screen stays, the processor
  sleeps lightly and a press of the power button wakes it at once where it was (no start). Only after a while asleep
  on battery does it switch itself off completely, as it used to do at once. A new setting, *Switch off after
  sleeping* (Power: at once, 15 or 30 minutes (the default), 1 or 2 hours, never), says when; *Never* keeps it asleep
  (it uses a little battery all the time, and still switches off when the battery is nearly empty). While it
  charges it stays asleep. *Sleep lightly on battery* (developer group) off, or the choice *At once*, is the old way.
  The X4 Pro and X4 Classic sleep as before. Not yet tested on a device.
- X4, X3, X4 Pro and X4 Classic: a plugin may ask to switch the reader off (`Command::PowerOff`, where the device allows
  it): plugins save as for a sleep, then the power goes at once, without the light sleep before. Not yet tested on a
  device.
- **Plugin interface 6** (every code plugin is built anew
  for it; interface 5 was never released). Plugin files are of format 4: the header (116 bytes) says the
  plugin interface a plugin was built for, how much of the firmware's
  table it needs and the revisions of the families of shared functions
  it uses, and a plugin's entry is 32 bytes on the chips. A plugin built
  for an older PaperPaw says so ("Built for an older PaperPaw: update it
  in the store"), one that needs a newer PaperPaw too
  (`PluginProblem::OlderInterface`, `PluginProblem::NeedsNewerPaperPaw`,
  `Native::problem`). The firmware's table (`Api`) gains the revisions
  of the families of shared functions and a build name, and grows at its
  end from now on without a new interface. `paperpaw-plugin.x` makes a
  word per family of shared functions (`PAPERPAW_TABLE_0` to `_7`).
- Plugin interface 6, the plugins' side: each of the first-party code
  plugins that the store offers (33 of them besides the ones that had a
  newer version already) has a newer version, so that a device fetches
  its interface-5 build as an update; until then its old build waits
  with "Built for an older PaperPaw: update it in the store" in the
  plugin list. Data plugins carry no code and keep their versions. The
  texts of these two messages are in English and German.
- Plugin interface 6: types that grow later are `#[non_exhaustive]` and
  are built with `new` and `with_*` (e.g. `Run::new`, `LineRun::new`,
  `DeviceInfo::new`, `PluginInfo::new`, `Mark::new`); the SDK's event
  payloads, `Progress`, `Metrics`, `library::Entry`, the platform's
  `DeviceInfo`, `DisplayInfo`, `SystemStatus`, `PointerEvent` and more,
  the text crate's `FontInfo`, `LineMetrics`, `Glyph`, and enums such as
  `Rotation`, `ButtonState`, `PointerPhase`, `Chip` (a `match` needs a
  `_` arm). `screen::Handled` stays as it is.
- `TextStyle` of `paperpaw-document` is 32 bits (bits 24 to 27 a
  block's inline language); cached books and page indexes are made anew
  (EPUB cache version 30, a new `paperpaw_layout::VERSION`).
- `paperpaw-i18n`: `Catalog::digits()` returns the ten digits (`[char; 10]`),
  not a name; `[meta] digits` takes a CLDR numbering system's name or the
  ten digits themselves (`digits = ០១២៣៤៥៦៧៨៩`), so any system is data.
  `DIGITS` and `CALENDARS` are replaced by `digit_systems()` and
  `calendars()` (25 systems, all 18 CLDR calendars), and `[meta] plural`
  knows all of the CLDR's rules for whole numbers (Welsh, Irish, Scottish
  Gaelic, Breton, Manx, Slovenian, Lithuanian, Latvian, Romanian, Maltese,
  Icelandic, Bosnian/Croatian/Serbian, Cornish, Tachelhit, Colognian,
  Filipino, Tamazight, and `one-many-other`, `zero-one-many-other` for the
  millions of Spanish, Italian, French and Portuguese).
- Font packs of version 2 have a table directory (anchors, mark-to-mark
  anchors, line metrics per script, language forms, contextual
  substitutions, vertical metrics); packs of version 1 read as before.
  Glyphs without a code point (Indic conjuncts, Khmer and Burmese stacks)
  take keys in the private-use planes 15 and 16; the language forms and
  substitutions tables have a format (version 1: single, ligature and
  multiple lookups per OpenType feature, script and language; unknown
  lookup kinds are skipped), read once a shaper uses them.
- `FontHandle::draw_in` and `measure_in` (text in parts of several fonts)
  are shared functions: plugins use the firmware's copy, so the way text
  is split between fonts improves without rebuilding them. A mark (Thai,
  Devanagari, Khmer, Burmese or Tibetan vowel signs among them) and a
  conjoining Hangul vowel or final stay in their letter's font:
  `paperpaw_text::is_mark` is Unicode's general category M (it knew only
  the combining blocks).
- `PixelFormat::Other { code, bits }` and `RefreshMode::Other(code)`: a
  device's own pixel format or waveform (a colour e-ink panel's) without a
  new interface. No framebuffer is made in such a format (the shell draws
  into a known one and converts); a shell that doesn't know a mode
  refreshes as with `Full`.
- Plugin interface 6: the error enums have an `Other(u16)` variant (a
  cause a later build adds, by its code): `StorageError`, `HttpError`,
  `InstallFailure`, `PluginProblem`, `install::local::Refusal`,
  `DocumentError`, `ImageError`, `ZipError`, `BtError`, `DevError`,
  `RegisterError`, `FontError`, `CatalogError`, the plugin header's and
  the update file's `Problem`. HTTP and Bluetooth errors keep the code in
  event parameters (`other-7`).
- Plugin interface 6: kinds have an open value. `Other(&'static str)` (a
  name; a later build's values are constants of it) in `GestureKind`,
  `MenuPlace`, `library::catalog::Sort` and `Show`,
  `ActionHint`, `screen::Hint`, `canvas::Role`, `decorations::Tone` and
  `LineKind`; `Extended` in `SettingKind` (a `&'static str` key and static
  parameters: settings live in statics, and a key that needs no drop
  keeps that code out of every plugin) and `StatusValue` (key and
  `Params`). A build that doesn't know a value
  falls back: the default sort and filter, a gray tone, a solid line,
  a setting's value kept as text, a status value left out.
- Plugin interface 6: `BlockKind::Extended(u16)`: a block kind of a
  later build by its number (reserved: 1 a table's row, 2 a list item,
  3 a rule, 4 a figure's caption, 5 an aside, 6 a verse line, 7 maths),
  its content in the block's text, runs and annotations. A build that
  doesn't know it lays it out as a paragraph without a first-line indent
  (#33).
- Shared functions: family 7 is kept for the chain to families 8 and up
  (`paperpaw_table::FAMILIES`), so the plugin file's header, the
  firmware's table and the store's check keep their format while seven
  families suffice; `tools/shared-table.py` refuses an eighth.
- Plugin files: the header's reserved 16 bits (bytes 82 and 83) stay 0;
  a file that sets them is refused as one of a newer format ("needs a
  newer PaperPaw", in the store a plugin for another build) rather than
  as malformed.
- Plugin interface 6: `Align::Left` and `Align::Right` (`bits()` 5, 6):
  the page's edges whatever the paragraph's direction, as CSS
  `text-align: left`/`right`; `Start` and `End` stay the paragraph's.
  Layout resolves them by the paragraph's resolved direction (#33).
- Plugin interface 6: `WordBox::bidi` (`LineBidi`, also as
  `paperpaw_sdk::reading::LineBidi`): how the word's line goes, its
  paragraph's direction included, so a selection on a line of an Arabic
  book that holds only an English quote knows the line belongs to a
  right-to-left paragraph. The layout digest covers `WordBox`,
  `PageView`, `Page`, `PageLine`, `LineRun` and `LineBidi`.
- Plugin interface 6, second sweep of open values (#33): `Other(u16)` in
  `native::Chip` (a file for an unknown chip is still refused),
  `image::Format` (decoded as unsupported), `document::LinkKind`
  (followed as a plain link), `document::AnnotationKind` (shown as a
  gloss), `annotations::Kind` (by its file letter, an ASCII letter other
  than `b` and `h`: a mark of it is written with that letter, one with
  another left out; lines of a kind this build doesn't know stay
  as they are, as before) and `PluginKind`;
  and, as for the other errors, in `SettingError`, `data::Error`,
  `GraphicsError`, `config::ErrorKind` and `MemoryError`.
  `WritingMode` gains `SidewaysRl` and `SidewaysLr` (all of CSS Writing
  Modes 4).
- Plugin interface 6: `MenuNeed::Has(Capability)`: a menu entry can need
  any capability, also one a later build adds (not met where unknown).
  `SystemStatus` holds eight named values (`value`, `with_value`:
  `temperature-c`, `frontlight-percent`) a later shell can fill without a
  new interface.

### Added (plugin interface 6, for plugin authors)

- More freedom over how things look (ticket #57, interface 6):
  - **Text tone:** `ToneCurve::from_alphas`, `parse`, `parse_for` and `to_text` make a curve of your own from 16 values or text
    (`smooth`, `contrast:1`, `sharp:128`, a list of numbers); the settings `core.text-curve` (book text, in place of *Text
    contrast* when set) and `core.ui-text-curve` (the interface; themes set them) take a list of `[script=]curve` entries.
    A `FontHandle` carries a `TextInk` (`inked`, `text_ink`) that `draw` and `draw_bolder` use.
  - **Looks:** `Painters::text` paints the text of `Label`s and of the SDK's rows, headers, buttons, hints and captions (a shadow,
    an outline), never changing its width; `ui::draw_label_in(canvas, font, style, role, ..)` does the same for text a plugin
    draws itself (`draw_label` stays the stock look);
    `Painters::extras` takes a static list of painters for parts of kinds that come later (`PartKind`), and `Painters::flags` is
    reserved. `TextRole` is now an open `TextRole(u8)` with constants (`TextRole::BODY`, new `HINT`, `BUTTON`, `ICON`).
    **Breaking for plugin authors:** `TextRole::Body` is now `TextRole::BODY` (and so on for every role); rename it, the compiler
    finds each one.
  - **Script:** `Lang::likely_script()` (and `paperpaw_lang::likely_script_of(tag)`, which reads a tag without the language tables, so a plugin does not carry them) gives the script of a language (the tag's, else the one the language is usually
    written in: `ar`, `fa`, `ur` and `ps` are `Arab`, `he` is `Hebr`), so `arab=contrast:1` in the curve settings reaches an
    Arabic book and interface; `PageType.script` and `HookCx::script()` use it. Languages in Latin letters have none.
  - **Smaller plugins:** the kernel makes how the interface's letters are inked of `core.ui-text-curve` and the language
    (`ui::text_ink(value, language)`, `Fonts::set_ui_ink`), and the handles of the UI font carry it, so no plugin carries the
    parser (about 3 KB of each code plugin); `Fonts` gained a field for it.
  - **Rebuild all plugins:** `FontHandle` gained a field (its `TextInk`) that the firmware's `draw` reads, so every plugin has to
    be built against interface 6 again; one built before would pass a shorter handle.
  - **Colour roles:** `Ink` (the interface's colour roles) is now an open `Ink(u8)` with constants: `Ink::Text` is `Ink::TEXT`
    and so on (rename it; plugins only change the spelling). New roles `ACCENT`, `ON_ACCENT`, `LINK`, `SELECTION` and
    `WARNING` (numbers 14 to 18; 19 to 31 are free). `Palette` holds 32 colours (100 bytes, was 42) and knows which a theme set:
    a role it leaves out reads its `Ink::fallback()` (`LINK` reads `TEXT`, `SELECTION` reads `CARD`), so themes made for 14
    roles keep working and nothing looks different until a theme sets a new role. `core.ui-palette` also takes `#rrggbb`
    (older readers skip such a pair; gray panels use its brightness). `Ink` has `value`, `from_value`, `fallback`. `TextInk` is
    how text is inked (tone); an `Ink` is a colour role.
  - **Render hooks:** a plugin can register a hook (`Registrar::add_render_hook`, `RenderHookSpec`, `HookStage`, `BandView`,
    `HookCx`, `FramePass`, `Region`) that changes finished bands, with kernel-owned carry for dithering across bands.
    `RenderHookSpec` is `#[non_exhaustive]`: built with `new` and the `with_` methods. The
    kernel accepts hooks and does not run them yet: `Capability::RENDER_HOOK`, `RENDER_REGIONS` and `LOOK_EXTRAS` are off.
  - **Drawing:** `Canvas::pixel`, `gray_at` (read back what this band drew) and `fill_dithered`.
  - The interface check now follows type aliases to the types they name (a type only a `fn` alias mentioned was not pinned).

- `paperpaw-lang`: `Lang` (a language tag in 32 bits: language, script,
  region; canonical, so `per` and `fa` are one), `Direction` and
  `WritingMode` (#33). `paperpaw_sdk::lang`.
- `paperpaw-table`: the families of functions plugins may share with the
  firmware on RISC-V chips, their revisions, and `shared!` (#44);
  `tools/shared-table.py` writes its index from `shared.list`.
- Right-to-left text (#33): blocks of `paperpaw-document` carry a
  direction, a writing mode, a language, inline languages and a drop
  cap; links say what they are (`LinkKind`: a note reference, the way
  back). Lines of `paperpaw-layout` carry their direction (`LineBidi`),
  runs their level and language; pages their notes (`PageNote`);
  `Style` reserves `justify_cap`, options for some scripts as flag bits
  (`Style::script_options`, `ScriptOptions`: kashida, hanging
  punctuation, keep-all, and reserved groups for CJK and Thai line
  breaking, read and set with `group` and `set_group`), and options and
  signed values for later page settings (`LayoutOptions`,
  `Style::values`): 0 is today's behaviour, so a later option is a new
  bit, not a field; `BookStyles::from_bits`, and `ALL` holds styles named
  later; `Typesetter::advance_text` measures a text as a whole. The
  options are in the page-index key, so every saved page index is made
  again once after the update.
- `paperpaw-text`: `StoredPack` and `GlyphCache` are handles made by the
  firmware (`StoredPack::open`, `GlyphCache::new`), whose state only the
  firmware's code reads, so how packs and the cache keep it can change
  without a new plugin interface. Packs read from a card hold up to
  65,535 characters (all 11,172 Hangul syllables, #61), in far less
  memory: a block held whole takes some bytes, advances that are the same
  are kept as runs, and records are read 8,000 bytes at a time. A pack's
  index of characters takes at most 8 KiB on devices of a few hundred
  kilobytes, 32 KiB on larger ones (`PackLimits`, from the device's
  profile, so the emulator of the X4 refuses what the X4 refuses; else
  `FontError::TooLarge`), a size's advances at most 4 KiB or 16 KiB kept
  whole (else read when needed); `StoredPack::open_within` takes them,
  `open` the small ones. `FontError::OutOfMemory`: a pack that may open
  later.
  `StoredPack::render` returns `None` for a glyph it could not read now (a
  card pulled), which the cache does not keep; an advance that could not
  be read is 0 and counted (`StoredPack::failed_reads`, over the loaded
  fonts `Fonts::failed_reads`), so a page index can keep no pages
  measured meanwhile. `StoredPack::bytes` and `trim`,
  `GlyphCache::capacity`, `set_capacity` and `trim`.
- `paperpaw-text`: character sets are no longer public constants; tools
  name them (`charset::named`), so a set grows without a new plugin
  interface (`charset::names` lists them). The Greek and Cyrillic sets
  have the modifier apostrophe U+02BC (#60), which packs without it show
  as a right quote (`stand_in`); new sets `hangul`, `hangul-jamo`,
  `cjk-punctuation` and `fullwidth`. `paperpaw-fontgen --chars @FILE`
  reads code points from a file starting `# paperpaw-charset 1`, e.g.
  the 2,350 Hangul syllables of KS X 1001
  (`tools/fontgen/sets/hangul-ks.txt`).
- `paperpaw-text`: `Around` (what lies around a text, its base direction
  and language), `draw_text_in` and `measure_in`: right-to-left order,
  Arabic letters joined (also across a text's edges), mirrored brackets,
  marks kept on their letters.
- SDK: `WordBox::rtl`, `widgets::Align::Start` and `End`,
  `UiStyle::is_rtl`, `Env::direction` and `Env::lang`; the painters
  `sheet`, `cover`, `pill` and `ticks` (`SheetPart`, `CoverPart`,
  `PillPart`, `TicksPart`); room to grow without a new interface
  (`Widget::on_extended`, `GestureRecognizer::extended`,
  `library::Library::extended`, `Document::extended`).
- Reading Arabic and Hebrew books (#33): EPUB blocks take their
  direction and language from `dir`, `lang`/`xml:lang` and CSS
  `direction`, and a book in a right-to-left language runs right to left;
  `text-align: left`/`right` and the margins at the sides are read
  against the direction; inline `dir`, `<bdi>` and `<bdo>` set their
  text apart (isolates and overrides of UAX #9). Lines are resolved by
  UAX #9, start at the right where they run right to left, and are drawn
  run by run in the order of their levels, joined Arabic letters as wide
  as drawn (`FontHandle::measure_in`, `draw_in`), a word whose
  characters come from more than one font (in quotes or brackets, with a
  colon or digits) in parts of one font each; the words of a page
  come in reading order where they are drawn. Not yet checked by readers
  of these languages.
- The pieces of a long paragraph (a chapter saved by Word as one) are
  resolved as one paragraph: `paperpaw_layout::PieceStarts` keeps the
  bidi state at their starts for a reader (`LayoutContext::pieces`), and
  `paperpaw_text::bidi` gives that state as bytes (`state_len`,
  `start_state`, `resolve_from`, `skip_from`, `state_paragraph`), which
  stay out of the plugin interface. `paperpaw_text::bidi` is public
  (levels, paragraph levels, rules L1 and L2).
- `paperpaw_text::is_invisible` (also `paperpaw_sdk::text`): the controls
  that draw nothing and take no room.
- `Block::continued`: the next block continues this one (the pieces of a
  long paragraph, set by the EPUB and TXT documents), so the layout reads
  ahead only then. `Lang::direction`: the direction a language is
  written in.
- `paperpaw-i18n`: a catalog says its direction, script, digits and
  calendar (`[meta]`); Arabic and Hebrew plural rules.
- Arabic harakat and Hebrew points sit where their font puts them (#33):
  `build_pack` (and `paperpaw-fontgen`) keeps the GPOS `mark` and `mkmk`
  anchors of Arabic and Hebrew in a pack's anchors table (for each
  letter's form, under or over its dots), and `draw_text_in` draws a mark
  on its letter, or on the mark before it, by them; a mark whose classes
  its letter lacks is centred over it. Packs read a glyph at a time and
  outline fonts keep the anchors of at most 48 keys in the glyph cache
  (about 2.5 KB, once marks are drawn so). Packs without the table, and
  every Latin, Cyrillic and Greek pack, are drawn as before. The Arabic
  and Hebrew data plugins (`tools/script-pack.py`) are about 12 % larger.
  `tools/script-pack.py` without a set builds the plugins
  `tools/plugin-files/data.toml` lists for it (else Cyrillic and Greek,
  as before; its folders keep their names, `script-<sets>`). Of two sizes of a pack
  as near to the size asked, the smaller is drawn, whatever their order
  in the pack.
- Plugins switch the device off where PaperPaw does so
  (`Command::PowerOff`, `Capabilities::power_off`) and hear how long it
  slept (`Env::slept_for`) (#51).
- The X4 and X3 share the **text family** with their code plugins (43
  functions of `paperpaw_text` and the SDK's font handles, revision 1):
  a plugin built for them calls the firmware's copy of measuring, drawing
  and wrapping text instead of carrying its own (about 26 KiB less in a
  plugin that draws text). The loader fills the families' words of a
  plugin (`Relocator::tables`), refuses a plugin built for a newer or an
  older revision of a family before it places anything, `paperpaw-sign
  pack` records the families a plugin reads, and a panic in shared code
  (a parser of what the plugin handed it, an allocation that failed
  there) is blamed on the plugin that called it, so safe mode switches
  off that plugin alone; the crash file says `[shared code, called from
  <plugin>]`.
  The X4 Pro and the X4 Classic keep their copies. A library built for a
  computer with `--cfg paperpaw_table_host` calls the same tables; CI job
  `table-compat` checks old plugins against the new firmware once an
  interface has been raised with tables.

### Fixed

- The device no longer falls asleep while the library is being walked (#105): a first walk of a few thousand books
  takes longer than "Sleep after", and sleep stopped it. The sleep is put off while the walk goes on, for an hour
  after your last press at most, and not at all with the battery at 10 % or less (not charging); "Switch off after"
  counts from the sleep, so it waits too. A walk paused for an open book holds nothing. The walk keeps the device
  awake even with "Plugins may keep awake" switched off (the help of that setting says so).
- X4 and X3: books copied to the card while the reader slept lightly (the card in a computer) show in the library
  again after the wake (#79, #373), also when the library or a book from it was open at the sleep, and a look at
  the card that was under way at the sleep is started again. A wake is no start, so the library had been left as it was.
- The store no longer counts code plugins that are switched off as taking room (#70): the next start skips their
  code and their room is free, so an install that fits then is offered (its page says it fits while the code
  plugins you switched off stay off), instead of asking to remove a plugin that could have been switched off.
  The update does the same when it fetches code plugins for a new version.
- The reader no longer keeps pages counted while a font pack on the card could not be read (a card pulled, a
  hiccup): the letters it could not measure were 0 wide, and a page index of them sent later pages to the
  wrong place. Such a step is made again (`Fonts::failed_reads` is taken before and after it); after three in a
  row the count is given up for that layout, as for a page that cannot be laid out.
- A pack that adds another script's letters in more sizes than the font
  it extends has (the Arabic one, to the firmware's Literata) no longer
  becomes that font's pack of larger sizes: the font's largest size
  stays its own, and Latin text at larger sizes is not set in it.
- `decorations::join_boxes`, and the reader's selection, join the boxes
  of words next to each other in a line that runs right to left (they
  came to nothing) and leave out a word drawn between two of a line that
  runs both ways; left and right move the selection to the word drawn
  beside.
- SDK: `layout::line_start` of a position before a block (a section's start,
  say) gives that block's start, where a page laid out from there starts, not
  the position itself. Comparing it with a page's start or looking it up in a
  `PageIndex` then needs no more care.
- Choosing text with the buttons: the page buttons (the X4's side buttons) now move by line, as Up and Down do,
  not word by word, and Left from the first word of the page reaches the last word (Right from the last
  reaches the first), instead of stopping there.
- *Page with a picture*: a picture made for the whole screen (the X4's 480 × 800, as CrossPoint's) now covers the
  whole screen at size 100 %. The size was a share of the screen's shorter side, so it was drawn in a square
  and covered only the middle; it is now a share of the screen, and at 100 % no margin is left. Pictures
  that are wider than high keep their size.
- A book read to its last page is now recorded as read to 100 % (it was recorded as how far the start of its last page
  is, often 95 %, so the library did not list it as finished). The reader's `core.book-finished` and
  `core.book-closed` now carry 100 once the last page was reached.
- A time zone rule with an offset of more than 24 hours (such as `<>200`) is now refused instead of read as an offset of days.
- Reader: going to a chapter whose table of contents entry names no place in
  it (a whole chapter of an EPUB) showed the last page of the chapter before,
  from the contents, a link and *Next chapter* alike, and *Next chapter* and
  *Previous chapter* could go no further from there.
- Reader: status lines show the page in the chapter in a book's first chapter
  too, where it starts at the book's start.

## [0.5.0-beta.12] - 2026-10-28

### Changed

- X4 (with an SSD1677 panel, as most have): a setting, *Fast changes* (Display), says how menus and a moved focus
  are shown: the panel's own waveform (500 ms, the cleanest) or a quicker
  one of 8, 6 or 4 frames (found in the waveform lab; about 160 ms with 4),
  which leave more ghosting that the next full refresh clears. It is
  *The panel's own* (the slowest, the cleanest) by default; page turns are as before.
  Black-and-white refreshes (screens that ask for `fastest`, such as games)
  take about 100 ms, with 1 frame. The frames and the kick of both are
  settings of the developer group (`x4.ssd1677.fast-frames`, `x4.ssd1677.fast-kick`,
  `x4.ssd1677.mono-frames`, `x4.ssd1677.mono-kick`, offered only where the panel's
  controller is an SSD1677; the fast ones where *Fast changes* is
  *Own values*), so a plugin without code can set them in its `settings.ini`
  or by an action; the firmware keeps them in the order of the modes (fast
  at most 20 frames, black and white no more than fast).

### Added

- X4 Pro (a UC8179 controller): the fastest refresh (screens that ask for `fastest`,
  such as games) has numbers of its own, the developer settings `x4.uc8179.mono-frames`
  (12), `x4.uc8179.mono-hold` (0) and `x4.uc8179.mono-rate` (200 Hz): about 175 ms
  by the formula (3 frames at 100 Hz were far too grey, 6 nearly clean: the drive
  needs about 60 ms, whatever the rate). The frame rates go up to 200 Hz (110, 130,
  150, 200 added); the log says how long the setup and the refresh waited for the
  controller. It took the step of *Fast changes* (7 frames, hold 3,
  about 330 ms, no ghosts) before. The numbers are a guess, to tune on a device.
- X4 Pro (a UC8179 controller): a developer setting, `x4.uc8179.transfer`, says how much
  of the picture goes to the controller for a fast change: the whole (what the
  stock firmware sends), only the rows and columns that changed (into both
  memories), or only into the new memory, leaving the controller's own copy to
  make the old one (the default: about 50 ms less a fast change, 329 instead of
  379 ms at 7 frames, judged right on a device). The refresh itself
  still covers the whole display, so the hold keeps working. The whole picture
  goes again after a start, a sleep, grays and the slow refreshes.

- X4 Pro (a UC8179 controller): *Fast changes* (Display) says how menus and a moved
  focus are shown, as on the X4: the panel's own waveform (625 ms) or a quicker
  one of the controller's registers, run at the voltages of the panel's own
  waveform (read from its OTP at start, about 65 ms more): 6, 7 or 10 (the panel's own is the default)
  frames with 3 held (about 360, 380 and 440 ms), or own values in the developer
  settings (`x4.uc8179.*`). Found with the waveform lab on a device.
- The waveform lab's direct drive on the X4 Pro can hold the pixels that stay (0 to 3
  frames driven to their own colour, as the controller's own fast waveform does
  for one frame), which keeps the ghosts of earlier frames down.
- The waveform lab's direct drive on the X4 Pro runs at the voltages and VCOM of the
  controller's own waveform (read from its OTP at start), not the reset values
  of its registers (VDH and VDL 14.0 V, VCOM_DC next to nothing); and the lab can
  show the panel's own waveform at a temperature (up to 120 degrees) on a UC8179
  too.
- Developer firmware of the X4 Pro (`x4pro` builds) reads the controller's OTP at start (a
  UC8179, about 100 ms) and tells the log what its own waveforms are: the
  temperature ranges, their frame rate, voltages and VCOM, and the states of the
  ranges 25 and 90 degrees select (`otp:` lines).
- The waveform lab also runs on the X4 Pro (a UC8179 controller): a direct drive
  made of the controller's waveform registers, with the frames, the kick, the
  frame rate (20 to 100 Hz), the repeat and the levels of white and black to try,
  at the controller's own voltages. Developer firmware (`x4pro` builds).
- `tools/flash-x4.py` builds the checked-out source and flashes it to an X4,
  X4 Pro, X4 Classic or X3 over USB, with a check of the tools, an optional
  backup of the reader's flash and a dry run.
- The developer firmware of the X4 has a *Waveform lab* under Tools, for
  trying how the panel is driven: its own tables at a temperature, or a
  direct drive with the frames and the kick chosen on the screen.

### Changed

- X4 (with an SSD1677 panel, as most have): a setting, *Fast changes* (Display), says how menus and a moved focus
  are shown: the panel's own waveform (500 ms, the cleanest) or a quicker
  one of 8, 6 or 4 frames (found in the waveform lab; about 160 ms with 4),
  which leave more ghosting that the next full refresh clears. It is
  *Quicker, some ghosting* (6 frames) by default; page turns are as before.
  Black-and-white refreshes (screens that ask for `fastest`, such as games)
  take about 100 ms, with 1 frame. The frames and the kick of both are
  settings of the developer group (`x4.ssd1677.fast-frames`, `x4.ssd1677.fast-kick`,
  `x4.ssd1677.mono-frames`, `x4.ssd1677.mono-kick`, offered only where the panel's
  controller is an SSD1677; the fast ones where *Fast changes* is
  *Own values*), so a plugin without code can set them in its `settings.ini`
  or by an action; the firmware keeps them in the order of the modes (fast
  at most 20 frames, black and white no more than fast).
- EPUB chapters convert and pages read faster, books open with fewer file
  lookups, and skimming to the next or previous chapter no longer converts
  every chapter of the book first.
- Folders on the SD card are read faster, and reading a large file at
  scattered places no longer walks its clusters from the start each time.
- Page turns finish sooner: the X4 Pro and X4 Classic no longer send each
  frame a second time after its refresh, and released firmware no longer
  checks every frame it sends.
- Text, light separator lines and shaded highlights draw faster in black and
  white and in grays, with the same pixels as before.
- Menus, settings pages and the home screen spend less time on their texts
  and settings, and a start reads the translations faster: a message is
  looked for only in the catalogs that can hold it.
- Large libraries open faster: the library's shelves and its index read
  the catalog once, a shelf shown again after a book or its actions is not
  read anew when nothing changed, home keeps its covers made small, and
  the vocabulary list reads only the words of the page shown.
- Book pages draw faster: lines outside the part of the screen being drawn
  are skipped instead of measured word by word. Searching a book needs less
  memory.
- Turning a page waits less for the card: the reading position and
  today's reading are written once the pages rest for a moment, the book
  closes or the device sleeps, and the book's details are read only when
  its shown percentage changes.
- Pages are laid out faster: hyphenation stops looking for patterns that
  cannot match, and characters are found in font packs with fewer lookups.
  Lines break exactly as before.
- Lists, menus and settings pages draw faster on readers that draw the
  screen in bands, such as the X4: rows, headers and status lines outside
  the band being drawn are skipped instead of laid out again for each band.
- A page turns as soon as its button goes down: holding a page button
  for the next or previous chapter is now off by default and can be
  switched on in the settings under Controls ("Hold for next chapter",
  an advanced setting).
- *Button response* on the X4s has a third choice, *Saving*: the processor
  sleeps up to 50 ms between readings of the buttons on battery (30 ms
  as *Normal*, 10 ms as *Faster*), for a little less current; a tap
  shorter than that may be missed. *Normal* stays as it was.
- Rules: the list to choose an action, a switch or a number from comes by
  plugin where it is longer than a page (some six pages of actions on the
  X4): first the plugins, with how many each brings, then its own, and
  back goes out of a plugin again. Every action stays on offer, also those
  that work on the screen shown.

### Fixed

- X4 and X4 Pro: a screen that runs at a pace (`ScreenMode::frame_every`) showed its
  frames with the waveform the last frame before it had left on the panel, not that of
  its own refresh mode. A game with `ScreenMode::fastest` and `grays(2)` kept the panel's
  own waveform (500 ms) until a frame that did not wait for the panel (a game-over
  card, say) set the quick one, and so seemed to be fast only after the first restart.
  Paced frames now set the waveform and the transfer as every other frame does.
- X4 Pro: the sleep screen with a cover, a picture or the cat (shown in grays) is made
  after a refresh of every pixel, as the others are (a page with pictures is not: it
  keeps the quicker first stage, also at its periodic full refresh); before, the grays were put over a fast
  refresh, which leaves a little ghosting of the screen shown before it. The fast
  refresh before any grays also no longer takes a quicker waveform that a game or
  a menu left set.
- Moving the focus between two items far apart on the home screen (the edit pencil
  and a *Continue reading* widget with a cover) no longer refreshes the whole panel
  in full at every step. The kernel took the rectangle round the two items for a page
  (half the screen or more) and, with a large picture on the screen, refreshed it in
  full; it also counted every such step towards the full refresh every few pages. It
  now goes by what was drawn (the areas added up), not by the rectangle round them;
  a whole screen drawn anew, a page, is as before. A widget of half the screen or
  more (the *Continue reading* widget with its cover) is a page by itself, and
  every focus step still showed the screen in four grays (about 1.7 s): a focus step
  in the widget grid now sets its refresh exactly (fast), as the kernel takes it for
  a step and not for a page. The first step after a screen was shown in grays still
  drives every pixel once.
- The row of hints that comes on a press (in the reader, with the buttons below the
  screen) is shown and taken away with a normal partial refresh, not a fast one: with
  the quicker waveforms of *Fast changes* it left ghosts of the page's text at the
  bottom at every page turned by those buttons.
- On the X4 and X4 Pro, what plugins do on waking now happens: *When the
  device wakes up* in the rules, *Review on waking* of the flash cards, and
  syncing a position at wake. Their sleep restarts the device, which plugins
  heard as a start only; the firmware leaves a mark (`.paperpaw/slept`) as
  it sleeps, and the start that finds it sends `core.resume` after
  `core.started`. The battery log keeps one start line for the two.
- The flash cards' *Retention wanted* setting shows its value as a
  percentage again, not as the key `vocabulary.unit.percent`: the unit
  needs its plural form (`.other = {count} %`).
- The side margins set for reading no longer get the book's own `body`
  margin and padding added on top, which left the sides wider than the
  setting (top and bottom were right). Books are converted again.

## [0.5.0-beta.11] - 2026-10-26

### Added

- X4 Pro firmware includes Bluetooth LE for HID page-turners and
  keyboards, pairing on the reader, and plugin GATT clients. Remotes
  remain manual by default; active Bluetooth prevents processor light
  sleep, and device sleep shuts the radio down. Other readers are
  unchanged. Advertising and plugin GATT servers remain unavailable.

### Fixed

- Bluetooth applies the requested remote connection parameters before HID
  discovery, so setup no longer blocks that change. Idle power policy is unchanged.
- Bluetooth reconnects known devices when the controller reports their resolved
  identity or a new private address, while retaining the existing paired entry.

## [0.5.0-beta.10] - 2026-10-26

### Fixed

- Justified text keeps bounded word spacing when font size or word
  spacing changes, without abruptly switching whole lines to tight
  spaces. Balanced line breaking considers the gaps it can distribute.
  The settings preview and book pages use the same behavior.
- X4 plugin-cache indexes are read with the current entry size, so a
  restart reuses verified cached code instead of rewriting every plugin.
- Xteink readers enter hardware sleep even when USB or charging is
  detected, instead of keeping the processor awake behind the sleep
  screen. Press the physical power button to wake them.
- A sleep request survives redrawing the screen after a grayscale image.
- X4 light-sleep transitions cannot overlap the button ADC interrupt's
  access to the internal analog bus.
- The X4 Pro holds battery power before memory, diagnostics and OTA
  startup work, so a short power press need not span that work.
- Screens that hide button hints, including the sleep screen, cannot
  retain a temporary hint row from the previous screen.

### Changed

- Xteink readers show a startup message before loading plugins on normal
  starts too. Its panel refresh overlaps plugin loading.
- X4 Pro and X4 Classic verify native plugin digests with the chip's
  SHA-256 engine. The same header, image and relocation bytes are checked
  before any plugin code runs.
- Kernel startup reads installed native-plugin digests once for the plugins
  it will load, after recovering pending installs. The bounded snapshot is
  discarded after loading; the plugin interface is unchanged.

### Added

- Text layout callers can add a cost for the unused width and internal
  spaces of a candidate balanced line with
  `wrap_lines_balanced_kerned_with_cost`. Existing calls keep their layout.


### Compatibility

- The native plugin interface remains i4, with the same pinned compilers.
  Existing compatible plugins continue to run. Reader layout caches are
  rebuilt for the updated justification behavior.

## [0.5.0-beta.9] - 2026-10-26

### Fixed

- Every restart shows a translated restart notice before saving state
  and resetting the reader, whichever menu requested it.
- The USB development helper reads current format 3 plugin digests when
  trusting a local build, while still accepting format 2 files.
- Plugins selected in the web flasher keep their Store origin after
  installation, so the plugin store can update them automatically.
  Manually installed plugins keep their local origin.
- First startup shows plugin checking, installation progress and startup
  preparation while the web flasher's bundle is copied to the SD card.
- Settings distinguish *Manage plugins* from *Plugin settings*, rather
  than showing two entries named *Plugins*.

- Resetting settings changes only explicitly stored values, avoiding
  repeated font and layout reloads for defaults that were never changed.
  Book-specific resets leave global values intact.

- The plugin store shows when it refreshes a saved plugin list, while
  keeping that list available until the request finishes.

- Crates.io releases use new versions for changed built-in plugins, so packaged applications compile against the matching plugin sources.

- A text shortened to its room (`ui::fit`, rows' titles and values)
  counts kerning as it is drawn: pairs set apart (`f\` in the UI font)
  could take it up to a few pixels past its room, into the value beside
  it.

## [0.5.0-beta.8] - 2026-10-25

### Changed

- Wi-Fi joins the saved network picked last on the Wi-Fi page first,
  when it is in reach, instead of the strongest: after a restart the
  reader stays in the network one chose. *Join first* (Last picked,
  Strongest) sets it.
- **Code plugins must be built again for beta.8.** The plugin interface
  is now 4: what crosses changed for a book's formatting and kerning
  (`Block`, `TextStyle`, `Typesetter`, `layout::Style`, `FontHandle`),
  the settings' pages (`SettingGroupSpec`), screens that run in real
  time, screens opened once, static registration (`Actions::get`,
  `Env::menus`, `Env::screens`) and lists that draw only what a step
  changes (`ListChange`), with room for what comes later (the developer
  link, `Command::Extended`), and room between the lines for page
  layers (`Block::leading_above`, `PageLine::leading`,
  `LineBox::leading`, `PageView::words`, `PageLayerSpec::words`),
  blocks filters insert (`Block::inserted`, `PageLine::inserted`),
  input on layers (`PageLayerSpec::items`, `LayerItem`,
  `PageLayerTapped::item`) and room for annotations
  (`Block::annotations`, `PageLine::annotations`).
  PaperPaw's own store plugins come as updates.
- Moving through the settings with the buttons is quicker: a step draws
  only the row left and the row reached, no longer the whole screen (on
  the X4 a sixth to a third of it), in every list of the settings.
- The firmware is leaner, so updates keep room: German hyphenation is a
  plugin of the store (`hyphenation-de`), as the other languages are,
  and the built-in one keeps English; Literata Bold Italic is no longer
  built into the firmware (bold stands in for it; the emulator and the
  app keep it), and its faces go up to 32 px there (about 10 pt). A data
  plugin may bring faces of a family without its regular one, and a pack
  that extends a face with larger sizes draws text that large (the font
  size goes as far), so the font library brings both back
  (`tools/literata-packs.py`). At the largest size, its help says where
  larger ones are found.
- The release keeps room for the updates to come: `paperpaw-sign
  check-image` prints the margin an image leaves in the smallest app
  slot (its own or CrossPoint's), warns below 512 KB and fails below
  256 KB.
- A step with the buttons draws only what it changes, on every screen
  with a list, a grid or buttons to move between: the item the focus
  left and the one it came to, no longer the whole screen (on the X4 a
  sixth to a third of a frame). `ListChange::Focus` carries the two
  items, so `ListChange::request` asks for them alone; `ListView`,
  `Rows` and `Sections` give `focus_change` for a focus that only came
  to show, `ListChange::focus` serves focuses of a screen's own and
  `ui::button_reach` says where a focused button draws. Their `request`
  methods are gone: `change.request(cx)` does it. A screen whose focus
  changes more than its items asks for that part too.
- The store finds its way in a library of thousands: its start shows
  what the library picks (Recommended, New, Popular now, Recently
  updated, Discover) between the search and the groups, and a group
  lists its plugins in the library's fair mix of the day, so that every
  plugin gets seen. A to Z stays a choice, sorted on the device a run at
  a time, and long groups go to a letter (*Go to letter*, a grid of A to
  Z for buttons and fingers). The list still streams to the card. A
  library without picks shows the start and the order as before.
- Plugins that work with another product say what they do with it:
  KOReader highlights, Calibre connection, Readwise export, Wallabag
  articles, Wikipedia lookup and Joplin notes (their ids stay, and so do their
  settings and data).
- Literata's sizes above 32 px and its Bold Italic, which the X4's
  firmware leaves out, are plugins of the store: the release builds
  PaperPaw's own data plugins (`tools/plugin-files/data.toml`) and
  publishes them with the others, versions checked as theirs.
- With buttons alone (the X4) the keyboard keeps a computer keyboard's
  rows at every size (qwertyuiop above asdfghjkl); the size makes keys
  higher and their letters larger, and a row is split only where its
  letters would not fit. Up and down keep to the column one came along.
  Typing takes about as many presses as before (627 against 633 for the
  sample texts in `plugins/keyboard/tests/presses.rs`), capitals fewer.
- Its keys are wider with buttons alone: the focus fills a key instead
  of framing it, so two pixels between keys do (on the X4 a letter's key
  is 39 pixels wide, was 25). On touch screens the room between keys
  follows the screen's size (0.6 mm). The plain X4s start with the
  keyboard at Small, the X4 Pro at Large; a size chosen stays. "Hold: ö"
  in place of "Hold for ö" leaves the field's title whole.
- Settings where readers look for them: *Date and time* (the clock, the
  first day of the week, the time zone), *Plugins* (each plugin's
  network) and *Words* are groups of their own; the auto sleep is under
  *Power*, the device's name under *Wi-Fi*, the sleep screen's logo with
  the sleep screen, the widgets' frame with the home screen. Reading's
  settings have shorter names (*Invert pictures*, *Paragraph spacing*,
  *Top and bottom margins* …).

### Added

- Plugins write between a book's lines, an interlinear reading with a
  translation above each line: a text filter asks for room above every
  line of a block (`Block::leading_above`, hundredths of an em, at most
  four), which the layout adds to the line's height so pages break with
  it, and each line tells it (`PageLine::leading`, `LineBox::leading`);
  a page layer that asks (`PageLayerSpec::words`) gets the page's words
  with their boxes and places (`PageView::words`, `reading::WordBox`,
  now without the feature `unstable`), measured once a page and only
  where a layer asks.
- Text filters insert blocks after the one they get
  (`Block::insert_after`: a translation after each paragraph, a gloss,
  a note), at most four with 4 KiB of text: shown after it, never apart
  from its last line, with no place in the book (`PageLine::inserted`),
  so positions, progress, bookmarks, highlights, search and the page's
  words stay the book's.
- Page layers take input on what they draw: a layer says where its items
  are (`PageLayerSpec::items`, `LayerItem`); a tap on one, or Select on
  one chosen with the buttons as links are, comes to its plugin
  (`PAGE_LAYER_TAPPED` with `item`), and every other tap and button
  stays the reader's. The room above a line counts as the line's
  (`PageView::line_at`).
- Room for annotations over words (ruby, furigana, phonetics, glosses):
  `Block::annotations` and `PageLine::annotations`, empty until the
  layout sets them (ADR 0032).
- A sample plugin, `samples/interlinear`: a translation above each line
  that opens on a tap, or after each paragraph.
- With buttons alone, holding Back finishes typing and keeps the text,
  wherever the focus is (Back alone cancels); the field says so. Holding
  Select still finishes on any key that is no letter.
- The keyboard locks capitals with *Shift* pressed twice in a row (the
  key filled until pressed again), and a long press on a letter offers
  its capital first, chosen: with buttons a long press and Select type
  it. *Automatic capitals* (off by default) starts a text and each
  sentence with one, not in passwords, numbers and addresses.
- SDK: `ask::TextQuestion::url()` and `email()` ask for a web or e-mail
  address (modes `url` and `email`), typed as it is. The browser,
  catalogs, news, KOReader sync, Joplin, Wallabag and the statistics'
  sync ask for their addresses so.
- Plugins can run in real time: games, emulators, animations
  ([ADR 0030](docs/adr/0030-real-time-screens.md)). A screen says how
  it runs in one place (`Screen::mode`, `realtime::ScreenMode`): full
  screen, buttons pressed and released (`Screen::on_key`,
  `Env::keys_held`; holding *Back*, or a finger in the top left corner,
  two seconds always leaves), its own refreshes, the fastest refresh,
  the grays enough, a pace, staying awake longer, and the memory it
  needs. The device says what it can do and how the last frame went
  (`Env::panel`, `PanelInfo`, `FrameReport`), so faster panels make
  plugins faster without a new build. `Canvas::blit` draws packed
  bitmaps at a whole scale, `memory::vec_in` keeps data in internal RAM
  or PSRAM, and code plugins get `Api::alloc_in`. On the X4 the main
  loop goes on while the panel refreshes for such a screen, and every
  frame's times are measured. `samples/frame-test` shows it all. The
  first time such a screen opens, a notice says how to leave it.
- Room for a screen that needs memory: the kernel lets caches go, holds
  background work back, switches Wi-Fi and Bluetooth off where the
  screen does not need them, asks the screens below to let go of what
  they make again (the reader its book, keeping its place;
  `Screen::on_squeeze`, `RoomSteps::SQUEEZED`) and closes them where
  that is not enough, until there is enough; everything comes back once
  it closes (`ScreenMode::memory`, `Env::room`).
- A screen opened again while it is open is gone back to, not stacked
  (several *Transfer over Wi-Fi* screens one over another took as many
  presses of *Back*): the screens above it close and it hears the
  params (`Screen::on_reopen`). Each screen says how many of it may be
  open (`ScreenSpec::instances`, `Instances`): one for each set of
  params (the default), one at most (the transfer, Wi-Fi, Bluetooth,
  store and diagnostics screens and other roots) or many (the
  keyboard). Params are alike in any order.
- Plugins see what the buttons are good for: `Capability::PAGE_KEYS`
  and `Capability::BUTTONS` (enough for every screen without touch),
  from the buttons the shell lists.
- Less memory at rest: actions, screens, menu entries and icons can be
  `static` lists that stay in flash (`FixedAction` and
  `Registrar::add_actions`, `FixedScreen` and `add_screens`,
  `add_menu_entries`, `add_icons`; `MenuEntry::new` is `const`), as the
  kernel's and the plugins' are now. `Actions::get` gives an `Action`
  of either kind; `Env::screens` gives a `ScreenList`. Menus no longer copy the labels of the actions and
  settings they list (`Env::menus` gives a `MenuView`). Translations
  keep only the languages in use as catalogs; English catalogs a
  language covers drop their index; the indexes of the catalogs built
  in are made when PaperPaw is built (`FixedIndexes`,
  `KernelBuilder::catalog_indexes`). The X4 emulator's home at start
  takes some 30 KiB less.
- Room for a screen that needs memory may also draw frames in more,
  smaller bands (`RoomSteps::BANDS`), as they were once it closes.
- Plugins can keep the device awake for a while (`power::WakeHold`,
  `Command::KeepAwake`, `Cx::keep_awake`): bounded, renewed by id, a
  screen's ended with it, never with a battery nearly empty. *Let
  plugins keep the device awake* (on) turns it off for all; the usage
  says each plugin's awake time.
- Hot code in code plugins (`paperpaw_sdk::hot!`): the X4 runs it from
  internal RAM, a bounded arena whose rest goes to the heap. Plugin
  files are of format 3 (the hot range and its patches); RISC-V plugins
  link with `--no-relax`.
- Room in the plugin interface for what comes later without a new one:
  `Command::Extended`, `Screen::on_extended`, `Contribution::Extended`,
  `Capabilities::has` with `Capability` constants, wider `Moments` and
  `BookStyles`, `Storage::write_at` and `truncate`.
- The reading settings in pages of their own: *Text*, *Layout*,
  *Hyphenation*, *Publisher's formatting*, *Display*, *Status lines*,
  *Marks in the text*, *Reading aloud*, *Reading aids* and *Behaviour*,
  each short, its rarely needed settings one level down under *More*
  (or with the others, with *Show advanced settings*). Settings about
  reading came along: the colours of text and paper (from *Look*),
  night mode, text darkness, pictures in grays and full refresh (from
  *Display*), the voice (from *System*) and *Battery low at* (now with
  the status lines). SDK: groups within groups
  (`SettingGroupSpec::within`, `Settings::parent_of` and `within`),
  `groups::READING_TEXT` and the other parts of `READING`;
  `STATUS_LINES` and `MARKS` are parts of it.
- Balanced line breaking: paragraphs are broken over all their lines,
  so lines end more evenly and justified text has fewer wide gaps;
  *Line breaking* can be set back to *Fast*, line by line
  (`core.line-breaking`). SDK: `text::wrap_lines_balanced`,
  `layout::Style::balanced`.
- A book's own styles one by one: with *Publisher's formatting* on, its
  alignment, indents, text sizes, spacing, fonts, margins and line
  spacing can each be left to the reader (its line spacing is by
  default). SDK: `layout::BookStyles`, `Style::book`, settings
  `core.book-*`.
- More of a book's formatting comes along: margins at the sides (block
  quotes, poems, lists set in), hanging indents, line heights,
  superscript and subscript, underline and strike, numbered and nested
  lists, and a table's rows each on a line of their own. SDK:
  `Block::margins`, `Block::line_height`, `TextStyle::SUPERSCRIPT`,
  `SUBSCRIPT`, `UNDERLINE` and `STRIKE` (a `TextStyle` has 16 bits now).
- Letter spacing, and the size of indents and of the space between
  paragraphs, as settings (`core.letter-spacing`, `core.indent-size`,
  `core.paragraph-space`); their defaults keep pages as they were.
- Kerning and ligatures: pairs such as *AV* and *To* are set as the
  font kerns them (its GPOS `kern` feature, else its legacy `kern`
  table), in books and menus, and *fi* and *fl* (*ff*, *ffi* and *ffl*
  where they take the room of their letters) are drawn as ligatures in
  books where the font has them. Settings *Kerning* and *Ligatures*
  (advanced, per book, on: `core.kerning`, `core.ligatures`); books are
  laid out again once. Font packs keep a kerning table of classes, read
  in place (still version 1: older firmware ignores it, older packs read
  without kerning); the default packs grow by about 134 KB of flash, no
  memory. SDK: `Font::kern`, `BitmapFont::kern`, `OutlineFont::kern` and
  `kerning_pairs`, `PackBuilder::kerning`, `charset::LIGATURES`,
  `FontHandle::kern`, `unkerned` and `ligatures`,
  `layout::Typesetter::kern`, `text::wrap_lines_kerned` and
  `wrap_lines_balanced_kerned`, `PageType::kerning` and `ligatures`,
  `settings::KERNING` and `LIGATURES`; `text::measure`, `draw_text` and
  `FontHandle::measure` and its drawing kern the text they get.
- Hyphenation for more languages, as store plugins of their own: French,
  Spanish, Italian, Dutch, Portuguese, Swedish, Danish, Norwegian
  (Bokmål), Polish, Russian and Ukrainian. The firmware keeps
  German and English; each language takes flash, almost no memory.
- Settings *Letters before hyphen* and *Letters after hyphen*
  (advanced, 0 to 5; 0: the language's own): how short a piece of a word
  may be. Books are laid out again when they change. SDK:
  `settings::HYPHEN_MIN_LEFT`, `HYPHEN_MIN_RIGHT`,
  `PageType::hyphen_minimums` and `patterns`,
  `hyphen::Patterns::with_minimums`.
- *Text contrast* (Settings, Reading): book text lighter, normal, darker
  or darkest, its letters' edges on a contrast curve, finer than *Font
  weight* and together with it. Normal draws as before; each book may
  have its own.
- *Smooth text* works on every screen now: off draws book text in black
  and white, without gray edges, also where the screen has grays (the
  Boox), on at first there. On the X4s it stays off at first, and off,
  pages of text turn without the slower refresh in grays. It is a
  setting of the kernel now (`core.smooth-text`, was
  `reader.smooth-text`, taken over), each book may have its own. SDK:
  `settings::SMOOTH_TEXT`, `settings::TEXT_CONTRAST`, `text::ToneCurve`,
  `FontHandle::draw_toned`, `Canvas::mono_coverage`.
- Room for USB developer tools as a plugin ([ADR 0031](docs/adr/0031-developer-link.md)):
  `Uses::DEVELOPER`, `Capability::USB_LINK`, `Command::Developer` with
  `developer::open`, `write`, `screen` and `close`, heard as `DevEvent`.
  Reserved: this release answers that the link is not available.
- SDK: `Ui::filled_button` and `filled_icon_button`, filled as an `on`
  button without saying it is on (a key whose fill is the focus).

### Fixed

- The X4s no longer crash when a server does not answer at all: the
  radio starts anew for such a connection, and the clock's question to
  the time server, kept from before, then reached into the new network
  stack. Handles of what the stack made now carry its number, and a
  later stack passes over an earlier one's (also in beta.7).
- A download to a file, or an upload from one, no longer breaks off
  after a time in all (10 minutes for downloads, 30 s for uploads from a
  file): it fails only once no byte came for a minute, so a large update
  on a slow network goes through. SDK: `HttpOptions::timeout` means that
  for them.
- The network picked last (*Join first*) is looked for again when a scan
  misses it, and gets a second try when joining it fails, before another
  network joined before is joined: the first scans after the radio came
  on often miss a weaker network, and one of its access points, weak or
  far, refuses a join now and then.
- The X4s leave the Wi-Fi network before they start again (after an
  update, a restart from the settings) and before they sleep. A router
  that was not told (a FRITZ!Box) refused their joins for a while after,
  so they joined another network or none.

## [0.5.0-beta.7] - 2026-10-25

### Added

- The notices of everything PaperPaw ships: the crates of the firmware
  and of the apps, each under its licence with its copyright lines and
  each licence's text once, and Mbed TLS, ESP-IDF's radio libraries and
  bootloader, the FreeInk and OpenX4 display code and Papyrix. Made from
  the lockfiles by `tools/licences/build.py` into NOTICE and About
  PaperPaw, Licences (packed, written out to read part by part); CI
  fails where a crate that ships is missing. The web, emulator,
  handbook and core library archives carry LICENSE and NOTICE.
- Weather names Open-Meteo, Wikipedia names each article below its
  summary, with their licences.
- The emulator starts online with `--wifi online` (over `--stdio`,
  `"wifi": "online"` in `start`): the X4s on USB, their radio on and in
- The emulator starts online with `--wifi online` (also given before
  `--stdio`, for the devices it starts; or `"wifi": "online"` in
  `start`): the X4s on USB, their radio on and in
  a network `Workshop Wi-Fi` joined before, so plugins' requests go out
  at once; Wi-Fi switches off and on in the settings as on a device.
  Without it, nothing changes.
- Plugins can be published from GitHub: tag a release in the plugin's
  own repository (topic `paperpaw-plugin`) and paperpaw.org lists it as
  "Community, unreviewed". `tools/dev/templates` has the release
  workflow that zips the folder, builds and signs a code plugin and
  makes the release; the handbook and the plugin skill explain it.
- `paperpaw-dev check-publish` (and `paperpaw-sign check-publish`)
  checks a plugin's repository and its release files by the plugin
  store's rules, the site's own (`tools/publish/store-rules.json`, served
  by paperpaw.org too), and says what to change; the release workflow
  runs it before it builds and on the files it made, and releases
  nothing where it fails. `paperpaw-dev new --github <id>` makes a
  repository that passes it, the workflow included.
- Community plugins: the store's new setting *Community plugins*
  (Settings, System, off at first) also lists plugins that their authors
  published on GitHub, which nobody reviewed. They are marked
  *Unreviewed*, with the repository they come from; a code plugin's page
  shows the key it is signed with.

### Changed

- Settings sorted where readers look for them (their ids kept, so what
  was set stays): pages of their own for the Keyboard (layouts, umlaut
  keys, size, shape of keys), the Screen lock (with Lock touch), Updates,
  the Sleep screen (from Display) and the Home screen (from Appearance);
  Record battery use with Power, Show hidden files with the Library,
  Number of test pages with Developer, the reading cat's settings with
  Reading statistics; Calibre and News in parts of their own in the
  Library. Hidden where they do nothing: the shape of keys on readers
  with buttons alone, the choice of gray covers on gray screens. Clearer
  names: Lines at the top and at the bottom (the status lines), Covers in
  the library and in catalogs, Selected row, Line thickness, List
  sections, the vocabulary's two switches; percentages with their unit.
- Keyboard size works on readers with buttons alone too: Large splits
  the rows into short ones (few presses along each, as before and the
  X4s' default), Medium splits the longest, Small keeps them whole as on
  touch screens (a lower keyboard, more presses).
- The on-screen keyboard: on readers with buttons alone (the X4s, the X3)
  the focus starts on *Phone*, where the text can be typed on a phone
  instead of a letter at a time; on the X4 Pro the keys are large by
  default (a size chosen before stays).
- On the Xteink readers the interface font, drawn from bitmap packs made
  from Source Sans 3, is PaperPaw Sans: its licence reserves the name
  for the unchanged font. A reading or status font chosen as Source Sans
  3 is kept. The unchanged font keeps its name where it is drawn as it
  is.
- A code plugin's key is trusted from its first install: an update or a
  new install after a removal signed by another key is refused with a
  short warning, from any library; there is no *Install anyway* any
  more.
- Firmware is installed only with the battery at 20 % or more, or on
  USB: below it the kernel refuses any install (the update, another
  firmware), checked as the writing would begin, and *Update* says
  "Charge to 20 % or plug in to install." Downloading goes on. SDK:
  `InstallFailure::LowBattery`, `firmware::INSTALL_BATTERY`,
  `firmware::battery_allows_install`.
- No surprise restart after an update: while the new firmware runs on
  trial, without its code plugins, the X4 shows "Finishing the
  update…" with a bar that fills, takes no input meanwhile, and starts
  again from there once the update is kept, with its code plugins. SDK:
  `builtin::screens::FINISHING_UPDATE`.
- Devices are counted without being known: the update search says
  whether the reader asked (`trigger`: `auto` or `manual`) and of which
  periods it is the device's first search (`first`: `day`, `week`,
  `month`, in UTC; left out while the clock is not set). No id and no
  request of its own; the device keeps only the date of its last search.
- Back to CrossPoint without a computer: *Update* installs another
  firmware's image from the card (not signed, only as chosen, with a
  warning), checked whole for this device first. SDK:
  `FirmwareCommand::InstallForeign`, `firmware::ForeignImage` and
  `firmware::ForeignCheck`.
- Bluetooth Low Energy for plugins (ADR 0027): `paperpaw_sdk::bluetooth`
  scans, connects and reads, writes and subscribes to a device's
  characteristics, pairs (always confirmed by the reader in the kernel's
  own question), advertises and serves GATT services, for plugins that
  declare `Uses::BLUETOOTH`. The kernel owns the radio, shares it
  between plugins, lets each see only the devices it found or was given,
  keeps bonds with the passwords and keys and switches the radio off
  when nothing needs it. Cheap by default: scans and advertising end
  after a while, passive and seldom unless asked, links at long
  intervals, the radio at the cheapest power that serves every plugin,
  and its time on counted (`Usage::bluetooth_ms`). A bonded remote's or keyboard's keys are
  buttons of the key maps (`button.hid.right`, `button.hid.page-down` …,
  pages turned by default). The emulator has made-up devices for the X4
  Pro's profile: a page-turner (`ble-key:right` in scripts) and a
  heart-rate belt.
- Bluetooth page (Settings, where there is Bluetooth): the devices bonded
  with the reader, Add a device (a search, then pairing confirmed on the
  reader), forgetting one (the menu button or a long press), which
  plugins may use a sensor, and Remotes: when a remote's or a keyboard's
  keys work (when switched on, while reading, or always). A remote that
  falls asleep connects again by itself when it wakes, as long as its
  keys are wanted.
- **Code plugins must be built again.** The plugin interface is now 3
  (`Command`, `Env`, `Capabilities`, `MenuNeed`, `Usage`, `Work` and
  `FirmwareCommand` changed, for Bluetooth and another firmware): a code
  plugin built for an earlier release does not run on this one until it
  is built again with this release's SDK, and the plugin manager says
  so. PaperPaw's own store plugins come as updates. `Work`'s `network`
  and `charging` became `needs` (`Needs::NETWORK | Needs::CHARGING`).

### Fixed

- Uploads over Wi-Fi no longer stall when the card fails to take a part:
  the X4s answer at once, while the file still comes (*500* where the
  card failed, *507* where it is full), instead of holding the upload
  until it timed out. The X4 Pro tries a write to a card still busy
  writing more often before it gives up, as some cards are while the
  radio sends.
- Status lines stand every item on one line: the battery, icons and bars
  centred on the figures of the time, the page and the percentage, and
  those in the middle of the line, to the pixel. The battery stood a
  pixel higher than its percentage.
- Customise menus names the menus of the font choosers (their way to
  more fonts) as the settings are named, e.g. *Status lines › Font*,
  instead of showing `core.status-font` and `core.reading-font`. A test
  walks the menu editor, every settings page and chooser and every
  menu in each language shipped and fails on an id shown as a name.
- Test builds for the X4 Pro and the X4 Classic (with remote control
  over USB) took no updates over the air ("OTA: no partitions for
  updates"): their image's two data segments hid which half of the
  flash runs. The firmware now finds it either way. Released firmware
  was not affected. The release now fails for an image that could not
  take the next update (`paperpaw-sign check-image`: two app slots in
  its table, one data segment, room in either slot and CrossPoint's),
  and host tests run whole updates with every partition table shipped
  and CrossPoint's.

## [0.5.0-beta.6] - 2026-10-25

### Added

- The web flasher can put every store plugin on a reader at once: it
  writes the plugins chosen into the reader's empty second app
  partition, where all of them fit (about 4.8 MB on the X4), instead of
  the room behind the firmware (about 1.9 MB). PaperPaw installs them
  onto the card at its first start and only then frees the partition
  for updates; a power cut in between installs them at the next start.
  The room behind the firmware is still read, for older flashers.

## [0.5.0-beta.5] - 2026-10-25

### Fixed

- After the Wi-Fi link is lost and joined again, the X4s count as
  online only once the router gave their address again. Before, they
  were online at once with the old one, which the router's answer then
  took back, so a request made in between failed.
- Touch lock: the way out it explains (tap twice, hold the second tap)
  unlocks; it wanted a third touch held after two taps, which still
  works too.
- Choosing another Wi-Fi network while joined to one works on the X4s:
  they leave the first network before joining, instead of failing with
  "did not answer". Networks are joined with the security the scan found
  (WPA and mixed WPA/WPA2 ones too), and a password that was not taken
  for another reason than being wrong is offered again, not typed anew.
- News: articles with a long body in a CDATA section (WordPress feeds, The
  Verge) show their whole text; the reader dropped any section over 4 KiB
  and fell back to the summary. The XML reader now passes a long CDATA
  section on in pieces, in the same small memory. Dates, Atom
  and RSS, show short as the language writes them (Sat 24 Oct 2026).

### Added

- The web flasher can write almost the whole store with the firmware:
  the X4s take its bundle of plugins from the other app partition too,
  empty on a first flash (8 MB), not only from behind the image (about
  2 MB), and say so in the image for the flasher to find.

- Switching from CrossPoint without a computer: each release has an
  install file per device (`paperpaw-<device>-<version>-install.bin`)
  for CrossPoint's *SD firmware update*, and CI fails a build that no
  longer fits CrossPoint's app slot. PaperPaw runs in the partitions it
  finds; where no room for code plugins is left there (the X4), the
  store says so and refuses them.
- The X4 Classic's firmware comes with each release, marked
  experimental: built, not yet tried on a real device. It follows its
  stock firmware where the FreeInk SDK and Papyrix agree: the panel at
  10 MHz with its factory frame rate, its controller as the factory's
  NVS names it (else as it answers, else a UC8279), the charging pin
  high while charging, the card started up to four times, and its
  power kept on through sleep. Its partitions keep the factory's NVS
  whole.
- The Xteink X3's firmware comes with each release too (`cargo x3`),
  experimental and not yet tried on one, without store plugins yet: the
  X4's chip and buttons, its UC8253 panel at 792 x 528 in black and
  white, a fuel gauge and a clock; an emulator profile too. What is not
  known without a device is listed in `shells/xteink`.
- The plugin store has five more groups, so every kind of plugin has a
  place: notes, learning, news, tools and games, before system. Of
  PaperPaw's own plugins, vocabulary moved to learning; news, wallabag
  and the browser to news; readwise and joplin to notes.
- paperpaw-dev checks the group in `store/store.ini` (`package.store-group`).

## [0.5.0-beta.4] - 2026-10-24

A beta for updates over the air, from testing the update to beta.2 on
the X4 and X4 Pro, and for the largest interface text, which every
screen now takes. Code plugins built for earlier betas run unchanged:
the plugin interface stays 2.

### Fixed

- After an update passed its first minute on trial, the X4s restart by
  themselves, so that code plugins (store tools) are there again; before,
  they were missing until the next start.
- A download that got no answer at all (seen on the second connection
  after the update check, until a restart) is tried once more with the
  radio started anew.
- After an update the device no longer asks to make Diagnostics the home
  screen: it no longer binds the home slot, and the plugin manager offers
  a new screen for a slot only when it is bound as high as the one in use.

### Added

- SDK: `UiStyle::noted_row_for` makes a list's rows as high as its
  notes need, up to `NOTE_LINES_MOST` (4) lines, and
  `ListView::set_row` sizes them anew; a row's note shows as many lines
  as its row holds.

### Fixed

- After an update the device no longer asks whether to make Diagnostics
  (or any screen bound only as a fallback) the home screen: Diagnostics
  binds no slot, and a newly offered screen is asked about only where it
  binds the slot as high as the one in use. `Env::slot_screens` lists
  the highest priority first.
- At the largest interface text on small screens the notes of About,
  Keys, News, a catalog's fields, Plugins, Profiles, Cards and Wi-Fi
  show whole.
- After an update over the air the plugins installed from the store
  (statistics, widgets, the syncs) work again by themselves: once the
  update ran its 30 seconds of trial, which go without them, the X4s
  start again, at once or once the book open is left. Before, an update
  that kept the plugins' builds left them off until the next start.
- A download that gets no answer at all (seen with updates on the X4
  and the X4 Pro: connected, then nothing until a restart) is made once
  more after the X4s start their radio anew, instead of failing with
  "The update could not be downloaded". Its log line says where it was
  connected and how much memory was free.

## [0.5.0-beta.3] - 2026-10-24

A beta with fixes from testing beta.2: a press made while a new screen
still refreshes is no longer lost, and more screens fit at the largest
interface text. Code plugins built for earlier betas run unchanged: the
plugin interface stays 2.

### Fixed

- A press made while a new screen was still being refreshed (opening
  Settings on the X4 takes a full refresh) was dropped, so the first
  press after opening a screen sometimes did nothing. The X4s now count
  a screen as shown when its refresh begins.
- At the largest interface text more screens fit: the statistics'
  calendar (its days no longer shrink to dots) and overview, the device
  check, the store's plugin page and source, the Calibre hint, the file
  finder and catalogs, the phone step of Wi-Fi, the statistics' sync
  and the profiles. Store plugins changed so carry version 1.0.1.
- Vocabulary 1.0.1: a card answered Again waits its ten minutes before
  it comes back (also published on its own, between betas).
- A store plugin released on its own is built for the X4 Classic too.

## [0.5.0-beta.2] - 2026-10-24

The second beta: the fixes from testing beta.1, as a new user would use
it, from the web flasher to the plugin workshop. Every plugin now has
a version of its own, so a fix to a store plugin reaches readers as an
update, with a release or alone between releases. Code plugins built
for beta.1 run unchanged: the plugin interface stays 2.

### Added

- Look for updates (Update, off unless chosen, also asked in the
  first-run setup): about once a week the device asks for a newer
  PaperPaw by itself, only while Wi-Fi is on anyway or it charges, never
  waking the radio on the battery. What it finds waits on the Update
  page, its entry in the settings marked New; nothing over reading.

- Every plugin has a version of its own, apart from PaperPaw's: its
  crate's, which its manifest says (`package_version!()`, the built-in
  plugins too), 1.0.0 to start. A release builds a store plugin that did
  not change again for its firmware at the version it has; the plugin
  library adds the build to it. CI fails where a store plugin changed
  since the last release and its version did not (a commit says
  `Plugin-unchanged: <id>` for a change no reader sees).
- A single store plugin can be updated between releases
  (`store-plugin.yml`): built for a release's firmware, signed and
  published as a release of its own, `<id>-v<version>`, which the plugin
  library imports.

- Restart now, last in the settings: asked once, it saves what is open
  as when the device goes off and restarts PaperPaw.
- The first-run setup's pages fit at the largest interface text on the
  X4: laid out closer there, with the buttons side by side where needed.

- A book downloaded from a catalog (OPDS) shows how far it came: its
  percent and a bar where the size is known, the bytes so far where not.
  Back stops the download, and nothing of it is left in the library.
- SDK: `HttpRequest::progress(&event)` and `HttpProgress` tell a plugin
  how far a download came (at most every 5 % or 2 seconds, never after
  the reply); shells report it with `Kernel::http_progress`. Only new
  items: the plugin interface stays 2.

- The emulator answers the requests of PaperPaw's own plugins offline
  with `--network fixtures`: a small plugin library (a data plugin with a
  long summary, screenshots and a source, a code plugin, a long name),
  so that the store's screens can be drawn and checked without a
  network, alike everywhere. On the X4 profiles the pretend radio's test
  network is known from the start, so it is joined when a plugin wants
  the network. Besides the library it answers an OPDS catalog (Project
  Gutenberg's, with books to download), fonts to download, KOReader's
  progress sync, the weather, Wikipedia and a news feed.
- `paperpaw-dev --network fixtures` checks offline with these answers:
  `visual` draws the screens that show them with their data, and `reach`
  takes a screen out of reach whose requests went unanswered for an
  error. `visual.overlap` now also finds a text under a row, a control
  or another text. CI runs `visual` and `reach` so.

### Fixed

- In the first-run setup's Add more step, plugins switched on are added
  also when you leave the page with Back, not only with its Add button;
  the setup's card says how many are being added, then Added.

- A code plugin built for the X4 or X4 Pro with the SDK's `outline`
  feature now stops with a message that says to build for the readers
  without it (it is in a plugin's fingerprint, and the readers draw font
  packs): on the X4 it failed in `once_cell` (no `compare_exchange` on
  its chip), on the X4 Pro it built a file the reader never loads. The
  emulator's build takes it from a target section of the manifest
  (docs/plugins.md, "The crate").

- On the X4 and X4 Pro on battery, clicks made while the screen refreshes
  are no longer lost, and a quick click is no longer taken as a long
  press (which opened the quick panel while turning pages).
- On the X4 and X4 Pro, a click of Right while a page is shown is no
  longer taken for a long press (which opened the quick panel).
- Clicks made while the X4 was busy, up to about a second before it
  showed the next page, count; only a long tail of them is dropped.
- Reading a long book on the X4 no longer stalls for seconds every few
  pages while its pages are counted in the background: a new chapter's
  cache took 4 to 8 seconds, now under one.
- Moving through the library and its shelves on the X4 takes a fast
  refresh of the two rows or covers that changed (some 0.6 s), not a
  frame in grays or a full refresh of the screen (1.7 to 2 s). Covers
  there show in black and white; a setting (Library › Covers) shows them
  in gray, finer but slower to move over.
- On the X4, pictures in gray (the sleeping cat, covers) are as dark as
  on the X4 Pro; they were faint.
- Small changes on the screen (a download's progress, a focus moved, the
  clock) no longer make the whole screen flash every few times: only
  page turns count towards Settings › Full refresh, as it says. A book's
  download refreshes its progress line without a flash, and goes faster.

## [0.5.0-beta.1] - 2026-10-24

This is a beta, for testers and developers. It is the first release of
PaperPaw's firmware for the Xteink X4 and X4 Pro, and it brings much
that is new on every device. Expect bugs, and expect some features to
change before 0.5.0. Keep a copy of your books and notes. When
something goes wrong, please open an
[issue](https://github.com/hthoene/paperpaw/issues). Say which device
and version you use and what you did. On the X4, a crash is written to
`.paperpaw/crashes.txt` on the card at the next start; please add that
file to your report.

### Reading

- Holding a page button goes to the next or previous chapter; *Hold a
  page button for the next chapter* under Controls turns this off.
  *Next chapter* and *Previous chapter* can go on any button or gesture.
- On the X4, the front buttons Left and Right turn pages in a book.
  *Swap the page buttons* under Controls turns them round.
- Holding *Select* while reading sets or removes the page's bookmark.
  *Bookmark* and *Home* are in the reader's bar. *Go to a percentage*
  and *Go to page* are in *More*.
- New status items: the page in the chapter, the pages left in it, and
  a short bar for how far into it you are. *Now reading* shows the
  pages and minutes left in the chapter.
- The status lines have a font of their own: as the menus, as the book,
  or any font installed. They sit closer to the edge, so a page holds
  more text. By default they are calm: the time and the page.
- More control over the page: margins at the top and bottom, space
  between paragraphs and indents as two switches, and line spacing down
  to 80 %. A live preview above the reading settings shows each change
  at once.
- A new text size lays out the open book again at once. *Turn the
  screen* and the full refresh can be a book's own, with *Only this
  book*.
- Text on black and white panels is fuller; *Text darkness* under
  Display chooses light, normal or full. *Smooth text* shows pages in
  four grays with soft letter edges, but pages turn more slowly.
- *Marks in the text* sets how each kind of mark looks: highlights,
  links, search hits and the words plugins mark. Highlights now show on
  black and white panels. *Mark vocabulary* marks the words you looked
  up.
- The dictionary can look a word up on Wikipedia: choose *Online* in its
  panel, and the summary shows beside the dictionary entries.
- Books whose pages go from right to left mirror taps and swipes.
- Font size offers only the sizes the reading font has: on the X4 and
  X4 Pro, whose built-in fonts stop at about 12 pt, no larger ones that
  looked the same. A size set before past them gives way to the
  largest. At the largest, the help says that a font with larger sizes
  gives more, and *Discover fonts* on the reading settings opens the
  font browser (or its page in the store, where it is not installed).
  Scalable fonts keep every size.
- A book that cannot open for lack of memory (on the X4, while Wi-Fi is
  on) says so. Before, the device could restart, or the book opened at
  its start and lost your place. A book whose file is gone says that
  too.

### Books and fonts

- HTML, FictionBook, Markdown and comic books are plugins of their own,
  so each can be switched off or replaced. Markdown files show the
  pictures beside them.
- *More fonts* below the font choice lists the fonts of paperpaw.org's
  font library. Download one and read with it at once. The first are
  Atkinson Hyperlegible Next, Crimson Pro, EB Garamond, Figtree, Fredoka
  and Nunito.
- On the X4, font packs on the card add other scripts. Packs for
  Cyrillic and Greek make Russian, Ukrainian and Greek books, menus and
  keyboards work.
- On the X4 and X4 Pro, pages with pictures, covers and sleep screen
  pictures show in four grays. *Pictures in grays* under Display does
  the same for screens with pictures, such as the library's covers.
- Pictures on the X4s are dithered in a new way: line drawings stay
  clean, and covers and photos keep their tones. Night mode leaves
  pictures as they are, unless *Pictures in night mode inverted* is on.
- Damaged or crafted books, pictures, fonts and dictionaries can no
  longer restart the X4 or use up its memory. What cannot be read is
  left out, and the rest carries on.
- Many fixes in EPUB, FictionBook, plain text, Markdown, comics and XTC.
  Plain text reads UTF-16 files, and file names that begin with a letter
  beyond ASCII ("Ärger.epub") no longer crash the X4.

### The library

- The library is a calm list. The book you are reading comes first, as a
  card. Then come all books with cover, title, author and progress; a
  check marks finished books, and *New* marks books added recently.
- Filters above the books: All books, Reading, Unread, Finished, Next in
  your series, Shelves and Find a book. Folders, favourites, authors and
  series are on the shelves. *Find a book* picks a book by a letter.
- *Show books as: Covers* shows a grid of covers with the progress on
  each; the books of a series form one stack.
- Hold a book for its sheet: go on reading, its series, finished or
  unread, favourite, information, archive and delete. Deleting asks
  first and removes what is kept for the book.
- *Open with* lists the reading apps that read a book. The reading
  settings choose a default app and one per format, and the file manager
  sets one per folder.
- The list of books is kept up to date in the background, never while a
  book is open. Large libraries work on the X4, and the library holds
  next to nothing while you read.
- What is kept for a book moves with it. *Clean up storage* under System
  removes what is left of books that are gone; it also runs by itself
  while the device charges, at most once a week.
- The file manager opens folders of any size a page at a time. Moving a
  folder takes the books' progress and notes along.

### The home screen and widgets

- The home screen is a grid of 3 columns by default, with up to eight
  pages. *Edit home screen* in the main menu edits it, by touch or with
  the buttons. On the X4 Pro, dragging keeps up with the finger.
- Many new widgets: an analogue clock, a word clock, a world clock, a
  calendar, the year so far, a countdown, the moon's phase, a quote, a
  picture, a note, a to-do list, the next book, a series, favourites, a
  highlight, the library in numbers, the battery and PaperPaw's logo.
- The reading statistics add a daily goal ring, a reading calendar,
  books this year and a streak. A weather plugin (Open-Meteo) adds the
  weather now and a forecast; nothing is sent before you choose a place.
- Every widget has settings of its own: what it shows, its style, size,
  alignment, spacing and frame. The same widget can be placed more than
  once. *Widget look* and *Frames of widgets* change all widgets at once.
- The clock has clearly different sizes: small, medium (the default),
  large, and as large as fits. On the X4s a large clock is truly large.
- The home screen shows no focus until the first press; *Show the focus
  at once* brings the old way back. Fewer columns or rows no longer
  leave a page empty.

### The sleep screen

- By default the sleep screen shows a sleeping cat, drawn for PaperPaw.
  The cover, pictures, a message and the other choices remain.
- *The page with a picture*: the page you read stays, and a picture or a
  widget goes over it. You choose where it sits, its size and how much
  of the page shows through.

### Menus, settings and the look

- A new look, rounder and friendlier: soft corners, pills for buttons
  and switches, plain headers, faint lines between rows and margins of
  4 mm. The menus' font is Source Sans 3. Empty screens say what belongs
  there and how to fill it.
- With advanced settings, Appearance sets the header, corners,
  highlight, lines between rows, cards and the size of titles, margins
  and rows. Themes are listed under *Theme*; the Pebble theme comes
  switched off.
- No label is cut any more. Long titles and button words get a little
  smaller or wrap onto a second line, at every text size and language.
- Hints: the first time a screen opens, a short note says what its
  controls do. *No more hints* switches them off, and *Show hints again*
  under System brings them back. Every plugin also says how it works, in
  the plugin manager and in the store.
- On touch screens, numbers you set by eye are sliders. A slider moves
  with the finger from where it was and never jumps. With the buttons,
  a changed value shows at once; *Select* keeps it and *Back* goes back.
- The quick panel and the reader's panel have wide sliders and larger
  buttons on touch screens. The front light is there now, no longer in
  the main menu.
- Under Settings, Plugins, *Screens*, you choose which plugin shows each
  screen: the home screen, the library, the reader, the lock, the
  keyboard, the sleep screen and more.
- Each settings page ends with *Reset to defaults*, the main page with
  *Reset all settings*. A setting with two choices switches at once, and
  moving past the last entry of a list goes round to the first.
- *Device name* under System names your reader for Calibre, the progress
  sync, Wi-Fi transfer and your router. *First day of the week* sets the
  start of the week for the statistics and calendars.
- New status items: *Battery when low* shows the battery only below a
  level, and a Wi-Fi sign shows while the device is in a network.
- A plugin's settings group that names no place on the settings' main
  page is listed at the end of *Customise*, by name, with a plugin's
  icon, instead of first among the books' groups.
- No explanation is cut short any more. Every setting's description fits
  the two lines of its row on the X4 and the X4 Pro, in English and
  German, and a test keeps it so. The text below *Progress sync*'s rows
  takes the room there is; where it still has none, a row *More about
  this* shows it whole.
- First-run setup: the sample of the font size is in the book's font,
  at the size chosen, as the reader shows it: no longer in the
  interface's font, its second line cut. The card grows with the size
  (fewer words where there is no more room), and follows the size as it
  changes.
- Every step of the text size changes the text, in the setup, the
  settings, the quick panel and with the reader's buttons: sizes the
  font of the X4 and X4 Pro draws alike (9 and 10 pt on the X4 Pro) are
  skipped. Sizes kept stay as they are.

### Touch, buttons and the keyboard

- The quick panel by the buttons: hold Right (on the X4), or choose
  *Quick panel* at the top of the main menu. The tips on the home screen
  and in a book say so.
- A number in the first-run setup and in a menu is changed with the
  buttons as on the settings pages: Select starts, left and right change
  it, Select keeps it, Back goes back to the value before, and the hint
  bar says so. Left and right no longer stay on a number in the setup,
  and the quick panel's sliders name Keep and Cancel too.
- The keyboard on touch screens has round keys and takes less of the
  screen; *Shape of keys* makes them a little taller or wider.
  *Keyboard size* (Small, Medium, Large) makes it a quarter lower or
  higher: with round keys about 30 %, 40 % or 50 % of the screen, never
  keys below 4 mm. German has ten keys a row: hold a, o, u or s for ä,
  ö, ü and ß, or switch on *Umlaut keys*. On narrow screens, a row too
  wide is split in two. The keyboard takes up to 2,000 characters.
- Touch lock: *Lock touch* in the quick panel and the reader's menu.
  Taps and swipes do nothing, and the buttons still work. To unlock, tap
  twice and keep the finger down, or hold the power button or the home
  key on the X4 Pro. However it was switched on (the quick panel, the
  reader's menu, Settings, a button), it says how to unlock when it
  turns on, and says when it is unlocked. While it is on, a tap or a
  swipe says it again, at most every ten seconds and calmly, never
  between the steps of the way out. It works on slow screens too: the
  two taps may be up to half a second long and a second apart, and a
  hold unlocks as soon as it is long enough if the finger moves a
  little.
- On the X4, a row above the buttons names what each does; *Show what
  the buttons do* under Controls switches it off.
- Buttons answer better: a press counts as soon as it holds, and presses
  made before a new screen shows are dropped. *Button response: Faster*
  reads the buttons more often, for a little more battery.
- On the X4, *Buttons and gestures* refuses a change that leaves no
  button to select or go back with. *Reset buttons and gestures* takes
  every change back.

### Tools

- Flash cards, in the vocabulary tool, with spaced repetition (FSRS,
  Anki's scheduler). Edit decks with your phone, also from CSV or an
  Anki deck. Words you look up can become cards, and a card due can be
  the sleep screen.
- Reading goal: a daily goal for the reading statistics, and a cat whose
  mood follows today's reading. The *Reading cat* shows it on the home
  screen or as the sleep screen.
- Joplin: a book's highlights and notes become a note in Joplin, through
  Joplin Cloud, Joplin Server or WebDAV. It is off until you want it.
- Battery use records the battery while the device is awake. It shows
  the use per hour and per 100 pages, how long the battery lasts, and
  which plugins kept the device busy.
- A guided device check in the diagnostics tool tests every part of a
  device, one page at a time, and writes a report to the card.

### Wi-Fi, sync and the store

- Wi-Fi transfer opens in the library folder (there from the first
  start) and says where uploads go; books sent elsewhere are pointed to
  the library, gently. Files and folders can be moved and renamed there,
  their books' progress and notes going along, and each file sent shows
  how far it is, can be tried again alone, and is in the list at once.
- Wi-Fi goes off as soon as you are back home or in a book and nothing
  uses the network, not a minute later. On the X4 this frees its memory
  before a book opens: a book opened right after the store could run
  out of memory. On the X4 a download also takes less memory at once.
- The store tells betas apart: a plugin's versions are compared whole,
  pre-release included (`0.5.0-beta.2` after `0.5.0-beta.1`, `0.5.0`
  after both), as firmware updates are. A newer beta is offered only
  where the device runs a beta or takes betas (*Beta updates*). Its
  message for code built for another plugin interface or compiler says
  so.
- The store marks PaperPaw's own plugins *By PaperPaw*, on their row
  and their page. The mark is taken only from PaperPaw's own library
  (paperpaw.org, over HTTPS), and for a plugin with code only where its
  key is PaperPaw's: a list from elsewhere cannot give it.
- A code plugin's version is its `plugin.ini`'s, everywhere: the
  plugin manager, the store's updates and the transfer's list all go by
  it. Code whose manifest names another version runs at `plugin.ini`'s,
  with a warning in the log; `paperpaw-dev package` finds it before.
- Wi-Fi on the X4 and X4 Pro: the radio is off while you read. It comes
  on when something needs the network, joins the strongest saved
  network and goes off a minute after the last use. HTTPS works, with
  the certificate checked. The X4 sets its clock once online.
- Set up Wi-Fi with your phone: the reader opens a network of its own
  and shows QR codes, and you pick your network and type its password on
  the phone. Only secured networks are joined by themselves.
- Wi-Fi transfer and Calibre work on the X4 and X4 Pro. *Without a
  router* on the transfer screen opens the reader's own network. A
  scan that finds no saved network is made again, up to three times,
  before the device says there is none.
- Calibre: a burst of books no longer stalls the connection as often,
  a connection Calibre stops answering ends after 45 seconds, and the
  device tells Calibre how much its card really has free.
- The Wi-Fi transfer installs plugins from a computer too, such as
  one's own build. The version before stays until the new one is whole,
  and a plugin with code is installed only after you say yes on the
  device.
- In Offline mode, the store, catalogs, Calibre and Wi-Fi transfer offer
  to go online once, instead of failing.
- A plugin store on the device: *More plugins*, in the tools and in the
  plugin manager, with search, groups, sorting and *Update all*. Code
  plugins come from it too, signed by their author; an update signed by
  someone else needs your consent. The store updates only the plugins
  it installed: one put in place otherwise (the Wi-Fi transfer, a card,
  one's own build) is local, and the plugin manager says which is which.
- The firmware is lean: many plugins, such as the statistics, widgets,
  Calibre, KOReader sync, weather and rules, come from the store instead
  of being built in. This leaves more memory.
- A first-run setup guides you through the language, the buttons, the
  text size, the time zone, Wi-Fi, the keyboard and the screen lock.
  *Add more* offers four plugins most readers add.
- Progress sync (kosync) fetches the position on the server before it
  sends its own, so a further position from KOReader is kept. It no
  longer keeps the radio on while you read.
- KOReader's notes exchange, the statistics sync, OPDS, Wallabag, news
  and Readwise no longer delete anything when an answer or a file cannot
  be read.

### Power and battery

- Power modes: Normal, Saving, Offline and Automatic, in the quick panel
  or under Settings, System, Power. Automatic goes to Saving below 20 %
  and to Offline below 10 %. *Network per plugin* sets the network for
  each plugin.
- Quiet reading: background work no longer shows notices over your book,
  and it waits for a good moment, such as when the radio is on anyway.
- The X4 and X4 Pro save much more power: the processor sleeps on
  battery when there is nothing to do, the radio sleeps between beacons,
  and the panel's power goes off after a refresh.

### Devices and updates

- On the X4 and X4 Pro, a secure (HTTPS) connection takes about 15 KB
  less memory and the firmware about 180 KB less flash: mbedTLS is now
  built with the firmware, with only what its client uses and 4 KB for
  what it sends.
- Transfer over USB, further: a plugin's files are put together in a
  folder of the cable's own and installed only if they are those
  offered, each with the SHA-256 offered; the cable and the open Wi-Fi
  transfer never send the same plugin at once. Offers are read a line
  at a time (up to 64 files and 64 books each), and only closing the
  question refuses an offer.
- The Xteink X4 Pro is supported: its display, touch screen, buttons,
  card, battery gauge, clock chip, front light with warmth, sleep and
  Wi-Fi. Its front light dims evenly in single percents.
- The X4 is much faster: a page turns in about 0.7 s and the library
  opens in under 2 s. The screen can be turned.
- Updates from the internet: *Search online* on the update page finds a
  newer PaperPaw, and *Download* fetches it. Updates are signed, and one
  that fails early in its start is undone.
- An update waits while code plugins in use have no build for it yet.
  The update page names them and offers *Keep waiting* or *Update
  anyway*.
- Releases carry signed firmware images of the X4 and X4 Pro, which
  paperpaw.org offers for flashing in the browser.
- Safe mode: after two starts that fail, PaperPaw starts without code
  plugins. Holding *Back* on the X4 or *Home* on the X4 Pro at power-on
  does the same.
- QuireInk is now called PaperPaw. Its settings, statistics and
  passwords are taken over once, at the first start.
- Android: a beta of the app comes with each release, as an APK. It is
  now `org.paperpaw.app` and installs beside the old one. It runs on
  devices with 16 KB memory pages, and on Android 8 to 10 it can write
  to the shared storage.
- Transfer over USB on the X4 and X4 Pro: a computer at the other end of
  the cable installs a plugin or adds books. The device shows what is to
  come, with names and sizes, and writes nothing until you allow it,
  never over a book.
- Betas: *Beta updates* in the update's settings, off at first, asks
  for betas; a device on a pre-release gets the next anyway, and the
  device takes a pre-release only so, whatever the server answers.
  Betas come in order before their release (`0.5.0-beta.2` after
  `beta.1`, `0.5.0` after both) and show calmly, as `0.5.0 Beta 1`.
  Android's version codes follow the same order (of 1.2.3: beta.1 to
  beta.49, then `-dev`, then rc.1 to rc.48, then the release), and a
  version they cannot order is refused.
- The X4 keeps less in memory from the start: ids take two words
  instead of three, translations index their messages in two bytes
  where they fit, and actions are not kept twice. Reading an EPUB peaks
  some 24 KiB lower.

### Your data and security

- PaperPaw never writes over a file it cannot read, such as
  `paperpaw.ini`, passwords, notes, statistics, profiles or rules.
- Writes are safe against power cuts: a file holds its old content or
  its new one, whole, also on the X4's card, where a file moved or
  renamed is at its old place or its new one, never at neither. Going
  back to an older update no longer writes over data that a newer
  PaperPaw wrote.
- An audit of everything PaperPaw keeps found ways it could lose notes,
  places, statistics, cards or passwords; each is closed, with a test.
- A web request printed for debugging (in a log or a report) shows no
  password or key: not the values of its headers and params, not its
  body, not the query of its address.
  Among them:
  - *Clean up storage* removes by itself only what can be made again.
    The notes and places of books not found go only when you agree.
  - A reading place that cannot be read is never written over with the
    start of the book; it is kept aside and you are told.
  - Highlights and notes in EPUB books keep their place when the book's
    own styles are switched on or off.
  - Progress sync, KOReader's notes and the statistics sync delete
    nothing on a failed fetch or an unclear file, and ask where one side
    lost everything.
  - Lines PaperPaw does not understand, from a newer version or written
    by hand, are kept. An old copy of the passwords goes only once the
    passwords file holds it.
  - Removing a profile, a password or a vocabulary word asks first;
    *Reset to defaults* keeps folders, accounts and passwords; the file
    manager protects PaperPaw's own folders.
- While the device is locked, nothing shows above the lock screen but
  what the lock opens itself. The lock's code is kept with a random
  salt.
- A redirect no longer takes a password to another server, nor goes
  from `https` down to `http`. Calibre connects only to a computer you
  chose, and a plugin cannot change another plugin's passwords.
- A book's position and annotations say which book they are of by its
  content, its size and KOReader's partial MD5, beside the hash of its
  path they are named by (`library::Fingerprint`, `positions::book`,
  `annotations::book`). The reader takes it once when it opens a book
  (12 reads of 1 KiB at most) and keeps it with the position
  (`positions::save_with_book`) and the annotations
  (`annotations::stamp`, a line `# book <size> <md5>`). Both files keep
  their versions: PaperPaw of before reads them as before.

### For plugin authors

Plugins written against 0.3 need changes: the lists below name what
replaces what. A code plugin is a file bound to the plugin interface
(`native::INTERFACE`, now 2), and is rebuilt only when a release raises
it. `crates/paperpaw-sdk/api.txt` lists the whole public API, and
[Designing the plugin SDK](docs/sdk-design.md) says how it is kept from
breaking again.

#### Breaking SDK changes

**Handlers, commands and replies**

- Every handler queues its commands in `Commands` instead of a
  `&mut Vec<Command>`: actions, events, observers, work steps, voices,
  widgets' input and the installer. It has `push`, `extend` and `emit`
  and reads as a slice. Its fields are private, so it can carry more
  later without a new signature. A closure such as `|env, out|` stays as
  it is; a parameter typed `&mut Vec<Command>` becomes `&mut Commands`.
- Actions have one shape, with params everywhere. A handler is
  `.handler(|env, params, out| ..)`; `handler_with` and
  `ActionParamsHandler` are gone. `Command::RunAction(id, params)` takes
  the place of `RunActionWith`, and `Command::run(id)` runs one without
  params. A screen reads them with `cx.params()` in `on_action`.
- Commands stay small, 88 bytes in place of 216 (a test keeps
  them so): `Command::Http`, `Speak`, `Respond`, `Broadcast`, `Work`
  and `SetSecret` hold their payload boxed, made with `Command::http`,
  `speak`, `respond`, `broadcast`, `work` and `set_secret`. `HttpRequest`,
  `Speech`, `ServerResponse` and the new `Broadcast` keep their fields
  to themselves: they are made with their builders
  (`HttpRequest::param` is new; `Speech::in_language`, `at_rate`,
  `with` and `queued` take the place of `language`, `rate`, `params`
  and `queue`) and read with methods.
- Replies are read typed only. The reply to `Command::Speak` is a
  `Spoken` (whether it was spoken, and the speech's parameters); the
  string keys of replies (`http_params`, `server_params`,
  `socket_params`, `speech_params`) are the kernel's. `Speech::new`
  takes an `Event<Spoken>` (or an id).
- An HTTP answer's body is bytes as they came (`HttpReply::body`
  is a `Vec<u8>`, `text()` reads it as text), no longer decoded with
  losses, and it reaches the plugin's handler without a copy: an event
  may carry its data as it is (`Params::carrying`,
  `EventData::into_params`), which observers do not see. A failed
  request says why (`HttpReply::error`, an `HttpError`: no network,
  DNS, TLS, timeout, refused, replaced, cancelled, I/O), and a body cut
  to the most the request takes says so (`truncated`). `EventData`
  types are `Clone`.
- Signatures are numbers, made without allocating: a text
  filter's and a status line's signature returns a `u64` (0: does
  nothing), like a decorator's. `digest::Fnv1a64` makes one, fed piece
  by piece, as a `Hasher` or with `write!`. A menu item's label and badge
  are a `Cow<'static, str>`, so a fixed text costs nothing.
- The registrar's methods take ids as `impl Into<Id>` and text as
  `impl Into<Cow<'static, str>>` or `impl Into<String>`, so `&ID`, a
  setting or an event and a `&'static str` or `String` go in as they
  are. `Id`, `Setting` and `Event` are `AsRef<str>`.
  `SettingExtra::follows` and `shown_if` name the setting by its `Id`.

**Settings and services**

- Settings are declared as `static` lists and take next to no
  memory. `SettingSpec` has one builder set, all `const`: `switch`,
  `number`, `choice`, `text`, `language` and `list` take the id, title,
  default and group, and `SettingSpec::new` takes any kind and an `Id`
  made at run time. `description`, `advanced`, `scoped`, `guarded` and
  `kept_on_reset` are `const` too. Units, steps, sections, sliders,
  editors and secrets come from a `static SettingExtra` (`with_extra`);
  `with_owned_extra` is for settings made from data. Gone: `fixed`,
  `fixed_switch` and the like, `described`, `for_advanced`, `group` and
  the builders that set an extra at run time. The kernel keeps each list
  given to `add_settings` once, not each spec, and the network settings
  it makes for every plugin share one extra
  (`SettingExtra::section_of_plugin`). On the X4, settings and groups
  kept so make the heap's peak while reading 11.8 KiB lower.
- `SettingGroupSpec`'s builders are `const`, and
  `add_setting_group` takes a `&'static SettingGroupSpec`, kept by
  reference, or one made at run time. The kernel's groups and the
  plugins' with a constant id are statics.
- Services have one path, by their contract: a service type names
  its key with `service_type!` (`ServiceType` is an `unsafe` trait, the
  key the promise that every type naming it is this one), is provided
  with `r.provide(id, value, version)` and found with
  `env.services().get::<T>(id, min_version)`. A trait object is wrapped
  in a struct of the plugin's own (`struct Weather(Box<dyn Forecast>)`).
  `publish`, the lookup by type name, `get_typed` and `get_versioned`
  are gone; a library of one's own is a `library::service::LibraryService`.
- What was deprecated is gone: `Registrar::add_service` and
  `Contribution::Service` (`Registrar::provide` offers a service),
  `Service::any` and `Id::placed` (`Id::unplaced`).

**Env, and what only the kernel uses**

- `Env`'s parts are read through methods (`env.settings()`,
  `env.i18n()`, `env.now()`, one for each part), so what it carries can
  change without a change to plugins. Its fields are private.
- `EnvParts`, `WorkStates::set_works`, `WorkStates::set_note` and
  `ScreenKey::new` come with the `kernel` feature: only the kernel and
  tests make an `Env`, set work states or number screens.
- Cleaned up before the first release (installers): what only the
  kernel and the plugins that install do comes with the `kernel`
  feature, the reading parts staying for every plugin. `native`:
  `remember_author`, `remember_code`, `remember_fetched`, `fetched_for`,
  `keep_code_room`, `installed_code_need`, `check_file`,
  `CROSSING_CRATES`, `LAYOUT` and `CodeRoom`'s constructors (`known_author`,
  `installed_code`, `code_room` and `CodeRoom`'s reading stay);
  `install::local` (`Staging`), `install::marks`, `remember_store_install`,
  `forget_store_install` and `adopt` (`origin`, `Origins` stay);
  `firmware::Update` and `FirmwareCommand::install`, the one way to ask
  for an install (`FirmwareCommand::Install` is matched, not built);
  `data::take_over_old_name`; `Secrets::encode` and `decode`;
  `library::save`, `catalog::write` and `Index::save`.
- `Contribution` is the kernel's: a plugin adds through the
  `Registrar`'s methods, and `Registrar::add`, `contributions` and
  `into_contributions` need the `kernel` feature (a plugin's tests that
  look at what it adds name it as a dev-dependency).

**Names, and types that may grow**

- One name per item of the core API. The hints have each their
  own: `help::HelpHint` (was `help::Hint`), `HintVisibility` (was
  `Hints`, when a screen shows the row of button hints), `ui::look::
  HintRowPart` (was `HintsPart`) and `widgets::CellSpan` (was
  `CellsHint`); `Hint` is the label of one button and `ActionHint` an
  action's. The three modules of sections have their own too:
  `builtin::menu_sections` (was `sections`), `settings_page` (was
  `settings_sections`) and `group_sections` (was `setting_sections`).
  `replies::FolderChosen` is the answer to `ask::folder` (was
  `Chosen`; `Picked` stays the answer to `ask::choice`).
- Types that may grow are `#[non_exhaustive]`, with constructors
  where plugins build them: `decorations::{PageWord, PageText,
  Decorated}::new`, `semantics::Part::new` and `Described::new` (with
  `.focused()`, `.on()`, `.clip()`); also `decorations::Mark`,
  `RowLines`, `SliderParts`, `PickerPart`, `Offer`, `Padding`. A match
  on `Offer` needs an arm for answers to come. `Header` and `Pager` keep
  their state: `area()` reads it, `Header::note_focused()` and
  `note_button()` set the note.
- More types that may grow are `#[non_exhaustive]`: `HttpReply`
  and `FolderChosen` (with `new`), `ServerResponse`, `SocketRead`, `MenuItem`,
  `help::{Input, Zone, HelpHint}` and in `work` `WorkState`, `Retry`, `Why`,
  `Refusal`, `WorkRefused` and `Note` (with `new`), `Step` and
  `Importance`. A match on them needs an arm for what comes later.
  `Handled` is exhaustive: an event is handled or not, and a plugin
  matches both.
- Cleaned up before the first release (platform): the types that
  grow are `#[non_exhaustive]`, with constructors where plugins build
  them: `DirEntry::new`, `files::Entry::new`, `LocalTime::new` (its
  weekday from the date), `MemoryUse::new` and `with_stack`,
  `PluginUsage::new`, `WifiNetwork::new`, `Firmware::new` with `keys`,
  `room`, `undone`, `state` and `plugins`, `BookData::new` and
  `SecretChange::changeable`; `install::Digest::text` writes a digest as
  a job keeps it. A match on `WifiCommand`, `PowerLevel`,
  `served::Access`, `install::Digest`, `FirmwareCommand` or
  `BookDataKind` needs an arm for what is to come. `Usage::refreshes`
  and `PluginUsage::refreshes` name their modes (`usage::Refreshes`:
  `fast`, `partial`, `full`, `total`) in place of an array.

**Data, time and storage**

- Cleaned up before the first release (data): the writers of data
  say why they did not write, `Result<(), data::Error>` in place of a
  `bool`: `annotations::save` and `add`; `annotations::set_note` and
  `remove` give `Ok(false)` where there was nothing to change;
  `library::save`, `Index::save`, `install::marks::write` and
  `catalog::write`. `data::Error` takes a `StorageError` (`?`).
- A storage tells when a file was last changed: `DirEntry::modified`
  (`with_modified`) and `Storage::modified`, `None` where it does not
  tell; the memory card's FAT and the computer's file system say it.
  `MemoryUse` tells the largest block the heap could give now
  (`largest_free`) and the heap's regions (`MemoryRegion`: the X4 Pro's
  internal RAM and PSRAM). `read_file` takes its buffer fallibly: a file
  the heap cannot take now is `StorageError::TooLarge`, not the end.
- `time::UnixTime` is a moment (milliseconds since 1970, UTC):
  `from_secs`, `from_millis`, `secs`, `millis`, `now(env)`, `local(offset)`,
  and the ISO 8601 and HTTP date helpers as its methods (`iso8601`,
  `parse_iso8601`, `parse_http_date`) in place of the free functions
  `time::iso8601`, `time::parse_iso8601` and `web::http_date`, which
  counted in different units. `LocalTime` has an `offset` (seconds ahead
  of UTC; 0 where made without a zone), `LocalTime::from_unix` and
  `LocalTime::unix`, and `time::local_time` fills it from the chosen
  zone. `time::unix_seconds` stays, as `UnixTime::now(env).secs()`.
- `Version` keeps a pre-release (`0.5.0-beta.2`) and orders versions
  as SemVer does (`Version::pre_name`, `pre_number`, `with_pre`,
  `release`, `try_parse`), in 8 bytes: a pre-release is `alpha`, `beta`,
  `dev`, `pre` or `rc` with an optional number up to 8190. Any other
  is refused, saying so: by `Version::try_parse` (`VersionError`), by
  the data plugin loader's warning, by `paperpaw-dev`'s package check
  and, for a code plugin's `package_version!`, by the build. So a plugin
  of a beta loads and a later beta is an update; `satisfies` goes by the
  numbers. `SDK_VERSION` is the crate's version, a pre-release between
  releases (`0.5.0-dev`); `firmware::compare_versions` orders by
  `Version` where it can.
- `Manifest` no longer names the SDK it was built against
  (`Manifest::sdk` and the field are gone), and the kernel no longer
  refuses a plugin for it: a plugin with code is loaded by its file's
  fingerprint and the plugin interface, a data plugin by its `formats`.
  `install::fits_in` asks the kernel's version.

**The interface's building blocks**

- Cleaned up before the first release (UI): `ui::fill` and
  `ui::HOLD_ICON` are gone (`Canvas::fill`, `icons::HOLD`);
  `ui::PAW_ICON` is `icons::PAW`. What only the kernel uses is no longer
  part of the plugin API: `ui::draw_button_hints`,
  `ui::button_hints_height`, `semantics::parts` and the constructors of
  `PageLayers`, `Decorators` and `Widgets` come with the `kernel`
  feature, and `Canvas`'s drawing internals are hidden. A cover drawn
  small is `ui::draw_small_image`; whether the device has a row of
  button hints is `env.caps().button_row`.
- `canvas.describe(role, text, area)` says more by its builder
  (`.focused()`, `.on()`, and `.disabled()` for what does nothing now),
  in place of five arguments; parts (`Described`, `semantics::Part`)
  tell whether they are disabled, and `canvas.disabling(true)` tells
  everything drawn into it so. Faint menu and quick panel entries are
  told as disabled, and the remote control's `describe` (and the web
  emulator's) says `disabled` in a part's state.
- Which page of how many a list shows is a `ui::Paging` (`page`,
  `pages`, `is_first()`, `is_last()`), in place of a pair of numbers:
  `Rows::paging()`, `Sections::paging()`, `ListView::paging()`, and what
  `Header` and `Pager` draw and hit.
- Dates and times as values: `time::Date` and `time::Time`, which
  `ui::DatePicker` and `ui::TimePicker` take and give; `line_at` of a
  page's layers and the widget grid's `cell_at` take a `Point`.
- `UiStyle` keeps its fields and is read with methods: sizes in
  pixels as `u16` (`row()`, `margin()`, `gap()`, `line()`, `corner()`,
  `header()`, `finger()`, `control_row()`), text sizes as `f32`
  (`text()`, `small()`, `title()`, `head()`), and its looks
  (`focus()`, `separators()`, `cards()`, `pill()`, `strong()`,
  `header_kind()` with `HeaderKind::{Light, Dark, Plain}`). How many
  grays the panel shows is `black_and_white()` or `smooth_grays()`. A
  style changed for a screen of its own is made with `with_row`,
  `with_margin`, `with_corner`, `with_pill`, `with_text` and the like.
- One path and one name per item of the UI. The types of the
  modules `ui::slider`, `ui::sections`, `ui::reorder` and `ui::offline`
  are reached through them (`ui::slider::Slider`,
  `ui::offline::OfflineOffer` …). The widgets' look is in `widgets` itself:
  `WidgetLook` (was `widgets::look::Look`, apart from `ui::look::Look`),
  `widget_look()`, `TextSize` (was `Size`; `SizeClass` is a widget area's
  class, `TextRole` the interface's text), `choice_setting()` (was
  `choice`), `WidgetStack` (was `widgets::Stack`, apart from
  `ui::Stack`). `ui::Grid` is `ui::CellGrid`, apart from the widgets'
  `grid`.
- `ui::choice::open` and `GridView::new` are gone (`ask::choice`,
  `GridView::asking`).
- `ui::Ui`, what a screen draws with: the canvas, the environment
  and the style together (`Ui::new(canvas, env, &style)`, read with
  `canvas()`, `env()`, `style()`), the interface's parts its methods
  (`ui.button(..)`, `ui.message(..)`, `ui.focus(..)` …). It borrows what
  it is given and allocates nothing.
- `ui::Label`, one builder for the interface's text:
  `Label::new(text).role(TextRole::Small)` (or `.px(size)`), `.ink(..)`
  or `.color(..)`, `.align(widgets::Align::Center)`, `.min(size)`
  (smaller before shortened), `.lines(n)` or `.wrap()`, `.font(..)` for
  a font of its own; `width`, `height`, `draw(&mut ui, area)` and
  `draw_from(&mut ui, x, area)` (on one line from an `x` between
  pixels). It borrows its text and allocates only the words of a line it
  shortens.
- The interface's parts draw with a `ui::Ui`: `Chips`, `Segments`,
  `Dialog`, `Header`, `Pager`, `TextView`, `DatePicker`, `TimePicker`,
  `offline::OfflineOffer` and `reorder::Reorder` take `&mut Ui` in place
  of canvas, environment and style, and the rows `ListView`, `Rows` and
  `sections::Sections` draw are given the `Ui` (`|ui, index, rect,
  focused|`; its canvas is `ui.canvas()`).
- What `ui::Ui`, `ui::Label` and `ui::Row` draw is drawn only
  through them: `draw_button`, `draw_faint_button`, `draw_icon_button`,
  `draw_focus`, `draw_separator`, `draw_heading`, `draw_switch`,
  `draw_message`, `draw_note_line`, `draw_empty`, `draw_card`,
  `draw_progress`, `draw_row_icon`, `draw_header`, `draw_text_field`,
  `draw_value_row`, `draw_noted_row`, `draw_switch_row` and the rows'
  heights are the SDK's own (`ui.button(..)`, `Row::value(..)` …);
  `draw_number` is gone (`Row::number`). Measuring without a `Ui` stays
  with the functions (`ui::message_height`, `heading_height`,
  `icon_room` …), and the `Ui` lost its copies of them and of
  `focus_shown`. `draw_label` stays for drawing without a style (the
  widgets).
- `ui::Row`, a row of a list as a value: `Row::value(title, v)`,
  `Row::plain(title)`, `Row::switch(title, on)`, `Row::number(title, v,
  room)` (or `Row::of(title, RowKind::..)`), with `.note(..)`,
  `.focused(..)` and for numbers `.step_focus(..)`. The one value gives
  `height(env, style, width)`, `draw(&mut ui, rect)` and `hit(..)` (a
  `RowPart`: the row, its switch, its `−` or `+`), so that they cannot
  disagree. `RowKind` and `RowPart` may grow.
- One input contract for the building blocks. Those that take the
  buttons' actions and gestures (`on_action`, `on_gesture`) answer with
  a `ui::Response`: `Ignored` (the screen's), `Same`, `Changed` (draw it
  again; `request` asks for it) or `Done` with a result. `TextView`,
  `Dialog` (`on_scroll_action` is `on_action`), `DatePicker` and
  `TimePicker` (`on_tap` is `on_gesture`), `NumberEdit` (`Done` with
  whether the value was kept) and `offline::OfflineOffer` (`on_tap` is
  `on_gesture`, `Done` with the `Offer`).
- `ui::focus_shown(env)` says whether a focus is shown: always
  without a touch screen, with one once the buttons were used last. The
  kernel starts with `Env::last_input` at `InputMode::at_start` (`Touch`
  on touch screens). The SDK's parts and the plugins ask it rather than
  the device.
- `ui::Chips` and `ui::Segments` keep their labels: they are given
  once when the part is made (`Segments::new` takes the labels, not
  their count) and `draw` no longer takes them; `label(index)` reads one.
  A label is a translation key (or text that is none), kept as a
  `Cow<'static, str>` without a copy and translated where it is measured
  and drawn.
- A look's painters take a `ui::look::PaintCx` (the canvas and the
  style, more to come) and the part: `fn(&mut PaintCx, &SwitchPart) ->
  bool`.
- The interface's colours come from the palette: `faint(Ink)` and
  `focus_faint(focused, Ink)` take a role (`Ink::Note`, the new
  `Ink::Detail` for what is told beside a row's title, `Ink::Muted` …)
  in place of a gray; the focus and the header use the palette's text
  and paper. `Palette` holds colours (`color(ink)`), `Ink::ALL` is a
  slice, and `Ink::CardPattern` is gone (cards on black and white
  panels are a sparser pattern of `Ink::Card`). The palette setting
  names `detail` too.

**Data plugins**

- Data plugins of formats 3 (not yet released) are written alike in
  every file. A list is a key a line: an action's steps are a `run`
  line each, a line one step with its commas, so a value with a comma
  needs no escape (`entries` and `hide` of `menus.ini` still read ids
  separated by commas). `notify:` takes a key of the plugin's
  translations and says its text in the reader's language. In
  `menus.ini`, `[menu <id>]` is another plugin's menu (`add`, `hide`)
  and any other section a menu of one's own (`title`, `entries`);
  `add` and `hide` in a plain section are still read. `paperpaw-dev
  check` says where a file is written the earlier way
  (`package.actions`, `package.menus`). The ini files take a section
  name of words with single spaces between them.

#### New

- The SDK's design rules ([Designing the plugin SDK](docs/sdk-design.md)):
  how it grows without breaking plugins. Tests check four of them over
  the crates whose types cross: growing types are `#[non_exhaustive]`,
  handlers take no `&mut Vec<Command>`, nothing is `#[doc(hidden)]`
  outside the `kernel` feature, and no deprecated item reaches a
  release. What they found before is listed and may only get less.
- Code plugins are bound to a plugin interface, not to a release
  (ADR 0026): a plugin file runs on every PaperPaw of its interface
  (`native::INTERFACE`, 2, in the fingerprint's top bits), betas and
  releases alike, and authors rebuild only when a release raises it. The
  SDK and `paperpaw-native` have no build scripts, so a plugin built on
  Windows is the same as one built on Linux (`tools/reproducible-plugin.py`,
  in CI on both); a plugin project keeps `paperpaw-plugin.x` beside its
  `.cargo/config.toml`. CI fails where what plugins see changes without
  the interface raised, and by default any change raises it: the public
  API and the shape of the types (fields, variants and trait items in
  order, private ones too, functions' pointers with their ABI) and the
  values of the constants of the SDK and every crate of PaperPaw below
  it, with every feature on (`api.txt` and `shape.txt` of each,
  `tools/interface.py`), and the crates of others whose types cross,
  each pinned to one version. Only a new type, a function outside a
  trait, a constant or an `impl` goes free.
- A code plugin's file names the compiler that built it (format 2 of
  `.ppn`, taken by `paperpaw-sign pack` from `.comment`, which
  `paperpaw-plugin.x` now keeps). The X4s, the store and an update's
  fetch take only files of the firmware's fingerprint and compiler
  (`Native::fits`), the emulator only libraries of its own compiler; an
  update answer may name both (`abi`, `compiler`). Files of format 1 are
  refused as built for an older PaperPaw. The repository's
  `rust-toolchain.toml` pins the release's compiler, 1.99.0. New in the
  SDK: `native::compiler` and `firmware::beta`.
- Native code plugins as files (`code.ppn`): the X4 and X4 Pro load
  them from the card at start. `paperpaw-sign pack` makes and signs
  them, and `samples/hello-native` shows how.
- Data plugins of formats 3 add menus, actions, icons, setup steps and
  fonts (`menus.ini`, `actions.ini`, `icons.ini`, `setup.ini`,
  `points/*.txt`), and mark words with `marks.ini`.
- `ui::NumberEdit` changes a number setting with the buttons as
  the settings pages do (Select, a preview while it moves, Keep or
  Cancel), with its hint bar (`ui::editing_hint`).
- `library::tell_moved` tells the plugins that a file or folder moved
  (its books' progress, notes and statistics go along, settings holding
  the folder follow), as the file manager and Wi-Fi transfer do;
  `library::moved_folder` gives a folder's new path after a move.
- `ui::shown_step`, `ui::shown_number` and `ui::shown_slider_value`
  give the value a number setting steps or slides to: for the size of
  book text the next one the reading font draws differently.
  `ui::next_value` steps so.
- `xml::Tokenizer::try_feed` tokenizes as `feed` does, but where the
  heap has no room for its buffer it returns an error rather than
  stopping the program. The buffer grows by 512 bytes at a time, never
  by doubling.
- `FontHandle::largest_size` says the largest size a font has. For the
  kernel, `Settings::set_range` and `Settings::set_description` narrow a
  setting's range and describe it.
- The SDK grew a lot: reading apps, lookup providers, voices, page
  layers, places for widgets, building blocks for screens, one choice
  and confirm dialog, declared work and power modes, typed events and
  settings, where an installed plugin came from (`install::origin`),
  installs file by file (`install::local::Staging`), serving over the
  USB cable (`served::CABLE_PORT`), typing into a text field
  (`events::TYPE_TEXT`), and book data that is made again apart from the
  reader's own (`add_book_cache`, `add_book_data`). It has breaking
  changes before 1.0; `api.txt` in `crates/paperpaw-sdk` lists its
  public API, and CI checks it.

#### Fixed

- What a plugin tells of the PaperPaw it runs in comes from the kernel
  that runs, not the SDK the plugin was built with:
  `web::user_agent_of(env)` names its version, `install::fits_in(env, ..)`
  takes its SDK (`install::fits_sdk` where no `Env` is at hand), from
  `Plugins::kernel`. The kernel's manifest now has the version of its
  crate. `web::user_agent()` and `install::fits()` stay, for the SDK a
  plugin is built with; PaperPaw's own plugins use the new ones.
- The files of a data plugin are checked by their SHA-256 where the
  plugin library gives it (`sha256` beside `sha1` in `latest.files` and
  in an update's `files`), else by their SHA-1 as before; downloads
  kept on the card with `sha1:` go on (`install::Digest::take`).

#### Devices and tools

- `paperpaw-dev` takes font packs (`.ppfp`) in a data plugin's `fonts/`,
  where the kernel reads them, and names one elsewhere. It no longer
  checks the X4 Classic by default, and `--list-profiles` says it is not
  supported yet. A release no longer lists the X4 Classic's firmware:
  it is built, but not yet tried on one.
- `paperpaw-dev` checks a plugin on the emulated X4, X4 Pro and Boox, in
  every language, light and dark. It finds cut text, uneven margins,
  small touch targets, missing translations and screens that buttons
  cannot reach, and measures heap and frame time. `paperpaw-dev flows`
  plays scripted flows, also at their edges.
- The web emulator runs PaperPaw in a browser as WebAssembly. The
  desktop emulator can be driven over `--stdio`, writes a registry of
  settings, widgets and actions (`--registry`) and checks a plugin
  (`--check`).
- The emulator's release ZIP for each computer
  (`paperpaw-emu-<version>-<target>.zip`) holds `paperpaw-sign` beside
  `paperpaw-emu` and `paperpaw-dev`, for signing plugins without a build
  of one's own.
- The web emulator's release asset (`paperpaw-web-<version>.zip`) is
  signed as the emulators are, and holds everything its pages load. Its
  frame loads no inline style or script, so it runs under a strict
  Content-Security-Policy from any path, as paperpaw.org's "Try it out"
  serves it (docs/web-emulator.md says what a server needs). The frame
  has no background of its own.
- The emulators' device profiles say where each physical key sits on
  the device's body and which computer keys press it (`body` in
  `paperpaw-emu --registry` and in the web emulator's device). The web
  emulator's frame draws the body with the keys there, for the mouse,
  touch and the keyboard, in the embedding page's colours (`look`).
- `tools/usb-transfer.py` and the emulator's `--cable` speak the USB
  transfer's lines (`docs/usb-transfer.md`); `Kernel::type_text` and the
  X4's USB remote type into a keyboard.
- Each release also brings the desktop emulator for Windows, macOS and
  Linux with `paperpaw-dev`, the web emulator and the handbook, each
  signed. `docs/porting.md` says how to add a device.
- The workspace publishes to crates.io as a pre-release
  (`tools/publish-dry-run.sh 0.5.0-beta.1`, and the *Publish check*
  workflow).
- `paperpaw-dev` warns of a settings group without an order or an icon,
  and of a widget's settings in a group of the main page rather than the
  widget's own (`widgets::settings_group`). SDK: `Kernel::menus`.
- `paperpaw-dev` finds an explanation of several lines cut short
  (`visual.explanation-cut`, an error): the SDK's text blocks now say
  what they draw, "…" included, and a text of the language with a "…" of
  its own no longer passes every "…" as one. It warns of a setting's
  description longer than its row (`lint.help-cut`). SDK:
  `ui::note_fits`, `ask::confirm(…).only_yes()`.

## [0.3.0] - 2026-10-16

- Screen lock (off by default): a PIN, a pattern or a password asked for
  when PaperPaw starts and when the device wakes up, set under System ›
  *Screen lock*. The code is kept only as a salted hash. Locked, nothing
  else can be used; after five wrong tries the next one waits half a
  minute. *Lock now* locks at once from any button, gesture or rule.
  It looks like a phone's: the time and the date above, round keys
  below, rings for the digits still to come.
- `Canvas::draw_line`.
- Plugin SDK: input filters (`Registrar::add_input_filter`) see every
  button and finger before screens and key maps do, and may take it or
  run an action instead; refresh filters (`Registrar::add_refresh_filter`)
  change how the panel is refreshed. `InputEvent` and `ButtonState` are
  part of the SDK.

- Plugin SDK: services, what one plugin offers others
  (`Registrar::add_service`, `Env::services`), and points plugins define
  for others to contribute to (`Registrar::contribute`); the event
  `core.library-changed` (`library::changed`), emitted when books come
  into the library or leave it (Wi-Fi transfer, catalogs, Calibre, the
  file manager), and the library shows them at once.
- Rules can have a condition: only between two times of day, only while
  reading or while not, only on battery or while charging, only while a
  switch is on or off (hold a rule, *Only if …*). They can also set a
  number (e.g. the light to 20) and show a message.

- Drag and drop in lists, as on phones: entries of menus (*Customise
  menus*), the widgets of the sleep screen and the items of status lines
  carry a grip (≡) at their end; drag it to move the row, a bar shows
  where it lands. The status line's chosen items now come first, in
  their order. Buttons move rows as before.
- Plugin SDK: `ui::Reorder` (rows moved by dragging a grip, for any
  `ListView`), and screens get a finger before the swipes in from the
  edges do (`Screen::on_pointer`), so grips and handles at the edges
  work.
- The quick panel has its name in *Customise menus*.

- Rules know many more moments: at a time of day on chosen days (every
  day, Monday to Friday, at weekends, one day), every so many minutes,
  when a book is closed or read to its end, when the device goes to
  sleep, when the battery falls below a percentage, when charging begins
  or ends, and when a switch is turned on or off.
- Plugin SDK: events `core.book-closed` and `core.book-finished` (the
  reader emits them), and `Screen::on_close`, called when a screen is
  closed.

- Gestures as on phones: up from the bottom edge goes home, down from the
  top edge opens the quick panel, in from a side goes back (while reading
  it turns the page). The quick panel holds night mode, the light,
  turning the screen, refreshing the screen, a screenshot, sleep and the
  settings, with the time, date and battery above (what shows there and
  how large is set under Appearance); plugins and the menu editor change
  what it holds. *Refresh the screen* (`core.refresh-screen`) clears
  e-ink ghosting with a full refresh, from the quick panel or any button
  or gesture it is bound to. Swipes in from the edges come before the screen sees them.
- More gestures as on phones: up on the home screen shows every tool at
  once (*All tools*, `home.all-tools`), down on it searches the library,
  and two fingers apart or together make the text larger or smaller
  while reading (`pinch.out`, `pinch.in`, in a section *Two fingers* of
  *Buttons and gestures*). The first home screen on a touch screen
  explains the gestures once (*Hint on the gestures* under Appearance
  shows it again).
- Gestures per tool: as standard, the tool's own (gestures go to it, a
  swipe in from an edge needs a second one: for games and drawing) or
  off. A tool sets its default (`gestures.policy` in its key map); the
  user changes it in *Buttons and gestures*.
- *Buttons and gestures* is easier to find one's way in: where a change
  applies (everywhere, home screen, reading, library, any tool), inputs
  in sections (edges, swipes, taps with a picture of the zone, held
  taps, buttons, held buttons), and the actions to choose from in groups
  with a search, *As standard* and *Nothing* first.

- The home screen is a grid, as on a phone: 4 columns by default (rows
  to fit the screen; both settings), each widget taking a rectangle of
  cells, each tool an icon in a cell of its own; empty cells stay empty.
  A long press or the pencil shows the grid: drag an item to move it, by
  its corner to size it, tap it for its options (move or size it with
  the buttons, its settings, remove it); *Add* adds widgets, tools and
  pages. The status line sits above the grid. Pages written before are
  placed in the grid by themselves.
- Widgets have more settings of their own: the clock's size (also as
  large as its cells allow), the date's size, whether the continue card
  shows its cover, its progress and a frame, and for the recently read
  books their heading, their titles, where the titles sit and how many
  books show. Tools with or without their names.

## [0.2.0] - 2026-10-16

- Books from a catalog (OPDS) or from Calibre keep the title, author,
  series and tags the source gives, where the book names none itself
  (text files, PDFs): `library::provide`.
- *Say what is chosen* (System, where the device speaks): says the row
  or button the buttons move to, with a switch's state, for using
  PaperPaw without looking at the screen.
- Screens say what they show: rows with their text and
  focus, buttons, switches and headings, told by the drawing helpers as
  they draw (`Canvas::describe`, `semantics::parts`,
  `Kernel::describe`). The emulator's scripts tap by name
  (`tap-on:Display`) and print a screen's parts (`describe`).
- The library is a service (`library` in the plugin SDK): what is known
  of books (title, author, series, tags, cover, progress, and fields
  plugins add), the books read last and the favourites, kept by the
  kernel from the reading events. The library, the home screen and the
  sleep screen show the same data instead of keeping copies of their
  own; what the library kept before is taken over.
- Bookmarks and highlights shared with KOReader (`koreader` plugin), for
  books read in both on one device or in one synced library folder:
  what KOReader has in `Book.sdr` comes over when a book is opened, and
  what is new here goes to KOReader the next time it opens the book;
  removing one, or a later note, goes across too. EPUB only.
- EPUB positions as KOReader writes them, down to the character: reading
  positions synced with KOReader land on the word, not the paragraph.
- Plugin SDK: bookmarks and highlights are kept by `annotations`, for
  every plugin, with when they were made and changed; event
  `core.annotations-changed`. `Document::xpointer` and
  `Document::locate_xpointer` are exact to the character.
- Colour pictures on colour screens (ADR 0015): covers, pictures in
  books, comics, OPDS covers and the sleep screen's pictures are shown
  in colour on phones and tablets and on colour e-ink (Boox models named
  "Color" or "C", with colours strengthened for the filter); text and
  the interface stay gray, monochrome e-ink is unchanged. JPEG (also
  progressive), PNG and GIF are decoded in colour where asked. Plugin
  SDK: `ImageSink::wants_color` and `row_rgb`, `ColorFile`,
  `ColorWriter`, `ImageFile`, `image::picture_from_file`,
  `OpenOptions::color`, `Formats::options`, `Canvas::draw_rgb_row`,
  `Framebuffer::set_saturation`, `ColorPanel::boost_percent`.
- Smaller text by default: menus 9 pt (was 11), status lines 6 pt
  (was 8). Both stay adjustable in the settings.
- Dictionary: the page of a long entry no longer covers *Online*.
- Text and paper colours on Android (Appearance): eight of each, or
  one's own colour code (e.g. #5b4636); night mode swaps them.
- Room between widgets: *Space*, a gap (small, medium or large), and
  *Free space*, which takes the room the others leave, e.g. to put the
  clock in the middle or the tools at the bottom. Both may be added more
  than once, on the home screen and the sleep screen.
- Appearance: *Corners* (square, slightly rounded, round) for
  highlighted rows, buttons and boxes, and *Contrast*: strong draws gray
  hints and notes in black, for screens with few grays. The Airy theme
  sample rounds its corners. Plugin SDK: `UiStyle` gains `corner` and
  `strong` (with `UiStyle::faint` for gray text), `ui::fill_round`,
  `ui::stroke_round`, and the canvas rounded rectangles.
- GIF pictures in books, comics and the sleep screen's own pictures: the
  first picture of the file (also of an animation, interlaced or with
  transparency), decoded to gray row by row like JPEG and PNG.
- Plugins can bring a stylesheet for books: a data plugin's
  `styles.css`, or `Registrar::add_stylesheet`, applied after a book's
  own and before one's own (EPUB). SDK: `Env::stylesheets`,
  `OpenOptions::css` (no longer `Copy`).
- Books' embedded fonts in WOFF are used too (WOFF 2 is not read).
- Calibre's wireless device connection (`calibre` plugin, in the tools):
  the device finds Calibre in the network (or connects to the address
  set), and Calibre sends books to the library, lists and deletes them
  as with a reader on a cable; a password set in Calibre is asked for
  once. SDK: `sha1`, and `json::raw_member`, `json::unquote`,
  `json::string_array`.
- Sockets for plugins (ADR 0014): TCP connections that read text or
  files of any size into storage, and UDP broadcasts to find services,
  in the emulator and on Android (`Command::Connect`, `SocketSend`,
  `SocketRead`, `Disconnect`, `Broadcast`).
- Small capitals (CSS `font-variant: small-caps`, `font-variant-caps`)
  in EPUB, HTML and the other converted formats: lower case letters as
  smaller capitals, the text itself unchanged (search, selection and
  notes as before). Books are prepared anew once. Document model:
  `TextStyle::SMALL_CAPS`; embedded font families at most 31 (was 63).
- Snapshot tests: screens of the X4 and the Boox are drawn headless and
  compared with reference pictures, so layout faults fail a test.
- The folder chooser says when it is still opening a folder (slow SD
  cards) and ignores taps until it shows it.
- Widgets have settings of their own, from the home screen's editor:
  where the status line, date, clock, today's reading and the word of
  the day sit (left, centre, right), and whether the clock shows the
  date. Plugins give their widgets settings the same way
  (`widgets::settings_group`, `widgets::align_setting`).

## [0.1.1] - 2026-10-15

The Android app for F-Droid.

- The app is built reproducibly: with the same tools (shells/android/
  versions.env) it comes out byte for byte the same anywhere, so F-Droid
  can publish it signed with PaperPaw's key. A dependency's macro that
  wrote its code in a random order is patched (patches/jni-macros).
- The app's library is stored in the APK and runs from there, without
  symbols: 6.8 MB to download, less space taken on the device.
- The licence names the copyright holder; README with screenshots.

## [0.1.0] - 2026-10-15

The first release: PaperPaw on Android e-readers (tested on a Boox Go 7)
and in the desktop emulator, with its plugin SDK. The firmware for the
Xteink X4 is built but has not run on a device yet.

### Reading

- EPUB 2 and 3 (with the book's styles, embedded fonts and pictures, or
  one's own stylesheets per folder or book), FB2, Markdown, plain text,
  comic archives (CBZ), Xteink's XTC and XTCH, and PDF on Android.
- Contents, search, bookmarks and highlights with notes, export of notes
  as Markdown, a dictionary (StarDict), Wikipedia and translation
  (DeepL), a vocabulary list of words looked up.
- Hyphenation (German, English), reading fonts with weight and size,
  margins, spacing, status lines to taste, reading aloud, pages turned
  by themselves, turning the screen.
- Reading statistics, shared with KOReader (also over WebDAV), and
  reading positions synced with KOReader's sync server.

### Books and more

- A home screen of widgets on pages (date, battery, continue reading,
  recently read, tools, statistics, headlines), editable by touch or
  buttons.
- The library by folders, authors, series and tags; a file manager;
  books from OPDS catalogs and WebDAV folders (Nextcloud), kept in sync.
- News feeds as books; Wallabag's read-later articles, highlights to
  Readwise and a browser, switched off until wanted; transfers from a
  computer or phone over Wi-Fi, typing on a phone.
- A sleep screen (cover, picture or widgets), screenshots.

### Making it one's own

- Every screen, menu, key and gesture can be changed, in the app or in
  `paperpaw.ini`; profiles, rules, themes, translations and fonts as
  plugins without code (data plugins).
- Settings with descriptions and a search, passwords and keys in one
  place, short menus by default, usable by touch and by buttons alike.
- English and German.

### For developers

- The plugin SDK (`paperpaw-sdk`) with a guide ([Writing
  plugins](docs/plugins.md)) and a sample plugin; plugins are tested
  against the real kernel.
- The desktop emulator with device profiles and memory budgets.
- The Xteink X4's firmware, its display, buttons, battery and SD card
  tested against simulated hardware ([ADR 0013](docs/adr/0013-x4-firmware.md)).

[0.3.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.3.0
[0.2.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.2.0
[0.1.1]: https://github.com/hthoene/paperpaw/releases/tag/v0.1.1
[0.1.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.1.0
