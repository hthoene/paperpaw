#!/usr/bin/env python3
"""Install CI's pinned espflash without compiling it or installing development headers."""

import hashlib
import http.client
import io
import os
import pathlib
import platform
import random
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
import zipfile

# Official esp-rs/espflash v4.6.0 release asset, checked against GitHub's
# SHA-256 digest. The static musl executable needs no build-time libudev headers.
# Review a new archive and its digest when versions.env changes.
ASSETS = {
    ("4.6.0", "Linux", "x86_64"): (
        "espflash-x86_64-unknown-linux-musl.zip",
        "d515ee13ae44ba913c31a2ca58dc5fd72d3362994ee1d7487a16b289dfbb6e4a",
    ),
}
MAX_BYTES = 32 * 1024 * 1024
# GitHub's release download answers 502/503/504 now and then (a CI round of
# ~40 minutes lost each time): try again, with growing waits, before failing.
ATTEMPTS = 6
FIRST_WAIT = 2.0


def matches(executable, version):
    try:
        result = subprocess.run([str(executable), "--version"], capture_output=True,
                                text=True, encoding="utf-8", timeout=10, check=True)
        return result.stdout.strip() == f"espflash {version}"
    except (OSError, subprocess.SubprocessError):
        return False


def fetch(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("espflash archive is larger than expected")
    return data


def transient(error):
    """Whether trying again can help: a gateway or server error, a rate
    limit, a timeout or a connection that broke; not a 404 or a bad pin."""
    if isinstance(error, urllib.error.HTTPError):
        return error.code in (408, 429) or error.code >= 500
    return isinstance(error, (OSError, http.client.HTTPException))


def download(url, checksum):
    """The archive at `url`, whose SHA-256 is `checksum`: fetched again, after
    a wait that doubles, where the server fails or the body is cut short."""
    wait = FIRST_WAIT
    for attempt in range(1, ATTEMPTS + 1):
        try:
            data = fetch(url)
            if hashlib.sha256(data).hexdigest() == checksum:
                return data
            problem = "the SHA-256 differs from the reviewed pin"
        except ValueError:
            raise
        except Exception as error:  # noqa: BLE001 - classified just below
            if not transient(error):
                raise
            problem = str(error)
        if attempt == ATTEMPTS:
            break
        print(f"espflash download attempt {attempt} of {ATTEMPTS} failed ({problem}); "
              f"trying again in {wait:.0f} s", file=sys.stderr)
        time.sleep(wait + random.uniform(0, 1))
        wait *= 2
    raise ValueError(f"espflash release archive: {problem} after {ATTEMPTS} attempts")


def cached_archive(directory, name, checksum):
    """The archive kept in CI's cache, if it is still the pinned one."""
    if not directory:
        return None
    path = pathlib.Path(directory) / name
    try:
        data = path.read_bytes()
    except OSError:
        return None
    return data if hashlib.sha256(data).hexdigest() == checksum else None


def keep_archive(directory, name, data):
    """Keeps a verified archive for the next run; a failure to is no failure."""
    if not directory:
        return
    try:
        path = pathlib.Path(directory)
        path.mkdir(parents=True, exist_ok=True)
        temporary = path / (name + ".tmp")
        temporary.write_bytes(data)
        temporary.replace(path / name)
    except OSError as error:
        print(f"espflash archive not cached: {error}", file=sys.stderr)


def install(version, destination):
    key = version, platform.system(), platform.machine()
    if key not in ASSETS:
        raise ValueError(f"no reviewed espflash archive for {key}; update tools/ci/espflash.py")
    existing = shutil.which("espflash")
    if existing and matches(existing, version):
        print(f"Reusing espflash {version}: {existing}", file=sys.stderr)
        return pathlib.Path(existing).resolve()
    name, checksum = ASSETS[key]
    # The cache (a directory of the archive, restored by the action) is only
    # trusted by its SHA-256, as a download is.
    archives = os.environ.get("ESPFLASH_ARCHIVE_DIR")
    data = cached_archive(archives, name, checksum)
    if data is None:
        data = download(f"https://github.com/esp-rs/espflash/releases/download/v{version}/{name}", checksum)
        keep_archive(archives, name, data)
    else:
        print(f"Reusing the cached espflash {version} archive", file=sys.stderr)
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        info = archive.getinfo("espflash")
        if info.is_dir() or info.file_size > MAX_BYTES or len(archive.infolist()) != 1:
            raise ValueError("unexpected espflash release archive contents")
        executable = archive.read(info)
    destination = pathlib.Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    temporary = destination / "espflash.tmp"
    temporary.write_bytes(executable)
    temporary.chmod(0o755)
    binary = destination / "espflash"
    temporary.replace(binary)
    if not matches(binary, version):
        raise ValueError("the verified espflash executable does not report the pinned version")
    print(f"Verified official espflash {version}: {name}", file=sys.stderr)
    return binary.resolve()


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: tools/ci/espflash.py <version> <install-directory>")
    try:
        print(install(sys.argv[1], sys.argv[2]))
    except (OSError, ValueError, KeyError, zipfile.BadZipFile, urllib.error.URLError) as error:
        sys.exit(f"espflash setup: {error}")


if __name__ == "__main__":
    main()
