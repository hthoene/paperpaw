"""Offline tests of CI setup: cache reuse, authenticated archives and apt fallback."""

import contextlib
import hashlib
import io
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "ci"))
import espflash  # noqa: E402


class Espflash(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.destination = pathlib.Path(self.directory.name) / "installed"
        self.patches = contextlib.ExitStack()
        self.addCleanup(self.patches.close)
        self.patches.enter_context(mock.patch.object(espflash.platform, "system", return_value="Linux"))
        self.patches.enter_context(mock.patch.object(espflash.platform, "machine", return_value="x86_64"))
        self.which = self.patches.enter_context(mock.patch.object(espflash.shutil, "which", return_value=None))
        self.fetch = self.patches.enter_context(mock.patch.object(espflash, "fetch"))
        self.matches = self.patches.enter_context(mock.patch.object(espflash, "matches", return_value=True))
        self.archive({"espflash": b"verified executable"})

    def archive(self, entries):
        data = io.BytesIO()
        with zipfile.ZipFile(data, "w") as archive:
            for name, content in entries.items():
                archive.writestr(name, content)
        self.fetch.return_value = data.getvalue()
        assets = {("4.6.0", "Linux", "x86_64"): ("espflash.zip", hashlib.sha256(data.getvalue()).hexdigest())}
        self.patches.enter_context(mock.patch.object(espflash, "ASSETS", assets))

    def test_matching_cached_binary_needs_no_download_or_install(self):
        existing = self.destination.parent / "cached-espflash"
        self.which.return_value = str(existing)
        self.assertEqual(espflash.install("4.6.0", self.destination), existing.resolve())
        self.fetch.assert_not_called()
        self.assertFalse(self.destination.exists())

    def test_cache_miss_installs_the_checksum_verified_release(self):
        binary = espflash.install("4.6.0", self.destination)
        self.assertEqual(binary.read_bytes(), b"verified executable")
        self.assertEqual(binary, (self.destination / "espflash").resolve())
        self.fetch.assert_called_once_with("https://github.com/esp-rs/espflash/releases/download/v4.6.0/espflash.zip")
        self.matches.assert_called_once_with(binary, "4.6.0")

    def test_wrong_cached_version_is_replaced_by_a_verified_tool(self):
        self.which.return_value = str(self.destination.parent / "old-espflash")
        self.matches.side_effect = [False, True]
        self.assertTrue(espflash.install("4.6.0", self.destination).is_file())
        self.fetch.assert_called_once()

    def test_corrupt_archive_is_never_executed(self):
        self.fetch.return_value += b"changed"
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            espflash.install("4.6.0", self.destination)
        self.matches.assert_not_called()
        self.assertFalse(self.destination.exists())

    def test_unexpected_archive_layout_is_not_extracted(self):
        self.archive({"espflash": b"tool", "../escape": b"bad"})
        with self.assertRaisesRegex(ValueError, "contents"):
            espflash.install("4.6.0", self.destination)
        self.assertFalse((self.destination.parent / "escape").exists())
        self.matches.assert_not_called()

    def test_wrong_reported_version_is_a_failure(self):
        self.matches.return_value = False
        with self.assertRaisesRegex(ValueError, "pinned version"):
            espflash.install("4.6.0", self.destination)

    def test_version_updates_require_a_reviewed_pin(self):
        with self.assertRaisesRegex(ValueError, "no reviewed"):
            espflash.install("4.7.0", self.destination)
        self.fetch.assert_not_called()


class BuildPrerequisites(unittest.TestCase):
    OPTIONS = "-o Acquire::Retries=5 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30"
    DEB = "ninja-build_1.12.1-1_amd64.deb"

    def setUp(self):
        self.bash = (r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else shutil.which("bash"))
        if not self.bash or not pathlib.Path(self.bash).is_file():
            self.skipTest("bash is needed for the Linux action's shell tests")
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = pathlib.Path(self.directory.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.archives = self.root / "archives"
        self.archives.mkdir()
        self.template = self.root / "tool"
        self.template.write_text("#!/bin/sh\nprintf 'fixture tool\\n'\n", encoding="utf-8")
        for tool in ["cmake", "ninja", "clang-18"]:
            self.write(tool, self.template.read_text())
        # The only commands besides the fixtures on the isolated PATH (the sudo mock uses wc).
        for tool in ["mkdir", "cp", "sleep", "wc", "rm", "sha256sum"]:
            (self.bin / tool).symlink_to(shutil.which(tool))
        # What the package list says of the one package: its file and SHA-256.
        self.good = b"the genuine package"
        self.sha = hashlib.sha256(self.good).hexdigest()
        self.write("apt-cache", "#!/bin/sh\nprintf 'Package: ninja-build\\nSHA256: %s\\n' " + repr(self.sha) + "\n")
        self.write("sudo", """#!/bin/sh
printf '%s\\n' "$*" >> "$CALLS"
case " $* " in
  # What the install would fetch (the authenticated list's file name).
  *' --print-uris '*) printf "'http://mirror/%s' %s 1 SHA256:x\\n" "$DEB" "$DEB";;
  # apt-get takes its -o options before the verb: look for `install` anywhere.
  *' apt-get '*' install '*)
    for package in "$@"; do
      case "$package" in
        cmake) tool=cmake;; ninja-build) tool=ninja;; clang-18) tool=clang-18;; *) continue;;
      esac
      /bin/cp "$TOOL_TEMPLATE" "$FAKE_BIN/$tool"
      /bin/chmod +x "$FAKE_BIN/$tool"
    done
    # apt puts what it downloaded in its archive folder.
    [ -e "$ARCHIVES/$DEB" ] || printf 'downloaded' > "$ARCHIVES/$DEB";;
  *' cp '*) shift; /bin/cp "$@";;
esac
""")

    def write(self, name, text):
        path = self.bin / name
        path.write_text(text, encoding="utf-8")
        path.chmod(0o755)

    def shell_path(self, path):
        if os.name != "nt":
            return str(path)
        return subprocess.check_output([self.bash, "--noprofile", "--norc", "-c",
                                        'cygpath -u "$1"', "-", str(path)], text=True).strip()

    def run_setup(self, **extra):
        env = os.environ | {
            "PATH": self.shell_path(self.bin), "MBEDTLS_CLANG_PACKAGE": "clang-18",
            "GITHUB_ENV": self.shell_path(self.root / "env"),
            "CALLS": self.shell_path(self.root / "calls"),
            "FAKE_BIN": self.shell_path(self.bin), "TOOL_TEMPLATE": self.shell_path(self.template),
            "APT_RETRY_PAUSE": "0", "APT_ARCHIVES": self.shell_path(self.archives),
            "ARCHIVES": self.shell_path(self.archives), "DEB": self.DEB,
        } | extra
        # Output goes to files, not pipes: a process the script left behind
        # (a grandchild of a killed command) must not hold up the test.
        out, err = self.root / "stdout", self.root / "stderr"
        with out.open("w") as stdout, err.open("w") as stderr:
            done = subprocess.run([self.bash, "--noprofile", "--norc", str(HERE / "ci/build-prerequisites.sh")],
                                  env=env, stdout=stdout, stderr=stderr, text=True)
        done.stdout, done.stderr = out.read_text(), err.read_text()
        return done

    def calls(self):
        return (self.root / "calls").read_text().splitlines()

    def test_preinstalled_commands_need_no_package_manager(self):
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.root / "calls").exists())
        environment = (self.root / "env").read_text()
        self.assertIn("CMAKE_GENERATOR=Ninja", environment)
        self.assertIn("/bin/clang-18", environment)

    def test_only_the_missing_tool_is_installed(self):
        (self.bin / "ninja").unlink()
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        # Retries and timeouts on every request (a stalled mirror must fail
        # the step), a kill after the grace period, and only the missing
        # package is installed.
        guard = "timeout --kill-after=10"
        self.assertEqual(self.calls(), [
            f"{guard} 90 apt-get {self.OPTIONS} update",
            f"{guard} 30 apt-get {self.OPTIONS} install --print-uris -qq -y ninja-build",
            f"{guard} 180 apt-get {self.OPTIONS} install -y ninja-build"])

    def test_a_stalled_attempt_is_retried_and_ends_after_three(self):
        (self.bin / "ninja").unlink()
        # Fails (as `timeout` does, 124) until the third call, then works.
        sudo = (self.bin / "sudo").read_text()
        self.write("sudo", sudo.replace("case \" $* \" in", 'n=$(wc -l < "$CALLS")\n'
                   'if [ "$n" -lt 3 ]; then exit 124; fi\ncase " $* " in', 1))
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sum("update" in call for call in self.calls()), 3)
        self.write("sudo", "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CALLS\"\nexit 124\n")
        (self.bin / "ninja").unlink()
        (self.root / "calls").unlink()
        result = self.run_setup()
        self.assertEqual(result.returncode, 124)
        self.assertEqual(len(self.calls()), 3)

    def test_an_attempt_that_ignores_the_stop_signal_is_killed(self):
        (self.bin / "ninja").unlink()
        self.write("sudo", '#!/bin/sh\nexec "$@"\n')
        # A fake apt-get that ignores TERM (the ignoring is kept by `exec`: one
        # process, so it makes no difference whether `timeout` signals a group).
        self.write("apt-get", "#!/bin/sh\ntrap '' TERM\nexec sleep 30\n")
        self.write("timeout", "#!/bin/sh\nexec " + shutil.which("timeout") + ' "$@"\n')
        started = time.monotonic()
        result = self.run_setup(APT_UPDATE_SECONDS="1", APT_KILL_GRACE="1")
        took = time.monotonic() - started
        self.assertNotEqual(result.returncode, 0)
        # Each attempt ended by the kill (137), not by the process: three times,
        # not once (the stop signal alone would have left each attempt at its 30 s).
        self.assertEqual(result.stderr.count("ended with 137"), 3, result.stderr)
        # A bound far below the 3 x 30 s of attempts that were not killed; not a tight one,
        # for a slow runner.
        self.assertLess(took, 25, result.stderr)

    def test_kept_packages_are_reused_only_if_they_match_the_package_list(self):
        (self.bin / "ninja").unlink()
        cache = self.root / "debs"
        cache.mkdir()
        (cache / self.DEB).write_bytes(self.good)
        result = self.run_setup(APT_DEB_CACHE=self.shell_path(cache))
        self.assertEqual(result.returncode, 0, result.stderr)
        # The kept file itself is what apt found in its archive folder.
        self.assertEqual((self.archives / self.DEB).read_bytes(), self.good)
        self.assertTrue(any(call.startswith("cp -n ") for call in self.calls()), self.calls())

    def test_a_tampered_kept_package_is_not_given_to_apt_and_is_dropped(self):
        (self.bin / "ninja").unlink()
        cache = self.root / "debs"
        cache.mkdir()
        (cache / self.DEB).write_bytes(b"a different file")
        result = self.run_setup(APT_DEB_CACHE=self.shell_path(cache))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(call.startswith("cp -n ") for call in self.calls()), self.calls())
        self.assertIn("does not match the package list", result.stderr)
        # What apt then downloaded is kept in its place.
        self.assertEqual((cache / self.DEB).read_bytes().decode(), "downloaded")

    def test_only_the_files_of_this_install_are_kept(self):
        (self.bin / "ninja").unlink()
        cache = self.root / "debs"
        (self.archives / "unrelated_1_amd64.deb").write_text("x")
        result = self.run_setup(APT_DEB_CACHE=self.shell_path(cache))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([f.name for f in cache.iterdir()], [self.DEB])

    def test_failed_install_does_not_publish_an_environment(self):
        (self.bin / "cmake").unlink()
        self.write("sudo", "#!/bin/sh\nexit 17\n")
        self.assertEqual(self.run_setup().returncode, 17)
        self.assertFalse((self.root / "env").exists())


