# Proposal

## Why

Editing environment variables on Windows means going through three nested modal dialogs (System Properties, Environment Variables, Edit), and the built-in editor refuses values longer than 2047 characters. winget adds one long per-package directory to the User `Path` for each portable install, so `Path` passes that limit quickly (2469 characters, 35 entries measured on the author's machine) and can no longer be edited with the built-in dialogs at all.

## What Changes

- New native Windows desktop application `envedit` (Rust, Win32 via `windows-rs`, no other runtime dependencies) that opens straight into a single window showing all User and Machine environment variables.
- Master-detail layout: filterable variable list on the left, editor on the right. `;`-separated values (such as `Path`) are edited as a list of entries; other values as plain text. No value length limit beyond the Windows limit of 32,767 characters.
- Edits are staged, not written immediately: a pending-changes view, session undo, and a single Apply that writes everything.
- Writes preserve the registry value type (`REG_SZ` vs `REG_EXPAND_SZ`), detect concurrent external modifications before overwriting, take a snapshot of the prior state, and broadcast `WM_SETTINGCHANGE` so new processes see the change.
- The application runs unelevated. Machine-scope changes are applied by relaunching the same executable elevated (UAC) in a windowless helper mode, only when Machine changes are staged.
- Passive diagnostics on entries and values: value length meter, missing-directory flag, duplicate flag (within and across scopes), and an expanded preview of `%VAR%` references.
- Command-line launch shortcuts: `envedit` opens the window; `envedit <name>` opens with that variable selected.
- Out of scope for this change: dark mode, drag-and-drop reordering, bulk removal of entries, a snapshot history browser, and an installer.

## Capabilities

### New Capabilities

- `env-storage`: Reading and writing User and Machine environment variables in the registry, including type preservation, conflict detection, snapshots before write, and change broadcast.
- `env-editor-ui`: The single-window editor: variable list and filtering, list and text editing modes, staged changes with undo, the Apply flow, and command-line launch.
- `elevated-apply`: Applying Machine-scope changes through an elevated helper mode of the same executable, including UAC cancellation and partial-success handling.
- `env-diagnostics`: Passive markers for value length, missing directories, duplicate entries, and expanded `%VAR%` previews.

### Modified Capabilities

None. The project has no existing specs.

## Impact

- Code: replaces the placeholder `src/main.rs`. The crate becomes a Windows GUI application.
- Dependencies: adds the `windows` crate. The build needs an embedded application manifest (Common Controls v6, PerMonitorV2 DPI awareness), which may add a build-time dependency for resource compilation.
- Platform: Windows only (Windows 10 and later).
- Systems: writes `HKCU\Environment` and `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`. A faulty write can break `Path` for all new processes, which is why snapshots and conflict checks are part of the scope.
