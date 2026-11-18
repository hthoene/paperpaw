"""Offline tests of CI setup: cache reuse, authenticated archives and apt fallback."""

import contextlib
import hashlib
import io
import os
import pathlib
import re
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
import prune_caches  # noqa: E402


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
        self.sleep = self.patches.enter_context(mock.patch.object(espflash.time, "sleep"))
        self.build = self.patches.enter_context(mock.patch.object(
            espflash, "build_from_crates", side_effect=ValueError("cargo install failed")))
        self.patches.enter_context(mock.patch.dict(espflash.os.environ))
        espflash.os.environ.pop("ESPFLASH_ARCHIVE_DIR", None)
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

    def gateway_timeout(self, code=504):
        return espflash.urllib.error.HTTPError("https://example.org/e.zip", code, "Gateway Time-out", {}, None)

    def test_a_gateway_timeout_is_tried_again_with_growing_waits(self):
        good = self.fetch.return_value
        self.fetch.side_effect = [self.gateway_timeout(), self.gateway_timeout(502), good]
        self.assertTrue(espflash.install("4.6.0", self.destination).is_file())
        self.assertEqual(self.fetch.call_count, 3)
        waits = [call.args[0] for call in self.sleep.call_args_list]
        self.assertEqual(len(waits), 2)
        self.assertGreaterEqual(waits[1], 2 * espflash.FIRST_WAIT)

    def test_a_cut_short_download_is_fetched_again(self):
        good = self.fetch.return_value
        self.fetch.side_effect = [espflash.http.client.IncompleteRead(good[:10], 100), good]
        self.assertTrue(espflash.install("4.6.0", self.destination).is_file())
        self.assertEqual(self.fetch.call_count, 2)

    def test_an_archive_that_is_not_the_pinned_one_fails_at_once(self):
        self.fetch.return_value += b"changed"
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            espflash.install("4.6.0", self.destination)
        self.fetch.assert_called_once()
        self.sleep.assert_not_called()

    def test_it_gives_up_after_the_attempts_and_says_so(self):
        self.fetch.side_effect = self.gateway_timeout()
        with self.assertRaisesRegex(ValueError, "504.*after 6 attempts.*fallback failed too"):
            espflash.install("4.6.0", self.destination)
        self.assertEqual(self.fetch.call_count, espflash.ATTEMPTS)
        self.build.assert_called_once()
        self.assertEqual(self.sleep.call_count, espflash.ATTEMPTS - 1)
        self.matches.assert_not_called()

    def test_a_missing_release_is_not_tried_again_but_built_from_crates_io(self):
        self.fetch.side_effect = self.gateway_timeout(404)
        self.build.side_effect = None
        self.build.return_value = self.destination / "espflash"
        self.assertEqual(espflash.install("4.6.0", self.destination), self.destination / "espflash")
        self.fetch.assert_called_once()
        self.sleep.assert_not_called()

    def test_when_the_release_is_down_espflash_is_built_from_crates_io(self):
        self.fetch.side_effect = self.gateway_timeout()
        self.build.side_effect = None
        self.build.return_value = self.destination / "espflash"
        espflash.os.environ["ESPFLASH_ARCHIVE_DIR"] = str(self.destination.parent / "cache")
        self.assertEqual(espflash.install("4.6.0", self.destination), self.destination / "espflash")
        self.assertEqual(self.fetch.call_count, espflash.ATTEMPTS)
        self.build.assert_called_once_with("4.6.0", self.destination, str(self.destination.parent / "cache"))

    def test_a_wrong_archive_never_falls_back_to_a_build(self):
        self.fetch.return_value += b"changed"
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            espflash.install("4.6.0", self.destination)
        self.build.assert_not_called()

    def kept_build(self):
        cache = self.destination.parent / "cache"
        cache.mkdir()
        (cache / "espflash-cargo-4.6.0").write_bytes(b"built earlier")
        espflash.os.environ["ESPFLASH_ARCHIVE_DIR"] = str(cache)

    def test_the_verified_release_comes_before_a_build_kept_from_crates_io(self):
        self.kept_build()
        self.assertEqual(espflash.install("4.6.0", self.destination).read_bytes(), b"verified executable")
        self.fetch.assert_called_once()
        self.build.assert_not_called()

    def test_a_build_kept_by_an_earlier_run_is_used_when_the_release_is_down(self):
        self.kept_build()
        self.fetch.side_effect = self.gateway_timeout()
        self.assertEqual(espflash.install("4.6.0", self.destination).read_bytes(), b"built earlier")
        self.assertEqual(self.fetch.call_count, espflash.ATTEMPTS)
        self.build.assert_not_called()

    def test_a_connection_that_breaks_is_tried_again(self):
        good = self.fetch.return_value
        self.fetch.side_effect = [espflash.http.client.IncompleteRead(b"x"), TimeoutError("slow"), good]
        self.assertTrue(espflash.install("4.6.0", self.destination).is_file())

    def test_a_downloaded_archive_is_kept_and_then_reused_without_a_download(self):
        cache = self.destination.parent / "archive-cache"
        espflash.os.environ["ESPFLASH_ARCHIVE_DIR"] = str(cache)
        espflash.install("4.6.0", self.destination)
        self.assertTrue((cache / "espflash.zip").is_file())
        self.fetch.reset_mock()
        shutil.rmtree(self.destination)
        self.assertTrue(espflash.install("4.6.0", self.destination).is_file())
        self.fetch.assert_not_called()

    def test_a_cached_archive_that_is_not_the_pinned_one_is_replaced(self):
        cache = self.destination.parent / "archive-cache"
        cache.mkdir()
        (cache / "espflash.zip").write_bytes(b"damaged")
        espflash.os.environ["ESPFLASH_ARCHIVE_DIR"] = str(cache)
        self.assertTrue(espflash.install("4.6.0", self.destination).is_file())
        self.fetch.assert_called_once()
        self.assertEqual((cache / "espflash.zip").read_bytes(), self.fetch.return_value)

    def test_an_archive_failing_its_pin_is_never_cached(self):
        cache = self.destination.parent / "archive-cache"
        espflash.os.environ["ESPFLASH_ARCHIVE_DIR"] = str(cache)
        self.fetch.return_value += b"changed"
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            espflash.install("4.6.0", self.destination)
        self.assertFalse(cache.exists())

    def test_version_updates_require_a_reviewed_pin(self):
        with self.assertRaisesRegex(ValueError, "no reviewed"):
            espflash.install("4.7.0", self.destination)
        self.fetch.assert_not_called()


