# Mail on interface 7 (slices 1 and 2, partly ported)

Status: **not complete.** The interface-6 plugin (`plugin.rs`, the sessions, the screens, the
store and the widgets) is still in place beside the native one. It is removed in the PR that
finishes the port. Nothing of Mail runs on the native host yet except the declarations.

## Ported in slice 1

- `declarations.rs`, `build.rs`, `native/metadata*`, `src/native_plugin.rs`: the group `mail.group`
  (interface 6: `mail`; every ported plugin names its group so), the pages `mail.account.1` to
  `3`, the 13 settings of each account and the three for all (42), all with their en/de texts
  unchanged. The password of an account is declared as a **secret**. The settings, the groups,
  the actions *open* and *write*, the Tools entry (order 47), the icons and the four screens are
  compared field by field with what the interface-6 plugin registers (`tests/parity.rs`); the
  settings are also a golden (`testdata/declarations.bin.v1`).
- The actions, the entry, the icons and the screens are built by `declarations::ui` and
  `declarations::screens` and are **not in the image**: they would open nothing.
  `metadata*.json` lists no feature yet (`uses.network`, `uses.secrets` and `uses.storage` join with
  the slices that make the calls).

- `src/native_model/` (host-free, the interface-6 code made fallible and tested to give the same
  answers, `tests/native_model.rs`): `codec` (base64 whole and in pieces, quoted-printable,
  RFC 2047 words, the charsets of mail, `AUTH PLAIN` credentials in a buffer that wipes itself),
  `header` (headers, Content-Type with RFC 2231 parameters, addresses, dates, reply subjects)
  and `mem` (fallible growth; `Secret` and `SecretBytes` wipe on drop and show nothing in
  `Debug`). Bounds the old code had not: a message has at most 256 headers of 8 KiB, a
  Content-Type 32 parameters, a header 32 addresses, and a date is read for the years 1 to 9999
  only. `tests/native_model_alloc_refusal.rs` refuses each allocation in turn;
  `hostile_bytes_are_read_without_a_panic` feeds random bytes to every reader.
## Ported in slice 2

- `src/native_model/html.rs` (`tests/native_html.rs`): the HTML-to-text reader made fallible and
  bounded. Same text as interface 6 on fixed mails, 4,000 random mails of tags, entities and
  comments, and the same mail cut into pieces anywhere; the intended differences, each tested:
  the text is cut at `MAX_TEXT` (128 KiB; `HtmlText::with_limit` for another, `is_cut` tells);
  a non-ASCII character inside `<style>` or `<script>` is no panic (interface 6 cut its watch for
  the end tag in the middle of a character); a comment longer than 1,024 bytes ends at its `-->`
  (interface 6 never found the end and dropped the rest of the mail). The entity name and the
  watch for the end tag are fixed arrays, the tag keeps its room from one tag to the next, and the
  tag name is compared without a copy. `tests/native_model_alloc_refusal.rs` refuses each
  allocation of whole and piecewise reading.
- `render` still uses the interface-6 `html` until it is ported with the store.

## Slice 3 (still interface 6)

- `imap` (the response parser) and `smtp` (the reply parser) and the connection code that uses
  them need the sockets host (SOCK), which is not there yet; then `render` and `store`.

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

| Piece | Waits for |
| --- | --- |
| The connection (IMAP, SMTP, STARTTLS, TLS, pinned keys), `session.rs`, `driver.rs`, `job.rs` | the sockets host and facade (`docs/abi/sockets.md` is a draft) |
| The password read, used and wiped | the secrets facade (#367 is open) |
| The connection of a screen ending with it | the work service (`docs/abi/work.md`) |
| Choosing a provider fills in the servers (`provider.rs`) | the settings write and the `SettingChanged` event on the native side |
| Folders, list, reader, compose (`browser.rs`, `reader.rs`, `writer.rs`, `compose.rs`) | `core.ui` compose; the card through `core.files` |
| The store of what was fetched (`store.rs`, `render.rs`) | `core.files` and the card root |
| The five home widgets and their settings | the widget host call (Core todo 76) |
| The question *Trust this server?* | `Question::choice` and the sockets certificate event |

Every interface-6 module that is not listed above as made fallible stays as it is.
