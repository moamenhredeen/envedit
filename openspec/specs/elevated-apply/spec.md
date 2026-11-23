# elevated-apply Specification

## Purpose

Defines how envedit applies Machine-scope changes while running unelevated: the same executable is relaunched elevated through UAC only for the Machine part of an Apply, with clear handling of cancellation, conflicts and partial success.

## Requirements

### Requirement: Run unelevated
The application SHALL start and run without elevation. It SHALL NOT request elevation at launch.

#### Scenario: Normal launch
- **WHEN** a standard user runs `envedit`
- **THEN** no UAC prompt appears

### Requirement: Elevate only for Machine changes
On Apply, the application SHALL write User changes itself without elevation. Only when Machine changes are pending SHALL it start an elevated helper, which triggers one UAC prompt per Apply. When the application is already running elevated, it SHALL write Machine changes directly without starting a helper.

#### Scenario: User-only apply
- **WHEN** only User changes are pending and the user applies
- **THEN** no UAC prompt appears

#### Scenario: Mixed apply
- **WHEN** User and Machine changes are pending and the user applies
- **THEN** User changes are written first, then one UAC prompt appears for the Machine changes

#### Scenario: Already elevated
- **WHEN** the application runs elevated and the user applies Machine changes
- **THEN** they are written without a UAC prompt

### Requirement: UAC cancellation
If the user declines the UAC prompt, the application SHALL write no Machine changes, SHALL keep them staged, and SHALL tell the user that the Machine changes were not applied. User changes from the same Apply SHALL stay applied.

#### Scenario: Declined prompt
- **WHEN** the user clicks No on the UAC prompt
- **THEN** Machine changes remain pending, User changes are applied, and a notice states that Machine changes were not applied

### Requirement: Helper behavior
The elevated helper SHALL show no window, SHALL write only the Machine-scope values named in the changes handed to it, SHALL apply the same conflict detection and partial-failure rules as unelevated writes, and SHALL report the outcome of each variable back to the application. The application SHALL wait for the helper to finish, then reload, broadcast the change and show the per-variable result.

#### Scenario: Helper conflict
- **WHEN** Machine `Path` changed externally after envedit loaded it and the helper is asked to write it
- **THEN** the helper does not write it and the application reports a conflict for Machine `Path`

#### Scenario: Result shown
- **WHEN** the helper writes two Machine variables successfully
- **THEN** the application shows both as applied and no Machine changes remain pending

### Requirement: Helper input validation
The helper SHALL reject input that is missing, unreadable or malformed and SHALL then write nothing. Running the helper mode manually without valid input SHALL change nothing.

#### Scenario: Malformed input
- **WHEN** the helper is started with an input that cannot be parsed
- **THEN** it exits with an error status and no registry value is changed

### Requirement: Machine snapshot by invoking user
The snapshot of the Machine scope required before writing SHALL be saved in the invoking (unelevated) user's snapshot directory before the helper starts, so it remains available even when elevation uses a different administrator account.

#### Scenario: Over-the-shoulder elevation
- **WHEN** a standard user applies Machine changes and an administrator enters credentials at the UAC prompt
- **THEN** the Machine snapshot is in the standard user's `%LOCALAPPDATA%\envedit\snapshots`, and User changes went to the standard user's `HKCU`
