# Tasks

## 1. Project setup

- [x] 1.1 Add the `windows` crate with the features listed in design D1 to `Cargo.toml`, set `#![windows_subsystem = "windows"]`, and create the `core/` and `ui/` module skeleton from design D2; verify `cargo build` succeeds
- [x] 1.2 Add `envedit.manifest` (Common Controls v6, PerMonitorV2, `asInvoker`, Windows 10 `supportedOS`) and `build.rs` embedding it via `/MANIFEST:EMBED` and `/MANIFESTINPUT`; verify with `mt.exe -inputresource:target\debug\envedit.exe;#1 -out:con` (or a resource viewer) that the manifest is embedded
- [x] 1.3 Add a release profile tuned for size (`opt-level`, `lto`, `codegen-units = 1`, `panic = "abort"`, `strip`); verify the release binary builds and record its size

## 2. Core model and records

- [x] 2.1 Implement `core/model.rs` (`Scope`, `ValueType`, `Var`, case-insensitive name key, entry split/join dropping empty entries, list-mode detection per design D11); verify with unit tests for split/join, `;;`/trailing `;`, and the `Path`/`PATHEXT`/`PSModulePath` rule
- [x] 2.2 Implement `core/records.rs` (header, record kinds, field escaping per design D8); verify with round-trip unit tests including tabs, newlines, backslashes, non-ASCII text and a 32,767-character value, and rejection tests for a bad header and malformed lines

## 3. Registry access

- [x] 3.1 Define the `EnvStore` trait and an in-memory implementation for tests; verify it compiles and is used by a trivial unit test
- [x] 3.2 Implement the Win32 registry `EnvStore` (enumerate, read raw value and type, write with terminating NUL, delete; non-string types marked read-only) with the key path as a parameter; verify with an integration test against a scratch key `HKCU\Software\envedit-test` that writes, reads back, preserves `REG_SZ`/`REG_EXPAND_SZ`, handles a 5,000-character value, deletes, and cleans up the key
- [x] 3.3 Add a read-only check that loading the real User and Machine environment keys works unelevated; verify by a test that loads both and finds `Path` in each

## 4. Staging and undo

- [x] 4.1 Implement `core/staging.rs` (staged map, set/delete/create, type change, no-op detection that clears a change equal to the loaded state, 32,767 limit, empty-value refusal); verify with unit tests for each `env-storage` and `env-editor-ui` staging scenario
- [x] 4.2 Add the undo stack and discard-all; verify with unit tests that repeated undo restores earlier states and clears pending markers when back at the loaded state
- [x] 4.3 Add the pending-diff listing (scope, name, old, new, kind); verify with a unit test covering set, create, delete and type-only changes

## 5. Snapshots and apply

- [x] 5.1 Implement `core/snapshot.rs` (write all variables of a scope to `%LOCALAPPDATA%\envedit\snapshots`, timestamped name, keep newest 50) with the base directory as a parameter; verify with unit tests in a temp dir for content, naming and retention at the 51st file, and that a write failure is reported
- [x] 5.2 Implement `core/apply.rs` (snapshot first and abort the scope on snapshot failure, per-variable conflict check against loaded value and type, write/delete, continue after failure, per-variable outcome list); verify with unit tests on the in-memory store for success, external modification, external deletion, one failing write among three, and snapshot failure
- [x] 5.3 Implement the `WM_SETTINGCHANGE` broadcast (`SendMessageTimeoutW`, `HWND_BROADCAST`, `"Environment"`, `SMTO_ABORTIFHUNG`) callable from a worker thread; verify manually that a new `cmd` started from Explorer sees a changed User variable after Apply

## 6. Expansion and diagnostics

- [x] 6.1 Implement `core/expand.rs` (lookup User, then Machine, then process environment, unresolved references reported); verify with unit tests including a reference to a newly staged variable and an undefined reference
- [x] 6.2 Implement length, duplicate (same-scope and cross-scope) and `REG_SZ`-with-`%` markers in `core/diagnostics.rs`; verify with unit tests for each `env-diagnostics` scenario (2469 + 40 → 2510, `C:\Tools` vs `c:\tools\`, cross-scope Git entry)
- [x] 6.3 Implement missing-directory classification (absolute drive path → check, UNC → unchecked, other → not a path) and the existence check with a per-session cache; verify with unit tests on classification and a temp-dir existence test

## 7. Elevated apply

- [x] 7.1 Implement the is-elevated check and helper input/result files using `core/records.rs`; verify with unit tests that input round-trips and that malformed input is rejected before any write
- [x] 7.2 Implement helper mode (`--apply-machine <in> --result <out>`): validate input, apply to the Machine store with the shared apply logic, write the result file, exit codes per design D9; verify with a unit test running the helper logic against the in-memory store, and that running `envedit --apply-machine missing.txt` changes nothing and exits non-zero
- [x] 7.3 Implement the unelevated launch path (Machine snapshot by invoking user, `ShellExecuteExW` with `runas` and `SEE_MASK_NOCLOSEPROCESS`, `ERROR_CANCELLED` handling, worker waiting on the process handle, result reading and file cleanup) and the in-process path when already elevated; verify manually with the checklist in 9.2

## 8. User interface

- [x] 8.1 Implement the main window, message loop, DPI handling (`GetDpiForWindow`, `WM_DPICHANGED`, font recreation) and layout from design D4; verify manually that the window opens with no intermediate dialog and stays sharp when moved between monitors with different scaling
- [x] 8.2 Implement the variable ListView with User/Machine groups, pending `*` markers and the case-insensitive name/value filter; verify manually with the `path` and `WinGet` filter scenarios
- [x] 8.3 Implement the editor pane in list mode (entry ListView with in-place edit, edit limit raised to 32,767, Add/Remove/Up/Down, Alt+Up/Down, Del, F2) and text mode (multiline Edit), plus the mode switch; verify manually that User `Path` opens as a list and that reordering marks the variable as changed
- [x] 8.4 Add the type selector, inline create-variable field and delete-variable action; verify manually by creating `TOOLS` with `%USERPROFILE%\tools` and deleting it again before Apply
- [x] 8.5 Show diagnostics in the editor: length meter with the 2047 note, missing/duplicate/unchecked markers (missing results arriving from the worker without freezing the window), expanded preview, and the `REG_SZ` warning; verify manually on the real User `Path`, which has 7 missing entries
- [x] 8.6 Implement the pending-changes pane toggle, Undo (Ctrl+Z), Discard, Apply (Ctrl+S), the status bar (pending count, last result, note that open terminals need a restart) and per-variable result display including conflict reload; verify manually that Apply writes, reloads and clears the pending count
- [x] 8.7 Implement the close prompt (apply / discard / cancel) when changes are pending; verify manually for each choice
- [x] 8.8 Implement command-line selection (`envedit <name>`, User preferred, unknown name goes to the filter); verify manually with `envedit path` and `envedit foo`

## 9. Integration verification

- [x] 9.1 Run `cargo test` and `cargo clippy` with no failures or warnings, and build the release binary; verify the binary size is under 1 MB and it starts without a visible delay
- [x] 9.2 Run the manual end-to-end checklist on a test variable (not `Path`): User-only Apply shows no UAC; mixed Apply shows one UAC prompt; declining UAC keeps Machine changes pending while User changes stay applied; an external change via `reg add` during editing produces a conflict; snapshots appear in `%LOCALAPPDATA%\envedit\snapshots`; then remove the test variables
- [x] 9.3 Edit the real User `Path` (over 2047 characters): remove one missing entry and apply; verify the value was written in full, its type is still `REG_SZ`, a snapshot of the previous value exists, and a new terminal sees the change
