<#
.SYNOPSIS
    upgrade_ - the storage-mode harness: fires the scanner's "Storage
    controller mode" check in BOTH SATA modes of one machine, one click
    (gate V5, risk R1).

.DESCRIPTION
    The scanner's highest-stakes line has three signals (kernel VMD IDs, the
    iaStorVD service, PCI RAID class 0104) and until this harness none had
    fired on real hardware in the positive direction. Proving it takes the
    same machine scanned twice - once with the firmware's SATA mode set to
    Intel RST / RAID / Optane, once set to AHCI - and the only thing no
    software can do for the person is change that setting on the vendor's
    setup screen. Everything else this harness does, from one double-click:

      leg 1  scan + capture in the mode the machine is in now; decide which
             mode to ask for (RAID if it is AHCI/NVMe now, AHCI if RAID)
      arm    a COPY of the Windows boot entry with safeboot=minimal, booted
             exactly once through the boot manager's one-time bootsequence
             (the documented way to change SATA mode without an
             INACCESSIBLE_BOOT_DEVICE stop: Safe Mode loads every installed
             storage-class driver, so the newly re-enumerated controller gets
             its driver bound); a SYSTEM startup task (the walk-away resume
             the prologue uses; RISKS R24); Task Scheduler allowed to start
             in Safe Mode (SafeBoot\Minimal\Schedule) so the Safe Mode boot
             restarts itself with nobody signed in - plus a *-prefixed RunOnce
             that does the same if someone does sign in; then a restart
             STRAIGHT INTO THE FIRMWARE SETUP (shutdown /fw), where the person
             changes SATA Mode and saves
      safe   Windows boots the safe copy once; the resume sees Safe Mode and
             restarts
      leg 2  the resume, as SYSTEM: scan + capture again. Mode unchanged ->
             the setup had no option or it was not saved: clean up, stop.
             Mode changed -> ask for the original mode back, re-arm, restart
             into setup again
      safe   as above
      leg 3  scan + capture; restored or not-restored; clean up everything
             (the copied entry, the bootsequence, the Safe Mode key, the
             RunOnce, the task); the record lands on the stick; a window
             says so at the next sign-in

    Nothing on the disk is written but the boot-configuration copy, all of
    it is undone on every exit path, and the person's own files are never
    touched: switching SATA mode does not change the data on the disk. The
    machine is expected to boot normally in either mode afterwards; if it
    does not (the blue screen), switching the mode back on the setup screen
    is the recovery, and the harness's record says which leg it reached.

    The rows are written by rig/v5-verdict.py from the record and the
    reports this leaves in upgrade_\storage-mode\ on the stick - never by
    hand. The evidence file is docs/validation-results/v5-controller-mode.csv.

.PARAMETER Start
    From RUN-STORAGE-MODE.cmd: leg 1, the arm, the restart into setup.
.PARAMETER StickDrive
    The stick's drive letter (the launcher passes %~d0).
.PARAMETER Bench
    Rig only: allow a machine with no Intel storage controller (a VM) to run
    the flow so the mechanics can be fired where the mode cannot change.
.PARAMETER NoPrompt
    Rig only: do not wait for the OK on the explanation window.
.PARAMETER Resume
    From the SYSTEM startup task after each restart.
.PARAMETER Abort
    Undo everything this harness armed, elevated, by hand.
.PARAMETER Notify
    From RunOnce at sign-in: show what the unattended phase queued.
.PARAMETER SelfTest
    Logic-only cases (no restart, no registry, no BCD).
