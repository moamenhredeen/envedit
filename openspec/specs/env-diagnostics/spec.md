# env-diagnostics Specification

## Purpose

Defines passive markers that help users keep environment variables healthy, such as length warnings, missing directories, duplicates and unexpanded references, without ever changing values automatically.

## Requirements

### Requirement: Markers are passive
Diagnostics SHALL only display information. They SHALL NOT modify, stage or remove any value or entry on their own.

#### Scenario: Missing entry stays
- **WHEN** a `Path` entry is flagged as missing
- **THEN** the entry remains in the value until the user removes it

### Requirement: Length meter
The editor SHALL show the selected value's length in characters next to the limit of 32,767, updated live while editing. Values longer than 2047 characters SHALL carry a note that the built-in Windows dialog cannot edit them.

#### Scenario: Live length
- **WHEN** the user adds a 40-character entry to a 2469-character `Path`
- **THEN** the meter shows 2510 (the entry plus one separator)

#### Scenario: Legacy limit note
- **WHEN** the selected value is longer than 2047 characters
- **THEN** the meter shows the note about the built-in dialog limit

### Requirement: Missing directory flag
In list mode, the system SHALL flag each entry that is an absolute local path (drive-letter form, after expanding `%VAR%` references) and does not exist as a directory. Entries that are not absolute paths SHALL NOT be flagged. Network (UNC) paths SHALL NOT be checked and SHALL be shown as unchecked. Checking SHALL NOT freeze the window.

#### Scenario: Missing directory
- **WHEN** `Path` contains `C:\Users\me\go\bin` and that directory does not exist
- **THEN** the entry is flagged as missing

#### Scenario: Not a path
- **WHEN** `PATHEXT` is shown in list mode with entries like `.COM`
- **THEN** no entry is flagged as missing

#### Scenario: Network path
- **WHEN** an entry is `\\server\share\bin`
- **THEN** it is shown as unchecked, not as missing or present

### Requirement: Duplicate flag
In list mode, the system SHALL flag an entry as a duplicate when another entry of the same variable is equal to it, comparing case-insensitively, after expanding `%VAR%` references and ignoring a trailing backslash. For variables that exist in both scopes, it SHALL also flag User entries that equal an entry of the Machine variable with the same name, and the reverse.

#### Scenario: Same-scope duplicate
- **WHEN** User `Path` contains `C:\Tools` and `c:\tools\`
- **THEN** both entries are flagged as duplicates

#### Scenario: Cross-scope duplicate
- **WHEN** User `Path` and Machine `Path` both contain `C:\Program Files\Git\cmd`
- **THEN** the entry is flagged as duplicate in both scopes

### Requirement: Expanded preview
For values and entries containing `%VAR%` references, the editor SHALL show the expanded result next to the raw text. References to undefined variables SHALL be shown as unresolved. A `REG_SZ` value containing `%` SHALL carry a warning that its references will not be expanded by Windows.

#### Scenario: Expanded entry
- **WHEN** an entry is `%USERPROFILE%\bin`
- **THEN** the editor shows the expanded path alongside it

#### Scenario: Undefined reference
- **WHEN** an entry is `%NOPE%\bin` and `NOPE` is not defined
- **THEN** the reference is shown as unresolved

#### Scenario: REG_SZ with reference
- **WHEN** User `Path` is `REG_SZ` and an entry contains `%USERPROFILE%`
- **THEN** the editor warns that the reference will not be expanded
