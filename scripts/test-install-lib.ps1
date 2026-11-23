# Tests for envedit-install-lib.ps1. Uses the scratch key HKCU\Software\envedit-test
# and a temp folder; the real environment is never touched.
# Run: powershell -ExecutionPolicy Bypass -File scripts\test-install-lib.ps1  (exit code 0 = pass)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'envedit-install-lib.ps1')

$scratch = 'Software\envedit-test\Env'
$tempDir = Join-Path ([IO.Path]::GetTempPath()) "envedit-install-test-$PID"
$script:failures = 0

function Check([string]$Name, [bool]$Condition) {
    if ($Condition) { Write-Host "ok   $Name" } else { Write-Host "FAIL $Name" -ForegroundColor Red; $script:failures++ }
}

function Reset($Value, [Microsoft.Win32.RegistryValueKind]$Kind) {
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('Software\envedit-test', $false)
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($scratch)
    if ($null -ne $Value) { $key.SetValue('Path', $Value, $Kind) }
    $key.Close()
}

function Raw { Get-RawEnvValue $scratch 'Path' }

$dir = 'C:\Users\someone\AppData\Local\Programs\envedit'
$sz = [Microsoft.Win32.RegistryValueKind]::String
$expand = [Microsoft.Win32.RegistryValueKind]::ExpandString

try {
    # Append keeps REG_SZ
    Reset 'C:\a;C:\b' $sz
    Check 'add to REG_SZ returns Added' ((Add-PathEntry -Dir $dir -KeyPath $scratch) -eq 'Added')
    Check 'REG_SZ kind kept' ((Raw).Kind -eq $sz)
    Check 'entry appended last' ((Raw).Value -eq "C:\a;C:\b;$dir")

    # Append keeps REG_EXPAND_SZ and leaves %VAR% unexpanded
    Reset '%SystemRoot%\x;C:\b' $expand
    Add-PathEntry -Dir $dir -KeyPath $scratch | Out-Null
    Check 'REG_EXPAND_SZ kind kept' ((Raw).Kind -eq $expand)
    Check '%VAR% not expanded' ((Raw).Value -eq "%SystemRoot%\x;C:\b;$dir")

    # No duplicates: same, other case, trailing backslash, via %VAR%
    Check 'second add is Present' ((Add-PathEntry -Dir $dir -KeyPath $scratch) -eq 'Present')
    Check 'case variant is Present' ((Add-PathEntry -Dir $dir.ToUpperInvariant() -KeyPath $scratch) -eq 'Present')
    Check 'trailing backslash is Present' ((Add-PathEntry -Dir "$dir\" -KeyPath $scratch) -eq 'Present')
    Reset '%LOCALAPPDATA%\Programs\envedit' $expand
    Check '%VAR% form is Present' ((Add-PathEntry -Dir (Join-Path $env:LOCALAPPDATA 'Programs\envedit') -KeyPath $scratch) -eq 'Present')

    # Missing Path is created as REG_EXPAND_SZ
    Reset $null $sz
    Check 'missing Path: Added' ((Add-PathEntry -Dir $dir -KeyPath $scratch) -eq 'Added')
    Check 'created as REG_EXPAND_SZ' ((Raw).Kind -eq $expand -and (Raw).Value -eq $dir)

    # Over limit is refused, value unchanged, BeforeWrite not run
    $long = 'C:\' + ('x' * 32760)
    Reset $long $sz
    $ran = $false
    Check 'over-limit returns TooLong' ((Add-PathEntry -Dir $dir -KeyPath $scratch -BeforeWrite { $script:ran = $true }) -eq 'TooLong')
    Check 'over-limit leaves value' ((Raw).Value -eq $long)
    Check 'over-limit skips BeforeWrite' (-not $ran)

    # BeforeWrite failure blocks the write
    Reset 'C:\a' $sz
    try { Add-PathEntry -Dir $dir -KeyPath $scratch -BeforeWrite { throw 'snapshot failed' } | Out-Null } catch { }
    Check 'failed BeforeWrite blocks write' ((Raw).Value -eq 'C:\a')

    # Remove keeps other entries in order and the kind
    Reset "C:\a;$dir;C:\b;$($dir.ToUpperInvariant())\;C:\c" $expand
    Check 'remove returns Removed' ((Remove-PathEntry -Dir $dir -KeyPath $scratch) -eq 'Removed')
    Check 'others kept in order' ((Raw).Value -eq 'C:\a;C:\b;C:\c')
    Check 'kind kept on remove' ((Raw).Kind -eq $expand)
    Check 'second remove is Absent' ((Remove-PathEntry -Dir $dir -KeyPath $scratch) -eq 'Absent')

    # Removing the only entry deletes the value
    Reset $dir $sz
    Remove-PathEntry -Dir $dir -KeyPath $scratch | Out-Null
    Check 'emptied Path deleted' ($null -eq (Raw))
    Check 'remove on missing Path is Absent' ((Remove-PathEntry -Dir $dir -KeyPath $scratch) -eq 'Absent')

    # Snapshot in envedit's record format
    Reset "C:\tab`there;%X%\y" $expand
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($scratch, $true)
    $key.SetValue('aaa', 'back\slash', $sz)
    $key.SetValue('BIN', [byte[]](1, 2), [Microsoft.Win32.RegistryValueKind]::Binary)
    $key.Close()
    $file = Write-EnvSnapshot -KeyPath $scratch -Dir $tempDir
    $bytes = [IO.File]::ReadAllBytes($file)
    $text = [Text.Encoding]::UTF8.GetString($bytes)
    $expected = "envedit-records 1`n" +
        "var`tuser`taaa`tREG_SZ`tback\\slash`n" +
        "var`tuser`tBIN`tTYPE:3`t`n" +
        "var`tuser`tPath`tREG_EXPAND_SZ`tC:\\tab\there;%X%\\y`n"
    Check 'snapshot name format' ((Split-Path $file -Leaf) -match '^\d{8}-\d{6}-\d{3}-user\.txt$')
    Check 'snapshot has no BOM' ($bytes[0] -ne 0xEF)
    Check 'snapshot content' ($text -ceq $expected)
} finally {
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('Software\envedit-test', $false)
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}

$left = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\envedit-test')
Check 'scratch key removed' ($null -eq $left)

if ($script:failures -gt 0) {
    Write-Host "$($script:failures) check(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host 'all checks passed'
exit 0
