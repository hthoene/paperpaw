# Updating from Beta 1, 2 and 3 to Beta 4, and back (#229, host side)

What a device on `0.6.0-beta.1`, `.2` or `.3` keeps when it gets this tree's
build (Beta 4, plugin interface 7), and what it keeps when it goes back. Host
checks only: no device, flash, signing or network. Run them all with

```sh
tools/qualify-update-paths.sh          # about 2 minutes warm, a few more cold
python3 tools/qualify-release-origins.py --download --cache DIR --out FILE.json \
  --sign-tool target/release/paperpaw-sign     # the released images (needs the network)
cargo test -p paperpaw-x4 --test update_path   # install, trial, kept, failed trial
```

The device rows (flash, update, rollback on hardware) are the plan in
[tools/pps/update-rollback/README.md](../../tools/pps/update-rollback/README.md).

## The ways

| Way | From Beta 1/2/3 | Where it is decided / tested |
| --- | --- | --- |
| In-app update ("Search online") | Not offered: the update server gives an interface-7 release only to a device that sends `abi_major` >= 7; Beta 1 to 3 send none (`git grep abi_major v0.6.0-beta.3` finds nothing). Decision 179: the way is flashing. | `paperpaw.org/src/updates.rs:1029` (`of_interface_7 && abi_major.is_none_or(<7)`), its tests; Beta 3 side: the origin's own installer on a device (open) |
| Packed update `-update-packed.bin` (#304) | Offered to `abi_major` >= 7 only; the plain `-update.bin` to every other device | `UpdateFile::offered_to`, same file |
| Flash (web flasher, `tools/flash-x4.py`) | The supported way; writes the table with 7.9 MB app slots | device (U1) |
| Signed `-update.bin` from the card | The current installer takes it on the model (trial, kept, old slot kept, books kept); Beta 3's own installer takes it on the host (below); the device run is U2 | `update_path.rs`: `a_signed_card_update_installs_and_is_kept_without_network` |
| OTA slots and rollback | All released origins: OTA data `0xd000`, app slots `0x10000` and `0x800000`, each `0x7f0000` B; the current code parses and transitions them (kept, successful trial, failed trial, torn entry write) | `qualify-release-origins.py` (real assets), `update_path.rs` (`an_update_that_fails_its_trial_is_undone`, `a_download_cut_short_is_not_installed`, `on_crosspoints_small_layout_…`, `an_update_from_beta_3_leaves_english_hyphenation_on_the_card`) |

## Forward: Beta N card, this build

| Check | Beta 1 | Beta 2 | Beta 3 |
| --- | --- | --- | --- |
| Every versioned format fixture of the tag reads as before (`tools/qualify-card-formats.py`: 87 / 87 / 93 fixtures; 81 / 81 / 87 byte-identical in this tree, the other 6 explained: a Home fixture edited to the store's tool, the widgets fixtures removed with the interface-6 pack, test fonts) | pass | pass | pass |
| Start on the card: `paperpaw.ini`, scoped settings, last book, old `progress.ini`; settings save and restart keep them; books byte-identical (`beta_upgrade.rs`) | pass (fixtures identical to Beta 2/3) | pass | pass |
| The old `progress.ini` is read as a position by the current writer; the file is not changed | pass | pass | pass |
| Plugin of interface 6 (Beta 3's real `hyphenation-it` file) and of interface 5: refused with the reason (`OlderInterface{6}` / `{5}`), file and plugin folder kept (`interface_five_and_six_files_are_refused_with_a_reason_and_kept`; the pack replaces Beta 3's hyphenation plugins, `code_plugin_migration_tests.rs`, with a power cut at every write) | pass | pass | pass |
| The reader's old files move, never copy: `progress.ini` to `.paperpaw/positions`, `annotations.txt` to `.paperpaw/annotations` (a shared file wins), `old_reader_tests.rs` | pass (all three readers read the shared position first and keep `progress.ini` as the fallback) | pass | pass |
| Passwords: veiled `secrets.txt` lines kept; passwords in `kosync/account.ini` and `opds/catalogs.txt` move into the passwords and keys (#81): those plugins' tests read the beta fixtures (`native_store.rs`, `native_catalogs.rs`). `stats/webdav.ini` moves in `native_webdav_net.rs`: `plugins/stats/tests/webdav_migration.rs` runs the real host on each tag's own fixture (#323): kept at the first start with the file untouched, the file gone at the second, nothing moved after | pass | pass | pass |
| Released images (X4 and Pro: full, install, update; checksum, chip/project, signature against today's keys, ESP checksum/hash, table, OTA transitions) | pass (#673) | pass | pass |

**Beta 3's own installer on the Beta 4 file** (#402): CI job `update-path`
(`tools/ci/update-path.sh`) makes a worktree of the release before's tag and
runs *its* `tests/update_path.rs` (its kernel, update plugin and
`paperpaw_x4::updates`) with the release before's update as the flashed image
and this run's signed install image as the update, the run's key built in.
Beta 3's installer downloads it online through the update plugin, writes the
other slot, starts it on trial and keeps it, and refuses the damaged, cut-short,
too-large and failing-trial files. Run by hand on the X4's real image of the
tree (restamped 0.6.0-beta.4; the tree's version is still beta.3): 9 / 9 pass.

Not shown by the host: real card timing, screens' wording, store plugins with code on a card, the Pro.

## Rollback: Beta N's code on a card Beta 4 has used

`tools/rollback/older_beta_reads_beta4_card.rs` is run in a worktree of each tag
(`tools/qualify-update-paths.sh`) on the folder `beta_upgrade.rs` writes as
Beta 4 leaves it: Beta 4's `paperpaw.ini` (with a section of a plugin the old
build does not have), the book, the beta fixtures, a position with its
conversion and book stamp, annotations in version 3, passwords veiled and
sealed (`s1:`), a format-5 plugin in `plugins/clock`.

| Check (Beta 1 / 2 / 3 alike: **pass, nothing lost**) | Result |
| --- | --- |
| Kernel builds and starts, no panic | ok |
| Beta 4's setting is read; the old build's settings save is written and keeps the unknown plugin section | ok |
| Position written by Beta 4 is read (place, percent) | ok |
| Every file of the card other than `paperpaw.ini` is byte-identical after the start and the saves | ok |
| Annotations version 3: not read (`InvalidData`) and the old build's add is refused, the file is not written over | **degrades** |
| Plugin of interface 7 (`formats = 5`): not listed, warning "made for a later PaperPaw (formats 5; this one reads 4)", folder and code file untouched | **degrades** |
| Passwords: all lines survive decode and encode byte for byte; a line sealed by Beta 4 is not decoded | **degrades** |

What the user sees after a rollback (degrades, nothing lost; all back after
updating to Beta 4 again):

1. **Highlights and bookmarks** in a book whose annotation file Beta 4 wrote
   (version 3 is written at the first annotation *edit* in Beta 4; only reading
   moves the old file unchanged) are not shown, and Beta 1 to 3 cannot add to
   it. The file stays. (The reader plugin's wording on that error is a device
   observation, R1.)
2. **Plugins of interface 7** do not appear; a warning names them. Their
   folders stay.
3. **Passwords sealed by Beta 4** (on a device whose `core.secrets` seals, the
   first save writes the whole list sealed) are not usable in Beta N: the Wi-Fi
   network is listed (its name is not sealed), its password must be entered
   again, and so for mail and other accounts. Owner decision, not a bug: an
   all-sealed file is what #81 is for; if rollback must keep Wi-Fi working, the
   old veiled lines would have to stay beside the sealed ones.
4. Nothing of settings, books, positions, scoped settings or last book is lost.

## Fixed on the way

- `crates/paperpaw-x4/tests/update_path.rs`: the signed-SD-update test of the
  earlier #229 work drove an update screen the test device no longer has (the
  update plugin is interface 7); it now installs through the same `Updates`
  path the firmware's page starts (`Device::install_card_file`).
- `crates/paperpaw-core/tests/beta_upgrade.rs`: did not compile on the current
  API (`Ask`, `driver`, `code_to_load`'s loader, interface 7); rewritten, with
  Beta 3's real plugin file and the rollback card.
- `tools/qualify-release-origins.py`: Beta 3 is published and now a default origin.
