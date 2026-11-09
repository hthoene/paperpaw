# Translation on interface 7 (slices 1 to 3, partly ported)

Status: **not complete.** The interface-6 plugin (`Translate`, the panel, the request and the
answer in `lib.rs`) is still in place beside the native one: it is what registers the language
setting and the slot `core.translate` until the native plugin can (draft 29 and #522, see "What is
still missing"). Everything else of the plugin is ported; the interface-6 code goes in the PR that
registers those two, and `PORTING-7.md` then says *complete*.

Version 1.0.6. Native plugin: `src/native_plugin.rs` (`NativeTranslate`), declarations in
`declarations.rs` built into the image by `build.rs`, the host-free parts in
`src/native_model.rs`, asking DeepL and the key in `src/native_ask.rs`, the panel in `src/native_panel.rs`, metadata in `native/` (regenerate with
`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).

## The plan (slices)

1. **Slice 1 (done):** the panel's screen declared and submitted, the model (target language,
   request, answer) fallible and compared with interface 6, the tests, the pending acceptance.
2. **Slice 2 (done):** the request and the key (see below). Asking DeepL and keeping the key are
   written and run against scripted `core.net` and `core.secrets` services.
3. **Slice 3 (this one):** the panel on `core.ui` (see below). What is left (the language setting,
   the reader's side of the slot offer) waits for the hosts and the contract, then the interface-6 code goes.

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
  first), and removed (with the file a cut write staged) when the request ends, is cancelled, or any step after the form was marked fails (a leftover of a cut-off run goes with
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

## Ported in slice 3

- `src/native_panel.rs` (`Panel`): the overlay over the lower 11 twentieths of the page (as
  interface 6 drew in a `region`): a bar along its top, the original in one line at the left of the
  header (the first 512 bytes, whitespace made single, fitted to the room the service's name
  leaves) and `translate.source` at the right, then the body: a message, the translation or the
  offer to go online. Drawn from prepared `Paint` operations in the draw callback (font and
  `Style` of `paperpaw-ui-kit`). Wrapping is a paragraph at a time with fallible allocation, at most
  400 lines (the last ends in "…"; 3,000 characters of any script come to fewer): **a difference
  from interface 6, which had no limit**.
- Keys and taps as in interface 6 (`Panel::action`, `Panel::tap`, tested without a host): the page
  buttons and arrows scroll, *Select* closes (or asks for the key while there is none or it was
  refused), a tap in the left third goes back, in the rest forward, and forward at the end closes;
  a tap while a key is needed asks for it. The seven messages are the existing texts
  (`translate.loading`, `needs-key`, `bad-key`, `quota`, `no-network`, `unreachable`, `failed`).
  The interface-6 *hint* (the button hint "Close" under *Select*) is not ported: the native input
  has no hint call yet.
- `src/native_plugin.rs`: the selected text is the hand-over's host text (`ScreenOpened` names the
  hand-over; read whole with `uitext::read_all`, at most 24,576 bytes: nothing, or text that is too
  long, is an empty text). The request is sent when the panel is first shown (interface 6:
  `on_show`), with the key read from the secrets; no network at all (`core.net` or the feature
  refused) says `no-network`, no key (or no secrets) `needs-key`. `http_done` shows the
  translation or the message of the outcome (`Asker::done`).
- **The key**: *Select* or a tap while it is needed asks for it on the keyboard slot as a **masked**
  text question (`Question::text(...).secret()`, titled `translate.key`); the answer (read whole in
  buffers that are wiped, trimmed, over 1,024 bytes is `bad-key`) is kept with the secrets and
  used **at once**, as in interface 6. Where the host answers `ERR_UNSUPPORTED` to keeping it (the
  device cannot save passwords, owner decision 141) the reader gets the shared notice
  **`core.secrets-unsupported`** and the key is used for this time only; any other failure to keep
  it is silent and the key is used all the same.
- **Offline power mode**: a request that ends with reason `offline` (`Outcome::Offline`) shows the
  offer once per panel (*Offline mode. This needs the network. Go online for it?* with the shared
  `core.offline.*` texts: two buttons, *Go online* first, keys move the focus, a tap chooses).
  *Go online* asks the host for the network for 10 minutes (`go_online`, the most the ABI allows;
  interface 6 held it until the screen closed, so the reader presses *Translate* again after the 10
  minutes) and sends the text again; *Not now* says `no-network`. A second
  `offline` on the same panel says `no-network` (not offered again).
- A refused redraw, close or `go_online` is not an error of the event or input that asked (the
  panel may not be on top any more); a failed ask or request shows `translate.failed` or
  `no-network`, never aborts. Closing the screen cancels the request and frees the panel.
- Metadata: services `core.net`, `core.secrets`, `core.ui` (plus the host calls behind them:
  `services.query`, `blob.read`, `blob.release`).
- `tests/native_panel_flow.rs` drives the real plugin through scripted `core.ui`, `core.net` and
  `core.secrets` services: the text read from the hand-over, nothing sent before the panel is
  shown and once only, the request as given to the host, a body read in pieces, no key (nothing
  sent, a masked question asked, the answer kept and used, an answer to another question or a
  cancelled one ignored), a host that cannot keep the key, the offer once and *Go online* sending
  again, *Not now*, a device without the network service, and a refusal of every service call and
  opening in turn (no abort). **The scripted host stages no screen commands**, so a redraw, a close
  and `go_online` are not seen in it (the panel's tests cover what each key means), and it
  answers no `i18n.copy`: the title of the key's question then falls back to its key.
  `src/native_panel/tests.rs` tests the panel's state and wrapping without a host;
  `tests/native_translations.rs` checks every text the code asks for, the plugin's and the shared
  ones (`core.offline.*`, `core.secrets-unsupported`), in both languages.
- The subscription to `http_done` (`image::EVENTS`) is submitted with the screen, as Battery and
  Wi-Fi submit theirs: the scripted host admits it; the kernel's own host (`Hosted`, as
  `plugins/help/tests/hosted_construction.rs` shows) refuses every event batch (and a screen
  without its views) until its declaration wiring (#944) lands, for every port alike.

## What is still missing (each returns with its host piece)

- **The language setting `translate.to`** is built (`declarations::settings`: a choice of the reader's
  own language and the 32 DeepL ones, default PaperPaw's, advanced, in the reading group, under
  the section `translate.section`) but **not in the image**: a `setting.spec` takes at most 32 choice
  values and this one has 33. Draft 29 (Kernel, to be blessed 2026-11-09, not on integration
  yet) raises the limit to 64; a follow-up emits it, submits it and compares it with the
  interface-6 setting. Until then the plugin reads `translate.to` if some plugin has registered it
  (interface 6 does, while it stays) and translates into PaperPaw's language otherwise.
  `tests/native_declarations.rs` pins the refusal and fails when the limit is raised.
- **The slot offer** for `core.translate` is in the image and **submitted** in `make`, after the screen it
  names (the host admits slot offers since #522; `tests/native_declarations.rs` validates it,
  `tests/native_asking.rs` pins the order). What is still open is the reader's side: its *Translate*
  finds the native panel only when the reader's slot host reads native offers (an ignored case in
  `tests/acceptance_pending.rs`); the interface-6 plugin still binds the slot until it goes.
- **Not run against the real hosts:** the panel's drawing (it needs the font and the style: a
  pixel harness backed by the kernel, as Battery's), the staging of a form over 2,048 bytes (the
  scripted host has no storage), allocation refusal inside the linked plugin (it allocates from the
  test's allocator, not the host's heap slot; every allocation is fallible by construction).
  Each is an ignored case in `tests/acceptance_pending.rs`.
- `tests/panel.rs` (the interface-6 test, driving a kernel) stays as the reference.
