# Translation on interface 7 (slices 1 and 2, partly ported)

Status: **not complete.** The interface-6 plugin (`Translate`, the panel, the request and the
answer in `lib.rs`) is still in place beside the native one. It is removed in the PR that finishes
the port.

Version 1.0.6. Native plugin: `src/native_plugin.rs` (`NativeTranslate`), declarations in
`declarations.rs` built into the image by `build.rs`, the host-free parts in
`src/native_model.rs`, asking DeepL and the key in `src/native_ask.rs`, metadata in `native/` (regenerate with
`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).

## The plan (slices)

1. **Slice 1 (done):** the panel's screen declared and submitted, the model (target language,
   request, answer) fallible and compared with interface 6, the tests, the pending acceptance.
2. **Slice 2 (this one):** the request and the key (see below). Asking DeepL and keeping the key are
   written and run against scripted `core.net` and `core.secrets` services.
3. **Slice 3:** the panel on `core.ui`: the original in one line in the header, the translation
   wrapped and scrolled by the page buttons or taps, *Select* or a tap at the end closes it, a tap
   asks for the key while there is none, the offer to go online in offline mode, the messages
   (`translate.loading`, `needs-key`, `bad-key`, `quota`, `no-network`, `unreachable`, `failed`).
   Then the interface-6 code goes, `PORTING-7.md` says *complete*, the language setting and the
   slot offer are in the image.

## Ported in slice 1

- The screen `translate.panel` (an overlay: it is drawn over the lower part of the page below it,
  as the interface-6 screen drew in a `region`), with the title `translate.title`; the en/de texts
  are the existing ones.
- `src/native_model.rs`, each part the interface-6 code made fallible and tested to give the same
  answers (`src/native_parity_tests.rs`): the DeepL target language for a setting value and PaperPaw's
  language (`target`), the service for a key (`host`, `url`: free keys, ending in `:fx`, go to
  `api-free.deepl.com`), the form body (`body`: the first 3,000 characters, `+` for spaces, percent
  encoding), the line that carries the key (`authorization`, a `Secret` that wipes itself and
  shows nothing in `Debug`), and the translation read from an answer (`translation`, `outcome`):
  200 to 299 with a `text` is the translation (no `text`: failed), 401 and 403 the key was not
  accepted, 456 the quota is used up, status 0 the network did not answer, anything else failed.
- Differences from interface 6, on purpose: a refused allocation is `NoMemory`, never an abort; the
  answer is read up to 24 KiB (`MAX_BODY`: 3,000 characters of any script with DeepL's members), a
  longer one is refused with `NoSpace`; a language code of a setting over 16 bytes is none (the
  reader's own language is asked for) and capitals are ASCII ones.
- `tests/native_alloc_refusal.rs` refuses each allocation of the model in turn;
  `any_body_is_read_without_a_panic` feeds random text to the reader.
- Declarations are compared with the interface-6 registration by the shared helper
  (`tests/native_declarations.rs`): the screen and its title, and the slot offer.

## Ported in slice 2

- `src/native_ask.rs` (`Asker`): **`core.net`** (`native::net`) and **`core.secrets`**
  (`native::secrets`), both opened at the first call that needs them (never in `make`: a service
  opened there has no live frame). Metadata: services `core.net`, `core.secrets`, feature
  `uses.network` (own secrets need none), host calls `services.query` and `storage.*` (the form
  file).
- `ask`: `POST` to `api.deepl.com` or, for a key ending in `:fx`, `api-free.deepl.com`, with the
  three headers of interface 6 (`Authorization: DeepL-Auth-Key <key>`, `Content-Type`,
  `User-Agent: PaperPaw/0.1 (e-reader)`) and `max_body` 24 KiB. A form of up to 2,048 bytes goes
  inline; a longer one (3,000 characters keep their full length) is written to `request.form` in the
  data folder and sent from there (`network.md` 2.1: bodies over 2 KiB are written to root 0
  first), and removed when the request ends or is cancelled (a leftover of a cut-off run goes with
  the first one). A newer `ask` cancels the one in flight (its end is told apart by the request id
  and ignored).
- `done` (the `http_done` event): reads the answer and, for a 2xx, the body in pieces of up to
  3,840 bytes into a buffer reserved once (at most `MAX_BODY`), and gives the same `Outcome` as
  interface 6. A request that ended without an answer (any reason but `ok`) is `Unreachable`, as
  status 0 was; **a cancelled or replaced one says nothing.** (The offline power mode comes as
  reason `offline`; slice 3 may offer to go online for it instead.)
- `key` and `keep_key`: the key is the plugin's own secret `deepl-key` (`secrets.md` 7), labelled
  `translate.key`, changeable in the list of passwords and keys. Interface 6 kept it as
  `translate.deepl-key`; the secret store moves it to the plugin's own name at the first start
  (as for Wi-Fi, decision 138), not the plugin. What is typed is trimmed, an empty text changes
  nothing (`Kept::Empty`), an empty kept key is no key. **`ERR_UNSUPPORTED` on keep is
  `Kept::Unsupported`**: the key is used for this time only and slice 3 shows the shared text
  `core.secrets-unsupported` (PR #557, owner decision 141, now on integration; nothing shows it
  until the panel exists). Every buffer that held the key (the secrets input and reply,
  the request input, the `Authorization` line) is wiped; nothing is logged.
- `tests/native_asker.rs` drives `Asker` from events with a probe plugin over scripted services:
  the request exactly as given to the host, the translation read from a body told in pieces,
  every status, a replaced and a cancelled request, the key kept trimmed, read back, empty and
  refused as unsupported, and a refusal of every service call and every opening in turn (no
  abort, an error status). `tests/native_asking.rs` drives the real plugin: an end that nothing
  waits for changes nothing and asks no service; a body that is no record is an error status.

## What the slice loses (each returns in a later slice or with its host piece)

- **The language setting `translate.to`** is built (`declarations::settings`: a choice of the reader's
  own language and the 32 DeepL ones, default PaperPaw's, advanced, in the reading group, under
  the section `translate.section`) but **not in the image**: a `setting.spec` takes at most 32 choice
  values and this one has 33. Kernel raises the limit to 64 in draft 29 (to be blessed on
  2026-11-09); a follow-up emits it, submits it and compares it with the interface-6 setting.
  `tests/native_declarations.rs` pins the refusal and fails when the limit is raised.
- **The slot offer** for `core.translate` is built and in the image but **not submitted**: the host
  checks a slot offer and refuses it (`ERR_UNSUPPORTED`) until it admits registry declarations, and
  a `make` that fails would keep the whole plugin from being admitted. The test pins the refusal.
  Until then the reader's *Translate* does not find the panel.
- **The subscription to `http_done`** (`declarations::events`, `image::EVENTS`) is built but **not
  submitted**: the kernel's host refuses event declarations until #944, and a `make` that fails
  keeps the plugin out. Without it `core.net` refuses a request as `State`, so the real plugin
  does not ask DeepL yet (nothing starts a request until the panel in slice 3); `native_asking`
  pins that it is not submitted.
- **Not run against the real hosts:** the staging of a form over 2,048 bytes (the scripted host has
  no storage: the test pins that the request is then refused whole, never sent cut), and
  allocation refusal inside `Asker` (the linked plugin allocates from the test's allocator, not
  the host's heap slot); every allocation there is fallible by construction.
- The panel's drawing, scrolling and input, the offer to go online and the messages: slice 3. Each
  is an ignored case in `tests/acceptance_pending.rs`, naming what blocks it.
- `tests/panel.rs` (the interface-6 test, driving a kernel) stays as the reference.
