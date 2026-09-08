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

    Both payloads self-record. The UEFI Shell payload writes fired.txt to the
    stick root (startup.nsh); the signed shim payload's grub.cfg saves
    upg_fired=1 into EFI\BOOT\grubenv on the stick (GRUB save_env). -Check
    reads either, so "did our entry run" never depends on a human watching.

    Refusals before touching anything (0.2.0): BitLocker state that cannot be
    determined refuses to arm - it is read with Get-BitLockerVolume, falling
    back to manage-bde for editions without the PowerShell module (Home) -
    and BitLocker ON refuses to arm unless -SuspendBitLocker is passed or the
    NoSuspend fail-mode is the experiment being run.

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

.PARAMETER SelfTest
    Run the harness's own logic tests (result classification, manage-bde
    parsing, grubenv marker handling) against fabricated inputs. Needs no
    elevation, no UEFI, touches nothing. CLAUDE.md rule #5, level 1.

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

    [Parameter(ParameterSetName = 'SelfTest', Mandatory = $true)]
    [switch]$SelfTest,

    [string]$StateDir,
    [string]$ResultsCsv
)

$ErrorActionPreference = 'Stop'
$HarnessVersion = '0.2.0'

# The marker the Shell payload writes to the root of the stick. Keep in sync
# with handoff-payload\startup.nsh.
$FiredMarker = 'fired.txt'
# The GRUB environment block the shim payload's grub.cfg writes into. Keep in
# sync with handoff-payload\grub.cfg. GRUB's save_env rewrites this file in
# place, so it must exist (1024 bytes, GRUB's header) before the boot.
$GrubEnvRel   = 'EFI\BOOT\grubenv'
$GrubFiredVar = 'upg_fired'

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

function ConvertFrom-ManageBdeStatus {
    # Pure parse of `manage-bde -status C:` output -> on / off / unknown.
    # Only the English "Protection Status: Protection On|Off" line is
    # understood; anything else (localized Windows, an error, no BitLocker
    # stack) is 'unknown', which -Arm refuses on. Better a refusal than a
    # guess about whether the return boot will demand a recovery key.
    param([string[]]$Lines)
    $text = (@($Lines) -join "`n")
    if ($text -match '(?im)^\s*Protection Status:\s*Protection (On|Off)\s*$') {
        return $matches[1].ToLower()
    }
    'unknown'
}

function Get-BitLockerState {
    # Returns @{ State = on|off|unknown; Source = cmdlet|manage-bde|none }.
    # Get-BitLockerVolume lives in the BitLocker module, which Windows Home
    # editions do not ship even though Device Encryption (BitLocker under
    # another name) may be ON. manage-bde.exe is present on every edition,
    # so it is the fallback - not a replacement, because its output is text.
    try {
        $v = Get-BitLockerVolume -MountPoint 'C:' -ErrorAction Stop
        $st = switch ($v.ProtectionStatus) { 'On' { 'on' } 'Off' { 'off' } default { 'unknown' } }
        if ($st -ne 'unknown') { return [pscustomobject]@{ State = $st; Source = 'cmdlet' } }
    } catch { }
    try {
        $out = & manage-bde -status C: 2>&1
        $st = ConvertFrom-ManageBdeStatus -Lines @($out | ForEach-Object { "$_" })
        if ($st -ne 'unknown') { return [pscustomobject]@{ State = $st; Source = 'manage-bde' } }
        return [pscustomobject]@{ State = 'unknown'; Source = 'none'; Raw = (@($out) -join "`n") }
    } catch {
        return [pscustomobject]@{ State = 'unknown'; Source = 'none'; Raw = "$_" }
    }
}

function New-GrubEnvBlock {
    # A clean 1024-byte GRUB environment block (what grub-editenv create
    # makes): the header line, padded with '#' to exactly 1024 bytes.
    $header = "# GRUB Environment Block`n"
    $bytes = New-Object byte[] 1024
    $h = [Text.Encoding]::ASCII.GetBytes($header)
    [Array]::Copy($h, $bytes, $h.Length)
    for ($i = $h.Length; $i -lt 1024; $i++) { $bytes[$i] = 0x23 }   # '#'
    , $bytes
}

