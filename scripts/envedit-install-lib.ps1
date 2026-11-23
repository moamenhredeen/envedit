# Shared functions for install.ps1, uninstall.ps1 and test-install-lib.ps1.
# Dot-source this file. Works in Windows PowerShell 5.1 and PowerShell 7.
#
# Path is edited through the registry API, never [Environment]::SetEnvironmentVariable,
# which would turn a REG_EXPAND_SZ value into REG_SZ and expand %VAR% references.

Set-StrictMode -Version 2

$script:ValueLimit = 32767
$script:EnvKeyPath = 'Environment'
$script:AppPathsKey = 'Software\Microsoft\Windows\CurrentVersion\App Paths\envedit.exe'

function Get-InstallDir { Join-Path $env:LOCALAPPDATA 'Programs\envedit' }
function Get-InstalledExe { Join-Path (Get-InstallDir) 'envedit.exe' }
function Get-ShortcutPath { Join-Path ([Environment]::GetFolderPath('Programs')) 'envedit.lnk' }
function Get-SnapshotDir { Join-Path $env:LOCALAPPDATA 'envedit\snapshots' }

function Write-Step([string]$Message) { Write-Host "  - $Message" }

function Write-ElevationWarning {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal $identity
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Write-Warning "Running elevated: envedit is set up for the account '$($identity.Name)'. Run from a normal PowerShell to set it up for your own account."
    }
}

# Stops when the installed envedit is running, before anything is changed.
function Assert-NotRunning {
    $exe = Get-InstalledExe
    $running = @(Get-Process -Name envedit -ErrorAction SilentlyContinue | Where-Object {
        $_.Path -and ($_.Path -ieq $exe)
    })
    if ($running.Count -gt 0) {
        throw "envedit is running from $exe. Close it and run this script again."
    }
}

# ---- raw Path access ----

# Returns @{ Value; Kind } with the unexpanded value, or $null if the value does not exist.
function Get-RawEnvValue([string]$KeyPath = $script:EnvKeyPath, [string]$Name = 'Path') {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($KeyPath, $false)
    if (-not $key) { return $null }
    try {
        if (@($key.GetValueNames()) -notcontains $Name) { return $null }
        $value = $key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        return [pscustomobject]@{ Value = [string]$value; Kind = $key.GetValueKind($Name) }
    } finally {
        $key.Close()
    }
}

function Set-RawEnvValue([string]$KeyPath, [string]$Name, [string]$Value, [Microsoft.Win32.RegistryValueKind]$Kind) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($KeyPath)
    try { $key.SetValue($Name, $Value, $Kind) } finally { $key.Close() }
}

function Remove-RawEnvValue([string]$KeyPath, [string]$Name) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($KeyPath, $true)
    if (-not $key) { return }
    try { $key.DeleteValue($Name, $false) } finally { $key.Close() }
}