class ActionsArePinned(unittest.TestCase):
    """Every third-party action is named by a full commit SHA, with the version as a comment."""

    USES = re.compile(r"^\s*(?:-\s*)?uses:\s*(?P<action>[^\s#]+)(?P<comment>\s*#.*)?$")

    def test_no_action_is_named_by_a_tag_or_branch(self):
        problems = []
        for path in sorted((HERE.parent / ".github").rglob("*.yml")):
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                found = self.USES.match(line)
                if not found or found["action"].startswith("./"):
                    continue
                name, _, ref = found["action"].partition("@")
                if not re.fullmatch(r"[0-9a-f]{40}", ref) or not found["comment"]:
                    problems.append(f"{path.relative_to(HERE.parent)}:{number}: {found['action']}")
        self.assertEqual(problems, [], "pin third-party actions by full SHA with a '# vX.Y.Z' comment")


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

    def stalled_mirror(self):
        """clang-18 is missing and apt never gets it (a timeout, 124)."""
        (self.bin / "clang-18").unlink()
        self.write("sudo", "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CALLS\"\nexit 124\n")

    def test_a_stalled_mirror_falls_back_to_the_runners_newest_clang_if_allowed(self):
        self.stalled_mirror()
        for number in (17, 19, 21):
            self.write(f"clang-{number}", self.template.read_text())
        result = self.run_setup(MBEDTLS_CLANG_FALLBACK="runner")
        self.assertEqual(result.returncode, 0, result.stderr)
        # The newest of at least 18; said in the log, and what the build is given.
        self.assertIn("::warning title=clang::", result.stdout)
        self.assertIn("clang-21 instead", result.stdout)
        self.assertIn("CLANG_PATH=" + self.shell_path(self.bin) + "/clang-21", (self.root / "env").read_text())
        # Two shorter tries, not three: the update never worked.
        guard = "timeout --kill-after=10"
        self.assertEqual(self.calls(), [f"{guard} 60 apt-get {self.OPTIONS} update"] * 2)

    def test_a_release_keeps_to_the_named_clang_whatever_the_runner_has(self):
        self.stalled_mirror()
        self.write("clang-21", self.template.read_text())
        result = self.run_setup()
        self.assertEqual(result.returncode, 124)
        self.assertEqual(len(self.calls()), 3)
        self.assertFalse((self.root / "env").exists())

    def test_the_fallback_is_not_used_where_apt_works(self):
        (self.bin / "clang-18").unlink()
        self.write("clang-21", self.template.read_text())
        result = self.run_setup(MBEDTLS_CLANG_FALLBACK="runner")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("::warning", result.stdout)
        self.assertIn("/clang-18", (self.root / "env").read_text())

    def test_the_fallback_needs_a_clang_of_version_18_or_later_and_cmake_and_ninja(self):
        self.stalled_mirror()
        self.write("clang-17", self.template.read_text())
        self.assertEqual(self.run_setup(MBEDTLS_CLANG_FALLBACK="runner").returncode, 124)
        self.write("clang-20", self.template.read_text())
        (self.bin / "ninja").unlink()
        self.assertEqual(self.run_setup(MBEDTLS_CLANG_FALLBACK="runner").returncode, 124)
        self.assertFalse((self.root / "env").exists())

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


