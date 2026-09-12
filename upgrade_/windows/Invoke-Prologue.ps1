<#
.SYNOPSIS
    upgrade_ - the prologue: the Windows-side, reversible half of the converter.

.DESCRIPTION
    Stage 1 of upgrade_ (docs/architecture.md, "Stage 1 - prologue"), grown
    from the V0 harness's lineage (Test-Handoff.ps1: one reversible change,
    its own restart, clean-up on return). It runs in Windows, elevated, from
    the stick evaluate wrote, and it does these steps in this order, each
    behind a collect/judge seam so the judgment halves are self-tested:

      1.  RE-VALIDATE job.json against the live machine: the identity triple
          (system disk serial / unique id / size), the BIOS serial and system
          UUID, firmware mode and Secure Boot, the stick's identity, the
          BitLocker state, the volume flag, the disk health. Any change
          since evaluate ran stops here.
      1b. THE DISK CHECK (RISKS R18, decided 2026-09-08) - only when C:
          carries NTFS's dirty flag and the person consented at evaluate:
          the read-only online scan (Repair-Volume -Scan) confirms the flag
          first; the physical disk must report Healthy or the prologue
          refuses outright; Windows' spot-fix is scheduled, or the full
          chkdsk /f only when the scan logged real errors; the machine
          restarts; on return the check's real outcome (Wininit event 1001,
          any found.000, the flag) is recorded. Then shrinkable space is
          RE-MEASURED by both read-only paths (Storage API, diskpart) and
          the fork the person pre-chose is taken: keep Windows if it fits,
          else fork.if_cannot_keep (clean slate, or stop).
      2.  KEEP WINDOWS: hibernation off (the kept volume must be mountable
          later), Resize-Partition C: by the planned amount. If the cold
          measurement does not fit, the pagefile is disabled and the machine
          restarts once to re-measure - never more than once.
          CLEAN SLATE: the person's files are staged to the stick with
          per-file checksums at a measured write speed, shown as a computed
          time estimate. (This version then STOPS before arming: the live
          session's two-minute human gate before the wipe is not built, and
          an unattended wipe is not something this code will arm.)
      3.  The typed confirmation word (-ConfirmWord CONVERT) is checked
          before anything at all - RUN-CONVERT.cmd asks for it.
      4.  BitLocker is suspended for one restart (an undetermined state
          refuses), the one-shot firmware boot entry is armed with
          /upgrade_/boot-install on the stick, and the machine restarts
          into the installer. The prologue's record of everything above
          (upgrade_/prologue.json on the stick, the `prologue` block of
          outcome.json) is what the cutover's %post writer carries into
          outcome.json. A stop at any step writes a stopped outcome.json
          itself, undoes what it can (grows C: back, re-enables BitLocker,
          removes the boot entry) and scrubs the stick's credentials.

    Restarts are resumed by a one-shot elevated logon task (-Resume). On the
    return to Windows after the handoff the same task classifies the
    handoff, removes the boot entry and the task, and leaves
    upgrade_/prologue-return.json on the stick.

    Nothing here crosses the commit line. Every refusal happens before the
    step it guards touches anything it cannot undo.

.PARAMETER Start
    Begin a conversion. Needs -StickDrive and -ConfirmWord CONVERT.

.PARAMETER Resume
    Continue after a restart (registered as the logon task; can be run by
    hand). Reads the state the previous phase left.

.PARAMETER Abort
    Stop an in-progress conversion between phases: remove the task and the
    boot entry if armed, keep the state as prologue-aborted.json for reading.

.PARAMETER SelfTest
    Logic tests against fabricated inputs. Touches nothing.
#>
[CmdletBinding(DefaultParameterSetName = 'Resume')]
param(
    [Parameter(ParameterSetName = 'Start', Mandatory = $true)][switch]$Start,
    [Parameter(ParameterSetName = 'Start', Mandatory = $true)][string]$StickDrive,
    [Parameter(ParameterSetName = 'Start')][string]$JobPath,
    [Parameter(ParameterSetName = 'Start')][string]$ConfirmWord,
    [Parameter(ParameterSetName = 'Resume', Mandatory = $true)][switch]$Resume,
    [Parameter(ParameterSetName = 'Abort', Mandatory = $true)][switch]$Abort,
    [Parameter(ParameterSetName = 'SelfTest', Mandatory = $true)][switch]$SelfTest,
    [string]$StateDir
)
$ErrorActionPreference = 'Stop'
$PrologueVersion = '0.1.1'
$TaskName = 'upgrade_ prologue resume'
$ConfirmExpected = 'CONVERT'
$GrubEnvRel = 'EFI\BOOT\grubenv'
$GrubFiredVar = 'upg_fired'
$PayloadEfi = '\EFI\BOOT\BOOTX64.EFI'
$WindowsKeepFreeBytes = 8GB      # what the kept Windows must still have free after the shrink
$FilesMargin = 1.2               # headroom over the harvested bytes Linux must hold until reclaim
$StageProbeBytes = 32MB
$script:LogFile = $null
$script:StickLog = $null

# =============================================================================
#  pure functions (self-tested)
# =============================================================================

function ConvertFrom-PrologueFsutilDirty {
    # `fsutil dirty query C:` -> clean / dirty / unknown (rig lines, 2026-09-08)
    param([string[]]$Lines)
    $t = (@($Lines) -join "`n")
    if ($t -match '(?i)\bis\s+NOT\s+Dirty\b') { return 'clean' }
    if ($t -match '(?i)\bis\s+Dirty\b') { return 'dirty' }
    'unknown'
}

function ConvertFrom-PrologueManageBde {
    param([string[]]$Lines)
    $t = (@($Lines) -join "`n")
    if ($t -match '(?im)^\s*Protection Status:\s*Protection (On|Off)\s*$') { return $matches[1].ToLower() }
    'unknown'
}

function ConvertFrom-PrologueDiskpartQueryMax {
    # `shrink querymax` -> GB or $null (the rig's real line: "... is:   17 GB (17417 MB)")
    param([string[]]$Lines)
    $t = (@($Lines) -join "`n")
    if ($t -match '(?im)reclaimable bytes is:\s*[\d.,]+\s*[KMGT]?B\s*\(\s*([\d,]+)\s*MB\s*\)') { return [math]::Round(([double]($matches[1] -replace ',', '')) / 1024, 1) }
    if ($t -match '(?im)reclaimable bytes is:\s*([\d,]+)\s*MB\b') { return [math]::Round(([double]($matches[1] -replace ',', '')) / 1024, 1) }
    $null
}

function ConvertFrom-PrologueChkntfs {
    # `chkntfs C:` / chkdsk's scheduling answer -> scheduled / dirty / clean / unknown.
    # 'scheduled' and 'dirty' both mean autochk will check the volume at the
    # next restart; 'clean' means nothing will run; anything else is unknown.
    param([string[]]$Lines)
    $t = (@($Lines) -join "`n")
    if ($t -match '(?i)has been scheduled manually to run on next reboot') { return 'scheduled' }
    if ($t -match '(?i)will be checked the next time the system restarts') { return 'scheduled' }
    if ($t -match '(?i)^\s*[A-Z]:\s+is\s+dirty\s*\.?\s*$|(?im)^\s*[A-Z]:\s+is\s+dirty') { return 'dirty' }
    if ($t -match '(?im)^\s*[A-Z]:\s+is\s+not\s+dirty') { return 'clean' }
    'unknown'
}

function Get-PrologueRepairMethod {
    # Guardrail 3 (RISKS R18): the online scan chooses the rung. No errors
    # logged -> Windows' spot-fix (offline for seconds, fixes only what the
    # scan logged; on a stale flag, nothing). Errors logged -> the full
    # check. A scan that answered anything else -> refuse: never repair on
    # a guess about what is wrong.
    param([string]$Scan)
    $s = "$Scan".Trim()
    switch -Regex ($s) {
        '^(NoErrorsFound|ErrorsFixed)$' { return 'spot-fix' }
        '^(ErrorsFound|ErrorsNotFixed)$' { return 'chkdsk-f' }
        default { return 'refuse' }
    }
}

function Test-PrologueDiskHealthGate {
    # Guardrail 2: only a disk that says Healthy gets a repair run on it.
    param([string]$Health)
    ("$Health".Trim() -eq 'Healthy')
}

function Compare-PrologueJob {
    # Step 1: every fact evaluate recorded that the live machine can contradict.
    # Returns the list of mismatches; empty means the job is this machine.
    param($Job, $F)
    $m = New-Object System.Collections.Generic.List[string]
    function cmp([string]$name, $want, $got) { if ("$want" -ne "$got") { $m.Add("$name`: job says '$want', machine says '$got'") } }
    cmp 'system_disk.unique_id' $Job.identity.system_disk.unique_id $F.Disk.UniqueId
    cmp 'system_disk.serial_number' $Job.identity.system_disk.serial_number $F.Disk.Serial
    cmp 'system_disk.size_bytes' $Job.identity.system_disk.size_bytes $F.Disk.Size
    cmp 'bios_serial' $Job.identity.bios_serial $F.BiosSerial
    cmp 'system_uuid' $Job.identity.system_uuid $F.Uuid
    cmp 'firmware_mode' $Job.identity.firmware_mode $F.Firmware
    cmp 'secure_boot' $Job.identity.secure_boot $F.SecureBoot
    cmp 'os_build' $Job.identity.os_build $F.OsBuild
    if ($F.Stick) {
        cmp 'stick.unique_id' $Job.stick.unique_id $F.Stick.UniqueId
        cmp 'stick.size_bytes' $Job.stick.size_bytes $F.Stick.Size
    } else { $m.Add("stick: the stick's disk identity could not be read ($($F.StickError))") }
    cmp 'bitlocker.status' $Job.harvest.bitlocker.status $F.BitLocker
    cmp 'volume_health.dirty' $Job.storage.volume_health.dirty $F.Dirty
    cmp 'physical_disk.health_status' $Job.storage.physical_disk.health_status $F.Health
    $m.ToArray()
}

function Get-PrologueShrinkPlan {
    # Pure arithmetic for the keep-windows shrink. Linux needs linux_min_gb
    # plus the harvested bytes with margin (the files are pulled into Linux
    # before Windows is reclaimed); the kept Windows must keep
    # $WindowsKeepFreeBytes free so it still runs as the rollback; and
    # Resize-Partition cannot go below SizeMin (immovable files). Fits only
    # when all three hold; the request is exactly the target, never more.
    param([long]$PartSize, [long]$SizeMin, [long]$FreeBytes, [double]$LinuxMinGB, [long]$FilesBytes)
    $shrinkable = [long]($PartSize - $SizeMin); if ($shrinkable -lt 0) { $shrinkable = 0 }
    $target = [long]([math]::Ceiling($LinuxMinGB * 1GB + $FilesBytes * $FilesMargin))
    $byFree = [long]($FreeBytes - $WindowsKeepFreeBytes)
    $fits = ($target -le $shrinkable) -and ($target -le $byFree)
    $reason = if ($fits) { 'fits' } elseif ($target -gt $shrinkable) { 'immovable files cap the shrink below what Linux needs' } else { 'Windows would be left with too little free space' }
    @{ ShrinkableBytes = $shrinkable; TargetBytes = $target; MaxByFreeBytes = $byFree; Fits = $fits
       RequestedBytes = $(if ($fits) { $target } else { $null }); Reason = $reason }
}

