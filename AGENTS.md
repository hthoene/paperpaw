# For AI assistants

There are two kinds of work here; tell which one a task is.

- **Work on PaperPaw itself**: its kernel, SDK, shells and firmware, and
  its own plugins in `plugins/`. Follow [CONTRIBUTING.md](CONTRIBUTING.md):
  its architecture rules and the checks to run. Put a change at the
  layer where it belongs; when a plugin cannot do something well, extend
  the SDK (with its API file and CHANGELOG brought up to date) rather
  than working around it in the plugin. For SDK changes, follow
  [docs/sdk-design.md](docs/sdk-design.md).
- **A plugin for PaperPaw as it is**, written outside the project without
  changing PaperPaw (e.g. a community plugin in its own repository):
  follow the skill in
  [`.claude/skills/paperpaw-plugin-development`](.claude/skills/paperpaw-plugin-development/SKILL.md),
  which builds on the SDK as released. Its limits do not apply to work
  on PaperPaw itself.

Before a push, run `tools/ci-local.sh` (`full` for a change to the SDK,
the firmware or the tools): it is CI's host-side checks, and CI minutes
are limited. See CONTRIBUTING.md.

[docs/plugins.md](docs/plugins.md) explains how plugins are written, for
both.

Where a task falls into parts that do not depend on each other (a new
module, a plugin, documentation, tests beside an API that is settled),
let several agents work on them in parallel, each in its own worktree,
and bring their work together after; parts that change the same files
or wait for each other's API stay in one hand.
