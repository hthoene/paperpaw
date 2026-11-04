# paperpaw-dev in a plugin's own repository

`paperpaw-dev` runs PaperPaw's standard app on emulated devices (the
Xteink X4s and the Boox) with your plugin added, and checks it before you
publish it: every screen and widget drawn on every device, in light and
dark, in every language and look; text cut or overflowing, touch targets,
contrast; translations; flows; time and memory against the X4's budgets;
snapshots to compare with; and, for a data plugin, whether the device's
installer and the plugin library at paperpaw.org take it.

These files start it in a plugin's repository. Copy them, then replace
`my_plugin::MyPlugin` with your plugin's type and `my-plugin` with its id.

| File | What it is |
| --- | --- |
| `examples/paperpaw-dev.rs` | The harness: `paperpaw_dev::main` with your plugin added. `cargo run --example paperpaw-dev -- check --plugin my-plugin` |
| `tests/snapshots.rs` | The snapshot test: the cases of `tests/snapshots/cases.ini` compared with the pictures beside it. |
| `tests/snapshots/cases.ini` | The cases: screens and widgets, the device, settings, script steps. |
| `tests/flows/open-and-close.flow` | A flow: steps and what should be true after them. Write one for every action of the plugin. |
| `.github/workflows/paperpaw-dev.yml` | CI: the tests, then `check --plugin my-plugin --quick` and the flows varied at the edges, the reports kept as the run's artifact. |
| `.github/workflows/paperpaw-release.yml` | CI: on a tag `v1.0.0`, zips the plugin's folder, builds and signs a code plugin for each reader, and makes the GitHub release that paperpaw.org takes the plugin from. |

## The dependency

`paperpaw-dev` is not on crates.io: it brings the emulator and the whole
standard app. Name it by git, with the tag of the PaperPaw release your
plugin is made for, as a dev-dependency (examples and tests may use those,
the plugin itself never sees it):

```toml
[dependencies]
paperpaw-sdk = "0.6.0-beta.2"

[dev-dependencies]
paperpaw-dev = { git = "https://github.com/hthoene/paperpaw", tag = "v0.6.0-beta.2" }

[patch.crates-io]
paperpaw-sdk = { git = "https://github.com/hthoene/paperpaw", tag = "v0.6.0-beta.2" }
```

The patch matters. Your plugin builds on `paperpaw-sdk` from crates.io;
`paperpaw-dev` on the one in the repository at that tag. To Cargo those are
two crates, and a plugin made with one is no plugin to an app made with the
other (`the trait Plugin is not implemented for MyPlugin`). The patch makes
your `paperpaw-sdk` the repository's at the same tag, one crate for both;
patch the other PaperPaw crates your plugin uses from crates.io the same
way (`paperpaw-digest`, `paperpaw-json`, and so on). Keep the two tags the
same, and move them together when you move to a new release. Tags before
`v0.5.0-beta.1` have no `paperpaw-dev`.

`cargo update -p paperpaw-dev` is never needed for a tag; with
`branch = "main"` instead (for PaperPaw's newest SDK, which changes every
day) it fetches the newest.

## Running it

```sh
cargo run --example paperpaw-dev -- check --plugin my-plugin --quick
cargo run --example paperpaw-dev -- flows --plugin my-plugin --flow tests/flows
cargo run --example paperpaw-dev -- gallery --plugin my-plugin --profile xteink-x4
cargo run --example paperpaw-dev -- --help
```

The report is in `target/paperpaw-dev`: `report.html` (open it in a
browser), `report.json` for tools, and the pictures. Errors make the run
fail, warnings too with `--strict`.

## Snapshots

`cargo test --test snapshots` draws every case of
`tests/snapshots/cases.ini` and compares it with `<name>.png` beside it. A
few pixels may differ between computers (glyph edges); more fails, and
`target/paperpaw-dev/test/report.html` shows the expected picture, the
actual one and the pixels that differ.

After a change meant to alter what is drawn, write the pictures anew, look
at each before committing it:

```sh
PAPERPAW_BLESS=1 cargo test --test snapshots
```

