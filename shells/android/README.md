# PaperPaw for Android

A native activity in Rust; its only Java code is a small listener for the
system's speech output (reading aloud). The app runs on Android
e-readers (Boox and others) and on any Android device with an arm64 CPU
and Android 8 or later. It is tested on a Boox Go 7 (Android 13). It is a
beta (0.6.0-beta.2), for testers: expect rough edges, and keep a copy of
your books.

## Building

Requirements:

- the Android SDK with build tools, a platform and the NDK (e.g. installed
  by Android Studio);
- a JDK (Android Studio includes one) and Python 3;
- `rustup target add aarch64-linux-android` and `cargo install cargo-ndk`.

Then build and install:

```bash
shells/android/build-apk.sh
adb install -r target/android/paperpaw.apk
```

The script finds the SDK through `ANDROID_HOME` and the usual install
locations, builds `libpaperpaw.so` with `cargo-ndk`, and packages and signs
the APK with the SDK's build tools. It does not use Gradle. Without other
settings, it signs with a debug key made on this computer. The top of
`build-apk.sh` names the variables for the release key. `--debug` builds
without optimizations. CI builds the same APK for every push to `main`
and every pull request. The run's artifact `paperpaw-android-apk` has it.

To install on an e-reader, enable USB debugging once: open *Settings →
About*, tap *Build number* seven times, then enable *Developer options →
USB debugging*.

## Using it

- **Plugins:** the plugins the X4s take from the store (the statistics,
  widgets, Calibre, KOReader sync and others) are built into the app,
  but switched off until you switch one on, in the plugin manager or the
  store. One you used before, whose data is there, stays on.

- **Books:** choose *Allow access to your books* on the home screen. On
  Android 11 and later, allow *All files access*. On older versions,
  allow the storage permission. PaperPaw restarts and shows the shared
  `Books` folder, where Boox and most e-readers keep books. Any other folder can be chosen under
  *Settings → Library → Library folder*. Until access is allowed, the
  library is `Android/data/org.paperpaw.app/files/books` with the
  sample books. `paperpaw.ini` and all caches stay in
  `Android/data/org.paperpaw.app/files`; passwords and keys
  in the app's private folder, which no other app reads.
- **Plugins without code:** copy plugin folders into `PaperPaw/plugins` on
  the device's storage (created once access is allowed) and restart.
- **PDF:** PDF books open through Android's own renderer. They have no
  search, no reading aloud and no table of contents yet.
- **Dictionaries and fonts:** put StarDict dictionaries into
  `PaperPaw/dictionaries` and fonts into `PaperPaw/fonts`. PaperPaw also
  finds KOReader's, in `koreader/data/dict` and `koreader/fonts`.
- **SD cards** show up as *SD card*.
- **Reading aloud** uses the system's speech output.
- **Touch:** tap the left or right side to turn pages, the middle for the
  menu, or swipe. Long-press for more options.
- **Buttons:** page keys and volume keys turn pages; Back goes back.
- **Language:** the system language, unless one is chosen in the settings.
- **Frontlight (Onyx Boox):** brightness and, where the device has warm
  and cold light, its warmth, in the reader's menu and under
  *Settings → Display*. They follow the device, so changes made with the
  system's own controls show up too.
- **Colour:** pictures and covers in colour on colour e-ink (Boox models
  named *Color* or *C*) and on phones and tablets, in grays elsewhere.
- **E-ink (Onyx Boox):** PaperPaw chooses the waveform for every frame:
  a flashing full refresh every few pages (*Settings → Display*) and on
  new screens, clean partial refreshes for page turns and fast ones for
  menus. Boox's own per-app refresh setting applies again when PaperPaw is
  in the background.
