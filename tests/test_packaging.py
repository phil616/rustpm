"""Packaging checks without installing services or changing the host PATH.

Run on macOS/Linux: python3 -m unittest discover -s tests -v
"""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[1]


class MsiAuthoringTests(unittest.TestCase):
    def test_path_is_appended_by_a_new_machine_component(self):
        ns = {"w": "http://wixtoolset.org/schemas/v4/wxs"}
        package = ET.parse(ROOT / "wix/main.wxs").getroot().find("w:Package", ns)
        self.assertEqual(package.get("Scope"), "perMachine")
        group = package.find("w:ComponentGroup[@Id='CliEntryPoint']", ns)
        self.assertEqual(group.get("Directory"), "CliDir")
        env = group.find("w:Component/w:Environment", ns)
        for key, value in dict(Name="PATH", Value="[CliDir]", Action="set",
                               Part="last", System="yes", Permanent="no").items():
            self.assertEqual(env.get(key), value)
        component = group.find("w:Component[@Id='cmpRustpmctlPath']", ns)
        self.assertEqual(component.find("w:RegistryValue", ns).get("KeyPath"), "yes")
        self.assertIsNotNone(package.find("w:Feature/w:ComponentGroupRef[@Id='CliEntryPoint']", ns))
        # Preserve the shipped CLI's component identity and location for upgrades.
        original = package.find("w:ComponentGroup[@Id='Binaries']/w:Component[@Id='cmpRustpmctlExe']", ns)
        self.assertEqual(original.get("Guid"), "{22345678-ABCD-4DEF-9012-34567890ABCD}")
        self.assertEqual(original.find("w:File", ns).get("Id"), "rustpmctlExe")


@unittest.skipUnless(os.name == "posix", "requires POSIX shell and symlinks")
class MacPackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="rustpm packaging ")
        self.addCleanup(self.temp.cleanup)
        self.volume = Path(self.temp.name)
        self.link = self.volume / "usr/local/bin/rustpmctl"
        self.link.parent.mkdir(parents=True)

    def preinstall(self):
        return subprocess.run(["sh", str(ROOT / "packaging/preinstall"),
                               "test.pkg", "/", str(self.volume)], capture_output=True)

    def test_preinstall_allows_new_install_and_reinstall(self):
        self.assertEqual(self.preinstall().returncode, 0)
        # A dangling package-owned link is valid too (repair after lost binary).
        self.link.symlink_to("../lib/rustpm/rustpmctl")
        self.assertEqual(self.preinstall().returncode, 0)
        self.assertEqual(os.readlink(self.link), "../lib/rustpm/rustpmctl")

    def test_preinstall_preserves_conflicting_entries(self):
        for kind in ("file", "directory", "symlink"):
            with self.subTest(kind=kind):
                if kind == "file":
                    self.link.write_text("unrelated command")
                elif kind == "directory":
                    self.link.mkdir()
                else:
                    self.link.symlink_to("/missing/unrelated-command")
                result = self.preinstall()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b"refusing to replace", result.stderr)
                self.assertTrue(os.path.lexists(self.link))
                if kind == "directory":
                    self.link.rmdir()
                else:
                    self.link.unlink()

    def uninstall(self, dry_run=False):
        # Redirect every filesystem operation into the temporary volume. Stub
        # privilege/service commands so tests never touch host services.
        mock_bin = self.volume / "mocks"
        mock_bin.mkdir(exist_ok=True)
        for command, body in (("id", "echo 0"), ("launchctl", "exit 0")):
            mock = mock_bin / command
            mock.write_text("#!/bin/sh\n" + body + "\n")
            mock.chmod(0o755)
        script = self.volume / "uninstall.sh"
        content = (ROOT / "scripts/uninstall_rustpm.sh").read_text()
        for prefix in ("/usr/local", "/Library/LaunchDaemons"):
            content = content.replace(prefix, "." + prefix)
        script.write_text(content)
        env = dict(os.environ, PATH=str(mock_bin) + os.pathsep + os.environ["PATH"])
        args = ["sh", str(script), "--yes"]
        if dry_run:
            args.append("--dry-run")
        subprocess.run(args, cwd=self.volume, env=env, check=True, capture_output=True)

    def test_uninstall_removes_only_expected_link(self):
        for target in ("../lib/rustpm/rustpmctl", "/unrelated/rustpmctl"):
            with self.subTest(target=target):
                self.link.symlink_to(target)
                self.uninstall(dry_run=True)
                self.assertTrue(self.link.is_symlink())
                self.uninstall()
                self.assertEqual(self.link.is_symlink(), target != "../lib/rustpm/rustpmctl")
                if self.link.is_symlink():
                    self.link.unlink()
        self.link.write_text("unrelated")
        self.uninstall()
        self.assertEqual(self.link.read_text(), "unrelated")

    def test_pkg_stages_a_working_relative_link(self):
        # Execute the real staging script in a fixture repository; replace only
        # cargo and pkgbuild, which require a toolchain and a macOS host.
        repo = self.volume / "repo"
        for directory in ("scripts", "packaging", "config"):
            shutil.copytree(ROOT / directory, repo / directory)
        shutil.copyfile(ROOT / "Cargo.toml", repo / "Cargo.toml")
        cargo = repo / "scripts/cargo-local.sh"
        cargo.write_text("#!/bin/sh\nexit 0\n")
        cargo.chmod(0o755)
        release = repo / "target/release"
        release.mkdir(parents=True)
        for binary in ("rustpmd", "rustpm-privd", "rustpmctl"):
            executable = release / binary
            executable.write_text("#!/bin/sh\necho cli-ok\n")
            executable.chmod(0o755)
        mock_bin = self.volume / "mock-pkgbuild"
        mock_bin.mkdir()
        pkgbuild = mock_bin / "pkgbuild"
        pkgbuild.write_text("#!/bin/sh\nexit 0\n")
        pkgbuild.chmod(0o755)
        env = dict(os.environ, PATH=str(mock_bin) + os.pathsep + os.environ["PATH"])
        env.pop("VERSION", None)
        subprocess.run(["sh", str(repo / "scripts/build_pkg.sh")], env=env,
                       check=True, capture_output=True)
        staged = repo / "target/pkg-root/usr/local/bin/rustpmctl"
        self.assertEqual(os.readlink(staged), "../lib/rustpm/rustpmctl")
        self.assertEqual(subprocess.check_output([str(staged)]).strip(), b"cli-ok")
        self.assertTrue(os.access(repo / "packaging/preinstall", os.X_OK))


if __name__ == "__main__":
    unittest.main()
