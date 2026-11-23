# Proposal

## Why

envedit is a single self-contained executable, but today it only runs from `target\release`. Launching it should be one step (Start menu, Win+R, or a terminal), which was the point of building it, and removing it should leave nothing behind.

## What Changes

- New `scripts/install.ps1`: builds the release binary (or takes a given `.exe`), installs it per user into `%LOCALAPPDATA%\Programs\envedit` without needing admin rights, and makes it launchable:
  - Start menu shortcut `envedit`
  - App Paths registration so Win+R → `envedit` works
  - the install folder appended to the User `Path`, so `envedit` works in new terminals
- Re-running the script upgrades the installed binary in place.
- New `scripts/uninstall.ps1`, also copied into the install folder: removes the binary, shortcut, App Paths registration and the `Path` entry. Snapshots in `%LOCALAPPDATA%\envedit` are user data and are kept.
- `Path` is edited the way envedit itself edits it: registry value type preserved, no duplicate entry, length limit respected, change broadcast to running programs, prior value backed up as an envedit snapshot.
- Out of scope: MSI, winget package, code signing, machine-wide installation.

## Capabilities

### New Capabilities

- `install-script`: Per-user installation, upgrade and removal of envedit with PowerShell, including Start menu, Win+R and terminal launch.

### Modified Capabilities

None.

## Impact

- New files: `scripts/install.ps1`, `scripts/uninstall.ps1`, a shared helper script, and a test script for the `Path` handling.
- Writes per user only: `%LOCALAPPDATA%\Programs\envedit`, the Start menu folder, `HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\envedit.exe`, and `HKCU\Environment\Path`.
- Building from source needs the Rust toolchain; installing a given `.exe` does not.
- No change to the envedit binary.
