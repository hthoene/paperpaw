# Interface-7 port

Version 1.0.7 (above Beta 3's, so no bump). Status: **complete except one subscription**: native
only, no interface-6 code left (`Readwise`, `Parser`, `send.rs` and the kernel-driven tests are
gone; their behaviours live in `native_send_tests.rs` and `testdata/parser.v1`). The one thing
missing is that the plugin does not yet *hear* the export (see "Open").

## What it is

- `declarations.rs`: the subscriptions (the start, the end of a request, the work that is due);
  no screen, setting or action. Built into the image by `build.rs`.
- `src/native_readwise.rs`: the Markdown reader (`Parser`) and the JSON of a highlight; fallible,
  swept. `src/native_golden_tests.rs` compares both with `testdata/parser.v1`, a transcript
  written while the interface-6 parser was still there and equal to it on every line (notes of
  every shape, CRLF, no author, long and multi-byte texts cut at the limits, odd percents, the
  JSON escapes). `PAPERPAW_BLESS=1 cargo test` writes it anew; a change is a change of what
  Readwise is sent, so it is reviewed.
- `src/native_send.rs`: the sending over a `Wire` (the data folder and the card as `Volume`s, the
  network, the token): `waiting.txt` (a path per line, rewritten through a copy), the body of a
  request written a highlight at a time into `body.json` in writes of at most 4 KiB and sent from
  there (`send_file`), the answer by status (2xx takes the file off the list, 401/403 lets the
  token go, anything else counts the file failed and goes on, no answer ends the sending),
  what to tell (`Step`, `Tell`). `src/native_send_tests.rs` has the cases of interface 6's
  `send.rs` and `sending.rs`, a memory sweep, a request refused, cancelled or ended by the end of
  the work run.
- `src/native_plugin.rs`, `src/native_entry.rs`: the host's side. The token is the plugin's own
  secret `token`, labelled `readwise.token` (interface 6 kept it as `readwise.token`; the secret
  store moves it to the plugin's own name, `secrets.md` §7); it is asked for as a masked
  question on the keyboard (`core.ui`), kept with `core.secrets`, and sent with at once. Every
  buffer that held it is wiped. The retry is declared work: `readwise.retry`, network, at the
  moment the network is there, within 30 minutes (`work.md`: "joplin, readwise").

## Decisions

- **No cap by memory class.** A highlight is bounded by Readwise's own limits (text and note 8,191
  bytes each, title 511, author 1,024) and by the fallible `Parser`; the notes are read 512 bytes
  at a time (lines cut at 4,096 as in interface 6) and the body is written a highlight at a time,
  so the peak is one highlight and its JSON (about 2 × 16 KiB, with room for the escapes of a usual
  text; a text of nothing but control characters, six times longer escaped, ends in `NoMemory`: that
  notes file counts as failed and waits). That fits the X4's heap, so no `core.device` read was
  added (news needs one for the text of an article, 8 KiB small / 64 KiB otherwise).
- **Readwise's answer is not kept.** Interface 6 saved it to a file unread; the request now asks
  for at most 256 bytes of it in memory (`max_body`), cut and ignored, so a long list of what
  Readwise took costs no file and no `too_large` end.
- The notes are on the card (root 1), so the plugin declares `uses.storage`; the data folder
  (root 0) holds the list and the body.
- A sending the work run began ends when the run ends (`Reason::WorkEnded`), quietly, and the
  files keep waiting. A sending that runs by itself tells only what went well, a refused token
  only when the reader started it, as in interface 6.
- A heap failure ends the sending with the notice *not enough memory* (new text, `readwise.no-memory`,
  en and de); the files wait.

## Open

- **The export.** The reader runs `core.notes-exported` as an event in interface 6. In interface 7
  it is a stage the reader defines, `reader.notes-exported@1` (input `root` u32, `path` str 512,
  `title` str 256; Kernel Engineer, #228). It is not there yet, so the plugin has no subscription
  for it: `NativeReadwise::exported(host, path)` does the rest (puts the file on the list, sends,
  tells, asks for the token) and is what the callback of the subscription will call. Add
  `SubscribeDecl::new("reader.notes-exported", EXPORT_TOKEN)` to `declarations.rs` and the stage's
  codec to `event` once the reader's `.def` lands; the input's `root` must be 1 (card) as the
  notes are read from it.
- Not measured on a device: the heap peak while sending, and the `.ppn` size against Beta 3
  (the flash gate counts the file once `complete.txt` lists it, which it does).
- The notice texts for the other 20 languages (`tools/plugin-files/languages/*.ini`) are not
  touched: `readwise.no-memory` falls back to English there.
