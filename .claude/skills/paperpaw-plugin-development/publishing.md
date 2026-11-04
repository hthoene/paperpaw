# Publishing a plugin in the PaperPaw store

How to guide an author, step by step, from a finished plugin to its
listing in the plugin library on paperpaw.org, which the website and
every reader's store (*Plugin store*) read. It is written for an AI
assistant that helps the author. The site's own guide for authors,
<https://paperpaw.org/authors>, is the reference for the site's side
(its names of files, keys, fields and checks); this file follows it and
adds the PaperPaw side. `docs/plugins.md` ("Publishing") explains the
same for people; ADR 0023 (the store) and ADR 0024 (code plugins as
files) say why it works so.

Check names before relying on them: `paperpaw-sign --help`,
`samples/hello-native`, and the site's guide. Where they disagree, the
site's guide and the repository win; tell the user that this file looks
out of date.

## 0. Rules for the assistant

You MUST NOT:

- **Ask for, show, copy or move the secret key.** Never print the key
  file (`cat`, `type`, `Get-Content`, an editor), never paste it into the
  chat, a file in a repository, an issue, a pull request, a log, a form
  field or a command line. Refer to it only by its path, as `--key
  <path>`. If its contents appear anywhere they should not, stop and
  tell the user that the key is compromised (section 9).
- **Commit the key.** Before every commit, check that no `*.key` file
  and no 64-digit secret is staged (`git status`, `git diff --cached`).
- **Act for the user where only they can or should**: signing in to
  paperpaw.org (it takes the link in their mail; you cannot), sending a
  submission to the moderators, pushing a branch
  or tag, creating a release, setting a repository secret. Do these, or
  have the user do them, only after the user's explicit yes for that
  step, and show what will be sent first.
- **Invent anything the steps say to read.** Never make up a
  fingerprint, a key, a SHA-256, a field, a check code or an address:
  read fingerprints and keys with `paperpaw-sign plugin`, hashes with
  `sha256sum`, and take names from this file or the site's guide.
- **Tag a release that `check-publish` refuses.** Run it before every
  tag (section 6b) and fix every error it names; never tag past one.
- **Get around a check.** Never change an `abi` or `sha256` to fit a
  file, strip or forge a signature, edit a `.ppn` by hand, or put one
  reader's file in another reader's field. Fix the cause (section 8).
- **Sign with another key for an update.** The form refuses it; a key
  changes only through the site's contact address (section 9).
- **Publish what the user may not publish**: work under a licence that
  forbids it, someone else's plugin as theirs, secrets (passwords, API
  tokens) inside the plugin's files or code. The site's rules for
  authors (<https://paperpaw.org/authors#rules>) hold for every plugin.
- **Claim a result you have not seen**: that a check passed, that the
  plugin is published, that it is "Verified". Report what the site or
  the tool said, word for word.

Work through sections 1 to 7 in order, run every check yourself where
you can, and end with the checklist in section 11.

## 1. What gets published

| | Data plugin | Code plugin |
|---|---|---|
| What it is | a folder: `plugin.ini` and data files | the same folder, and its code built for each reader |
| Uploaded | the folder as a ZIP, with `store/store.ini` | the folder as a ZIP, with `store/store.ini`; one signed `.ppn` per reader (`x4`, `x4-pro`, `x4-classic`); or all of it in one store file (section 6) |
| Source | `source`, where it lives | also the exact source of the builds: a public repository and a tag or commit (`[source]`), or a source ZIP |
| Signed | no | every `.ppn`, all with the author's one Ed25519 key |
| Checked | the ZIP's contents and the entry | the same, every build, the key, the source; then a rebuild from the source |

Everyone sees on the website and on the readers whether a plugin has
code, who made it, its licence, its source. The site keeps every code
plugin's source and shows it file by file at
`/plugins/<id>/<version>/source`, rebuilds each build, and shows
**Verified: matches its source**, **Not rebuilt** or **Rebuilt, but does
not match its source**. The public version log (`/log`) lists every
version published or taken down.

## 2. The way in

There are two ways. Ask the user which they want.

1. **The upload form** at `https://paperpaw.org/submit` (section 6): the
   author signs in by a link sent by e-mail, uploads, reads the checks,
   completes the entry, looks at the preview and sends it to the
   moderators. They decide; the author gets a mail either way. The
   plugin is listed as reviewed.
2. **A tag on GitHub** (section 6b): the plugin's own public repository
   has the topic `paperpaw-plugin`, and a GitHub release holds its
   files. The site finds it, checks it like an upload and lists it as
   "Community, unreviewed": no moderator has read it. No sign-in, no
   form.

The `catalogue/` folder of the paperpaw.org repository holds PaperPaw's
own plugins only; it is not a way for others to publish.

## 3. Gather what the listing needs

Ask the user for each, and suggest values from the plugin where you can
(section 6 has the rules):

- **id**: lower-case letters, digits and hyphens, at most 40, not
  starting or ending with a hyphen. The same in `plugin.ini`, the
  folder's name, the code's manifest and the start of every id in the
  code (`clock.open`). It must be free: not a plugin of the site's
  own, not another author's, not one taken down (look at
  `https://paperpaw.org/api/v1/plugins.json`). It cannot change later.
- The **name** says what the plugin does. It is never only another
  product's, project's or service's name, which would suggest the
  plugin is theirs: "KOReader highlights", not "KOReader"; "Wikipedia
  lookup", not "Wikipedia". Naming the other product to say what the
  plugin works with is fine. Use no other's logo as the icon. The
  store's review reports such names.
- **name** (at most 60), **summary** (one sentence, at most 160),
  **description** (at most 2000): *How it works*, a few sentences on
  what the plugin does, where it is found and how it is used, shown on
  its page in the store on the device and the site. A plugin with code
  should give it, and the same text in `how/en.txt` (and a file per
  other language) named by its manifest (`Manifest::how`), which the
  plugin manager shows once it is installed; `paperpaw-dev` warns where
  that is missing or short.
