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

## In-process kit

`plugins/test-support/acceptance.rs` (with `hosted.rs`) runs the same comparison without the
loader: a linked native plugin is installed on a real kernel with `Kernel::register_linked`,
one of its screens is opened, physical keys are pressed and the frame is compared with the
unchanged golden, pixel for pixel (`Session::start`, `open`, `keys`, `frame().matches_golden`).
A porter includes both files with `#[path]` and adds `png` as a dev-dependency; the battery
screen is the pilot (`plugins/battery/tests/hosted_acceptance.rs`). A step the host cannot do
is an `Err` that names it (`Session::dropped()` lists the callbacks the host dropped, with the
status); a case that cannot pass yet stays `#[ignore = "BLOCKED ..."]` with that reason. What a
screen draws with is given to the kernel with `Setup::with_kernel` (the default font packs;
a catalog plugin, until the host loads a native plugin's `i18n/*.ini`). Battery passes in en and
de. It does not replace the loader run above (todo 90), only what the kernel's stack and
frame do for a native screen.
