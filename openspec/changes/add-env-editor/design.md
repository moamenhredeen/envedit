# Design

## Context

The repository is a bare Rust crate (`edition = "2024"`, no dependencies, placeholder `src/main.rs`). The toolchain on the development machine is `x86_64-pc-windows-msvc` (nightly 1.102). There are no existing specs. Motivation and scope are in `proposal.md`; behavior is defined in `specs/`.

Facts that shape the approach:

- User variables live in `HKCU\Environment`; Machine variables in `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`. Writing HKLM needs elevation.
- On the author's machine User `Path` is `REG_SZ` and Machine `Path` is `REG_EXPAND_SZ`, so types differ in practice and must be preserved.
- The prime requirement is a fast, lightweight native tool: no runtime besides what Windows ships, instant startup, small binary.

## Goals / Non-Goals

**Goals:**
- A core that holds all logic (model, staging, registry access, snapshots, diagnostics, helper protocol) and can be unit-tested without a window or the real environment keys.
- A thin Win32 UI layer on top of the core.
- One executable, no runtime dependencies besides the `windows` crate.

**Non-Goals:**
- Reacting live to external registry changes while the app is open (conflicts are detected at Apply only).
- Volatile environment (`HKCU\Volatile Environment`) and per-process environment editing.
- A snapshot restore UI (see Risks).
- Localization; the UI is English.

## Decisions

### D1: Raw Win32 via the `windows` crate

