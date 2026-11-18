### Changed

- `tools/plugin-files/published.py` downloads the published files through GitHub's API with the job's token (`GH_TOKEN` or `GITHUB_TOKEN`) when there is one, so releases of a private fork or a test repository can read their own earlier assets; without a token it asks the public address as before.
