# env-editor-ui Specification

## Purpose

Defines the single-window editor that replaces the nested Windows environment variable dialogs: browsing and filtering variables, editing list and plain values, staging changes with undo, applying them, and launching from the command line.

## Requirements

### Requirement: Single window
The application SHALL open directly into one main window that shows the variables of both scopes and the editor for the selected variable. Viewing, editing, creating and deleting variables SHALL NOT open additional windows or modal dialogs. Only the confirmation on closing with pending changes, Apply results that need attention, and the Windows UAC prompt MAY appear as separate dialogs.

#### Scenario: Direct launch
- **WHEN** the user runs `envedit`
- **THEN** the main window shows both scopes' variables without any intermediate dialog

#### Scenario: Editing stays in window
- **WHEN** the user selects User `Path` and edits an entry
- **THEN** the edit happens in the main window's editor pane

### Requirement: Variable list
The main window SHALL list variables grouped by scope (User, Machine), showing each variable's name, and SHALL mark variables that have staged changes. A filter field SHALL narrow the list to variables whose name or raw value contains the filter text, case-insensitively.

#### Scenario: Filter by name
- **WHEN** the user types `path` in the filter
- **THEN** the list shows `Path` and `PATHEXT` of both scopes and hides non-matching variables

#### Scenario: Filter by value
- **WHEN** the user types `WinGet` in the filter
- **THEN** the list shows every variable whose value contains `WinGet`

#### Scenario: Pending marker
- **WHEN** a variable has a staged change
- **THEN** it is visibly marked in the list

### Requirement: List editing mode
The editor SHALL show a value as a list of entries, one per row, when the value contains a `;` or the variable is `Path`, `PATHEXT` or `PSModulePath`. In list mode the user SHALL be able to add, edit in place, remove, move up and move down entries; moving SHALL be available through buttons and through Alt+Up and Alt+Down. The resulting value SHALL be the entries joined with `;`, with empty entries removed.

#### Scenario: Path as list
- **WHEN** the user selects User `Path`
- **THEN** each `;`-separated entry is shown as its own row

#### Scenario: Reorder with keyboard
- **WHEN** the user selects the third entry and presses Alt+Up
- **THEN** it becomes the second entry and the variable is marked as changed

#### Scenario: Empty entries dropped
- **WHEN** a value `C:\a;;C:\b;` is edited in list mode
- **THEN** the staged value is `C:\a;C:\b`

### Requirement: Text editing mode
The editor SHALL show other values as a single text field. The user SHALL be able to switch any variable between list mode and text mode.

#### Scenario: Plain value
- **WHEN** the user selects `TEMP`
- **THEN** its value is shown in a text field

#### Scenario: Switch mode
- **WHEN** the user switches `Path` to text mode
- **THEN** the full raw value is shown as one editable text

### Requirement: Value type control
The editor SHALL show the selected variable's registry value type and SHALL let the user switch it between `REG_SZ` and `REG_EXPAND_SZ` as a staged change.

#### Scenario: Show type
- **WHEN** the user selects Machine `Path`
- **THEN** the editor shows `REG_EXPAND_SZ`

### Requirement: Create and delete in window
The user SHALL be able to create a variable in a chosen scope and to delete the selected variable from within the main window, both as staged changes.

#### Scenario: Create variable
- **WHEN** the user creates User variable `TOOLS` with a value
- **THEN** `TOOLS` appears in the User group marked as a pending addition

#### Scenario: Delete variable
- **WHEN** the user deletes User variable `OLD_VAR`
- **THEN** `OLD_VAR` is marked as a pending deletion until Apply

### Requirement: Staged changes and undo
Edits SHALL be staged in memory and SHALL NOT be written until the user applies. The window SHALL show the number of pending changes and a view listing each pending change with its old and new value. The user SHALL be able to undo the most recent edit repeatedly (Ctrl+Z) and to discard all pending changes.

#### Scenario: Nothing written before Apply
- **WHEN** the user edits User `Path` and has not applied
- **THEN** the registry still holds the old value

#### Scenario: Undo
- **WHEN** the user removes an entry and presses Ctrl+Z
- **THEN** the entry is restored and, if no other edits remain, the variable is no longer marked as changed

#### Scenario: Review pending changes
- **WHEN** the user opens the pending changes view
- **THEN** each changed variable is listed with its scope, old value and new value

#### Scenario: Discard all
- **WHEN** the user discards all pending changes
- **THEN** every variable shows its loaded value and nothing is pending

### Requirement: Apply
Apply SHALL write all pending changes, then reload both scopes from the registry and show the result. After a fully successful Apply no changes SHALL remain pending; conflicting, failed or cancelled changes SHALL remain pending.

#### Scenario: Successful apply
- **WHEN** the user applies two User changes and both succeed
- **THEN** both are written, the list is reloaded, and the pending count is zero

#### Scenario: Reload after conflict
- **WHEN** Apply reports a conflict for a variable
- **THEN** the user can reload that variable's current registry value, which discards their staged edit for it

### Requirement: Close with pending changes
When the user closes the window while changes are pending, the application SHALL ask whether to apply, discard or cancel closing.

#### Scenario: Close prompt
- **WHEN** the user closes the window with one pending change
- **THEN** the application asks to apply, discard or cancel

### Requirement: Command-line selection
Running `envedit <name>` SHALL open the main window with variable `<name>` selected, preferring the User scope when the name exists in both scopes. If no variable has that name, the window SHALL open with `<name>` in the filter field.

#### Scenario: Open on Path
- **WHEN** the user runs `envedit path`
- **THEN** the window opens with User `Path` selected in list mode

#### Scenario: Unknown name
- **WHEN** the user runs `envedit foo` and no variable `foo` exists
- **THEN** the window opens with `foo` in the filter field

### Requirement: Native look and scaling
The window SHALL use the current Windows visual styles for standard controls and SHALL render sharply at any display scaling, including when moved between monitors with different scaling. A light appearance is sufficient; dark mode is not required.

#### Scenario: High-DPI monitor
- **WHEN** the window is moved from a 100% to a 150% scaled monitor
- **THEN** text and controls are re-laid out at the new scale without blurring
