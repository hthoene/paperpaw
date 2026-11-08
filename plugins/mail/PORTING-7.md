# Mail on interface 7 (slices 1 to 5, partly ported)

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
  the text is cut at `MAX_TEXT` (24 KiB, as interface 6's `store::MAX_TEXT`; `HtmlText::with_limit` for another, `is_cut` tells);
  a non-ASCII character inside `<style>` or `<script>` is no panic (interface 6 cut its watch for
  the end tag in the middle of a character); a comment longer than 1,024 bytes ends at its `-->`
  (interface 6 never found the end and dropped the rest of the mail). The entity name and the
  watch for the end tag are fixed arrays, the tag keeps its room from one tag to the next, and the
  tag name is compared without a copy. `tests/native_model_alloc_refusal.rs` refuses each
  allocation of whole and piecewise reading.
- `render` still uses the interface-6 `html` until it is ported with the store. In slice 3 it must
  make the reader with `HtmlText::with_limit` (the room the store has left) and flush the text in
  chunks as it comes, never building a String of the whole mail.

## Ported in slice 3

- `src/native_model/imap.rs` and `smtp.rs` (`tests/native_imap_smtp.rs`): the IMAP response reader
  (line buffer, tokens, responses, FETCH helpers, quoting, Modified UTF-7 names, flag letters) and
  the SMTP reply parser, extensions, commands and DATA, fallible. Same answers as interface 6 on
  fixed lines, 6,000 random IMAP lines, random reply sequences and the buffer cut anywhere (compared
  by `Debug` text). `tests/native_model_alloc_refusal.rs` refuses each allocation in turn.
- Differences from interface 6, on purpose, each tested: a line is at most 8 KiB (`response` and
  `tokens` refuse a longer one, `LineBuffer::line` drops it) and the buffer holds at most 16 KiB
  (the session takes what it has between two pushes; it grows by exactly what is pushed, so a
  push that enlarges it holds the old and the new block at once, 32 KiB at the most); a line makes
  at most 256 tokens (a line of single-letter atoms would make some 4,100, about 200 KiB) and a
  CAPABILITY line, a response code's arguments and a list of flags at most 64 words: past a bound
  the answer is `Fault::TooLong`, never a larger allocation; a FETCH line is read from its own bytes
  (interface 6 lost the items of a line ending in white space or holding bytes that are no UTF-8);
  `smtp::auth_plain` and `imap::quoted_secret` make the lines that carry the password in a buffer that
  wipes itself and shows nothing in `Debug`; the types that grow are `#[non_exhaustive]` and do not
  derive `Clone` (it allocates without asking).
- The SMTP reply bounds are as they were (line 1,000 bytes, reply 4,096, 64 lines). `smtp::data` leaves
  the bound of the message to the caller, who holds it whole.

## Ported in slice 4

- `src/native_model/render.rs` (`tests/native_render_store.rs`): the message renderer (headers, MIME
  structure, quoted-printable, base64, charsets, HTML, attachments) made fallible and bounded, and
  `src/native_model/store.rs`: what Mail keeps (paths, the index with its serial, counts, folders and
  their sort, the letter) and `fail.rs` (`Fail`: `NoMemory` or the card's `StorageError`; a refused
  allocation is never taken for a card that is gone). Same text, same headers, same files as
  interface 6 on fixed mails, 3,000 random mails of every structure the renderer knows, the same
  index, counts, folders and letters (compared byte for byte); `tests/native_model_alloc_refusal.rs`
  refuses each allocation of rendering and of the store in turn on a card double that never allocates;
  hostile bytes are rendered without a panic.
- Differences from interface 6, on purpose, each tested: `read_index` returns at most 50 rows and `read_folders` at most 64 (what a write keeps; interface 6 read every line of a file under its size cap); the paths are relative to the plugin's data
  folder (`a1/f<name>-<hash>/index.txt`, not `.paperpaw/plugins/mail/…` on the card; what an
  interface-6 Mail kept there is not read, see "The interface-6 files on the card" below); no `.part` file of Mail's own (the data
  folder's `write` makes the parent folders and is the old file or the new one whole); the scratch
  file of a letter is `m<uid>.txt.tmp`; the HTML reader of a part is made with
  `HtmlText::with_limit` for the room the text has left, as slice 2 asked; folders are sorted by
  lower-case characters without a lower-case copy per comparison (a word-final sigma sorts as `σ`,
  where `to_lowercase` made it `ς`); an index over 64 KiB, a list of folders over 32 KiB and a letter
  over its bound read as nothing, as before; `Row`, `Counts`, `Folder`, `Letter` and `Rendered` are
  `#[non_exhaustive]` and cannot be cloned without asking (`Row::parse` and `Folder::new` make them).
- `HtmlText` (slice 2) now drops everything after the first text that did not fit the limit: what is
  kept is a prefix. It kept a later, smaller character before; interface 6 cut the text as a prefix,
  and the renderer now gives the same text. Tests of slice 2 are unchanged.

## What the session slice must do with these (slice 5)

- Cap the `{n}` literals it accepts (a body part is read in chunks into the card, never whole; an
  `n` over the cap ends the fetch of that message), since `Token::Literal(n)` is only a number here.
- Close the connection on `Fault::TooLong` (a server whose line or response is past the bounds is
  not speaking IMAP) and on a refused allocation after telling the user; never go on with a line
  half read.
- Build the final `LOGIN` line (the tag, the user and the password quoted by `quoted_secret`) in a
  buffer that wipes itself (`Secret`), send it from there, and wipe it; the same for `AUTH PLAIN`
  (`smtp::auth_plain` already makes it so). No `format!` with the password.

## Ported in slice 5

- `src/native_model/session.rs` (with `session/imap_session.rs` and `session/smtp_session.rs`;
  `tests/native_session.rs`): the IMAP and SMTP sessions as state machines, fallible and bounded.
  Bytes and socket notices go in (`step`), what to send and read goes out through the `Io` trait,
  what happened comes back as `Event`s (`Stage`, `Done`, `Failed`, `Untrusted`). Same lines sent,
  same events, same files on the card as interface 6 on the same fake servers (the interface-6
  `Fake` of `tests/fake.rs` plays the server for both): the whole read (folders, headers, a message,
  a flag) over TLS from the start and with STARTTLS in pieces of 1 to 100,000 bytes, SASL-IR, LOGIN,
  LOGINDISABLED, a wrong password, an untrusted server, no clock, an empty folder, a message over
  `MAX_FETCH` read into a file, a server that hangs up, and SMTP over both with AUTH PLAIN and AUTH
  LOGIN, no STARTTLS, a refused recipient, a message too large for the server.
  `tests/native_model_alloc_refusal.rs` refuses every allocation of a whole IMAP session (all four
  tasks) and a whole SMTP session in turn, on a card and a server double that never allocate: each
  ends `Failed(NoMemory)` (the socket closed) or succeeds.
- Differences from interface 6, on purpose: a refused allocation ends the session and is told as
  `Failure::NoMemory` (it never aborts); the password is a `Secret` and every line that carries it
  (the final `LOGIN`, `AUTHENTICATE`, the credentials after a `+`, `AUTH PLAIN`, the password of
  `AUTH LOGIN`) is built in a buffer that wipes itself and wiped as soon as `Io::send` returns
  (`Io::send` copies what it keeps, and the implementation wipes that copy once sent); a line or
  response past the bounds of slice 3 (`Fault::TooLong`) closes the connection as
  `Failure::Protocol` (tested); an `Io` call that fails ends the session as `NoTls` (`Unsupported`),
  `Busy`, `NoMemory` or a network failure (tested); the pinned key is a `[u8; 32]`, and why a
  certificate is not trusted and why a socket failed are the SDK's `Untrusted` and `Reason`;
  `Endpoint`, `Task` and the rest are `#[non_exhaustive]` and not `Clone` (the endpoint holds the
  password); `Imap::receiving` and the `Saved` notice are as before; the scratch file of a large
  message is `<file>.<n>` in the data folder (`core.sockets` reads a file into the data folder).
- What the connection slice (6) must do with it: an `Io` over `native::sockets` (`Sockets`,
  `Reader::fill` and `Outbox::pump` in the callback of each socket event, the `Tls` pins from
  `Io`'s `pin`, `SocketEvent` kinds as `Input`), one socket per screen kind, the password read from
  the secrets facade into a `Secret` (and `Kept::Unsupported` shown as `core.secrets-unsupported`),
  the `{n}` literals a body part reads into a file by `FileRead`, and the driver and job
  (`driver.rs`, `job.rs`, still interface 6).

## The interface-6 files on the card

Interface-6 Mail keeps its files on the card under `.paperpaw/plugins/mail/` (`aN/…`). The folders, lists,
counts and messages in them are a cache that comes again from the server, but **the drafts
(`aN/draft.txt`) and the outbox (`aN/outbox.eml`) are the user's own writing**, so cleaning the folder
at the first native start would lose mail not yet sent. Neither a migration nor a clean-up fits this
slice: it has no start code, and reading the card's files needs `uses.storage` (root 1) in the
metadata, which only the slice that makes the first start can justify. It is a gap, with an ignored
case (`the_interface_6_cache_and_drafts_are_migrated_or_cleaned_once` in `tests/acceptance_pending.rs`):
the slice with the first start must copy the drafts and outboxes into the data folder, then remove the
old folder (cache and all), once, and say so here. Until then the old files stay on the card.

## Still interface 6

- `driver.rs`, `job.rs` (the connection and the commands over `native::sockets`; the sessions themselves are native since slice 5) and `session.rs` (the interface-6 one stays until they are replaced), `render.rs` and `store.rs` (the interface-6 ones stay
  as the old sessions and screens use them; the native ones of slice 4 take their place with the sessions),
  then the screens. They are the next slices; the pieces above are what they will call.

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

| Piece | Waits for |
| --- | --- |
| The connection over the socket (IMAP, SMTP, STARTTLS, TLS, pinned keys): `driver.rs`, `job.rs` | the connection slice over `native::sockets` (#587) |
| The password read, used and wiped | the secrets facade (#367 is open) |
| The connection of a screen ending with it | the work service (`docs/abi/work.md`) |
| Choosing a provider fills in the servers (`provider.rs`) | the settings write and the `SettingChanged` event on the native side |
| Folders, list, reader, compose (`browser.rs`, `reader.rs`, `writer.rs`, `compose.rs`) | `core.ui` compose; the card through `core.files` |
| The store of what was fetched, called by the sessions (`store.rs`, `render.rs` are native since slice 4) | the sessions and the screens that call them |
| The five home widgets and their settings | the widget host call (Core todo 76) |
| The question *Trust this server?* | `Question::choice` and the sockets certificate event |

Every interface-6 module that is not listed above as made fallible stays as it is.
