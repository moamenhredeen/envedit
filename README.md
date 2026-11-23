# envedit

A fast, native editor for Windows environment variables. One window instead of
the nested System Properties dialogs, no 2047-character limit on values such as
a long User `Path`, staged changes with undo, and a snapshot before every write.

- Rust + Win32, a single ~350 KB executable without runtime dependencies
- User and Machine variables; Machine changes ask for administrator approval
  (UAC) only when you apply them
- Markers for missing directories, duplicate entries and `%VAR%` expansion

## Install

Per user, no administrator rights needed. From a clone of this repository:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

This builds the release binary with cargo (Rust toolchain required). To install
an already built `envedit.exe` instead:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -ExePath C:\path\to\envedit.exe
```

The script works in Windows PowerShell 5.1 and PowerShell 7. It:

| Changes | Where |
|---|---|
| Copies `envedit.exe`, `uninstall.ps1` and `envedit-install-lib.ps1` | `%LOCALAPPDATA%\Programs\envedit` |
| Creates the Start menu shortcut `envedit` | `%APPDATA%\Microsoft\Windows\Start Menu\Programs` |
| Registers `envedit` for Win+R (App Paths) | `HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\envedit.exe` |
| Appends the install folder to the User `Path` (once, registry type kept) | `HKCU\Environment` |

Before changing `Path`, the previous User variables are saved as an envedit
snapshot in `%LOCALAPPDATA%\envedit\snapshots`. Run the script again to upgrade;
close envedit first.

## Start

- Start menu: search `envedit`
- Win+R: `envedit`
- Terminal (new ones after install): `envedit`, or `envedit path` to open with `Path` selected

## Uninstall

```powershell
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\envedit\uninstall.ps1"
```

(`scripts\uninstall.ps1` in the repository does the same.) It removes everything
listed above, including the `Path` entry, and keeps the snapshots in
`%LOCALAPPDATA%\envedit`.

## Development

```powershell
cargo test                                                        # core unit tests
cargo build --release                                             # target\release\envedit.exe
powershell -ExecutionPolicy Bypass -File scripts\test-install-lib.ps1  # install script tests
```

Changes are planned with [OpenSpec](https://github.com/Fission-AI/OpenSpec); specs live in `openspec/specs/`.
