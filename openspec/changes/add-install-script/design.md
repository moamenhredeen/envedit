# Design

## Context

envedit is one self-contained `.exe` without runtime dependencies (see `openspec/specs/env-storage` and `env-editor-ui` for its behavior). Motivation and scope are in `proposal.md`; behavior in `specs/install-script`. The scripts must not repeat the mistakes envedit was built to avoid: converting `REG_EXPAND_SZ` to `REG_SZ`, duplicating `Path` entries, or writing `Path` without a way back.

## Goals / Non-Goals

**Goals:**
- Dependency-free scripts that work in Windows PowerShell 5.1 and PowerShell 7.
- `Path` handling testable against a scratch registry key, not the real environment.

**Non-Goals:**
- Installing for all users, MSI, winget, code signing.
- A test framework dependency (the machine only has Pester 3.4).

## Decisions

### D1: Three scripts with a shared helper

```
scripts/
  envedit-install-lib.ps1   shared functions (dot-sourced)
  install.ps1               build or take exe, copy, shortcut, App Paths, Path
  uninstall.ps1             reverse of install
  test-install-lib.ps1      tests for the Path functions on a scratch key
```

`install.ps1` copies `uninstall.ps1` and the helper into the install folder, so uninstall works after the repository is gone. Alternative: one script with `-Uninstall`; rejected because the copy in the install folder would then also carry the build logic.

### D2: Path editing through the .NET registry API, not `[Environment]`

`[Environment]::SetEnvironmentVariable('Path', ..., 'User')` writes a plain `REG_SZ` whatever the existing type, and `GetEnvironmentVariable` returns the expanded value, so a round trip turns a `REG_EXPAND_SZ` `Path` into a `REG_SZ` one and can bake expanded paths in place of `%VAR%` references. The helper instead opens `HKCU\Environment` with `Microsoft.Win32.Registry`, reads the raw value with `DoNotExpandEnvironmentNames`, reads its kind with `GetValueKind`, and writes back with the same kind. The key path is a parameter, so tests use `HKCU\Software\envedit-test`.

Entry comparison follows envedit's duplicate rule (`env-diagnostics`): expand `%VAR%`, lowercase, trim trailing `\`. Splitting drops empty entries only when the script actually writes; an unchanged `Path` is never rewritten.

### D3: Snapshot in envedit's own format

Before a `Path` write, the helper writes all User variables to `%LOCALAPPDATA%\envedit\snapshots\<yyyyMMdd-HHmmss-fff>-user.txt` (UTC) in the `envedit-records 1` format (`var`, scope, name, type token, value; `\`, tab, CR, LF escaped). envedit's 50-file retention prunes these too, since the names match. If the snapshot cannot be written, `Path` is not changed.

### D4: Broadcast with P/Invoke

`SendMessageTimeout(HWND_BROADCAST, WM_SETTINGCHANGE, 0, "Environment", SMTO_ABORTIFHUNG, 5000)` via a small `Add-Type` signature, the same call envedit makes.

### D5: Shortcut and App Paths

- Shortcut through the `WScript.Shell` COM object (`CreateShortcut`) in `%APPDATA%\Microsoft\Windows\Start Menu\Programs\envedit.lnk`; works in both PowerShell editions.
- App Paths: `HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\envedit.exe` with the default value set to the full exe path. Per-user App Paths are honored by `ShellExecute` (Win+R, Start → Run).

### D6: Upgrade safety

Before any change, the scripts check for a running `envedit` process whose path is the installed exe and stop if found. The binary is copied to a temporary name in the install folder and then renamed over the old one, so a failed copy never leaves a half-written exe.

### D7: Order of steps

Install: resolve/build binary → check running instance → copy files → shortcut → App Paths → `Path` (snapshot, write, broadcast) → summary. Everything that can fail without side effects (build, missing file, running instance) happens before the first change. Uninstall runs the same steps in reverse.

## Risks / Trade-offs

- [Execution policy blocks unsigned scripts] → Document `powershell -ExecutionPolicy Bypass -File scripts\install.ps1`; the scripts themselves do not change policy.
- [Elevated run installs into the admin account's profile, not the user's] → Warn when elevated (spec); do not refuse, because a developer running as their own admin account is a valid case.
- [`Path` changed by another program between read and write] → The window is milliseconds; the snapshot allows recovery. envedit's conflict check is not replicated.
- [Start menu index may take a moment to list a new shortcut] → Accepted; launching via Win+R or terminal works immediately.
