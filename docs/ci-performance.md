# CI setup and timing

Every existing check still runs. Setup reuses tools already present on
GitHub's Ubuntu runner instead of reinstalling duplicate apt packages.
Missing CMake, Ninja or the release's named clang still goes through apt;
their versions are printed and the firmware builds remain the checks of
their suitability. Each apt call has retries, a timeout per request and a ceiling per
attempt (three attempts; a process that ignores the stop signal is killed
10 s later): at most about 17 minutes if every attempt stalls, instead of
the job's whole timeout. The `.deb` files are kept between runs
(`~/apt-debs`, about 150 MB, saved by pushes to `main` and `abi7/integration`
only, one entry for each, never by a manual workflow run), and a kept file is handed to apt only if its SHA-256 is the one the
authenticated package list (`apt-cache show`) names; apt itself trusts a
file already in its archive folder by its size alone. A stalled Ubuntu
mirror once held a firmware job for 30–49 minutes (#876). `CLANG_PATH`, Ninja and the firmware compiler settings
are unchanged.

Where the mirror is too slow to install `clang-18` (two attempts of 60 s for
the package lists and 120 s for the install, 8 min 6 s at most (plus local processing); the
partial files are kept between the attempts), the firmware jobs of the source
checks (`clang-fallback: "true"` of the `xteink-tools` action, `ci.yml` only)
go on with the newest `clang-NN` of at least version 18 that the runner image
brings, and say so as a warning of the job. mbedTLS is then built by that
compiler, not by `clang-18`: its bytes may differ slightly, so a size check
that fails by a few bytes only after such a warning is a rerun, not a finding.
Releases and store packages (`release.yml`, `store-plugin.yml`) never fall back:
they keep to the named compiler or fail. A pull request restores the `.deb`
cache of its base branch only; no entry exists before a push to `main` or
`abi7/integration` has saved one.

`tools/ci/espflash.py` first checks the actual cached executable's exact
version. A cache miss downloads the official static Linux x86-64 archive,
verifies the SHA-256 pinned in the script, extracts only `espflash`, and
checks its version before adding it to PATH. This needs no `libudev-dev`
or source compilation. It is shared by source CI and release setup. The
tool version remains in `shells/xteink/versions.env`; changing it also
requires reviewing the matching upstream archive and updating the pin.
Unsupported versions/platforms, changed archives and failed version checks
stop setup. There is no unverified download/source-install fallback.

The archive pin was checked against the asset digest of the official
[espflash 4.6.0 release](https://github.com/esp-rs/espflash/releases/tag/v4.6.0).
Its `espflash-x86_64-unknown-linux-musl.zip` digest is
`d515ee13ae44ba913c31a2ca58dc5fd72d3362994ee1d7487a16b289dfbb6e4a`.
Reusing the installed tool follows the existing trusted-branch Rust cache;
no credentials, signing keys or toolchain target libraries are added to it.

Run the offline setup tests with:

```sh
python3 -m unittest discover -s tools -p test_ci_tools.py
bash -n tools/ci/build-prerequisites.sh
```

They cover cache hits/misses, stale versions, tampering, unexpected archive
contents, wrong executable versions, preinstalled build tools, installation
of only missing tools and a package-manager failure. They do not flash a
device or need network access.

Measure job/step timestamps and setup logs for the same source with a cold
and warm cache. Record the runner image, exact tools, cache keys/hits and
queue time separately. The four firmware jobs must still pass both Clippy
variants, release builds, install-image limits, native sample fingerprints
and store plugin checks; the Pro reproducibility checks also remain.
Before adopting a new packaging tool, compare its `save-image` output with
the previous tool on the **same ELF and partition table**, byte for byte.
The independent reproducibility workflow must keep fresh build directories;
an artifact cache must not turn the two independent builds into one.

Useful primary references:

- [GitHub's runner software inventory](https://github.com/actions/runner-images/blob/main/images/ubuntu/Ubuntu2404-Readme.md)
- [Rust cache contents, keys and cleanup](https://github.com/Swatinem/rust-cache#cache-details)
- [GitHub cache scoping and security](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)
- [Cargo install and its temporary build directory](https://doc.rust-lang.org/cargo/commands/cargo-install.html)

Keep version, toolchain, target, flags and ABI separation in any future
cache work. Measure restore/save overhead and eviction before caching more
compiled data. A restored artifact never replaces executing the tests,
Clippy, security checks or reproducibility comparisons.

## Fewer runs

GitHub Actions minutes are limited, so a run is only started when its
result is wanted:

- A newer push to the same pull request or branch cancels the run before it
  (`concurrency` in `ci.yml`; `codeql.yml` and `changelog.yml` for pull
  requests). That includes `integration` and `abi7/integration`, which get one
  push per merge: the newest commit's run is the one that counts, and merge
  tools check the checks of the exact head SHA. `main` always finishes a full
  run per commit. A red push run on an integration branch is triaged by QA.
- Changes only to `docs/`, `CHANGELOG.md`, `changelog.d/`, `README.md`,
  `CONTRIBUTING.md`, `RELEASING.md`, `AGENTS.md` or `.claude/` skip full CI.
  A separate lightweight changelog check still runs for every pull request.
- A draft pull request skips full CI. Mark it ready for review to run
  them (`ready_for_review` starts a run).
- A pull request runs the emulator on Linux only; Windows and macOS run on
  `main` and `integration`. The fuzz job runs on pushes to `main` and
  `integration`, not on pull requests.
- CodeQL scans a pull request only when it changes `.github/`, JavaScript or
  TypeScript; it scans `main` on every push and every Monday.

## Shorter runs, and the cache quota

- **"Format, lint, test"** is the required check and is an aggregate job
  (`check` in `ci.yml`): it runs after the two parts `lint` (fmt, clippy,
  docs, the Bluetooth host tests) and `test` (the workspace tests, the
  line-break conformance test, the no-default-features tests) and passes only
  when both of them succeeded. A failed, skipped or cancelled part fails it.
  The parts run side by side and share the one rust-cache entry
  (`shared-key: check`, saved by the test job alone), so the lint (about 4
  minutes) is off the path of the tests (about 16).
- **The repository's caches are 10 GB.** When they are full GitHub evicts the
  least recently used entry, and with the 1 GB firmware entries that included
  the ones that save the most: without the espup toolchain every S3 job spends
  5.5 minutes installing it, and the `.deb` cache saves the apt step. After a
  push, `prune-caches` (`tools/ci/prune_caches.py`) deletes all but the newest
  `v0-rust-*` entry of each prefix and branch; other caches are never touched.
  A failing prune job changes nothing and is no required check. Try it with
  `python3 -I tools/ci/prune_caches.py <owner/repo> --dry-run`.
- **`tools/train-smoke.sh`** runs the checks a merge train went red on
  (clippy of the workspace, rustdoc with `-D warnings`, the wasm lint, `cargo
  deny`, the tag test, the stage codec, the workspace tests, and the S3
  frame-budget scan where `tools/check-s3-budget.sh` exists) before its pull
  request is opened.

## Fewer workflow runs

- **Billing:** `paperpaw` is public, so its standard hosted runners (every
  `runs-on` here is `ubuntu-latest`, `windows-latest` or `macos-*`, no larger
  runner, no self-hosted one) use no included minutes; the private repositories
  (`paperpaw.org`: one scheduled workflow plus a few pull request runs a month,
  `paperpaw-notes`: none) use almost none.
- **Drafts:** the `Changelog` workflow skips its job for a draft pull request
  (it runs on `ready_for_review` and on every push after that); the `CI`
  jobs already skip drafts. A skipped run still appears in the list, it costs
  no minutes.
- **Already so:** a push to a pull request cancels the run before it (`CI`,
  `Changelog`, `CodeQL`); `push` triggers only on `main`, `integration` and
  `abi7/integration` (a merge train is one push); CodeQL and the reproducible
  build run on `main`, tags and weekly only.