function Test-GrubEnvFired {
    # Pure: does a grubenv's content carry our variable set to 1?
    param([byte[]]$Bytes)
    if (-not $Bytes -or $Bytes.Length -eq 0) { return $false }
    $text = [Text.Encoding]::ASCII.GetString($Bytes)
    [bool]($text -match ('(?m)^' + [regex]::Escape($GrubFiredVar) + '=1\s*$'))
}

function Reset-GrubEnv {
    # Put a clean block in place if this stick carries the shim payload
    # (grubx64.efi beside BOOTX64.EFI), so a stale upg_fired=1 from a previous
    # run cannot produce a false 'fired-once'. The Shell stick has no grubenv
    # and gets none.
    param([string]$Root)
    $env = Join-Path $Root $GrubEnvRel
    $grub = Join-Path $Root 'EFI\BOOT\grubx64.efi'
    if ((Test-Path $env) -or (Test-Path $grub)) {
        [IO.File]::WriteAllBytes($env, (New-GrubEnvBlock))
        return $true
    }
    $false
}

function Get-HandoffResult {
    # Pure classifier. Inputs are the three facts -Check establishes plus the
    # fail-mode that was armed; output is the CSV result vocabulary
    # (docs/validation-results/README.md).
    param([bool]$Fired, [bool]$SequenceCleared, [bool]$OrderUnchanged, [string]$FailMode)
    if ($FailMode -eq 'NoFile' -or $FailMode -eq 'SecureBootUnsigned') {
        # Expected outcome for these is a clean fall-through to Windows.
        if (-not $Fired -and $OrderUnchanged) { return 'ignored' }        # PASS for a fail-mode
        if ($Fired) { return 'persisted' }                                # firmware ran a bad/unsigned entry - notable
        return 'reordered'
    }
    if ($Fired -and $SequenceCleared -and $OrderUnchanged) { return 'fired-once' }
    if ($Fired -and -not $SequenceCleared)                 { return 'persisted' }
    if (-not $Fired -and $OrderUnchanged)                  { return 'ignored' }
    if (-not $OrderUnchanged)                              { return 'reordered' }
    'error'
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
    $grubEnvReset = Reset-GrubEnv -Root $root

    $cs   = Get-CimInstance Win32_ComputerSystem
    $bios = Get-CimInstance Win32_BIOS
    $os   = Get-CimInstance Win32_OperatingSystem
    $sb   = Get-SecureBootState
    $blq  = Get-BitLockerState
    $bl   = $blq.State

    New-Line ''
    New-Line '  upgrade_  V0 handoff test  -  ARM' 'Cyan'
    New-Line "  $($cs.Manufacturer) $($cs.Model)   firmware $($bios.SMBIOSBIOSVersion)" 'DarkGray'
    New-Line "  $($os.Caption) build $($os.BuildNumber)" 'DarkGray'
    New-Line "  Secure Boot: $sb    BitLocker(C:): $bl (via $($blq.Source))    payload: $root" 'DarkGray'
    if ($grubEnvReset) { New-Line "  shim payload detected: $GrubEnvRel reset to a clean block" 'DarkGray' }
    if ($FailMode) { New-Line "  FAIL MODE: $FailMode" 'Yellow' }
    New-Line ''

    # Refuse-by-default, before the BCD is touched. An unknown BitLocker
    # state means we cannot say whether the return boot will stop at a
    # recovery-key prompt - on a machine nobody is watching, in the shipping
    # prologue. No flag overrides this: make the state known instead (check
    # Settings > Device encryption, decrypt, or report the manage-bde output
    # below so the parser learns it).
    if ($bl -eq 'unknown') {
        if ($blq.Raw) { New-Line "  manage-bde said:`n$($blq.Raw)" 'DarkGray' }
        throw 'BitLocker state on C: could not be determined; refusing to arm. Make it known (Settings > Privacy & security > Device encryption, or manage-bde -status C:) and re-run.'
    }
    if ($bl -eq 'on' -and -not $SuspendBitLocker -and $FailMode -ne 'NoSuspend') {
        throw 'BitLocker is ON. Refusing to arm without suspension: re-run with -SuspendBitLocker (the shipping default), or -FailMode NoSuspend if the no-suspend path is the experiment. Have the recovery key saved somewhere that is not this computer first.'
    }

    # 1. Undo button, before anything else.
    $backup = Join-Path $state 'bcd-backup.bin'
    New-Line '  exporting BCD backup...' 'DarkGray'
    & bcdedit /export $backup | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'bcdedit /export failed; refusing to arm without a backup.' }

    # 2. Snapshot boot order for the reordered/persisted checks.
    $before = Get-FwbootmgrSnapshot

    # 3. Suspend BitLocker unless we are specifically testing the no-suspend
    #    path (the refusal above guarantees -SuspendBitLocker is set here).
    $didSuspend = $false
    if ($bl -eq 'on' -and $FailMode -ne 'NoSuspend') {
        New-Line '  suspending BitLocker for one reboot...' 'DarkGray'
        $susOut = & manage-bde -protectors -disable C: -rebootcount 1 2>&1
        if ($LASTEXITCODE -ne 0) { throw "manage-bde could not suspend BitLocker; refusing to arm. Output: $($susOut -join ' ')" }
        $didSuspend = $true
    }
    if ($bl -eq 'on' -and $FailMode -eq 'NoSuspend') {
        New-Line '  ! NoSuspend: BitLocker stays ON through the handoff. Recovery key at hand?' 'Yellow'
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
        OsCaption      = $os.Caption
        OsBuild        = $os.BuildNumber
        SecureBoot     = $sb
        BitLocker      = $bl
        BitLockerSource= $blq.Source
        GrubEnvArmed   = $grubEnvReset
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
    $grubEnv = Join-Path $r.PayloadRoot $GrubEnvRel
    $firedVia = @()
    if (Test-Path $marker) { $firedVia += 'fired.txt' }
    if (Test-Path $grubEnv) {
        if (Test-GrubEnvFired -Bytes ([IO.File]::ReadAllBytes($grubEnv))) { $firedVia += 'grubenv' }
    }
    $fired = ($firedVia.Count -gt 0)

    $after = Get-FwbootmgrSnapshot
    $sequenceCleared = [string]::IsNullOrWhiteSpace($after.BootSequence)

    # -Arm creates a temporary boot entry ($r.Guid) and points bootsequence at
    # it. That entry stays in {fwbootmgr} displayorder until the cleanup below
    # removes it - and we snapshot the order HERE, before that cleanup runs, so
    # our own one-shot is still listed. Comparing raw would flag every run as
    # 'reordered' on firmware that keeps the entry in displayorder (e.g. OVMF).
    # Filter our own entry out of the after-order, so 'reordered' means the
    # firmware moved the REAL entries - a genuine danger - not that our test
    # entry is still present pre-cleanup. (Confirmed on QEMU+OVMF 2026-08-23:
    # after removing $r.Guid the order is identical to Before, token for token.)
    $afterTokens  = @($after.DisplayOrder   -split '\s+' | Where-Object { $_ -and $_ -ne $r.Guid })
    $beforeTokens = @($r.Before.DisplayOrder -split '\s+' | Where-Object { $_ })
    $orderUnchanged = (($afterTokens -join ' ') -eq ($beforeTokens -join ' '))

    # Classify (pure function, self-tested).
    $result = Get-HandoffResult -Fired $fired -SequenceCleared $sequenceCleared -OrderUnchanged $orderUnchanged -FailMode ([string]$r.FailMode)

    $resultColor = switch ($result) {
        'fired-once' { 'Green' } 'ignored' { 'Yellow' } default { 'Red' }
    }
    New-Line "  marker present:      $fired$(if ($fired) { ' (' + ($firedVia -join ', ') + ')' })"
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
    if (Test-Path $marker) { Remove-Item $marker -Force -ErrorAction SilentlyContinue }
    Reset-GrubEnv -Root $r.PayloadRoot | Out-Null

    # Human-supplied fields.
    New-Line ''
    $keypress = Read-Host '  Did the machine reach the payload/Windows with NO keypress? (y/n/na)'
    $winBack  = Read-Host '  Are you back in Windows normally right now? (y/n)'
    $notes    = Read-Host '  Notes (recovery prompt? vendor logo hang? blank = none)'

    # Harness-written facts go first in the notes, so the row says what the
    # machine was and how the marker was read, independent of the operator.
    $auto = "[harness: os=$($r.OsCaption) $($r.OsBuild); bitlocker-via=$($r.BitLockerSource); fired-via=$(if ($fired) { $firedVia -join '+' } else { 'none' })]"
    $notes = if ([string]::IsNullOrWhiteSpace($notes)) { $auto } else { "$auto $notes" }

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
#  self-test (logic only - nothing here touches the machine)
# =============================================================================

function Invoke-SelfTest {
    $cases = @(
        # result classification: every branch of the vocabulary
        @{ Name = 'classify: fired + cleared + order intact is fired-once'
           Run = { Get-HandoffResult -Fired $true -SequenceCleared $true -OrderUnchanged $true -FailMode '' }; Expect = 'fired-once' }
        @{ Name = 'classify: fired but one-shot not cleared is persisted'
           Run = { Get-HandoffResult -Fired $true -SequenceCleared $false -OrderUnchanged $true -FailMode '' }; Expect = 'persisted' }
        @{ Name = 'classify: not fired, order intact is ignored (fail-safe)'
           Run = { Get-HandoffResult -Fired $false -SequenceCleared $true -OrderUnchanged $true -FailMode '' }; Expect = 'ignored' }
        @{ Name = 'classify: fired, cleared, but order changed is reordered'
           Run = { Get-HandoffResult -Fired $true -SequenceCleared $true -OrderUnchanged $false -FailMode '' }; Expect = 'reordered' }
        @{ Name = 'classify: not fired and order changed is reordered'
           Run = { Get-HandoffResult -Fired $false -SequenceCleared $true -OrderUnchanged $false -FailMode '' }; Expect = 'reordered' }
        @{ Name = 'classify: NoSuspend is a baseline-shaped row (fired-once)'
           Run = { Get-HandoffResult -Fired $true -SequenceCleared $true -OrderUnchanged $true -FailMode 'NoSuspend' }; Expect = 'fired-once' }
        @{ Name = 'classify: NoFile not fired, order intact is ignored (the pass)'
           Run = { Get-HandoffResult -Fired $false -SequenceCleared $true -OrderUnchanged $true -FailMode 'NoFile' }; Expect = 'ignored' }
        @{ Name = 'classify: SecureBootUnsigned that fired is persisted (loud)'
           Run = { Get-HandoffResult -Fired $true -SequenceCleared $true -OrderUnchanged $true -FailMode 'SecureBootUnsigned' }; Expect = 'persisted' }
        @{ Name = 'classify: SecureBootUnsigned not fired but order changed is reordered'
           Run = { Get-HandoffResult -Fired $false -SequenceCleared $true -OrderUnchanged $false -FailMode 'SecureBootUnsigned' }; Expect = 'reordered' }
        # manage-bde parsing: the Home-edition fallback
        @{ Name = 'manage-bde: Protection On parses as on'
           Run = { ConvertFrom-ManageBdeStatus -Lines @('BitLocker Drive Encryption: Configuration Tool version 10.0.19041', 'Volume C: [Windows]', '[OS Volume]', '', '    Size:                 237.00 GB', '    Conversion Status:    Fully Encrypted', '    Protection Status:    Protection On', '    Lock Status:          Unlocked') }; Expect = 'on' }
        @{ Name = 'manage-bde: Protection Off parses as off'
           Run = { ConvertFrom-ManageBdeStatus -Lines @('Volume C: [Windows]', '    Conversion Status:    Fully Decrypted', '    Protection Status:    Protection Off') }; Expect = 'off' }
        @{ Name = 'manage-bde: encrypted but suspended (Protection Off) is off - no TPM prompt'
           Run = { ConvertFrom-ManageBdeStatus -Lines @('    Conversion Status:    Fully Encrypted', '    Protection Status:    Protection Off') }; Expect = 'off' }
        @{ Name = 'manage-bde: an access-denied error is unknown, not off'
           Run = { ConvertFrom-ManageBdeStatus -Lines @('ERROR: An attempt to access a required resource was denied.', '', 'Check that you have administrative rights on the computer.') }; Expect = 'unknown' }
        @{ Name = 'manage-bde: localized output is unknown, not a guess'
           Run = { ConvertFrom-ManageBdeStatus -Lines @('    Schutzstatus:         Der Schutz ist aktiviert.') }; Expect = 'unknown' }
        @{ Name = 'manage-bde: empty output is unknown'
           Run = { ConvertFrom-ManageBdeStatus -Lines @() }; Expect = 'unknown' }
        # grubenv marker: the shim payload's self-record
        @{ Name = 'grubenv: a clean block is exactly 1024 bytes with the GRUB header'
           Run = { $b = New-GrubEnvBlock; if ($b.Length -eq 1024 -and [Text.Encoding]::ASCII.GetString($b, 0, 25) -eq "# GRUB Environment Block`n" -and $b[1023] -eq 0x23) { 'ok' } else { "bad: len=$($b.Length)" } }; Expect = 'ok' }
        @{ Name = 'grubenv: a clean block is not fired'
           Run = { Test-GrubEnvFired -Bytes (New-GrubEnvBlock) }; Expect = $false }
        @{ Name = 'grubenv: upg_fired=1 written by save_env is fired'
           Run = { $t = "# GRUB Environment Block`nupg_fired=1`n" + ('#' * 990); Test-GrubEnvFired -Bytes ([Text.Encoding]::ASCII.GetBytes($t)) }; Expect = $true }
        @{ Name = 'grubenv: upg_fired=0 is not fired'
           Run = { $t = "# GRUB Environment Block`nupg_fired=0`n" + ('#' * 990); Test-GrubEnvFired -Bytes ([Text.Encoding]::ASCII.GetBytes($t)) }; Expect = $false }
        @{ Name = 'grubenv: an unrelated variable is not fired'
           Run = { $t = "# GRUB Environment Block`nboot_success=1`n" + ('#' * 990); Test-GrubEnvFired -Bytes ([Text.Encoding]::ASCII.GetBytes($t)) }; Expect = $false }
        @{ Name = 'grubenv: empty file is not fired'
           Run = { Test-GrubEnvFired -Bytes ([byte[]]@()) }; Expect = $false }
        # drive-letter parsing feeding the bcdedit device line
        @{ Name = 'drive: E: normalizes to E:\'
           Run = { Get-DriveRoot 'E:' }; Expect = 'E:\' }
        @{ Name = 'drive: lowercase e normalizes to E:\'
           Run = { Get-DriveRoot 'e' }; Expect = 'E:\' }
        @{ Name = 'drive: a path is refused'
           Run = { try { Get-DriveRoot 'E:\EFI'; 'accepted' } catch { 'refused' } }; Expect = 'refused' }
    )
    $failed = 0
    New-Line ''
    New-Line "  upgrade_  V0 handoff harness $HarnessVersion  -  SELF-TEST" 'Cyan'
    New-Line ''
    foreach ($c in $cases) {
        $got = & $c.Run
        $ok = ("$got" -eq "$($c.Expect)")
        if ($ok) { New-Line "    PASS  $($c.Name)" 'Green' }
        else { New-Line "    FAIL  $($c.Name)  (expected '$($c.Expect)', got '$got')" 'Red'; $failed++ }
    }
    New-Line ''
    if ($failed -gt 0) { New-Line "  $failed check(s) failed" 'Red'; exit 1 }
    New-Line '  all checks passed' 'Green'
    New-Line ''
}

# =============================================================================
#  main
# =============================================================================

if ($SelfTest) { Invoke-SelfTest; return }

if (-not (Test-Elevated)) {
    throw 'Run this from an elevated PowerShell (Administrator). bcdedit requires it.'
}
if (-not (Test-UefiBoot)) {
    throw 'This machine is not UEFI-booted, so {fwbootmgr} does not exist. V0 does not apply to legacy BIOS.'
}

if ($Arm)   { Invoke-Arm;   return }
if ($Check) { Invoke-Check; return }
