# Plugin store on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (the library's list and plugin
pages, the index kept on the card, the downloads, the install with its checks
and keys, updates, requests from other plugins, the first-run offer, the
screens) is still in place beside the native one. It is removed in the PR that
finishes the port; until then nothing is dropped.

## Ported in slice 1 (`declarations.rs`, `src/native_model.rs`, `src/native_plugin.rs`)

- Declarations: the two settings, the library's address (developer group) and
  *Community plugins* (system group, guarded). `build.rs` and `native/metadata*`
  are generated with `tools/abi-metadata.py`. En/de texts are the existing ones.
- The rules (`native_model.rs`, host-free): the interface-6 code made fallible,
  tested to give the same answers (`src/native_parity_tests.rs`): the groups and
  sections and the limits read from the library; the line the index keeps of a
  plugin (written and read back, tabs and line ends of the list made spaces); what
  a build needs of the firmware and whether it fits (`Needs`, `Provides`, `Fit`);
  the address of the list (`list_url`, `encode`); hexadecimal keys and signatures
  read without allocating (`hex_array`, the form that `decode_array` takes in #507,
  to which the old calls move); the revocations a library names, kept only if
  they could hold (of a plugin here, naming its author's key, not stopped yet),
  with a bound on the entries read (4,096) and on those kept (256).
  `tests/native_alloc_refusal.rs` refuses each allocation of these. A library is
  untrusted (community plugins too): every text is cut or refused at a limit (a line, name or summary past its cap is not read; runs and sections stop growing) named in the
  file, and nothing indexes text or bytes in a way that can panic.
- Declarations the host refuses for now are left out of the batch (none here).

## Pending host (come back before activation, keeping parity)

- **The install** of plugins, data and code (the check of a code plugin whole, the
  author's key remembered at the first install and followed through signed
  rotations, the digests, the room for code, the revocations handed to the kernel,
  updates waiting for a restart). **Kernel todo 145** (the install design for
  native plugins) is open; nothing of it is invented here. Owner decisions that
  apply: a plugin-triggered install or update needs the host's confirmation (132),
  and a change of the firmware always confirms (134).
- Fetching the list and the plugins' pages, the downloads, the index and its
  sorting on the card: `core.net` and the files facade (#384).
- The screens (list, plugin page, search, shots), the actions, the menu entry and
  icon, the queue and the notices, "how it works": H1/H3/H4.
- Requests from other plugins (`Command::request_plugins`) and the first-run offer
  (`core.registry` op 22, Host todo 99).
- **No downgrade**: the install must refuse a version older than the installed
  one (the interface-6 rule); it belongs with the install, so it comes with Kernel
  todo 145's design.
- The reading of the library's JSON into `Entry` values with the fallible reader
  (`paperpaw_sdk::json::stream::try_feed`, #414) is not in this slice: it goes with
  the fetch, on top of the rules here. Its bound is the largest file taken
  (the list 4 MiB, a plugin's page 512 KiB, a picture 2 MiB: the interface-6
  `MAX_LIST_FILE`, `MAX_PAGE_FILE`, `MAX_SHOT_FILE`; enforced by the fetch, which
  reads the files).

Each is an ignored case in `tests/acceptance_pending.rs`. The interface-6 tests
(`tests/*.rs`) stay as the references.