class UpdatePath(unittest.TestCase):
    """tools/ci/update-path.sh with a release list, `gh`, `paperpaw-sign`
    and the test replaced: a job that tested nothing must not be green."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = pathlib.Path(self.directory.name)
        self.release = self.root / "release"
        self.release.mkdir()
        self.images = self.root / "images"
        self.work = self.root / "work"
        self.log = self.root / "tested"
        self.tags = ["v0.5.0-beta.11", "v0.4.0", "v9.9.9"]
        for device in ("x4", "x3", "x4-pro", "x4-classic"):
            image = self.images / f"install-image-{device}"
            image.mkdir(parents=True)
            (image / "install.bin").write_bytes(b"image")

    def asset(self, device, tag="0.5.0-beta.11"):
        name = f"paperpaw-{device}-{tag}-update.bin"
        (self.release / name).write_bytes(b"old " + device.encode())
        digest = hashlib.sha256((self.release / name).read_bytes()).hexdigest()
        (self.release / (name + ".sha256")).write_text(f"{digest}  {name}\n")

    def tool(self, name, text):
        path = self.root / name
        path.write_text("#!/bin/sh\n" + text)
        path.chmod(0o755)
        return path

    def run_job(self, test="true"):
        gh = self.tool("gh", f"""
if [ "$1 $2" = "release list" ]; then printf '%s\\n' {" ".join(self.tags)}; exit 0; fi
if [ "$1 $2" = "release download" ]; then
  while [ $# -gt 0 ]; do
    case "$1" in
      -D) dir=$2; shift 2 ;;
      -p) f="{self.release}/$2"; [ -f "$f" ] || exit 1; cp "$f" "$dir/"; shift 2 ;;
      *) shift ;;
    esac
  done
  exit 0
