# Interface-7 port (slices 1 to 3: the decisions, the declarations, the readers and the files)

Version 1.0.7. Partial: what the plugin decides and what it declares, without the host. No native
set-up screen and no `NATIVE_ENTRY` yet; the interface-6 `Joplin` and everything of it stays until the
host activates native plugins (then it goes together, no shim).

## Ported

- `src/native_joplin.rs`: the kinds of sync target (`Service`: setting values, the one after, whether
  it has an address or signs in, the addresses of a file and of the locks), Joplin's ids (`new_id`
  from an MD5 of the seed, `is_id`, `file_name`), the properties at the end of an item (`parse_props`),
  the text of a notebook (`notebook`) and of a note's properties (`Note::props`, with the escapes of
  line breaks) and the title and author at the start of the notes a reader exports (`parse_head`).
  Allocation is fallible and swept. `src/native_parity_tests.rs` compares each with the interface-6
  code (`escape`, `one_line` and `Note::props` became `pub(crate)` for it, nothing else changed).
- Reads of files (slice 2), with the caps of interface 6 enforced in code: `item_props(size, tail)` reads
  an item's properties and where its text ends from the end of its file, **at most `TAIL` = 2,048
  bytes** (a longer `tail` is cut from the front; properties further up are not seen; a shorter one
  is the file's end as far as it goes, and `text_end` counts from `size` less its length), and
  `head_of(start)` reads a notes file's title and author from **at most `HEAD` = 1,024 bytes** of its
  start. Both decode invalid UTF-8 as `String::from_utf8_lossy` does, in fallible memory.
  `Props::text_end` is new (as `read_props`'s). `src/native_read_tests.rs` compares them with
  `item::read_props` and `markdown::head` on the same files and sweeps every allocation. The caller
  reads the bytes (files) and gives them.
- `declarations.rs`: the four settings (the service as a choice, the address, user name and notebook
  as advanced texts; all kept on reset, in the library group, edited on the set-up screen), the
  screen (one at a time), the actions `open` and `send`, the place in the tools (order 49) and the
  icons. `tests/abi7_declarations.rs` decodes the batches by the host's rules and compares each with
  the interface-6 registration, including the screen's instance policy from the decoded flags.

- The files over a `paperpaw_ui_kit::files::Volume` (slice 3; the card's notes and the plugin's data
  folder), each compared with interface 6 on the same input (`src/native_kept_tests.rs`,
  `src/native_note_tests.rs`) and swept for refused memory:
  - `src/native_kept.rs`: `sent.txt`, `sending.txt`, `waiting.txt`, `notebook.txt` (same lines, same
    files; the files of version 1 in `testdata/` still read; a list is rewritten through a copy
    and a failure leaves it as it was). **`waiting()` holds at most 32 paths** (`MOST_WAITING`),
    oldest first; the rest stay in the file and come with the next sending.
  - `src/native_note.rs`: `write_note` (the title, the body copied from the notes in 1 KiB pieces,
    the properties; returns the MD5 of the text; byte for byte as `Note::write`; it writes straight
    to the file, with no staging: a cut leaves a partial file that the next sending writes anew, and
    the hash is kept in `sending.txt` before the note goes, so a partial file is never taken for a
    sent note), `read_props`,
    `text_hash` and `head` over volumes, and the body copy (`Body`). One deviation: the line
    breaks held back at the end of a piece until text follows are **at most 4,096** in a row (a
    body with more blank lines in a row between two texts keeps 4,096 of them); interface 6 held
    them all.
  - `src/native_locks.rs`: whether an exclusive lock is held (WebDAV listing read with the XML
    tokenizer, Joplin Server's list with the JSON parser), in pieces, `NoMemory` as an error so
    nothing is written then. The request for the list is the sending's (next slice).
  - `for_each_line_on` (new, `paperpaw_ui_kit::files`) reads a text file a line at a time (lines cut
    at 4 KiB, as interface 6's `for_each_line`).

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The sending itself: sign-in, the request for the locks,
  the notebook and note lookups, the retries (network, declared work).
- The set-up screen (H1, H2), the password (secrets, #367), the keyboard slot.
- The events it hears: `core.notes-exported`, `setting.changed`, `started` and its own answers.