Use `windows` (Microsoft's official bindings) with only the needed features (`Win32_Foundation`, `Win32_System_Registry`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Controls`, `Win32_UI_Shell`, `Win32_Graphics_Gdi`, `Win32_Security`, `Win32_System_Threading`, `Win32_UI_HiDpi`, `Win32_UI_Input_KeyboardAndMouse`, `Win32_System_LibraryLoader`).

Alternatives considered:
- `winsafe`: less boilerplate, but an extra dependency; verbosity is acceptable to the author.
- `native-windows-gui`: appears to have had little maintenance for years.
- GPUI: GPU renderer and a large dependency tree, against the lightweight goal.
- C# WinUI 3 / WPF: needs the .NET runtime or a large self-contained build.

### D2: Module layout

```
src/
  main.rs          entry: parse args, dispatch to UI or helper mode
  core/
    model.rs       Scope, ValueType, Var, entry split/join
    registry.rs    EnvStore trait + Win32 registry implementation
    staging.rs     staged changes, undo stack, pending diff
    apply.rs       conflict check, write loop, per-variable outcomes
    snapshot.rs    snapshot write + retention
    records.rs     line-based record encoding (snapshots, helper files)
    expand.rs      %VAR% expansion against loaded/staged state
    diagnostics.rs length, missing, duplicate, expansion markers
    elevation.rs   is-elevated check, helper launch, helper mode
  ui/
    window.rs      main window, layout, message loop
    varlist.rs     variable ListView with scope groups and filter
    editor.rs      list mode / text mode editor pane
    pending.rs     pending changes pane
    dpi.rs         DPI helpers
```

`core` has no UI imports. `registry.rs` exposes an `EnvStore` trait (`load(scope)`, `read(scope, name)`, `write(scope, name, value, type)`, `delete(scope, name)`) so that `apply` and the helper can be tested against an in-memory store.

### D3: Single-threaded UI, workers post back

The UI runs one message loop. Slow work runs on `std::thread` workers that post results back with `PostMessageW(WM_APP + n)`:
- directory-existence checks for diagnostics,
- the `WM_SETTINGCHANGE` broadcast (it can take seconds when windows are slow),
- waiting for the elevated helper process.

Alternative: blocking calls on the UI thread. Rejected because the broadcast with `SMTO_ABORTIFHUNG` still waits up to the timeout per window and would freeze the window.

### D4: Window layout without modal dialogs

```
+-----------------------------------------------------------------+
| [filter_____________]                [Discard] [Undo] [Apply]   |
+---------------------+-------------------------------------------+
| ListView (report,   | header: name, scope, type [REG_SZ v]      |
|  group view)        |         length meter                      |
|  User               |-------------------------------------------|
|   * Path            | list mode: ListView, in-place edit        |
|     TEMP            |   (LVS_EDITLABELS), marker column         |
|  Machine            | or text mode: multiline Edit              |
|     Path            |-------------------------------------------|
|                     | [Add][Remove][Up][Down]  [List|Text]      |
| [New] [Delete]      | expanded preview / warnings line          |
+---------------------+-------------------------------------------+
| status bar: 2 pending changes  |  last result                   |
+-----------------------------------------------------------------+
```

- The variable list uses a report-mode ListView with group view enabled (User / Machine groups). Pending variables are prefixed with `*`.
- The right pane switches between the editor and the pending-changes pane (a toggle, not a dialog). The pending pane is a ListView with scope, name, old value and new value.
- Creating a variable uses an inline name field at the top of the editor pane, not a dialog.
- Layout is computed in `WM_SIZE` with a fixed left-pane ratio and a minimum width; all metrics are scaled with `GetDpiForWindow`. `WM_DPICHANGED` applies the suggested rectangle and recreates fonts.
- Keyboard: Ctrl+F focuses the filter, Ctrl+Z undo, Ctrl+S apply, Alt+Up/Down move entry, Del removes entry, F2 edits entry. Implemented with an accelerator table.

### D5: Manifest via linker flags

Embed an application manifest (Common Controls v6 dependency, `PerMonitorV2` DPI awareness, `asInvoker` execution level, Windows 10 `supportedOS`) using `build.rs` with `cargo:rustc-link-arg-bins=/MANIFEST:EMBED` and `/MANIFESTINPUT:<path>`. This works with the MSVC linker without extra crates.

Alternative: `embed-resource` / `winres` with an `.rc` file. Only needed later if an icon or version resource is added.

`#![windows_subsystem = "windows"]` is set so no console window appears; the helper mode never needs a console.

### D6: Staging model and undo

Loaded state is an immutable map `(Scope, name) -> Var { value, type }`. Staged changes are a map `(Scope, name) -> Change { Set { value, type } | Delete }`. Every user edit produces a new staged map; the previous map is pushed onto the undo stack. The data is small (tens of variables, a few KB each), so cloning per edit is cheap and simpler than inverse operations. A change that makes a variable equal to its loaded state removes the entry from the map, so undo back to the original clears the pending marker.

Names compare case-insensitively, matching Windows.

### D7: Registry I/O

- Read with `RegEnumValueW` / `RegQueryValueExW` into UTF-16 buffers, strip the terminating NUL. Only `REG_SZ` and `REG_EXPAND_SZ` are editable; values of other types are shown read-only.
- Write with `RegSetValueExW` including the terminating NUL; delete with `RegDeleteValueW`.
- Conflict check: immediately before each write, re-read the value and type and compare with the loaded state. The remaining time window between read and write is accepted (see Risks).

### D8: Record format for snapshots and helper files

A small hand-written UTF-8 line format instead of `serde_json`, to keep zero extra dependencies:

```
envedit-records 1
<kind>\t<scope>\t<name>\t<type>\t<value>[\t<type>\t<value>]
```

Fields are escaped (`\\`, `\t`, `\n`, `\r`). Snapshots use kind `var`. Helper input uses kind `set` (before type/value, after type/value) and `del` (before type/value). Helper results use kind `ok`, `conflict`, `failed` with a message. A malformed header or line makes the whole file invalid.

Snapshots are written to `%LOCALAPPDATA%\envedit\snapshots\<yyyyMMdd-HHmmss-fff>-<scope>.txt`; after writing, files beyond the newest 50 are deleted.

Alternative: `serde` + `serde_json`. Rejected for size and dependency count; the format has only a few record kinds.

### D9: Elevated helper protocol

```
UI (unelevated)
  write User changes (snapshot, conflict check, write)
  if Machine changes:
    snapshot Machine scope into own %LOCALAPPDATA%
    write input file  %LOCALAPPDATA%\envedit\pending\<random>.in
    ShellExecuteExW(verb "runas", self exe,
                    "--apply-machine <in> --result <out>",
                    SEE_MASK_NOCLOSEPROCESS)
      ERROR_CANCELLED (1223) -> keep Machine changes staged, notice
    worker thread waits on process handle -> PostMessage
    read result file, delete both files
  reload, broadcast (worker), show per-variable outcomes
```

- Already elevated (`GetTokenInformation(TokenElevation)`) skips the relaunch and applies Machine changes in-process.
- The helper validates the input fully before writing anything, writes only Machine values named in the input, runs the same apply logic as D7 against the HKLM store, and writes the result file. Exit code 0 on completed run (per-variable outcomes in the file), non-zero on invalid input.
- The input file lives in the invoking user's profile. An unelevated process could tamper with it between write and read; the impact is bounded because such a process could start its own UAC prompt, and the helper can only write environment values.

Alternative: pipes or a shared-memory channel. Rejected as more code for no gain at this size.

### D10: Expansion and diagnostics

- `%VAR%` expansion uses the current loaded+staged state: User variables first, then Machine, then the process environment for volatile variables such as `USERPROFILE`. This makes references to newly created variables preview correctly, unlike `ExpandEnvironmentStringsW`.
- Missing check: entry is a candidate if, after expansion, it matches `^[A-Za-z]:\\`. UNC entries are marked unchecked. Candidates are checked with `GetFileAttributesW` on a worker thread; results are cached per expanded path for the session.
- Duplicate key: expanded entry, lowercase, trailing `\` trimmed. Cross-scope comparison only between variables with the same name.
- The length meter counts UTF-16 code units, which is the unit of the 32,767 limit.

### D11: List mode detection and joining

List mode applies if the value contains `;` or the name is `Path`, `PATHEXT` or `PSModulePath` (case-insensitive). Joining drops empty entries. A variable that is never edited is never written, so its original formatting (including `;;` or a trailing `;`) is kept.

## Risks / Trade-offs

- [A faulty write breaks `Path` for every new process] → Snapshots before every write, conflict checks, per-variable outcomes, and the pending diff to review before Apply.
- [No restore UI in v1; restoring a snapshot is manual] → Snapshot files are plain text and readable; a restore command is a candidate follow-up.
- [Conflict check races with an external writer in the few microseconds between re-read and write] → Accepted; the window is tiny and the snapshot allows recovery.
- [Large amount of `unsafe` Win32 code] → Keep all `unsafe` in thin wrappers in `core/registry.rs`, `core/elevation.rs` and `ui/`; core logic stays safe Rust and unit-tested.
- [ListView in-place editing is fiddly (label length limit, focus handling)] → Set the edit control's limit to 32,767 via `LVM_GETEDITCONTROL` + `EM_SETLIMITTEXT`; fall back to text mode for entries that are awkward to edit in place.
- [Unsigned executable shows an "unknown publisher" UAC prompt and may trigger SmartScreen] → Accepted for a personal tool; code signing is out of scope.
- [Broadcast reaches only top-level windows; already-running processes keep their old environment] → Inherent to Windows; status bar notes that open terminals need a restart.
