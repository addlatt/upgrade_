<#
.SYNOPSIS
    upgrade_ / V0 validation harness - the one-time UEFI boot handoff.

.DESCRIPTION
    Proves (or disproves) the single mechanism the whole walk-away promise
    rests on: that

        bcdedit /set {fwbootmgr} bootsequence {guid}

    boots a USB payload EXACTLY ONCE, and that every failure leaves the
    machine booting Windows normally with no keypress. See docs/VALIDATION.md
    gate V0 and docs/RISKS.md R15.

    This is deliberately NOT a Linux installer. The payload is an instrumented
    EFI program that writes a marker file and reboots, so "did the firmware run
    our entry" is a machine-readable fact, not a human judgement.

    A reboot happens in the middle of the test, so the harness runs twice:

        .\Test-Handoff.ps1 -Arm -PayloadDrive E:      (before the reboot)
        <reboot - the firmware either runs the payload or does not>
        .\Test-Handoff.ps1 -Check                     (after Windows returns)

    -Arm is fully reversible. It exports the BCD first, creates ONE extra boot
    entry, and sets a one-shot that the firmware is supposed to self-clear.
    -Check restores everything regardless of outcome and records one row of
    evidence.

    This harness is the first version of code that will ship in the converter's
    prologue. It is written to that standard - it refuses before it touches
    anything it cannot cleanly undo.

.PARAMETER Arm
    Set up the handoff and stop before the reboot. Requires -PayloadDrive.

.PARAMETER Check
    Run after the reboot. Classify the result, restore the BCD, log a row.

.PARAMETER PayloadDrive
    Drive letter of the FAT32 payload partition (the "stick"), e.g. E: or E.
    Must contain \EFI\BOOT\BOOTX64.EFI. See handoff-payload\README.md.

.PARAMETER SuspendBitLocker
    With -Arm: suspend BitLocker on C: for one reboot before arming, so the
    boot-config change does not trigger a recovery-key prompt. This is the
    same protection the shipping prologue will use.

.PARAMETER FailMode
    Arm a deliberately broken variant to test a failure path:
      NoFile              point the entry at a nonexistent EFI file
      SecureBootUnsigned  (documentation only - use an unsigned payload with
                          Secure Boot ON; the harness records the intent)
      NoSuspend           arm without suspending BitLocker, to confirm the
                          recovery prompt is what suspension prevents

.PARAMETER RestoreBcd
    With -Check: also re-import the exported BCD backup, not just delete the
    test entry. Belt and braces; the delete alone is normally sufficient.

.PARAMETER StateDir
    Where the harness keeps its cross-reboot state and BCD backup.
    Default: %ProgramData%\upgrade_\v0

.PARAMETER ResultsCsv
    Where -Check appends its evidence row.
    Default: the repo's docs\validation-results\v0-handoff.csv if found,
    else %ProgramData%\upgrade_\v0\v0-handoff.csv

.EXAMPLE
    .\Test-Handoff.ps1 -Arm -PayloadDrive E: -SuspendBitLocker
    shutdown /r /t 0
    # ... after it comes back ...
    .\Test-Handoff.ps1 -Check

.EXAMPLE
    .\Test-Handoff.ps1 -Arm -PayloadDrive E: -FailMode NoFile
#>
[CmdletBinding(DefaultParameterSetName = 'Check')]
param(
    [Parameter(ParameterSetName = 'Arm', Mandatory = $true)]
    [switch]$Arm,

    [Parameter(ParameterSetName = 'Arm', Mandatory = $true)]
    [string]$PayloadDrive,

    [Parameter(ParameterSetName = 'Arm')]
    [switch]$SuspendBitLocker,

    [Parameter(ParameterSetName = 'Arm')]
    [ValidateSet('NoFile', 'SecureBootUnsigned', 'NoSuspend')]
    [string]$FailMode,

    [Parameter(ParameterSetName = 'Check', Mandatory = $true)]
    [switch]$Check,

    [Parameter(ParameterSetName = 'Check')]
    [switch]$RestoreBcd,

    [string]$StateDir,
    [string]$ResultsCsv
)

$ErrorActionPreference = 'Stop'
$HarnessVersion = '0.1.0'

# The marker the payload writes to the root of the stick. Keep in sync with
# handoff-payload\startup.nsh.
$FiredMarker = 'fired.txt'

# =============================================================================
#  helpers
# =============================================================================

