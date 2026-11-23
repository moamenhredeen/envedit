<#
.SYNOPSIS
Installs envedit for the current user.

.DESCRIPTION
Builds the release binary (or takes -ExePath), copies it to
%LOCALAPPDATA%\Programs\envedit, creates a Start menu shortcut, registers
envedit for Win+R (App Paths) and appends the install folder to the User Path.
Running it again upgrades the installed binary. No administrator rights needed.

.PARAMETER ExePath
Install this envedit.exe instead of building from source (no Rust toolchain needed).

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts\install.ps1

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -ExePath C:\Downloads\envedit.exe
#>
[CmdletBinding()]
param(
    [string]$ExePath
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'envedit-install-lib.ps1')

try {
    Write-Host 'Installing envedit'
    Write-ElevationWarning

    # 1. Binary: everything that can fail without side effects comes first.
    if ($ExePath) {
        if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
            throw "File not found: $ExePath"
        }
        $source = (Resolve-Path -LiteralPath $ExePath).Path
        Write-Step "Using $source"
    } else {
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
            throw 'cargo was not found. Install the Rust toolchain, or pass -ExePath with a built envedit.exe.'
        }
        $repo = Split-Path $PSScriptRoot -Parent
        Write-Step 'Building the release binary (cargo build --release)'
        Push-Location $repo
        try {
            & cargo build --release
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE." }
        } finally {
            Pop-Location
        }
        $source = Join-Path $repo 'target\release\envedit.exe'
    }
    Assert-NotRunning

    # 2. Files. Copy under a temporary name, then swap, so a failed copy never
    #    leaves a half-written envedit.exe.
    $dir = Get-InstallDir
    $exe = Get-InstalledExe
    $upgrade = Test-Path -LiteralPath $exe
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $temp = Join-Path $dir 'envedit.exe.new'
    Copy-Item -LiteralPath $source -Destination $temp -Force
    try {
        if ($upgrade) {
            # [NullString]: PowerShell would pass $null as "" (no backup file wanted).
            [IO.File]::Replace($temp, $exe, [NullString]::Value)
        } else {
            [IO.File]::Move($temp, $exe)
        }
    } finally {
        Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue
    }
    foreach ($file in 'uninstall.ps1', 'envedit-install-lib.ps1') {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $dir -Force
    }
    if ($upgrade) { Write-Step "Upgraded $exe" } else { Write-Step "Installed $exe" }

    # 3. Start menu shortcut.
    $lnkPath = Get-ShortcutPath
    $shell = New-Object -ComObject WScript.Shell
    $lnk = $shell.CreateShortcut($lnkPath)
    $lnk.TargetPath = $exe
    $lnk.WorkingDirectory = $dir
    $lnk.Description = 'Edit environment variables'
    $lnk.Save()
    Write-Step "Start menu shortcut: $lnkPath"

    # 4. Win+R via App Paths.
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($script:AppPathsKey)
    try {
        $key.SetValue('', $exe, [Microsoft.Win32.RegistryValueKind]::String)
        $key.SetValue('Path', $dir, [Microsoft.Win32.RegistryValueKind]::String)
    } finally {
        $key.Close()
    }
    Write-Step "Win+R registration: HKCU\$($script:AppPathsKey)"

    # 5. User Path, snapshot first.
    $script:snapshot = $null
    $result = Add-PathEntry -Dir $dir -BeforeWrite { $script:snapshot = Write-EnvSnapshot }
    switch ($result) {
        'Added' {
            Send-EnvironmentChanged
            Write-Step "Added $dir to the User Path (previous variables saved to $($script:snapshot))"
        }
        'Present' { Write-Step 'User Path already contains the install folder' }
        'TooLong' {
            Write-Warning "User Path would exceed 32767 characters; it was not changed, so 'envedit' will not start from a terminal. Start menu and Win+R work."
        }
    }

    Write-Host ''
    Write-Host 'envedit is installed. Start it from the Start menu, with Win+R -> envedit, or with `envedit` in a new terminal.'
    if ($result -eq 'Added') { Write-Host 'Terminals that are already open need a restart to see the new Path.' }
    Write-Host "To remove it: powershell -ExecutionPolicy Bypass -File `"$(Join-Path $dir 'uninstall.ps1')`""
    exit 0
} catch {
    Write-Host "Install failed: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