class PruneCaches(unittest.TestCase):
    def entry(self, number, key, ref="refs/heads/abi7/integration", created="2026-11-09T08:00:00Z"):
        return {"id": number, "key": key, "ref": ref, "created_at": created, "size_in_bytes": 1}

    def test_the_older_entry_of_a_prefix_goes_and_the_newest_stays(self):
        entries = [
            self.entry(1, "v0-rust-x4-pro-firmware-Linux-x64-6d4f8e32-6f6f3ce5", created="2026-11-09T07:00:00Z"),
            self.entry(2, "v0-rust-x4-pro-firmware-Linux-x64-6d4f8e32-8629a0a7", created="2026-11-09T10:00:00Z"),
            self.entry(3, "v0-rust-x3-firmware-Linux-x64-c942fa7c-8629a0a7"),
        ]
        self.assertEqual(prune_caches.stale(entries), [1])

    def test_other_caches_and_other_branches_are_never_touched(self):
        entries = [
            self.entry(1, "esp-Linux-1.97.0.0-14.2.0_20241119-0.17.1", created="2026-11-09T07:00:00Z"),
            self.entry(2, "esp-Linux-1.97.0.0-14.2.0_20241119-0.17.2"),
            self.entry(3, "apt-debs-Linux-clang-18-b265fc2d", created="2026-11-09T07:00:00Z"),
            self.entry(4, "apt-debs-Linux-clang-18-b265fc2e"),
            self.entry(5, "v0-rust-check-Linux-x64-fc21961f-79722096", ref="refs/heads/main", created="2026-11-01T00:00:00Z"),
            self.entry(6, "v0-rust-check-Linux-x64-fc21961f-79722096"),
            self.entry(7, "v0-rust-odd-key"),
        ]
        self.assertEqual(prune_caches.stale(entries), [])

    def test_a_job_name_that_looks_like_another_is_its_own_prefix(self):
        entries = [
            self.entry(1, "v0-rust-x4-firmware-Linux-x64-c942fa7c-8629a0a7"),
            self.entry(2, "v0-rust-x4-pro-firmware-Linux-x64-6d4f8e32-8629a0a7"),
        ]
        self.assertEqual(prune_caches.stale(entries), [])


class FirmwareRemap(unittest.TestCase):
    """tools/ci/firmware-remap.sh: the released firmware's path remapping (#258)."""

    def setUp(self):
        self.bash = shutil.which("bash")
        if os.name == "nt" or not self.bash:
            self.skipTest("bash is needed for the Linux build's script")
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.home = pathlib.Path(self.directory.name) / "cargo"
        (self.home / "registry" / "src" / "index.crates.io-0123").mkdir(parents=True)

    def run_script(self, *arguments, home=None, sysroot="/opt/rust"):
        environment = dict(os.environ, CARGO_HOME=str(home or self.home), FIRMWARE_SYSROOT=sysroot)
        return subprocess.run(
            [self.bash, str(HERE / "ci" / "firmware-remap.sh"), *arguments],
            env=environment, capture_output=True, text=True, check=False,
        )

    def test_the_repository_the_registry_and_the_toolchain_lose_their_folders(self):
        done = self.run_script("riscv32imc-unknown-none-elf")
        self.assertEqual(done.returncode, 0, done.stderr)
        key, _, value = done.stdout.strip().partition("=")
        self.assertEqual(key, "target.riscv32imc-unknown-none-elf.rustflags")
        flags = re.findall(r'"--remap-path-prefix=([^"=]*)=([^"]*)"', value)
        self.assertEqual(value, "[" + ",".join(f'"--remap-path-prefix={a}={b}"' for a, b in flags) + "]")
        # rustc takes the last prefix that matches: the cargo home's own
        # folders come after the home itself.
        self.assertEqual(flags, [
            (str(self.home), "cargo"),
            (f"{self.home}/registry/src/index.crates.io-0123/", ""),
            ("/opt/rust/lib/rustlib/src/rust/", ""),
            (f"{HERE.parent}/", ""),
        ])

    def test_a_path_that_would_break_the_flag_is_refused(self):
        for name in ['with"quote', "with=sign", "with\\slash"]:
            home = pathlib.Path(self.directory.name) / name
            home.mkdir()
            done = self.run_script("riscv32imc-unknown-none-elf", home=home)
            self.assertEqual(done.returncode, 1, name)
            self.assertEqual(done.stdout, "")
            self.assertIn("cannot remap", done.stderr)

    def test_the_target_is_needed(self):
        self.assertEqual(self.run_script().returncode, 2)

    def test_the_released_and_the_checked_build_use_it(self):
        for workflow in ["release.yml", "ci.yml"]:
            text = (HERE.parent / ".github" / "workflows" / workflow).read_text(encoding="utf-8")
            self.assertIn('tools/ci/firmware-remap.sh" "$FIRMWARE_TARGET" "$FIRMWARE_TOOLCHAIN")', text)
            self.assertIn('--config "$remap" --features "$DEVICE', text)


if __name__ == "__main__":
    unittest.main()
