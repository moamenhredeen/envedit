# install-script Specification

## Purpose

Defines how envedit is installed, upgraded and removed per user with PowerShell scripts, so it can be launched from the Start menu, Win+R and terminals, and removed without leaving anything behind.

## Requirements

### Requirement: Per-user install without elevation
`install.ps1` SHALL install envedit for the current user into `%LOCALAPPDATA%\Programs\envedit` and SHALL NOT require administrator rights. When run elevated, it SHALL warn that it installs for the account it runs as and continue.

#### Scenario: Standard user install
- **WHEN** a standard user runs `install.ps1` from a non-elevated PowerShell
- **THEN** `envedit.exe` exists in `%LOCALAPPDATA%\Programs\envedit` and no UAC prompt appeared

#### Scenario: Elevated run
- **WHEN** `install.ps1` runs in an elevated PowerShell
- **THEN** it prints a warning naming the account it installs for, then installs

### Requirement: Binary source
By default, `install.ps1` SHALL build the release binary from the repository with cargo and install it. With `-ExePath <file>`, it SHALL install that file instead and SHALL NOT require cargo. If the build fails or the given file does not exist, it SHALL stop before changing anything and exit with a non-zero code.

#### Scenario: Build from source
- **WHEN** `install.ps1` runs in a clone with the Rust toolchain installed
- **THEN** it builds the release binary and installs it

#### Scenario: Prebuilt binary
- **WHEN** `install.ps1 -ExePath C:\Downloads\envedit.exe` runs on a machine without cargo
- **THEN** that file is installed

#### Scenario: Missing binary
- **WHEN** `-ExePath` names a file that does not exist
- **THEN** the script exits non-zero and nothing is installed or changed

### Requirement: Launch entry points
`install.ps1` SHALL create a Start menu shortcut named `envedit` and SHALL register `envedit.exe` under `HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths` pointing to the installed binary.

#### Scenario: Start menu
- **WHEN** the install has finished
- **THEN** searching `envedit` in the Start menu finds and starts it

#### Scenario: Win+R
- **WHEN** the user types `envedit` in the Win+R dialog
- **THEN** the installed envedit starts

### Requirement: User Path entry
`install.ps1` SHALL append the install folder to the User `Path` unless an equal entry already exists (compared case-insensitively, after expanding `%VAR%` references, ignoring a trailing `\`). The write SHALL keep the registry value type of `Path`, SHALL NOT be made if the result would exceed 32,767 characters, and SHALL be followed by a `WM_SETTINGCHANGE` broadcast. Before changing `Path`, the script SHALL save the previous User variables as an envedit snapshot in `%LOCALAPPDATA%\envedit\snapshots`. If `Path` does not exist, it SHALL be created as `REG_EXPAND_SZ` holding only the install folder.

#### Scenario: Terminal launch
- **WHEN** the install has finished and the user opens a new terminal
- **THEN** `envedit` starts envedit

#### Scenario: Type preserved
- **WHEN** User `Path` is `REG_SZ` before install
- **THEN** it is still `REG_SZ` after install, with the install folder as its last entry

#### Scenario: No duplicate on reinstall
- **WHEN** `install.ps1` runs a second time
- **THEN** User `Path` contains the install folder exactly once

#### Scenario: Limit
- **WHEN** appending would make User `Path` longer than 32,767 characters
- **THEN** `Path` is left unchanged, the script warns that terminal launch is not set up, and the other steps still complete

### Requirement: Upgrade in place
Running `install.ps1` when envedit is already installed SHALL replace the installed binary and keep the shortcut, App Paths registration and `Path` entry. If the installed envedit is running, the script SHALL stop before changing anything, tell the user to close it, and exit non-zero.

#### Scenario: Upgrade
- **WHEN** a newer build is installed over an existing install
- **THEN** the installed binary is the new one and launch entry points still work

#### Scenario: Running instance
- **WHEN** envedit is open while `install.ps1` runs
- **THEN** the script exits non-zero without changes and asks to close envedit

### Requirement: Uninstall
`uninstall.ps1` SHALL be available both in the repository and in the install folder. It SHALL remove the installed binary and folder, the Start menu shortcut, the App Paths registration and the install folder's `Path` entry, keeping the registry value type of `Path`, snapshotting before the change and broadcasting it afterwards. It SHALL keep `%LOCALAPPDATA%\envedit` (snapshots). Running it when envedit is not installed SHALL succeed and report that nothing was installed. If envedit is running, it SHALL stop before changing anything and exit non-zero.

#### Scenario: Clean removal
- **WHEN** the user runs `uninstall.ps1` after an install
- **THEN** the install folder, shortcut, App Paths key and `Path` entry are gone, and snapshots remain

#### Scenario: Other Path entries untouched
- **WHEN** User `Path` holds other entries besides the install folder
- **THEN** after uninstall those entries are unchanged and in the same order

#### Scenario: Not installed
- **WHEN** `uninstall.ps1` runs and envedit is not installed
- **THEN** it reports that nothing was installed and exits with code 0

### Requirement: Clear output
Both scripts SHALL print each step they perform and a final summary, and SHALL exit with code 0 on success and non-zero on failure. They SHALL run in Windows PowerShell 5.1 and PowerShell 7.

#### Scenario: Summary
- **WHEN** `install.ps1` succeeds
- **THEN** its output lists the install folder, the shortcut, the App Paths registration, and whether `Path` was changed, and it notes that open terminals need a restart
