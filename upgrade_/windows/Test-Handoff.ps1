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

    One click (0.3.0): -Arm -Auto registers an elevated scheduled task that
    runs -Check -Auto itself at the next logon, then reboots. On return the
    check classifies, cleans up, asks the one human question in a popup
    (did any key have to be pressed? - times out to 'unknown'), and writes
    the row to v0-handoff.csv on the stick. The stick is found again by its
    volume id, not its drive letter. This is the prologue's walk-away and
    cleanup-on-return shape, built here first. One stick carries both
    payloads: -Payload shim (signed, EFI\BOOT\BOOTX64.EFI, the product
    path) or shell (unsigned, EFI\SHELL\SHELLX64.EFI, the matrix rows).

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

.PARAMETER Payload
    With -Arm: which payload on the stick the one-time entry points at.
      shim   (default) Fedora's signed shim -> grub -> grub.cfg, at
             \EFI\BOOT\BOOTX64.EFI. Works with Secure Boot on. The product path.
      shell  the unsigned UEFI Shell at \EFI\SHELL\SHELLX64.EFI (startup.nsh at
             the stick root). Secure Boot off, or the SecureBootUnsigned row.

.PARAMETER Auto
    With -Arm: register the return check as a one-shot elevated logon task
    and reboot after a countdown - the one-click flow. With -Check: run
    without a console operator (popups instead of Read-Host; 'windows
    returned' is derived; the CSV lands on the stick).

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
    [ValidateSet('shim', 'shell')]
    [string]$Payload = 'shim',

    [Parameter(ParameterSetName = 'Arm')]
    [Parameter(ParameterSetName = 'Check')]
    [switch]$Auto,

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
$HarnessVersion = '0.3.1'

# The marker the Shell payload writes to the root of the stick. Keep in sync
# with handoff-payload\startup.nsh.
$FiredMarker = 'fired.txt'
# The GRUB environment block the shim payload's grub.cfg writes into. Keep in
# sync with handoff-payload\grub.cfg. GRUB's save_env rewrites this file in
# place, so it must exist (1024 bytes, GRUB's header) before the boot.
$GrubEnvRel   = 'EFI\BOOT\grubenv'
$GrubFiredVar = 'upg_fired'
# Where each payload lives on the (single) stick. Keep in sync with
# make-kit.sh and handoff-payload\README.md.
$PayloadPaths = @{ shim = '\EFI\BOOT\BOOTX64.EFI'; shell = '\EFI\SHELL\SHELLX64.EFI' }
# The one-shot logon task -Auto registers; -Check always removes it.
$ReturnTaskName = 'upgrade_ V0 handoff return check'

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

function Get-PayloadPath {
    # Pure: the EFI path the boot entry points at, for a payload + fail-mode.
    param([string]$PayloadName, [string]$FailMode)
    if ($FailMode -eq 'NoFile') { return '\EFI\BOOT\DOES-NOT-EXIST.EFI' }
    if (-not $PayloadPaths.ContainsKey($PayloadName)) { throw "Unknown payload '$PayloadName'." }
    $PayloadPaths[$PayloadName]
}

function Find-StickRoot {
    # Pure: given volume objects (DriveLetter, UniqueId) and the id recorded
    # at arm time, the root the stick has NOW - a USB stick can come back
    # under a different letter after a reboot, and the return check must not
    # depend on the letter it had. $null if the stick is not present.
    param($Volumes, [string]$UniqueId)
    foreach ($v in @($Volumes)) {
        if ($v.UniqueId -eq $UniqueId -and $v.DriveLetter) { return "$($v.DriveLetter):\" }
    }
    $null
}

function Get-VolumeUniqueId {
    param([string]$Root)
    try { (Get-Volume -DriveLetter $Root.Substring(0, 1) -ErrorAction Stop).UniqueId } catch { $null }
}

function Show-Popup {
    # A message box with a timeout (WScript.Shell.Popup): returns 6 = Yes,
    # 7 = No, 1 = OK, -1 = timed out. Used only in -Auto, where there is no
    # console operator to answer Read-Host.
    param([string]$Text, [string]$Title, [int]$Seconds, [int]$Buttons = 0)
    try { (New-Object -ComObject WScript.Shell).Popup($Text, $Seconds, $Title, $Buttons) } catch { -1 }
}

function Unregister-ReturnTask {
    try {
        if (Get-ScheduledTask -TaskName $ReturnTaskName -ErrorAction SilentlyContinue) {
            Unregister-ScheduledTask -TaskName $ReturnTaskName -Confirm:$false -ErrorAction Stop
            return $true
        }
    } catch { }
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
    $efiPath = Get-PayloadPath -PayloadName $Payload -FailMode $FailMode
    $payloadEfi = Join-Path $root $efiPath.TrimStart('\')
    if ($FailMode -ne 'NoFile' -and -not (Test-Path $payloadEfi)) {
        throw "No payload at $payloadEfi. See handoff-payload\README.md to build the stick."
    }
    $stickId = Get-VolumeUniqueId -Root $root
    if ($Auto -and -not $stickId) { throw "Could not read the stick's volume id for $root; -Auto needs it to find the stick again after the reboot." }

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
    New-Line "  Secure Boot: $sb    BitLocker(C:): $bl (via $($blq.Source))    payload: $Payload at $root$($efiPath.TrimStart('\'))" 'DarkGray'
    if ($Auto) { New-Line '  AUTO: the return check will run itself at the next logon' 'DarkGray' }
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
        Payload        = $Payload
        StickUniqueId  = $stickId
        Auto           = [bool]$Auto
        ResultsCsvArg  = $ResultsCsv
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

    if (-not $Auto) {
        New-Line ''
        New-Line '  ARMED.' 'Green'
        New-Line '  Reboot now, watch what happens, then run:  .\Test-Handoff.ps1 -Check' 'White'
        New-Line ''
        New-Line '  If nothing is watching the screen, that is fine - the payload records' 'DarkGray'
        New-Line '  itself. But note by hand whether any keypress was needed.' 'DarkGray'
        New-Line ''
        return
    }

    # 6. -Auto: the return check runs itself. A copy of this script lives in
    #    the state dir (the stick's letter may change; the state dir will
    #    not), registered as a one-shot logon task for the user who armed,
    #    elevated without a second UAC prompt. If any of this fails, the boot
    #    entry is removed again - never leave an armed machine with no return.
    try {
        Copy-Item $PSCommandPath (Join-Path $state 'Test-Handoff.ps1') -Force
        $user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
        $args = "-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $state 'Test-Handoff.ps1')`" -Check -Auto -StateDir `"$state`""
        if ($ResultsCsv) { $args += " -ResultsCsv `"$ResultsCsv`"" }
        $action    = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $args
        $trigger   = New-ScheduledTaskTrigger -AtLogOn -User $user
        $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
        $settings  = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 1)
        Register-ScheduledTask -TaskName $ReturnTaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
        if (-not (Get-ScheduledTask -TaskName $ReturnTaskName -ErrorAction SilentlyContinue)) { throw 'task not present after registration' }
    } catch {
        & bcdedit /deletevalue '{fwbootmgr}' bootsequence 2>&1 | Out-Null
        & bcdedit /delete $guid 2>&1 | Out-Null
        Remove-Item (Join-Path $state 'handoff-state.json') -Force -ErrorAction SilentlyContinue
        throw "Could not register the return check ($_); the boot entry was removed again. Nothing is armed."
    }

    New-Line ''
    New-Line '  ARMED. Rebooting in 20 seconds.' 'Green'
    New-Line ''
    & shutdown /r /t 20 /c 'upgrade_ V0 handoff test: rebooting to test the boot handoff. Leave the USB stick in.' | Out-Null
    Show-Popup -Title 'upgrade_ V0 handoff test' -Seconds 15 -Text ("Armed. This computer restarts in 20 seconds.`n`n" +
        "Leave the USB stick plugged in. Watch the screen if you can.`n`n" +
        "When Windows comes back, sign in as usual - the result appears by itself.") | Out-Null
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
    $isAuto = ($Auto -or $r.Auto)

    # The stick may have come back under another letter; find it by volume id.
    $stickRoot = $r.PayloadRoot
    if ($r.StickUniqueId) {
        $now = Find-StickRoot -Volumes (Get-Volume -ErrorAction SilentlyContinue) -UniqueId $r.StickUniqueId
        if ($now) { $stickRoot = $now } else { $stickRoot = $null }
    }
    if (-not $stickRoot) {
        New-Line "  ! the stick is not present (armed as $($r.PayloadRoot)); markers unreadable, result will be 'error'" 'Yellow'
        $stickRoot = $r.PayloadRoot
    }

    New-Line ''
    New-Line '  upgrade_  V0 handoff test  -  CHECK' 'Cyan'
    New-Line "  $($r.Vendor) $($r.Model)   firmware $($r.Firmware)" 'DarkGray'
    if ($r.FailMode) { New-Line "  FAIL MODE: $($r.FailMode)" 'Yellow' }
    New-Line ''

    $marker = Join-Path $stickRoot $FiredMarker
    $grubEnv = Join-Path $stickRoot $GrubEnvRel
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
    if (Unregister-ReturnTask) { New-Line '  removed the return-check logon task' 'DarkGray' }
    New-Line '  removing test boot entry...' 'DarkGray'
    & bcdedit /delete $r.Guid 2>&1 | Out-Null
    if (-not $sequenceCleared) { & bcdedit /deletevalue '{fwbootmgr}' bootsequence 2>&1 | Out-Null }
    if ($RestoreBcd -and (Test-Path $r.BcdBackup)) {
        New-Line '  re-importing BCD backup...' 'DarkGray'
        & bcdedit /import $r.BcdBackup 2>&1 | Out-Null
    }
    if (Test-Path $marker) { Remove-Item $marker -Force -ErrorAction SilentlyContinue }
    Reset-GrubEnv -Root $stickRoot | Out-Null

    # Human-supplied fields. In -Auto there is no console operator: 'back in
    # Windows' is true by construction (this code is running there, after the
    # armed reboot), and the one genuinely human fact - whether a key had to
    # be pressed - is asked in a popup that times out to 'unknown'.
    New-Line ''
    if ($isAuto) {
        $winBack = 'y'
        $ans = Show-Popup -Title "upgrade_ V0 handoff test - result: $result" -Seconds 300 -Buttons (4 + 32) -Text (
            "Result: $result`n`n" +
            "During the restart, did this computer come back to Windows WITHOUT anyone pressing a key?`n`n" +
            "Yes = no key was needed.   No = a key or a menu was needed.`n(This closes by itself in 5 minutes and records 'unknown'.)")
        $keypress = switch ($ans) { 6 { 'y' } 7 { 'n' } default { 'unknown' } }
        $notes = ''
    } else {
        $keypress = Read-Host '  Did the machine reach the payload/Windows with NO keypress? (y/n/na)'
        $winBack  = Read-Host '  Are you back in Windows normally right now? (y/n)'
        $notes    = Read-Host '  Notes (recovery prompt? vendor logo hang? blank = none)'
    }

    # Harness-written facts go first in the notes, so the row says what the
    # machine was and how the marker was read, independent of the operator.
    $auto = "[harness: os=$($r.OsCaption) $($r.OsBuild); bitlocker-via=$($r.BitLockerSource); fired-via=$(if ($fired) { $firedVia -join '+' } else { 'none' }); mode=$(if ($isAuto) { 'auto' } else { 'manual' }); payload=$($r.Payload)]"
    $notes = if ([string]::IsNullOrWhiteSpace($notes)) { $auto } else { "$auto $notes" }

    # Append evidence. -Auto without an explicit -ResultsCsv writes to the
    # stick itself (the thing that travels back), falling back to the state
    # dir if the stick is gone.
    if ($r.ResultsCsvArg -and -not $ResultsCsv) { $ResultsCsv = $r.ResultsCsvArg }
    if ($isAuto -and -not $ResultsCsv) {
        $ResultsCsv = if (Test-Path $stickRoot) { Join-Path $stickRoot 'v0-handoff.csv' } else { Join-Path $state 'v0-handoff.csv' }
    }
    $csv = Resolve-ResultsCsv -State $state
    $dir = Split-Path $csv -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    # Write the header as UTF-8 WITHOUT a byte-order mark. PS 5.1's
    # `Out-File -Encoding UTF8` emits a BOM, which lands in front of the
    # first column name of a CSV the harness creates fresh on a stick - and
    # then every naive parser reads that column as "\ufefftimestamp". The
    # data rows never carry one, so a BOM'd header is a transport trap;
    # docs/validation-results/README.md says append data rows only.
    if (-not (Test-Path $csv)) {
        [IO.File]::WriteAllText($csv, $CsvHeader + "`r`n", (New-Object Text.UTF8Encoding($false)))
    }

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
    if ($isAuto) {
        Show-Popup -Title 'upgrade_ V0 handoff test - done' -Seconds 120 -Buttons 64 -Text (
            "Result: $result`n`nThe row was saved to:`n$csv`n`n" +
            "The test boot entry has been removed; this computer is back to normal.`n" +
            "You can unplug the USB stick now and send it back.") | Out-Null
    }
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
        # payload paths and stick relocation (the -Auto return)
        @{ Name = 'payload: shim points at EFI\BOOT\BOOTX64.EFI'
           Run = { Get-PayloadPath -PayloadName 'shim' -FailMode '' }; Expect = '\EFI\BOOT\BOOTX64.EFI' }
        @{ Name = 'payload: shell points at EFI\SHELL\SHELLX64.EFI'
           Run = { Get-PayloadPath -PayloadName 'shell' -FailMode '' }; Expect = '\EFI\SHELL\SHELLX64.EFI' }
        @{ Name = 'payload: NoFile points at a missing file whatever the payload'
           Run = { Get-PayloadPath -PayloadName 'shim' -FailMode 'NoFile' }; Expect = '\EFI\BOOT\DOES-NOT-EXIST.EFI' }
        @{ Name = 'payload: an unknown name is refused'
           Run = { try { Get-PayloadPath -PayloadName 'usb' -FailMode ''; 'accepted' } catch { 'refused' } }; Expect = 'refused' }
        @{ Name = 'stick: found again under a new letter by volume id'
           Run = { Find-StickRoot -UniqueId 'ID-STICK' -Volumes @(
                     [pscustomobject]@{ DriveLetter = 'C'; UniqueId = 'ID-C' },
                     [pscustomobject]@{ DriveLetter = 'F'; UniqueId = 'ID-STICK' }) }; Expect = 'F:\' }
        @{ Name = 'stick: absent stick is null, never a guess'
           Run = { $x = Find-StickRoot -UniqueId 'ID-STICK' -Volumes @([pscustomobject]@{ DriveLetter = 'C'; UniqueId = 'ID-C' }); if ($null -eq $x) { 'null' } else { $x } }; Expect = 'null' }
        @{ Name = 'stick: a matching volume with no letter does not count'
           Run = { $x = Find-StickRoot -UniqueId 'ID-STICK' -Volumes @([pscustomobject]@{ DriveLetter = $null; UniqueId = 'ID-STICK' }); if ($null -eq $x) { 'null' } else { $x } }; Expect = 'null' }
        # drive-letter parsing feeding the bcdedit device line
        # the CSV the harness creates on a stick must not carry a BOM
        @{ Name = 'csv: a freshly created header has no byte-order mark'
           Run = { $f = [IO.Path]::GetTempFileName(); Remove-Item $f -Force
                   [IO.File]::WriteAllText($f, $CsvHeader + "`r`n", (New-Object Text.UTF8Encoding($false)))
                   $b = [IO.File]::ReadAllBytes($f); Remove-Item $f -Force -ErrorAction SilentlyContinue
                   if ($b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF) { 'bom' } else { 'no-bom' } }
           Expect = 'no-bom' }
        @{ Name = 'csv: the created header matches the column list exactly'
           Run = { $f = [IO.Path]::GetTempFileName(); Remove-Item $f -Force
                   [IO.File]::WriteAllText($f, $CsvHeader + "`r`n", (New-Object Text.UTF8Encoding($false)))
                   $h = (Get-Content $f -TotalCount 1); Remove-Item $f -Force -ErrorAction SilentlyContinue
                   if ($h -eq $CsvHeader) { 'ok' } else { "differs: $h" } }
           Expect = 'ok' }
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