# Comparison key for Path entries, matching envedit's duplicate rule:
# %VAR% expanded, case-insensitive, trailing backslash ignored.
function Get-EntryKey([string]$Entry) {
    [Environment]::ExpandEnvironmentVariables($Entry).TrimEnd('\').ToLowerInvariant()
}

function Split-PathEntries([string]$Value) {
    , @($Value -split ';' | Where-Object { $_ -ne '' })
}

function Test-PathEntry([string]$Value, [string]$Entry) {
    $wanted = Get-EntryKey $Entry
    foreach ($e in (Split-PathEntries $Value)) {
        if ((Get-EntryKey $e) -eq $wanted) { return $true }
    }
    return $false
}

function Assert-StringKind($Current, [string]$Name) {
    $ok = [Microsoft.Win32.RegistryValueKind]::String, [Microsoft.Win32.RegistryValueKind]::ExpandString
    if ($ok -notcontains $Current.Kind) {
        throw "$Name has registry type $($Current.Kind); only REG_SZ and REG_EXPAND_SZ are edited."
    }
}

# Appends $Dir to Path unless an equal entry exists. $BeforeWrite runs right
# before the write (snapshot); if it throws, nothing is written.
# Returns 'Added', 'Present' or 'TooLong'.
function Add-PathEntry {
    param(
        [Parameter(Mandatory)] [string]$Dir,
        [string]$KeyPath = $script:EnvKeyPath,
        [string]$Name = 'Path',
        [scriptblock]$BeforeWrite = {}
    )
    $current = Get-RawEnvValue $KeyPath $Name
    if ($null -eq $current) {
        $newValue = $Dir
        $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
    } else {
        Assert-StringKind $current $Name
        if (Test-PathEntry $current.Value $Dir) { return 'Present' }
        $newValue = ((Split-PathEntries $current.Value) + $Dir) -join ';'
        $kind = $current.Kind
    }
    if ($newValue.Length -gt $script:ValueLimit) { return 'TooLong' }
    & $BeforeWrite
    Set-RawEnvValue $KeyPath $Name $newValue $kind
    return 'Added'
}

# Removes every entry equal to $Dir from Path, keeping the other entries in
# order and the value kind. An emptied Path is deleted (empty values are not
# allowed). Returns 'Removed' or 'Absent'.
function Remove-PathEntry {
    param(
        [Parameter(Mandatory)] [string]$Dir,
        [string]$KeyPath = $script:EnvKeyPath,
        [string]$Name = 'Path',
        [scriptblock]$BeforeWrite = {}
    )
    $current = Get-RawEnvValue $KeyPath $Name
    if ($null -eq $current) { return 'Absent' }
    Assert-StringKind $current $Name
    $wanted = Get-EntryKey $Dir
    $entries = Split-PathEntries $current.Value
    $kept = @($entries | Where-Object { (Get-EntryKey $_) -ne $wanted })
    if ($kept.Count -eq $entries.Count) { return 'Absent' }
    & $BeforeWrite
    if ($kept.Count -eq 0) {
        Remove-RawEnvValue $KeyPath $Name
    } else {
        Set-RawEnvValue $KeyPath $Name ($kept -join ';') $current.Kind
    }
    return 'Removed'
}

# ---- snapshot in envedit's record format ----

function ConvertTo-RecordField([string]$Text) {
    $Text.Replace('\', '\\').Replace("`t", '\t').Replace("`n", '\n').Replace("`r", '\r')
}

function Get-TypeToken([Microsoft.Win32.RegistryValueKind]$Kind) {
    switch ($Kind) {
        'String' { 'REG_SZ' }
        'ExpandString' { 'REG_EXPAND_SZ' }
        default { 'TYPE:' + [Math]::Max(0, [int]$Kind) }
    }
}

# Writes all variables of the key as an envedit snapshot; returns the file path.
function Write-EnvSnapshot {
    param(
        [string]$KeyPath = $script:EnvKeyPath,
        [string]$Dir = (Get-SnapshotDir),
        [string]$Scope = 'user'
    )
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add('envedit-records 1')
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($KeyPath, $false)
    if ($key) {
        try {
            $names = @($key.GetValueNames() | Where-Object { $_ -ne '' } | Sort-Object { $_.ToUpperInvariant() })
            foreach ($name in $names) {
                $kind = $key.GetValueKind($name)
                $value = ''
                if ($kind -eq 'String' -or $kind -eq 'ExpandString') {
                    $value = [string]$key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                }
                $fields = 'var', $Scope, $name, (Get-TypeToken $kind), $value
                $lines.Add((($fields | ForEach-Object { ConvertTo-RecordField $_ }) -join "`t"))
            }
        } finally {
            $key.Close()
        }
    }
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    $stamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff', [Globalization.CultureInfo]::InvariantCulture)
    $path = Join-Path $Dir "$stamp-$Scope.txt"
    # UTF-8 without BOM, LF line ends: what envedit writes and expects.
    [IO.File]::WriteAllText($path, (($lines -join "`n") + "`n"), (New-Object Text.UTF8Encoding $false))
    return $path
}

# ---- broadcast ----

function Send-EnvironmentChanged {
    if (-not ('EnveditInstall.Native' -as [type])) {
        Add-Type -Namespace EnveditInstall -Name Native -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam,
    uint flags, uint timeout, out UIntPtr result);
'@
    }
    $result = [UIntPtr]::Zero
    # HWND_BROADCAST, WM_SETTINGCHANGE, SMTO_ABORTIFHUNG, 5 s per window
    [EnveditInstall.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result) | Out-Null
}