function Get-PrologueFork {
    # The fork, exactly as job.json pre-chose it (RISKS R18): never asked here.
    param([string]$JobPath, [bool]$Fits, [string]$IfCannotKeep)
    if ($JobPath -eq 'clean-slate') { return 'clean-slate' }
    if ($Fits) { return 'keep-windows' }
    if ($IfCannotKeep -in @('clean-slate', 'stop')) { return $IfCannotKeep }
    'stop'
}

function Get-PrologueTimeEstimate {
    # Seconds to write $Bytes at a MEASURED $Mbps (MB/s, 1e6); null without a measurement.
    param([long]$Bytes, $Mbps)
    if ($null -eq $Mbps -or [double]$Mbps -le 0) { return $null }
    [int][math]::Ceiling($Bytes / 1e6 / [double]$Mbps)
}

function Format-PrologueDuration {
    param($Seconds)
    if ($null -eq $Seconds) { return 'unknown (no write speed measured)' }
    $s = [int]$Seconds
    if ($s -lt 90) { return "about $s seconds" }
    if ($s -lt 5400) { return "about $([math]::Ceiling($s / 60)) minutes" }
    "about $([math]::Round($s / 3600, 1)) hours"
}

function Get-HandoffResult {
    # The harness's classifier, unchanged (docs/validation-results/README.md).
    param([bool]$Fired, [bool]$SequenceCleared, [bool]$OrderUnchanged)
    if ($Fired -and $SequenceCleared -and $OrderUnchanged) { return 'fired-once' }
    if ($Fired -and -not $SequenceCleared) { return 'persisted' }
    if (-not $Fired -and $OrderUnchanged) { return 'ignored' }
    if (-not $OrderUnchanged) { return 'reordered' }
    'error'
}

function New-GrubEnvBlock {
    $header = "# GRUB Environment Block`n"
    $bytes = New-Object byte[] 1024
    $h = [Text.Encoding]::ASCII.GetBytes($header)
    [Array]::Copy($h, $bytes, $h.Length)
    for ($i = $h.Length; $i -lt 1024; $i++) { $bytes[$i] = 0x23 }
    , $bytes
}

function Test-GrubEnvFired {
    param([byte[]]$Bytes)
    if (-not $Bytes -or $Bytes.Length -eq 0) { return $false }
    [bool]([Text.Encoding]::ASCII.GetString($Bytes) -match ('(?m)^' + [regex]::Escape($GrubFiredVar) + '=1\s*$'))
}