fi
exit 2
""")
        sign = self.tool("sign", """
if [ "$1" = key ]; then echo secret > "$2"; echo "public key:  abcd"; exit 0; fi
if [ "$1" = sign ]; then cp "$2" "$3"; exit 0; fi
exit 2
""")
        run = self.tool("run-test", f'echo "$PAPERPAW_UPDATE_DEVICE" >> {self.log}\n{test}\n')
        return subprocess.run(
            [str(HERE / "ci" / "update-path.sh"), str(self.images), str(self.work)],
            env={**os.environ, "GH": str(gh), "SIGN": str(sign), "UPDATE_PATH_TEST": str(run)},
            capture_output=True, text=True,
        )

    def devices_run(self):
        return self.log.read_text().split() if self.log.exists() else []

    def test_every_device_with_an_update_is_tested(self):
        for device in ("x4", "x3", "x4-pro", "x4-classic"):
            self.asset(device)
        result = self.run_job()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.devices_run(), ["x4", "x3", "x4-pro", "x4-classic"])

    def test_devices_added_after_the_release_are_only_warned_about(self):
        self.asset("x4")
        result = self.run_job()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.devices_run(), ["x4"])
        self.assertIn("::warning::v0.5.0-beta.11 has no update for the x3", result.stdout)

    def test_no_update_of_any_device_fails(self):
        result = self.run_job()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("::error::", result.stdout)
        self.assertEqual(self.devices_run(), [])

    def test_a_missing_x4_update_fails_though_the_others_are_there(self):
        for device in ("x3", "x4-pro", "x4-classic"):
            self.asset(device)
        result = self.run_job()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no update for the x4", result.stdout)
        self.assertEqual(self.devices_run(), [])

    def test_a_failing_test_fails(self):
        self.asset("x4")
        self.assertNotEqual(self.run_job(test="exit 1").returncode, 0)

    def test_a_damaged_download_fails(self):
        self.asset("x4")
        (self.release / "paperpaw-x4-0.5.0-beta.11-update.bin").write_bytes(b"changed")
        result = self.run_job()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.devices_run(), [])

    def test_no_release_before_fails(self):
        self.tags = ["v9.9.9"]
        self.asset("x4")
        result = self.run_job()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no release of PaperPaw before", result.stdout)


if __name__ == "__main__":
    unittest.main()
