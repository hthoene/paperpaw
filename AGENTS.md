# For AI assistants

Tell which kind of work a task is.

- **Work on PaperPaw itself** (kernel, SDK, shells, firmware, its own
  plugins in `plugins/`): follow [CONTRIBUTING.md](CONTRIBUTING.md), its
  architecture rules and code rules. Put a change at the layer where it
  belongs; when a plugin cannot do something well, extend the SDK (API
  file and changelog fragment, see [docs/sdk-design.md](docs/sdk-design.md))
  instead of working around it in the plugin.
- **A plugin for PaperPaw as it is**, written outside the project: follow
  [`.claude/skills/paperpaw-plugin-development`](.claude/skills/paperpaw-plugin-development/SKILL.md).
  Its limits do not apply to work on PaperPaw itself.

[docs/plugins.md](docs/plugins.md) explains how plugins are written.

## How a change lands

1. Branch from `origin/abi7/integration`, or from the unlanded branch your
   work builds on (name it; both land together).
2. Build little: read and write the whole change first, then check once.
   Use the fastest check that catches your own mistake: the tests of the
   crates you touched, clippy and fmt on what changed, the one size or
   stack number the change can move (`tools/check-local.sh [crate ...]`
   does the first two). No full local suite.
3. Push the branch once, when it is complete, and hand it over with
   `merge_when_green`. No pull request, and nobody starts or watches CI.
4. The repository lands first: branches are merged at once, CI runs
   afterwards on the base, once for everything that landed, and a red base
   is repaired by one assignment. Only a branch that conflicts comes back
   to its author.

## Changelog

For every user-visible change add `changelog.d/<issue>-<slug>.md` as
[its README](changelog.d/README.md) describes. Do not edit `CHANGELOG.md`.
Without a visible effect, put a `No-changelog: <reason>` trailer on the head
commit. Documentation-only changes are exempt.
