# Battery tests on interface 7

Battery is a native plugin only; the interface-6 `Plugin` and its Kernel-based
tests are gone. What is tested, and where:

- `native_goldens.rs`: the events table (12 rows plus "no battery"), the exact
  bytes of `log.csv` and the shape of `plugins.csv` for a scripted sequence,
  rotation at 128 KiB for both logs, the terminated cut tail (the one
  intended deviation) and a cut at every byte of a tail. They run the real
  entry glue against the test-only host in `support/native_host.rs`.
- `native_recording.rs`: duplicate suspend, disabled recorder, torn tail,
  rotation, and that every host blob is released.
- `native_screens.rs`: an Env-only host refuses the Core-only data services.
  The 16 pixel goldens (`testdata/screens/*.png`, en and de) run in
  `crates/paperpaw-core/src/native_battery_pixel_tests.rs` on a real
  shared host, because only Core can supply the frame, fonts and input.
- `abi7_preparation.rs`, `linked_entry.rs`, and `build.rs`: declarations
  (settings, screen, action, event subscriptions) are compared with the
  reviewed golden `testdata/declarations.bin.v1` and with the static image
  the plugin ships.
- `src/log/hostile_tests.rs`: parsing damaged and hostile logs, bounded history,
  property-based round trips.

Host acceptance still pending (explicit, not faked): activation in the app
(Core publishes native plugins; the builder has no native entry yet, so
`paperpaw-app` does not register Battery), the desktop `native_ui` run
(`shells/desktop/tests/support/NATIVE-UI.md`: H1, H3, H4 refusals), and
everything on the X4.
