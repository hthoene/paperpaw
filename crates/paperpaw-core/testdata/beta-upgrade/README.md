# Beta upgrade data fixtures (#229)

These bytes are copied from the tagged source fixtures, not from a user card:

- `v0.6.0-beta.1`: `6e43f7008a0151294b1a62d91c9b2627ec6fad1d`
- `v0.6.0-beta.2`: `40a290f5563e7747afe83a68fb8a6ed9721ad3db`

Both tags contain exactly the same bytes for every fixture below. Keep this
frozen copy when current format fixtures are changed. Verify with
`git show <tag>:<source-path> | sha256sum`. The serialized examples are real
released-source fixtures, not an end-to-end captured device profile.

| Copy | Source path in both tags | SHA-256 |
| --- | --- | --- |
| `paperpaw.ini` | `crates/paperpaw-core/testdata/paperpaw.ini.v1` | `db731292ec9a009150b119e650449c24c062eec885185cb68006160adbb756e0` |
| `settings.ini` | `crates/paperpaw-core/testdata/book-settings.ini.v1` | `d4b96ee3611461013e5764b2b2f9b76c8c84af9788a6a0c364b50defe1686cbd` |
| `reader-state.ini` | `plugins/reader/testdata/state.ini.v1` | `4de9971836c56ab6776a870aed4c86bd9ea69237ba909be6ab1f026e1123fc18` |
| `progress.ini` | `plugins/reader/testdata/progress.ini.v1` | `95668b3ae98a1a8301d2a1268b3b5b00e40547570590b344a1b672fa4c5b2077` |

The host tests cover current kernel startup, settings write/restart, position
load/save and preservation of unrelated files. The sample book bytes come
from `samples/library/books/alice-in-wonderland.txt`, also byte-identical
in both tags and the test baseline. No book parser or device updater runs.
The plugin header cases are explicitly synthetic compatibility inputs;
they are not signed release plugin files or evidence of executing old code.

Both released betas use native interface 6. Interface 5 is refused; interface
6 remains eligible within a matching compiler/build and supported API/family
revisions. A future bump needs these tests and the device matrix updated.
These tests do not qualify automatic legacy-position-path relocation,
partition/OTA handling, rollback on hardware or a future interface 7.