(In PowerShell: `$env:PAPERPAW_BLESS = 1; cargo test --test snapshots`.)
`paperpaw-dev check` and `paperpaw-dev snapshots` look at the same folder
and cases (`--bless` writes them there too). Cases can also be written in
code, with `paperpaw_dev::snapshot::check` and `Case::screen`,
`Case::widget`, `Case::home`; keep their pictures in a folder of their own
then, since `check` takes `tests/snapshots` as the folder of `cases.ini`.

When CI fails on a snapshot, the run's artifact `paperpaw-dev` has the
pictures that differ.

## A data plugin

A plugin without code needs no harness: install the command once and give
it the folder.

```sh
cargo install --locked --git https://github.com/hthoene/paperpaw --tag v0.6.0-beta.2 paperpaw-dev
paperpaw-dev check path/to/my-theme --quick
paperpaw-dev package path/to/my-theme --release 0.6.0-beta.2
```

Or take `paperpaw-dev` ready built from a release. It is in
`paperpaw-emu-<version>-<computer>.zip`, beside the emulator.

`package` checks it as the device's installer and the plugin library take
it, and installs it on emulated devices as the store does: what either
would refuse is an error. In CI, the job above with these two steps in
place of the tests and the example.

## Publish by tagging

`paperpaw-release.yml` publishes a plugin from its own repository, with
no upload form. paperpaw.org lists public repositories with the topic
`paperpaw-plugin` (not archived, not forks), takes each one's newest
GitHub release, checks it as it checks an upload, keeps the files and
lists the plugin as "Community, unreviewed".

The easiest start is `paperpaw-dev new --github <id>` (with `--code`
for a plugin with code): it writes the whole repository, the workflow
with its switches set included. Otherwise:

1. Give the repository the topic `paperpaw-plugin`.
2. Copy `paperpaw-release.yml` to `.github/workflows/` and set its
   switches at the top: `CODE` (`"true"` for a plugin with code, `"false"`
   for a folder of data), the readers to build for (`X4`, `X4_PRO`), the
   plugin's folder `PLUGIN_DIR` and, for code, the binary `BIN` and the
   PaperPaw release's pins, as in `samples/hello-native`. The X4 Classic
   gets the X4 Pro's file under its own name (`X4_CLASSIC`).
3. For a plugin with code, make a key once with
   `paperpaw-sign key author.key`, keep the file safe and out of every
   repository, and paste its contents as the repository's secret
   `PAPERPAW_PLUGIN_KEY` (*Settings*, *Secrets and variables*,
   *Actions*). Data plugins need no key.
4. Raise `version` in `plugin.ini` (and in the code), run
   `paperpaw-dev check-publish` (or `paperpaw-sign check-publish .`) and
   fix what it says, commit, then:

```sh
git tag v1.0.0 && git push --tags
```

The tag must be `v` and `plugin.ini`'s version, or the job stops. It
runs `check-publish` before it builds and on the files it made, and
releases nothing where it fails. It makes a published release (not a draft, not a pre-release) with exactly
`<id>-<version>.zip`, the plugin's folder with its `store/`, and for code
one signed `<id>-<version>-<device>.ppn` per reader. The site picks it
up within the hour. What fails its checks is not offered; the warnings
are shown.

Some things to know:

- The files are kept on the site; readers never download from GitHub.
- A plugin with code needs its source: `store/store.ini` names it
  (`[source]`), or the site takes the repository at the release's tag.
  Commit `Cargo.lock`.
- An id belongs to whoever it first appeared with, on GitHub or through
  the upload form: by its key, or for a data plugin by its account or
  GitHub owner. A version with another's id or another key is not
  offered, and readers refuse an update signed by another key. The ids
  and names of PaperPaw's own plugins are reserved.
- To get it reviewed, name the repository in the store's form (*From
  GitHub*). After that, every new release waits for the moderators as an
  update and goes live once approved.
- Whoever installs it sees the author (the GitHub owner), the link to the
  repository and the key's fingerprint, and can report it.
