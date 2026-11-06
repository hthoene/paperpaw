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
`[hyphenation]`: a line `<BCP 47 tag> = <relative path>` for each language, at most 8
a plugin; two tags may name the same file (`nb` and `no`). The registry key is the tag,
and where several plugins provide it the greatest plugin id wins. A table is PPHP
version 1 and at most 2 MiB (provisional until the X4 gate). A bad line is skipped
with a warning at load; a missing, oversize or non-PPHP file makes the language
missing at activation. `check` refuses all of these on the host.

The pattern sources, catalogs and `how/` texts stay in `plugins/<id>/` until the
interface-6 code plugins are removed; the builder copies the catalogs and texts
unchanged and compiles the table with the same inputs as each plugin's `build.rs`
(tests compare them). Packages start at version 2.0.0, newer than the code plugins
they replace. They are not yet in `tools/plugin-files/data.toml`: the ids still belong
to the code plugins in `plugins.txt`, and the switch happens with the card-backed
hyphenation reader.
