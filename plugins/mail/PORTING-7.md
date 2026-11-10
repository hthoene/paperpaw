# Mail on interface 7 (slices 1 to 6, complete in code)

Status: **complete in code, with one piece waiting for the host.** The plugin is native only: the
interface-6 mail (`plugin.rs`, the sessions, the screens, the store, the widgets, the provider fill-in)
is gone, and with it the app's built-in offer of it and its interface-6 plugin file. The native plugin
declares its settings, screens, actions, widgets and sockets, and runs its sessions over `core.sockets`
(TLS from the start and STARTTLS). What waits for the host is under "Not ported yet": the cases are
ignored ones in `tests/acceptance_pending.rs`. No host with `core.ui`, storage and sockets runs a native
plugin end to end yet (#944), so nothing of what the screens do with the host is run end to end. The
interface-6 code that the native tests compared with is gone: the tests check the native code against the
answers interface 6 (Beta 3) gave, kept in `testdata/pins/` (`tests/pin/mod.rs`); a change of an answer
fails a test and is reviewed, `PAPERPAW_BLESS=1 cargo test -p paperpaw-plugin-mail` writes the files anew.

## Ported in slice 1

- `declarations.rs`, `build.rs`, `native/metadata*`, `src/native_plugin.rs`: the group `mail.group`
  (interface 6: `mail`; every ported plugin names its group so), the pages `mail.account.1` to
  `3`, the 13 settings of each account and the three for all (42), all with their en/de texts
  unchanged. The password of an account is declared as a **secret**. The settings, the groups,
  the actions *open* and *write*, the Tools entry (order 47), the icons and the four screens are
  compared field by field with what the interface-6 plugin registers (`tests/parity.rs`, against the pinned interface-6 values); the
  settings are also a golden (`testdata/declarations.bin.v1`).
- The actions, the entry, the icons and the screens are built by `declarations::ui` and
  `declarations::screens` and are **not in the image**: they would open nothing.
  `metadata*.json` lists the feature `uses.network` (slice 6); `uses.storage` is never needed (the data
  folder is root 0, no feature).

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
  folder (`a1/f<name>-<hash>/index.txt`, not `.paperpaw/plugins/mail/…` on the card: the data folder is that folder, see "The interface-6 files on the card" below); no `.part` file of Mail's own (the data
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
  `tests/native_session_starttls.rs`: bytes behind the STARTTLS OK (IMAP) or 220 (SMTP) and bytes
  that arrive while the switch is under way are never parsed: the session ends `Failed(Unsafe)`; so
  does a `start_tls` the socket refuses with `Error::State` (the SDK reader still holds bytes of the
  plain stream). The slice that adds the `Io` over `native::sockets` must pass that refusal through.
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

## Ported in slice 6 (the connection and the account)

- `src/native_model/link.rs`: the link between a session and its socket. What a session asks of `Io` becomes calls
  on a `Net` (the socket; a double in the tests), what the socket tells becomes the session's `Input`: bytes of a
  `Received` event are read in pieces (`PIECE`, 3,840) and given one at a time, a finished session stops the reading,
  a `Saved` names the file the read asked for, `Secured` and `Untrusted` carry the peer's hashes. What a session sends
  is queued in a buffer that wipes itself (a login line passes through it) and handed to the socket piece by piece as
  its queue has room: a full queue (`Busy`) waits for the next `Sent`, and a send while one waits follows it in order
  (a secret never grows in place: a new buffer, the old one wiped). `start_tls` passes the socket's `State`
  refusal (bytes of the plain stream still held) to the session, which ends as unsafe (slice 5). `tests/native_link.rs`.
- `src/native_model/job.rs`: one connection, as the plugin keeps it between callbacks (`Run`: working, done, failed,
  untrusted with its hashes and which server); one at a time (`Busy`). A socket silent for too long ends with the
  host's `idle_ms` (70 s on the connection, 45 s to connect: interface 6's silence of 45 s, and 70 s more while a
  message streams into a file), which reaches the session as a failed socket: no timer of the job's own.
  `tests/native_job.rs` runs whole IMAP sessions (TLS from the start and STARTTLS, in pieces of 1 to 100,000 bytes) and
  SMTP sends on the fake server, a wrong password, an untrusted server, a device with no clock (the password is not
  sent), a refused recipient, a socket that fails, and a second start while one runs.
- `src/native_model/account.rs`: an account read from the settings (`mail.N.*`) with the servers, the pins (64 hex
  digits) and the presets, fallible (an allocation refused is an error, never an account that is not there). **The
  password is not in the account**: `Account::password` reads it from the secrets for one connection and it is a
  `Secret` (wiped when dropped, hidden from `Debug`); a device that keeps no secrets gives `Unsupported`. The offer
  for an untrusted server (`Ask`, `Changed`, `Attack`) is `account::offer`. `tests/native_account.rs`.