- **group**: `reading`, `formats`, `library`, `sync`, `home`, `look`,
  `notes` (notes, highlights, journals), `learning` (vocabulary,
  languages, flash cards), `news` (news, feeds, articles, web reading),
  `tools` (calculators, timers, utilities), `games` or `system`.
- **tags**: up to five, lower-case letters, digits and hyphens, at most
  24 each.
- **licence**: an SPDX identifier (`MIT`, `Apache-2.0`,
  `GPL-3.0-or-later`, `CC0-1.0`, `OFL-1.1`).
- **source**: an `https://` address of its source or home.
- **sdk**: the PaperPaw SDK it is made for, such as `0.5`.
- **devices** (data plugins): those it runs on, of `android`, `x4`,
  `x4-pro`, `x4-classic`; left out, every device. A code plugin runs on
  the readers it has builds for. **A plugin does not have to be for every
  device**: ask the author which devices it is for (never assume all), and
  publish only for those. The store shows it, on the site and on each
  reader, only on the devices it names; a code plugin only on the readers
  it has a build for, never on Android. To limit a data plugin, write
  `devices = x4, x4-pro` in `[store]`; to limit a code plugin, give only
  those readers' `.ppn` files. The X3 is served as `x4`. Say the limit in
  the description ("for the X4 and X4 Pro"), as the page shows "only"
  beside it. `docs/plugins.md`, "Which devices your plugin is for", has
  the rest.
- **translations** of name and summary, and **screenshots** (up to four,
  of the plugin at work, with what each shows).
- The author's name is the account's.

## 4. Prepare the plugin's folder

The folder is named as the id. `store/` inside it is for the store only
and is taken out of what readers install.

```text
night-owl/                 a data plugin
  plugin.ini
  settings.ini
  i18n/de.ini
  store/store.ini          for the store only
  store/home.png           a screenshot

clock/                     a code plugin
  plugin.ini
  i18n/de.ini
  store/store.ini
  store/home.png
clock-1.0.0-x4.ppn         its builds, signed, beside the folder
clock-1.0.0-x4-pro.ppn
```

### plugin.ini

```ini
[plugin]
id = night-owl
name = Night owl
description = Dark headers and roomy rows for reading at night.
version = 1.0.0
formats = 1
```

`id`, `name` and `version` are needed. `version` is three numbers,
a pre-release after them if wanted (`1.2.0-beta.1`, which comes before
`1.2.0`, as SemVer orders them), and the code's manifest says the same; `description` is one sentence the
store takes as the summary when `store.ini` gives none.

### store/store.ini

PaperPaw's INI: `[section]` lines and `key = value` lines. A comment is
a line of its own starting with `#` or `;`; **never a comment after a
value** (`group = look ; mine` makes the group `look ; mine`, and a
section line with a comment after it is not read). A value is one line;
to keep spaces or write line breaks, put it in quotes and write `\n`.
Every key is optional; the form asks for what is missing.

```ini
[store]
summary = A large clock on the home screen, with the date below it.
description = "The clock shows the time in large figures.\n\nIt adds a widget for the home screen."
group = home
tags = clock, widget
licence = MIT
source = https://codeberg.org/your-name/clock
sdk = 0.5
# Data plugins only; left out, every device.
# devices = android, x4-pro

[source]
# Code plugins: the exact source of the builds.
repository = https://codeberg.org/your-name/clock
revision = v1.0.0
# Only when the crate is not at the repository's top:
# path = plugins/clock
# Only when the binary is not named as the crate's package:
# bin = clock

[screenshots]
home.png = The home screen with the clock widget, on an Xteink X4.

[i18n.de]
name = Uhr
summary = Eine große Uhr auf der Startseite, mit dem Datum darunter.
```

`samples/hello-native/plugin/store/store.ini` is one to fill in.

| Key | Rule |
|---|---|
| `summary` | one sentence, at most 160 characters; without it, `plugin.ini`'s `description` if it fits |
| `description` | at most 2000 characters |
| `group` | one of the seven groups |
| `tags` | up to five, separated by commas |
| `licence` | an SPDX identifier |
| `source` | an `https://` address |
| `sdk` | like `0.5` |
| `devices` | data plugins: `android`, `x4`, `x4-pro`, `x4-classic`, separated by commas |
| `[source] repository` | code plugins: a public repository, `https://` on `github.com`, `codeberg.org` or `gitlab.com` |
| `[source] revision` | the tag (`v1.0.0`) or the full commit (40 hexadecimal digits) built; never a branch |
| `[source] path` | the crate's folder, when it is not at the top |
| `[source] bin` | the binary to pack, when it is not named as the crate's package (letters, digits, `_`, `-`) |
| `[screenshots]` | `<file> = <what it shows>`, the file in `store/`, `.png`, `.jpg` or `.webp`, named in lower-case letters, digits and hyphens, at most 500 KB; the text at most 200 characters |
| `[i18n.<lang>]` | `name` and `summary`, both; `<lang>` two or three lower-case letters, not `en` |

Translations need no `[i18n.<lang>]` when the folder's
`i18n/<lang>.ini` has the messages `<id>.plugin.name` and
`<id>.plugin.description`; `store.ini` wins where it gives both.

### What the folder may hold

- Outside `store/`, only `.ini`, `.css`, `.ttf`, `.otf`, `.ttc`, `.ppfp`,
  `.txt`, `.md` and `.png` files; never a `code.ppn` (builds are
  uploaded apart).
- At most 64 files a reader installs, so 63 in a code plugin's folder.
- Paths of letters, digits, `.`, `_` and `-`, joined by `/`, no part
  starting with a dot, at most 100 bytes.
