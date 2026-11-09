# Interface-7 port (slices 1 to 3: the decisions, the declarations, the readers and the fetch)

Version 1.0.4 (Beta 3's is checked in the last slice; no bump if it is not above). Partial: what the
plugin decides, what it declares and the fetch as a state machine, without the host. No native set-up
screen and no `NATIVE_ENTRY` yet; the interface-6 `ReadLaterCloud` and everything of it stays until the
host glue is there (then it goes together, no shim).

**Blocked on the SDK for the last slice** (see "Open: the library"): the books go into the library with
`Library::import` and out with `Library::delete` (`docs/abi/library.md` 2.3, host side in
`paperpaw-core` `native_host/lib_service`), whose SDK facade is on `services/abi7-sdk-library-files-228`
(`b19fc08ca`, `87338ce64`, `762e5218d`) and in `port-b/wallabag-fetch-228`, not in this branch's base. A
cherry-pick conflicts (it sits on the position facade of the same branch), so nothing of it was copied.

## Ported

- `src/native_read_later.rs`: which texts are article ids (`is_id`), where Reader keeps what is
  asked for (`location`), the number that tells accounts apart (`account`), the start and end of
  an article's book (`head`, `TAIL`), the lists it keeps (`articles`, `finished`, `articles_text`),
  when a page turn is the article read to the end (`is_read`, from the native reading event's page,
  pages and progress in millionths), and the copying of an article's text out of Readwise's answer
  (`Copier`: fed the answer in pieces, finds `html_content`, unescapes as it goes, holds only what
  the caller has not written yet, cut at 1 MiB). Allocation is fallible and swept; `Copier::feed`
  refuses (`NoMemory`) where the text taken cannot be kept, so the caller writes and clears it
  often. `src/native_parity_tests.rs` compares each with the interface-6 code, the copier over
  every kind of escape, broken or missing key and value, fed in pieces of 1 to 100,000 bytes.
- `src/native_answers.rs` (slice 2): the list of articles (`Listed`: the first `max` with an id that
  is letters and digits, `None` for a list that never ends) and the answer to a list asking for one
  article (`Asked`: `Gone`, or where it is and who wrote it). Both read the SDK parser
  (`paperpaw_sdk::json::stream::Parser`) with `try_feed`/`try_finish`, so a refused allocation of
  its path or text ends in `NoMemory` and nothing is returned in part; what the plugin keeps is
  allocated with `try_reserve`. The caller feeds the file in pieces (`feed`, then `finish`); after
  an error the reading is over and every later `feed` says the same. `src/native_answers_tests.rs`
  compares each with `answers.rs` at piece sizes from 1 byte to 2 KiB and sweeps every allocation.
- Read caps (slice 2), those of interface 6, enforced in code:
  `articles` accepts a file of at most `ARTICLES_CAP` (64 KiB) and `finished` one of at most
  `FINISHED_CAP` (16 KiB), counted with a byte order mark, which is left out of the text; over the
  cap they say `NoSpace`, never an empty list, so the caller does not write the file over what it
  holds. An answer with a list or one article's data is at most `MAX_LIST` (512 KiB), one with an
  article's text at most `MAX_ANSWER` (4 MiB): `fits(size, cap)` is the check on the file's size
  before it is read (interface 6 took no answer over it: `max_file`), and `Listed`, `Asked` and
  `Copier` refuse with `NoSpace` the bytes fed over their cap.
- A title, a location and an author are cut to `FIELD_CAP` (256 bytes, at a character's end) when
  kept: a new cap (interface 6 kept them up to the parser's 4 KiB; 20 hostile titles were 82 KiB).
  Tested with multi-byte letters; the item stays.
- The copier after an error: `Copier::feed` stops inside the piece when memory is refused (or the
  answer is over `MAX_ANSWER`), so its state is no longer the text's: it is spent, every later
  `feed` returns the same error, and **the fetch for that article must end** (the book is not
  finished from what was taken). The caller must not use `outcome` after an error. A test sweeps
  every allocation and checks it.
- `declarations.rs`: the setting for where articles come from (*Later* or *Inbox*, kept on reset,
  library group), the set-up screen (one at a time), the actions `open` and `fetch`, the place in the
  tools (order 49) and the icons. `tests/abi7_declarations.rs` decodes the batches by the host's rules
  and compares each with the interface-6 registration, including the screen's instance policy.

- `src/native_fetch.rs` (slice 3): the fetch, `Fetcher` over two seams, tested without a host.
  `Wire` is the data folder (`Volume`), the network (`network`, `send(token, Call)` with the end told
  as `http_done`, token `REQUEST_TOKEN`, `cancel`), the token and the account (the secret `account`);
  `Shelf` is the library (`put(from, file, free_name)` = `Library::import` into the folder `Reader`,
  `remove(path)` = `Library::delete` without `with_notes`). The order and the rules are those of
  interface 6's `Fetch`: the articles read to the end are archived (`PATCH`, 404 counts as archived), the
  list (at most 20, saved to `list.json`, 512 KiB), those fetched that are not in it asked about (at
  most 8, oldest first, `doc.json`, 512 KiB), the new ones downloaded (at most 10, oldest of the newest
  first, `doc.json`, 4 MiB) and copied into `article.tmp` in pieces, then put into the library as
  `name.html` or, where that is taken (by a book, or by an article fetched earlier whose book the reader
  deleted: compared by file name, ignoring case), `name (id).html` with a free name; 19 requests at most.
  401 and 403 (`Stop::BadToken`), 429 (`Wait`), no answer (`Unreachable`), the network gone
  (`NoNetwork`) and an answer over its cap (`TooBig`, from the host's `max_file` as `Reason::TooLarge`
  and from the file's size) end the fetch; a failed step counts and the fetch goes on; a refused
  allocation is `Ended::Memory`. The end of the work run (`Reason::WorkEnded`) ends the fetch quietly
  with its counts (as news). Another token's lists are let go of first (the account is a number of the
  token, kept as the secret `account`, `changeable` off); the books stay in the library.
  `src/native_fetch_tests.rs` holds the cases of interface 6's `tests/fetch.rs` (fetch, archive and
  follow archiving elsewhere, a book that stays, a book with notes, the stops, no token or network, an
  article without text, another token, the caps) and the native ones (a cancelled fetch, a foreign
  request, the end of the run, a refused next request, a name taken, the 10/8 limits, the page turn, a
  sweep of every allocation of a whole fetch). Two more files: `src/native_lists.rs`
  (`articles.txt`/`finished.txt` over a `Volume`, `page_turned`: the page turn that ends an article
  notes its id, only for a path in a folder `Reader` that the list knows) and `src/native_secret.rs`
  (`Secret`, wiped on drop, `Debug` shows nothing).
- `native_answers::Asked::with_cap` (new): the answer that holds an article's text is read up to 4 MiB
  (`MAX_ANSWER`), the parser passing the text by; `Asked::new` keeps the 512 KiB of the questions.

## Differences from interface 6 (slice 3)

- The archive removes a book with `Library::delete` without `with_notes`: the library refuses a book
  with the reader's marks (`State`), which is interface 6's `annotated` test, so the plugin reads no
  annotations. Any other refusal also keeps the book and lets the article go, as interface 6 did where
  `remove` failed.
- A page turn is looked at only for a path with a folder `Reader` just above the file (interface 6:
  below the library folder's `Reader`, which a native plugin does not read); the list still decides.
- Answers over a cap end the fetch from the host's `max_file` too (`Reason::TooLarge`), not only from
  the file's size afterwards.
- No `library::added` notice: `Library::import` catalogues the book.
- Secrets: the token and the account are the plugin's own secrets `token` and `account` (labels
  `read-later-cloud.secret` and `read-later-cloud.secret.account`). Interface 6 kept them as
  `read-later-cloud.token` / `read-later-cloud.account`, which the host's file reads as owner
  `read-later-cloud`, name `token` / `account` (`secrets.md` 1.2, 7: the first segment is the owner), so
  Beta 3's token is found and not asked for again; nothing is migrated by the plugin.

## Open: the library

- The host side of the fetch: `HostWire` (core.net, core.secrets), `HostShelf` (core.library `import`
  into the folder `Reader`, `delete`), the subscriptions (`http_done`, `page_turned`, `work_due`,
  `stopping`), the set-up screen and the token question (copy `plugins/readwise/src/native_plugin.rs`
  and `plugins/wallabag` on `port-b/wallabag-fetch-228`), `build.rs`, `native_entry.rs`, the metadata,
  the notices (`read-later-cloud.fetching`, `.done`, ... exist in `i18n/`), and then the deletion of
  the interface-6 code, `tests/fetch.rs` and the cases of `tests/acceptance_pending.rs` that are
  done, the changelog fragment and the move from `tools/plugin-files` to `tools/plugin-files7`.
  These wait for `Library::import`/`delete` in the SDK.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The set-up screen (H1, H2), the token (secrets, #367), the keyboard slot and the set-up's row.
- The archive and the account check on the host: the page-turn subscription (`native_lists::page_turned`
  is the decision), `Library::delete` and the secret `account` through `core.secrets` (see "Open").
- The fetch started by a rule as declared work (`core.work` is there: `Work` with the network at the
  moment it is online, as readwise's retry; the action's glue is the last slice's).
