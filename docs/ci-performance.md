# CI setup and timing

Every existing check still runs. Setup reuses tools already present on
GitHub's Ubuntu runner instead of reinstalling duplicate apt packages.
Missing CMake, Ninja or the release's named clang still goes through apt;
their versions are printed and the firmware builds remain the checks of
their suitability. `CLANG_PATH`, Ninja and the firmware compiler settings
are unchanged.

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

- A newer push to the same branch or pull request cancels the run before it
  (`concurrency` in `ci.yml`, `codeql.yml`); `main` always finishes.
- Changes only to `docs/`, `CHANGELOG.md`, `README.md`, `CONTRIBUTING.md`,
  `AGENTS.md` or `.claude/` start no CI run.
- A draft pull request starts no CI job. Mark it ready for review to run
  them (`ready_for_review` starts a run).
- A pull request runs the emulator on Linux only; Windows and macOS run on
  `main` and `integration`. The fuzz job runs on pushes to `main` and
  `integration`, not on pull requests.
- CodeQL scans a pull request only when it changes `.github/`, JavaScript or
  TypeScript; it scans `main` on every push and every Monday.