function Test-Elevated {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Test-UefiBoot {
    # bcdedit {fwbootmgr} only exists on a UEFI-booted machine. On legacy BIOS
    # this whole mechanism does not apply and the test is meaningless.
    if ($env:firmware_type -eq 'UEFI') { return $true }
    try {
        $out = & bcdedit /enum '{fwbootmgr}' 2>&1
        return ($LASTEXITCODE -eq 0 -and ($out -join "`n") -match 'bootsequence|displayorder')
    } catch { return $false }
}

function Get-DriveRoot {
    param([string]$Letter)
    $l = $Letter.TrimEnd(':', '\').ToUpper()
    if ($l.Length -ne 1) { throw "PayloadDrive must be a single drive letter, got '$Letter'." }
    "${l}:\"
}

function Get-SecureBootState {
    try { if (Confirm-SecureBootUEFI) { 'on' } else { 'off' } }
    catch { 'unknown' }   # cmdlet throws on legacy BIOS / unsupported
}

function Get-BitLockerState {
    try {
        $v = Get-BitLockerVolume -MountPoint 'C:' -ErrorAction Stop
        switch ($v.ProtectionStatus) { 'On' { 'on' } 'Off' { 'off' } default { 'unknown' } }
    } catch { 'unknown' }
}

function Get-FwbootmgrSnapshot {
    # Capture displayorder + bootsequence so -Check can prove the boot order
    # was not permanently reordered (the 'reordered' failure).
    $raw = & bcdedit /enum '{fwbootmgr}' 2>&1
    $text = ($raw -join "`n")
    $display = ''
    $sequence = ''
    if ($text -match '(?m)^\s*displayorder\s+(.+(?:\r?\n\s{20,}.+)*)') { $display = ($matches[1] -replace '\s+', ' ').Trim() }
    if ($text -match '(?m)^\s*bootsequence\s+(.+(?:\r?\n\s{20,}.+)*)') { $sequence = ($matches[1] -replace '\s+', ' ').Trim() }
    [pscustomobject]@{
        DisplayOrder = $display
        BootSequence = $sequence
        Raw          = $text
    }
}

function New-Line { param([string]$s = '', [string]$c = 'Gray') Write-Host $s -ForegroundColor $c }

# =============================================================================
#  state
# =============================================================================

function Resolve-StateDir {
    if ($StateDir) { return $StateDir }
    Join-Path $env:ProgramData 'upgrade_\v0'
}

function Resolve-ResultsCsv {
    param([string]$State)
    if ($ResultsCsv) { return $ResultsCsv }
    # Prefer the repo's evidence file if we're running from the source tree.
    $repo = Join-Path $PSScriptRoot '..\..\docs\validation-results\v0-handoff.csv'
    try { $repo = [IO.Path]::GetFullPath($repo) } catch { }
    if (Test-Path (Split-Path $repo -Parent)) { return $repo }
    Join-Path $State 'v0-handoff.csv'
}

$CsvHeader = 'timestamp,harness,vendor,model,firmware_version,secureboot,bitlocker,payload,failmode,result,keypress_free,windows_returned,notes'

# =============================================================================
#  arm
# =============================================================================

function Invoke-Arm {
    $state = Resolve-StateDir
    New-Item -ItemType Directory -Path $state -Force | Out-Null

    if (Test-Path (Join-Path $state 'handoff-state.json')) {
        throw "A test is already armed (state exists in $state). Run -Check first, or delete the state directory."
    }

    $root = Get-DriveRoot $PayloadDrive
    if (-not (Test-Path $root)) { throw "Payload drive $root not found." }

    # The one refusal that matters most: the entry we are about to create must
    # point at a payload that actually exists. NoFile deliberately skips this.
    $payloadEfi = Join-Path $root 'EFI\BOOT\BOOTX64.EFI'
    if ($FailMode -ne 'NoFile' -and -not (Test-Path $payloadEfi)) {
        throw "No payload at $payloadEfi. See handoff-payload\README.md to build the stick."
    }

    # A stale marker from a previous run would produce a false 'fired-once'.
    $marker = Join-Path $root $FiredMarker
    if (Test-Path $marker) { Remove-Item $marker -Force }

    $cs   = Get-CimInstance Win32_ComputerSystem
    $bios = Get-CimInstance Win32_BIOS
    $sb   = Get-SecureBootState
    $bl   = Get-BitLockerState

    New-Line ''
    New-Line '  upgrade_  V0 handoff test  -  ARM' 'Cyan'
    New-Line "  $($cs.Manufacturer) $($cs.Model)   firmware $($bios.SMBIOSBIOSVersion)" 'DarkGray'
    New-Line "  Secure Boot: $sb    BitLocker(C:): $bl    payload: $root" 'DarkGray'
    if ($FailMode) { New-Line "  FAIL MODE: $FailMode" 'Yellow' }
    New-Line ''

    # 1. Undo button, before anything else.
    $backup = Join-Path $state 'bcd-backup.bin'
    New-Line '  exporting BCD backup...' 'DarkGray'
    & bcdedit /export $backup | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'bcdedit /export failed; refusing to arm without a backup.' }

    # 2. Snapshot boot order for the reordered/persisted checks.
    $before = Get-FwbootmgrSnapshot

    # 3. Suspend BitLocker unless we are specifically testing the no-suspend path.
    $didSuspend = $false
    if ($bl -eq 'on' -and $FailMode -ne 'NoSuspend') {
        if ($SuspendBitLocker) {
            New-Line '  suspending BitLocker for one reboot...' 'DarkGray'
            & manage-bde -protectors -disable C: -rebootcount 1 | Out-Null
            $didSuspend = $true
        } else {
            New-Line '  ! BitLocker is ON and -SuspendBitLocker was not passed.' 'Yellow'
            New-Line '    The return boot may hit a recovery-key prompt. Have the key ready,' 'Yellow'
            New-Line '    or Ctrl+C now and re-run with -SuspendBitLocker.' 'Yellow'
        }
    }

    # 4. The actual sequence under test - verbatim from docs/architecture.md.
    New-Line '  creating one-time boot entry...' 'DarkGray'
    $copyOut = & bcdedit /copy '{bootmgr}' /d 'upgrade_ V0 handoff test' 2>&1
    if ($LASTEXITCODE -ne 0) { throw "bcdedit /copy failed: $copyOut" }
    if (($copyOut -join "`n") -notmatch '\{[0-9a-fA-F-]{36}\}') {
        throw "Could not parse the new entry GUID from: $copyOut"
    }
    $guid = $matches[0]

    $efiPath = if ($FailMode -eq 'NoFile') { '\EFI\BOOT\DOES-NOT-EXIST.EFI' } else { '\EFI\BOOT\BOOTX64.EFI' }
    & bcdedit /set $guid device "partition=$($root.TrimEnd('\'))" | Out-Null
    & bcdedit /set $guid path $efiPath | Out-Null
    & bcdedit /set '{fwbootmgr}' bootsequence $guid | Out-Null
    if ($LASTEXITCODE -ne 0) { & bcdedit /delete $guid | Out-Null; throw 'Setting bootsequence failed; test entry removed.' }

    # 5. Persist everything -Check needs.
    $record = [pscustomobject]@{
        HarnessVersion = $HarnessVersion
        ArmedUtc       = (Get-Date).ToUniversalTime().ToString('o')
        Guid           = $guid
        PayloadRoot    = $root
        PayloadEfi     = $efiPath
        FailMode       = $FailMode
        DidSuspend     = $didSuspend
        Vendor         = $cs.Manufacturer
        Model          = $cs.Model
        Firmware       = $bios.SMBIOSBIOSVersion
        SecureBoot     = $sb
        BitLocker      = $bl
        Before         = $before
        BcdBackup      = $backup
    }
    $record | ConvertTo-Json -Depth 6 | Out-File (Join-Path $state 'handoff-state.json') -Encoding UTF8

    New-Line ''
    New-Line '  ARMED.' 'Green'
    New-Line '  Reboot now, watch what happens, then run:  .\Test-Handoff.ps1 -Check' 'White'
    New-Line ''
    New-Line '  If nothing is watching the screen, that is fine - the payload records' 'DarkGray'
    New-Line '  itself. But note by hand whether any keypress was needed.' 'DarkGray'
    New-Line ''
}

# =============================================================================
#  check
# =============================================================================

function Invoke-Check {
    $state = Resolve-StateDir
    $statePath = Join-Path $state 'handoff-state.json'
    if (-not (Test-Path $statePath)) {
        throw "No armed test found in $state. Run -Arm first."
    }
    $r = Get-Content $statePath -Raw | ConvertFrom-Json

    New-Line ''
    New-Line '  upgrade_  V0 handoff test  -  CHECK' 'Cyan'
    New-Line "  $($r.Vendor) $($r.Model)   firmware $($r.Firmware)" 'DarkGray'
    if ($r.FailMode) { New-Line "  FAIL MODE: $($r.FailMode)" 'Yellow' }
    New-Line ''

    $marker = Join-Path $r.PayloadRoot $FiredMarker
    $fired = Test-Path $marker

    $after = Get-FwbootmgrSnapshot
    $sequenceCleared = [string]::IsNullOrWhiteSpace($after.BootSequence)
    $orderUnchanged  = ($after.DisplayOrder -eq $r.Before.DisplayOrder)

    # Classify.
    $result = 'error'
    if ($r.FailMode -eq 'NoFile' -or $r.FailMode -eq 'SecureBootUnsigned') {
        # Expected outcome for these is a clean fall-through to Windows.
        if (-not $fired -and $orderUnchanged) { $result = 'ignored' }        # PASS for a fail-mode
        elseif ($fired) { $result = 'persisted' }                            # firmware ran a bad/unsigned entry - notable
        else { $result = 'reordered' }
    } else {
        if ($fired -and $sequenceCleared -and $orderUnchanged) { $result = 'fired-once' }
        elseif ($fired -and -not $sequenceCleared)             { $result = 'persisted' }
        elseif (-not $fired -and $orderUnchanged)              { $result = 'ignored' }
        elseif (-not $orderUnchanged)                          { $result = 'reordered' }
    }

    $resultColor = switch ($result) {
        'fired-once' { 'Green' } 'ignored' { 'Yellow' } default { 'Red' }
    }
    New-Line "  marker present:      $fired"
    New-Line "  bootsequence clear:  $sequenceCleared"
    New-Line "  boot order intact:   $orderUnchanged"
    New-Line ''
    New-Line "  RESULT: $result" $resultColor
    if ($result -eq 'persisted')  { New-Line '  -> firmware did NOT consume the one-shot. Shipping prologue needs a cleanup-on-return step.' 'Red' }
    if ($result -eq 'reordered')  { New-Line '  -> firmware permanently changed the boot order. Design input, not just a data point.' 'Red' }
    New-Line ''

    # Restore, always, whatever happened.
    New-Line '  removing test boot entry...' 'DarkGray'
    & bcdedit /delete $r.Guid 2>&1 | Out-Null
    if (-not $sequenceCleared) { & bcdedit /deletevalue '{fwbootmgr}' bootsequence 2>&1 | Out-Null }
    if ($RestoreBcd -and (Test-Path $r.BcdBackup)) {
        New-Line '  re-importing BCD backup...' 'DarkGray'
        & bcdedit /import $r.BcdBackup 2>&1 | Out-Null
    }
    if ($fired) { Remove-Item $marker -Force -ErrorAction SilentlyContinue }

    # Human-supplied fields.
    New-Line ''
    $keypress = Read-Host '  Did the machine reach the payload/Windows with NO keypress? (y/n/na)'
    $winBack  = Read-Host '  Are you back in Windows normally right now? (y/n)'
    $notes    = Read-Host '  Notes (recovery prompt? vendor logo hang? blank = none)'

    # Append evidence.
    $csv = Resolve-ResultsCsv -State $state
    $dir = Split-Path $csv -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    if (-not (Test-Path $csv)) { $CsvHeader | Out-File $csv -Encoding UTF8 }

    function Esc { param($v) '"' + (($v -as [string]) -replace '"', '""') + '"' }
    $row = @(
        Esc((Get-Date).ToUniversalTime().ToString('o'))
        Esc($r.HarnessVersion)
        Esc($r.Vendor); Esc($r.Model); Esc($r.Firmware)
        Esc($r.SecureBoot); Esc($r.BitLocker)
        Esc(Split-Path $r.PayloadEfi -Leaf)
        Esc($r.FailMode)
        Esc($result)
        Esc($keypress); Esc($winBack); Esc($notes)
    ) -join ','
    Add-Content -Path $csv -Value $row -Encoding UTF8

    Remove-Item $statePath -Force
    New-Line ''
    New-Line "  logged to $csv" 'Cyan'
    New-Line ''
}

# =============================================================================
#  main
# =============================================================================

if (-not (Test-Elevated)) {
    throw 'Run this from an elevated PowerShell (Administrator). bcdedit requires it.'
}
if (-not (Test-UefiBoot)) {
    throw 'This machine is not UEFI-booted, so {fwbootmgr} does not exist. V0 does not apply to legacy BIOS.'
}

if ($Arm)   { Invoke-Arm;   return }
if ($Check) { Invoke-Check; return }
