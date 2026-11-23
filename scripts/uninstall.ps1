<#
.SYNOPSIS
Removes envedit for the current user.

.DESCRIPTION
Removes the installed binary and folder, the Start menu shortcut, the Win+R
registration and the install folder's User Path entry. Snapshots in
%LOCALAPPDATA%\envedit are kept. Also installed next to envedit.exe, so it
works without the repository.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\envedit\uninstall.ps1"
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'envedit-install-lib.ps1')

try {
    Write-Host 'Removing envedit'
    Write-ElevationWarning

    $dir = Get-InstallDir
    $lnkPath = Get-ShortcutPath
    $appPaths = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($script:AppPathsKey)
    if ($appPaths) { $appPaths.Close() }
    $pathValue = Get-RawEnvValue
    $inPath = ($null -ne $pathValue) -and (Test-PathEntry $pathValue.Value $dir)
    $installed = (Test-Path -LiteralPath $dir) -or (Test-Path -LiteralPath $lnkPath) -or ($null -ne $appPaths) -or $inPath
    if (-not $installed) {
        Write-Host 'envedit is not installed. Nothing to do.'
        exit 0
    }
    Assert-NotRunning

    # Reverse order of install.
    $script:snapshot = $null
    $result = Remove-PathEntry -Dir $dir -BeforeWrite { $script:snapshot = Write-EnvSnapshot }
    if ($result -eq 'Removed') {
        Send-EnvironmentChanged
        Write-Step "Removed $dir from the User Path (previous variables saved to $($script:snapshot))"
    }

    if ($null -ne $appPaths) {
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($script:AppPathsKey, $false)
        Write-Step 'Removed the Win+R registration'
    }

    if (Test-Path -LiteralPath $lnkPath) {
        Remove-Item -LiteralPath $lnkPath -Force
        Write-Step 'Removed the Start menu shortcut'
    }

    if (Test-Path -LiteralPath $dir) {
        # This script may run from the install folder; leave it before deleting.
        if ((Get-Location).ProviderPath -like "$dir*") { Set-Location $HOME }
        Remove-Item -LiteralPath $dir -Recurse -Force
        Write-Step "Removed $dir"
    }

    Write-Host ''
    Write-Host "envedit is removed. Snapshots are kept in $(Split-Path (Get-SnapshotDir) -Parent)."
    exit 0
} catch {
    Write-Host "Uninstall failed: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
