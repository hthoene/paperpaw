# Interface-7 port (slice 1: the decisions and the declarations)

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
- `declarations.rs`: the four settings (the service as a choice, the address, user name and notebook
  as advanced texts; all kept on reset, in the library group, edited on the set-up screen), the
  screen (one at a time), the actions `open` and `send`, the place in the tools (order 49) and the
  icons. `tests/abi7_declarations.rs` decodes the batches by the host's rules and compares each with
  the interface-6 registration, including the screen's instance policy from the decoded flags.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- Writing a note (`Note::write`: the title, the body copied from the notes file in pieces, the
  properties, the MD5 of the text) and reading an item's end (`read_props`, `text_hash`): files.
- The sending itself: sign-in, the locks (`locks.rs`, WebDAV and server, read with the XML reader),
  the notebook and note lookups, the waiting and sent lists (`kept.rs`), the retries (network, files,
  declared work).
- The set-up screen (H1, H2), the password (secrets, #367), the keyboard slot.
- The events it hears: `core.notes-exported`, `setting.changed`, `started` and its own answers.