#>
[CmdletBinding(DefaultParameterSetName = 'Resume')]
param(
    [Parameter(ParameterSetName = 'Start', Mandatory = $true)][switch]$Start,
    [Parameter(ParameterSetName = 'Start', Mandatory = $true)][string]$StickDrive,
    [Parameter(ParameterSetName = 'Start')][switch]$Bench,
    [Parameter(ParameterSetName = 'Start')][switch]$NoPrompt,
    [Parameter(ParameterSetName = 'Resume', Mandatory = $true)][switch]$Resume,
    [Parameter(ParameterSetName = 'Abort', Mandatory = $true)][switch]$Abort,
    [Parameter(ParameterSetName = 'Notify', Mandatory = $true)][switch]$Notify,
    [Parameter(ParameterSetName = 'SelfTest', Mandatory = $true)][switch]$SelfTest,
    [string]$StateDir
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

$HarnessVersion    = '0.1.0'
$TaskName          = 'upgrade_ storage-mode resume'
$SafeRunOnceName   = '*upgrade_storage-mode-safe'      # the * makes RunOnce fire in Safe Mode too
$NoticeRunOnceName = 'upgrade_storage-mode-notice'
$BcdDescription    = 'upgrade_ storage-mode test (Safe Mode, one boot)'
$SafeBootOptionKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\SafeBoot\Option'
$SafeBootMinimalSchedule = 'HKLM:\SYSTEM\CurrentControlSet\Control\SafeBoot\Minimal\Schedule'
$RunOnceKey        = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce'
$StickWaitSeconds  = 120
$StickSubdir       = 'upgrade_\storage-mode'
$script:LogFile = $null; $script:StickLog = $null

# =============================================================================
#  pure logic (self-tested)
# =============================================================================

function Get-SmPciId {
    param([string]$DeviceId)
    if ($DeviceId -match 'VEN_([0-9A-Fa-f]{4})&DEV_([0-9A-Fa-f]{4})') { return ("$($Matches[1]):$($Matches[2])").ToLower() }
    ''
}

function Get-SmClassCodes {
    # Windows publishes the PCI class in CompatibleIDs as PCI\CC_ccss[pp]; the
    # class+subclass (4 hex digits) of every entry, deduplicated.
    param($CompatibleIds)
    $codes = @()
    foreach ($c in @($CompatibleIds)) {
        if ("$c" -match 'CC_([0-9A-Fa-f]{4})') { $x = $Matches[1].ToUpper(); if ($codes -notcontains $x) { $codes += $x } }
    }
    $codes
}

function Get-SmStorageControllers {
    # Intel PCI devices that are mass-storage class (CC_01xx) or carry an
    # Intel RST service - what the scanner's check reads, recorded verbatim.
    param($Pnp)
    $out = @()
    foreach ($d in @($Pnp)) {
        $did = "$(Get-Prop $d 'DeviceID')"
        if (-not $did.StartsWith('PCI\')) { continue }
        $pciId = Get-SmPciId $did
        if (-not $pciId.StartsWith('8086:')) { continue }
        $codes = @(Get-SmClassCodes (Get-Prop $d 'CompatibleID'))
        $svc = "$(Get-Prop $d 'Service')"
        $storage = ($codes | Where-Object { $_.StartsWith('01') }) -or ((Get-Prop $d 'PNPClass') -in @('HDC', 'SCSIAdapter'))
        if ($storage -or $svc -match '^(?i)iastor') {
            $out += [ordered]@{ Name = "$(Get-Prop $d 'Name')"; PciId = $pciId; Classes = @($codes | Where-Object { $_.StartsWith('01') }); Service = $svc
                                CompatibleIds = @(@(Get-Prop $d 'CompatibleID') | Where-Object { $_ } | ForEach-Object { "$_" }) }
        }
    }
    $out
}

function Get-SmModeWord {
    # What the controllers declare: raid (class 0104 or the VMD driver), ahci,
    # nvme (only NVMe controllers), none (no Intel storage controller).
    param($Controllers)
    $c = @($Controllers)
    if ($c.Count -eq 0) { return 'none' }
    foreach ($x in $c) { if (($x.Classes -contains '0104') -or ("$($x.Service)" -match '^(?i)iastorvd')) { return 'raid' } }
    foreach ($x in $c) { if ($x.Classes -contains '0106') { return 'ahci' } }
    foreach ($x in $c) { if ($x.Classes -contains '0108') { return 'nvme' } }
    'other'
}

function Get-SmNextMode {
    # Which mode to ask the person to set: the other one.
    param([string]$Current)
    switch ($Current) {
        'raid' { 'ahci' }
        'ahci' { 'raid' }
        'nvme' { 'raid' }
        default { $null }
    }
}

function Get-SmModeLabel {
    param([string]$Mode)
    switch ($Mode) {
        'raid' { 'Intel RST Premium / Optane / RAID' }
        'ahci' { 'AHCI' }
        default { "$Mode" }
    }
}

function Get-Prop {
    # A property that may be absent (strict mode throws on a missing member).
    param($Object, [string]$Name)
    if ($null -eq $Object) { return $null }
    if ($Object -is [System.Collections.IDictionary]) { if ($Object.Contains($Name)) { return $Object[$Name] }; return $null }
    $p = $Object.PSObject.Properties[$Name]
    if ($p) { return $p.Value }
    $null
}

function Get-SmReportCheck {
    # The scanner report's storage line and verdict.
    param($Report)
    $chk = $null
    foreach ($c in @(Get-Prop $Report 'Checks')) { if ("$(Get-Prop $c 'Title')" -eq 'Storage controller mode') { $chk = $c; break } }
    [ordered]@{
        Status  = $(if ($chk) { "$(Get-Prop $chk 'Status')" } else { '(no check emitted)' })
        Detail  = $(if ($chk) { "$(Get-Prop $chk 'Detail')" } else { '' })
        Verdict = "$(Get-Prop (Get-Prop $Report 'Verdict') 'Level')"
        ScannerVersion = "$(Get-Prop $Report 'ScannerVersion')"
        ScannedUtc = "$(Get-Prop $Report 'ScannedUtc')"
    }
}

function ConvertFrom-SmBcdCopy {
    # bcdedit /copy prints "The entry was successfully copied to {guid}." - the
    # GUID is what we need and the only part that survives localisation.
    param([string]$Text)
    if ("$Text" -match '(\{[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\})') { return $Matches[1] }
    $null
}

function Test-SmSafeMode {
    # HKLM\SYSTEM\CurrentControlSet\Control\SafeBoot\Option exists only during
    # a Safe Mode boot; OptionValue 1 = minimal, 2 = with networking.
    param($OptionValue)
    $null -ne $OptionValue
}

function Get-SmLegOutcome {
    # Leg 2 against leg 1: did the setting take? Leg 3 against leg 1: is it back?
    param([int]$Leg, [string]$FirstMode, [string]$ThisMode)
    if ($Leg -eq 2) { if ($ThisMode -eq $FirstMode) { return 'mode-unchanged' } else { return 'changed' } }
    if ($Leg -eq 3) { if ($ThisMode -eq $FirstMode) { return 'restored' } else { return 'not-restored' } }
    'unexpected-leg'
}

function New-SmSafeRunOnceCommand {
    # Fires at sign-in as the person (RunOnce; the * makes it run in Safe
    # Mode). It restarts ONLY inside Safe Mode - in a normal session it does
    # nothing, so a leftover entry can never restart someone's normal boot.
    "powershell.exe -NoProfile -WindowStyle Hidden -Command `"if (Test-Path '$SafeBootOptionKey') { shutdown /r /t 5 /c 'upgrade_ storage-mode test: Safe Mode boot done, restarting' }`""
}

function Get-SmFlowResult {
    # The flow's own verdict from its record.
    param([string]$Stage, [int]$Legs, [string]$LastOutcome)
    if ($Stage -like 'stopped:*') { return $Stage.Substring(8) }
    if ($Legs -ge 3) { return $LastOutcome }
    if ($Legs -eq 2 -and $LastOutcome -eq 'mode-unchanged') { return 'mode-unchanged' }
    'in-progress'
}

function ConvertTo-SmCsvLine {
    param([object[]]$Fields)
    (@($Fields | ForEach-Object { '"' + (("$_" -replace '[\r\n]+', ' ') -replace '"', '""') + '"' }) -join ',')
}

# =============================================================================
#  live helpers
# =============================================================================

function Test-Elevated {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-DriveRoot { param([string]$Letter) $l = $Letter.TrimEnd(':', '\').ToUpper(); "${l}:\" }

function Resolve-StateDir { if ($StateDir) { return $StateDir }; Join-Path $env:ProgramData 'upgrade_\storage-mode' }

function Write-Log {
    param([string]$Line, [string]$Color = 'Gray')
    Write-Host $Line -ForegroundColor $Color
    $stamp = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    foreach ($p in @($script:LogFile, $script:StickLog)) { if ($p) { try { Add-Content -Path $p -Value "$stamp $Line" -Encoding UTF8 } catch { } } }
}

function ConvertTo-SmHashtable {
    param($o)
    if ($null -eq $o) { return $null }
    if ($o -is [System.Collections.IDictionary]) { $h = [ordered]@{}; foreach ($k in $o.Keys) { $h[$k] = ConvertTo-SmHashtable $o[$k] }; return $h }
    if ($o -is [Array]) { return , @($o | ForEach-Object { ConvertTo-SmHashtable $_ }) }
    if ($o -is [PSCustomObject]) { $h = [ordered]@{}; foreach ($p in $o.PSObject.Properties) { $h[$p.Name] = ConvertTo-SmHashtable $p.Value }; return $h }
    $o
}

function Save-Json { param($Obj, [string]$Path) [IO.File]::WriteAllText($Path, ($Obj | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false))) }
function Save-State { param($S, [string]$State) $S.UpdatedUtc = (Get-Date).ToUniversalTime().ToString('o'); Save-Json $S (Join-Path $State 'state.json') }
function Read-State { param([string]$State) $p = Join-Path $State 'state.json'; if (-not (Test-Path $p)) { return $null }; ConvertTo-SmHashtable (Get-Content $p -Raw | ConvertFrom-Json) }

function Get-LiveContext {
    $explorer = [bool](Get-Process -Name explorer -ErrorAction SilentlyContinue)
    $ui = [Environment]::UserInteractive; $sid = [Diagnostics.Process]::GetCurrentProcess().SessionId
    [ordered]@{ Utc = (Get-Date).ToUniversalTime().ToString('o'); RunAs = [Security.Principal.WindowsIdentity]::GetCurrent().Name; Interactive = $ui; SessionId = $sid
                ExplorerRunning = $explorer; Unattended = (-not $ui -or $sid -eq 0); UptimeSeconds = [int]([Environment]::TickCount / 1000) }
}

function Get-LiveSafeMode {
    $v = Get-ItemProperty -Path $SafeBootOptionKey -Name OptionValue -ErrorAction SilentlyContinue
    if ($v) { return [int]$v.OptionValue }
    $null
}

function Find-StickRoot {
    param($S)
    foreach ($v in @(Get-Volume -ErrorAction SilentlyContinue)) { if ("$($v.UniqueId)" -eq "$($S.StickVolumeId)" -and $v.DriveLetter) { return "$($v.DriveLetter):\" } }
    $null
}

function Wait-Stick {
    param($S)
    $t0 = Get-Date
    while ($true) {
        $r = Find-StickRoot $S
        if ($r) { return $r }
        if (((Get-Date) - $t0).TotalSeconds -ge $StickWaitSeconds) { return $null }
        Start-Sleep -Seconds 5
    }
}

function Get-Facts {
    param([string]$Root)
    $cs = Get-CimInstance Win32_ComputerSystem; $os = Get-CimInstance Win32_OperatingSystem; $bios = Get-CimInstance Win32_BIOS
    $f = [ordered]@{ Vendor = "$($cs.Manufacturer)"; Model = "$($cs.Model)"; BiosVersion = "$($bios.SMBIOSBIOSVersion)"; OsCaption = "$($os.Caption)"; OsBuild = [int]$os.BuildNumber
                     SecureBoot = $(try { if (Confirm-SecureBootUEFI) { 'on' } else { 'off' } } catch { 'unknown' }); BitLocker = 'unknown'; StickVolumeId = $null; StickBus = $null }
    try { $bl = Get-BitLockerVolume -MountPoint 'C:' -ErrorAction Stop; $f.BitLocker = "$($bl.ProtectionStatus)".ToLower() } catch { }
    $l = $Root.Substring(0, 1)
    $sv = Get-Volume -DriveLetter $l -ErrorAction Stop; $f.StickVolumeId = "$($sv.UniqueId)"
    try { $sp = Get-Partition -DriveLetter $l -ErrorAction Stop; $f.StickBus = "$((Get-Disk -Number $sp.DiskNumber).BusType)" } catch { }
    $f
}

function Show-Popup { param([string]$Text, [string]$Title, [int]$Seconds, [int]$Buttons = 0) try { (New-Object -ComObject WScript.Shell).Popup($Text, $Seconds, $Title, $Buttons) } catch { -1 } }

function Set-Notice {
    param([string]$State, [string]$Title, [string]$Text, [int]$Buttons = 64)
    try {
        Save-Json ([ordered]@{ title = $Title; text = $Text; buttons = $Buttons; queued_utc = (Get-Date).ToUniversalTime().ToString('o') }) (Join-Path $State 'notice.json')
        Set-ItemProperty -Path $RunOnceKey -Name $NoticeRunOnceName -Value "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$(Join-Path $State 'Test-StorageMode.ps1')`" -Notify -StateDir `"$State`""
        'queued'
    } catch { Write-Log "  ! could not queue the sign-in notice: $($_.Exception.Message)" 'Yellow'; 'not-queued' }
}

function Show-Or-Queue {
    param([string]$State, [string]$Title, [string]$Text, [int]$Buttons = 64)
    $ctx = Get-LiveContext
    if ($ctx.Unattended) { return (Set-Notice -State $State -Title $Title -Text $Text -Buttons $Buttons) }
    Show-Popup -Title $Title -Seconds 600 -Buttons $Buttons -Text $Text | Out-Null; 'shown'
}

function Protect-StateDir {
    param([string]$State)
    $acl = Get-Acl -LiteralPath $State
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($r in @($acl.Access)) { [void]$acl.RemoveAccessRule($r) }
    $inh = [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
    foreach ($who in @('NT AUTHORITY\SYSTEM', 'BUILTIN\Administrators')) { $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($who, 'FullControl', $inh, 'None', 'Allow'))) }
    $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule('BUILTIN\Users', 'ReadAndExecute', $inh, 'None', 'Allow')))
    Set-Acl -LiteralPath $State -AclObject $acl
    $bad = (Get-Acl -LiteralPath $State).Access | Where-Object { $_.AccessControlType -eq 'Allow' -and "$($_.IdentityReference)" -match '(?i)Users|Everyone|Authenticated' -and "$($_.FileSystemRights)" -match '(?i)Write|Modify|FullControl|CreateFiles' }
    if ($bad) { throw 'the state directory still grants write access to non-administrators; refusing to register a SYSTEM task over it' }
}

function Register-ResumeTask {
    param([string]$State)
    $args = "-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $State 'Test-StorageMode.ps1')`" -Resume -StateDir `"$State`""
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $args
    $trigger = New-ScheduledTaskTrigger -AtStartup
    $principal = New-ScheduledTaskPrincipal -UserId 'NT AUTHORITY\SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Hours 1)
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    $t = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if (-not $t) { throw 'the resume task is not present after registration' }
    if ("$($t.Principal.UserId)" -notmatch '(?i)SYSTEM') { Unregister-ResumeTask | Out-Null; throw "the resume task registered as '$($t.Principal.UserId)', not SYSTEM; removed again" }
}

function Unregister-ResumeTask {
    try { if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) { Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop; return $true } } catch { }
    $false
}

function Invoke-Bcd { param([string[]]$Arguments) $out = & bcdedit @Arguments 2>&1; [ordered]@{ Exit = $LASTEXITCODE; Text = (($out | ForEach-Object { "$_" }) -join ' ').Trim() } }

function Set-SafeBootOnce {
    # The boot manager boots $Guid exactly once (bootsequence is consumed on
    # use); the entry itself stays until cleanup.
    param([string]$Guid)
    $r = Invoke-Bcd @('/bootsequence', $Guid)
    if ($r.Exit -ne 0) { throw "bcdedit /bootsequence failed: $($r.Text)" }
    $chk = Invoke-Bcd @('/enum', '{bootmgr}')
    if ($chk.Text -notmatch [regex]::Escape($Guid)) { throw "bootsequence not visible on {bootmgr} after setting it: $($chk.Text)" }
}

function Arm-SafeEntry {
    # Copy the running Windows entry, make the copy Safe Mode, boot it once.
    $r = Invoke-Bcd @('/copy', '{current}', '/d', $BcdDescription)
    if ($r.Exit -ne 0) { throw "bcdedit /copy failed: $($r.Text)" }
    $guid = ConvertFrom-SmBcdCopy $r.Text
    if (-not $guid) { throw "bcdedit /copy gave no entry id: $($r.Text)" }
    $s = Invoke-Bcd @('/set', $guid, 'safeboot', 'minimal')
    if ($s.Exit -ne 0) { Invoke-Bcd @('/delete', $guid) | Out-Null; throw "bcdedit /set safeboot failed: $($s.Text)" }
    $e = Invoke-Bcd @('/enum', $guid)
    if ($e.Text -notmatch '(?i)safeboot\s+Minimal') { Invoke-Bcd @('/delete', $guid) | Out-Null; throw "the copied entry does not show safeboot Minimal: $($e.Text)" }
    Set-SafeBootOnce $guid
    [ordered]@{ Guid = $guid; Enum = $e.Text }
}

function Enable-ScheduleInSafeMode {
    # Task Scheduler is not on Safe Mode's service list; adding it lets the
    # SYSTEM startup task fire in Safe Mode and restart without a sign-in.
    $existed = Test-Path $SafeBootMinimalSchedule
    if (-not $existed) { New-Item -Path $SafeBootMinimalSchedule -Force | Out-Null; Set-ItemProperty -Path $SafeBootMinimalSchedule -Name '(default)' -Value 'Service' }
    $existed
}

function Set-SafeRunOnce { Set-ItemProperty -Path $RunOnceKey -Name $SafeRunOnceName -Value (New-SmSafeRunOnceCommand) }

function Invoke-Cleanup {
    # Every exit path: undo the copy, the one-time sequence, the Safe Mode
    # key (only if we created it), the RunOnce, the task. Records what it did.
    param($S)
    $c = [ordered]@{}
    if ($S.Contains('SafeEntry') -and $S.SafeEntry -and $S.SafeEntry.Guid) {
        $bs = Invoke-Bcd @('/enum', '{bootmgr}')
        if ($bs.Text -match [regex]::Escape("$($S.SafeEntry.Guid)")) { $c.BootSequenceRemoved = ((Invoke-Bcd @('/deletevalue', '{bootmgr}', 'bootsequence')).Exit -eq 0) } else { $c.BootSequenceRemoved = 'not-set' }
        $c.SafeEntryDeleted = ((Invoke-Bcd @('/delete', "$($S.SafeEntry.Guid)")).Exit -eq 0)
    }
    if ($S.Contains('ScheduleKeyCreated') -and $S.ScheduleKeyCreated) { Remove-Item -Path $SafeBootMinimalSchedule -Force -ErrorAction SilentlyContinue; $c.ScheduleKeyRemoved = -not (Test-Path $SafeBootMinimalSchedule) } else { $c.ScheduleKeyRemoved = 'not-ours' }
    Remove-ItemProperty -Path $RunOnceKey -Name $SafeRunOnceName -ErrorAction SilentlyContinue; $c.SafeRunOnceRemoved = -not [bool](Get-ItemProperty -Path $RunOnceKey -Name $SafeRunOnceName -ErrorAction SilentlyContinue)
    $c.TaskRemoved = Unregister-ResumeTask
    $c
}

function Restart-IntoFirmware {
    # shutdown /fw boots straight into the firmware setup (UEFI OsIndications);
    # firmware without it makes shutdown fail, so fall back to a plain restart
    # and the person presses the setup key. Both recorded.
    param([string]$Why)
    $out = & shutdown /r /fw /t 20 /c "upgrade_ storage-mode test: $Why" 2>&1
    if ($LASTEXITCODE -eq 0) { return 'fw' }
    Write-Log "  shutdown /fw refused (${LASTEXITCODE}: $(($out | ForEach-Object { "$_" }) -join ' ')); plain restart instead - press the setup key at power-on" 'Yellow'
    & shutdown /r /t 20 /c "upgrade_ storage-mode test: $Why. Press the setup key (F2 on Acer) as soon as the screen goes dark." 2>&1 | Out-Null
    'plain'
}

# =============================================================================
#  the legs
# =============================================================================

function Invoke-Leg {
    # Run the scanner and the capture into upgrade_\storage-mode\legN on the
    # stick, then read both back.
    param([int]$N, [string]$Root, [string]$Scanner, [string]$Asked)
    $dir = Join-Path (Join-Path $Root $StickSubdir) "leg$N"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    Get-ChildItem $dir -File -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    Write-Log "  leg ${N}: scanning ($Scanner)..."
    $scanOut = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Scanner -OutDir $dir 2>&1
    [IO.File]::WriteAllLines((Join-Path $dir 'scan.log'), @($scanOut | ForEach-Object { "$_" }))
    $capPath = Join-Path $dir 'machine-capture.json'
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Scanner -DumpMachine $capPath 2>&1 | Out-Null
    $repFile = Get-ChildItem $dir -Filter 'upgrade-report-*.json' | Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $repFile) { throw "leg ${N}: the scanner left no JSON report in $dir (see scan.log)" }
    if (-not (Test-Path $capPath)) { throw "leg ${N}: no machine capture was written" }
    $rep = Get-Content $repFile.FullName -Raw | ConvertFrom-Json
    $cap = Get-Content $capPath -Raw | ConvertFrom-Json
    $ctrls = @(Get-SmStorageControllers -Pnp @($cap.Pnp))
    $mode = Get-SmModeWord $ctrls
    $chk = Get-SmReportCheck $rep
    $ctx = Get-LiveContext
    $leg = [ordered]@{ N = $N; Utc = (Get-Date).ToUniversalTime().ToString('o'); Asked = $Asked; Mode = $mode; Controllers = $ctrls; Check = $chk
                       ReportFile = $repFile.Name; CaptureFile = 'machine-capture.json'; RunAs = $ctx.RunAs; SessionId = $ctx.SessionId; Unattended = $ctx.Unattended }
    Write-Log "  leg ${N}: mode=$mode  controllers=$(@($ctrls | ForEach-Object { "$($_.PciId)[$($_.Classes -join '/')] $($_.Service)" }) -join '; ')  check=$($chk.Status) '$($chk.Detail)'  verdict=$($chk.Verdict)" 'Cyan'
    $leg
}

function Write-Record {
    param($S, [string]$Root)
    $rec = [ordered]@{ schema = 'storage-mode/1'; harness_version = $HarnessVersion; flow_result = (Get-SmFlowResult -Stage "$($S.Stage)" -Legs @($S.Legs).Count -LastOutcome "$($S.LastOutcome)")
                       facts = $S.Facts; legs = $S.Legs; resumes = $S.Resumes; safe_boots = $S.SafeBoots; safe_entry = $S.SafeEntry; fw_reboots = $S.FwReboots
                       schedule_key_created = $S.ScheduleKeyCreated; cleanup = $S.Cleanup; notice = $S.Notice; stage = $S.Stage; started_utc = $S.StartedUtc; updated_utc = $S.UpdatedUtc }
    if ($Root) { Save-Json $rec (Join-Path (Join-Path $Root $StickSubdir) 'storage-mode.json') }
    Save-Json $rec (Join-Path (Resolve-StateDir) 'storage-mode.json')
}

function Finish {
    param($S, [string]$State, [string]$Root, [string]$Result, [string]$Text)
    $S.Cleanup = Invoke-Cleanup $S
    $S.Stage = "done:$Result"
    $S.Notice = Show-Or-Queue -State $State -Title 'upgrade_ - storage-mode test' -Text ("Storage-mode test finished: $Result.`n`n$Text`n`nEverything the test armed has been removed (boot entry copy, one-time boot sequence, Safe Mode key, startup task). The record is on the USB stick in upgrade_\storage-mode\.")
    Save-State $S $State
    Write-Record $S $Root
    Write-Log "  finished: $Result  cleanup=$(($S.Cleanup.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' ')" 'Green'
    Move-Item (Join-Path $State 'state.json') (Join-Path $State 'state-done.json') -Force
}

function Arm-Next {
    # (Re)arm the Safe Mode boot and restart into the firmware setup.
    param($S, [string]$State, [string]$Root, [string]$Ask, [string]$Why)
    if (-not ($S.Contains('SafeEntry') -and $S.SafeEntry -and $S.SafeEntry.Guid)) { $S.SafeEntry = Arm-SafeEntry } else { Set-SafeBootOnce "$($S.SafeEntry.Guid)" }
    Set-SafeRunOnce
    $S.Ask = $Ask
    $S.Stage = "armed-$(@($S.Legs).Count)"
    Save-State $S $State
    Write-Record $S $Root
    $how = Restart-IntoFirmware $Why
    if (-not $S.Contains('FwReboots')) { $S.FwReboots = @() }
    $S.FwReboots = @($S.FwReboots) + , ([ordered]@{ Utc = (Get-Date).ToUniversalTime().ToString('o'); Ask = $Ask; Method = $how })
    Save-State $S $State
    Write-Record $S $Root
    Write-Log "  armed: Safe Mode once via $($S.SafeEntry.Guid); asking for $(Get-SmModeLabel $Ask); restarting ($how) in 20 s" 'Green'
}

# =============================================================================
#  phases
# =============================================================================

function Invoke-StartPhase {
    if (-not (Test-Elevated)) { throw 'run this elevated (the launcher asks for it)' }
    $state = Resolve-StateDir; New-Item -ItemType Directory -Path $state -Force | Out-Null
    if (Test-Path (Join-Path $state 'state.json')) { throw "a storage-mode test is already in progress (state in $state). Restart to let it resume, or run -Abort." }
    $root = Get-DriveRoot $StickDrive
    if (-not (Test-Path $root)) { throw "stick $root not found" }
    $scanner = Join-Path $PSScriptRoot 'upgrade-scan.ps1'
    if (-not (Test-Path $scanner)) { throw "upgrade-scan.ps1 is not next to this harness ($PSScriptRoot) - not a complete kit" }
    $stickDir = Join-Path $root $StickSubdir; New-Item -ItemType Directory -Path $stickDir -Force | Out-Null
    $script:LogFile = Join-Path $state 'storage-mode.log'; Remove-Item $script:LogFile -Force -ErrorAction SilentlyContinue
    $script:StickLog = Join-Path $stickDir 'storage-mode.log'; Remove-Item $script:StickLog -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $stickDir 'storage-mode.json') -Force -ErrorAction SilentlyContinue
    Write-Log ''; Write-Log "  upgrade_  storage-mode harness $HarnessVersion  -  the Storage controller mode check, both SATA modes (V5 / R1)" 'Cyan'
    $F = Get-Facts -Root $root
    Write-Log "  $($F.Vendor) $($F.Model)   BIOS $($F.BiosVersion)   $($F.OsCaption) $($F.OsBuild)   Secure Boot $($F.SecureBoot)   BitLocker $($F.BitLocker)   stick on $($F.StickBus) as $root" 'DarkGray'
    if ($F.BitLocker -eq 'on') { throw 'BitLocker protection is ON for C:. A Safe Mode boot through a copied boot entry can trip BitLocker recovery; suspend BitLocker (manage-bde -protectors -disable C:) or run this on a machine without it. Nothing was changed.' }
    $S = [ordered]@{ HarnessVersion = $HarnessVersion; StartedUtc = (Get-Date).ToUniversalTime().ToString('o'); Stage = 'start'; Facts = $F; StickVolumeId = $F.StickVolumeId; StickRootAtStart = $root
                     Bench = [bool]$Bench; Legs = @(); Resumes = @(); SafeBoots = @(); FwReboots = @(); SafeEntry = $null; ScheduleKeyCreated = $false; Ask = $null; LastOutcome = ''; Cleanup = $null; Notice = $null }
    $leg1 = Invoke-Leg -N 1 -Root $root -Scanner $scanner -Asked 'initial'
    $S.Legs = @($leg1)
    if ($leg1.Mode -eq 'none' -and -not $Bench) {
        $S.Stage = 'stopped:no-intel-controller'; Save-State $S $state; Write-Record $S $root
        Move-Item (Join-Path $state 'state.json') (Join-Path $state 'state-stopped.json') -Force
        Write-Log '  no Intel storage controller on this machine - the SATA-mode test does not apply here (AMD, or a VM). Nothing was changed; the leg-1 scan is on the stick.' 'Yellow'
        if (-not $NoPrompt) { Show-Popup -Title 'upgrade_ - storage-mode test' -Seconds 600 -Buttons 48 -Text 'This computer has no Intel storage controller, so the SATA-mode test does not apply. Nothing was changed. The scan is on the stick.' | Out-Null }
        return
    }
    $ask = Get-SmNextMode $leg1.Mode
    if (-not $ask) { $ask = 'raid' }   # bench: a VM has no mode to flip; exercise the mechanics anyway
    $back = $(if ($leg1.Mode -in @('raid', 'ahci')) { $leg1.Mode } else { 'ahci' })
    $text = "This computer's storage controller is in $(Get-SmModeLabel $leg1.Mode) mode now. The test needs one scan in each mode.`n`n" +
            "When you click OK the computer restarts straight into its setup screen (if it does not, press the setup key - F2 on Acer - the moment the screen goes dark).`n`n" +
            "  1st setup screen:  Main tab -> SATA Mode -> set it to  $(Get-SmModeLabel $ask)  -> F10 -> Yes`n" +
            "      (Acer hides SATA Mode on some models: press Ctrl+S on the Main tab to show it. If it is still not there, press Esc and exit WITHOUT saving - the test then stops by itself.)`n" +
            "  Windows boots once into Safe Mode and restarts on its own. Do not sign in; if you do, it still restarts.`n" +
            "  Windows scans by itself, then restarts into the setup screen again.`n" +
            "  2nd setup screen:  set SATA Mode back to  $(Get-SmModeLabel $back)  -> F10 -> Yes`n" +
            "  Safe Mode once more, then Windows scans a last time and cleans up. Sign in: a window shows the result.`n`n" +
            "Your files are not touched - changing SATA mode changes how the disk is addressed, not what is on it. If Windows ever shows a blue screen after a change, go back into setup and set the mode back; the test records how far it got.`n`nLeave the USB stick in the whole time."
    if (-not $NoPrompt) {
        $r = Show-Popup -Title 'upgrade_ - storage-mode test: what happens next' -Seconds 300 -Buttons (1 + 64) -Text $text
        if ($r -eq 2) { $S.Stage = 'stopped:cancelled'; Save-State $S $state; Write-Record $S $root; Move-Item (Join-Path $state 'state.json') (Join-Path $state 'state-stopped.json') -Force; Write-Log '  cancelled at the explanation - nothing was changed' 'Yellow'; return }
    }
    Copy-Item $PSCommandPath (Join-Path $state 'Test-StorageMode.ps1') -Force
    Copy-Item $scanner (Join-Path $state 'upgrade-scan.ps1') -Force
    Protect-StateDir -State $state
    Register-ResumeTask -State $state
    $S.ScheduleKeyCreated = -not (Enable-ScheduleInSafeMode)
    Save-State $S $state
    Arm-Next -S $S -State $state -Root $root -Ask $ask -Why "set SATA Mode to $(Get-SmModeLabel $ask) on the setup screen, then save"
    Write-Log "  On the setup screen: SATA Mode -> $(Get-SmModeLabel $ask) -> F10 -> Yes. Leave the stick in." 'Yellow'
}

function Invoke-ResumePhase {
    $state = Resolve-StateDir
    $S = Read-State $state
    if (-not $S) { Write-Host "  nothing to resume (no state in $state)"; Unregister-ResumeTask | Out-Null; return }
    $script:LogFile = Join-Path $state 'storage-mode.log'
    $ctx = Get-LiveContext
    $safe = Get-LiveSafeMode
    if (Test-SmSafeMode $safe) {
        # Safe Mode: the driver for the re-enumerated controller is bound by
        # now (Safe Mode loads every storage-class driver). Record, restart.
        if (-not $S.Contains('SafeBoots')) { $S.SafeBoots = @() }
        $S.SafeBoots = @($S.SafeBoots) + , ([ordered]@{ Utc = $ctx.Utc; RunAs = $ctx.RunAs; SessionId = $ctx.SessionId; OptionValue = $safe; UptimeSeconds = $ctx.UptimeSeconds; Stage = "$($S.Stage)" })
        Save-State $S $state
        Write-Log "  Safe Mode boot (option $safe) as $($ctx.RunAs), session $($ctx.SessionId), $($ctx.UptimeSeconds) s after boot - restarting in 10 s" 'Cyan'
        & shutdown /r /t 10 /c 'upgrade_ storage-mode test: Safe Mode boot done, restarting' 2>&1 | Out-Null
        return
    }
    $root = Wait-Stick $S
    $ctx.StickWaitSeconds = [int]((Get-Date) - [DateTime]::Parse($ctx.Utc, $null, [Globalization.DateTimeStyles]::RoundtripKind).ToLocalTime()).TotalSeconds
    $ctx.Stage = "$($S.Stage)"
    if (-not $S.Contains('Resumes')) { $S.Resumes = @() }
    $S.Resumes = @($S.Resumes) + , $ctx
    Save-State $S $state
    if ($root) { $script:StickLog = Join-Path (Join-Path $root $StickSubdir) 'storage-mode.log' }
    Write-Log ''; Write-Log "  upgrade_  storage-mode harness $HarnessVersion  -  RESUME (stage $($S.Stage))" 'Cyan'
    Write-Log "  running as $($ctx.RunAs), session $($ctx.SessionId), uptime $($ctx.UptimeSeconds) s, stick after $($ctx.StickWaitSeconds) s -> $(if ($ctx.Unattended) { 'unattended' } else { 'attended' })" 'DarkGray'
    if (-not $root) {
        Write-Log "  ! the USB stick is not present after $StickWaitSeconds s; leaving the task in place" 'Yellow'
        Show-Or-Queue -State $state -Title 'upgrade_ - storage-mode test' -Buttons 48 -Text 'The USB stick is not plugged in. Plug it in and restart the computer - the test continues by itself.' | Out-Null
        return
    }
    if ("$($S.Stage)" -notlike 'armed-*') { Write-Log "  state is at stage '$($S.Stage)', nothing to continue; cleaning up" 'Yellow'; Finish -S $S -State $state -Root $root -Result 'unexpected-stage' -Text "The test was at stage '$($S.Stage)' when the resume ran."; return }
    $n = @($S.Legs).Count + 1
    $leg = Invoke-Leg -N $n -Root $root -Scanner (Join-Path $state 'upgrade-scan.ps1') -Asked "$($S.Ask)"
    $S.Legs = @($S.Legs) + , $leg
    $first = @($S.Legs)[0]
    $outcome = Get-SmLegOutcome -Leg $n -FirstMode "$($first.Mode)" -ThisMode "$($leg.Mode)"
    $S.LastOutcome = $outcome
    Save-State $S $state; Write-Record $S $root
    Write-Log "  leg ${n}: asked $(Get-SmModeLabel $S.Ask), saw $($leg.Mode) -> $outcome"
    switch ($outcome) {
        'changed' {
            $back = $(if ($first.Mode -in @('raid', 'ahci')) { "$($first.Mode)" } else { 'ahci' })
            Arm-Next -S $S -State $state -Root $root -Ask $back -Why "set SATA Mode back to $(Get-SmModeLabel $back) on the setup screen, then save"
        }
        'mode-unchanged' { Finish -S $S -State $state -Root $root -Result 'mode-unchanged' -Text "The SATA mode did not change between the two scans (both $($leg.Mode)). Either the setup screen has no SATA Mode option on this computer, or it was not saved. The scanner's line was: [$($leg.Check.Status)] $($leg.Check.Detail)" }
        'restored'       { Finish -S $S -State $state -Root $root -Result 'restored' -Text "Both modes were scanned and the original mode ($($first.Mode)) is back.`n  leg 1 ($($first.Mode)): [$($first.Check.Status)] $($first.Check.Detail)`n  leg 2 ($(@($S.Legs)[1].Mode)): [$(@($S.Legs)[1].Check.Status)] $(@($S.Legs)[1].Check.Detail)`n  leg 3 ($($leg.Mode)): [$($leg.Check.Status)] $($leg.Check.Detail)" }
        'not-restored'   { Finish -S $S -State $state -Root $root -Result 'not-restored' -Text "Both modes were scanned, but the controller is in $($leg.Mode) mode now, not the original $($first.Mode). Windows boots in this mode; set it back on the setup screen when convenient.`n  leg 2 ($(@($S.Legs)[1].Mode)): [$(@($S.Legs)[1].Check.Status)] $(@($S.Legs)[1].Check.Detail)" }
        default          { Finish -S $S -State $state -Root $root -Result $outcome -Text 'Unexpected leg.' }
    }
}

function Invoke-NotifyPhase {
    $state = Resolve-StateDir
    $p = Join-Path $state 'notice.json'
    if (-not (Test-Path $p)) { return }
    $n = Get-Content $p -Raw | ConvertFrom-Json
    Show-Popup -Title "$($n.title)" -Seconds 600 -Buttons ([int]$n.buttons) -Text "$($n.text)" | Out-Null
    Remove-Item $p -Force -ErrorAction SilentlyContinue
}

function Invoke-AbortPhase {
    if (-not (Test-Elevated)) { throw 'run this elevated' }
    $state = Resolve-StateDir
    $S = Read-State $state
    if (-not $S) {
        Unregister-ResumeTask | Out-Null
        Remove-ItemProperty -Path $RunOnceKey -Name $SafeRunOnceName -ErrorAction SilentlyContinue
        Write-Host '  nothing in progress; task and RunOnce removed if present'; return
    }
    $S.Cleanup = Invoke-Cleanup $S
    Remove-ItemProperty -Path $RunOnceKey -Name $NoticeRunOnceName -ErrorAction SilentlyContinue
    $S.Stage = "stopped:aborted"
    Save-State $S $state
    Move-Item (Join-Path $state 'state.json') (Join-Path $state 'state-aborted.json') -Force
    Write-Host "  aborted; cleanup: $(($S.Cleanup.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' ')"
}

# =============================================================================
#  self-test (logic only)
# =============================================================================

function Invoke-SelfTest {
    $aspireAhci = [pscustomobject]@{ Name = 'Intel(R) 6th Generation Core Processor Family Platform I/O SATA AHCI Controller'; DeviceID = 'PCI\VEN_8086&DEV_9D03&SUBSYS_11931025&REV_21\3&11583659&0&B8'; PNPClass = 'HDC'; Service = 'iaStorAC'
                                     CompatibleID = @('PCI\VEN_8086&DEV_9D03&REV_21', 'PCI\VEN_8086&DEV_9D03', 'PCI\VEN_8086&CC_010601', 'PCI\VEN_8086&CC_0106', 'PCI\VEN_8086', 'PCI\CC_010601', 'PCI\CC_0106') }
    $rstRaid = [pscustomobject]@{ Name = 'Intel Chipset SATA/PCIe RST Premium Controller'; DeviceID = 'PCI\VEN_8086&DEV_282A&SUBSYS_11931025&REV_21\3&11583659&0&B8'; PNPClass = 'HDC'; Service = 'iaStorAC'
                                  CompatibleID = @('PCI\VEN_8086&DEV_282A&REV_21', 'PCI\VEN_8086&DEV_282A', 'PCI\VEN_8086&CC_010400', 'PCI\VEN_8086&CC_0104', 'PCI\VEN_8086', 'PCI\CC_010400', 'PCI\CC_0104') }
    $vmd = [pscustomobject]@{ Name = 'Intel RST VMD Controller 9A0B'; DeviceID = 'PCI\VEN_8086&DEV_9A0B&SUBSYS_00000000&REV_00\3&0&0A'; PNPClass = 'SCSIAdapter'; Service = 'iaStorVD'; CompatibleID = @('PCI\CC_010400', 'PCI\CC_0104') }
    $nvme = [pscustomobject]@{ Name = 'Standard NVM Express Controller'; DeviceID = 'PCI\VEN_1344&DEV_5413&SUBSYS_21001344&REV_03\4&23CF4B8A&0&0011'; PNPClass = 'SCSIAdapter'; Service = 'stornvme'; CompatibleID = @('PCI\CC_010802', 'PCI\CC_0108') }
    $intelNvme = [pscustomobject]@{ Name = 'Standard NVM Express Controller'; DeviceID = 'PCI\VEN_8086&DEV_F1A8&SUBSYS_390D8086&REV_03\4&1&0&0011'; PNPClass = 'SCSIAdapter'; Service = 'stornvme'; CompatibleID = @('PCI\VEN_8086&CC_010802', 'PCI\CC_010802', 'PCI\CC_0108') }
    $usb = [pscustomobject]@{ Name = 'Intel(R) USB 3.0 eXtensible Host Controller'; DeviceID = 'PCI\VEN_8086&DEV_9D2F&SUBSYS_11931025&REV_21\3&11583659&0&A0'; PNPClass = 'USB'; Service = 'USBXHCI'; CompatibleID = @('PCI\VEN_8086&CC_0C0330', 'PCI\CC_0C0330', 'PCI\CC_0C03') }
    $hv = [pscustomobject]@{ Name = 'Microsoft Hyper-V SCSI Controller'; DeviceID = 'VMBUS\{BA6163D9-04A1-4D29-B605-72E2FFB1DC7F}\{...}'; PNPClass = 'SCSIAdapter'; Service = 'storvsc'; CompatibleID = @($null) }
    $report = [pscustomobject]@{ ScannerVersion = '0.3.0'; ScannedUtc = '2026-09-13T14:08:12Z'; Verdict = [pscustomobject]@{ Level = 'RED' }
                                 Checks = @([pscustomobject]@{ Title = 'Memory'; Status = 'ok'; Detail = '12 GB' }, [pscustomobject]@{ Title = 'Storage controller mode'; Status = 'fail'; Detail = 'Intel RST / VMD active (X)' }) }
    $cases = @(
        @{ Name = 'controllers: the Aspire in AHCI is one Intel HDC with class 0106 and iaStorAC'; Run = { $c = @(Get-SmStorageControllers @($usb, $aspireAhci, $hv)); "$($c.Count) $($c[0].PciId) $($c[0].Classes -join ',') $($c[0].Service)" }; Expect = '1 8086:9d03 0106 iaStorAC' }
        @{ Name = 'controllers: a non-Intel NVMe controller is not counted (the G16)'; Run = { @(Get-SmStorageControllers @($nvme, $usb)).Count }; Expect = 0 }
        @{ Name = 'controllers: an Intel USB controller is not storage'; Run = { @(Get-SmStorageControllers @($usb)).Count }; Expect = 0 }
        @{ Name = 'controllers: a Hyper-V VM has none'; Run = { @(Get-SmStorageControllers @($hv)).Count }; Expect = 0 }
        @{ Name = 'mode: class 0104 is raid (the pre-VMD RST Premium controller)'; Run = { Get-SmModeWord @(Get-SmStorageControllers @($rstRaid)) }; Expect = 'raid' }
        @{ Name = 'mode: the VMD driver is raid even without a class code'; Run = { Get-SmModeWord @(Get-SmStorageControllers @($vmd)) }; Expect = 'raid' }
        @{ Name = 'mode: class 0106 with iaStorAC bound is ahci (the Aspire)'; Run = { Get-SmModeWord @(Get-SmStorageControllers @($aspireAhci)) }; Expect = 'ahci' }
        @{ Name = 'mode: an Intel NVMe controller alone is nvme'; Run = { Get-SmModeWord @(Get-SmStorageControllers @($intelNvme)) }; Expect = 'nvme' }
        @{ Name = 'mode: raid wins when an AHCI controller and a RAID one coexist'; Run = { Get-SmModeWord @(Get-SmStorageControllers @($aspireAhci, $rstRaid)) }; Expect = 'raid' }
        @{ Name = 'mode: no controllers is none'; Run = { Get-SmModeWord @() }; Expect = 'none' }
        @{ Name = 'ask: ahci now asks for raid; raid asks for ahci; nvme asks for raid; none asks nothing'; Run = { "$(Get-SmNextMode 'ahci')/$(Get-SmNextMode 'raid')/$(Get-SmNextMode 'nvme')/$(if ($null -eq (Get-SmNextMode 'none')) { 'null' })" }; Expect = 'raid/ahci/raid/null' }
        @{ Name = 'leg 2 same mode is mode-unchanged; different is changed'; Run = { "$(Get-SmLegOutcome 2 'ahci' 'ahci')/$(Get-SmLegOutcome 2 'ahci' 'raid')" }; Expect = 'mode-unchanged/changed' }
        @{ Name = 'leg 3 back to the first mode is restored; else not-restored'; Run = { "$(Get-SmLegOutcome 3 'ahci' 'ahci')/$(Get-SmLegOutcome 3 'ahci' 'raid')" }; Expect = 'restored/not-restored' }
        @{ Name = 'report: the storage line and verdict are read'; Run = { $c = Get-SmReportCheck $report; "$($c.Status)|$($c.Detail)|$($c.Verdict)|$($c.ScannerVersion)" }; Expect = 'fail|Intel RST / VMD active (X)|RED|0.3.0' }
        @{ Name = 'report: a report without the line says so instead of guessing'; Run = { (Get-SmReportCheck ([pscustomobject]@{ Checks = @(); Verdict = [pscustomobject]@{ Level = 'GREEN' } })).Status }; Expect = '(no check emitted)' }
        @{ Name = 'bcdedit /copy: the GUID is parsed from the English line'; Run = { ConvertFrom-SmBcdCopy 'The entry was successfully copied to {6a3c1f2e-0b4d-4c8a-9e7f-1a2b3c4d5e6f}.' }; Expect = '{6a3c1f2e-0b4d-4c8a-9e7f-1a2b3c4d5e6f}' }
        @{ Name = 'bcdedit /copy: any other wording still yields the GUID; none yields null'; Run = { "$(ConvertFrom-SmBcdCopy 'Der Eintrag wurde erfolgreich in {6A3C1F2E-0B4D-4C8A-9E7F-1A2B3C4D5E6F} kopiert.')/$(if ($null -eq (ConvertFrom-SmBcdCopy 'The boot configuration data store could not be opened.')) { 'null' })" }; Expect = '{6A3C1F2E-0B4D-4C8A-9E7F-1A2B3C4D5E6F}/null' }
        @{ Name = 'safe mode: OptionValue present is Safe Mode, absent is not'; Run = { "$(Test-SmSafeMode 1)/$(Test-SmSafeMode $null)" }; Expect = 'True/False' }
        @{ Name = 'safe RunOnce: restarts only inside Safe Mode (guards on the SafeBoot\Option key)'; Run = { $c = New-SmSafeRunOnceCommand; [bool]($c -match [regex]::Escape($SafeBootOptionKey)) -and [bool]($c -match 'shutdown /r') -and [bool]($c -match '^if|Test-Path') }; Expect = $true }
        @{ Name = 'safe RunOnce: the value name carries the Safe Mode asterisk'; Run = { $SafeRunOnceName.StartsWith('*') }; Expect = $true }
        @{ Name = 'flow: three legs ending restored is restored; two legs unchanged is mode-unchanged; a stop names itself'; Run = { "$(Get-SmFlowResult 'done:restored' 3 'restored')/$(Get-SmFlowResult 'done:mode-unchanged' 2 'mode-unchanged')/$(Get-SmFlowResult 'stopped:no-intel-controller' 1 '')/$(Get-SmFlowResult 'armed-1' 1 '')" }; Expect = 'restored/mode-unchanged/no-intel-controller/in-progress' }
        @{ Name = 'csv: every field quoted, quotes doubled, newlines flattened'; Run = { ConvertTo-SmCsvLine @('a', 'b"c', "d`ne") }; Expect = '"a","b""c","d e"' }
    )
    $failed = 0
    Write-Host ''; Write-Host "  Test-StorageMode $HarnessVersion self-test" -ForegroundColor Cyan
    foreach ($c in $cases) {
        $got = $null; $err = $null
        try { $got = & $c.Run } catch { $err = $_.Exception.Message }
        if ($null -eq $err -and "$got" -eq "$($c.Expect)") { Write-Host ("    PASS  " + $c.Name) -ForegroundColor Green }
        else { $failed++; Write-Host ("    FAIL  " + $c.Name) -ForegroundColor Red; Write-Host ("          got '$got' expected '$($c.Expect)' $err") -ForegroundColor Red }
    }
    Write-Host ''
    if ($failed -eq 0) { Write-Host '  all checks passed' -ForegroundColor Green; Write-Host ''; exit 0 }
    Write-Host "  $failed failed" -ForegroundColor Red; Write-Host ''; exit 1
}

# =============================================================================
#  main
# =============================================================================

if ($SelfTest) { Invoke-SelfTest; return }
try {
    if ($Start)  { Invoke-StartPhase; return }
    if ($Resume) { Invoke-ResumePhase; return }
    if ($Notify) { Invoke-NotifyPhase; return }
    if ($Abort)  { Invoke-AbortPhase; return }
} catch {
    $msg = $_.Exception.Message
    Write-Host ''; Write-Host "  storage-mode harness stopped: $msg" -ForegroundColor Red
    try { Write-Log "  ! stopped: $msg" 'Red' } catch { }
    if ($Start -or $Resume) {
        # a failure must not leave the machine armed: undo whatever was armed, tell, leave
        try {
            $st = Resolve-StateDir; $S = Read-State $st
            if ($S) {
                $S.Cleanup = Invoke-Cleanup $S; $S.Stage = 'stopped:error'; $S.Error = $msg; Save-State $S $st
                $r = Find-StickRoot $S; Write-Record $S $r
                $txt = "The storage-mode test stopped with an error and cleaned up after itself:`n$msg`n`nNothing armed is left behind. The record is in $st" + $(if ($r) { " and on the stick (upgrade_\storage-mode\)." } else { '.' })
                if ($Resume) { Set-Notice -State $st -Title 'upgrade_ - storage-mode test' -Text $txt -Buttons 48 | Out-Null } else { Write-Host "  cleanup: $(($S.Cleanup.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' ')" -ForegroundColor Yellow }
                Move-Item (Join-Path $st 'state.json') (Join-Path $st 'state-stopped.json') -Force
            } else { Unregister-ResumeTask | Out-Null }
        } catch { Write-Host "  cleanup also failed: $($_.Exception.Message) - run -Abort elevated" -ForegroundColor Red }
    }
    exit 1
}
