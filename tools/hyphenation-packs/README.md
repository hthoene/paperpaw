# Hyphenation data plugins

Builds and checks the thirteen hyphenation plugins (the English base and twelve
languages) as plugins **without code**: a folder with `plugin.ini`, `i18n/`, `how/`
and the compiled table `hyphenation/<lang>.pphp`. This is the first port batch of the
interface-7 round (#228); the contract is
`docs/abi/proposals/declarative-registration.md` (Q1, Q14, D4 and its
`[hyphenation]` file kind).

```sh
cargo run -p paperpaw-hyphenation-packs -- build . /tmp/hyphenation-packs
cargo run -p paperpaw-hyphenation-packs -- check /tmp/hyphenation-packs/*
cargo test -p paperpaw-hyphenation-packs
```

`plugin.ini` holds `[plugin]` (`id`, `name`, `description`, `version`, `formats = 5`) and
`[hyphenation]`: a line `<tag> = <relative path>` for each language, at most 8 a plugin.
The contract text is in `docs/abi/proposals/declarative-registration.md`, "The `[hyphenation]`
data-plugin kind"; this tool refuses on the host what the loader and the reader refuse:

- A tag is `[A-Za-z0-9-]`, 1 to 35 bytes, kept in lowercase; a tag twice in one plugin is refused
  (the loader would skip the line with a warning). A path is relative, `[A-Za-z0-9._-]` and `/`, at
  most 3 segments and 100 bytes, with no empty, `.`, `..` or dot-leading segment. Two tags may name
  one file (`nb` and `no`).
- A table is at most 2 MiB (provisional until the X4 gate) and PPHP version 1 only: the older `QIHP`
  magic is refused. The table check follows the contract: block count, exceptions offset and count in
  the 24 byte header, an ascending index inside the patterns, and an exceptions walk that ends exactly
  at the end of the file, so a truncated or padded table is refused. It is `paperpaw_hyphen::check_table`, the one function the kernel's
  reader shares with this tool (behind that crate's `kernel` feature).
- At run time the registry key is the tag alone. A book's tag is lowercased (`_` read as `-`) and looked
  up whole, then with its last subtag removed repeatedly (`de-ch-1996`, `de-ch`, `de`); there is no
  fallback to a longer tag or to another language (`no` finds only what a plugin registers as `no`).
  Where several plugins provide a tag the greatest plugin id wins (byte-wise, lowercase), with a
  warning naming `<plugin>:<tag>`. The base `hyphenation` provides `en` with no special rank; every
  `hyphenation-<lang>` id sorts after it. The packages here provide each tag once (a test).

The pattern sources, catalogs and `how/` texts live in `plugins/<id>/` (`patterns/`, `i18n/`,
`how/`); the interface-6 code crates are gone (#228), so no `Cargo.toml`, `build.rs` or `src/` is
left there (a test checks it). The builder copies the catalogs and texts unchanged and compiles the
table from the sources (`paperpaw_hyphen::compile`). The word tests of the old crates (known words
break where the dictionaries break them, minimums, case, breaks inside the word) run on the built
tables in `src/words_tests.rs`. Packages are at version 2.0.0, newer than the code plugins they
replaced (1.0.3; the English one 1.1.1), and are in `tools/plugin-files/data.toml`.
