# Interface-7 port (complete in code; the host does not run it yet)

Version 1.0.7. Native only: the interface-6 `Wallabag` and its tests are gone. `NativeWallabag`
(`NATIVE_ENTRY`, features `linked` and `native-entry`, `native/metadata*`, `build.rs`) is the plugin. It is
not linked into the app, so Wallabag is absent from the app's plugins and from the store's list until the
host activates native plugins (Core #944, Host #417); `tools/plugin-files/` has no file for it until the
store build moves to interface 7.

## Ported

- `src/native_wallabag.rs`: which texts are article ids (`is_id`), the set-up's rows (`Row`: title,
  keyboard field, and the row asked for by a field), the lists it keeps (`articles` and `finished`
  over the file's text, `articles_text` writing the first back), and when a page turn is the article
  read to the end (`is_read`, from the native reading event's page, pages and progress in millionths:
  95 % where pages are not counted). Allocation is fallible and swept. `src/native_wallabag_tests.rs` pins each (lines of another shape, CRLF lists).
- `src/native_fetch.rs` (slice 2): the list of unread articles (`Unread`: the first 30 with an id that
  is a number, `None` for a list that never ends) and an article's archived flag (`Archived`). Both
  read `paperpaw_sdk::json::stream::Parser` with `try_feed`/`try_finish`, so a refused allocation
  of the parser's path or text ends in `NoMemory` and nothing is returned in part; what the plugin
  keeps (items, ids, titles) is allocated with `try_reserve`. A title is kept as the book's name
  (`clean_name(title, NAME_UNITS)`, 80 UTF-16 units, the only use the plugin has for it; cleaning is
  idempotent, so the same book comes of it): 30 titles of 4 KiB do not stay in memory. The caller feeds the file in pieces as
  it reads it (`feed`, then `finish`); a list of any length is read in the parser's bounded memory,
  and only the first 30 items with an id are kept. `src/native_fetch_tests.rs` pins each at piece sizes from one byte to 2 KiB and sweeps
  every allocation.
- Read caps (slice 2): the native caller keeps interface 6's caps. `articles` accepts a file of at
  most `ARTICLES_CAP` (64 KiB) and `finished` one of at most `FINISHED_CAP` (16 KiB), counted with
  a byte order mark, which is left out of the text as in interface 6; over the cap they say
  `NoSpace`, never an empty list, so the caller does not write the file over what it holds. The
  streamed answers (`entries.json`, `entry.json`) have no cap, as in interface 6, as they are read
  in pieces.
- `declarations.rs`: the three settings (server; user name and client ID as advanced ones; all kept on
  reset, in the library group, edited on the set-up screen), the screen (one at a time), the actions
  `open` and `fetch`, the place in the tools (order 48), the icons, and the subscriptions to the end of a
  request (`http_done`) and to the reader's page turns. `tests/abi7_declarations.rs`
  decodes the batches by the host's rules and pins them, including the screen's instance policy
  from the decoded flags.

## The set-up (`src/native_plugin.rs`, `src/native_keep.rs`)

The set-up screen on `core.ui`: six rows (server, user name, password, client ID, client secret, *Fetch
articles*), a text question for each (the server as an address, the secrets masked, the others filled with
the setting), the next row focused after an answer. The three texts are settings; the password and the
client's secret are kept with `core.secrets` (names `password` and `client-secret`, changeable, as in
`secrets.md` 7). `Secret` zeroes a secret when it is dropped and its `Debug` shows nothing; the buffers of
each secrets call are wiped. An empty answer for a secret keeps what was kept; where the device keeps no
secrets the shared text `core.secrets-unsupported` says so; a save refused for another reason (a value
too long) says `wallabag.not-kept`. A question whose text could not be filled stays open with an empty
field and its answer is still taken. The note below the rows says how the last fetch went.

## The fetch (`src/native_fetcher.rs`, `src/native_lists.rs`)

Started by the *Fetch articles* row or the action `wallabag.fetch`, in the order of interface 6: a token for
the account (`POST /oauth/v2/token`, the form built in one 2,048-byte buffer, percent-encoded, zeroed
after), the articles read to the end archived (`PATCH`, then the book goes), the list of unread articles
(saved to `entries.json`, read in pieces by `native_fetch`), each article fetched earlier that is not in
it asked about (`entry.json`: gone with a 404 or archived), and each new article downloaded as an EPUB
(`article.tmp`) and put into the library.

- `core.net`: one request at a time, ended by one `http_done` (the plugin subscribes to it), the token's
  answer read as a memory body in pieces. The bearer header is a `Secret`. Caps the plugin sets: the list
  and the single article 1 MiB, a book 32 MiB, the token answer 4 KiB (interface 6 had none of these; the
  host's own cap is 64 MiB).
- `core.library`: `Library::import` puts the book into the folder `Wallabag` of the library (`name.epub`
  under *Refuse*, else `name (id).epub` with a free name, so no book of the reader's is replaced; an
  article fetched before whose book was deleted keeps its name apart); `Library::delete` without
  `with_notes` takes a book away, and its refusal for annotations (`State`) keeps the book, only the article
  is let go of, as in interface 6.
- The lists (`articles.txt`, `finished.txt`, `account.txt`) live in the data folder behind the `Files`
  trait of `native_lists` (the host's `DataFiles`, or memory in the tests); the lists of another server
  or user are let go of first, as before.
- Only a 404 counts as "gone" when a fetched article that is no longer listed is asked about; a 2xx says archived or not, any other answer is a failed check and the book stays.
- The files of a fetch (`entries.json`, `entry.json`, `article.tmp`, up to 32 MiB) are cleared when a fetch starts (a power cut leaves them) and when the plugin stops (`Fetcher::cancel`, on `stopping`).
- The reader's page turns (`PageTurned`) note an article read to the end; only a book under a `Wallabag`
  folder looks at the lists.
- Allocation is fallible; a refused allocation ends the fetch with `Error::NoMemory`, every other failure
  of a step counts as one failed article and the fetch goes on. The ids are numbers, a title is cleaned,
  what is not a whole list changes nothing. `tests/native_fetch_flow.rs` drives the fetcher against
  scripted `core.net` and `core.library` services and an in-memory data folder (requests as sent,
  imports, archives, deletions, a bad login, a name taken, another account, a list cut short) and
  sweeps refused service calls and allocations.

Deliberately different from interface 6:

- No offer to go online first. A fetch the reader starts is within the host's 2,000 ms window of user input
  (`network.md` 3.2), so the offline power mode lets it through; a request ended for the power mode or the
  network says `wallabag.no-network`.
- The token is read by the JSON parser, whose own copy of the value is not zeroed (the token is a
  short-lived session key that is never stored); every copy the plugin makes is.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- Host acceptance: the plugin through the production loader needs the activation (#944, #417) and the
  shells' production `core.net` and storage wiring (Files/Firmware); no acceptance case passes by itself.
- The fetch started by a rule as declared work (no native work declaration yet): interface 6's
  `wallabag.fetch-due` has no counterpart.
- The set-up screen on the production host (H1, H2) and the keyboard slot, as before.
- Not measured: the X4 file size (`tools/plugin-files7`, no `complete.txt` entry before it is) and peak
  memory of a fetch.