function Find-StickRoot {
    param($Volumes, [string]$UniqueId)
    foreach ($v in @($Volumes)) { if ($v.UniqueId -eq $UniqueId -and $v.DriveLetter) { return "$($v.DriveLetter):\" } }
    $null
}

function Get-DriveRoot {
    param([string]$Letter)
    $l = $Letter.TrimEnd(':', '\').ToUpper()
    if ($l.Length -ne 1) { throw "StickDrive must be a single drive letter, got '$Letter'." }
    "${l}:\"
}

function ConvertTo-PrologueHashtable {
    # JSON round-trips come back as PSCustomObject; the state is worked on as ordered hashtables.
    param($o)
    if ($null -eq $o) { return $null }
    if ($o -is [System.Collections.IDictionary]) { $h = [ordered]@{}; foreach ($k in @($o.Keys)) { $h[$k] = ConvertTo-PrologueHashtable $o[$k] }; return $h }
    if ($o -is [System.Management.Automation.PSCustomObject]) { $h = [ordered]@{}; foreach ($p in $o.PSObject.Properties) { $h[$p.Name] = ConvertTo-PrologueHashtable $p.Value }; return $h }
    if ($o -is [array]) { $a = @(); foreach ($x in $o) { $a += , (ConvertTo-PrologueHashtable $x) }; return , $a }
    $o
}

function ConvertTo-PrologueJson {
    param($Obj)
    (($Obj | ConvertTo-Json -Depth 12) -replace "`r`n", "`n")
}

function New-PrologueState {
    param([string]$JobId, [string]$StickId, [string]$Root)
    [ordered]@{
        PrologueVersion = $PrologueVersion; Stage = 'started'; StartedUtc = (Get-Date).ToUniversalTime().ToString('o'); UpdatedUtc = $null
        JobId = $JobId; StickUniqueId = $StickId; StickRootAtStart = $Root; Restarts = 0; Mismatches = @()
        VolumeCheck = [ordered]@{ Needed = $false; Ran = $false; Scan = $null; DiskHealthAtCheck = $null; Method = 'none'; ArmedUtc = $null; ArmText = $null; Chkntfs = $null; Wininit1001 = $null; Found000 = $null; DirtyAfter = 'unknown'; Restarts = 0 }
        Shrink = [ordered]@{ RemeasuredGB = $null; RemeasuredBy = $null; DiskpartGB = $null; ApiError = $null; DiskpartError = $null; PartSize = $null; SizeMin = $null; FreeBytes = $null; Plan = $null; ForkTaken = $null; RequestedBytes = $null; FreedBytes = 0; SizeBefore = $null; PagefileDisabled = $false; HibernationDisabled = $false; Mitigated = $false }
        Staged = $null
        BitLocker = [ordered]@{ StatusBefore = $null; Source = $null; Suspended = $false; RebootCount = $null }
        Handoff = [ordered]@{ Armed = $false; Marker = $null; EntryGuid = $null; ArmedUtc = $null; BcdBackup = $null; Before = $null; GrubEnvReset = $false }
        Return = $null
    }
}

function New-PrologueBlock {
    # The `prologue` block of outcome.json (schemas/outcome.schema.json), from the state.
    param($S)
    $vc = $S.VolumeCheck; $sh = $S.Shrink; $bl = $S.BitLocker; $ho = $S.Handoff
    $health = if ("$($vc.DiskHealthAtCheck)" -in @('Healthy', 'Warning', 'Unhealthy')) { "$($vc.DiskHealthAtCheck)" } elseif ($vc.DiskHealthAtCheck) { 'Unknown' } else { $null }
    $b = [ordered]@{
        revalidated = (@($S.Mismatches).Count -eq 0)
        mismatches = @($S.Mismatches)
        volume_check = [ordered]@{
            needed = [bool]$vc.Needed; ran = [bool]$vc.Ran; disk_health_at_check = $health; method = "$($vc.Method)"
            scan = $vc.Scan; restarts = [int]$vc.Restarts; wininit_1001 = $vc.Wininit1001
            found000_present = $vc.Found000; dirty_after = "$($vc.DirtyAfter)"
        }
        shrink = [ordered]@{
            remeasured_gb = $sh.RemeasuredGB; remeasured_by = $sh.RemeasuredBy; remeasured_diskpart_gb = $sh.DiskpartGB
            fork_taken = $sh.ForkTaken; requested_bytes = $sh.RequestedBytes; freed_bytes = [long]$sh.FreedBytes
            pagefile_disabled = [bool]$sh.PagefileDisabled; hibernation_disabled = [bool]$sh.HibernationDisabled
        }
    }
    if ($S.Staged) {
        $st = $S.Staged
        $b.staged = [ordered]@{ files = [int]$st.Files; bytes = [long]$st.Bytes; failed = [int]$st.Failed; write_mbps = $st.WriteMbps; estimated_seconds = $st.EstimatedSeconds; manifest = "$($st.Manifest)" }
    }
    $b.bitlocker = [ordered]@{ status_before = $(if ($bl.StatusBefore -in @('on', 'off')) { $bl.StatusBefore } else { 'off' }); suspended = [bool]$bl.Suspended; reboot_count = $bl.RebootCount }
    $b.handoff = [ordered]@{ armed = [bool]$ho.Armed; marker = $ho.Marker; entry_guid = $ho.EntryGuid; armed_utc = $ho.ArmedUtc; bcd_backup = $ho.BcdBackup }
    $b
}

function New-PrologueStoppedOutcome {
    # A refusal is an outcome too: settle-in (or a person) can read where and why it stopped.
    param($Job, $S, [string]$StoppedAt, [string]$Reason, $WindowsPartition)
    $path = $S.Shrink.ForkTaken
    if ($path -notin @('keep-windows', 'clean-slate')) { $path = $null }
    [ordered]@{
        schema = 'outcome/1'; job_id = "$($Job.job_id)"
        created_utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        converter_version = "prologue $PrologueVersion"
        status = 'stopped'; stopped_at = $StoppedAt; reason = $Reason; path_taken = $path
        commit_line = [ordered]@{ crossed = $false; crossed_utc = $null; act = $null }
        prologue = (New-PrologueBlock $S)
        windows = [ordered]@{ kept = $true; partition = $WindowsPartition; reachable_via = 'firmware-entry' }
        credentials = [ordered]@{ scrubbed = $true; scrub_after = $(if ($path -eq 'keep-windows') { 'settle-in-pull' } else { 'cutover' }) }
        logs = @('upgrade_/report/prologue.log')
    }
}

# =============================================================================
#  live halves: reads
# =============================================================================

function Test-Elevated {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Test-UefiBoot {
    if ($env:firmware_type -eq 'UEFI') { return $true }
    try { $out = & bcdedit /enum '{fwbootmgr}' 2>&1; return ($LASTEXITCODE -eq 0 -and ($out -join "`n") -match 'bootsequence|displayorder') } catch { return $false }
}

function Get-BitLockerState {
    try {
        $v = Get-BitLockerVolume -MountPoint 'C:' -ErrorAction Stop
        $st = switch ($v.ProtectionStatus) { 'On' { 'on' } 'Off' { 'off' } default { 'unknown' } }
        if ($st -ne 'unknown') { return [pscustomobject]@{ State = $st; Source = 'cmdlet' } }
    } catch { }
    try {
        $out = & manage-bde -status C: 2>&1
        $st = ConvertFrom-PrologueManageBde -Lines @($out | ForEach-Object { "$_" })
        if ($st -ne 'unknown') { return [pscustomobject]@{ State = $st; Source = 'manage-bde' } }
        return [pscustomobject]@{ State = 'unknown'; Source = 'none'; Raw = (@($out) -join "`n") }
    } catch { return [pscustomobject]@{ State = 'unknown'; Source = 'none'; Raw = "$_" } }
}

function Get-PrologueDiskHealth {
    param([int]$DiskNumber, [string]$UniqueId)
    try {
        $pd = Get-PhysicalDisk | Where-Object { "$($_.DeviceId)" -eq "$DiskNumber" } | Select-Object -First 1
        if (-not $pd -and $UniqueId) { $pd = Get-PhysicalDisk | Where-Object { "$($_.UniqueId)" -eq $UniqueId } | Select-Object -First 1 }
        if ($pd) { return "$($pd.HealthStatus)" }
    } catch { }
    'Unknown'
}

function Get-PrologueDirty {
    try { ConvertFrom-PrologueFsutilDirty -Lines @(& fsutil dirty query C: 2>&1 | ForEach-Object { "$_" }) } catch { 'unknown' }
}

function Get-PrologueFacts {
    param([string]$Root)
    $f = [ordered]@{}
    $cs = Get-CimInstance Win32_ComputerSystem; $os = Get-CimInstance Win32_OperatingSystem
    $bios = Get-CimInstance Win32_BIOS; $sys = Get-CimInstance Win32_ComputerSystemProduct
    $f.Vendor = "$($cs.Manufacturer)"; $f.Model = "$($cs.Model)"; $f.Uuid = "$($sys.UUID)"; $f.BiosSerial = "$($bios.SerialNumber)"
    $f.OsCaption = "$($os.Caption)"; $f.OsBuild = [int]$os.BuildNumber
    $f.Firmware = "$env:firmware_type"
    $f.SecureBoot = try { if (Confirm-SecureBootUEFI) { 'on' } else { 'off' } } catch { 'unknown' }
    $part = Get-Partition -DriveLetter C -ErrorAction Stop
    $disk = Get-Disk -Number $part.DiskNumber -ErrorAction Stop
    $f.Disk = [ordered]@{ Number = [int]$disk.Number; Serial = ("$($disk.SerialNumber)" -replace '\s', ''); UniqueId = "$($disk.UniqueId)"; Size = [long]$disk.Size }
    $f.Partition = [ordered]@{ number = [int]$part.PartitionNumber; guid = "$($part.Guid)"; size_bytes = [long]$part.Size }
    $f.Health = Get-PrologueDiskHealth -DiskNumber $disk.Number -UniqueId $disk.UniqueId
    $f.Dirty = Get-PrologueDirty
    $blq = Get-BitLockerState; $f.BitLocker = $blq.State; $f.BitLockerSource = $blq.Source; $f.BitLockerRaw = $blq.Raw
    $f.Stick = $null; $f.StickError = $null
    try {
        $l = $Root.Substring(0, 1)
        $sv = Get-Volume -DriveLetter $l -ErrorAction Stop
        $sp = Get-Partition -DriveLetter $l -ErrorAction Stop
        $sd = Get-Disk -Number $sp.DiskNumber -ErrorAction Stop
        $f.Stick = [ordered]@{ UniqueId = "$($sd.UniqueId)"; Size = [long]$sd.Size; Bus = "$($sd.BusType)"; VolumeId = "$($sv.UniqueId)"; Free = [long]$sv.SizeRemaining }
    } catch { $f.StickError = "$($_.Exception.Message)" }
    $f.Hiberfil = Test-Path 'C:\hiberfil.sys'
    $f.Pagefile = Test-Path 'C:\pagefile.sys'
    $f
}

function Invoke-PrologueScan {
    # Guardrail 1: read-only. Repair-Volume -Scan never repairs.
    try { "$(Repair-Volume -DriveLetter C -Scan -ErrorAction Stop)".Trim() } catch { "scan failed: $($_.Exception.Message)" }
}

function Get-PrologueChkntfs {
    try { ConvertFrom-PrologueChkntfs -Lines @(& chkntfs C: 2>&1 | ForEach-Object { "$_" }) } catch { 'unknown' }
}

function Invoke-PrologueDiskpartQueryMax {
    $script = [IO.Path]::GetTempFileName(); $out = [IO.Path]::GetTempFileName()
    try {
        "select volume C`r`nshrink querymax`r`n" | Set-Content -Path $script -Encoding ASCII
        $p = Start-Process -FilePath 'diskpart.exe' -ArgumentList "/s `"$script`"" -RedirectStandardOutput $out -WindowStyle Hidden -PassThru
        if (-not $p.WaitForExit(60000)) { try { $p.Kill() } catch { }; return @('diskpart: timed out after 60 s') }
        return @(Get-Content -Path $out -ErrorAction SilentlyContinue)
    } catch { return @("diskpart: $($_.Exception.Message)") }
    finally { Remove-Item $script, $out -Force -ErrorAction SilentlyContinue }
}

function Measure-PrologueShrink {
    # Both read-only paths, every time; the errors are kept verbatim.
    $r = [ordered]@{ PartSize = $null; SizeMin = $null; ApiError = $null; DiskpartGB = $null; DiskpartError = $null; FreeBytes = $null }
    $part = Get-Partition -DriveLetter C -ErrorAction Stop; $r.PartSize = [long]$part.Size
    $vol = Get-Volume -DriveLetter C -ErrorAction Stop; $r.FreeBytes = [long]$vol.SizeRemaining
    try { $s = Get-PartitionSupportedSize -DriveLetter C -ErrorAction Stop; $r.SizeMin = [long]$s.SizeMin }
    catch { $r.ApiError = ($_.Exception.Message -replace '\s+', ' ').Trim() }
    $dp = Invoke-PrologueDiskpartQueryMax
    $g = ConvertFrom-PrologueDiskpartQueryMax -Lines $dp
    if ($null -ne $g) { $r.DiskpartGB = $g } else { $r.DiskpartError = (@($dp | Where-Object { $_ -match '\S' } | Select-Object -Last 2) -join ' | ') }
    $r
}

function Get-PrologueCheckOutcome {
    # After the disk-check restart: what actually ran. Wininit logs chkdsk's
    # boot-time output as event 1001 - and it does so AFTER logon (rig run 1,
    # 2026-09-12: the event landed 17 s after the resume had looked for it),
    # so this polls for up to two minutes before saying it is not there.
    # found.000 holds orphaned fragments.
    param([string]$SinceUtc, [int]$WaitSeconds = 120)
    $r = [ordered]@{ Wininit1001 = $null; Found000 = $false; Dirty = 'unknown'; WaitedSeconds = 0 }
    $since = try { ([DateTime]::Parse($SinceUtc, $null, [Globalization.DateTimeStyles]::RoundtripKind)).ToLocalTime().AddMinutes(-2) } catch { (Get-Date).AddHours(-2) }
    $t0 = Get-Date
    while ($true) {
        try {
            # filter by log, id and time only; the provider is matched here - a
            # ProviderName in the hashtable threw "The parameter is incorrect" on the rig
            $ev = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1001; StartTime = $since } -ErrorAction SilentlyContinue |
                  Where-Object { "$($_.ProviderName)" -eq 'Microsoft-Windows-Wininit' } | Sort-Object TimeCreated -Descending | Select-Object -First 1
            if ($ev) { $t = "$($ev.Message)" -replace "`r", ''; if ($t.Length -gt 6000) { $t = $t.Substring(0, 6000) + "`n[truncated]" }; $r.Wininit1001 = $t; break }
        } catch { }
        $r.WaitedSeconds = [int]((Get-Date) - $t0).TotalSeconds
        if ($r.WaitedSeconds -ge $WaitSeconds) { break }
        Start-Sleep -Seconds 10
    }
    try { $r.Found000 = [bool](Get-ChildItem -Path 'C:\' -Force -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^found\.\d{3}$' }) } catch { }
    $r.Dirty = Get-PrologueDirty
    $r
}

function Get-FwbootmgrSnapshot {
    $raw = & bcdedit /enum '{fwbootmgr}' 2>&1
    $text = ($raw -join "`n"); $display = ''; $sequence = ''
    if ($text -match '(?m)^\s*displayorder\s+(.+(?:\r?\n\s{20,}.+)*)') { $display = ($matches[1] -replace '\s+', ' ').Trim() }
    if ($text -match '(?m)^\s*bootsequence\s+(.+(?:\r?\n\s{20,}.+)*)') { $sequence = ($matches[1] -replace '\s+', ' ').Trim() }
    [ordered]@{ DisplayOrder = $display; BootSequence = $sequence }
}

# =============================================================================
#  live halves: writes (each one reversible, each one recorded)
# =============================================================================

function Invoke-PrologueRepairArm {
    # Schedules the chosen rung for the next restart and confirms Windows
    # accepted it (chkntfs). Records every answer verbatim.
    param([string]$Method)
    $texts = @()
    if ($Method -eq 'spot-fix') {
        try { $rv = Repair-Volume -DriveLetter C -SpotFix -ErrorAction Stop; $texts += "Repair-Volume -SpotFix: $rv" }
        catch { $texts += "Repair-Volume -SpotFix failed: $($_.Exception.Message)" }
        $ck = Get-PrologueChkntfs; $texts += "chkntfs after Repair-Volume: $ck"
        if ($ck -notin @('scheduled', 'dirty')) {
            $out = & cmd /c 'echo Y| chkdsk C: /spotfix' 2>&1
            $texts += "chkdsk C: /spotfix: " + ((@($out | ForEach-Object { "$_" }) | Where-Object { $_ -match '\S' }) -join ' | ')
            $ck = ConvertFrom-PrologueChkntfs -Lines @($out | ForEach-Object { "$_" })
            if ($ck -eq 'unknown') { $ck = Get-PrologueChkntfs }
            $texts += "chkntfs after chkdsk: $ck"
        }
    } elseif ($Method -eq 'chkdsk-f') {
        $out = & cmd /c 'echo Y| chkdsk C: /f' 2>&1
        $texts += "chkdsk C: /f: " + ((@($out | ForEach-Object { "$_" }) | Where-Object { $_ -match '\S' }) -join ' | ')
        $ck = ConvertFrom-PrologueChkntfs -Lines @($out | ForEach-Object { "$_" })
        if ($ck -eq 'unknown') { $ck = Get-PrologueChkntfs }
        $texts += "chkntfs after chkdsk: $ck"
    } else { throw "no such repair method '$Method'" }
    [ordered]@{ Text = ($texts -join "`n"); Chkntfs = $ck; Scheduled = ($ck -in @('scheduled', 'dirty')) }
}

function Invoke-PrologueHibernationOff {
    & powercfg /h off 2>&1 | Out-Null
    -not (Test-Path 'C:\hiberfil.sys')
}

function Invoke-ProloguePagefileOff {
    # Takes effect at the next restart; pagefile.sys stays until then.
    try {
        $cs = Get-CimInstance Win32_ComputerSystem
        if ($cs.AutomaticManagedPagefile) { $cs | Set-CimInstance -Property @{ AutomaticManagedPagefile = $false } }
        Get-CimInstance Win32_PageFileSetting -ErrorAction SilentlyContinue | Remove-CimInstance -ErrorAction SilentlyContinue
        $true
    } catch { $false }
}

function Invoke-PrologueShrink {
    param([long]$RequestedBytes)
    $p = Get-Partition -DriveLetter C -ErrorAction Stop
    $target = [long]$p.Size - $RequestedBytes
    Resize-Partition -DriveLetter C -Size $target -ErrorAction Stop
    $p2 = Get-Partition -DriveLetter C -ErrorAction Stop
    [ordered]@{ SizeBefore = [long]$p.Size; SizeAfter = [long]$p2.Size; Freed = [long]($p.Size - $p2.Size) }
}

function Invoke-PrologueGrowBack {
    param([long]$SizeBefore)
    try { Resize-Partition -DriveLetter C -Size $SizeBefore -ErrorAction Stop; return $true } catch { return $false }
}

function Invoke-PrologueStage {
    # clean-slate: every harvested folder to <stick>\upgrade_\staging\, sha256 per file,
    # at a write speed measured on this stick right now.
    param($Job, [string]$Root)
    $dir = Join-Path $Root 'upgrade_\staging'
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $folders = @($Job.harvest.folders | Where-Object { $_.exists -and $_.path })
    $total = 0; foreach ($fo in $folders) { $total += [long]$fo.bytes }
    # measure, then estimate - never the other way round
    $probe = Join-Path $dir '.probe'; $buf = New-Object byte[] $StageProbeBytes; (New-Object Random).NextBytes($buf)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $fs = [IO.File]::Open($probe, 'Create'); $fs.Write($buf, 0, $buf.Length); $fs.Flush($true); $fs.Close(); $sw.Stop()
    Remove-Item $probe -Force -ErrorAction SilentlyContinue
    $mbps = [math]::Round($StageProbeBytes / 1e6 / [math]::Max($sw.Elapsed.TotalSeconds, 0.001), 1)
    $est = Get-PrologueTimeEstimate -Bytes $total -Mbps $mbps
    Write-Log "  staging $($folders.Count) folder(s), $([math]::Round($total/1GB,2)) GB, stick writes at $mbps MB/s: $(Format-PrologueDuration $est)"
    $free = (Get-Volume -DriveLetter $Root.Substring(0, 1)).SizeRemaining
    if ($total * 1.02 + 64MB -gt $free) { return [ordered]@{ Files = 0; Bytes = 0; Failed = 0; WriteMbps = $mbps; EstimatedSeconds = $est; Manifest = 'upgrade_/staging/SHA256SUMS'; Error = "the files ($total bytes) do not fit the stick's free space ($free bytes)" } }
    $lines = New-Object System.Collections.Generic.List[string]; $files = 0; $bytes = 0; $failed = 0
    foreach ($fo in $folders) {
        $src = "$($fo.path)"; $dst = Join-Path $dir "$($fo.name)"
        foreach ($fi in @(Get-ChildItem -LiteralPath $src -File -Recurse -Force -ErrorAction SilentlyContinue)) {
            $rel = $fi.FullName.Substring($src.TrimEnd('\').Length).TrimStart('\')
            $to = Join-Path $dst $rel
            try {
                New-Item -ItemType Directory -Path (Split-Path $to -Parent) -Force | Out-Null
                Copy-Item -LiteralPath $fi.FullName -Destination $to -Force -ErrorAction Stop
                $h = (Get-FileHash -LiteralPath $to -Algorithm SHA256).Hash.ToLower()
                $lines.Add("$h  ./staging/$($fo.name)/$($rel -replace '\\', '/')")
                $files++; $bytes += [long]$fi.Length
            } catch { $failed++; Write-Log "  ! could not stage $($fi.FullName): $($_.Exception.Message)" }
        }
    }
    [IO.File]::WriteAllText((Join-Path $dir 'SHA256SUMS'), (($lines -join "`n") + $(if ($lines.Count) { "`n" } else { '' })), (New-Object Text.UTF8Encoding($false)))
    [ordered]@{ Files = $files; Bytes = [long]$bytes; Failed = $failed; WriteMbps = $mbps; EstimatedSeconds = $est; Manifest = 'upgrade_/staging/SHA256SUMS'; Error = $null }
}

function Reset-GrubEnv {
    param([string]$Root)
    $env = Join-Path $Root $GrubEnvRel
    if ((Test-Path $env) -or (Test-Path (Join-Path $Root 'EFI\BOOT\grubx64.efi'))) { [IO.File]::WriteAllBytes($env, (New-GrubEnvBlock)); return $true }
    $false
}

function Register-ResumeTask {
    param([string]$State)
    # a resumed run IS the state dir's copy already (run 1 on the rig, 2026-09-12:
    # "Cannot overwrite the item ... with itself" stopped the conversion at the arm)
    $copy = Join-Path $State 'Invoke-Prologue.ps1'
    if ([IO.Path]::GetFullPath($PSCommandPath).ToLower() -ne [IO.Path]::GetFullPath($copy).ToLower()) { Copy-Item $PSCommandPath $copy -Force }
    $user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    $args = "-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $State 'Invoke-Prologue.ps1')`" -Resume -StateDir `"$State`""
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $args
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 4)
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    if (-not (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) { throw 'the resume task is not present after registration' }
}

function Unregister-ResumeTask {
    try { if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) { Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop; return $true } } catch { }
    $false
}

function Restart-Machine {
    param([string]$Why)
    Write-Log "  restarting in 15 s: $Why"
    & shutdown /r /t 15 /c "upgrade_: $Why. Leave the USB stick in and do not interrupt." | Out-Null
}

function Show-Popup {
    param([string]$Text, [string]$Title, [int]$Seconds, [int]$Buttons = 0)
    try { (New-Object -ComObject WScript.Shell).Popup($Text, $Seconds, $Title, $Buttons) } catch { -1 }
}

# =============================================================================
#  state, records, log
# =============================================================================

function Resolve-StateDir { if ($StateDir) { return $StateDir }; Join-Path $env:ProgramData 'upgrade_\prologue' }

function Write-Log {
    param([string]$Line, [string]$Color = 'Gray')
    Write-Host $Line -ForegroundColor $Color
    $stamp = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    foreach ($p in @($script:LogFile, $script:StickLog)) { if ($p) { try { Add-Content -Path $p -Value "$stamp $Line" -Encoding UTF8 } catch { } } }
}

function Save-State {
    param($S, [string]$State)
    $S.UpdatedUtc = (Get-Date).ToUniversalTime().ToString('o')
    [IO.File]::WriteAllText((Join-Path $State 'state.json'), (ConvertTo-PrologueJson $S), (New-Object Text.UTF8Encoding($false)))
}

function Read-State {
    param([string]$State)
    $p = Join-Path $State 'state.json'
    if (-not (Test-Path $p)) { return $null }
    ConvertTo-PrologueHashtable (Get-Content $p -Raw | ConvertFrom-Json)
}

function Write-Record {
    # upgrade_/prologue.json on the stick: what the cutover's %post carries into outcome.json,
    # and what a bench reads. Written at every stage transition.
    param($S, [string]$Root)
    if (-not $Root -or -not (Test-Path $Root)) { return }
    $rec = [ordered]@{ schema = 'prologue/1'; prologue_version = $PrologueVersion; job_id = $S.JobId; stage = $S.Stage
                       updated_utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'); prologue = (New-PrologueBlock $S); state = $S }
    New-Item -ItemType Directory -Path (Join-Path $Root 'upgrade_') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $Root 'upgrade_\prologue.json'), (ConvertTo-PrologueJson $rec), (New-Object Text.UTF8Encoding($false)))
}

function Read-Job {
    param([string]$Path)
    if (-not (Test-Path $Path)) { throw "no job at $Path - run the scanner and the job writer first" }
    $j = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    if ($j.schema -ne 'job/1') { throw "job schema '$($j.schema)' is not job/1; refusing" }
    foreach ($k in @('job_id', 'identity', 'intent', 'fork', 'storage', 'harvest', 'stick')) { if (-not $j.PSObject.Properties[$k]) { throw "job.json lacks '$k'; refusing" } }
    $j
}

function Find-Stick {
    param($S)
    if ($S.StickUniqueId) { $r = Find-StickRoot -Volumes (Get-Volume -ErrorAction SilentlyContinue) -UniqueId $S.StickUniqueId; if ($r) { return $r } }
    if ($S.StickRootAtStart -and (Test-Path (Join-Path $S.StickRootAtStart 'upgrade_\job.json'))) { return $S.StickRootAtStart }
    $null
}

function Stop-Prologue {
    # A refusal: undo what this run did, record everything, write the stopped
    # outcome to the stick, scrub the stick's credentials, and exit 2.
    param($S, [string]$State, [string]$Root, $Job, [string]$StoppedAt, [string]$Reason)
    Write-Log ''; Write-Log "  STOPPED at $StoppedAt`: $Reason" 'Red'
    $S.Stage = "stopped:$StoppedAt"
    if ($S.Handoff.Armed -and $S.Handoff.EntryGuid) {
        & bcdedit /deletevalue '{fwbootmgr}' bootsequence 2>&1 | Out-Null; & bcdedit /delete $S.Handoff.EntryGuid 2>&1 | Out-Null
        $S.Handoff.Armed = $false; $S.Handoff.Marker = $null
        Write-Log '  removed the one-shot boot entry'
    }
    if ($S.BitLocker.Suspended) { & manage-bde -protectors -enable C: 2>&1 | Out-Null; Write-Log '  BitLocker protection re-enabled' }
    if ($S.Shrink.FreedBytes -gt 0 -and $S.Shrink.SizeBefore) {
        if (Invoke-PrologueGrowBack -SizeBefore ([long]$S.Shrink.SizeBefore)) { Write-Log "  C: grown back to its original size"; $S.Shrink.FreedBytes = 0 }
        else { Write-Log "  ! could not grow C: back; $($S.Shrink.FreedBytes) bytes remain unallocated (Disk Management can extend C: into them)" 'Yellow' }
    }
    if ($Root) { Remove-Item (Join-Path $Root 'upgrade_\boot-install') -Force -ErrorAction SilentlyContinue }
    Unregister-ResumeTask | Out-Null
    if ($Root -and $Job) {
        $wp = $null
        try { $p = Get-Partition -DriveLetter C -ErrorAction Stop; $wp = [ordered]@{ number = [int]$p.PartitionNumber; guid = "$($p.Guid)"; size_bytes = [long]$p.Size } } catch { }
        $o = New-PrologueStoppedOutcome -Job $Job -S $S -StoppedAt $StoppedAt -Reason $Reason -WindowsPartition $wp
        [IO.File]::WriteAllText((Join-Path $Root 'upgrade_\outcome.json'), (ConvertTo-PrologueJson $o), (New-Object Text.UTF8Encoding($false)))
        $cred = Join-Path $Root 'upgrade_\artifacts\credentials'
        if (Test-Path $cred) { Get-ChildItem $cred -File -Force | ForEach-Object { [IO.File]::WriteAllText($_.FullName, 'SCRUBBED by the prologue at a stop'); Remove-Item $_.FullName -Force } }
        Write-Log "  outcome.json (stopped) written to the stick; credentials scrubbed"
    }
    Write-Record -S $S -Root $Root
    Copy-Item (Join-Path $State 'state.json') (Join-Path $State 'state-stopped.json') -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $State 'state.json') -Force -ErrorAction SilentlyContinue
    if (-not $Start) { Show-Popup -Title 'upgrade_ - stopped' -Seconds 600 -Buttons 48 -Text ("The conversion stopped at: $StoppedAt`n`n$Reason`n`nWindows is as it was. Nothing was installed. The record is on the USB stick (upgrade_\outcome.json).") | Out-Null }
    exit 2
}

# =============================================================================
#  the stages
# =============================================================================

function Invoke-VolumeStage {
    # Step 1b. Returns 'continue' or 'restart'; stops on its own otherwise.
    param($S, [string]$State, [string]$Root, $Job, $F)
    if ($Job.intent.path -ne 'keep-windows') { Write-Log '  1b. disk check: not needed (the job does not keep Windows)'; return 'continue' }
    if ($F.Dirty -eq 'clean') { Write-Log '  1b. disk check: not needed (C: carries no dirty flag)'; return 'continue' }
    if ($F.Dirty -ne 'dirty') { Stop-Prologue $S $State $Root $Job 'volume-check' "the volume flag on C: could not be read (fsutil answered in a form this prologue does not understand)" }
    if (-not $Job.fork.volume_check_consented) { Stop-Prologue $S $State $Root $Job 'volume-check' 'C: is flagged for a disk check and the job carries no consent to run one' }
    Write-Log '  1b. C: carries the dirty flag - running the read-only online scan...'
    $scan = Invoke-PrologueScan; $S.VolumeCheck.Scan = $scan; $S.VolumeCheck.Needed = $true
    Write-Log "      online scan: $scan"
    $health = Get-PrologueDiskHealth -DiskNumber $F.Disk.Number -UniqueId $F.Disk.UniqueId
    $S.VolumeCheck.DiskHealthAtCheck = $health
    Write-Log "      physical disk health: $health"
    if (-not (Test-PrologueDiskHealthGate $health)) { Stop-Prologue $S $State $Root $Job 'volume-check' "C: is flagged for a disk check but the physical disk reports HealthStatus=$health; a repair on a failing drive can finish it off. Copy your files off this computer and replace the drive. Nothing was changed." }
    $method = Get-PrologueRepairMethod $scan
    if ($method -eq 'refuse') { Stop-Prologue $S $State $Root $Job 'volume-check' "the online scan did not give a usable answer ('$scan'); refusing to repair on a guess" }
    Write-Log "      method: $method"
    $arm = Invoke-PrologueRepairArm -Method $method
    $S.VolumeCheck.Method = $method; $S.VolumeCheck.ArmText = $arm.Text; $S.VolumeCheck.Chkntfs = $arm.Chkntfs
    Write-Log ('      ' + ($arm.Text -replace "`n", "`n      "))
    if (-not $arm.Scheduled) { Stop-Prologue $S $State $Root $Job 'volume-check' "Windows did not accept the $method for the next restart (chkntfs says '$($arm.Chkntfs)')" }
    $S.VolumeCheck.ArmedUtc = (Get-Date).ToUniversalTime().ToString('o')
    $S.VolumeCheck.Restarts = [int]$S.VolumeCheck.Restarts + 1; $S.Restarts = [int]$S.Restarts + 1
    $S.Stage = 'check-armed'
    Save-State $S $State; Write-Record $S $Root
    try { Register-ResumeTask -State $State } catch { Stop-Prologue $S $State $Root $Job 'volume-check' "could not register the resume task ($_); the scheduled check will still run at the next restart, but this conversion is not continuing" }
    Write-Log "      the disk check runs at the next restart; it may be slow - DO NOT interrupt it." 'Yellow'
    Restart-Machine 'running the disk check on C:'
    'restart'
}

function Invoke-CheckReturn {
    # Back from the disk-check restart. Returns 'continue' or 'restart'; stops otherwise.
    param($S, [string]$State, [string]$Root, $Job)
    $o = Get-PrologueCheckOutcome -SinceUtc $S.VolumeCheck.ArmedUtc
    $S.VolumeCheck.Wininit1001 = $o.Wininit1001; $S.VolumeCheck.Found000 = [bool]$o.Found000; $S.VolumeCheck.DirtyAfter = $o.Dirty
    $S.VolumeCheck.Ran = [bool]($o.Wininit1001) -or ($o.Dirty -eq 'clean')
    Write-Log "  1b. after the restart: Wininit 1001 $(if ($o.Wininit1001) { 'recorded' } else { 'NOT found' }); found.000 $($o.Found000); C: is now $($o.Dirty)"
    if ($o.Wininit1001) { Write-Log ('      ' + (($o.Wininit1001 -split "`n" | Select-Object -First 12) -join "`n      ")) 'DarkGray' }
    if ($o.Dirty -eq 'clean') { return 'continue' }
    if ($o.Dirty -ne 'dirty') { Stop-Prologue $S $State $Root $Job 'volume-check' 'after the disk check the volume flag could not be read' }
    if ($S.VolumeCheck.Method -eq 'chkdsk-f' -or [int]$S.VolumeCheck.Restarts -ge 2) { Stop-Prologue $S $State $Root $Job 'volume-check' "C: still carries the dirty flag after $($S.VolumeCheck.Method) ($($S.VolumeCheck.Restarts) restart(s)); Windows needs a disk check this prologue will not escalate further" }
    # the spot-fix left the flag: escalate only if a fresh scan now logs errors
    $scan = Invoke-PrologueScan; $S.VolumeCheck.Scan = "$($S.VolumeCheck.Scan); rescan: $scan"
    Write-Log "      flag still set; rescan: $scan"
    if ((Get-PrologueRepairMethod $scan) -ne 'chkdsk-f') { Stop-Prologue $S $State $Root $Job 'volume-check' "C: still carries the dirty flag after the spot-fix and the online scan logs no errors ('$scan'); refusing to run the full check on a guess" }
    $health = Get-PrologueDiskHealth -DiskNumber ([int]$Job.identity.system_disk.number) -UniqueId "$($Job.identity.system_disk.unique_id)"
    $S.VolumeCheck.DiskHealthAtCheck = $health
    if (-not (Test-PrologueDiskHealthGate $health)) { Stop-Prologue $S $State $Root $Job 'volume-check' "the physical disk now reports HealthStatus=$health; refusing the full check" }
    $arm = Invoke-PrologueRepairArm -Method 'chkdsk-f'
    $S.VolumeCheck.Method = 'chkdsk-f'; $S.VolumeCheck.ArmText = "$($S.VolumeCheck.ArmText)`n$($arm.Text)"; $S.VolumeCheck.Chkntfs = $arm.Chkntfs
    if (-not $arm.Scheduled) { Stop-Prologue $S $State $Root $Job 'volume-check' "Windows did not accept chkdsk /f for the next restart (chkntfs says '$($arm.Chkntfs)')" }
    $S.VolumeCheck.ArmedUtc = (Get-Date).ToUniversalTime().ToString('o'); $S.VolumeCheck.Restarts = [int]$S.VolumeCheck.Restarts + 1; $S.Restarts = [int]$S.Restarts + 1
    $S.Stage = 'check-armed'; Save-State $S $State; Write-Record $S $Root
    Write-Log '      the full disk check runs at the next restart; it may take a long time - DO NOT interrupt it.' 'Yellow'
    Restart-Machine 'running the full disk check on C:'
    'restart'
}

function Invoke-Continue {
    # Re-measure, take the fork, shrink or stage, then arm. Stops on its own.
    param($S, [string]$State, [string]$Root, $Job)
    Write-Log '  1b. re-measuring shrinkable space by both read-only paths...'
    $m = Measure-PrologueShrink
    $S.Shrink.PartSize = $m.PartSize; $S.Shrink.SizeMin = $m.SizeMin; $S.Shrink.FreeBytes = $m.FreeBytes
    $S.Shrink.ApiError = $m.ApiError; $S.Shrink.DiskpartGB = $m.DiskpartGB; $S.Shrink.DiskpartError = $m.DiskpartError
    if ($null -ne $m.SizeMin) { $S.Shrink.RemeasuredGB = [math]::Round(($m.PartSize - $m.SizeMin) / 1GB, 1); $S.Shrink.RemeasuredBy = 'storage-api' }
    elseif ($null -ne $m.DiskpartGB) { $S.Shrink.RemeasuredGB = $m.DiskpartGB; $S.Shrink.RemeasuredBy = 'diskpart' }
    else { $S.Shrink.RemeasuredGB = $null; $S.Shrink.RemeasuredBy = $null }
    Write-Log "      Storage API: $(if ($null -ne $m.SizeMin) { "$($S.Shrink.RemeasuredGB) GB shrinkable" } else { "refused - $($m.ApiError)" });  diskpart: $(if ($null -ne $m.DiskpartGB) { "$($m.DiskpartGB) GB" } else { "no figure - $($m.DiskpartError)" });  C: free $([math]::Round($m.FreeBytes/1GB,1)) GB"
    $linuxMin = [double]$Job.storage.linux_min_gb
    $filesBytes = 0; foreach ($fo in @($Job.harvest.folders)) { if ($fo.exists) { $filesBytes += [long]$fo.bytes } }
    $fits = $false; $plan = $null
    if ($Job.intent.path -eq 'keep-windows' -and $null -ne $S.Shrink.RemeasuredGB) {
        $sizeMin = if ($null -ne $m.SizeMin) { [long]$m.SizeMin } else { [long]($m.PartSize - $m.DiskpartGB * 1GB) }
        $plan = Get-PrologueShrinkPlan -PartSize ([long]$m.PartSize) -SizeMin $sizeMin -FreeBytes ([long]$m.FreeBytes) -LinuxMinGB $linuxMin -FilesBytes $filesBytes
        $S.Shrink.Plan = $plan; $fits = [bool]$plan.Fits
        Write-Log "      plan: Linux needs $([math]::Round($plan.TargetBytes/1GB,1)) GB (linux_min $linuxMin GB + files $([math]::Round($filesBytes/1GB,2)) GB x $FilesMargin); shrinkable $([math]::Round($plan.ShrinkableBytes/1GB,1)) GB; Windows keeps $([math]::Round($WindowsKeepFreeBytes/1GB,0)) GB free -> $($plan.Reason)"
        if (-not $fits -and -not $S.Shrink.Mitigated -and $plan.TargetBytes -gt $plan.ShrinkableBytes) {
            # the immovable-file floor: pagefile off (effective after a restart), one restart, one re-measure
            Write-Log '      does not fit cold; disabling the pagefile and restarting once to re-measure'
            $S.Shrink.HibernationDisabled = Invoke-PrologueHibernationOff
            $S.Shrink.PagefileDisabled = Invoke-ProloguePagefileOff
            $S.Shrink.Mitigated = $true; $S.Restarts = [int]$S.Restarts + 1; $S.Stage = 'mitigated'
            Save-State $S $State; Write-Record $S $Root
            try { Register-ResumeTask -State $State } catch { Stop-Prologue $S $State $Root $Job 'shrink' "could not register the resume task ($_)" }
            Restart-Machine 'freeing space on C: for the measurement'
            return
        }
    }
    $fork = Get-PrologueFork -JobPath "$($Job.intent.path)" -Fits $fits -IfCannotKeep "$($Job.fork.if_cannot_keep)"
    $S.Shrink.ForkTaken = $fork
    Write-Log "      fork: $fork (job path $($Job.intent.path), if_cannot_keep $($Job.fork.if_cannot_keep))"
    if ($fork -eq 'stop') { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'shrink' "re-measured $(if ($null -ne $S.Shrink.RemeasuredGB) { "$($S.Shrink.RemeasuredGB) GB" } else { 'no figure' }) shrinkable; Linux needs $([math]::Round(($(if ($plan) { $plan.TargetBytes } else { $linuxMin * 1GB }))/1GB,1)) GB; you chose to stop rather than give up Windows$(if ($plan) { " ($($plan.Reason))" })" }

    if ($fork -eq 'keep-windows') {
        Write-Log '  2.  keep Windows: hibernation off, then the shrink'
        $S.Shrink.HibernationDisabled = Invoke-PrologueHibernationOff
        $S.Shrink.RequestedBytes = [long]$plan.RequestedBytes
        try {
            $r = Invoke-PrologueShrink -RequestedBytes ([long]$plan.RequestedBytes)
            $S.Shrink.SizeBefore = $r.SizeBefore; $S.Shrink.FreedBytes = $r.Freed
            Write-Log "      Resize-Partition: C: $([math]::Round($r.SizeBefore/1GB,1)) GB -> $([math]::Round($r.SizeAfter/1GB,1)) GB, freed $([math]::Round($r.Freed/1GB,1)) GB"
        } catch { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'shrink' "Resize-Partition refused: $($_.Exception.Message)" }
        if ([long]$S.Shrink.FreedBytes -lt [long]$plan.RequestedBytes) { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'shrink' "the shrink freed $($S.Shrink.FreedBytes) bytes, less than the $($plan.RequestedBytes) planned" }
        Save-State $S $State; Write-Record $S $Root
    } else {
        Write-Log '  2.  clean slate: staging your files to the stick'
        $st = Invoke-PrologueStage -Job $Job -Root $Root
        $S.Staged = $st; Save-State $S $State; Write-Record $S $Root
        if ($st.Error) { Stop-Prologue $S $State $Root $Job 'stage-files' $st.Error }
        if ($st.Failed -gt 0) { Stop-Prologue $S $State $Root $Job 'stage-files' "$($st.Failed) file(s) could not be staged to the stick" }
        Write-Log "      staged $($st.Files) files, $([math]::Round($st.Bytes/1GB,2)) GB, checksums in $($st.Manifest)"
        Stop-Prologue $S $State $Root $Job 'confirm' "clean slate needs the live session's two-minute human check before the wipe, and that gate is not built in this version; the prologue will not arm an unattended wipe. Your files are staged on the stick with checksums; Windows is untouched."
    }

    # 4. suspend BitLocker, arm the handoff, restart into the installer
    Write-Log '  4.  arming the one-shot boot handoff'
    $blq = Get-BitLockerState; $S.BitLocker.StatusBefore = $blq.State; $S.BitLocker.Source = $blq.Source
    if ($blq.State -eq 'unknown') { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'arm-handoff' "BitLocker state on C: could not be determined$(if ($blq.Raw) { " (manage-bde said: $($blq.Raw))" }); refusing to arm a boot that might stop at a recovery-key prompt" }
    $report = Join-Path $Root 'upgrade_\report'; New-Item -ItemType Directory -Path $report -Force | Out-Null
    $backup = Join-Path $State 'bcd-backup.bin'
    & bcdedit /export $backup | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $backup)) { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'arm-handoff' 'bcdedit /export failed; refusing to arm without a backup' }
    Copy-Item $backup (Join-Path $report 'bcd-backup.bin') -Force
    $S.Handoff.BcdBackup = 'upgrade_/report/bcd-backup.bin'
    $S.Handoff.Before = Get-FwbootmgrSnapshot
    if ($blq.State -eq 'on') {
        $out = & manage-bde -protectors -disable C: -rebootcount 1 2>&1
        if ($LASTEXITCODE -ne 0) { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'arm-handoff' "manage-bde could not suspend BitLocker: $($out -join ' ')" }
        $S.BitLocker.Suspended = $true; $S.BitLocker.RebootCount = 1
        Write-Log '      BitLocker suspended for one restart'
    }
    Remove-Item (Join-Path $Root 'upgrade_\boot-verify') -Force -ErrorAction SilentlyContinue
    [IO.File]::WriteAllText((Join-Path $Root 'upgrade_\boot-install'), "prologue $PrologueVersion job $($S.JobId)`n", (New-Object Text.UTF8Encoding($false)))
    $S.Handoff.Marker = 'boot-install'
    $S.Handoff.GrubEnvReset = Reset-GrubEnv -Root $Root
    $copyOut = & bcdedit /copy '{bootmgr}' /d 'upgrade_' 2>&1
    if ($LASTEXITCODE -ne 0 -or ($copyOut -join "`n") -notmatch '\{[0-9a-fA-F-]{36}\}') { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'arm-handoff' "bcdedit /copy failed: $copyOut" }
    $guid = $matches[0]
    & bcdedit /set $guid device "partition=$($Root.TrimEnd('\'))" | Out-Null
    & bcdedit /set $guid path $PayloadEfi | Out-Null
    & bcdedit /set '{fwbootmgr}' bootsequence $guid | Out-Null
    if ($LASTEXITCODE -ne 0) { & bcdedit /delete $guid | Out-Null; Save-State $S $State; Stop-Prologue $S $State $Root $Job 'arm-handoff' 'setting bootsequence failed; the entry was removed again' }
    $S.Handoff.Armed = $true; $S.Handoff.EntryGuid = $guid; $S.Handoff.ArmedUtc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    $S.Stage = 'armed'; $S.Restarts = [int]$S.Restarts + 1
    Save-State $S $State; Write-Record $S $Root
    try { Register-ResumeTask -State $State } catch { Stop-Prologue $S $State $Root $Job 'arm-handoff' "could not register the return check ($_); the boot entry was removed again" }
    Write-Log "      armed: entry $guid -> $($Root)$($PayloadEfi.TrimStart('\')), marker boot-install" 'Green'
    Write-Log ''; Write-Log '  This computer restarts into the installer in 15 seconds. Leave the stick in.' 'Green'
    Write-Log '  Windows is still here and still bootable; it stays that way until you reclaim it in Linux.' 'DarkGray'
    Restart-Machine 'starting the installer from the USB stick'
    if (-not $Start) { Show-Popup -Title 'upgrade_' -Seconds 12 -Text "Restarting into the installer in 15 seconds. Leave the USB stick in." | Out-Null }
}

function Invoke-Return {
    # Windows is back after the handoff. Classify, clean up, record, leave.
    param($S, [string]$State, [string]$Root)
    Write-Log '  return: Windows is back after the handoff'
    $fired = $false; $via = @()
    if ($Root) {
        $ge = Join-Path $Root $GrubEnvRel
        if ((Test-Path $ge) -and (Test-GrubEnvFired -Bytes ([IO.File]::ReadAllBytes($ge)))) { $fired = $true; $via += 'grubenv' }
        if (Test-Path (Join-Path $Root 'upgrade_\outcome.json')) { $via += 'outcome.json' }
    }
    $after = Get-FwbootmgrSnapshot
    $cleared = [string]::IsNullOrWhiteSpace($after.BootSequence)
    $guid = "$($S.Handoff.EntryGuid)"
    $afterTokens = @($after.DisplayOrder -split '\s+' | Where-Object { $_ -and $_ -ne $guid })
    $beforeTokens = @("$($S.Handoff.Before.DisplayOrder)" -split '\s+' | Where-Object { $_ })
    $unchanged = (($afterTokens -join ' ') -eq ($beforeTokens -join ' '))
    $result = Get-HandoffResult -Fired $fired -SequenceCleared $cleared -OrderUnchanged $unchanged
    Write-Log "      fired=$fired ($($via -join '+')) sequence_cleared=$cleared order_unchanged=$unchanged -> $result"
    Unregister-ResumeTask | Out-Null
    if ($guid) { & bcdedit /delete $guid 2>&1 | Out-Null }
    if (-not $cleared) { & bcdedit /deletevalue '{fwbootmgr}' bootsequence 2>&1 | Out-Null }
    if ($Root) { Remove-Item (Join-Path $Root 'upgrade_\boot-install') -Force -ErrorAction SilentlyContinue; Reset-GrubEnv -Root $Root | Out-Null }
    $S.Return = [ordered]@{ ReturnedUtc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'); Fired = $fired; FiredVia = ($via -join '+'); SequenceCleared = $cleared; OrderUnchanged = $unchanged
                            Result = $result; DisplayOrderAfter = $after.DisplayOrder; BitLockerNow = (Get-BitLockerState).State }
    $S.Stage = 'returned'
    Save-State $S $State
    if ($Root) {
        $rec = [ordered]@{ schema = 'prologue-return/1'; prologue_version = $PrologueVersion; job_id = $S.JobId; handoff = $S.Return; secure_boot = $(try { if (Confirm-SecureBootUEFI) { 'on' } else { 'off' } } catch { 'unknown' }) }
        [IO.File]::WriteAllText((Join-Path $Root 'upgrade_\prologue-return.json'), (ConvertTo-PrologueJson $rec), (New-Object Text.UTF8Encoding($false)))
        Write-Record $S $Root
    }
    Copy-Item (Join-Path $State 'state.json') (Join-Path $State 'state-returned.json') -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $State 'state.json') -Force -ErrorAction SilentlyContinue
    Write-Log "      boot entry removed, task removed; record on the stick" 'Green'
    Show-Popup -Title 'upgrade_ - back in Windows' -Seconds 120 -Buttons 64 -Text ("The one-time boot entry has been removed (handoff: $result).`n`nIf the conversion completed, Linux is the first boot choice and Windows is in its menu. The record is on the USB stick.") | Out-Null
}

# =============================================================================
#  entry points
# =============================================================================

function Invoke-StartPhase {
    if ($ConfirmWord -cne $ConfirmExpected) { throw "the confirmation word was not typed (expected $ConfirmExpected); nothing was started" }
    $state = Resolve-StateDir; New-Item -ItemType Directory -Path $state -Force | Out-Null
    if (Test-Path (Join-Path $state 'state.json')) { throw "a conversion is already in progress (state in $state). Sign out and in to let it resume, or run -Abort." }
    $root = Get-DriveRoot $StickDrive
    if (-not (Test-Path $root)) { throw "stick $root not found" }
    $jobFile = if ($JobPath) { $JobPath } else { Join-Path $root 'upgrade_\job.json' }
    $job = Read-Job $jobFile
    if (-not (Test-Path (Join-Path $root $PayloadEfi.TrimStart('\')))) { throw "no payload at $root$($PayloadEfi.TrimStart('\')) - this is not the kit stick" }
    if (-not (Test-Path (Join-Path $root 'upgrade_\ks.cfg'))) { throw 'no kickstart on the stick (upgrade_\ks.cfg) - run the generator first' }
    $report = Join-Path $root 'upgrade_\report'
    if (Test-Path $report) { Remove-Item $report -Recurse -Force -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Path $report -Force | Out-Null
    $script:LogFile = Join-Path $state 'prologue.log'; Remove-Item $script:LogFile -Force -ErrorAction SilentlyContinue
    $script:StickLog = Join-Path $report 'prologue.log'
    Write-Log ''; Write-Log "  upgrade_  prologue $PrologueVersion  -  START" 'Cyan'
    Write-Log "  job $($job.job_id)   path $($job.intent.path)   desktop $($job.intent.desktop)   stick $root" 'DarkGray'
    $F = Get-PrologueFacts -Root $root
    $S = New-PrologueState -JobId "$($job.job_id)" -StickId "$($F.Stick.VolumeId)" -Root $root
    $S.Facts = [ordered]@{ vendor = $F.Vendor; model = $F.Model; os = "$($F.OsCaption) $($F.OsBuild)"; secure_boot = $F.SecureBoot; disk = $F.Disk; health = $F.Health; dirty_at_start = $F.Dirty; bitlocker = $F.BitLocker; bitlocker_via = $F.BitLockerSource; hiberfil = $F.Hiberfil; pagefile = $F.Pagefile }
    Write-Log "  $($F.Vendor) $($F.Model)   $($F.OsCaption) $($F.OsBuild)   Secure Boot $($F.SecureBoot)   BitLocker $($F.BitLocker) (via $($F.BitLockerSource))   disk health $($F.Health)   C: $($F.Dirty)" 'DarkGray'
    Save-State $S $state
    Write-Log '  1.  re-validating job.json against this machine...'
    $mm = Compare-PrologueJob -Job $job -F $F
    $S.Mismatches = @($mm)
    if (@($mm).Count -gt 0) { foreach ($x in $mm) { Write-Log "      ! $x" 'Yellow' }; Save-State $S $state; Stop-Prologue $S $state $root $job 'revalidate' ("job.json no longer matches this machine: " + ($mm -join '; ')) }
    Write-Log '      matches: disk identity, firmware, Secure Boot, stick, BitLocker, volume flag, disk health'
    Save-State $S $state; Write-Record $S $root
    if ((Invoke-VolumeStage $S $state $root $job $F) -eq 'restart') { return }
    Invoke-Continue $S $state $root $job
}

function Invoke-ResumePhase {
    $state = Resolve-StateDir
    $S = Read-State $state
    if (-not $S) { Write-Host "  nothing to resume (no state in $state)"; Unregister-ResumeTask | Out-Null; return }
    $script:LogFile = Join-Path $state 'prologue.log'
    $root = Find-Stick $S
    if ($root) { $script:StickLog = Join-Path $root 'upgrade_\report\prologue.log'; New-Item -ItemType Directory -Path (Split-Path $script:StickLog -Parent) -Force | Out-Null }
    Write-Log ''; Write-Log "  upgrade_  prologue $PrologueVersion  -  RESUME (stage $($S.Stage), restart $($S.Restarts))" 'Cyan'
    if (-not $root) {
        Write-Log '  ! the USB stick is not present; plug it in and sign out and in again' 'Yellow'
        Show-Popup -Title 'upgrade_' -Seconds 300 -Buttons 48 -Text "The USB stick is not plugged in. Plug it in, then sign out and back in - the conversion continues by itself." | Out-Null
        return
    }
    if ($S.Stage -eq 'armed') { Invoke-Return $S $state $root; return }
    $job = Read-Job (Join-Path $root 'upgrade_\job.json')
    if ("$($job.job_id)" -ne "$($S.JobId)") { Stop-Prologue $S $state $root $job 'revalidate' "the job on the stick ($($job.job_id)) is not the one this conversion started with ($($S.JobId))" }
    switch ($S.Stage) {
        'check-armed' { if ((Invoke-CheckReturn $S $state $root $job) -eq 'restart') { return } }
        'mitigated' { Write-Log '  back from the pagefile restart' }
        default { throw "state is at stage '$($S.Stage)', which -Resume does not continue from" }
    }
    Invoke-Continue $S $state $root $job
}

function Invoke-AbortPhase {
    $state = Resolve-StateDir
    $S = Read-State $state
    Unregister-ResumeTask | Out-Null
    if (-not $S) { Write-Host '  nothing in progress'; return }
    if ($S.Handoff.Armed -and $S.Handoff.EntryGuid) { & bcdedit /deletevalue '{fwbootmgr}' bootsequence 2>&1 | Out-Null; & bcdedit /delete $S.Handoff.EntryGuid 2>&1 | Out-Null; Write-Host '  removed the one-shot boot entry' }
    if ($S.BitLocker.Suspended) { & manage-bde -protectors -enable C: 2>&1 | Out-Null; Write-Host '  BitLocker protection re-enabled' }
    $root = Find-Stick $S
    if ($root) { Remove-Item (Join-Path $root 'upgrade_\boot-install') -Force -ErrorAction SilentlyContinue }
    Move-Item (Join-Path $state 'state.json') (Join-Path $state 'state-aborted.json') -Force
    Write-Host "  aborted at stage '$($S.Stage)'; state kept as state-aborted.json. A shrink already made is not undone here (Disk Management can extend C:)."
}

# =============================================================================
#  self-test (logic only)
# =============================================================================

function Invoke-SelfTest {
    $job = [pscustomobject]@{
        job_id = '3f2b6a1e-7c4d-4e8f-9a0b-1c2d3e4f5a6b'
        identity = [pscustomobject]@{ bios_serial = 'S1'; system_uuid = 'U1'; firmware_mode = 'UEFI'; secure_boot = 'on'; os_build = 19045
                                      system_disk = [pscustomobject]@{ number = 0; unique_id = 'eui.1'; serial_number = 'SER'; size_bytes = 250059350016 } }
        intent = [pscustomobject]@{ path = 'keep-windows' }
        fork = [pscustomobject]@{ if_cannot_keep = 'stop'; volume_check_consented = $true }
        storage = [pscustomobject]@{ linux_min_gb = 25; volume_health = [pscustomobject]@{ dirty = 'dirty' }; physical_disk = [pscustomobject]@{ health_status = 'Healthy' } }
        harvest = [pscustomobject]@{ bitlocker = [pscustomobject]@{ status = 'on' }; folders = @() }
        stick = [pscustomobject]@{ unique_id = 'USBSTOR\X'; size_bytes = 8053063680 }
    }
    $facts = [ordered]@{ BiosSerial = 'S1'; Uuid = 'U1'; Firmware = 'UEFI'; SecureBoot = 'on'; OsBuild = 19045
                         Disk = [ordered]@{ Number = 0; UniqueId = 'eui.1'; Serial = 'SER'; Size = 250059350016 }
                         Stick = [ordered]@{ UniqueId = 'USBSTOR\X'; Size = 8053063680 }; StickError = $null
                         BitLocker = 'on'; Dirty = 'dirty'; Health = 'Healthy' }
    function With { param($h, [string]$k, $v) $c = [ordered]@{}; foreach ($e in $h.GetEnumerator()) { $c[$e.Key] = $e.Value }; $c[$k] = $v; $c }
    function WithDisk { param($h, [string]$k, $v) $c = With $h 'Disk' (With $h.Disk $k $v); $c }
    $cases = @(
        # step 1: re-validation
        @{ Name = 'revalidate: an identical machine has no mismatches'; Run = { @(Compare-PrologueJob -Job $job -F $facts).Count }; Expect = 0 }
        @{ Name = 'revalidate: a different system disk id is a mismatch'; Run = { @(Compare-PrologueJob -Job $job -F (WithDisk $facts 'UniqueId' 'eui.2')) -join ';' }; Expect = "system_disk.unique_id: job says 'eui.1', machine says 'eui.2'" }
        @{ Name = 'revalidate: a different disk size is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (WithDisk $facts 'Size' 1)) -match 'size_bytes') }; Expect = $true }
        @{ Name = 'revalidate: a different stick is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'Stick' ([ordered]@{ UniqueId = 'OTHER'; Size = 1 }))) -match 'stick') }; Expect = $true }
        @{ Name = 'revalidate: an unreadable stick is a mismatch, not a pass'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With (With $facts 'Stick' $null) 'StickError' 'gone')) -match 'stick') }; Expect = $true }
        @{ Name = 'revalidate: BitLocker turned off since evaluate is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'BitLocker' 'off')) -match 'bitlocker') }; Expect = $true }
        @{ Name = 'revalidate: the flag cleared since evaluate is a change, and a change stops'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'Dirty' 'clean')) -match 'volume_health') }; Expect = $true }
        @{ Name = 'revalidate: a disk health that changed is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'Health' 'Warning')) -match 'health_status') }; Expect = $true }
        @{ Name = 'revalidate: Secure Boot toggled is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'SecureBoot' 'off')) -match 'secure_boot') }; Expect = $true }
        # step 1b: guardrails
        @{ Name = 'scan: NoErrorsFound chooses the spot-fix'; Run = { Get-PrologueRepairMethod 'NoErrorsFound' }; Expect = 'spot-fix' }
        @{ Name = 'scan: ErrorsFound chooses chkdsk /f'; Run = { Get-PrologueRepairMethod 'ErrorsFound' }; Expect = 'chkdsk-f' }
        @{ Name = 'scan: ErrorsNotFixed chooses chkdsk /f'; Run = { Get-PrologueRepairMethod ' ErrorsNotFixed ' }; Expect = 'chkdsk-f' }
        @{ Name = 'scan: a failed scan refuses, never a guess'; Run = { Get-PrologueRepairMethod 'scan failed: Access denied' }; Expect = 'refuse' }
        @{ Name = 'scan: an empty answer refuses'; Run = { Get-PrologueRepairMethod '' }; Expect = 'refuse' }
        @{ Name = 'health gate: Healthy passes'; Run = { Test-PrologueDiskHealthGate 'Healthy' }; Expect = $true }
        @{ Name = 'health gate: Warning refuses'; Run = { Test-PrologueDiskHealthGate 'Warning' }; Expect = $false }
        @{ Name = 'health gate: Unknown refuses'; Run = { Test-PrologueDiskHealthGate 'Unknown' }; Expect = $false }
        @{ Name = 'health gate: an empty read refuses'; Run = { Test-PrologueDiskHealthGate '' }; Expect = $false }
        @{ Name = 'chkntfs: "is dirty" means a check runs at the restart'; Run = { ConvertFrom-PrologueChkntfs @('The type of the file system is NTFS.', 'C: is dirty.') }; Expect = 'dirty' }
        @{ Name = 'chkntfs: "is not dirty" means nothing will run'; Run = { ConvertFrom-PrologueChkntfs @('The type of the file system is NTFS.', 'C: is not dirty.') }; Expect = 'clean' }
        @{ Name = 'chkntfs: a manual schedule is scheduled'; Run = { ConvertFrom-PrologueChkntfs @('The type of the file system is NTFS.', 'Chkdsk has been scheduled manually to run on next reboot on C:.') }; Expect = 'scheduled' }
        @{ Name = "chkntfs: chkdsk's own Y answer is scheduled"; Run = { ConvertFrom-PrologueChkntfs @('Chkdsk cannot run because the volume is in use by another process.', 'Would you like to schedule this volume to be checked the next time the system restarts? (Y/N) Y', 'This volume will be checked the next time the system restarts.') }; Expect = 'scheduled' }
        @{ Name = 'chkntfs: localized output is unknown'; Run = { ConvertFrom-PrologueChkntfs @('Der Typ des Dateisystems ist NTFS.', 'C: ist nicht fehlerhaft.') }; Expect = 'unknown' }
        @{ Name = 'fsutil: NOT Dirty is clean, Dirty is dirty, else unknown'; Run = { "$(ConvertFrom-PrologueFsutilDirty @('Volume - C: is NOT Dirty'))/$(ConvertFrom-PrologueFsutilDirty @('Volume - C: is Dirty'))/$(ConvertFrom-PrologueFsutilDirty @('Error: Access is denied.'))" }; Expect = 'clean/dirty/unknown' }
        @{ Name = 'diskpart: the rig line parses to the MB figure'; Run = { ConvertFrom-PrologueDiskpartQueryMax @('The maximum number of reclaimable bytes is:   17 GB (17417 MB)') }; Expect = 17.0 }
        @{ Name = 'diskpart: a refusal parses to null'; Run = { $null -eq (ConvertFrom-PrologueDiskpartQueryMax @('Use Chkdsk to fix the corruption problem, and then try to shrink the volume again.')) }; Expect = $true }
        @{ Name = 'manage-bde: Protection On / Off / localized'; Run = { "$(ConvertFrom-PrologueManageBde @('    Protection Status:    Protection On'))/$(ConvertFrom-PrologueManageBde @('    Protection Status:    Protection Off'))/$(ConvertFrom-PrologueManageBde @('    Schutzstatus: aktiviert'))" }; Expect = 'on/off/unknown' }
        # the shrink plan and the fork
        @{ Name = 'plan: 85.8 GB C:, 65 GB free, SizeMin 30 GB, Linux 25 GB fits, requests exactly 25 GB'
           Run = { $p = Get-PrologueShrinkPlan -PartSize 85775613952 -SizeMin 32212254720 -FreeBytes 69793218560 -LinuxMinGB 25 -FilesBytes 0; "$($p.Fits):$($p.RequestedBytes -eq 25GB):$($p.ShrinkableBytes -eq (85775613952-32212254720))" }; Expect = 'True:True:True' }
        @{ Name = 'plan: immovable files cap it below Linux -> does not fit, no request'
           Run = { $p = Get-PrologueShrinkPlan -PartSize 85775613952 -SizeMin 70000000000 -FreeBytes 69793218560 -LinuxMinGB 25 -FilesBytes 0; "$($p.Fits):$($null -eq $p.RequestedBytes):$($p.Reason)" }; Expect = 'False:True:immovable files cap the shrink below what Linux needs' }
        @{ Name = 'plan: Windows kept with too little free space -> does not fit'
           Run = { $p = Get-PrologueShrinkPlan -PartSize 85775613952 -SizeMin 10GB -FreeBytes 30GB -LinuxMinGB 25 -FilesBytes 0; "$($p.Fits):$($p.Reason)" }; Expect = 'False:Windows would be left with too little free space' }
        @{ Name = 'plan: harvested files raise the target by 1.2x'
           Run = { $p = Get-PrologueShrinkPlan -PartSize 500GB -SizeMin 100GB -FreeBytes 300GB -LinuxMinGB 25 -FilesBytes 100GB; "$($p.Fits):$($p.TargetBytes -eq [long](25GB + 120GB))" }; Expect = 'True:True' }
        @{ Name = 'plan: a SizeMin above the partition size is zero shrinkable, never negative'
           Run = { (Get-PrologueShrinkPlan -PartSize 10GB -SizeMin 20GB -FreeBytes 1GB -LinuxMinGB 25 -FilesBytes 0).ShrinkableBytes }; Expect = 0 }
        @{ Name = 'fork: keep-windows job that fits keeps Windows'; Run = { Get-PrologueFork -JobPath 'keep-windows' -Fits $true -IfCannotKeep 'stop' }; Expect = 'keep-windows' }
        @{ Name = 'fork: keep-windows job that does not fit takes if_cannot_keep=clean-slate'; Run = { Get-PrologueFork -JobPath 'keep-windows' -Fits $false -IfCannotKeep 'clean-slate' }; Expect = 'clean-slate' }
        @{ Name = 'fork: keep-windows job that does not fit takes if_cannot_keep=stop'; Run = { Get-PrologueFork -JobPath 'keep-windows' -Fits $false -IfCannotKeep 'stop' }; Expect = 'stop' }
        @{ Name = 'fork: an unknown if_cannot_keep stops, never guesses'; Run = { Get-PrologueFork -JobPath 'keep-windows' -Fits $false -IfCannotKeep 'ask' }; Expect = 'stop' }
        @{ Name = 'fork: a clean-slate job is clean slate whatever the number'; Run = { Get-PrologueFork -JobPath 'clean-slate' -Fits $true -IfCannotKeep 'stop' }; Expect = 'clean-slate' }
        # the time estimate is computed from a measurement, never assumed
        @{ Name = 'estimate: 20 GB at 20 MB/s is 1000 s, shown as about 17 minutes'; Run = { $s = Get-PrologueTimeEstimate -Bytes 20000000000 -Mbps 20; "$s/$(Format-PrologueDuration $s)" }; Expect = '1000/about 17 minutes' }
        @{ Name = 'estimate: no measurement gives null, shown as unknown'; Run = { $s = Get-PrologueTimeEstimate -Bytes 1 -Mbps $null; "$($null -eq $s)/$(Format-PrologueDuration $s)" }; Expect = 'True/unknown (no write speed measured)' }
        @{ Name = 'estimate: hours when it is hours'; Run = { Format-PrologueDuration 7200 }; Expect = 'about 2 hours' }
        # the handoff classifier and the marker, unchanged from the harness
        @{ Name = 'classify: fired + cleared + intact is fired-once'; Run = { Get-HandoffResult -Fired $true -SequenceCleared $true -OrderUnchanged $true }; Expect = 'fired-once' }
        @{ Name = 'classify: fired but not cleared is persisted'; Run = { Get-HandoffResult -Fired $true -SequenceCleared $false -OrderUnchanged $true }; Expect = 'persisted' }
        @{ Name = 'classify: order changed is reordered'; Run = { Get-HandoffResult -Fired $true -SequenceCleared $true -OrderUnchanged $false }; Expect = 'reordered' }
        @{ Name = 'classify: not fired, intact is ignored'; Run = { Get-HandoffResult -Fired $false -SequenceCleared $true -OrderUnchanged $true }; Expect = 'ignored' }
        @{ Name = 'grubenv: clean block is 1024 bytes and not fired; upg_fired=1 is fired'; Run = { $b = New-GrubEnvBlock; $t = "# GRUB Environment Block`nupg_fired=1`n" + ('#' * 990); "$($b.Length):$(Test-GrubEnvFired $b):$(Test-GrubEnvFired ([Text.Encoding]::ASCII.GetBytes($t)))" }; Expect = '1024:False:True' }
        @{ Name = 'stick: found again under a new letter by volume id; absent is null'; Run = { $a = Find-StickRoot -UniqueId 'S' -Volumes @([pscustomobject]@{ DriveLetter = 'F'; UniqueId = 'S' }); $b = Find-StickRoot -UniqueId 'S' -Volumes @([pscustomobject]@{ DriveLetter = 'C'; UniqueId = 'X' }); "$a/$($null -eq $b)" }; Expect = 'F:\/True' }
        # the record: the outcome block from the state
        @{ Name = 'record: a fresh state makes a schema-shaped block (needed false, method none, freed 0)'
           Run = { $b = New-PrologueBlock (New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'); "$($b.revalidated):$($b.volume_check.needed):$($b.volume_check.method):$($b.volume_check.restarts):$($b.shrink.freed_bytes):$($b.bitlocker.status_before):$($b.handoff.armed):$($null -eq $b.staged)" }; Expect = 'True:False:none:0:0:off:False:True' }
        @{ Name = 'record: a check that ran carries Healthy, its method, the scan and one restart'
           Run = { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $s.VolumeCheck.Needed = $true; $s.VolumeCheck.Ran = $true; $s.VolumeCheck.DiskHealthAtCheck = 'Healthy'; $s.VolumeCheck.Method = 'spot-fix'; $s.VolumeCheck.Scan = 'NoErrorsFound'; $s.VolumeCheck.Restarts = 1; $s.VolumeCheck.DirtyAfter = 'clean'
                   $b = New-PrologueBlock $s; "$($b.volume_check.ran):$($b.volume_check.disk_health_at_check):$($b.volume_check.method):$($b.volume_check.scan):$($b.volume_check.restarts):$($b.volume_check.dirty_after)" }; Expect = 'True:Healthy:spot-fix:NoErrorsFound:1:clean' }
        @{ Name = 'record: an unrecognised health reading is written as Unknown, never Healthy'
           Run = { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $s.VolumeCheck.DiskHealthAtCheck = 'Degraded'; (New-PrologueBlock $s).volume_check.disk_health_at_check }; Expect = 'Unknown' }
        @{ Name = 'record: mismatches make revalidated false'
           Run = { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $s.Mismatches = @('x'); (New-PrologueBlock $s).revalidated }; Expect = $false }
        @{ Name = 'record: the JSON round-trips through the state file as hashtables with the same block'
           Run = { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $s.Shrink.RemeasuredGB = 61.4; $s.Shrink.RemeasuredBy = 'storage-api'; $s.Shrink.FreedBytes = 34359738368
                   $back = ConvertTo-PrologueHashtable ((ConvertTo-PrologueJson $s) | ConvertFrom-Json); $b = New-PrologueBlock $back; "$($b.shrink.remeasured_gb):$($b.shrink.remeasured_by):$($b.shrink.freed_bytes):$($back.Mismatches.Count)" }; Expect = '61.4:storage-api:34359738368:0' }
        @{ Name = 'stopped outcome: status stopped, the stage, the reason, line not crossed, credentials scrubbed'
           Run = { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $s.VolumeCheck.Needed = $true; $s.VolumeCheck.DiskHealthAtCheck = 'Warning'
                   $o = New-PrologueStoppedOutcome -Job $job -S $s -StoppedAt 'volume-check' -Reason 'Warning disk' -WindowsPartition ([ordered]@{ number = 3; guid = 'g'; size_bytes = 1 })
                   "$($o.status):$($o.stopped_at):$($o.commit_line.crossed):$($o.path_taken -eq $null):$($o.credentials.scrubbed):$($o.credentials.scrub_after):$($o.windows.kept):$($o.prologue.volume_check.ran)" }; Expect = 'stopped:volume-check:False:True:True:cutover:True:False' }
        @{ Name = 'stopped outcome: a stop after the keep-windows fork keeps scrub_after settle-in-pull'
           Run = { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $s.Shrink.ForkTaken = 'keep-windows'; $o = New-PrologueStoppedOutcome -Job $job -S $s -StoppedAt 'arm-handoff' -Reason 'x' -WindowsPartition $null; "$($o.path_taken):$($o.credentials.scrub_after)" }; Expect = 'keep-windows:settle-in-pull' }
        @{ Name = 'json: LF only, schema string present'
           Run = { $j = ConvertTo-PrologueJson (New-PrologueStoppedOutcome -Job $job -S (New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\') -StoppedAt 'revalidate' -Reason 'r' -WindowsPartition $null); (-not $j.Contains("`r")) -and ($j -match '"schema":\s*"outcome/1"') }; Expect = $true }
        @{ Name = 'drive: e normalizes to E:\, a path is refused'; Run = { "$(Get-DriveRoot 'e')/$(try { Get-DriveRoot 'E:\x'; 'accepted' } catch { 'refused' })" }; Expect = 'E:\/refused' }
    )
    $failed = 0
    Write-Host ''; Write-Host "  upgrade_  prologue $PrologueVersion  -  SELF-TEST" -ForegroundColor Cyan; Write-Host ''
    foreach ($c in $cases) {
        $got = & $c.Run
        if ("$got" -eq "$($c.Expect)") { Write-Host "    PASS  $($c.Name)" -ForegroundColor Green }
        else { Write-Host "    FAIL  $($c.Name)  (expected '$($c.Expect)', got '$got')" -ForegroundColor Red; $failed++ }
    }
    Write-Host ''
    if ($failed -gt 0) { Write-Host "  $failed check(s) failed" -ForegroundColor Red; exit 1 }
    Write-Host '  all checks passed' -ForegroundColor Green; Write-Host ''
}

# =============================================================================
#  main
# =============================================================================

if ($SelfTest) { Invoke-SelfTest; return }
if (-not (Test-Elevated)) { throw 'the prologue needs Administrator: it reads and changes the disk and the boot configuration' }
if (-not (Test-UefiBoot)) { throw 'this machine is not UEFI-booted; the boot handoff does not apply' }
if ($Abort) { Invoke-AbortPhase; return }
if ($Start) { Invoke-StartPhase; return }
Invoke-ResumePhase
