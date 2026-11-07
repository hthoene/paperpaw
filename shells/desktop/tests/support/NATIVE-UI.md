# Native UI acceptance

`native_ui` uses the desktop's production `HostLoader`, simulated clock, panel
and key-script driver. PNG decoding is shared with `snapshots.rs`. Comparison
is exact, as in the Battery pilot; neither a missing reference nor
`PAPERPAW_BLESS` creates or changes a golden.

Current status: preparation only. H1 (screen commands), H3 (composition) and H4
(input) each have an explicit `BLOCKED` refusal in `require_host`. Replace a
refusal only when that production path and its routing evidence are checked.
H2 dialog services must additionally be available for dialog cases. The ignored
test is not pixel or activation evidence until these dependencies are resolved.
There are no fake frame, command, input or successful-callback implementations.

Prepare a disposable card using the same book/data/settings fixtures as the
original screen snapshot. Put a real Linux x86-64 interface-7 plugin folder
under `plugins/<id>/`, including its native library, plugin.ini and catalogs.
Create `.native-ui-test-card` in the card root. The loader installs trust on this
test card only. Do not point this at a user's library. The test rejects an
interface-6 artifact, missing golden, built-in fallback or failed admission.

Set these environment variables:

- `PAPERPAW_NATIVE_UI_LIBRARY`: disposable card root.
- `PAPERPAW_NATIVE_UI_ID`: installed plugin id.
- `PAPERPAW_NATIVE_UI_SCREEN`: full screen id to open.
- `PAPERPAW_NATIVE_UI_PROFILE`: snapshot profile, such as `xteink-x4`.
- `PAPERPAW_NATIVE_UI_SETTINGS`: complete deterministic configuration text;
  include language, `help.show=false`, and `reader.open-last=false` as appropriate.
- `PAPERPAW_NATIVE_UI_KEYS`: comma-separated physical key presses, for example
  `page-forward,confirm,back`. Actions, direct taps and waits are refused here.
- `PAPERPAW_NATIVE_UI_GOLDEN`: existing expected frame after the key sequence.
- `PAPERPAW_NATIVE_UI_EVIDENCE`: output directory for actual.png and diff.png.

Run one isolated device per test process:

```sh
cargo test -p paperpaw-emulator --test native_ui \
  loaded_native_plugin_matches_golden_after_keys -- --ignored --exact --nocapture
```

Until activation lands the command must fail, never pass on a blank frame.
Failure before pixel comparison is a host/admission blocker, not layout drift.
Run en/de and each existing scenario separately; retain original golden files.
This is desktop emulation, not evidence of device timing, heap or X4 activation.
