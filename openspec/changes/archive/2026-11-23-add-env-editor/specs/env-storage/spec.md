# Spec Delta

## Purpose

Defines how envedit reads and writes persistent User and Machine environment variables so that every write keeps registry value types, never silently overwrites external changes, can be undone from a snapshot, and reaches newly started processes.

## ADDED Requirements

### Requirement: Read both scopes
The system SHALL read all persistent environment variables from the User scope (`HKCU\Environment`) and the Machine scope (`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`), keeping for each variable its name, its raw unexpanded value and its registry value type (`REG_SZ` or `REG_EXPAND_SZ`). Reading SHALL NOT require elevation.

#### Scenario: Load on startup
- **WHEN** the application starts without elevation
- **THEN** it shows every variable of both scopes with its raw value and type, including values longer than 2047 characters

#### Scenario: Unexpanded value
- **WHEN** a variable is stored as `REG_EXPAND_SZ` with value `%USERPROFILE%\bin`
- **THEN** the loaded raw value is `%USERPROFILE%\bin`, not the expanded path

### Requirement: Preserve value type
When writing a modified variable, the system SHALL keep its existing registry value type unless the user explicitly changed the type. A newly created variable SHALL be written as `REG_EXPAND_SZ` if its value contains a `%` character and as `REG_SZ` otherwise.

#### Scenario: Modified REG_EXPAND_SZ stays expandable
- **WHEN** the user edits Machine `Path`, which is stored as `REG_EXPAND_SZ`, and applies
- **THEN** Machine `Path` is still stored as `REG_EXPAND_SZ`

#### Scenario: New variable with reference
- **WHEN** the user creates a User variable `TOOLS` with value `%USERPROFILE%\tools` and applies
- **THEN** `TOOLS` is stored as `REG_EXPAND_SZ`

#### Scenario: Explicit type change
- **WHEN** the user switches User `Path` from `REG_SZ` to `REG_EXPAND_SZ` and applies
- **THEN** User `Path` is stored as `REG_EXPAND_SZ` with its value unchanged

### Requirement: Value length limit
The system SHALL reject any value longer than 32,767 characters and SHALL accept any shorter value, regardless of the 2047-character limit of the built-in Windows dialog.

#### Scenario: Long value accepted
- **WHEN** the user stages a `Path` value of 5,000 characters and applies
- **THEN** the value is written in full

#### Scenario: Over-limit value rejected
- **WHEN** an edit would make a value longer than 32,767 characters
- **THEN** the edit is not staged and the user is told the limit

### Requirement: Conflict detection
Before writing a variable, the system SHALL compare the current registry value and type with the ones it loaded. If they differ, the system SHALL NOT write that variable, SHALL report it as a conflict, and SHALL leave the user's edit staged.

#### Scenario: External change during editing
- **WHEN** an installer changes User `Path` after envedit loaded it, and the user then applies an edit to User `Path`
- **THEN** User `Path` is not overwritten, a conflict is reported for it, and the edit stays staged

#### Scenario: Variable deleted externally
- **WHEN** a variable the user edited was deleted from the registry by another process before Apply
- **THEN** it is reported as a conflict and not re-created

#### Scenario: No conflict
- **WHEN** the registry still matches the loaded state at Apply
- **THEN** the variable is written

### Requirement: Snapshot before write
Before writing any change to a scope, the system SHALL save a snapshot of all variables of that scope (names, raw values, types) as a file in the invoking user's `%LOCALAPPDATA%\envedit\snapshots` directory, named by timestamp and scope. If the snapshot cannot be saved, the system SHALL NOT write to that scope. The system SHALL keep the 50 most recent snapshots and delete older ones.

#### Scenario: Snapshot taken
- **WHEN** the user applies a change to User `Path`
- **THEN** a snapshot file containing the previous state of all User variables exists before the registry is modified

#### Scenario: Snapshot failure blocks write
- **WHEN** the snapshot directory cannot be written
- **THEN** no variable of that scope is written and the user is told why

#### Scenario: Retention
- **WHEN** a 51st snapshot is saved
- **THEN** the oldest snapshot is deleted

### Requirement: Create and delete variables
The system SHALL support creating a variable in either scope and deleting an existing variable, which removes its registry value. An empty value SHALL NOT be staged; the user must delete the variable instead.

#### Scenario: Delete variable
- **WHEN** the user deletes User variable `OLD_VAR` and applies
- **THEN** `OLD_VAR` no longer exists in `HKCU\Environment`

#### Scenario: Empty value refused
- **WHEN** the user clears a value to empty
- **THEN** the edit is not staged and the user is told to delete the variable instead

### Requirement: Change broadcast
After a successful write, the system SHALL broadcast `WM_SETTINGCHANGE` with the parameter `Environment` to all top-level windows, using a timeout so that unresponsive windows cannot block the application.

#### Scenario: New processes see change
- **WHEN** the user applies a change to User `Path`
- **THEN** a terminal started from Explorer afterwards sees the new `Path`

#### Scenario: Hung window
- **WHEN** a top-level window does not respond to the broadcast
- **THEN** the application stays responsive and the Apply completes

### Requirement: Partial failure reporting
If writing one variable fails, the system SHALL continue with the remaining variables, SHALL keep the failed and conflicting changes staged, and SHALL report per variable whether it was applied, conflicted or failed.

#### Scenario: One write fails
- **WHEN** three User variables are staged and writing the second fails
- **THEN** the first and third are written, the second stays staged, and the result lists all three outcomes