- `src/native_socket.rs` (`HostNet`, the SDK's `Sockets` as a `Net`) and `src/native_values.rs` (`HostValues`: the
  settings through `settings.read`, the password through `core.secrets`, the reply buffers wiped at once). They are
  tested only by what they call: the SDK's own tests; no host runs native plugins with sockets yet.
- `NativeMail` holds the job and the socket, opens `core.sockets` and `core.secrets` when first needed, takes the
  socket events (`event.socket`, subscribed in `declarations::events`) and has `start_imap` and `start_smtp` for the
  screens of the next slices. Metadata: services `core.sockets` and `core.secrets`, feature `uses.network`
  (no `uses.storage`: the cache is in the data folder), and the storage and blob calls. Frames: nothing here keeps a
  buffer on the stack above 320 bytes (the call buffers, 8 KiB, and a piece of what came, 3,840 bytes, are on the
  heap and wiped). A host error in a notice ends the job and closes the socket at once.
- Differences from interface 6, on purpose: the password is read when a connection is made and not kept in the
  account; the silence timer is the host's.

## Ported in slice 7 (the list screens)

- `src/native_model/browser.rs` (`tests/native_browser.rs`): the lines of *Mail* (the folders of every account with their
  unseen mails, the check, writing, the settings; the one line that says to set up an account) and of a folder (its mails
  newest first, with the unseen and flagged marks), the tasks of a check (folders and inbox for *Mail*, the one folder for
  a folder), the names of folders by purpose, the title of a folder, `when_of` (the time of day today, else the date, in the
  reader's zone), and the fingerprint in two lines. Everything allocated is asked for; the words come through `Words`.
- `src/native_model/texts.rs` (`tests/native_texts.rs`): which catalog message says where a connection is and why it failed.
- `src/native_screens.rs`, `src/native_words.rs`: the screens `mail.boxes` and `mail.list` on `ItemList`, one `View` for each
  open instance (the handover carries `account` and `folder`). Choosing the first row checks for mail: one account after
  the other for *Mail*, the folder's account for a folder, one connection at a time, and the status line under the rows says
  where it is and how it went. A screen opened above (a folder, the settings) lets go of the connection, and the one below
  checks again when it shows; a screen that closes closes its socket. An untrusted server is asked about once with
  `Question::confirm` (Trust, Don't trust) and the key is kept as the account's pin setting (`set_setting`) when the reader
  trusts it; a provider's server or a changed key is only said, as in interface 6. The Tools entry opens *Mail*.
- *Write a mail* opens the compose window of slice 9. **Mail stays out of `NATIVE_PLUGINS` and `complete.txt`** until the
  provider fill-in and the removal of interface 6 are done. No host with `core.ui`, storage and sockets runs
  a native plugin end to end yet (#944), so what the screens do with the host is an ignored case; the lines, the tasks,
  the words and the times are tested without one.
- A reader or the compose window that meets an untrusted server says what `account::offer` allows, as Beta 3 did: *open the
  list* where the list can ask, *the key changed* or *a provider's server* where nothing is offered (`texts::pin_message`,
  tested for the three keys). **Known gap, as in Beta 3:** only the compose window starts SMTP and the list checks IMAP
  only, so an untrusted SMTP server (*Ask*) is told to open the list, which never asks about it. Asking in the compose
  window with `Question::confirm` and sending again after the answer is a follow-up, not done here.
- Interface 7 has no event for a screen of another plugin put above one of Mail's, so a connection is not let go then as
  it is when a Mail screen opens above (interface 6 did not either: it lets go only when its own screens open or close);
  the host's `idle_ms` ends a connection nobody serves, and a screen that closes closes its socket.
- Differences from interface 6, on purpose: the hash is grouped by a fallible function; the date is the reader's zone from
  the host's snapshot.

## Ported in slice 8 (the reader)

- `src/native_model/reader.rs` (`tests/native_reader.rs`): the headers and the text as the reader shows them (the sender, the
  copies, the date, the attachments where there is something to say, the text, and a line where it was cut), the tasks that
  fetch a mail (and mark it read where the setting says so), the star and read marks in the folder's list, where the parts
  of the screen go (`layout`), how the text scrolls (a third of a page for up and down, a page for page keys, swipes and taps
  in the text) and how the focus goes round the three buttons.
- `src/native_reader.rs`: the screen `mail.read` (the handover carries `account`, `folder` and `uid`), drawn from owned
  paint like the other native screens: the header (back on touch), the lines of the mail from the one the reader is at
  (wrapped once for the width, at most 1,024 lines, 24 KiB of text), the status line and *Reply*, *Reply all* and *Star*.
  The mail is fetched the first time it shows if it is not on the card, becomes a letter on the card and is marked read in
  the list; *Star* marks the list at once and the server after; leaving the screen (or opening the compose window above)
  lets go of its connection. An untrusted server is said (*Open Mail to trust this server*): the list asks.
- Differences from interface 6, on purpose: the letter is not kept in memory beside the text shown (it is read again from
  the card where the reply needs it).
- **Gates, unmeasured (no host runs this end to end yet, #944):** the heap peak of the first show of a mail (the raw message
  read up to 48 KiB, the letter 24 KiB, the text shown 24 KiB and the wrapped lines, up to 1,024 of them; keeping the lines
  as ranges into the text shown would halve the lines' share) and the time of the first draw's wrap on the X4. A refused
  allocation while the mail is made says *not enough memory* on the screen instead of waiting for it.

## Ported in slice 9 (compose and send)

- `src/native_model/compose.rs` (`tests/native_compose.rs`, against interface 6 on random drafts): the draft kept between
  openings, the recipients and what refuses a draft, the message (headers, MIME, quoted-printable where it is not ASCII or
  a line is too long, no header can be broken out of), the reply to a letter, the Date header and the quote; fallible.
- `src/native_model/writer.rs` (`tests/native_writer.rs`): the rows (the sender only where there are several accounts), what
  each says, what a typed answer changes, the refusal messages, the fresh draft with the signature, the sender's name,
  and the id of a message (eight random bytes of `core.secrets`, else the time).
- `src/native_writer.rs`: the screen `mail.compose` (one at a time) as an `ItemList`: *From* picks the next account, *To*,
  *Cc*, *Subject* and *Message* ask for text with `Question::text` (email keyboard for the addresses, several lines for the
  text, starting from what is there, written in parts), every answer keeps the draft on the card, *Send* checks the draft,
  makes the message and sends it through the account's SMTP server; sent, the draft is removed and the window says so (and
  any key goes back). A reply starts from the letter on the card, the other ways from the draft kept or the signature.
  The *write* action opens it. Closing it lets go of its connection.
- Differences from interface 6, on purpose: the texts are bounded (a draft's text 16 KiB, an address list or a subject 2 KiB).

## The interface-6 files on the card

Interface-6 Mail keeps its files under `.paperpaw/plugins/mail/` (`aN/…`), and that is the plugin's data
folder (storage root 0, `docs/abi/files.md` 1.1; `Env::plugin_dir("mail")` in interface 6): the native
paths (`aN/draft.txt`, `aN/outbox.eml`, `aN/f<name>-<hash>/…`) name the same files. So **nothing is migrated,
copied or removed**: the drafts and the outbox (the user's own writing) are found where they were, the
cache is read or comes again from the server, and the plugin needs no `uses.storage`. Deleting the folder at
the first native start would lose mail not yet sent. A test (`tests/native_render_store.rs`) holds the
draft and outbox paths equal to interface 6's `store::draft_file` and `store::outbox_file` under that folder.

## Ported in slice 6: the home widgets (`src/native_widgets.rs`, `declarations::widgets`)

- *Unread mail*, *Latest mail*, *Starred mail*, *Mailbox* and *Write a mail* are declared with the shared look and their
  own settings (account, folder, count, the parts caption, subject, time, newest, total), without `.previews` (#861). The
  host makes an instance per placement (`event.widget_made`), asks its height through the `core.widget` stage and calls
  the plugin to draw it (`DrawPurpose::Widget`). A tap (one target: the whole widget) opens the folder, the list of
  folders, or a new mail.
- They read what the card holds of the mail checked last (the index and the counts of the store), never the network. On
  the tick the plugin compares a stamp of what the widget reads (the index's writes, the counts, the day) with the last
  one and tells the host (`widget_changed`) when it changed; a setting change or a check's end tells every instance.
- The frames are X4-safe: the rows are sorted with the SDK's `sort_by` (the std unstable sort took a 3,952 B frame on the
  X4); the packed file `mail-1.0.4-x4.ppn` is 338,272 B and the packer accepts it (no frame over 2,048 B).
- Not measured: the heap peak of a measure on the X4; no scripted-host test of the stage and the draw yet (the lines
  the widgets read are the store's, tested in `native_render_store.rs`).

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

| Piece | Waits for |
| --- | --- |
| A connection started by a screen and ending with it (the list screens do it) | a scripted host with `core.ui`, sockets and storage (#944) |
| Choosing a provider fills in the servers (`native_provider.rs` holds the table; interface 6's `provider.rs` is gone) | the host's `SettingChanged` event for plugins (#763); until then the servers are typed in |
| TLS on the desktop shell (mail runs over `core.sockets` with TLS; the desktop shell has none yet) | the desktop TLS of Firmware's #990 (approved, not merged) |
| The store of what was fetched is called by the sessions and the screens (done) | a scripted host (#944) |
| The question *Trust this server?* (asked by the list screens) | a scripted host with `core.ui` (#944) |

