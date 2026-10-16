# Package command discovery validation

Related issue: <https://github.com/phil616/rustpm/issues/1>.

Automated checks (no installation or host PATH changes):

```sh
python3 -m unittest discover -s tests -v
shellcheck packaging/preinstall scripts/build_pkg.sh scripts/uninstall_rustpm.sh
```

The Python tests run on Linux and macOS in `packaging.yml`. They execute pkg
staging with mocked build tools, check conflict detection on a target volume,
and exercise uninstall and dry-run behavior against temporary files.

On Windows, build and inspect the actual MSI:

```powershell
.\scripts\build_msi.ps1
.\scripts\test_msi.ps1 -Msi dist\rustpm-0.1.5-x64.msi
```

The release workflow also inspects the MSI before publication. These checks
validate the compiled Environment table, component ownership, CLI directory
and native environment actions. They do not establish installer lifecycle behavior.

Windows uses the native [Environment table](https://learn.microsoft.com/en-us/windows/win32/msi/environment-table)
through WiX's [Environment element](https://docs.firegiant.com/wix/schema/wxs/environment/).
The new `cli` directory is reserved for package management, keeping existing
manual PATH entries for the installation root outside MSI PATH ownership.
Native MSI does not track who added an identical PATH segment; do not manually
register the package-owned `cli` directory. Its component GUIDs must remain
stable in subsequent releases so late major-upgrade removal retains the new
product's shared component.

Before releasing, use disposable Windows/macOS machines for these checks:

| Scenario | Expected result |
| --- | --- |
| Clean MSI install, interactive and silent | Fresh PowerShell and cmd find the packaged CLI from another directory; existing PATH entries remain |
| Upgrade from released 0.1.4, then another version | CLI directory is appended once; removal of the older product does not remove the shared PATH component |
| Repair/reinstall | No duplicated PATH entry; missing CLI copy is restored |
| Custom installation directory, spaces, long PATH | Correct unquoted directory, with unrelated PATH data preserved |
| Manual PATH entries for original installation root, including case/slash variations | Original entries remain after uninstall; MSI manages only its dedicated `cli` directory |
| Failed install / rollback | Prior PATH is restored |
| Normal and silent uninstall | Package-owned CLI directory and PATH segment removed; other PATH segments retained |
| Another command earlier on PATH | Earlier command retains priority; `Get-Command rustpmctl -All` identifies both |
| New versus existing terminal | Restart parent terminal processes to pick up the new environment |
| Standard versus administrator identity | Discovery works for both; protected control operations still require authorization |
| Both Windows services | Installation, startup, recovery and removal still work; Program Files remains protected against ordinary-user writes |
| macOS pkg clean install / reinstall / upgrade | Receipt includes `/usr/local/bin/rustpmctl`; relative link resolves to the installed CLI |
| macOS conflicting file, directory or symlink | Preinstall refuses replacement, leaving the existing entry intact |
| macOS uninstall and dry-run | Only the expected relative link is eligible for removal; unrelated replacements survive |
| macOS shell or sudo PATH omits `/usr/local/bin` | Full-path invocation works; no profile edits are required |

The macOS preinstall guard runs before payload extraction. As with other
installer preflight checks, it assumes no concurrent process replaces the
destination between validation and extraction.
