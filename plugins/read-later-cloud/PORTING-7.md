# Interface-7 port (slices 1 and 2: the decisions, the declarations and the readers)

Version 1.0.5. Partial: what the plugin decides and what it declares, without the host. No native
set-up screen and no `NATIVE_ENTRY` yet; the interface-6 `ReadLaterCloud` and everything of it stays
until the host activates native plugins (then it goes together, no shim).

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
- The copier after an error: `Copier::feed` stops inside the piece when memory is refused (or the
  answer is over `MAX_ANSWER`), so its state is no longer the text's: it is spent, every later
  `feed` returns the same error, and **the fetch for that article must end** (the book is not
  finished from what was taken). The caller must not use `outcome` after an error. A test sweeps
  every allocation and checks it.
- `declarations.rs`: the setting for where articles come from (*Later* or *Inbox*, kept on reset,
  library group), the set-up screen (one at a time), the actions `open` and `fetch`, the place in the
  tools (order 49) and the icons. `tests/abi7_declarations.rs` decodes the batches by the host's rules
  and compares each with the interface-6 registration, including the screen's instance policy.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The fetch: the calls (network, files) that feed the readers above, and the files' sizes for `fits`.
- The set-up screen (H1, H2), the token (secrets, #367), the keyboard slot and the set-up's row.
- The archive: the page-turn event, annotations (a book with the reader's marks stays), the account
  check that lets go of another account's lists (secrets), and the book's free name (files).
- The fetch started by a rule as declared work (no native work declaration yet).