- No `.DS_Store`, `__MACOSX/`, `Thumbs.db`, `desktop.ini` or other
  hidden files (they are left out with a warning, and a warning keeps an
  update from going live by itself).
- `i18n/*.ini` parse as translation catalogs; `formats` is a number.

Try the plugin before publishing it: in the emulator, and a code plugin
on a reader if the user has one (`docs/plugins.md`, "Trying it on a
device").

## 5a. Make the ZIP

Zip the folder itself, as the site's guide does:

```sh
# Linux and macOS:
zip -r night-owl.zip night-owl -x '*/.*'
unzip -l night-owl.zip

# Windows 10 and 11, in PowerShell or the command prompt:
tar -a -c -f night-owl.zip night-owl
tar -t -f night-owl.zip
```

The list must show `night-owl/plugin.ini` (or `plugin.ini` at the top)
and the files of `store/` under the same folder. Never PowerShell's
`Compress-Archive`: Windows PowerShell 5.1 writes `\` into names, which
the check `upload` refuses.

A code plugin from the sample: `build.sh` (and the CI job) writes
`dist/<id>-<version>-upload.zip`, the folder with `store/`, for the
form, and `dist/<id>-<version>.zip`, without `store/`: what readers
install.

## 5b. A code plugin: key, build, sign, source

### The repository

Start from `samples/hello-native` in the PaperPaw repository; copy its
files into the plugin's own repository:

| File | What it is for |
|---|---|
| `Cargo.toml`, `src/main.rs` | the crate that makes the file: `paperpaw_sdk::plugin!(...)` around the plugin |
| `.cargo/config.toml` | how a plugin is linked, for both chips, and the aliases `x4` and `x4pro`; keep it as it is |
| `rust-toolchain.toml` | the X4's compiler of the PaperPaw release built for |
| `Cargo.lock` | committed: without it, the source cannot be rebuilt |
| `plugin/` | the plugin's folder, with `plugin/store/store.ini` |
| `build.sh` | builds both readers, packs, signs, zips, writes `dist/` |
| `.github/workflows/build.yml` | runs `build.sh` on every tag `v*` and attaches the files to the tag's release |
| `.gitignore` | keeps `target/`, `dist/` and `*.key` out |

Set the binary's name in the workflow's `PLUGIN` (and `[source] bin`
when it differs from the package's name). The plugin's own code stays a
library crate, tested on a computer and tried in the emulator.

### The key (once per author)

```sh
# paperpaw-sign of the release built for (its tag, here v0.6.0-beta.2):
cargo install --locked --git https://github.com/hthoene/paperpaw --tag v0.6.0-beta.2 paperpaw-sign
mkdir -p ~/.paperpaw
paperpaw-sign key ~/.paperpaw/author.key
```

Expected output (other digits):

```text
public key:  3f1c...64 hexadecimal digits...9a0b
fingerprint: 3F1C 22A0 9B7D 0E15
```

- The key file is one line of 64 hexadecimal digits: the secret. Keep
  it outside every repository, with a copy kept apart (a password
  manager, an encrypted drive). Whoever has it can sign in the author's
  name; without it, no update installs without a warning.
- The public key (64 digits) is what the store shows as the author's
  and readers remember at the first install. The form reads it from the
  builds.
- `paperpaw-sign key` never overwrites a file (`error: <file> exists: a
  key is never overwritten`). An author with a key uses it for every
  plugin and every update.
- The signing page `https://paperpaw.org/authors/sign` can make a key
  too (saved as `paperpaw-author.key`); it is the same kind of file.
- For CI, the file's contents become the repository's secret
  `PAPERPAW_PLUGIN_KEY`. The user does it in the repository's *Settings
  > Secrets and variables > Actions*, or, with their yes, you run (it
  reads the file without showing it):

  ```sh
  gh secret set PAPERPAW_PLUGIN_KEY < ~/.paperpaw/author.key
  ```

### Build for a release

Until PaperPaw 1.0 a `.ppn` runs only on firmware built by the same
compiler for the same chip from the same SDK. Its **fingerprint** (16
hexadecimal digits in the file's header) says which; a reader loads only
files with its own. *About* on the reader shows its fingerprint and
compiler; each PaperPaw release lists the fingerprints in its files
`paperpaw-<reader>-<version>-abi.txt`.

- Name the SDK of the release in `Cargo.toml` (`paperpaw-sdk = "0.6.0-beta.2"`),
  never a path or a development version.
- Build for the readers without the SDK's `outline` feature: it is in the
  fingerprint, and the readers' firmware has none. Give it to the
  emulator's build only, in a target section
  (`[target.'cfg(not(target_os = "none"))'.dependencies]`, see
  `docs/plugins.md`, "The crate"). With `outline` the X4 build fails in
  `once_cell` (``no method named `compare_exchange` ``); from SDK
  0.5.0-beta.2 on it stops with a message that says so.
- Pin its compilers: `rust-toolchain.toml`'s `channel` as the release's
  `shells/xteink/rust-toolchain.toml`; the workflow's `RUST_X4`,
  `RUST_X4_PRO` and `ESP_GCC` as the release's `shells/xteink/versions.env`
  (`ESP_RUST_VERSION`, `ESP_GCC_VERSION`); `PAPERPAW` its version.
- For the X4 Pro, install the release's core libraries
  (`paperpaw-xtensa-std-<version>.tar.gz`, with its `.sig`) into the
  `esp` toolchain: PaperPaw's `tools/xtensa-std/install.sh`, or as its
  README says; the sample's CI job does it. The X4's come from rustup.
- One file for each reader. The X4 Classic has the X4 Pro's chip but a
  firmware of its own: give it a build only with the fingerprint its
  *About* shows.
- After every release until 1.0, build again with its SDK and compilers
  and publish a new version (section 7).

**From PaperPaw 0.5.0-beta.1 on** (ADR 0026), this changes:

- The fingerprint is the **plugin interface** in its top 16 bits, and
  below them the chip and the layouts of the types that cross. A file
  runs on every PaperPaw of its interface, whichever release made it, so
  build again only when a release raises the interface (its release
  notes say so; `paperpaw-sign show` prints a file's `interface`).
- The file also names the **compiler** that built it, taken from the ELF
  by `paperpaw-sign pack`. A reader loads it only where the same
  compiler built the firmware; the release's are still those of
  `shells/xteink/rust-toolchain.toml` and `versions.env`.
- The SDK has no build script any more, so nothing brings the linker
  script: copy `paperpaw-plugin.x` of the release (as
  `samples/hello-native` keeps it) to the top of the plugin's project,
  beside `.cargo/config.toml`, whose rustflags link with it
  (`-C link-arg=-Tpaperpaw-plugin.x`, with `-Wl,` for the X4 Pro). Use the
  release's copy unchanged: it keeps the compiler's names (`.comment`),
  which `pack` needs, and a build linked with an older copy is refused
  with "no .comment names the compiler".
- Files of the older format (made by a `paperpaw-sign` before 0.5.0) are
  refused as "of an older format, to be built anew".

### So the rebuild matches

The site rebuilds each build from the kept source in a container with
the toolchain the fingerprint names, with the sample's aliases (`cargo
x4`, `cargo +esp x4pro`, `--locked`), packs it unsigned and compares it
byte for byte with the uploaded file without its signature. Paths end
up in a build (in its panic messages), so builds and rebuild write them
as the same everywhere: `build.sh` adds to the flags of
`.cargo/config.toml`

| Path | Written as |
|---|---|
| the source's top (the repository, `git rev-parse --show-toplevel`) | `/plugin` |
| Cargo's home (`$CARGO_HOME`, else `~/.cargo`) | `/cargo` |

with `--remap-path-prefix`, given through `cargo --config` so that the
flags that link the plugin stay. The core libraries come built, the
same for everyone (rustup's for the X4, the release's for the X4 Pro),
and the rebuild installs the same. So:

- **Build with `build.sh`** (or the sample's CI job, which runs it), in
  any folder, on Linux or macOS. On Windows, paths inside the source
  keep their `\`: such a build matches only builds on Windows, and the
  rebuild shows "Rebuilt, but does not match its source"; use WSL or
  the CI job there.
- **Commit `Cargo.lock`**; `build.sh` builds with `--locked`.
- **Keep the pinned toolchains**, never `stable` or `nightly`.
- **Never set `RUSTFLAGS`** and never build with plain `cargo build`
  for publishing: `RUSTFLAGS` replaces the flags in `.cargo/config.toml`
  that link the plugin, and a build without the remapped paths keeps
  this computer's paths.
- **Nothing of the moment or the computer in the code**: no build
  script that writes the date, the time, a user name, a random value, a
  git hash, or files from outside the repository.
- **Dependencies by version or a fixed commit** (`rev = "<40 digits>"`),
  never a branch, and never a `path` outside the repository: such a
  crate's folder is in no remapped path, and it also changes the hashes
  Cargo gives every crate, so the code itself comes out different.
- **Tag the commit built and push the tag**; never build from changes
  not committed. `[source] revision` names that tag.

To see that it holds, build in two folders with two Cargo homes and
compare (`cmp` prints nothing when the files are the same):

```sh
git clone <repository> /tmp/one && git clone <repository> /tmp/elsewhere/two
(cd /tmp/one && ./build.sh) && (cd /tmp/elsewhere/two && CARGO_HOME=/tmp/cargo-two ./build.sh)
cmp /tmp/one/dist/clock-1.0.0-x4.ppn /tmp/elsewhere/two/dist/clock-1.0.0-x4.ppn
cmp /tmp/one/dist/clock-1.0.0-x4-pro.ppn /tmp/elsewhere/two/dist/clock-1.0.0-x4-pro.ppn
```

When they differ, `paperpaw-sign plugin` of both says how: another
`fingerprint:` means another compiler or SDK; the same fingerprint with
another `digest:` means something of the computer or the moment got into
the code.

Builds for the X4 Classic are not rebuilt yet.

### Build, pack and sign

In CI: commit, then, with the user's yes, tag and push:

```sh
git tag v1.0.0
git push origin v1.0.0
```

The job fails at once when the tag is not `v` + `plugin.ini`'s version.
It attaches to the tag's GitHub release (and to its artifact `plugin`):

```text
dist/clock-1.0.0-upload.zip      the folder with store/, for the form
dist/clock-1.0.0.zip             the folder without store/, as readers install it
dist/clock-1.0.0-x4.ppn          the X4's build, signed
dist/clock-1.0.0-x4-pro.ppn      the X4 Pro's build, signed
dist/SHA256SUMS                  the SHA-256 of each
```

Or on one's own computer (Linux or macOS with both toolchains), the same:

```sh
. ~/export-esp.sh
./build.sh --key ~/.paperpaw/author.key
```

Or by hand, for each reader:

```sh
paperpaw-sign pack target/riscv32imc-unknown-none-elf/release/clock clock-1.0.0-x4.ppn --key ~/.paperpaw/author.key
paperpaw-sign pack target/xtensa-esp32s3-none-elf/release/clock clock-1.0.0-x4-pro.ppn --key ~/.paperpaw/author.key
```

Or sign in the browser: pack without `--key`, open
`https://paperpaw.org/authors/sign`, choose the key file and the `.ppn`
files, sign and save each. Nothing leaves the computer (Firefox 129,
Chrome and Edge 137, Safari 17 or later).

### Check each build

```sh
paperpaw-sign plugin dist/clock-1.0.0-x4.ppn
paperpaw-sign check-plugin dist/clock-1.0.0-x4.ppn
```

Expected (`check-plugin`; digits differ):

```text
plugin:      61892 bytes for the ESP32-C3, fingerprint 85073a61323717ff
signer:      3F1C 22A0 9B7D 0E15 (3f1c...9a0b)
signature:   holds (...)
```

For every file:

- `chip:` (of `plugin`) is `ESP32-C3` for `x4`, `ESP32-S3` for `x4-pro`
  and `x4-classic`;
- `fingerprint:` is the one the reader's *About* shows, or the release's
  `paperpaw-<reader>-<version>-abi.txt`;
- the 64 digits in brackets after `signer:` are the same in every file
  and every version;
- `signature:   holds`. `signer:      none, not signed` means it was
  packed without `--key`: `paperpaw-sign sign-plugin <file> --key
  <path>`.

### The source

Make sure the repository is public on `github.com`, `codeberg.org` or
`gitlab.com`, the tag is pushed and points at the commit built, and
`Cargo.lock` is in it. `[source]` in `store.ini` names that repository
and tag (with `path` and `bin` when needed); the site fetches the
forge's ZIP of that revision itself. A source elsewhere goes up as a
ZIP in the form's `source-zip` instead, with `Cargo.toml` at its top or
in one folder at its top:

```sh
git archive --format=zip -o clock-1.0.0-source.zip v1.0.0
```

## 6. Submit with the form

1. **Sign in** (the user): `https://paperpaw.org/account/sign-in`, enter
   the e-mail address, open the link sent (it holds 30 minutes, once)
   and press Sign in. The first time, give the name the store shows (at
   most 80 characters) and accept the rules for authors.
2. **Upload** at `https://paperpaw.org/submit` (`POST /submit`,
   multipart). One file is the easiest: made in the PaperPaw Workshop,
   it is `<id>-<version>-store.zip`; the user chooses it in the form's
   first field, and nothing else. The form says what it found ("From
   the Workshop: plugin ✓, X4 build ✓, source ✓") and puts the other
   fields away. Without the Workshop, upload by the fields:

   | Field | What |
   |---|---|
   | `package` | required: the folder as a ZIP, or a store file with all in it (then nothing else) |
   | `build-x4`, `build-x4-pro`, `build-x4-classic` | code plugins: the signed `.ppn` of each reader it runs on |
   | `source-zip` | optional: the source as a ZIP, only when `store.ini` names no repository and revision |

   Each `.ppn` is taken for the reader its header names, whichever
   field it is in; two builds for one reader are refused.

   A store file can also be packed by hand, for one upload from a CI
   job: a ZIP (entries stored or deflated) holding the parts at its top
   and `paperpaw-store.ini`, which names them:

   ```ini
   [store-file]
   format = 1
   id = clock
   version = 1.0.0
   package = clock-1.0.0-upload.zip
   source = clock-1.0.0-source.zip

   [builds]
   x4 = clock-1.0.0-x4.ppn
   x4-pro = clock-1.0.0-x4-pro.ppn
   ```

   Every file in it must be named there; `source` is left out when
   `store.ini` names the repository and revision. The site checks each
   part as if uploaded apart, within the same limits, and `id` and
   `version` must be `plugin.ini`'s.

   The site makes a draft only the author sees, at `/submit/<n>`.
3. **Read the checks.** Each names its code and says Passed, Warning or
   Failed with a sentence; what failed comes first. Ask the user for the
   sentences, then fix each failure (section 8) and upload again, or
   save the entry again.
4. **Complete the entry** on `/submit/<n>`: what `plugin.ini` and
   `store.ini` named is filled in. The form's fields: `name`, `summary`,
   `description`, `group`, `tags` (comma separated), `licence`,
   `source`, `sdk`, `devices`, `source-repository`, `source-revision`,
   `source-path`, `source-bin`, `i18n-<lang>-name`,
   `i18n-<lang>-summary`, `screenshot`, `alt-<file>`, `remove-<file>`.
   Save and check: every check runs again. What only a new upload can
   change (the folder's files, the builds), change by uploading again.
5. **Look at the preview** with the user: the plugin's page at
   `/submit/<n>/page`, and where the site runs PaperPaw's emulator the
   readers' page at `/submit/<n>/preview/<device>.png`. The kept source
   and files are at `/submit/<n>/source` and `/submit/<n>/files`.
6. **Send it** (`Send to the moderators`, shown only when no check
   fails) after the user says so. Until they decide, the entry can
   still change (it becomes a draft again) or be withdrawn. One version
   of a plugin waits at a time.
7. **The decision** comes by mail: Published, or Refused with reasons.
   States: Draft, Waiting, Published, Refused, Withdrawn, Taken down;
   `/account` lists them all.

Limits: ZIP 20 MB (40 MB unpacked), each build 4 MB, source 50 MB (the
same for each part in a store file), one upload 100 MB, 10 submissions a
day (the site's setting).

## 6b. Publish from GitHub

The second way in (section 2). Everything is a file in the repository
and a tag; the site does the rest. It suits an author who already keeps
the plugin on GitHub. Sections 3 to 5b (the id, `plugin.ini`,
`store/store.ini`, the key, the builds) hold as they are.

**Always** make the repository with `paperpaw-dev new --github <id>`
(`--code` for a plugin with code; `--dir <path>` where it goes): it
writes the plugin's folder `plugin/` with `plugin.ini` and
`store/store.ini` (a valid group), the release workflow with its
switches set, a README that says what is left, and `.gitignore`; with
`--code` also the crate, its build settings and the linker script. Never
lay out such a repository by hand. Then, **before every tag**, check it
as the site will, and fix every error it names:

```sh
paperpaw-dev check-publish            # in the repository
paperpaw-sign check-publish .         # the same check, a smaller tool
```

Both take the rules the site checks by (`--rules <file>` takes the
site's newest, `https://paperpaw.org/api/v1/store-rules.json`). The
workflow runs the same check before it builds and again on the files it
made (`--release-dir`), and releases nothing where it fails. paperpaw-dev
comes from PaperPaw's repository at the release's tag (`cargo install
--git https://github.com/hthoene/paperpaw --tag v0.6.0-beta.2
paperpaw-dev`); paperpaw-sign the same way, and much smaller.

1. **The repository**: public on `github.com`, not archived, not a
   fork, with the topic `paperpaw-plugin` (*About*, the gear, *Topics*,
   or with the user's yes `gh repo edit --add-topic paperpaw-plugin`).
   The site searches for that topic every hour (at most 1000
   repositories).
2. **The workflow**: copy
   `tools/dev/templates/.github/workflows/paperpaw-release.yml` of the
   PaperPaw repository to `.github/workflows/` and set its switches at
   the top: `CODE` (`"false"` for a data plugin, `"true"` for a plugin
   with code), `X4` and `X4_PRO` (the readers to build for), `PLUGIN_DIR`
   (the folder with `plugin.ini`), `BIN` (the binary crate) and, for
   code, `PAPERPAW`, `RUST_X4`, `RUST_X4_PRO` and `ESP_GCC` as in
   section 5b. It builds as `build.sh` does, and publishes the X4 Pro's
   file for the X4 Classic too (`X4_CLASSIC`: same chip, same
   fingerprint). Keep one workflow that makes the release: the sample's
   `build.yml` makes one too (the site then takes its
   `<id>-<version>-upload.zip`, the folder with `store/`).
3. **The key** (code plugins): section 5b. The file's contents are the
   repository's secret `PAPERPAW_PLUGIN_KEY`; the job writes it to a
   temporary file and never prints it. All builds of a version, and all
   versions, are signed by this one key: the first version the site
   takes fixes the plugin's key, and a later version signed by another
   key fails the check `key` and is not offered. Data plugins need no
   key.
4. **The tag**: `plugin.ini`'s `version` is the number, the tag is `v` +
   it (the site takes `v1.0.0` and `1.0.0`; the template wants the
   first). With the user's yes:

   ```sh
   git tag v1.0.0
   git push origin v1.0.0
   ```

   The job stops at once when the tag is not `v` + `plugin.ini`'s
   version, and when a code plugin has no secret.
5. **The release** the job makes must be published, not a draft and not
   a pre-release. Its assets, by name:

   | Asset | What |
   |---|---|
   | `<id>-<version>.zip` | the plugin's folder as the form's `package` takes it: `plugin.ini` at the top or in one folder, `store/store.ini` and the screenshots under `store/` |
   | `<id>-<version>-<device>.ppn` | code plugins: one signed build per reader, `<device>` one of `x4`, `x4-pro`, `x4-classic` (the X3 runs the X4's); a fingerprint may follow the device in the name, and the file's header decides which reader it is for; two builds for one reader fail |

   Or one asset, `<id>-<version>-store.zip`, a store file (section 6):
   then nothing else is read. Other assets are ignored; at most 10 are
   read, with the form's limits (ZIP 20 MB, a build 4 MB, source 50 MB).
6. **The source** of a code plugin: `[source]` in `store/store.ini`, as
   for the form; when it names none, the site takes the repository at
   the release's tag and keeps it. Commit `Cargo.lock`.
7. **What the site does** within the hour: it takes the repository's
   newest release (and keeps the versions it took before), runs the
   form's checks on it (section 8 has them; warnings are shown, a
   failure means the version is not offered), keeps the files, and lists
   the plugin as "Community, unreviewed" with the GitHub owner as author,
   a link to the repository, the key's fingerprint and a report button.
   Readers download from the site, never from GitHub. The admins can
   block a repository, an owner or a key, which takes it down at once.
8. **Ids**: an id belongs to whoever it first appeared with, reviewed or
   from GitHub (by key; a data plugin by account or GitHub owner). A
   release with another's id or key is not offered, and the ids and
   names of PaperPaw's own plugins are reserved. Look at
   `https://paperpaw.org/api/v1/plugins.json?community=1` first (section
   3).
9. **Reviewed later**: the author names the repository in the form at
   `https://paperpaw.org/submit` (*From GitHub*, signed in); the site
   fills in everything from the newest release, and the author sends it
   to the moderators. A plugin with code is theirs by its key. Once
   approved, each new release waits for the moderators as an update.

**Checklist before the tag**:

- [ ] made with `paperpaw-dev new --github`, the topic `paperpaw-plugin`
  set on GitHub;
- [ ] `plugin.ini`: id, name, a one-sentence description, the version
  to tag;
- [ ] `store/store.ini`: one of the 12 groups, at most 5 tags, a licence;
  each screenshot named in `[screenshots]` with what it shows;
- [ ] code: `Cargo.lock` committed, `paperpaw-sdk` by the release's
  version, the secret `PAPERPAW_PLUGIN_KEY` set, `*.key` ignored;
- [ ] `check-publish` passes with no error;
- [ ] the tag is `v` + `plugin.ini`'s version.

**Refused most often, and the fix**:

| `check-publish` says | Fix |
|---|---|
| `the group ... is none of the store's` | `group =` one of reading, formats, library, sync, home, look, notes, learning, news, tools, games, system |
| `N tags: at most 5`, `the tag ...` | at most five, lower-case words with hyphens, separated by commas |
| `say what it shows: home.png = ...` | a line under `[screenshots]` for each picture in `store/` |
| `no summary`, `the summary is one sentence` | `description` in `plugin.ini` of one sentence (at most 160 characters), or `summary` in `store.ini` |
| `the id ...` | lower-case letters, digits and hyphens, at most 40 |
| `a plugin's folder holds only ...` | keep README, LICENSE and the like outside the plugin's folder (`PLUGIN_DIR: plugin`) |
| `tag it v...` | tag `v` + `plugin.ini`'s version, or change the version |
| `no Cargo.lock`, `name the SDK ... by its version` | build once and commit `Cargo.lock`; `paperpaw-sdk = "<release>"` |
| `not signed`, `signed by different keys` | the secret `PAPERPAW_PLUGIN_KEY` set, one key for every build |
| `names no reader`, `not the ...'s chip` | let the workflow name the builds; never rename a `.ppn` |

Do not say the plugin is published until the user has seen it listed on
the site; report what the site says, word for word. An update is a new
tag in the same way (section 7).

## 7. Updates

1. Raise the version in `plugin.ini` and in the code's manifest (and
   `Cargo.toml`): higher than the last. A number once published, also
   one taken down, is never used again.
2. Code plugins: new builds, signed **with the same key**, from a new
   tag; `[source] revision` the new tag.
3. Form: upload the new folder (or the new store file) at `/submit`
   with the account that published the plugin. What the upload leaves out is taken from the
   last published version (summary, description, group, licence,
   source, sdk, tags, translations, screenshots, `path`, `bin`).
4. An update goes live at once only when the store's admins allow it,
   it is signed by the same key and every check passed without a
   warning; otherwise it waits for the moderators.
5. From GitHub (section 6b): a new tag and release with the same key;
   the site takes the newest release within the hour. There is no
   moderator step; the checks decide.

## 8. Checks, messages and how to fix them

### The site's checks

The codes stay the same; the sentences are the site's own words and may
change, so go by the code.

| Code | What it checks | When it fails, and what to do |
|---|---|---|
| `upload` | the files can be read: a ZIP of at most 20 MB (40 MB unpacked), no name leading outside the folder, builds at most 4 MB, source at most 50 MB; a store file holds what its `paperpaw-store.ini` names and nothing else; each `.ppn` is a build for a reader, one per reader; warns when it left out files that are no part of a plugin | "Two files are builds for the Xteink X4": the same reader's build was chosen twice; choose the other reader's, or leave its field empty. "is no build for a reader": choose the `.ppn` that `paperpaw-sign pack` made. "The Workshop's file holds the builds and the source already": choose the store file alone. "holds ..., which paperpaw-store.ini does not name": name it or leave it out. "not a ZIP", "larger than 20 MB", "a name outside the folder" (`..` or a backslash): zip again as in section 5a. "Left out what is no part of a plugin: .DS_Store" (warning): zip without them |
| `manifest` | `plugin.ini` at the top or in one folder at the top, with `id`, `name`, `version` | "The ZIP has no plugin.ini at its top": zip the plugin's folder itself, not the folder around it. "plugin.ini names no id": add the key |
| `id` | the id is free or the author's | "belongs to another author", "is one of PaperPaw's own plugins", "was taken down and cannot be used again": choose another id in `plugin.ini` and the code |
| `version` | three numbers (a pre-release after them or not), not published before, not older than the newest | "was published before; a new version needs a new number", "is older than": raise `version` |
| `metadata` | the entry: id's letters, name, summary, description, licence, group, tags, languages, screenshots' names and alt, `source` https, `sdk` like `0.5`; for code its readers, key and source | fix the field the sentence names, in the form or `store.ini`. "sdk is not like 0.3": fill in `sdk`. "source: the revision is a tag or a commit": fill in `source-revision` |
| `package` | the folder readers install: allowed file types, names, at most 64 files (63 for code), `plugin.ini`'s id and version, `formats`, translations that parse | "only ini, css, ttf, otf, ttc, ppfp, txt, md, png files belong in a data plugin": take that file out. "names are letters, digits, dots, hyphens and underscores": rename. "more than 63 files": fewer. A translation that does not parse names its line |
| `build.x4`, `build.x4-pro`, `build.x4-classic` | each build: a plugin file of format 1, that reader's chip, as long as its header says, digest whole, signed | "The file is not signed": sign it. "not a PaperPaw plugin file": that is the linked program; make the `.ppn` with `paperpaw-sign pack`. "Changed or damaged": build and pack again. "The file of this build is missing": upload it again |
| `signature` | all builds signed by one key | "signed by 2 different keys": sign every build with the one key file. "another key than the entry names": sign with the key in `author_key`, or correct it |
| `key` | the key is that of the versions before | "Earlier versions are signed by the key": sign with the earlier key; lost, section 9 |
| `source` | fetched at the revision or uploaded; at most 50 MB; `Cargo.toml` at the top or at `path`; warns without `Cargo.lock` | "name a repository and a revision": fill in `[source]` or the fields. "could not be fetched from": push the tag, make the repository public, check the address. "has no Cargo.toml at its top": set `path`. "no Cargo.lock": commit it and tag again |
| `screenshots` | at most four, PNG, JPEG or WebP, 500 KB each; warns when there are none | "at most 500 KB": smaller (fewer colours, or WebP). "is a PNG, JPEG or WebP picture": convert it. "No screenshots" (warning): add one |

### paperpaw-sign

Each starts with `error: `.

| Message (or its end) | Cause | Fix |
|---|---|---|
| `... exists: a key is never overwritten` | a key file is there | use that key |
| `the key is not 64 hexadecimal digits` | another file passed to `--key` | pass the key file |
| `no --key and no PAPERPAW_SIGNING_KEY` | `sign-plugin` without a key | add `--key <path>` |
| `an object, not a linked plugin: pack the plugin as linked` | an `.o` or `.rlib` passed | pass `target/<target>/release/<bin>` |
| `made for machine N, neither RISC-V nor Xtensa` | built for the computer | `cargo x4`, `cargo +esp x4pro` |
| `no symbol table: do not strip the plugin` | `strip` in the profile | remove `strip` from `[profile.release]` |
| `... : link the plugin with paperpaw-plugin.x` | `.cargo/config.toml` missing or changed, or `RUSTFLAGS` set | restore the sample's `.cargo/config.toml`; unset `RUSTFLAGS` |
| ``no relocations kept: link the plugin with `--emit-relocs` `` | as above | as above |
| ``... : a plugin's entry is made by the SDK's `plugin!` `` | no `plugin!` in the binary crate | `paperpaw_sdk::plugin!(my_plugin::MyPlugin::default());` in `src/main.rs` |
| `a plugin has no writable data (its state lives on the kernel's heap), but this one has: ...` | a `static mut`, a static with interior mutability, or a crate keeping one | keep state on the heap (fields of the plugin, `Rc` given to closures); drop the crate named |
| `an unexpected section X (n bytes): a plugin is only its .rodata and .text` | thread locals, `#[link_section]`, a crate's own sections | remove what makes it |
| `the image is N bytes, more than the M a plugin may have` | over 4 MiB | `docs/plugins.md`, "Keeping it small" |
| ``the plugin cannot be packed (...): undefined symbol `x` (used at ...)`` | it calls what neither it nor the SDK brings (`std`, C code, libm) | remove the call, or bring the code in Rust |
| `... : not known to this tool` / `... points outside the image` | code that reaches a fixed address | use no addresses of the firmware or ROM |
| `<file>: not signed` (`check-plugin`) | packed without `--key` | `paperpaw-sign sign-plugin <file> --key <path>` |
| `<file>: changed or damaged` | the file changed after packing | build and pack again; never edit it |
| `<file>: its author's signature does not hold` | the file or its trailer changed | sign again |
| `<file>: not a plugin file` / `not as long as it says` | not a `.ppn`, or cut short | use the file `pack` wrote |

### The rebuild

Not a check that holds a plugin back: readers always get the signed
file. After publishing, anyone can repeat it, from a clone of the
paperpaw.org repository with Docker:

```sh
cargo run -- verify clock 1.0.0 --device x4
rebuild/verify.sh clock 1.0.0 x4
```

It prints `MATCHES` or `DIFFERS` with both hashes and ends with 0 when
they match, 1 when they differ, 2 when it could not tell. "Not rebuilt"
means waiting, no toolchain known for the fingerprint, or no rebuild
possible. "Rebuilt, but does not match its source": build with
`build.sh` (or the CI job) from the tagged commit on Linux or macOS,
check it with two folders (section 5b), and publish a new version.

## 9. A lost or leaked key

The store takes no other key than the earlier versions' without a
moderator's transfer of the id (check `key`, `claim`).

- **Lost**: write to the site's contact address (`/contact`). A
  moderator can transfer the id to the new key in the open, logged;
  readers who ever had the plugin refuse every version signed by the new
  key and keep the one they have. For them a lost key means a new id.
- **Leaked** (shown in a chat, committed, in a log): the same, at once;
  remove the old secret from the CI (`PAPERPAW_PLUGIN_KEY`) and from the
  repository's history, and tell the user plainly.

## 10. What readers see

The plugin's page on the site and in each reader's store: whether it
has code (and, before a reader installs the first code plugin, what
that means), who made it, its licence and source, its badge, and for
code the PaperPaw it is built for. A reader refuses an update or a new
install signed by another key than the one it first installed. `/log` and
`/api/v1/log.json?after=<n>` list every version with the SHA-256 of its
package and builds, its key and its source's revision.

## 11. Final checklist

- [ ] The id is valid, free, and the same in `plugin.ini`, the folder's
      name and the code.
- [ ] `version` is higher than every published one, the same in
      `plugin.ini` and the code's manifest; the tag is `v` + it.
- [ ] `store/store.ini`: no comment after a value, every section line on
      its own, `group` from the list, at most five `tags`, `licence` an
      SPDX identifier, `source` https, `sdk` like `0.5`, a real
      description of each screenshot.
- [ ] The ZIP holds `plugin.ini` at its top or in one folder at its top,
      `store/store.ini` beside it; only allowed file types outside
      `store/`; at most 64 files (63 for code); no `code.ppn`; no hidden
      files.
- [ ] Code: one `.ppn` for each reader; `paperpaw-sign plugin` shows the
      right chip for each and the same signer for all;
      `check-plugin` says `signature:   holds`.
- [ ] Code: each fingerprint is the one the reader's *About* shows, or
      the release's `paperpaw-<reader>-<version>-abi.txt`.
- [ ] Code: built with `build.sh` (or its CI job) from the tag, on Linux
      or macOS; `Cargo.lock`
      committed; the repository public on `github.com`, `codeberg.org`
      or `gitlab.com`; `[source]` names it and the tag, with `path` and
      `bin` when needed.
- [ ] From GitHub: the repository is public, not a fork, with the topic
      `paperpaw-plugin`; the release is published (no draft, no
      pre-release) and holds `<id>-<version>.zip` with `store/`, and for
      code one signed `.ppn` per reader named `<id>-<version>-<device>`;
      the tag is `v` + `plugin.ini`'s version; the secret
      `PAPERPAW_PLUGIN_KEY` is the key of the earlier versions.
- [ ] No secret anywhere: no `*.key` in a repository or its history, the
      key in no chat, file or log; `PAPERPAW_PLUGIN_KEY` only as a CI
      secret.
- [ ] Tried in the emulator; a code plugin on a reader if the user has
      one.
- [ ] The user has seen the files and the entry, and says yes to each
      step that sends something.

## 12. Not settled yet

- Whether the X4 Pro and the X4 Classic share a fingerprint, and
  rebuilds for the X4 Classic, are open.
- The checks' sentences are not frozen word for word; their codes are.
