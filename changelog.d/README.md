# Changelog fragments

Each pull request with a change readers or plugin authors can see adds a
file here instead of editing `CHANGELOG.md`. Name it
`<ticket>-<slug>.md`, or `pr-<number>-<slug>.md` when there is no ticket;
for example, `226-changelog-fragments.md`. Use a distinct slug for each PR.

Write one or more of these headings, with at least one bullet each:

```markdown
### Fixed

- **A short description** (#226). Say what happened before and what
  happens now, in the same plain English as CHANGELOG.md.
```

The headings are `### Added`, `### Changed` and `### Fixed`. Several
sections and bullets are allowed. Indent continuation lines by two
spaces. Keep explanations useful to readers and plugin authors; put
implementation and test details in the PR description.

Run `python3 tools/changelog.py check`; the PR workflow runs it too and
rejects a malformed fragment. After committing,
`python3 tools/changelog.py check --base <the branch your PR targets>`
(for example `origin/abi7/integration`) is an optional local check that a
code change adds a fragment, even if it also changes an older one. For a
change with no visible effect, put `No-changelog: <reason>` in the final
trailer block of the head commit. An empty reason or a mention in the
message body does not count. Documentation-only PRs need neither. Do not
edit `CHANGELOG.md` in a PR: that file is the one place parallel PRs
conflict on, and GitHub then runs no CI on them.

At release preparation, follow [RELEASING.md](../RELEASING.md). Collection
includes existing Unreleased entries, groups fragments by section and
orders them by number then filename. It leaves published sections alone
and removes the consumed files. Do not collect during an ordinary PR.
