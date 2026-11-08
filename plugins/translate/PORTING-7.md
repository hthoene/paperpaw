# Translation on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (`Translate`, the panel, the request and the
answer in `lib.rs`) is still in place beside the native one. It is removed in the PR that finishes
the port.

Version 1.0.6. Native plugin: `src/native_plugin.rs` (`NativeTranslate`), declarations in
`declarations.rs` built into the image by `build.rs`, the host-free parts in
`src/native_model.rs`, metadata in `native/` (regenerate with
`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).

## The plan (slices)

1. **Slice 1 (this one):** the panel's screen declared and submitted, the model (target language,
   request, answer) fallible and compared with interface 6, the tests, the pending acceptance.
2. **Slice 2:** the request and the key. `Net` (typed `core.net`, `native::net`) sends the form
   body to DeepL with the `Authorization` line from `native_model::authorization` (in a buffer
   that wipes itself); the key is read from and kept with `native::secrets` (#367), asked for on
   the keyboard slot as a password. Answers are read through `native_model::outcome`. Needs the
   host's net and secrets services in a test host (`#491` scripted host support).
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
- Asking DeepL, the key, the panel's drawing, scrolling and input, the offer to go online and the
  messages: slices 2 and 3. Each is an ignored case in `tests/acceptance_pending.rs`, naming
  what blocks it.
- `tests/panel.rs` (the interface-6 test, driving a kernel) stays as the reference.
