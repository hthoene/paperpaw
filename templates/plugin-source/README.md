# A plugin source for PaperPaw

A template for a **community plugin source**: a plugin library of your own
that readers of [PaperPaw](https://paperpaw.org) add on their device by its
address, beside PaperPaw's own library. Copy this folder into a new, public
GitHub repository; its workflow builds the plugin for every device, signs it
with your key and publishes the source with GitHub Pages. Nobody at PaperPaw
has to do anything, and PaperPaw does not check what a source offers.

It holds one example plugin, *Hello paw*: an entry in the tools menu that
opens a screen, with a setting and an event. [CODE.md](CODE.md) says how its
code is laid out and built; [Writing plugins](https://github.com/hthoene/paperpaw/blob/main/docs/plugins.md)
is the guide.

| File | What it is |
| --- | --- |
| `plugin/` | The plugin's folder as readers install it: `plugin.ini`, its words in `i18n/`. `plugin/store/store.ini` says what the list shows of it and is not installed. |
| `src/main.rs`, `Cargo.toml`, `native/` | The plugin's code, for plugin interface 7 (`native_plugin!`), and the record of what it calls. |
| `.cargo/config.toml`, `paperpaw-plugin7.x`, `rust-toolchain.toml` | How the code is built for each device. Leave them as they are. |
| `.github/workflows/plugin-source.yml` | Builds, signs and publishes on every push to `main`. |

## 1. Make the repository yours

1. Copy the folder's files (the hidden ones too: `.github`, `.cargo`,
   `.gitignore`) into a new public repository on GitHub.
2. Choose the plugin's id (lower-case letters, digits and hyphens, beginning
   with a letter, at most 40) and put it in place of `hello-paw` everywhere:
   `Cargo.toml` (the package and the program), `plugin/plugin.ini`,
   `plugin/i18n/en.ini` and `src/main.rs`. An id belongs to one source on a
   device: a reader who has `hello-paw` from another source does not get
   yours.
3. Fill in `plugin/plugin.ini` (name, one sentence of description, version)
   and `plugin/store/store.ini` (group, how it works, licence, where the
   source is).
4. Commit the `Cargo.lock` that the first build writes.

## 2. Make the key (once)

The source has one Ed25519 key. It signs the list and the code of every
plugin. A device keeps the key of the first list it reads from your address
and from then on takes only lists and code signed with it.

```sh
# paperpaw-sign of the PaperPaw release the plugin is built for:
cargo install --locked --git https://github.com/hthoene/paperpaw --tag v0.6.0-beta.3 paperpaw-sign
mkdir -p ~/.paperpaw
paperpaw-sign key ~/.paperpaw/source.key
```

It prints the public key and nothing else of it. The file is one line of 64
hexadecimal digits, the secret:

- Keep it outside every repository, with a copy kept apart (a password
  manager, an encrypted drive). `.gitignore` leaves `*.key` out.
- **A lost key cannot be replaced.** Devices that added your source refuse a
  list signed with another key ("This source has another key") until their
  reader removes the source and adds it again.
- Whoever has the key can publish code in your name to everyone who added
  your source. If it leaks, make a new one and tell your readers.

## 3. Give the key to the workflow

In the repository: *Settings > Secrets and variables > Actions > New
repository secret*, named `PAPERPAW_SOURCE_KEY`, its value the file's line.
Or with GitHub's command line, which reads the file without showing it:

```sh
gh secret set PAPERPAW_SOURCE_KEY < ~/.paperpaw/source.key
```

The workflow's signing job is the only one that sees the secret, and it runs
none of the repository's code: only PaperPaw's `paperpaw-sign`.

## 4. Publish

1. *Settings > Pages > Build and deployment > Source*: **GitHub Actions**.
2. Push to `main` (or run the workflow *plugin-source* by hand). It builds
   the plugin for the Xteink X4 and X3, the X4 Pro and X4 Classic and the
   desktop emulator, signs the builds and the list, and publishes them.
3. The run's summary says the source's address and how its key begins:

   ```text
   https://<you>.github.io/<repository>/plugins.json
   ```

   Tell your readers both. The page at `https://<you>.github.io/<repository>/`
   says them too.

A new version is a higher `version` in `plugin/plugin.ini` (and
`Cargo.toml`), pushed to `main`: devices offer it as an update. Never
publish other files under a version already published.

The run's artifact `emulator` is the plugin for the desktop emulator on
Linux: unpack it and start `paperpaw-emu --library <folder> --trust-code`.

## 5. How readers add the source

On the device (the X4, X3, X4 Pro and X4 Classic), with Wi-Fi on:

1. Open **Plugin store**. The last row of its start is **Sources**.
2. Choose **Add a source** and type the address
   (`https://<you>.github.io/<repository>/plugins.json`). Instead of typing,
   put a file `sources.txt` with the address on a line of its own into
   `.paperpaw/plugins/store` on the card: it then shows under *Sources* as
   new.
3. The device says that PaperPaw does not check the source or its plugins,
   and that its code plugins get full access to the device. Once the reader
   agrees, it reads the list and says how the source's key begins: the 16
   digits you told them. From then on it takes only lists and code signed
   with that key.
4. The source's plugins are listed in place of PaperPaw's and install as
   any other. Leaving the store returns to PaperPaw's own library; *Sources*
   lists yours again, or removes it. A device keeps up to 8 sources.

A plugin gets its updates only from the source it was installed from.

## More than one plugin, or none with code

`paperpaw-sign pack-source <folder> <out>` makes the source from a folder
that holds, for each plugin, its folder named as its id and, for a plugin
with code, its builds beside it:

```text
source-in/
  hello-paw/plugin.ini ...        a plugin with code
  hello-paw-0.1.0-x4.ppn          its build for the X4 and X3
  hello-paw-0.1.0-x4-pro.ppn      its build for the X4 Pro and X4 Classic
  night-owl/plugin.ini ...        a plugin without code: nothing to build
```

Add your other plugins' folders in the workflow's step *Gather the plugin
and its builds*. A plugin without code needs no build and no signature of
its own. To publish by hand instead:

```sh
paperpaw-sign sign-plugin source-in/hello-paw-0.1.0-x4.ppn --key ~/.paperpaw/source.key
paperpaw-sign pack-source source-in site --author "Your name" --key ~/.paperpaw/source.key
paperpaw-sign check-source site/plugins.json
```

and serve the folder `site` over `https://` from any web server, as it is.
A list changed after it was signed is not read: run `pack-source` (or
`paperpaw-sign sign-source site/plugins.json`) again after every change.

## Being listed on paperpaw.org

[paperpaw.org/sources](https://paperpaw.org/sources) lists sources whose
publishers asked for it. It is a list of addresses only: PaperPaw neither
checks nor hosts them. The page says how to get listed.
