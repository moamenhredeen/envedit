# Tasks

## 1. Shared helper

- [x] 1.1 Create `scripts/envedit-install-lib.ps1` with raw `Path` read/write on a parameterized key (value kind preserved, `DoNotExpandEnvironmentNames`), entry comparison per design D2, add/remove entry functions that return whether anything changed, and the 32,767 limit check; verify by dot-sourcing it in PowerShell 5.1 and 7 without errors
- [x] 1.2 Add the snapshot writer (envedit-records format, UTC name, per design D3) and the `WM_SETTINGCHANGE` broadcast (design D4); verify that a snapshot written by the helper is listed and decoded by envedit's format (header, `var` records, escaping) with a test in 1.3
- [x] 1.3 Create `scripts/test-install-lib.ps1` that runs against `HKCU\Software\envedit-test` and checks: append to `REG_SZ` and `REG_EXPAND_SZ` keeps the kind, no duplicate on second add (case and trailing `\` variants), remove keeps other entries and order, missing `Path` is created as `REG_EXPAND_SZ`, over-limit append is refused, snapshot content; verify it passes in PowerShell 5.1 and 7 and removes the scratch key

## 2. Install

- [x] 2.1 Create `scripts/install.ps1`: elevation warning, binary from `cargo build --release` or `-ExePath`, failures before any change exit non-zero; verify with a missing `-ExePath` (non-zero exit, nothing created)
- [x] 2.2 Add running-instance check, copy via temporary name, and copy of `uninstall.ps1` plus helper into the install folder (design D6); verify by running install while the installed envedit is open (non-zero exit, binary unchanged)
- [x] 2.3 Add Start menu shortcut and App Paths registration (design D5); verify the `.lnk` target and the App Paths default value point to the installed exe
- [x] 2.4 Add the User `Path` step with snapshot and broadcast, and the summary output; verify after install that `Path` kind is unchanged, the install folder appears once, a snapshot was written, and `envedit` resolves in a new terminal (`Get-Command envedit`)

## 3. Uninstall

- [x] 3.1 Create `scripts/uninstall.ps1` reversing install (running-instance check, `Path` entry removal with snapshot and broadcast, App Paths, shortcut, install folder; snapshots kept); verify after uninstall that all install artifacts are gone, `Path` holds the other entries in their previous order with the same kind, and `%LOCALAPPDATA%\envedit\snapshots` still exists
- [x] 3.2 Make uninstall succeed when nothing is installed; verify a second uninstall run exits 0 with a "nothing installed" message

## 4. Integration

- [x] 4.1 Run install → reinstall (upgrade) → uninstall in both Windows PowerShell 5.1 and PowerShell 7; verify the `Path` value afterwards equals the value before install, with the same kind
- [x] 4.2 Verify Win+R → `envedit` and the Start menu entry start the installed envedit (manual check by the user)
- [x] 4.3 Document installation and removal in a `README.md` section (commands including `-ExecutionPolicy Bypass`, `-ExePath`, what gets changed); verify the documented commands match the scripts' parameters
