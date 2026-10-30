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
    def setUp(self):
        self.bash = (r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else shutil.which("bash"))
        if not self.bash or not pathlib.Path(self.bash).is_file():
            self.skipTest("bash is needed for the Linux action's shell tests")
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = pathlib.Path(self.directory.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.template = self.root / "tool"
        self.template.write_text("#!/bin/sh\nprintf 'fixture tool\\n'\n", encoding="utf-8")
        for tool in ["cmake", "ninja", "clang-18"]:
            self.write(tool, self.template.read_text())
        self.write("sudo", """#!/bin/sh
printf '%s\n' "$*" >> "$CALLS"
if [ "$1 $2" = 'apt-get install' ]; then
  for package in "$@"; do
    case "$package" in
      cmake) tool=cmake;; ninja-build) tool=ninja;; clang-18) tool=clang-18;; *) continue;;
    esac
    /bin/cp "$TOOL_TEMPLATE" "$FAKE_BIN/$tool"
    /bin/chmod +x "$FAKE_BIN/$tool"
  done
fi
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

    def run_setup(self):
        env = os.environ | {
            "PATH": self.shell_path(self.bin), "MBEDTLS_CLANG_PACKAGE": "clang-18",
            "GITHUB_ENV": self.shell_path(self.root / "env"),
            "CALLS": self.shell_path(self.root / "calls"),
            "FAKE_BIN": self.shell_path(self.bin), "TOOL_TEMPLATE": self.shell_path(self.template),
        }
        return subprocess.run([self.bash, "--noprofile", "--norc", str(HERE / "ci/build-prerequisites.sh")],
                              env=env, capture_output=True, text=True)

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
        self.assertEqual((self.root / "calls").read_text().splitlines(),
                         ["apt-get update", "apt-get install -y ninja-build"])

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
