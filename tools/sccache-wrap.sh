#!/bin/bash
# A compiler wrapper (`RUSTC_WRAPPER`) that puts `sccache` in front of rustc,
# so that builds in other build folders (worktrees, agents) hit what this one
# compiled: the dependencies from crates.io, which are the same everywhere.
#
#   export RUSTC_WRAPPER=$PWD/tools/sccache-wrap.sh     # or a copy on PATH
#
# What it does besides calling `sccache`:
# - sccache puts every CARGO_* variable in its cache key, and CARGO_TARGET_DIR
#   (a build folder of its own for each worktree or agent) is one: without
#   dropping it no build hits another's. Nothing else is changed.
# - Builds for the firmware's chips (`*-none-elf`) go to rustc directly: their
#   link-time optimisation fails through sccache, without a message, and
#   `RUSTC_WRAPPER` in the environment overrides shells/xteink's own
#   `rustc-wrapper = ""`.
# - Without `sccache` it just runs rustc.
#
# Written for bash and `env` and not `sh`: dash drops the variables whose
# names are no shell names (CARGO_BIN_EXE_paperpaw-emu), which tests need.
command -v sccache > /dev/null || exec "$@"
for argument in "$@"; do
    case "$argument" in
        *-none-elf) exec "$@" ;;
    esac
done
exec env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET_DIR sccache "$@"
