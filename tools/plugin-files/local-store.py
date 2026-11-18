#!/usr/bin/env python3
"""A local stand-in for a release's store files, to try the plugin library
without publishing anything (nothing here leaves the computer).

    python3 tools/plugin-files/local-store.py <out> --x4 <dir> --x4-pro <dir> [--version V] [--port N]

`<dir>` holds `<id>-<version>-<device>.ppn` made by `tools/plugin-files7/build.sh`
(signed with a throw-away key: `paperpaw-sign key <file>`; the key is the
site's `PAPERPAW_FIRST_PARTY_KEY` below). Only the ids of `plugins.txt` are used;
the X4 Classic takes the X4 Pro's files under its own name. Writes

- `<out>/dist/plugins`: the files, the folders' ZIPs and `catalogue/<id>.toml`
  (`catalogue.py`), and `<out>/dist/paperpaw-<V>-catalogue.zip`;
- `<out>/mirror`: what GitHub would serve (`releases.json`, `download/v<V>/`,
  `download/plugins-<year>/`), to be served by `python3 -m http.server <port>
  --bind 127.0.0.1` from there (the default port is 18989);
- `<out>/card/<device>/plugins/<id>/`: each plugin's folder with its file as
  `code.ppn`, to copy to a card (an unsigned code plugin, Decision 205).

paperpaw.org (a build with plugin interface 7, PR #49) then imports it:

    PAPERPAW_ROOT=<the site checkout> PAPERPAW_DATA=<empty dir> PAPERPAW_LISTEN=127.0.0.1:18990 \\
    PAPERPAW_BASE_URL=http://127.0.0.1:18990 PAPERPAW_TEST_SITE=1 \\
    PAPERPAW_RELEASES_API=http://127.0.0.1:<port>/releases.json \\
    PAPERPAW_RELEASE_DOWNLOADS=http://127.0.0.1:<port>/download/ \\
    PAPERPAW_FIRST_PARTY_KEY=<the throw-away key's public key> \\
    PAPERPAW_WORKSHOP_RELEASES_API=none PAPERPAW_HUB_API=none paperpaw-website

and answers `/api/v1/plugins.json?device=x4&lang=en` with the builds.
"""
import argparse
import json
import pathlib
import shutil
import subprocess
import sys
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import catalogue  # noqa: E402


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--x4", type=pathlib.Path, required=True)
    parser.add_argument("--x4-pro", type=pathlib.Path, required=True)
    parser.add_argument("--version", default="0.6.0-beta.4")
    parser.add_argument("--port", type=int, default=18989)
    args = parser.parse_args()
    out = args.out.resolve()
    shutil.rmtree(out, ignore_errors=True)
    plugins = out / "dist" / "plugins"
    plugins.mkdir(parents=True)
    ids = [plugin for plugin, _ in catalogue.plugins()]
    for plugin in ids:
        version = catalogue.plugin_version(plugin)
        shutil.copy(args.x4 / f"{plugin}-{version}-x4.ppn", plugins)
        pro = args.x4_pro / f"{plugin}-{version}-x4-pro.ppn"
        shutil.copy(pro, plugins)
        shutil.copy(pro, plugins / pro.name.replace("-x4-pro.ppn", "-x4-classic.ppn"))
    download = f"http://127.0.0.1:{args.port}/download/"
    subprocess.run(
        [sys.executable, str(HERE / "catalogue.py"), str(plugins), args.version,
         download + "plugins-2026", "--date", "2026-01-01"],
        check=True,
    )
    zipped = out / "dist" / f"paperpaw-{args.version}-catalogue.zip"
    with zipfile.ZipFile(zipped, "w", zipfile.ZIP_DEFLATED) as z:
        for entry in sorted((plugins / "catalogue").glob("*.toml")):
            z.write(entry, "catalogue/" + entry.name)
    store, release = out / "mirror/download/plugins-2026", out / f"mirror/download/v{args.version}"
    store.mkdir(parents=True)
    release.mkdir(parents=True)
    shutil.copy(zipped, release)
    for file in plugins.iterdir():
        if file.is_file():
            shutil.copy(file, store)
    listing = [
        {"tag_name": f"v{args.version}", "draft": False, "assets": [
            {"name": zipped.name, "browser_download_url": f"{download}v{args.version}/{zipped.name}"}]},
        {"tag_name": "plugins-2026", "draft": False, "assets": [
            {"name": f.name, "browser_download_url": f"{download}plugins-2026/{f.name}"}
            for f in sorted(store.iterdir())]},
    ]
    (out / "mirror/releases.json").write_text(json.dumps(listing), encoding="utf-8")
    for device in ("x4", "x4-pro", "x4-classic"):
        for plugin in ids:
            folder = out / "card" / device / "plugins" / plugin
            version = catalogue.plugin_version(plugin)
            with zipfile.ZipFile(plugins / f"{plugin}-{version}.zip") as z:
                z.extractall(folder)
            shutil.copy(plugins / f"{plugin}-{version}-{device}.ppn", folder / "code.ppn")
    print(f"{len(ids)} plugins; {len(list(store.glob('*.ppn')))} files; serve {out / 'mirror'} on port {args.port}")


if __name__ == "__main__":
    main()
