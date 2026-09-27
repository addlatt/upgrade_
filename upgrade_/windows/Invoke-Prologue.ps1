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
          removes the boot entry, puts hibernation and the pagefile back
          as it found them) and scrubs the stick's credentials.

    Restarts are resumed by a one-shot task that runs as SYSTEM at startup
    (-Resume), before and without anyone signing in - the walk-away half of
    the promise (decided 2026-09-13; RISKS R18). Nothing after the typed word
    needs a person: the fork is pre-chosen in job.json. Anything the person
    should read (a stop, the return) is queued as a one-shot notice shown at
    their next sign-in, since a task in session 0 has no screen. The state
    directory is locked to SYSTEM and Administrators before the task is
    registered: a SYSTEM task must never run a script a standard user can
    replace. On the return to Windows after the handoff the same task
    classifies the handoff, removes the boot entry and the task, and leaves
    upgrade_/prologue-return.json on the stick.

    Nothing here crosses the commit line. Every refusal happens before the
    step it guards touches anything it cannot undo.

.PARAMETER Start
    Begin a conversion. Needs -StickDrive and -ConfirmWord CONVERT.

.PARAMETER Resume
    Continue after a restart (registered as the SYSTEM startup task; can be
    run by hand, elevated). Reads the state the previous phase left.

.PARAMETER Notify
    Show the notice the last unattended phase queued for the next sign-in
    (registered under HKLM RunOnce; runs as the person, unelevated).

.PARAMETER Probe
    The walk-away probe (read-only, one restart): register the same SYSTEM
    startup task the conversion uses, restart, and on the way back record
    who ran the resume, whether anyone was signed in, how long the stick
    took to appear, and whether the sign-in notice queued - then remove
    the task. Touches nothing but the state directory and the task. Row to
    upgrade_/walkaway-probe.csv on the stick (never edited by hand),
    record to upgrade_/probe.json. The physical residue of the SYSTEM
    resume: a real USB stick at real firmware's boot, a real sign-in.

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
    [Parameter(ParameterSetName = 'Start')][string]$AcknowledgeDataLoss,
    [Parameter(ParameterSetName = 'Start')][string]$EraseConsent,
    [Parameter(ParameterSetName = 'Resume', Mandatory = $true)][switch]$Resume,
    [Parameter(ParameterSetName = 'Abort', Mandatory = $true)][switch]$Abort,
    [Parameter(ParameterSetName = 'SelfTest', Mandatory = $true)][switch]$SelfTest,
    [Parameter(ParameterSetName = 'Notify', Mandatory = $true)][switch]$Notify,
    [Parameter(ParameterSetName = 'Probe', Mandatory = $true)][switch]$Probe,
    [Parameter(ParameterSetName = 'Probe', Mandatory = $true)][string]$ProbeStickDrive,
    [string]$StateDir
)
$ErrorActionPreference = 'Stop'
$PrologueVersion = '0.11.0'   # 0.11.0 (2026-09-27): every stop and every return to Windows removes the stick's Wi-Fi passwords (the owner); 0.10.0 (2026-09-26): the erase-and-install path (RISKS R27); 0.9.1: only the no-folders stop message
$TaskName = 'upgrade_ prologue resume'
$NoticeRunOnceName = 'upgrade_ prologue notice'
$ProbeCsvHeader = @('timestamp', 'prologue_version', 'vendor', 'model', 'bios', 'os', 'secure_boot', 'stick_bus', 'run_as', 'session_id', 'interactive', 'explorer_running', 'uptime_s', 'stick_wait_s', 'notice', 'task_removed', 'result', 'notes')
$StickWaitSeconds = 120           # a USB stick can enumerate well after the startup task starts
$ConfirmExpected = 'CONVERT'
# The acknowledged-data-loss path (RISKS R23, decided 2026-09-13): when the job
# carries risk_acknowledgement, the same sentence must be typed for THIS run,
# and only the two named refusals are lifted - the disk-health gate and the
# volume-health stop. Nothing else in this file reads it.
$RiskStatement = 'I confirm that I understand the risks and could lose data'
# The one-click erase and install (RISKS R27, decided 2026-09-26): a job that
# carries erase_consent is started only with this sentence typed for THIS run
# (-EraseConsent); it stands in for the CONVERT word on that path and lifts
# nothing. The prologue changes nothing on the drives; the erase is the
# installer's, after its countdown.
$EraseStatement = 'I confirm that everything on this computer will be deleted and nothing will be kept'
$GrubEnvRel = 'EFI\BOOT\grubenv'
$GrubFiredVar = 'upg_fired'
$PayloadEfi = '\EFI\BOOT\BOOTX64.EFI'
$WindowsKeepFreeBytes = 8GB      # what the kept Windows must still have free after the shrink
$FilesMargin = 1.2               # headroom over the harvested bytes Linux must hold until reclaim
$StageProbeBytes = 32MB
# RISKS R25 (found 2026-09-23 on the Aspire, built 2026-09-26): a Windows update
# waiting for a restart is let finish before anything changes, and nothing is
# armed while one waits. At most this many restarts of our own for it:
$UpdateMaxRestarts = 3
$UpdateWaitSeconds = 600          # after an update restart, how long Windows gets to finish before we look again
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

function Test-PrologueRepairQueued {
    # Pure (self-tested): Windows' own word that C: has an offline repair
    # queued, whatever the dirty bit says - the volume's OperationalStatus
    # naming a repair ("Full Repair Needed") or NTFS event 98 ("needs to be
    # taken offline to perform a Full Chkdsk"). On the Aspire (2026-09-17,
    # RISKS R18) the bit read clean with both of these set, and the Storage
    # API answered 0 GB shrinkable with no error.
    # An event is history, not state (2026-09-20, the diagnostic over SSH):
    # the full check had run on 09-15 (autochk log, Wininit 1001) and the
    # 09-13 event still sat in the 30-day window - 0.4.0 scheduled a check
    # for a repair already done. An event 98 counts only when no completed
    # check postdates it; the volume's current status always counts.
    param([string]$VolumeStatus, $NtfsFullChkdsk, $LastCheck)
    $why = @(); $stale = ''
    if ("$VolumeStatus" -match '(?i)repair') { $why += "Get-Volume reports '$VolumeStatus'" }
    if ($NtfsFullChkdsk) {
        if (Test-PrologueNtfs98Fresh -Ntfs98 $NtfsFullChkdsk -LastCheck $LastCheck) { $why += "NTFS logged at $NtfsFullChkdsk that C: needs a full chkdsk" }
        else { $stale = "NTFS asked for a full chkdsk at $NtfsFullChkdsk; a boot-time check completed after it, at $LastCheck" }
    }
    @{ Queued = ($why.Count -gt 0); Why = ($why -join '; '); Stale = $stale }
}

function Test-PrologueNtfs98Fresh {
    # Pure (self-tested): does NTFS's request still stand? Times are the
    # round-trip strings the evidence carries (or DateTimes).
    param($Ntfs98, $LastCheck)
    if (-not $Ntfs98) { return $false }
    if (-not $LastCheck) { return $true }
    $rt = [Globalization.DateTimeStyles]::RoundtripKind
    $a = if ($Ntfs98 -is [DateTime]) { $Ntfs98 } else { [DateTime]::Parse("$Ntfs98", $null, $rt) }
    $b = if ($LastCheck -is [DateTime]) { $LastCheck } else { [DateTime]::Parse("$LastCheck", $null, $rt) }
    $a.ToUniversalTime() -gt $b.ToUniversalTime()
}

function ConvertFrom-PrologueDefrag259 {
    # Pure (self-tested): Defrag event 259 -> the last unmovable file, or $null.
    param([string]$Message)
    if ("$Message" -match '(?im)last unmovable file appears to be:\s*(\S.*?)\s*$') { return ($matches[1] -replace '::\$DATA$', '') }
    $null
}

function Get-PrologueLastUnmovable {
    # Live, read-only: what Windows named after the shrink analysis just run.
    param([DateTime]$Since)
    try { $e = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Microsoft-Windows-Defrag'; Id = 259; StartTime = $Since } -ErrorAction SilentlyContinue | Sort-Object TimeCreated -Descending | Select-Object -First 1; if ($e) { return (ConvertFrom-PrologueDefrag259 -Message "$($e.Message)") } } catch { }
    $null
}

function Get-PrologueVolumeTrigger {
    # Pure (self-tested): whether step 1b runs, and on which of Windows' two
    # statements. 'none' skips it; 'unreadable' is a stop, never a skip.
    param([string]$Dirty, [bool]$RepairQueued)
    if ($Dirty -eq 'dirty') { return 'dirty-flag' }
    if ($Dirty -eq 'clean') { if ($RepairQueued) { return 'repair-queued' } else { return 'none' } }
    'unreadable'
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

function ConvertFrom-PrologueChkdskEvent {
    # Pure: a Chkdsk-provider event's text -> what it concluded (real lines,
    # Acer Aspire 2026-09-13). The cmdlet's return string said NoErrorsFound
    # on that machine while this log said "found problems" every day.
    param([string]$Message)
    $t = "$Message"
    $r = [ordered]@{ Verdict = 'unknown'; Records = 0; Queued = 0 }
    if ($t -match '(?i)Examining\s+(\d+)\s+corruption records') { $r.Records = [int]$matches[1] }
    $r.Queued = ([regex]::Matches($t, '(?i)queued for offline repair')).Count
    if ($t -match '(?i)found problems') { $r.Verdict = 'found-problems' }
    elseif ($t -match '(?i)found no problems') { $r.Verdict = 'no-problems' }
    elseif ($r.Queued -gt 0) { $r.Verdict = 'found-problems' }
    $r
}

function ConvertFrom-PrologueDiskEvents {
    # Pure: 'disk' provider events -> bad-block / paging / reset counts for \Device\HarddiskN.
    param($Events, [int]$DiskNumber)
    $r = [ordered]@{ BadBlock = 0; Paging = 0; Reset = 0; First = $null; Last = $null }
    $pat = [regex]::Escape('\Device\Harddisk' + $DiskNumber + '\')
    foreach ($e in @($Events)) {
        if (-not $e -or "$($e.Message)" -notmatch $pat) { continue }
        switch ([int]$e.Id) { 7 { $r.BadBlock++ } 51 { $r.Paging++ } 153 { $r.Reset++ } default { continue } }
        if ($null -eq $r.First -or $e.TimeCreated -lt $r.First) { $r.First = $e.TimeCreated }
        if ($null -eq $r.Last -or $e.TimeCreated -gt $r.Last) { $r.Last = $e.TimeCreated }
    }
    $r
}

function Get-PrologueRepairMethod {
    # Guardrail 3 (RISKS R18): the evidence chooses the rung. Real errors -
    # the Chkdsk log saying "found problems", the volume reporting "Full
    # Repair Needed", NTFS's own event 98 asking for a full chkdsk, or the
    # cmdlet answering ErrorsFound - mean the full check (/f). A cmdlet
    # NoErrorsFound with none of those means the spot-fix. Anything else is
    # a refusal: never repair on a guess about what is wrong. The cmdlet's
    # string alone never chooses /f over the evidence and never overrides it
    # (on the Aspire it said NoErrorsFound against 18 queued corruption records).
    param([string]$Scan, [string]$LogVerdict = '', [bool]$RepairNeeded = $false, [bool]$NtfsFullChkdsk = $false)
    $s = "$Scan".Trim()
    if ($LogVerdict -eq 'found-problems' -or $RepairNeeded -or $NtfsFullChkdsk -or $s -match '^(ErrorsFound|ErrorsNotFixed)$') { return 'chkdsk-f' }
    if ($s -match '^(NoErrorsFound|ErrorsFixed)$') { return 'spot-fix' }
    'refuse'
}

function Test-PrologueDiskHealthGate {
    # Guardrail 2: a repair runs only on a disk that says Healthy AND whose
    # error log holds no bad-block events (the Aspire's said Healthy over 261
    # of them). The acknowledged-data-loss path (R23) lifts this gate, and
    # the record says so in words. Returns @{ Pass; Reason }.
    param([string]$Health, [int]$BadBlocks = 0, [bool]$Acknowledged = $false)
    $h = "$Health".Trim()
    $why = @()
    if ($h -ne 'Healthy') { $why += "HealthStatus is '$h', not Healthy" }
    if ($BadBlocks -gt 0) { $why += "Windows logged $BadBlocks bad-block errors on this disk in the last 30 days" }
    if ($why.Count -eq 0) { return @{ Pass = $true; Reason = 'Healthy, no bad-block events' } }
    if ($Acknowledged) { return @{ Pass = $true; Reason = 'DATA LOSS ACCEPTED: ' + ($why -join '; ') + ' - the person acknowledged the disk-health refusal' } }
    @{ Pass = $false; Reason = ($why -join '; ') + '; a repair on a failing drive can finish it off' }
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
    cmp 'volume_health.repair_queued' ([bool]$Job.storage.volume_health.repair_queued) ([bool]$F.RepairQueued)
    cmp 'physical_disk.health_status' $Job.storage.physical_disk.health_status $F.Health
    $m.ToArray()
}

function Compare-PrologueEraseDisks {
    # Pure (self-tested). Every drive an erase job names must still be here,
    # by unique id and exact size, and the first must be the drive holding C:.
    param($Job, $F)
    $m = @()
    $disks = @($Job.erase_consent.disks)
    if ($disks.Count -lt 1) { return @('erase_consent names no drives') }
    if ("$($disks[0].role)" -ne 'system' -or "$($disks[0].unique_id)" -ne "$($F.Disk.UniqueId)" -or [long]$disks[0].size_bytes -ne [long]$F.Disk.Size) { $m += "erase_consent.disks[0] is not the drive holding C: ($($F.Disk.UniqueId), $($F.Disk.Size) bytes)" }
    foreach ($d in @($disks | Select-Object -Skip 1)) {
        $hit = @($F.AllDisks | Where-Object { "$($_.UniqueId)" -eq "$($d.unique_id)" })
        if ($hit.Count -eq 0) { $m += "the $($d.role) drive the job names ($($d.friendly_name), $($d.unique_id)) is not attached" }
        elseif ([long]$hit[0].Size -ne [long]$d.size_bytes) { $m += "the $($d.role) drive ($($d.friendly_name)) is $($hit[0].Size) bytes; the job says $($d.size_bytes)" }
    }
    $m
}

function Get-PrologueEraseStartRefusal {
    # Pure (self-tested). Which start is allowed: an erase job only with the
    # erase sentence typed for this run (it stands in for CONVERT); any other
    # job only with CONVERT, and never with the erase sentence.
    param($Job, [string]$ConfirmWord, [string]$EraseConsent)
    $isErase = [bool]($Job.PSObject.Properties['erase_consent'] -and $Job.erase_consent)
    if ($isErase) {
        if ("$($Job.erase_consent.statement)" -cne $EraseStatement) { return 'the job carries an erase consent whose sentence is not the one this prologue knows; refusing' }
        if ($EraseConsent -cne $EraseStatement) { return 'this job erases every drive, but the erase sentence was not typed for this run (-EraseConsent); nothing was started' }
        return $null
    }
    if ($EraseConsent) { return 'an erase sentence was given but the job is not an erase job; use the normal launcher' }
    if ($ConfirmWord -cne $ConfirmExpected) { return "the confirmation word was not typed (expected $ConfirmExpected); nothing was started" }
    $null
}

function Get-PrologueEraseReturn {
    # Pure (self-tested). Windows came back on the erase path, so nothing was
    # erased: say why, from what the installer left on the stick.
    param($Countdown, $Verify)
    if ($Countdown -and "$($Countdown.result)" -eq 'cancelled') { return @{ StoppedAt = 'countdown'; Reason = "a key was pressed during the countdown in the installer ($($Countdown.ended_utc)); nothing was erased and Windows is as it was" } }
    if ($Countdown -and "$($Countdown.result)" -eq 'elapsed') { return @{ StoppedAt = 'install'; Reason = "the countdown ended ($($Countdown.ended_utc)) but Windows started again: the install did not complete - the drives may be partly erased; read upgrade_/report on the stick" } }
    if ($Verify -and "$($Verify.identity.result)" -eq 'fail') { return @{ StoppedAt = 'identity'; Reason = 'the installer did not find the drives the job names, by identity and exact size; it refused before the countdown and nothing was erased' } }
    if ($Verify -and "$($Verify.payload.result)" -eq 'fail') { return @{ StoppedAt = 'verify-stick'; Reason = "the desktop image on the stick did not read back correctly ($($Verify.payload.detail)); the installer refused before the countdown and nothing was erased" } }
    @{ StoppedAt = 'install'; Reason = 'Windows came back before the countdown ended (the installer stopped, or the computer was restarted); nothing was erased - upgrade_/report/verify.log on the stick says where it stopped' }
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
    # A clean-slate job the writer forced for lack of room is only honoured when
    # the person's fork allowed a wipe: job writer 0.7.0 forced one under
    # if_cannot_keep = stop (the Aspire, 2026-09-22, R18); a stale job like
    # that stops here, never stages toward a wipe the person declined.
    param([string]$JobPath, [bool]$Fits, [string]$IfCannotKeep, [string]$PathReason)
    if ($JobPath -eq 'clean-slate') {
        if ($PathReason -eq 'forced-no-room' -and $IfCannotKeep -ne 'clean-slate') { return 'stop' }
        return 'clean-slate'
    }
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

function Get-ResumeContext {
    # Who is running this resume, and is there a screen. A SYSTEM startup task
    # runs in session 0 with no interactive desktop: UserInteractive is false
    # there, so every notice goes through RunOnce instead of a popup.
    param([string]$UserName, [bool]$UserInteractive, [int]$SessionId, [bool]$ExplorerRunning)
    [ordered]@{ RunAs = $UserName; Interactive = $UserInteractive; SessionId = $SessionId; ExplorerRunning = $ExplorerRunning
                Unattended = (-not $UserInteractive -or $SessionId -eq 0) }
}

function ConvertTo-ResumeEvidence {
    # The contract's view of one resume (outcome.schema.json prologue.resumes):
    # SYSTEM or user, never the raw account name.
    param($R)
    [ordered]@{ utc = "$($R.Utc)"; run_as = $(if ("$($R.RunAs)" -match '(?i)(^|\\)SYSTEM$') { 'SYSTEM' } else { 'user' })
                session_id = [int]$R.SessionId; unattended = [bool]$R.Unattended
                stick_wait_seconds = $(if ($null -ne $R.StickWaitSeconds) { [int]$R.StickWaitSeconds } else { $null }) }
}

function Get-ProbeResult {
    # The probe's verdict from its own facts. 'resumed-unattended' is the
    # walk-away property; anything else names what was missing.
    param([bool]$Unattended, [bool]$StickFound, [bool]$TaskRemoved)
    if (-not $StickFound) { return 'stick-not-found' }
    if (-not $Unattended) { return 'resumed-attended' }
    if (-not $TaskRemoved) { return 'task-not-removed' }
    'resumed-unattended'
}

function ConvertTo-ProbeCsvLine {
    # every field quoted, quotes doubled, no newlines
    param([object[]]$Fields)
    (@($Fields | ForEach-Object { '"' + (("$_" -replace '[\r\n]+', ' ') -replace '"', '""') + '"' }) -join ',')
}

function New-NoticeCommand {
    # The RunOnce value: show the notice file at the next sign-in, as the
    # person, unelevated, from the state dir's copy of this script.
    param([string]$ScriptPath, [string]$State)
    "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$ScriptPath`" -Notify -StateDir `"$State`""
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
        Ack = [ordered]@{ Present = $false; DiskHealth = $false; VolumeHealth = $false }
        VolumeCheck = [ordered]@{ Trigger = $null; Needed = $false; Ran = $false; Scan = $null; DiskHealthAtCheck = $null; BadBlocks = 0; Gate = $null; Evidence = $null; Method = 'none'; ArmedUtc = $null; ArmText = $null; Chkntfs = $null; Wininit1001 = $null; Found000 = $null; DirtyAfter = 'unknown'; Restarts = 0 }
        Shrink = [ordered]@{ LastUnmovable = $null; RemeasuredGB = $null; RemeasuredBy = $null; DiskpartGB = $null; ApiError = $null; DiskpartError = $null; PartSize = $null; SizeMin = $null; FreeBytes = $null; Plan = $null; ForkTaken = $null; RequestedBytes = $null; FreedBytes = 0; SizeBefore = $null; PagefileDisabled = $false; HibernationDisabled = $false; Mitigated = $false; Before = $null; Restored = $null; RestorePoints = $null; UsnJournal = $null }
        Update = [ordered]@{ Checks = @(); Restarts = 0; ResumeTo = $null }
        Staged = $null
        BitLocker = [ordered]@{ StatusBefore = $null; Source = $null; Suspended = $false; RebootCount = $null }
        Handoff = [ordered]@{ Armed = $false; Marker = $null; EntryGuid = $null; ArmedUtc = $null; BcdBackup = $null; Before = $null; GrubEnvReset = $false }
        Resumes = @()
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
            trigger = $(if ($vc.Trigger) { "$($vc.Trigger)" } else { $null })
        }
        shrink = [ordered]@{
            remeasured_gb = $sh.RemeasuredGB; remeasured_by = $sh.RemeasuredBy; remeasured_diskpart_gb = $sh.DiskpartGB
            fork_taken = $sh.ForkTaken; requested_bytes = $sh.RequestedBytes; freed_bytes = [long]$sh.FreedBytes
            pagefile_disabled = [bool]$sh.PagefileDisabled; hibernation_disabled = [bool]$sh.HibernationDisabled; restore_points_deleted = $(if ($sh.RestorePoints) { [int]$sh.RestorePoints.Deleted } else { 0 }); usn_journal_deleted = $(if ($sh.UsnJournal) { [int]$sh.UsnJournal.Deletions } else { 0 })
        }
    }
    if ($S.Contains('Resumes') -and @($S.Resumes).Count -gt 0) {
        $b.resumes = @(foreach ($r in @($S.Resumes)) { ConvertTo-ResumeEvidence $r })
    }
    if ($S.Contains('Update') -and $S.Update -and @($S.Update.Checks).Count -gt 0) {
        # RISKS R25: every pending-restart check and the restarts it took
        $b.windows_update = [ordered]@{ checks = @($S.Update.Checks).Count; pending_seen = [bool](@($S.Update.Checks | Where-Object { $_.Pending }).Count -gt 0); restarts = [int]$S.Update.Restarts }
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
    $o = [ordered]@{
        schema = 'outcome/1'; job_id = "$($Job.job_id)"
        created_utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        converter_version = "prologue $PrologueVersion"
        status = 'stopped'; stopped_at = $StoppedAt; reason = $Reason; path_taken = $path
        commit_line = [ordered]@{ crossed = $false; crossed_utc = $null; act = $null }
        prologue = (New-PrologueBlock $S)
        risk_acknowledgement = $(if ($Job.PSObject.Properties['risk_acknowledgement'] -and $Job.risk_acknowledgement) { [ordered]@{ statement = "$($Job.risk_acknowledgement.statement)"; accepted_utc = "$($Job.risk_acknowledgement.accepted_utc)"; overrides = @($Job.risk_acknowledgement.overrides) } } else { $null })
        windows = [ordered]@{ kept = $true; partition = $WindowsPartition; reachable_via = 'firmware-entry' }
        credentials = [ordered]@{ scrubbed = $true; scrub_after = $(if ($path -eq 'keep-windows') { 'settle-in-pull' } else { 'cutover' }) }
        logs = @('upgrade_/report/prologue.log')
    }
    if ($null -eq $o.risk_acknowledgement) { $o.Remove('risk_acknowledgement') }
    if ($Job.PSObject.Properties['erase_consent'] -and $Job.erase_consent) { $o.erase_consent = $Job.erase_consent }
    $o
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

function Get-PrologueDiskEvents {
    param([int]$DiskNumber)
    try {
        $ev = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'disk'; StartTime = (Get-Date).AddDays(-30) } -ErrorAction SilentlyContinue |
                ForEach-Object { [pscustomobject]@{ Id = [int]$_.Id; TimeCreated = $_.TimeCreated; Message = "$($_.Message)" } })
        ConvertFrom-PrologueDiskEvents -Events $ev -DiskNumber $DiskNumber
    } catch { [ordered]@{ BadBlock = 0; Paging = 0; Reset = 0; First = $null; Last = $null; Error = "$($_.Exception.Message)" } }
}

function Get-PrologueVolumeEvidence {
    # What Windows itself says about C:, beyond the cmdlet's string: the
    # volume's OperationalStatus, NTFS event 98 (needs a Full Chkdsk) and the
    # latest Chkdsk-provider event since $Since.
    param([DateTime]$Since)
    $r = [ordered]@{ VolumeStatus = $null; VolumeHealth = $null; NtfsFullChkdsk = $null; LastCheck = $null; LogVerdict = 'unknown'; LogRecords = 0; LogQueued = 0; LogWhen = $null }
    # the last completed boot-time check: its Wininit 1001, or autochk's own log
    try {
        $last = $null
        $w = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1001; StartTime = (Get-Date).AddDays(-60) } -ErrorAction SilentlyContinue | Where-Object { "$($_.ProviderName)" -match 'Wininit' } | Sort-Object TimeCreated -Descending | Select-Object -First 1
        if ($w) { $last = $w.TimeCreated }
        $l = Get-ChildItem 'C:\System Volume Information\Chkdsk' -Force -Filter 'Chkdsk*.log' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($l -and (-not $last -or $l.LastWriteTime -gt $last)) { $last = $l.LastWriteTime }
        if ($last) { $r.LastCheck = $last.ToUniversalTime().ToString('o') }
    } catch { }
    try { $v = Get-Volume -DriveLetter C -ErrorAction Stop; $r.VolumeStatus = (@($v.OperationalStatus) -join ','); $r.VolumeHealth = "$($v.HealthStatus)" } catch { }
    try {
        $n98 = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; Id = 98; StartTime = (Get-Date).AddDays(-30) } -ErrorAction SilentlyContinue |
                 Where-Object { "$($_.ProviderName)" -match 'Ntfs' -and "$($_.Message)" -match '(?i)Full Chkdsk' -and "$($_.Message)" -match '(?i)Volume C:' } | Sort-Object TimeCreated -Descending | Select-Object -First 1)
        if ($n98.Count) { $r.NtfsFullChkdsk = $n98[0].TimeCreated.ToUniversalTime().ToString('o') }
    } catch { }
    try {
        $ce = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Chkdsk'; StartTime = $Since } -ErrorAction SilentlyContinue | Sort-Object TimeCreated -Descending | Select-Object -First 1)
        if ($ce.Count) { $lg = ConvertFrom-PrologueChkdskEvent -Message "$($ce[0].Message)"; $r.LogVerdict = $lg.Verdict; $r.LogRecords = $lg.Records; $r.LogQueued = $lg.Queued; $r.LogWhen = $ce[0].TimeCreated.ToUniversalTime().ToString('o') }
    } catch { }
    $r
}

function Format-PrologueScan {
    # One string for outcome.json's volume_check.scan: the cmdlet's answer AND the evidence.
    param([string]$Cmdlet, $Ev)
    $parts = @("cmdlet: $Cmdlet")
    if ($Ev) {
        $parts += "log: $($Ev.LogVerdict)" + $(if ($Ev.LogRecords -gt 0) { " ($($Ev.LogRecords) corruption records" + $(if ($Ev.LogQueued -gt 0) { ", $($Ev.LogQueued) queued for offline repair" }) + ')' })
        if ($Ev.VolumeStatus) { $parts += "volume: $($Ev.VolumeStatus)" }
        if ($Ev.NtfsFullChkdsk) { $parts += "ntfs98: $($Ev.NtfsFullChkdsk)" }
    }
    $parts -join '; '
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
    $f.OsCaption = "$($os.Caption)"; $f.OsBuild = [int]$os.BuildNumber; $f.BiosVersion = "$($bios.SMBIOSBIOSVersion)"
    $f.Firmware = "$env:firmware_type"
    $f.SecureBoot = try { if (Confirm-SecureBootUEFI) { 'on' } else { 'off' } } catch { 'unknown' }
    $part = Get-Partition -DriveLetter C -ErrorAction Stop
    $disk = Get-Disk -Number $part.DiskNumber -ErrorAction Stop
    $f.Disk = [ordered]@{ Number = [int]$disk.Number; Serial = ("$($disk.SerialNumber)" -replace '\s', ''); UniqueId = "$($disk.UniqueId)"; Size = [long]$disk.Size }
    $f.Partition = [ordered]@{ number = [int]$part.PartitionNumber; guid = "$($part.Guid)"; size_bytes = [long]$part.Size }
    $f.Health = Get-PrologueDiskHealth -DiskNumber $disk.Number -UniqueId $disk.UniqueId
    $f.Dirty = Get-PrologueDirty
    $ev0 = Get-PrologueVolumeEvidence -Since (Get-Date).AddDays(-30)
    $rq = Test-PrologueRepairQueued -VolumeStatus "$($ev0.VolumeStatus)" -NtfsFullChkdsk $ev0.NtfsFullChkdsk -LastCheck $ev0.LastCheck
    $f.RepairQueued = [bool]$rq.Queued; $f.RepairQueuedWhy = "$($rq.Why)"; $f.RepairStale = "$($rq.Stale)"
    $blq = Get-BitLockerState; $f.BitLocker = $blq.State; $f.BitLockerSource = $blq.Source; $f.BitLockerRaw = $blq.Raw
    $f.Stick = $null; $f.StickError = $null
    try {
        $l = $Root.Substring(0, 1)
        $sv = Get-Volume -DriveLetter $l -ErrorAction Stop
        $sp = Get-Partition -DriveLetter $l -ErrorAction Stop
        $sd = Get-Disk -Number $sp.DiskNumber -ErrorAction Stop
        $f.Stick = [ordered]@{ UniqueId = "$($sd.UniqueId)"; Size = [long]$sd.Size; Bus = "$($sd.BusType)"; VolumeId = "$($sv.UniqueId)"; Free = [long]$sv.SizeRemaining }
    } catch { $f.StickError = "$($_.Exception.Message)" }
    $f.AllDisks = @(Get-Disk | ForEach-Object { [ordered]@{ Number = [int]$_.Number; UniqueId = "$($_.UniqueId)"; Size = [long]$_.Size; Bus = "$($_.BusType)" } })
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

function Test-PrologueShadowStorageFile {
    # System Restore's shadow-copy storage, as Defrag 259 names it (Aspire,
    # 2026-09-20): \System Volume Information\{id}{3808876b-c176-4e48-b7ae-04046e6cc752}.
    # The second GUID is the Volume Shadow Copy service's own; nothing else under
    # System Volume Information counts.
    param([string]$LastUnmovable)
    [bool]("$LastUnmovable" -match '(?i)^\\?System Volume Information\\[^\\]*\{3808876b-c176-4e48-b7ae-04046e6cc752\}')
}

function Get-PrologueRestorePointStep {
    # Judge (R18, decided 2026-09-20). Restore points are deleted only when the
    # number does not fit, Windows itself names their storage as the file in the
    # way, the job carries the person's consent, and it has not been done already.
    param([bool]$Fits, [string]$LastUnmovable, [bool]$Consented, [bool]$AlreadyDone)
    if ($Fits -or -not (Test-PrologueShadowStorageFile -LastUnmovable $LastUnmovable)) { return 'none' }
    if ($AlreadyDone) { return 'already-done' }
    if (-not $Consented) { return 'no-consent' }
    'delete'
}

function Get-PrologueShadowCopyCount {
    try { $dev = (Get-CimInstance Win32_Volume -Filter "DriveLetter='C:'" -ErrorAction Stop).DeviceID
          @(Get-CimInstance Win32_ShadowCopy -ErrorAction Stop | Where-Object { $_.VolumeName -eq $dev }).Count } catch { $null }
}

function Get-PrologueRestorePointVerdict {
    # Pure (self-tested). What the counts say happened - never what was meant to
    # happen. On the Aspire (2026-09-23, R18 sixth run) vssadmin left 2 of 2 and
    # the next pass said "already deleted": the words come from here now.
    param($Before, $After)
    if ($null -eq $Before -or $null -eq $After) { return 'unknown' }
    if ([int]$Before -eq 0) { return 'none-there' }
    if ([int]$After -eq 0) { return 'deleted-all' }
    if ([int]$After -lt [int]$Before) { return 'deleted-some' }
    'deleted-none'
}

function Invoke-PrologueDeleteRestorePoints {
    # The one thing the prologue does that no stop can undo. C: only, two of
    # Windows' own documented ways, and a record of what each answered:
    # vssadmin (whose /quiet hides every message, so its exit code is kept),
    # then - only if the count did not drop - each shadow copy's WMI object,
    # one by one, with the error Windows gives for any it refuses.
    $before = Get-PrologueShadowCopyCount
    $text = ''; $code = $null
    try { $text = (& vssadmin delete shadows /for=C: /all /quiet 2>&1 | Out-String).Trim(); $code = $LASTEXITCODE }
    catch { $text = "vssadmin raised: $($_.Exception.Message)"; $code = $LASTEXITCODE }
    $afterVss = Get-PrologueShadowCopyCount
    $wmi = @()
    if ((Get-PrologueRestorePointVerdict -Before $before -After $afterVss) -in @('deleted-none', 'deleted-some', 'unknown')) {
        try {
            $dev = (Get-CimInstance Win32_Volume -Filter "DriveLetter='C:'" -ErrorAction Stop).DeviceID
            foreach ($sc in @(Get-CimInstance Win32_ShadowCopy -ErrorAction Stop | Where-Object { $_.VolumeName -eq $dev })) {
                try { $sc | Remove-CimInstance -ErrorAction Stop; $wmi += "$($sc.ID): removed" }
                catch { $wmi += "$($sc.ID): $($_.Exception.Message)" }
            }
        } catch { $wmi += "listing shadow copies failed: $($_.Exception.Message)" }
    }
    $after = Get-PrologueShadowCopyCount
    [ordered]@{ Before = $before; AfterVssadmin = $afterVss; After = $after
                Deleted = $(if ($null -ne $before -and $null -ne $after) { [math]::Max(0, [int]$before - [int]$after) } else { 0 })
                Verdict = (Get-PrologueRestorePointVerdict -Before $before -After $after)
                VssadminExit = $code; Text = $text; Wmi = @($wmi); Utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ') }
}

function Format-PrologueRestorePoints {
    # Pure (self-tested): the log line for a restore-point attempt, from its record.
    param($R)
    $what = switch ("$($R.Verdict)") {
        'deleted-all'  { "deleted all $($R.Before)" }
        'deleted-some' { "deleted $($R.Deleted) of $($R.Before) - $($R.After) remain" }
        'deleted-none' { "deleted NONE of $($R.Before)" }
        'none-there'   { 'there were none to delete' }
        default        { 'the count could not be read, so it is not known whether any were deleted' }
    }
    $how = "vssadmin exit $(if ($null -ne $R.VssadminExit) { $R.VssadminExit } else { 'unknown' })$(if ("$($R.Text)") { ": $((("$($R.Text)" -split "`n") | Select-Object -Last 1).Trim())" })"
    $w = @(@($R.Wmi) | Where-Object { "$_" })
    if ($w.Count -gt 0) { $how += "; then one by one: $($w -join '; ')" }
    "restore points: $what ($how)"
}

function Test-PrologueUsnJournalFile {
    # NTFS's change journal, as Defrag 259 names it (Aspire, 2026-09-22):
    # \$Extend\$UsnJrnl:$J:$DATA. Only that stream of that file counts.
    param([string]$LastUnmovable)
    [bool]("$LastUnmovable" -match '(?i)^\\?\$Extend\\\$UsnJrnl(:\$J)?(:\$DATA|::\$DATA)?$')
}

function Get-PrologueUsnJournalStep {
    # Judge (R18, decided 2026-09-22). The change journal is deleted only when
    # the number does not fit, Windows itself names the journal as the file in
    # the way, the job carries the person's consent, and it has not been done
    # since the last restart - Windows creates the journal again, so a restart
    # (the pagefile rung) can put it back in the way, and then once more is allowed.
    param([bool]$Fits, [string]$LastUnmovable, [bool]$Consented, [bool]$DoneThisBoot)
    if ($Fits -or -not (Test-PrologueUsnJournalFile -LastUnmovable $LastUnmovable)) { return 'none' }
    if ($DoneThisBoot) { return 'already-done' }
    if (-not $Consented) { return 'no-consent' }
    'delete'
}

function ConvertFrom-PrologueUsnQuery {
    # Pure (self-tested): `fsutil usn queryjournal C:` -> the journal's two sizes
    # in bytes, or Active = $false when there is no active journal to report.
    param([string[]]$Lines)
    $t = (@($Lines) -join "`n")
    $m = [regex]::Match($t, '(?im)^\s*Maximum Size\s*:\s*0x([0-9a-f]+)')
    $a = [regex]::Match($t, '(?im)^\s*Allocation Delta\s*:\s*0x([0-9a-f]+)')
    if ($m.Success -and $a.Success) { return [ordered]@{ Active = $true; MaxBytes = [Convert]::ToInt64($m.Groups[1].Value, 16); DeltaBytes = [Convert]::ToInt64($a.Groups[1].Value, 16) } }
    [ordered]@{ Active = $false; MaxBytes = $null; DeltaBytes = $null }
}

function Invoke-PrologueDeleteUsnJournal {
    # Windows' own tool, C: only; /n returns once the journal is gone. What is
    # lost is Windows' record of recent file changes, not a file. The journal's
    # sizes are read first, so the shrink and every stop create it again as it was.
    param($S)
    $before = ConvertFrom-PrologueUsnQuery @(& fsutil usn queryjournal C: 2>&1 | ForEach-Object { "$_" })
    $text = (& fsutil usn deletejournal /n C: 2>&1 | Out-String).Trim(); $code = $LASTEXITCODE
    $j = $S.Shrink.UsnJournal
    if (-not $j) { $j = [ordered]@{ Before = $before; Deletions = 0; LastRestarts = $null; Recreated = $false; ExitCode = $null; Text = $null; Utc = $null } }
    elseif (-not ($j.Before -and $j.Before.Active) -and $before.Active) { $j.Before = $before }
    if ($code -eq 0) { $j.Deletions = [int]$j.Deletions + 1; $j.Recreated = $false }
    $j.LastRestarts = [int]$S.Restarts; $j.ExitCode = $code; $j.Text = $text; $j.Utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    $S.Shrink.UsnJournal = $j
    $j
}

function Invoke-PrologueRecreateUsnJournal {
    # Puts the journal back with the sizes it had (a no-op resize if Windows
    # already created one). Returns what was done, in words.
    param($S)
    $b = $S.Shrink.UsnJournal.Before
    & fsutil usn createjournal "m=$([long]$b.MaxBytes)" "a=$([long]$b.DeltaBytes)" C: 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) { $S.Shrink.UsnJournal.Recreated = $true; return "change journal created again ($([math]::Round([long]$b.MaxBytes/1MB,1)) MB; its record of earlier changes is gone)" }
    "! the change journal could not be created again (fsutil exit $LASTEXITCODE; Windows creates it when a program next needs it)"
}

function Test-PrologueUpdatePending {
    # Pure (self-tested). Windows' own markers that an update waits for a
    # restart. None of them is a documented contract (RISKS R25): every check
    # records all three as read, and any one of them counts - the cautious reading.
    param($U)
    [bool]($U.CbsRebootPending -or $U.CbsRebootInProgress -or $U.WuRebootRequired)
}

function Get-PrologueUpdateFacts {
    # Live, read-only.
    $cbs = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing'
    [ordered]@{ Utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
                CbsRebootPending = [bool](Test-Path "$cbs\RebootPending"); CbsRebootInProgress = [bool](Test-Path "$cbs\RebootInProgress")
                WuRebootRequired = [bool](Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired') }
}

function Get-PrologueUpdateStep {
    # Pure (self-tested): clear | restart | stop. Before anything changes and
    # before the shrink, a waiting update gets a restart of ours (walk-away);
    # right before the arm it gets none - a restart there is the one R25 fears.
    param([bool]$Pending, [int]$Restarts, [string]$Where, [int]$Max = $UpdateMaxRestarts)
    if (-not $Pending) { return 'clear' }
    if ($Where -eq 'before-arm') { return 'stop' }
    if ($Restarts -lt $Max) { return 'restart' }
    'stop'
}

function Invoke-UpdateGate {
    # R25: read, record, and act on the step. Returns 'clear' or 'restart'; a stop does not return.
    param($S, [string]$State, [string]$Root, $Job, [string]$Where, [string]$ResumeTo)
    if (-not $S.Update) { $S.Update = [ordered]@{ Checks = @(); Restarts = 0; ResumeTo = $null } }
    $u = Get-PrologueUpdateFacts; $u.Where = $Where
    $pending = Test-PrologueUpdatePending $u; $u.Pending = $pending
    $S.Update.Checks = @($S.Update.Checks) + , $u
    $step = Get-PrologueUpdateStep -Pending $pending -Restarts ([int]$S.Update.Restarts) -Where $Where
    Write-Log "      Windows Update ($Where): $(if ($pending) { "an update is waiting for a restart (CBS RebootPending $($u.CbsRebootPending), RebootInProgress $($u.CbsRebootInProgress), WU RebootRequired $($u.WuRebootRequired))" } else { 'nothing is waiting for a restart' })"
    if ($step -eq 'clear') { Save-State $S $State; return 'clear' }
    if ($step -eq 'restart') {
        $S.Update.Restarts = [int]$S.Update.Restarts + 1; $S.Update.ResumeTo = $ResumeTo
        $S.Restarts = [int]$S.Restarts + 1; $S.Stage = 'update-restart'
        Save-State $S $State; Write-Record $S $Root
        try { Register-ResumeTask -State $State } catch { Stop-Prologue $S $State $Root $Job 'windows-update' "could not register the resume task ($_)" }
        Restart-Machine 'letting Windows finish installing an update before the conversion goes on'
        return 'restart'
    }
    Save-State $S $State
    $why = if ($Where -eq 'before-arm') { 'Windows began waiting to restart for an update after the shrink; the boot to the USB stick is not set up while it waits (RISKS R25)' }
           else { "Windows still has an update waiting for a restart after $($S.Update.Restarts) restart(s) to let it finish; the conversion does not interrupt it. Let Windows finish updating, then run the conversion again" }
    Stop-Prologue $S $State $Root $Job 'windows-update' $why
}

function Invoke-UpdateReturn {
    # Back from an update restart. Windows may restart again by itself while it
    # finishes (the Aspire, 2026-09-23: twice); the task resumes after each boot.
    param($S, [string]$State, [string]$Root, $Job)
    Write-Log '  back from the update restart; giving Windows time to finish updating'
    $deadline = (Get-Date).AddSeconds($UpdateWaitSeconds)
    while ((Get-Date) -lt $deadline -and (Test-PrologueUpdatePending (Get-PrologueUpdateFacts))) { Start-Sleep -Seconds 20 }
    Invoke-UpdateGate $S $State $Root $Job 'after-update-restart' "$($S.Update.ResumeTo)"
}

function Get-PrologueMemoryFilesBefore {
    # Read before either is touched (Aspire, 2026-09-20: a stop left both off
    # and nothing had recorded what they were). Read-only.
    $b = [ordered]@{ HibernateEnabled = $null; AutoPagefile = $null; PagefileSettings = @() }
    try { $b.HibernateEnabled = [bool]((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Power' -ErrorAction Stop).HibernateEnabled) } catch { }
    try { $b.AutoPagefile = [bool](Get-CimInstance Win32_ComputerSystem -ErrorAction Stop).AutomaticManagedPagefile } catch { }
    try { $b.PagefileSettings = @(Get-CimInstance Win32_PageFileSetting -ErrorAction Stop | ForEach-Object { [ordered]@{ Name = "$($_.Name)"; InitialSize = [int]$_.InitialSize; MaximumSize = [int]$_.MaximumSize } }) } catch { }
    $b
}

function Get-PrologueRestorePlan {
    # Judge: what a stop (or, -KeepHibernationOff, the return after an install,
    # where the kept volume must stay mountable) puts back. Only what this run
    # turned off, and only to what it was. With no record of what it was, a
    # Windows with no pagefile is the worse guess, so that one goes to automatic;
    # hibernation is left alone and the caller says so.
    param($Shrink, [switch]$KeepHibernationOff)
    $plan = @()
    if (-not $Shrink) { return $plan }
    # the change journal (R18, 2026-09-22) comes back on every path, the return after an install included
    $uj = $Shrink.UsnJournal
    if ($uj -and [int]$uj.Deletions -gt 0 -and -not $uj.Recreated -and $uj.Before -and $uj.Before.Active) { $plan += 'usn-journal' }
    $b = $Shrink.Before
    if ($Shrink.HibernationDisabled -and -not $KeepHibernationOff) {
        if ($b -and $b.HibernateEnabled -eq $true) { $plan += 'hibernation-on' }
        elseif (-not $b -or $null -eq $b.HibernateEnabled) { $plan += 'hibernation-unknown' }
    }
    if ($Shrink.PagefileDisabled) {
        if ($b -and $b.AutoPagefile -eq $false -and @($b.PagefileSettings).Count -gt 0) { $plan += 'pagefile-settings' }
        elseif ($b -and $b.AutoPagefile -eq $false) { }   # it had none before; it has none now
        else { $plan += 'pagefile-auto' }
    }
    $plan
}

function Invoke-PrologueRestoreMemoryFiles {
    # Act on the plan; returns what was done, in words, for the log and the record.
    param($S, [switch]$KeepHibernationOff)
    $done = @()
    foreach ($a in (Get-PrologueRestorePlan -Shrink $S.Shrink -KeepHibernationOff:$KeepHibernationOff)) {
        try {
            switch ($a) {
                'hibernation-on' { & powercfg /h on 2>&1 | Out-Null; if ($LASTEXITCODE -eq 0) { $S.Shrink.HibernationDisabled = $false; $done += 'hibernation back on' } else { $done += "! hibernation could not be turned back on (powercfg exit $LASTEXITCODE)" } }
                'usn-journal' { $done += (Invoke-PrologueRecreateUsnJournal -S $S) }
                'hibernation-unknown' { $done += '! hibernation left off: no record of how it was set (powercfg /h on turns it back on)' }
                'pagefile-auto' { Get-CimInstance Win32_ComputerSystem | Set-CimInstance -Property @{ AutomaticManagedPagefile = $true } -ErrorAction Stop; $S.Shrink.PagefileDisabled = $false; $done += 'pagefile back to automatic (returns at the next restart)' }
                'pagefile-settings' {
                    foreach ($p in @($S.Shrink.Before.PagefileSettings)) {
                        $i = New-CimInstance -ClassName Win32_PageFileSetting -Property @{ Name = "$($p.Name)" } -ErrorAction Stop
                        $i | Set-CimInstance -Property @{ InitialSize = [uint32]$p.InitialSize; MaximumSize = [uint32]$p.MaximumSize } -ErrorAction Stop
                    }
                    $S.Shrink.PagefileDisabled = $false; $done += "pagefile settings put back ($(@($S.Shrink.Before.PagefileSettings).Count); returns at the next restart)"
                }
            }
        } catch { $done += "! $a failed: $($_.Exception.Message)" }
    }
    $S.Shrink.Restored = @($done)
    $done
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

function Get-PrologueStageRefusal {
    # Pure (self-tested). Clean slate wipes Windows, and what is staged is the
    # person's only copy. On the Aspire (2026-09-22, R18) a job with no folder
    # map staged 0 files and the stop said "your files are staged". Nothing to
    # stage, or nothing staged, is a refusal - never a copy that reads complete.
    param([int]$Folders, $StagedFiles)
    if ($Folders -lt 1) { return 'the job lists none of your folders (none of the six was found on this computer), so there is nothing to copy to the stick; refusing to prepare a wipe with no copy of your files' }
    if ($null -ne $StagedFiles -and [int]$StagedFiles -lt 1) { return "no files were copied to the stick from the $Folders folder(s) the job lists; refusing to prepare a wipe with no copy of your files" }
    $null
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
    $script:StateDirAcl = @(Protect-StateDir -State $State)
    # SYSTEM at startup: runs before and without a sign-in (the walk-away half;
    # decided 2026-09-13). The task is registered by a run that already holds
    # UAC-consented elevation, only to survive its own restart, and every exit
    # path of this script removes it.
    $args = "-NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $State 'Invoke-Prologue.ps1')`" -Resume -StateDir `"$State`""
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $args
    $trigger = New-ScheduledTaskTrigger -AtStartup
    $principal = New-ScheduledTaskPrincipal -UserId 'NT AUTHORITY\SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Hours 4)
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    $t = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if (-not $t) { throw 'the resume task is not present after registration' }
    if ("$($t.Principal.UserId)" -notmatch '(?i)SYSTEM') { Unregister-ResumeTask | Out-Null; throw "the resume task registered as '$($t.Principal.UserId)', not SYSTEM; removed again" }
}

function Protect-StateDir {
    # The startup task runs this directory's copy of the script as SYSTEM, so
    # nothing but SYSTEM and Administrators may write here (ProgramData's
    # default ACL lets any user create files). Users keep read, for -Notify.
    param([string]$State)
    $acl = Get-Acl -LiteralPath $State
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($r in @($acl.Access)) { [void]$acl.RemoveAccessRule($r) }
    $inh = [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
    foreach ($who in @('NT AUTHORITY\SYSTEM', 'BUILTIN\Administrators')) {
        $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($who, 'FullControl', $inh, 'None', 'Allow')))
    }
    $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule('BUILTIN\Users', 'ReadAndExecute', $inh, 'None', 'Allow')))
    Set-Acl -LiteralPath $State -AclObject $acl
    $back = (Get-Acl -LiteralPath $State).Access
    $check = $back | Where-Object { $_.AccessControlType -eq 'Allow' -and "$($_.IdentityReference)" -match '(?i)Users|Everyone|Authenticated' -and "$($_.FileSystemRights)" -match '(?i)Write|Modify|FullControl|CreateFiles' }
    if ($check) { throw "the state directory still grants write access to $(($check | ForEach-Object { $_.IdentityReference }) -join ', '); refusing to register a SYSTEM task over it" }
    # the mitigation as evidence (RISKS R24): what the directory grants, read back
    @($back | ForEach-Object { "$($_.IdentityReference)=$($_.FileSystemRights)" })
}

function Get-LiveResumeContext {
    $explorer = [bool](Get-Process -Name explorer -ErrorAction SilentlyContinue)
    Get-ResumeContext -UserName ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -UserInteractive ([Environment]::UserInteractive) -SessionId ([Diagnostics.Process]::GetCurrentProcess().SessionId) -ExplorerRunning $explorer
}

function Wait-Stick {
    # At startup the stick may not be enumerated yet: poll by volume id.
    param($S, [int]$Seconds = $StickWaitSeconds)
    $t0 = Get-Date
    while ($true) {
        $r = Find-Stick $S
        if ($r) { return $r }
        if (((Get-Date) - $t0).TotalSeconds -ge $Seconds) { return $null }
        Start-Sleep -Seconds 5
    }
}

function Set-Notice {
    # Queue text for the person's next sign-in (HKLM RunOnce: runs once, as
    # whoever signs in, unelevated). Used whenever there is no screen to show.
    param([string]$State, [string]$Title, [string]$Text, [int]$Buttons = 64)
    try {
        $n = [ordered]@{ title = $Title; text = $Text; buttons = $Buttons; queued_utc = (Get-Date).ToUniversalTime().ToString('o') }
        [IO.File]::WriteAllText((Join-Path $State 'notice.json'), (ConvertTo-PrologueJson $n), (New-Object Text.UTF8Encoding($false)))
        $k = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce'
        Set-ItemProperty -Path $k -Name $NoticeRunOnceName -Value (New-NoticeCommand -ScriptPath (Join-Path $State 'Invoke-Prologue.ps1') -State $State)
        $true
    } catch { Write-Log "  ! could not queue the sign-in notice: $($_.Exception.Message)" 'Yellow'; $false }
}

function Show-Or-Queue {
    # A popup when there is a screen, a queued notice when there is not.
    param([string]$State, [string]$Title, [string]$Text, [int]$Seconds, [int]$Buttons = 64)
    $ctx = Get-LiveResumeContext
    if ($ctx.Unattended) { Set-Notice -State $State -Title $Title -Text $Text -Buttons $Buttons | Out-Null; return 'queued' }
    Show-Popup -Title $Title -Seconds $Seconds -Buttons $Buttons -Text $Text | Out-Null; 'shown'
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

function Get-PrologueStopSentence {
    # "Windows is as it was" only when that is true (Aspire, 2026-09-20).
    param($Restored)
    $r = @($Restored | Where-Object { $_ })
    $bad = @($r | Where-Object { $_ -like '!*' })
    if ($bad.Count -gt 0) { return "Windows was NOT fully put back: $((($bad | ForEach-Object { $_.TrimStart('!',' ') }) -join '; '))." }
    if (@($r | Where-Object { $_ -like '*next restart*' }).Count -gt 0) { return 'Windows is as it was, once it has restarted: the pagefile returns at the next restart.' }
    'Windows is as it was.'
}

function Remove-PrologueWifiSecrets {
    # The Wi-Fi passwords the job writer exported (artifacts\credentials\wifi)
    # leave the stick at every stop and every return to Windows, not only at
    # the end of an install (decided 2026-09-27, the owner): the stick never
    # carries them longer than one attempt. A re-run exports them again.
    # Returns how many files went.
    param([string]$Root)
    if (-not $Root) { return 0 }
    $dir = Join-Path $Root 'upgrade_\artifacts\credentials\wifi'
    if (-not (Test-Path -LiteralPath $dir)) { return 0 }
    $n = 0
    foreach ($f in @(Get-ChildItem -LiteralPath $dir -File -Recurse -Force)) { [IO.File]::WriteAllText($f.FullName, 'SCRUBBED by the prologue'); Remove-Item -LiteralPath $f.FullName -Force; $n++ }
    Remove-Item -LiteralPath $dir -Recurse -Force
    $n
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
    $restored = @(Invoke-PrologueRestoreMemoryFiles -S $S)
    foreach ($r in $restored) { if ($r -like '!*') { Write-Log "  $r" 'Yellow' } else { Write-Log "  $r" } }
    if ($Root) { Remove-Item (Join-Path $Root 'upgrade_\boot-install') -Force -ErrorAction SilentlyContinue }
    Unregister-ResumeTask | Out-Null
    if ($Root -and $Job) {
        $wp = $null
        try { $p = Get-Partition -DriveLetter C -ErrorAction Stop; $wp = [ordered]@{ number = [int]$p.PartitionNumber; guid = "$($p.Guid)"; size_bytes = [long]$p.Size } } catch { }
        $o = New-PrologueStoppedOutcome -Job $Job -S $S -StoppedAt $StoppedAt -Reason $Reason -WindowsPartition $wp
        [IO.File]::WriteAllText((Join-Path $Root 'upgrade_\outcome.json'), (ConvertTo-PrologueJson $o), (New-Object Text.UTF8Encoding($false)))
        $cred = Join-Path $Root 'upgrade_\artifacts\credentials'
        $wifiGone = Remove-PrologueWifiSecrets -Root $Root
        if (Test-Path $cred) { Get-ChildItem $cred -File -Force | ForEach-Object { [IO.File]::WriteAllText($_.FullName, 'SCRUBBED by the prologue at a stop'); Remove-Item $_.FullName -Force } }
        Write-Log "  outcome.json (stopped) written to the stick; credentials scrubbed ($wifiGone Wi-Fi password file(s) removed)"
    }
    Write-Record -S $S -Root $Root
    Copy-Item (Join-Path $State 'state.json') (Join-Path $State 'state-stopped.json') -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $State 'state.json') -Force -ErrorAction SilentlyContinue
    if (-not $Start) { Show-Or-Queue -State $State -Title 'upgrade_ - stopped' -Seconds 600 -Buttons 48 -Text ("The conversion stopped at: $StoppedAt`n`n$Reason`n`n$(Get-PrologueStopSentence -Restored $restored) Nothing was installed. The record is on the USB stick (upgrade_\outcome.json).") | Out-Null }
    exit 2
}

# =============================================================================
#  the stages
# =============================================================================

function Invoke-VolumeStage {
    # Step 1b. Returns 'continue' or 'restart'; stops on its own otherwise.
    param($S, [string]$State, [string]$Root, $Job, $F)
    if ($Job.intent.path -ne 'keep-windows') { Write-Log '  1b. disk check: not needed (the job does not keep Windows)'; return 'continue' }
    $trigger = Get-PrologueVolumeTrigger -Dirty "$($F.Dirty)" -RepairQueued ([bool]$F.RepairQueued)
    if ($trigger -eq 'none') { Write-Log '  1b. disk check: not needed (C: carries no dirty flag and Windows has no repair queued)'; return 'continue' }
    if ($trigger -eq 'unreadable') { Stop-Prologue $S $State $Root $Job 'volume-check' "the volume flag on C: could not be read (fsutil answered in a form this prologue does not understand)" }
    if (-not $Job.fork.volume_check_consented) { Stop-Prologue $S $State $Root $Job 'volume-check' 'C: needs a disk check and the job carries no consent to run one' }
    $S.VolumeCheck.Trigger = $trigger
    if ($trigger -eq 'dirty-flag') { Write-Log '  1b. C: carries the dirty flag - running the read-only online scan...' }
    else { Write-Log "  1b. C: carries no dirty flag, but Windows says a repair is queued ($($F.RepairQueuedWhy)) - its shrink answer cannot be trusted until that check has run (R18, 2026-09-17); running the read-only online scan..." }
    $scanStarted = (Get-Date).AddSeconds(-5)
    $scan = Invoke-PrologueScan; $S.VolumeCheck.Needed = $true
    $ev = Get-PrologueVolumeEvidence -Since $scanStarted; $S.VolumeCheck.Evidence = $ev
    $S.VolumeCheck.Scan = Format-PrologueScan -Cmdlet $scan -Ev $ev
    Write-Log "      online scan: $($S.VolumeCheck.Scan)"
    $health = Get-PrologueDiskHealth -DiskNumber $F.Disk.Number -UniqueId $F.Disk.UniqueId
    $de = Get-PrologueDiskEvents -DiskNumber $F.Disk.Number
    $S.VolumeCheck.DiskHealthAtCheck = $health; $S.VolumeCheck.BadBlocks = [int]$de.BadBlock
    Write-Log "      physical disk health: $health; disk error log (30 days): $($de.BadBlock) bad-block, $($de.Paging) paging, $($de.Reset) reset events"
    $gate = Test-PrologueDiskHealthGate -Health $health -BadBlocks ([int]$de.BadBlock) -Acknowledged ([bool]$S.Ack.DiskHealth)
    $S.VolumeCheck.Gate = $gate.Reason
    if (-not $gate.Pass) { Stop-Prologue $S $State $Root $Job 'volume-check' "C: is flagged for a disk check but the drive is not one to repair: $($gate.Reason). Copy your files off this computer and replace the drive. Nothing was changed." }
    if ($gate.Reason -like 'DATA LOSS ACCEPTED*') { Write-Log "      $($gate.Reason)" 'Red' } else { Write-Log "      disk gate: $($gate.Reason)" }
    $method = Get-PrologueRepairMethod -Scan $scan -LogVerdict "$($ev.LogVerdict)" -RepairNeeded ("$($ev.VolumeStatus)" -match '(?i)repair') -NtfsFullChkdsk (Test-PrologueNtfs98Fresh -Ntfs98 $ev.NtfsFullChkdsk -LastCheck $ev.LastCheck)
    if ($method -eq 'refuse') { Stop-Prologue $S $State $Root $Job 'volume-check' "the online scan did not give a usable answer ('$scan') and nothing in Windows' own log says what is wrong; refusing to repair on a guess" }
    Write-Log "      method: $method$(if ($method -eq 'chkdsk-f') { ' (Windows logged real corruption; the full check is the only rung that clears it - files on unreadable sectors come out truncated or missing)' })"
    $arm = Invoke-PrologueRepairArm -Method $method
    $S.VolumeCheck.Method = $method; $S.VolumeCheck.ArmText = $arm.Text; $S.VolumeCheck.Chkntfs = $arm.Chkntfs
    Write-Log ('      ' + ($arm.Text -replace "`n", "`n      "))
    if (-not $arm.Scheduled) { Stop-Prologue $S $State $Root $Job 'volume-check' "Windows did not accept the $method for the next restart (chkntfs says '$($arm.Chkntfs)')" }
    $S.VolumeCheck.ArmedUtc = (Get-Date).ToUniversalTime().ToString('o')
    $S.VolumeCheck.Restarts = [int]$S.VolumeCheck.Restarts + 1; $S.Restarts = [int]$S.Restarts + 1
    $S.Stage = 'check-armed'
    Save-State $S $State; Write-Record $S $Root
    try { Register-ResumeTask -State $State; $S.StateDirAcl = $script:StateDirAcl; Save-State $S $State } catch { Stop-Prologue $S $State $Root $Job 'volume-check' "could not register the resume task ($_); the scheduled check will still run at the next restart, but this conversion is not continuing" }
    Write-Log "      the disk check runs at the next restart; it may be slow - DO NOT interrupt it." 'Yellow'
    Restart-Machine 'running the disk check on C:'
    'restart'
}

function Invoke-CheckReturn {
    # Back from the disk-check restart. Returns 'continue' or 'restart'; stops otherwise.
    param($S, [string]$State, [string]$Root, $Job)
    $o = Get-PrologueCheckOutcome -SinceUtc $S.VolumeCheck.ArmedUtc
    $S.VolumeCheck.Wininit1001 = $o.Wininit1001; $S.VolumeCheck.Found000 = [bool]$o.Found000; $S.VolumeCheck.DirtyAfter = $o.Dirty
    $S.VolumeCheck.Ran = [bool]($o.Wininit1001) -or ("$($S.VolumeCheck.Trigger)" -ne 'repair-queued' -and $o.Dirty -eq 'clean')
    Write-Log "  1b. after the restart: Wininit 1001 $(if ($o.Wininit1001) { 'recorded' } else { 'NOT found' }); found.000 $($o.Found000); C: is now $($o.Dirty)"
    if ($o.Wininit1001) { Write-Log ('      ' + (($o.Wininit1001 -split "`n" | Select-Object -First 12) -join "`n      ")) 'DarkGray' }
    if ("$($S.VolumeCheck.Trigger)" -eq 'repair-queued') {
        # The bit was clean before the check, so "clean after" proves nothing
        # here: the check must have run (Wininit 1001) and Windows must no
        # longer report a repair. NTFS event 98 stays in the log after a
        # successful check, so only the volume's own status decides now.
        $armedLocal = [DateTime]::Parse("$($S.VolumeCheck.ArmedUtc)", $null, [Globalization.DateTimeStyles]::RoundtripKind).ToLocalTime()
        $after = Get-PrologueVolumeEvidence -Since $armedLocal
        $rqa = Test-PrologueRepairQueued -VolumeStatus "$($after.VolumeStatus)" -NtfsFullChkdsk $null
        Write-Log "      Windows after the check: volume '$($after.VolumeStatus)' ($($after.VolumeHealth)); check log $($after.LogVerdict)"
        if (-not $o.Wininit1001) { Stop-Prologue $S $State $Root $Job 'volume-check' "the $($S.VolumeCheck.Method) scheduled for Windows' queued repair did not run at the restart (no Wininit 1001); refusing to measure a volume Windows still wants to repair" }
        if ($rqa.Queued) { Stop-Prologue $S $State $Root $Job 'volume-check' "after $($S.VolumeCheck.Method) Windows still reports a repair queued ($($rqa.Why)); this prologue will not escalate further" }
        if ($o.Dirty -ne 'clean') { Stop-Prologue $S $State $Root $Job 'volume-check' "after $($S.VolumeCheck.Method) the volume flag on C: reads '$($o.Dirty)'" }
        return 'continue'
    }
    if ($o.Dirty -eq 'clean') { return 'continue' }
    if ($o.Dirty -ne 'dirty') { Stop-Prologue $S $State $Root $Job 'volume-check' 'after the disk check the volume flag could not be read' }
    if ($S.VolumeCheck.Method -eq 'chkdsk-f' -or [int]$S.VolumeCheck.Restarts -ge 2) { Stop-Prologue $S $State $Root $Job 'volume-check' "C: still carries the dirty flag after $($S.VolumeCheck.Method) ($($S.VolumeCheck.Restarts) restart(s)); Windows needs a disk check this prologue will not escalate further" }
    # the spot-fix left the flag: escalate only if the evidence now says real
    # errors - or if the person acknowledged the volume-health refusal (R23)
    $scanStarted = (Get-Date).AddSeconds(-5)
    $scan = Invoke-PrologueScan
    $ev = Get-PrologueVolumeEvidence -Since $scanStarted; $S.VolumeCheck.Evidence = $ev
    $rescan = Format-PrologueScan -Cmdlet $scan -Ev $ev
    $S.VolumeCheck.Scan = "$($S.VolumeCheck.Scan) | rescan: $rescan"
    Write-Log "      flag still set; rescan: $rescan"
    $m = Get-PrologueRepairMethod -Scan $scan -LogVerdict "$($ev.LogVerdict)" -RepairNeeded ("$($ev.VolumeStatus)" -match '(?i)repair') -NtfsFullChkdsk (Test-PrologueNtfs98Fresh -Ntfs98 $ev.NtfsFullChkdsk -LastCheck $ev.LastCheck)
    if ($m -ne 'chkdsk-f') {
        if ($S.Ack.VolumeHealth) { Write-Log '      DATA LOSS ACCEPTED: the flag survived the spot-fix and nothing names the cause; the person acknowledged the volume-health refusal, so the full check runs' 'Red' }
        else { Stop-Prologue $S $State $Root $Job 'volume-check' "C: still carries the dirty flag after the spot-fix and neither the online scan nor Windows' own log names an error ($rescan); refusing to run the full check on a guess" }
    }
    $health = Get-PrologueDiskHealth -DiskNumber ([int]$Job.identity.system_disk.number) -UniqueId "$($Job.identity.system_disk.unique_id)"
    $de = Get-PrologueDiskEvents -DiskNumber ([int]$Job.identity.system_disk.number)
    $S.VolumeCheck.DiskHealthAtCheck = $health; $S.VolumeCheck.BadBlocks = [int]$de.BadBlock
    $gate = Test-PrologueDiskHealthGate -Health $health -BadBlocks ([int]$de.BadBlock) -Acknowledged ([bool]$S.Ack.DiskHealth)
    $S.VolumeCheck.Gate = $gate.Reason
    if (-not $gate.Pass) { Stop-Prologue $S $State $Root $Job 'volume-check' "before the full check the drive is not one to repair: $($gate.Reason)" }
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
    $measureStart = (Get-Date).AddSeconds(-5)
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
        if (-not $fits) {
            # Windows names what pins the floor (Defrag 259); on the Aspire, 2026-09-20, it was hiberfil.sys on the last cluster
            $lu = Get-PrologueLastUnmovable -Since $measureStart
            if ($lu) { try { $S.Shrink.LastUnmovable = "$lu" } catch { }; Write-Log "      Windows names the last unmovable file: $lu" }
            # restore points in the way (R18, decided 2026-09-20): deleted with the job's consent, once, then one re-measure
            $rp = Get-PrologueRestorePointStep -Fits $fits -LastUnmovable "$lu" -Consented ([bool]$Job.fork.restore_points_consented) -AlreadyDone ([bool]$S.Shrink.RestorePoints)
            if ($rp -eq 'delete') {
                Write-Log "      that is System Restore's storage; the job consents - deleting Windows' restore points on C: (this cannot be undone)" 'Yellow'
                $S.Shrink.RestorePoints = Invoke-PrologueDeleteRestorePoints
                Write-Log "      $(Format-PrologueRestorePoints -R $S.Shrink.RestorePoints)"
                Save-State $S $State; Write-Record $S $Root
                return (Invoke-Continue $S $State $Root $Job)
            }
            elseif ($rp -eq 'no-consent') { Write-Log "      that is System Restore's storage; the job carries no consent to delete restore points - left alone" }
            elseif ($rp -eq 'already-done') { Write-Log "      deleting restore points was already tried in this run ($(Format-PrologueRestorePoints -R $S.Shrink.RestorePoints)) and their storage is still named - nothing more to try there" }
            # the change journal in the way (R18, decided 2026-09-22; the Aspire's fifth run): deleted with the job's consent, once per boot, then one re-measure
            $uj = Get-PrologueUsnJournalStep -Fits $fits -LastUnmovable "$lu" -Consented ([bool]$Job.fork.usn_journal_consented) -DoneThisBoot ([bool]($S.Shrink.UsnJournal -and $null -ne $S.Shrink.UsnJournal.LastRestarts -and [int]$S.Shrink.UsnJournal.LastRestarts -eq [int]$S.Restarts))
            if ($uj -eq 'delete') {
                Write-Log "      that is NTFS's change journal; the job consents - deleting it (Windows' record of recent file changes, not a file; it is created again afterwards)" 'Yellow'
                $r = Invoke-PrologueDeleteUsnJournal -S $S
                Write-Log "      change journal: $(if ($r.ExitCode -eq 0) { 'deleted' } else { "fsutil exit $($r.ExitCode)" }); was $(if ($r.Before.Active) { "$([math]::Round([long]$r.Before.MaxBytes/1MB,1)) MB max" } else { 'not active' }); fsutil: $((($r.Text -split "`n") | Select-Object -Last 1).Trim())"
                Save-State $S $State; Write-Record $S $Root
                return (Invoke-Continue $S $State $Root $Job)
            }
            elseif ($uj -eq 'no-consent') { Write-Log "      that is NTFS's change journal; the job carries no consent to delete it - left alone" }
            elseif ($uj -eq 'already-done') { Write-Log "      the change journal was already deleted since the last restart and is still named - nothing more to try there" }
        }
        if (-not $fits -and -not $S.Shrink.Mitigated -and $plan.TargetBytes -gt $plan.ShrinkableBytes) {
            # the immovable-file floor: pagefile off (effective after a restart), one restart, one re-measure
            Write-Log '      does not fit cold; disabling the pagefile and restarting once to re-measure'
            if (-not $S.Shrink.Before) { $S.Shrink.Before = Get-PrologueMemoryFilesBefore; Save-State $S $State }
            $S.Shrink.HibernationDisabled = Invoke-PrologueHibernationOff
            $S.Shrink.PagefileDisabled = Invoke-ProloguePagefileOff
            $S.Shrink.Mitigated = $true; $S.Restarts = [int]$S.Restarts + 1; $S.Stage = 'mitigated'
            Save-State $S $State; Write-Record $S $Root
            try { Register-ResumeTask -State $State } catch { Stop-Prologue $S $State $Root $Job 'shrink' "could not register the resume task ($_)" }
            Restart-Machine 'freeing space on C: for the measurement'
            return
        }
    }
    $fork = Get-PrologueFork -JobPath "$($Job.intent.path)" -Fits $fits -IfCannotKeep "$($Job.fork.if_cannot_keep)" -PathReason "$($Job.intent.path_reason)"
    $S.Shrink.ForkTaken = $fork
    Write-Log "      fork: $fork (job path $($Job.intent.path), if_cannot_keep $($Job.fork.if_cannot_keep))"
    if ($fork -eq 'stop') { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'shrink' "re-measured $(if ($null -ne $S.Shrink.RemeasuredGB) { "$($S.Shrink.RemeasuredGB) GB" } else { 'no figure' }) shrinkable; Linux needs $([math]::Round(($(if ($plan) { $plan.TargetBytes } else { $linuxMin * 1GB }))/1GB,1)) GB; you chose to stop rather than give up Windows$(if ($plan) { " ($($plan.Reason))" })" }

    if ((Invoke-UpdateGate $S $State $Root $Job 'before-shrink' 'continue') -eq 'restart') { return }

    if ($fork -eq 'keep-windows') {
        Write-Log '  2.  keep Windows: hibernation off, then the shrink'
        if (-not $S.Shrink.Before) { $S.Shrink.Before = Get-PrologueMemoryFilesBefore; Save-State $S $State }
        $S.Shrink.HibernationDisabled = Invoke-PrologueHibernationOff
        $S.Shrink.RequestedBytes = [long]$plan.RequestedBytes
        try {
            $r = Invoke-PrologueShrink -RequestedBytes ([long]$plan.RequestedBytes)
            $S.Shrink.SizeBefore = $r.SizeBefore; $S.Shrink.FreedBytes = $r.Freed
            Write-Log "      Resize-Partition: C: $([math]::Round($r.SizeBefore/1GB,1)) GB -> $([math]::Round($r.SizeAfter/1GB,1)) GB, freed $([math]::Round($r.Freed/1GB,1)) GB"
        } catch { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'shrink' "Resize-Partition refused: $($_.Exception.Message)" }
        if ([long]$S.Shrink.FreedBytes -lt [long]$plan.RequestedBytes) { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'shrink' "the shrink freed $($S.Shrink.FreedBytes) bytes, less than the $($plan.RequestedBytes) planned" }
        # the space is freed; the change journal goes back now, not at the end (R18, 2026-09-22)
        if (@(Get-PrologueRestorePlan -Shrink $S.Shrink -KeepHibernationOff) -contains 'usn-journal') { Write-Log "      $(Invoke-PrologueRecreateUsnJournal -S $S)" }
        Save-State $S $State; Write-Record $S $Root
    } else {
        Write-Log '  2.  clean slate: staging your files to the stick'
        $nFolders = @($Job.harvest.folders | Where-Object { $_.exists -and $_.path }).Count
        $why = Get-PrologueStageRefusal -Folders $nFolders
        if ($why) { Save-State $S $State; Stop-Prologue $S $State $Root $Job 'stage-files' $why }
        $st = Invoke-PrologueStage -Job $Job -Root $Root
        $S.Staged = $st; Save-State $S $State; Write-Record $S $Root
        if ($st.Error) { Stop-Prologue $S $State $Root $Job 'stage-files' $st.Error }
        if ($st.Failed -gt 0) { Stop-Prologue $S $State $Root $Job 'stage-files' "$($st.Failed) file(s) could not be staged to the stick" }
        $why = Get-PrologueStageRefusal -Folders $nFolders -StagedFiles $st.Files
        if ($why) { Stop-Prologue $S $State $Root $Job 'stage-files' $why }
        Write-Log "      staged $($st.Files) files, $([math]::Round($st.Bytes/1GB,2)) GB, checksums in $($st.Manifest)"
        Stop-Prologue $S $State $Root $Job 'confirm' "clean slate needs the live session's two-minute human check before the wipe, and that gate is not built in this version; the prologue will not arm an unattended wipe. $($st.Files) file(s), $([math]::Round($st.Bytes/1GB,2)) GB, are staged on the stick with checksums; Windows is untouched."
    }

    Invoke-Arm $S $State $Root $Job
}

function Invoke-EraseContinue {
    # The erase-and-install path (RISKS R27): nothing on the drives changes in
    # Windows. The installer erases them, after its countdown; until then this
    # Windows is untouched and boots as before.
    param($S, [string]$State, [string]$Root, $Job)
    $S.Shrink.ForkTaken = 'clean-slate'
    $names = @($Job.erase_consent.disks | ForEach-Object { "$($_.friendly_name) ($($_.role))" }) -join ' and '
    Write-Log "  2.  erase and install: nothing is changed here. In the installer a 2-minute countdown comes first; when it ends, $names are erased." 'Yellow'
    Save-State $S $State; Write-Record $S $Root
    Invoke-Arm $S $State $Root $Job
}

function Invoke-Arm {
    # 4. suspend BitLocker, arm the handoff, restart into the installer
    param($S, [string]$State, [string]$Root, $Job)
    $isErase = [bool]($Job.PSObject.Properties['erase_consent'] -and $Job.erase_consent)
    Invoke-UpdateGate $S $State $Root $Job 'before-arm' $null | Out-Null
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
    if ($isErase) { Write-Log '  In the installer a 2-minute countdown comes first. Press any key during it to cancel and come back to this Windows, untouched. When it ends, everything is erased and Fedora is installed.' 'Yellow' }
    else { Write-Log '  Windows is still here and still bootable; it stays that way until you reclaim it in Linux.' 'DarkGray' }
    Restart-Machine 'starting the installer from the USB stick'
    if (-not $Start) { $ctx = Get-LiveResumeContext; if (-not $ctx.Unattended) { Show-Popup -Title 'upgrade_' -Seconds 12 -Text "Restarting into the installer in 15 seconds. Leave the USB stick in." | Out-Null } }
}

function Invoke-Return {
    # Windows is back after the handoff. Classify, clean up, record, leave.
    param($S, [string]$State, [string]$Root)
    Write-Log '  return: Windows is back after the handoff'
    if ($Root) { Write-Log "  $(Remove-PrologueWifiSecrets -Root $Root) Wi-Fi password file(s) removed from the stick" }
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
    # the kept Windows stays a normal Windows: the pagefile comes back; hibernation stays off (the volume must be mountable from Linux)
    foreach ($r in @(Invoke-PrologueRestoreMemoryFiles -S $S -KeepHibernationOff)) { Write-Log "      $r" }
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
    # the erase path (R27): Windows is back, so nothing was erased - record why
    $eraseJob = $null
    if ($Root) { try { $j = Read-Job (Join-Path $Root 'upgrade_\job.json'); if ($j.PSObject.Properties['erase_consent'] -and $j.erase_consent -and "$($j.job_id)" -eq "$($S.JobId)") { $eraseJob = $j } } catch { } }
    if ($eraseJob) {
        $rd = Join-Path $Root 'upgrade_\report'
        $cd = try { Get-Content (Join-Path $rd 'countdown.json') -Raw -ErrorAction Stop | ConvertFrom-Json } catch { $null }
        $vf = try { Get-Content (Join-Path $rd 'verify.json') -Raw -ErrorAction Stop | ConvertFrom-Json } catch { $null }
        $er = Get-PrologueEraseReturn -Countdown $cd -Verify $vf
        $o = New-PrologueStoppedOutcome -Job $eraseJob -S $S -StoppedAt $er.StoppedAt -Reason $er.Reason -WindowsPartition $null
        [IO.File]::WriteAllText((Join-Path $Root 'upgrade_\outcome.json'), (ConvertTo-PrologueJson $o), (New-Object Text.UTF8Encoding($false)))
        Write-Log "  STOPPED at $($er.StoppedAt): $($er.Reason)" 'Yellow'
        Show-Or-Queue -State $State -Title 'upgrade_ - nothing was erased' -Seconds 300 -Buttons 64 -Text ("Windows is back and nothing was erased.`n`n$($er.Reason).`n`nThe record is on the USB stick (upgrade_\outcome.json).") | Out-Null
        return
    }
    Show-Or-Queue -State $State -Title 'upgrade_ - back in Windows' -Seconds 120 -Buttons 64 -Text ("The one-time boot entry has been removed (handoff: $result).`n`nIf the conversion completed, Linux is the first boot choice and Windows is in its menu. The record is on the USB stick.") | Out-Null
}

# =============================================================================
#  entry points
# =============================================================================

function Invoke-StartPhase {
    $state = Resolve-StateDir; New-Item -ItemType Directory -Path $state -Force | Out-Null
    if (Test-Path (Join-Path $state 'state.json')) { throw "a conversion is already in progress (state in $state). Restart to let it resume, or run -Abort." }
    $root = Get-DriveRoot $StickDrive
    if (-not (Test-Path $root)) { throw "stick $root not found" }
    $jobFile = if ($JobPath) { $JobPath } else { Join-Path $root 'upgrade_\job.json' }
    $job = Read-Job $jobFile
    $startRefusal = Get-PrologueEraseStartRefusal -Job $job -ConfirmWord $ConfirmWord -EraseConsent $EraseConsent
    if ($startRefusal) { throw $startRefusal }
    $isErase = [bool]($job.PSObject.Properties['erase_consent'] -and $job.erase_consent)
    $jobAck = if ($job.PSObject.Properties['risk_acknowledgement'] -and $job.risk_acknowledgement) { $job.risk_acknowledgement } else { $null }
    if ($jobAck) {
        if ("$($jobAck.statement)" -cne $RiskStatement) { throw 'the job carries a risk acknowledgement whose statement is not the one this prologue knows; refusing' }
        if ($AcknowledgeDataLoss -cne $RiskStatement) { throw "the job carries a data-loss acknowledgement but the statement was not typed for this run (-AcknowledgeDataLoss); refusing" }
    } elseif ($AcknowledgeDataLoss) { throw 'a data-loss statement was given but the job carries no acknowledgement; use the normal launcher' }
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
    if ($jobAck) {
        $ov = @($jobAck.overrides | ForEach-Object { "$_" })
        $S.Ack = [ordered]@{ Present = $true; DiskHealth = ($ov -contains 'disk-health'); VolumeHealth = ($ov -contains 'volume-health'); AcceptedUtc = "$($jobAck.accepted_utc)" }
        Write-Log "  DATA LOSS ACCEPTED (typed $($jobAck.accepted_utc)): this run lifts $($ov -join ', '). Files on this machine may be lost." 'Red'
    }
    $S.Facts = [ordered]@{ vendor = $F.Vendor; model = $F.Model; os = "$($F.OsCaption) $($F.OsBuild)"; secure_boot = $F.SecureBoot; disk = $F.Disk; health = $F.Health; dirty_at_start = $F.Dirty; bitlocker = $F.BitLocker; bitlocker_via = $F.BitLockerSource; hiberfil = $F.Hiberfil; pagefile = $F.Pagefile }
    Write-Log "  $($F.Vendor) $($F.Model)   $($F.OsCaption) $($F.OsBuild)   Secure Boot $($F.SecureBoot)   BitLocker $($F.BitLocker) (via $($F.BitLockerSource))   disk health $($F.Health)   C: $($F.Dirty)$(if ($F.RepairQueued) { " (repair queued: $($F.RepairQueuedWhy))" })$(if ($F.RepairStale) { " ($($F.RepairStale) - not a queued repair)" })" 'DarkGray'
    Save-State $S $state
    Write-Log '  1.  re-validating job.json against this machine...'
    $mm = @(Compare-PrologueJob -Job $job -F $F)
    if ($isErase) { $mm += @(Compare-PrologueEraseDisks -Job $job -F $F) }
    $S.Mismatches = @($mm)
    if (@($mm).Count -gt 0) { foreach ($x in $mm) { Write-Log "      ! $x" 'Yellow' }; Save-State $S $state; Stop-Prologue $S $state $root $job 'revalidate' ("job.json no longer matches this machine: " + ($mm -join '; ')) }
    Write-Log '      matches: disk identity, firmware, Secure Boot, stick, BitLocker, volume flag, disk health'
    Save-State $S $state; Write-Record $S $root
    if ($isErase) {
        Write-Log "  ERASE AND INSTALL (typed $($job.erase_consent.accepted_utc)): every drive named in the job is erased in the installer, after its countdown." 'Red'
        if ((Invoke-UpdateGate $S $state $root $job 'before-changes' 'erase') -eq 'restart') { return }
        Invoke-EraseContinue $S $state $root $job
        return
    }
    if ((Invoke-UpdateGate $S $state $root $job 'before-changes' 'volume') -eq 'restart') { return }
    if ((Invoke-VolumeStage $S $state $root $job $F) -eq 'restart') { return }
    Invoke-Continue $S $state $root $job
}

function Invoke-ResumePhase {
    $state = Resolve-StateDir
    $S = Read-State $state
    if (-not $S) { Write-Host "  nothing to resume (no state in $state)"; Unregister-ResumeTask | Out-Null; return }
    $script:LogFile = Join-Path $state 'prologue.log'
    $ctx = Get-LiveResumeContext
    $ctx.Utc = (Get-Date).ToUniversalTime().ToString('o'); $ctx.Stage = "$($S.Stage)"; $ctx.UptimeSeconds = [int]([Environment]::TickCount / 1000)
    $root = Wait-Stick $S
    $ctx.StickWaitSeconds = [int]((Get-Date) - [DateTime]::Parse($ctx.Utc, $null, [Globalization.DateTimeStyles]::RoundtripKind).ToLocalTime()).TotalSeconds
    if (-not $S.Contains('Resumes')) { $S.Resumes = @() }
    $S.Resumes = @($S.Resumes) + , $ctx
    Save-State $S $state
    if ($root) { $script:StickLog = Join-Path $root 'upgrade_\report\prologue.log'; New-Item -ItemType Directory -Path (Split-Path $script:StickLog -Parent) -Force | Out-Null }
    Write-Log ''; Write-Log "  upgrade_  prologue $PrologueVersion  -  RESUME (stage $($S.Stage), restart $($S.Restarts))" 'Cyan'
    Write-Log "  running as $($ctx.RunAs), session $($ctx.SessionId), interactive $($ctx.Interactive), explorer $($ctx.ExplorerRunning), uptime $($ctx.UptimeSeconds) s, stick after $($ctx.StickWaitSeconds) s -> $(if ($ctx.Unattended) { 'unattended' } else { 'attended' })" 'DarkGray'
    if (-not $root -and $S.Stage -eq 'probe-armed') { Invoke-ProbeReturn $S $state $null; return }
    if (-not $root) {
        # the task stays registered (startup, StartWhenAvailable): the next
        # restart with the stick in continues by itself
        Write-Log "  ! the USB stick is not present after $StickWaitSeconds s; leaving the task in place" 'Yellow'
        Show-Or-Queue -State $state -Title 'upgrade_' -Seconds 300 -Buttons 48 -Text "The USB stick is not plugged in. Plug it in and restart the computer - the conversion continues by itself." | Out-Null
        return
    }
    if ($S.Stage -eq 'armed') { Invoke-Return $S $state $root; return }
    if ($S.Stage -eq 'probe-armed') { Invoke-ProbeReturn $S $state $root; return }
    $job = Read-Job (Join-Path $root 'upgrade_\job.json')
    if ("$($job.job_id)" -ne "$($S.JobId)") { Stop-Prologue $S $state $root $job 'revalidate' "the job on the stick ($($job.job_id)) is not the one this conversion started with ($($S.JobId))" }
    switch ($S.Stage) {
        'check-armed' { if ((Invoke-CheckReturn $S $state $root $job) -eq 'restart') { return } }
        'mitigated' { Write-Log '  back from the pagefile restart' }
        'update-restart' {
            if ((Invoke-UpdateReturn $S $state $root $job) -eq 'restart') { return }
            if ("$($S.Update.ResumeTo)" -eq 'volume') { $F = Get-PrologueFacts -Root $root; if ((Invoke-VolumeStage $S $state $root $job $F) -eq 'restart') { return } }
        }
        default { throw "state is at stage '$($S.Stage)', which -Resume does not continue from" }
    }
    if ("$($S.Update.ResumeTo)" -eq 'erase') { Invoke-EraseContinue $S $state $root $job; return }
    Invoke-Continue $S $state $root $job
}

function Invoke-ProbeStart {
    $state = Resolve-StateDir; New-Item -ItemType Directory -Path $state -Force | Out-Null
    if (Test-Path (Join-Path $state 'state.json')) { throw "a conversion or probe is already in progress (state in $state). Restart to let it resume, or run -Abort." }
    $root = Get-DriveRoot $ProbeStickDrive
    if (-not (Test-Path $root)) { throw "stick $root not found" }
    $report = Join-Path $root 'upgrade_\report'; New-Item -ItemType Directory -Path $report -Force | Out-Null
    $script:LogFile = Join-Path $state 'prologue.log'; Remove-Item $script:LogFile -Force -ErrorAction SilentlyContinue
    $script:StickLog = Join-Path $report 'probe.log'
    Write-Log ''; Write-Log "  upgrade_  prologue $PrologueVersion  -  WALK-AWAY PROBE (read-only, one restart)" 'Cyan'
    $F = Get-PrologueFacts -Root $root
    if (-not $F.Stick) { throw "the stick's identity could not be read ($($F.StickError)); the probe needs it to find the stick again after the restart" }
    $S = New-PrologueState -JobId 'probe' -StickId "$($F.Stick.VolumeId)" -Root $root
    $S.Stage = 'probe-armed'
    $S.Facts = [ordered]@{ vendor = $F.Vendor; model = $F.Model; bios = $F.BiosVersion; os = "$($F.OsCaption) $($F.OsBuild)"; secure_boot = $F.SecureBoot; stick_bus = $F.Stick.Bus }
    Write-Log "  $($F.Vendor) $($F.Model)   BIOS $($F.BiosVersion)   $($F.OsCaption) $($F.OsBuild)   Secure Boot $($F.SecureBoot)   stick on $($F.Stick.Bus) as $root" 'DarkGray'
    Save-State $S $state
    Register-ResumeTask -State $state
    $S.StateDirAcl = $script:StateDirAcl; $S.Restarts = 1; Save-State $S $state
    Write-Log '  the SYSTEM startup task is registered; the state directory is locked; restarting.' 'Green'
    Write-Log '  Do NOT sign in when Windows comes back - leave it at the sign-in screen for two minutes. The record lands on the stick by itself.' 'Yellow'
    Restart-Machine 'the walk-away probe'
}

function Invoke-ProbeReturn {
    # Back from the probe's restart, as whatever ran the task. Record, clean up, leave.
    param($S, [string]$State, [string]$Root)
    Write-Log '  probe: back after the restart'
    $ctx = @($S.Resumes)[-1]
    $removed = Unregister-ResumeTask
    $result = Get-ProbeResult -Unattended ([bool]$ctx.Unattended) -StickFound ([bool]$Root) -TaskRemoved $removed
    $facts = if ($S.Contains('Facts') -and $S.Facts) { $S.Facts } else { [ordered]@{} }
    $notice = Show-Or-Queue -State $State -Title 'upgrade_ - walk-away probe' -Seconds 600 -Buttons 64 -Text ("The walk-away probe finished: $result.`n`nThe resume ran as $($ctx.RunAs) in session $($ctx.SessionId), $($ctx.UptimeSeconds) s after boot; the stick appeared after $($ctx.StickWaitSeconds) s.`n`nNothing on this computer was changed. The row is on the USB stick (upgrade_\walkaway-probe.csv).")
    $S.Stage = "probe-done:$result"
    $S.Return = [ordered]@{ ReturnedUtc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'); Result = $result; Notice = $notice; TaskRemoved = $removed }
    Save-State $S $State
    $row = @(((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')), $PrologueVersion, $facts.vendor, $facts.model, $facts.bios, $facts.os, $facts.secure_boot, $facts.stick_bus,
             $ctx.RunAs, $ctx.SessionId, $ctx.Interactive, $ctx.ExplorerRunning, $ctx.UptimeSeconds, $ctx.StickWaitSeconds, $notice, $removed, $result,
             "state stage=$($S.Stage); resume stage=$($ctx.Stage); resumed at $($ctx.Utc)")
    if ($Root) {
        $csv = Join-Path $Root 'upgrade_\walkaway-probe.csv'
        if (-not (Test-Path $csv)) { [IO.File]::WriteAllText($csv, (ConvertTo-ProbeCsvLine $ProbeCsvHeader) + "`n", (New-Object Text.UTF8Encoding($false))) }
        [IO.File]::AppendAllText($csv, (ConvertTo-ProbeCsvLine $row) + "`n", (New-Object Text.UTF8Encoding($false)))
        $rec = [ordered]@{ schema = 'walkaway-probe/1'; prologue_version = $PrologueVersion; result = $result; facts = $facts; resume = $ctx; notice = $notice; task_removed = $removed; state = $S }
        [IO.File]::WriteAllText((Join-Path $Root 'upgrade_\probe.json'), (ConvertTo-PrologueJson $rec), (New-Object Text.UTF8Encoding($false)))
        Write-Log "  probe: $result - row appended to upgrade_\walkaway-probe.csv, record in upgrade_\probe.json" 'Green'
    }
    Copy-Item (Join-Path $State 'state.json') (Join-Path $State 'state-probe.json') -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $State 'state.json') -Force -ErrorAction SilentlyContinue
}

function Invoke-NotifyPhase {
    # RunOnce at sign-in, as the person: show what the unattended phase queued.
    $state = Resolve-StateDir
    $p = Join-Path $state 'notice.json'
    if (-not (Test-Path $p)) { return }
    $n = Get-Content $p -Raw | ConvertFrom-Json
    Show-Popup -Title "$($n.title)" -Seconds 600 -Buttons ([int]$n.buttons) -Text "$($n.text)" | Out-Null
    Remove-Item $p -Force -ErrorAction SilentlyContinue
}

function Invoke-AbortPhase {
    $state = Resolve-StateDir
    $S = Read-State $state
    Unregister-ResumeTask | Out-Null
    Remove-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce' -Name $NoticeRunOnceName -ErrorAction SilentlyContinue
    if (-not $S) { Write-Host '  nothing in progress'; return }
    if ($S.Handoff.Armed -and $S.Handoff.EntryGuid) { & bcdedit /deletevalue '{fwbootmgr}' bootsequence 2>&1 | Out-Null; & bcdedit /delete $S.Handoff.EntryGuid 2>&1 | Out-Null; Write-Host '  removed the one-shot boot entry' }
    if ($S.BitLocker.Suspended) { & manage-bde -protectors -enable C: 2>&1 | Out-Null; Write-Host '  BitLocker protection re-enabled' }
    foreach ($r in @(Invoke-PrologueRestoreMemoryFiles -S $S)) { Write-Host "  $r" }
    $root = Find-Stick $S
    if ($root) { Remove-Item (Join-Path $root 'upgrade_\boot-install') -Force -ErrorAction SilentlyContinue; Write-Host "  $(Remove-PrologueWifiSecrets -Root $root) Wi-Fi password file(s) removed from the stick" }
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
        storage = [pscustomobject]@{ linux_min_gb = 25; volume_health = [pscustomobject]@{ dirty = 'dirty'; repair_queued = $false }; physical_disk = [pscustomobject]@{ health_status = 'Healthy' } }
        harvest = [pscustomobject]@{ bitlocker = [pscustomobject]@{ status = 'on' }; folders = @() }
        stick = [pscustomobject]@{ unique_id = 'USBSTOR\X'; size_bytes = 8053063680 }
    }
    $facts = [ordered]@{ BiosSerial = 'S1'; Uuid = 'U1'; Firmware = 'UEFI'; SecureBoot = 'on'; OsBuild = 19045
                         Disk = [ordered]@{ Number = 0; UniqueId = 'eui.1'; Serial = 'SER'; Size = 250059350016 }
                         Stick = [ordered]@{ UniqueId = 'USBSTOR\X'; Size = 8053063680 }; StickError = $null
                         BitLocker = 'on'; Dirty = 'dirty'; Health = 'Healthy'; RepairQueued = $false; RepairQueuedWhy = '' }
    function With { param($h, [string]$k, $v) $c = [ordered]@{}; foreach ($e in $h.GetEnumerator()) { $c[$e.Key] = $e.Value }; $c[$k] = $v; $c }
    function WithDisk { param($h, [string]$k, $v) $c = With $h 'Disk' (With $h.Disk $k $v); $c }
    function Sh { param($hib, $pf, $before) [ordered]@{ HibernationDisabled = $hib; PagefileDisabled = $pf; Before = $before } }
    $bAuto = [ordered]@{ HibernateEnabled = $true; AutoPagefile = $true; PagefileSettings = @() }
    $bCustom = [ordered]@{ HibernateEnabled = $false; AutoPagefile = $false; PagefileSettings = @([ordered]@{ Name = 'C:\pagefile.sys'; InitialSize = 2048; MaximumSize = 4096 }) }
    $bNone = [ordered]@{ HibernateEnabled = $true; AutoPagefile = $false; PagefileSettings = @() }
    $vss = '\System Volume Information\{1d038256-b528-11f1-af8c-00f48d7649b6}{3808876b-c176-4e48-b7ae-04046e6cc752}'
    $usn = '\$Extend\$UsnJrnl:$J:$DATA'
    $cases = @(
        # a Windows update waiting for a restart (RISKS R25, the Aspire's sixth run: our restart let one finish, and it restarted twice more)
        @{ Name = 'update pending: any one marker counts; none, or a record with none of them, is clear'
           Run = { "$(Test-PrologueUpdatePending @{ CbsRebootPending = $true; CbsRebootInProgress = $false; WuRebootRequired = $false })/$(Test-PrologueUpdatePending @{ CbsRebootPending = $false; CbsRebootInProgress = $true; WuRebootRequired = $false })/$(Test-PrologueUpdatePending @{ CbsRebootPending = $false; CbsRebootInProgress = $false; WuRebootRequired = $true })/$(Test-PrologueUpdatePending @{ CbsRebootPending = $false; CbsRebootInProgress = $false; WuRebootRequired = $false })/$(Test-PrologueUpdatePending @{})" }; Expect = 'True/True/True/False/False' }
        @{ Name = 'update step: nothing pending is clear, wherever it is asked'; Run = { "$(Get-PrologueUpdateStep -Pending $false -Restarts 0 -Where 'before-changes')/$(Get-PrologueUpdateStep -Pending $false -Restarts 5 -Where 'before-arm')" }; Expect = 'clear/clear' }
        @{ Name = 'update step: pending before anything changes or before the shrink takes a restart of ours, up to the limit, then stops'
           Run = { "$(Get-PrologueUpdateStep -Pending $true -Restarts 0 -Where 'before-changes')/$(Get-PrologueUpdateStep -Pending $true -Restarts 2 -Where 'before-shrink')/$(Get-PrologueUpdateStep -Pending $true -Restarts 3 -Where 'after-update-restart')" }; Expect = 'restart/restart/stop' }
        @{ Name = 'update step (R25): pending right before the arm never restarts - it stops, whatever the count'; Run = { Get-PrologueUpdateStep -Pending $true -Restarts 0 -Where 'before-arm' }; Expect = 'stop' }
        @{ Name = 'update record: the outcome block counts checks, what was seen and the restarts; a state from before 0.9.0 has none'
           Run = { $st = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $none = $null -eq (New-PrologueBlock $st).windows_update
                   $st.Update.Checks = @([ordered]@{ Where = 'before-changes'; Pending = $true }, [ordered]@{ Where = 'after-update-restart'; Pending = $false }); $st.Update.Restarts = 1
                   $w = (New-PrologueBlock $st).windows_update; "$none/$($w.checks)/$($w.pending_seen)/$($w.restarts)" }; Expect = 'True/2/True/1' }
        # what a restore-point attempt did, from the counts (R18, the Aspire's sixth run: 2 before, 2 after, then "already deleted")
        @{ Name = 'restore-point verdict: all, some, none, none there, unreadable'; Run = { "$(Get-PrologueRestorePointVerdict 2 0)/$(Get-PrologueRestorePointVerdict 3 1)/$(Get-PrologueRestorePointVerdict 2 2)/$(Get-PrologueRestorePointVerdict 0 0)/$(Get-PrologueRestorePointVerdict $null 2)/$(Get-PrologueRestorePointVerdict 2 $null)" }; Expect = 'deleted-all/deleted-some/deleted-none/none-there/unknown/unknown' }
        @{ Name = 'restore-point line: the Aspire run 6 record (0.8.0 kept no exit code) says NONE, never "deleted"'
           Run = { Format-PrologueRestorePoints -R ([ordered]@{ Before = 2; After = 2; Deleted = 0; Text = ''; Utc = '2026-09-23T20:30:18Z'; Verdict = (Get-PrologueRestorePointVerdict 2 2) }) }; Expect = 'restore points: deleted NONE of 2 (vssadmin exit unknown)' }
        @{ Name = 'restore-point line: vssadmin exit and each WMI answer are carried'
           Run = { Format-PrologueRestorePoints -R ([ordered]@{ Before = 2; After = 1; Deleted = 1; Verdict = 'deleted-some'; VssadminExit = 2; Text = "line one`nError: Snapshots were found, but they were outside of your allowed context."; Wmi = @('{A}: removed', '{B}: Access denied') }) }
           Expect = 'restore points: deleted 1 of 2 - 1 remain (vssadmin exit 2: Error: Snapshots were found, but they were outside of your allowed context.; then one by one: {A}: removed; {B}: Access denied)' }
        @{ Name = 'restore-point line: all deleted by vssadmin alone'; Run = { Format-PrologueRestorePoints -R ([ordered]@{ Before = 3; After = 0; Deleted = 3; Verdict = 'deleted-all'; VssadminExit = 0; Text = ''; Wmi = @() }) }; Expect = 'restore points: deleted all 3 (vssadmin exit 0)' }
        @{ Name = 'restore-point line: an unreadable count never claims a deletion'; Run = { [bool]((Format-PrologueRestorePoints -R ([ordered]@{ Before = $null; After = $null; Verdict = 'unknown'; VssadminExit = 0 })) -match 'not known whether any were deleted') }; Expect = $true }
        # the change journal in the way of the shrink (R18, decided 2026-09-22; the Aspire's real Defrag 259 name and queryjournal text)
        @{ Name = 'change journal (R18): the Aspire''s name is recognised; \$Extend\$ObjId, \$MFT, hiberfil and a person''s file named UsnJrnl are not'
           Run = { "$(Test-PrologueUsnJournalFile $usn)/$(Test-PrologueUsnJournalFile '\$Extend\$UsnJrnl')/$(Test-PrologueUsnJournalFile '\$Extend\$ObjId:$O:$INDEX_ALLOCATION')/$(Test-PrologueUsnJournalFile '\$Mft')/$(Test-PrologueUsnJournalFile '\hiberfil.sys')/$(Test-PrologueUsnJournalFile '\Users\a\$UsnJrnl')/$(Test-PrologueUsnJournalFile '')" }; Expect = 'True/True/False/False/False/False/False' }
        @{ Name = 'change journal (R18): named, consented, not done this boot -> delete'; Run = { Get-PrologueUsnJournalStep -Fits $false -LastUnmovable $usn -Consented $true -DoneThisBoot $false }; Expect = 'delete' }
        @{ Name = 'change journal (R18): no consent in the job (a 0.8.0-or-older job) -> never deleted'; Run = { Get-PrologueUsnJournalStep -Fits $false -LastUnmovable $usn -Consented ([bool]$job.fork.usn_journal_consented) -DoneThisBoot $false }; Expect = 'no-consent' }
        @{ Name = 'change journal (R18): not twice in one boot'; Run = { Get-PrologueUsnJournalStep -Fits $false -LastUnmovable $usn -Consented $true -DoneThisBoot $true }; Expect = 'already-done' }
        @{ Name = 'change journal (R18): a number that fits, or another file in the way, deletes nothing'; Run = { "$(Get-PrologueUsnJournalStep -Fits $true -LastUnmovable $usn -Consented $true -DoneThisBoot $false)/$(Get-PrologueUsnJournalStep -Fits $false -LastUnmovable $vss -Consented $true -DoneThisBoot $false)" }; Expect = 'none/none' }
        @{ Name = 'change journal: the Aspire''s queryjournal text parses to 32 MB max, 8 MB delta'
           Run = { $q = ConvertFrom-PrologueUsnQuery @('Usn Journal ID   : 0x01d47af5555868d0', 'First Usn        : 0x00000007db660000', 'Maximum Size     : 0x0000000002000000 (32.0 MB)', 'Allocation Delta : 0x0000000000800000 ( 8.0 MB)', 'Minimum record version supported : 2'); "$($q.Active):$($q.MaxBytes):$($q.DeltaBytes)" }; Expect = 'True:33554432:8388608' }
        @{ Name = 'change journal: no active journal parses to Active=False'; Run = { (ConvertFrom-PrologueUsnQuery @('Error:  The volume change journal is not active.')).Active }; Expect = $false }
        @{ Name = 'restore (R18): a deleted journal that was active comes back, on a stop and on the return; once created, not again'
           Run = { $d = Sh $false $false $bAuto; $d.UsnJournal = [ordered]@{ Before = [ordered]@{ Active = $true; MaxBytes = 33554432; DeltaBytes = 8388608 }; Deletions = 1; Recreated = $false }
                   $a = (Get-PrologueRestorePlan -Shrink $d) -join ','; $b = (Get-PrologueRestorePlan -Shrink $d -KeepHibernationOff) -join ','; $d.UsnJournal.Recreated = $true; "$a/$b/$(@(Get-PrologueRestorePlan -Shrink $d).Count)" }; Expect = 'usn-journal/usn-journal/0' }
        @{ Name = 'restore (R18): a journal that was not active, or a delete that failed, is not created'
           Run = { $d = Sh $false $false $bAuto; $d.UsnJournal = [ordered]@{ Before = [ordered]@{ Active = $false }; Deletions = 1; Recreated = $false }; $e = Sh $false $false $bAuto; $e.UsnJournal = [ordered]@{ Before = [ordered]@{ Active = $true; MaxBytes = 1; DeltaBytes = 1 }; Deletions = 0; Recreated = $false }; "$(@(Get-PrologueRestorePlan -Shrink $d).Count)/$(@(Get-PrologueRestorePlan -Shrink $e).Count)" }; Expect = '0/0' }
        # restore points in the way of the shrink (R18, decided 2026-09-20; the Aspire's real file name)
        @{ Name = 'restore points (R18): the Aspire''s shadow-storage file is recognised; other System Volume Information files, hiberfil and a person''s file are not'
           Run = { "$(Test-PrologueShadowStorageFile $vss)/$(Test-PrologueShadowStorageFile '\System Volume Information\tracking.log')/$(Test-PrologueShadowStorageFile '\hiberfil.sys')/$(Test-PrologueShadowStorageFile '\Users\a\{3808876b-c176-4e48-b7ae-04046e6cc752}')" }; Expect = 'True/False/False/False' }
        @{ Name = 'restore points (R18): named, consented, not yet done -> delete'; Run = { Get-PrologueRestorePointStep -Fits $false -LastUnmovable $vss -Consented $true -AlreadyDone $false }; Expect = 'delete' }
        @{ Name = 'restore points (R18): no consent in the job -> never deleted'; Run = { Get-PrologueRestorePointStep -Fits $false -LastUnmovable $vss -Consented $false -AlreadyDone $false }; Expect = 'no-consent' }
        @{ Name = 'restore points (R18): never twice'; Run = { Get-PrologueRestorePointStep -Fits $false -LastUnmovable $vss -Consented $true -AlreadyDone $true }; Expect = 'already-done' }
        @{ Name = 'restore points (R18): a number that fits deletes nothing, consent or not'; Run = { Get-PrologueRestorePointStep -Fits $true -LastUnmovable $vss -Consented $true -AlreadyDone $false }; Expect = 'none' }
        @{ Name = 'restore points (R18): another file in the way deletes nothing'; Run = { Get-PrologueRestorePointStep -Fits $false -LastUnmovable '\$Mft::$DATA' -Consented $true -AlreadyDone $false }; Expect = 'none' }
        @{ Name = 'restore points (R18): a job written before the consent existed reads as no consent'; Run = { Get-PrologueRestorePointStep -Fits $false -LastUnmovable $vss -Consented ([bool]$job.fork.restore_points_consented) -AlreadyDone $false }; Expect = 'no-consent' }
        # a stop puts back what the mitigation turned off (R18, Aspire 2026-09-20: it did not, and said it had)
        @{ Name = 'restore (R18): both off, both were on -> both come back'; Run = { (Get-PrologueRestorePlan -Shrink (Sh $true $true $bAuto)) -join ',' }; Expect = 'hibernation-on,pagefile-auto' }
        @{ Name = 'restore (R18): nothing was turned off -> nothing to do'; Run = { @(Get-PrologueRestorePlan -Shrink (Sh $false $false $bAuto)).Count }; Expect = 0 }
        @{ Name = 'restore (R18): hibernation was already off -> it stays off; custom pagefile settings go back as they were'; Run = { (Get-PrologueRestorePlan -Shrink (Sh $true $true $bCustom)) -join ',' }; Expect = 'pagefile-settings' }
        @{ Name = 'restore (R18): a machine that had no pagefile is not given one'; Run = { (Get-PrologueRestorePlan -Shrink (Sh $true $true $bNone)) -join ',' }; Expect = 'hibernation-on' }
        @{ Name = 'restore (R18): no record of before (a 0.5.0 state) -> pagefile to automatic, hibernation named as left off'; Run = { (Get-PrologueRestorePlan -Shrink (Sh $true $true $null)) -join ',' }; Expect = 'hibernation-unknown,pagefile-auto' }
        @{ Name = 'restore (R18): the return after an install keeps hibernation off and brings the pagefile back'; Run = { (Get-PrologueRestorePlan -Shrink (Sh $true $true $bAuto) -KeepHibernationOff) -join ',' }; Expect = 'pagefile-auto' }
        @{ Name = 'restore (R18): one custom pagefile setting survives the state round trip across the restart'; Run = { $rt = ConvertTo-PrologueHashtable ((ConvertTo-PrologueJson (Sh $true $true $bCustom)) | ConvertFrom-Json); (Get-PrologueRestorePlan -Shrink $rt) -join ',' }; Expect = 'pagefile-settings' }
        @{ Name = 'stop sentence (R18): nothing changed -> as it was'; Run = { Get-PrologueStopSentence -Restored @() }; Expect = 'Windows is as it was.' }
        @{ Name = 'stop sentence (R18): a pagefile pending a restart is said'; Run = { [bool]((Get-PrologueStopSentence -Restored @('hibernation back on', 'pagefile back to automatic (returns at the next restart)')) -match 'once it has restarted') }; Expect = $true }
        @{ Name = 'stop sentence (R18): a failed restore never reads "as it was"'; Run = { $t = Get-PrologueStopSentence -Restored @('! hibernation could not be turned back on (powercfg exit 1)'); [bool]($t -match 'NOT fully put back' -and $t -notmatch 'as it was') }; Expect = $true }
        # step 1: re-validation
        @{ Name = 'revalidate: an identical machine has no mismatches'; Run = { @(Compare-PrologueJob -Job $job -F $facts).Count }; Expect = 0 }
        @{ Name = 'revalidate: a different system disk id is a mismatch'; Run = { @(Compare-PrologueJob -Job $job -F (WithDisk $facts 'UniqueId' 'eui.2')) -join ';' }; Expect = "system_disk.unique_id: job says 'eui.1', machine says 'eui.2'" }
        @{ Name = 'revalidate: a different disk size is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (WithDisk $facts 'Size' 1)) -match 'size_bytes') }; Expect = $true }
        @{ Name = 'revalidate: a different stick is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'Stick' ([ordered]@{ UniqueId = 'OTHER'; Size = 1 }))) -match 'stick') }; Expect = $true }
        @{ Name = 'revalidate: an unreadable stick is a mismatch, not a pass'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With (With $facts 'Stick' $null) 'StickError' 'gone')) -match 'stick') }; Expect = $true }
        @{ Name = 'revalidate: BitLocker turned off since evaluate is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'BitLocker' 'off')) -match 'bitlocker') }; Expect = $true }
        @{ Name = 'revalidate (R18): a repair queued since evaluate is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'RepairQueued' $true)) -match 'repair_queued') }; Expect = $true }
        # step 1b's trigger (R18, 2026-09-17): the Aspire's clean bit with a queued repair
        @{ Name = 'trigger (R18): the dirty flag runs the check'; Run = { Get-PrologueVolumeTrigger -Dirty 'dirty' -RepairQueued $false }; Expect = 'dirty-flag' }
        @{ Name = 'trigger (R18): a clean bit with a queued repair runs it too'; Run = { Get-PrologueVolumeTrigger -Dirty 'clean' -RepairQueued $true }; Expect = 'repair-queued' }
        @{ Name = 'trigger (R18): clean with nothing queued skips it'; Run = { Get-PrologueVolumeTrigger -Dirty 'clean' -RepairQueued $false }; Expect = 'none' }
        @{ Name = 'trigger (R18): an unreadable bit never skips, queued or not'; Run = { "$(Get-PrologueVolumeTrigger -Dirty 'unknown' -RepairQueued $false)/$(Get-PrologueVolumeTrigger -Dirty 'unknown' -RepairQueued $true)" }; Expect = 'unreadable/unreadable' }
        @{ Name = 'repair queued (R18): Full Repair Needed / NTFS 98 alone / neither'; Run = { "$((Test-PrologueRepairQueued -VolumeStatus 'Full Repair Needed' -NtfsFullChkdsk $null).Queued)/$((Test-PrologueRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk '2026-09-13T15:16:48Z').Queued)/$((Test-PrologueRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk $null).Queued)" }; Expect = 'True/True/False' }
        @{ Name = 'repair queued (R18): the reason names the source'; Run = { (Test-PrologueRepairQueued -VolumeStatus 'Full Repair Needed' -NtfsFullChkdsk $null).Why }; Expect = "Get-Volume reports 'Full Repair Needed'" }
        @{ Name = 'repair queued (R18, 2026-09-20): an event 98 older than the last completed check is history'; Run = { $t = Test-PrologueRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk '2026-09-13T19:16:48.1962215Z' -LastCheck '2026-09-15T22:02:32.0000000Z'; "$($t.Queued):$([bool]$t.Stale)" }; Expect = 'False:True' }
        @{ Name = 'repair queued (R18, 2026-09-20): an event 98 after the last check still counts; with no check on record it counts'; Run = { "$((Test-PrologueRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk '2026-09-16T09:00:00.0000000Z' -LastCheck '2026-09-15T22:02:32.0000000Z').Queued)/$((Test-PrologueRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk '2026-09-13T19:16:48.1962215Z' -LastCheck $null).Queued)" }; Expect = 'True/True' }
        @{ Name = 'repair queued (R18, 2026-09-20): the volume''s own status counts whatever the history says'; Run = { (Test-PrologueRepairQueued -VolumeStatus 'Full Repair Needed' -NtfsFullChkdsk '2026-09-13T19:16:48.1962215Z' -LastCheck '2026-09-15T22:02:32.0000000Z').Queued }; Expect = $true }
        @{ Name = 'ntfs98 fresh: DateTimes and round-trip strings compare alike; none is not fresh'; Run = { "$(Test-PrologueNtfs98Fresh -Ntfs98 ([DateTime]'2026-09-16') -LastCheck '2026-09-15T22:02:32.0000000Z')/$(Test-PrologueNtfs98Fresh -Ntfs98 $null -LastCheck $null)" }; Expect = 'True/False' }
        @{ Name = 'the Aspire on 2026-09-20: clean bit, volume OK, stale 98 -> step 1b does not run'; Run = { $t = Test-PrologueRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk '2026-09-13T19:16:48.1962215Z' -LastCheck '2026-09-15T22:01:15.0000000Z'; Get-PrologueVolumeTrigger -Dirty 'clean' -RepairQueued ([bool]$t.Queued) }; Expect = 'none' }
        @{ Name = 'defrag 259: the Aspire''s real text parses to \hiberfil.sys; other text to null'; Run = { "$(ConvertFrom-PrologueDefrag259 -Message " - The last unmovable file appears to be: \hiberfil.sys::`$DATA`n - The last cluster of the file is: 0x3b562fe")/$($null -eq (ConvertFrom-PrologueDefrag259 -Message 'shrink estimation completed'))" }; Expect = '\hiberfil.sys/True' }
        @{ Name = 'method (R18): a queued repair picks chkdsk-f even when the cmdlet says NoErrorsFound'; Run = { Get-PrologueRepairMethod -Scan 'NoErrorsFound' -LogVerdict 'unknown' -RepairNeeded $true -NtfsFullChkdsk $false }; Expect = 'chkdsk-f' }
        @{ Name = 'revalidate: the flag cleared since evaluate is a change, and a change stops'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'Dirty' 'clean')) -match 'volume_health') }; Expect = $true }
        @{ Name = 'revalidate: a disk health that changed is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'Health' 'Warning')) -match 'health_status') }; Expect = $true }
        @{ Name = 'revalidate: Secure Boot toggled is a mismatch'; Run = { [bool](@(Compare-PrologueJob -Job $job -F (With $facts 'SecureBoot' 'off')) -match 'secure_boot') }; Expect = $true }
        # step 1b: guardrails
        @{ Name = 'rung: NoErrorsFound with no other evidence chooses the spot-fix'; Run = { Get-PrologueRepairMethod 'NoErrorsFound' }; Expect = 'spot-fix' }
        @{ Name = 'rung: ErrorsFound / ErrorsNotFixed choose chkdsk /f'; Run = { "$(Get-PrologueRepairMethod 'ErrorsFound')/$(Get-PrologueRepairMethod ' ErrorsNotFixed ')" }; Expect = 'chkdsk-f/chkdsk-f' }
        @{ Name = "rung: the Chkdsk log's found-problems outranks the cmdlet's NoErrorsFound (the Aspire)"; Run = { Get-PrologueRepairMethod -Scan 'NoErrorsFound' -LogVerdict 'found-problems' }; Expect = 'chkdsk-f' }
        @{ Name = 'rung: Get-Volume "Full Repair Needed" chooses chkdsk /f'; Run = { Get-PrologueRepairMethod -Scan 'NoErrorsFound' -RepairNeeded $true }; Expect = 'chkdsk-f' }
        @{ Name = 'rung: NTFS event 98 chooses chkdsk /f'; Run = { Get-PrologueRepairMethod -Scan 'NoErrorsFound' -NtfsFullChkdsk $true }; Expect = 'chkdsk-f' }
        @{ Name = 'rung: a failed scan with no evidence refuses, never a guess'; Run = { Get-PrologueRepairMethod 'scan failed: Access denied' }; Expect = 'refuse' }
        @{ Name = 'rung: a failed scan but the log found problems still goes to chkdsk /f'; Run = { Get-PrologueRepairMethod -Scan 'scan failed: x' -LogVerdict 'found-problems' }; Expect = 'chkdsk-f' }
        @{ Name = 'rung: an empty answer refuses'; Run = { Get-PrologueRepairMethod '' }; Expect = 'refuse' }
        @{ Name = 'chkdsk event: found problems with records and queued items; found no problems; localized unknown'
           Run = { $a = ConvertFrom-PrologueChkdskEvent "Examining 18 corruption records ...`n ... queued for offline repair.`nWindows has examined the list of previously identified potential issues and found problems."; $b = ConvertFrom-PrologueChkdskEvent 'Windows has scanned the file system and found no problems.'; $c = ConvertFrom-PrologueChkdskEvent 'keine Probleme gefunden'; "$($a.Verdict)/$($a.Records)/$($a.Queued)/$($b.Verdict)/$($c.Verdict)" }; Expect = 'found-problems/18/1/no-problems/unknown' }
        @{ Name = 'disk events: only \Device\Harddisk1\ counts (not Harddisk10), 7 and 51 tallied'
           Run = { $t = Get-Date; $r = ConvertFrom-PrologueDiskEvents -DiskNumber 1 -Events @(
                     [pscustomobject]@{ Id = 7; TimeCreated = $t; Message = 'The device, \Device\Harddisk1\DR1, has a bad block.' },
                     [pscustomobject]@{ Id = 7; TimeCreated = $t; Message = 'The device, \Device\Harddisk10\DR9, has a bad block.' },
                     [pscustomobject]@{ Id = 51; TimeCreated = $t; Message = 'An error was detected on device \Device\Harddisk1\DR1 during a paging operation.' }); "$($r.BadBlock)/$($r.Paging)" }; Expect = '1/1' }
        @{ Name = 'health gate: Healthy with no bad blocks passes'; Run = { (Test-PrologueDiskHealthGate -Health 'Healthy' -BadBlocks 0).Pass }; Expect = $true }
        @{ Name = 'health gate: Healthy with bad-block events refuses and names the count'; Run = { $g = Test-PrologueDiskHealthGate -Health 'Healthy' -BadBlocks 261; "$($g.Pass)/$([bool]($g.Reason -match '261 bad-block'))" }; Expect = 'False/True' }
        @{ Name = 'health gate: Warning refuses; Unknown refuses; empty refuses'; Run = { "$((Test-PrologueDiskHealthGate 'Warning').Pass)/$((Test-PrologueDiskHealthGate 'Unknown').Pass)/$((Test-PrologueDiskHealthGate '').Pass)" }; Expect = 'False/False/False' }
        @{ Name = 'health gate (R23): the acknowledgement lifts it and the reason says DATA LOSS ACCEPTED'; Run = { $g = Test-PrologueDiskHealthGate -Health 'Healthy' -BadBlocks 261 -Acknowledged $true; "$($g.Pass)/$($g.Reason -like 'DATA LOSS ACCEPTED*')" }; Expect = 'True/True' }
        @{ Name = 'health gate (R23): acknowledged on a clean disk says nothing about data loss'; Run = { (Test-PrologueDiskHealthGate -Health 'Healthy' -BadBlocks 0 -Acknowledged $true).Reason -like 'DATA LOSS*' }; Expect = $false }
        @{ Name = 'scan string: cmdlet and evidence in one line'; Run = { Format-PrologueScan -Cmdlet 'NoErrorsFound' -Ev ([ordered]@{ LogVerdict = 'found-problems'; LogRecords = 18; LogQueued = 12; VolumeStatus = 'Full Repair Needed'; NtfsFullChkdsk = '2026-09-13T14:10:04Z' }) }; Expect = 'cmdlet: NoErrorsFound; log: found-problems (18 corruption records, 12 queued for offline repair); volume: Full Repair Needed; ntfs98: 2026-09-13T14:10:04Z' }
        @{ Name = 'stopped outcome (R23): carries the job''s acknowledgement when present, none otherwise'
           Run = { $j2 = $job | ConvertTo-Json -Depth 8 | ConvertFrom-Json; $j2 | Add-Member -NotePropertyName risk_acknowledgement -NotePropertyValue ([pscustomobject]@{ statement = 'I confirm that I understand the risks and could lose data'; accepted_utc = '2026-09-13T15:00:00Z'; overrides = @('disk-health') })
                   $a = New-PrologueStoppedOutcome -Job $j2 -S (New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\') -StoppedAt 'shrink' -Reason 'x' -WindowsPartition $null
                   $b = New-PrologueStoppedOutcome -Job $job -S (New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\') -StoppedAt 'shrink' -Reason 'x' -WindowsPartition $null
                   "$($a.risk_acknowledgement.overrides -join ',')/$($b.Contains('risk_acknowledgement'))" }; Expect = 'disk-health/False' }
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
        @{ Name = 'fork: a clean-slate job the person chose is clean slate whatever the number'; Run = { Get-PrologueFork -JobPath 'clean-slate' -Fits $true -IfCannotKeep 'stop' -PathReason 'user-chose-clean-slate' }; Expect = 'clean-slate' }
        @{ Name = 'fork (R18, 2026-09-22): a forced clean-slate job under if_cannot_keep=stop stops - the Aspire run 5 job'; Run = { "$(Get-PrologueFork -JobPath 'clean-slate' -Fits $false -IfCannotKeep 'stop' -PathReason 'forced-no-room')/$(Get-PrologueFork -JobPath 'clean-slate' -Fits $false -IfCannotKeep 'ask' -PathReason 'forced-no-room')" }; Expect = 'stop/stop' }
        @{ Name = 'fork: a forced clean-slate job the person allowed (if_cannot_keep=clean-slate) is clean slate'; Run = { Get-PrologueFork -JobPath 'clean-slate' -Fits $false -IfCannotKeep 'clean-slate' -PathReason 'forced-no-room' }; Expect = 'clean-slate' }
        # --- the erase-and-install path (R27, 2026-09-26) ---------------------------
        @{ Name = 'erase start: an erase job starts with the erase sentence and no CONVERT word'
           Run = { $j = [pscustomobject]@{ erase_consent = [pscustomobject]@{ statement = $EraseStatement } }; $null -eq (Get-PrologueEraseStartRefusal -Job $j -ConfirmWord '' -EraseConsent $EraseStatement) }; Expect = $true }
        @{ Name = 'erase start: CONVERT alone does not start an erase job'
           Run = { $j = [pscustomobject]@{ erase_consent = [pscustomobject]@{ statement = $EraseStatement } }; [bool]((Get-PrologueEraseStartRefusal -Job $j -ConfirmWord 'CONVERT' -EraseConsent '') -match 'the erase sentence was not typed for this run') }; Expect = $true }
        @{ Name = 'erase start: a paraphrase does not start it'
           Run = { $j = [pscustomobject]@{ erase_consent = [pscustomobject]@{ statement = $EraseStatement } }; [bool](Get-PrologueEraseStartRefusal -Job $j -ConfirmWord '' -EraseConsent 'I confirm everything will be deleted') }; Expect = $true }
        @{ Name = 'erase start: a job whose sentence differs from this prologue''s is refused'
           Run = { $j = [pscustomobject]@{ erase_consent = [pscustomobject]@{ statement = 'delete it all' } }; [bool]((Get-PrologueEraseStartRefusal -Job $j -ConfirmWord '' -EraseConsent 'delete it all') -match 'not the one this prologue knows') }; Expect = $true }
        @{ Name = 'erase start: the erase sentence never starts a keep-windows job'
           Run = { $j = [pscustomobject]@{ job_id = 'x' }; [bool]((Get-PrologueEraseStartRefusal -Job $j -ConfirmWord 'CONVERT' -EraseConsent $EraseStatement) -match 'not an erase job') }; Expect = $true }
        @{ Name = 'erase start: a keep-windows job still needs CONVERT'
           Run = { $j = [pscustomobject]@{ job_id = 'x' }; "$($null -eq (Get-PrologueEraseStartRefusal -Job $j -ConfirmWord 'CONVERT' -EraseConsent '')):$([bool](Get-PrologueEraseStartRefusal -Job $j -ConfirmWord 'convert' -EraseConsent ''))" }; Expect = 'True:True' }
        @{ Name = 'erase disks: both drives present at their sizes is no mismatch'
           Run = { $j = [pscustomobject]@{ erase_consent = [pscustomobject]@{ disks = @([pscustomobject]@{ role = 'system'; unique_id = 'ssd'; size_bytes = 256; friendly_name = 'SSD' }, [pscustomobject]@{ role = 'home'; unique_id = 'hdd'; size_bytes = 1000; friendly_name = 'HDD' }) } }
                   $F = @{ Disk = @{ UniqueId = 'ssd'; Size = 256 }; AllDisks = @(@{ UniqueId = 'ssd'; Size = 256 }, @{ UniqueId = 'hdd'; Size = 1000 }) }
                   @(Compare-PrologueEraseDisks -Job $j -F $F).Count }; Expect = 0 }
        @{ Name = 'erase disks: a home drive that is gone, or a different size, is a mismatch (never erase a stranger''s drive)'
           Run = { $j = [pscustomobject]@{ erase_consent = [pscustomobject]@{ disks = @([pscustomobject]@{ role = 'system'; unique_id = 'ssd'; size_bytes = 256; friendly_name = 'SSD' }, [pscustomobject]@{ role = 'home'; unique_id = 'hdd'; size_bytes = 1000; friendly_name = 'HDD' }) } }
                   $gone = @(Compare-PrologueEraseDisks -Job $j -F @{ Disk = @{ UniqueId = 'ssd'; Size = 256 }; AllDisks = @(@{ UniqueId = 'ssd'; Size = 256 }) })
                   $size = @(Compare-PrologueEraseDisks -Job $j -F @{ Disk = @{ UniqueId = 'ssd'; Size = 256 }; AllDisks = @(@{ UniqueId = 'ssd'; Size = 256 }, @{ UniqueId = 'hdd'; Size = 999 }) })
                   "$([bool]($gone -match 'is not attached')):$([bool]($size -match 'is 999 bytes; the job says 1000'))" }; Expect = 'True:True' }
        @{ Name = 'erase disks: a first drive that is not the C: drive is a mismatch'
           Run = { $j = [pscustomobject]@{ erase_consent = [pscustomobject]@{ disks = @([pscustomobject]@{ role = 'system'; unique_id = 'other'; size_bytes = 256; friendly_name = 'X' }) } }
                   [bool](@(Compare-PrologueEraseDisks -Job $j -F @{ Disk = @{ UniqueId = 'ssd'; Size = 256 }; AllDisks = @() }) -match 'is not the drive holding C:') }; Expect = $true }
        @{ Name = 'erase return: a key in the countdown is stopped_at countdown, nothing erased'
           Run = { $r = Get-PrologueEraseReturn -Countdown ([pscustomobject]@{ result = 'cancelled'; ended_utc = '2026-09-26T22:00:00Z' }) -Verify $null; "$($r.StoppedAt):$([bool]($r.Reason -match 'nothing was erased'))" }; Expect = 'countdown:True' }
        @{ Name = 'erase return: the installer refusing on identity is stopped_at identity'
           Run = { (Get-PrologueEraseReturn -Countdown $null -Verify ([pscustomobject]@{ identity = [pscustomobject]@{ result = 'fail' }; payload = [pscustomobject]@{ result = 'pass' } })).StoppedAt }; Expect = 'identity' }
        @{ Name = 'erase return: a countdown that elapsed and still came back says the drives may be partly erased - never "nothing"'
           Run = { $r = Get-PrologueEraseReturn -Countdown ([pscustomobject]@{ result = 'elapsed'; ended_utc = 't' }) -Verify $null; "$($r.StoppedAt):$([bool]($r.Reason -match 'may be partly erased')):$([bool]($r.Reason -match 'nothing was erased'))" }; Expect = 'install:True:False' }
        @{ Name = 'erase: a stopped outcome carries the erase consent'
           Run = { $j = [pscustomobject]@{ job_id = 'j'; erase_consent = [pscustomobject]@{ statement = $EraseStatement; accepted_utc = 't'; disks = @() } }; $o = New-PrologueStoppedOutcome -Job $j -S (New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\') -StoppedAt 'countdown' -Reason 'r' -WindowsPartition $null; "$($o.erase_consent.statement -ceq $EraseStatement):$($o.status):$($o.commit_line.crossed)" }; Expect = 'True:stopped:False' }
        @{ Name = 'stage (R18, 2026-09-22): no folders in the job refuses before anything is staged, and says why'; Run = { [bool]((Get-PrologueStageRefusal -Folders 0) -match '^the job lists none of your folders .* refusing to prepare a wipe with no copy of your files$') }; Expect = $true }
        @{ Name = 'stage (R18, 2026-09-22): folders listed but 0 files staged refuses'; Run = { [bool]((Get-PrologueStageRefusal -Folders 3 -StagedFiles 0) -match '^no files were copied to the stick from the 3 folder') }; Expect = $true }
        @{ Name = 'stage: folders listed, before staging, and files staged, after, are not refusals'; Run = { "$($null -eq (Get-PrologueStageRefusal -Folders 3))/$($null -eq (Get-PrologueStageRefusal -Folders 3 -StagedFiles 1204))" }; Expect = 'True/True' }
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
        # the unattended resume (SYSTEM at startup, no sign-in) and its notices
        @{ Name = 'resume context: SYSTEM in session 0 is unattended'; Run = { (Get-ResumeContext -UserName 'NT AUTHORITY\SYSTEM' -UserInteractive $false -SessionId 0 -ExplorerRunning $false).Unattended }; Expect = $true }
        @{ Name = 'resume context: a signed-in person in session 1 is attended'; Run = { (Get-ResumeContext -UserName 'PC\rig' -UserInteractive $true -SessionId 1 -ExplorerRunning $true).Unattended }; Expect = $false }
        @{ Name = 'resume context: session 0 is unattended even if the flag says interactive'; Run = { (Get-ResumeContext -UserName 'NT AUTHORITY\SYSTEM' -UserInteractive $true -SessionId 0 -ExplorerRunning $false).Unattended }; Expect = $true }
        @{ Name = 'notice: the RunOnce command is hidden, unelevated -Notify from the state copy'; Run = { New-NoticeCommand -ScriptPath 'C:\ProgramData\upgrade_\prologue\Invoke-Prologue.ps1' -State 'C:\ProgramData\upgrade_\prologue' }; Expect = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "C:\ProgramData\upgrade_\prologue\Invoke-Prologue.ps1" -Notify -StateDir "C:\ProgramData\upgrade_\prologue"' }
        @{ Name = 'state: a fresh state carries an empty resume log; an old state without one round-trips'; Run = { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $old = ConvertTo-PrologueHashtable ((ConvertTo-PrologueJson $s) | ConvertFrom-Json); $old.Remove('Resumes'); "$(@($s.Resumes).Count):$($old.Contains('Resumes'))" }; Expect = '0:False' }
        @{ Name = 'record: resumes carry SYSTEM/user, the session, unattended and the stick wait - never the raw account'
           Run = { $s = New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\'; $s.Resumes = @([ordered]@{ Utc = '2026-09-13T23:33:45Z'; RunAs = 'NT AUTHORITY\SYSTEM'; SessionId = 0; Unattended = $true; StickWaitSeconds = 4 }, [ordered]@{ Utc = '2026-09-13T23:40:00Z'; RunAs = 'PC\rig'; SessionId = 1; Unattended = $false; StickWaitSeconds = $null })
                   $b = New-PrologueBlock $s; "$($b.resumes.Count):$($b.resumes[0].run_as):$($b.resumes[0].session_id):$($b.resumes[0].unattended):$($b.resumes[0].stick_wait_seconds):$($b.resumes[1].run_as):$($null -eq $b.resumes[1].stick_wait_seconds)" }; Expect = '2:SYSTEM:0:True:4:user:True' }
        @{ Name = 'record: no resumes yet means no resumes key (the field is optional in the contract)'; Run = { (New-PrologueBlock (New-PrologueState -JobId 'j' -StickId 's' -Root 'E:\')).Contains('resumes') }; Expect = $false }
        @{ Name = 'probe: SYSTEM, stick found, task removed is resumed-unattended'; Run = { Get-ProbeResult -Unattended $true -StickFound $true -TaskRemoved $true }; Expect = 'resumed-unattended' }
        @{ Name = 'probe: a session present is resumed-attended'; Run = { Get-ProbeResult -Unattended $false -StickFound $true -TaskRemoved $true }; Expect = 'resumed-attended' }
        @{ Name = 'probe: no stick names that first, whatever else'; Run = { Get-ProbeResult -Unattended $false -StickFound $false -TaskRemoved $false }; Expect = 'stick-not-found' }
        @{ Name = 'probe: a task left behind is task-not-removed'; Run = { Get-ProbeResult -Unattended $true -StickFound $true -TaskRemoved $false }; Expect = 'task-not-removed' }
        @{ Name = 'probe csv: fields quoted, quotes doubled, newlines flattened, header has 18 columns'; Run = { "$(ConvertTo-ProbeCsvLine @('a', 'say ""hi""', "x`r`ny", 3, $true))/$(@($ProbeCsvHeader).Count)" }; Expect = '"a","say ""hi""","x y","3","True"/18' }
        @{ Name = 'wifi secrets: every file in the stick''s Wi-Fi folder goes, and the folder; none is 0; no root is 0'
           Run = { $t = Join-Path ([IO.Path]::GetTempPath()) ("upg-st-" + [guid]::NewGuid()); $w = Join-Path $t 'upgrade_\artifacts\credentials\wifi'; New-Item -ItemType Directory -Path $w -Force | Out-Null
                   '<k>secret</k>' | Set-Content (Join-Path $w '01.xml'); '<k>secret</k>' | Set-Content (Join-Path $w '02.xml'); Set-Content (Join-Path $t 'upgrade_\artifacts\credentials\bitlocker-C.txt') 'k'
                   $a = Remove-PrologueWifiSecrets -Root $t; $gone = -not (Test-Path $w); $kept = Test-Path (Join-Path $t 'upgrade_\artifacts\credentials\bitlocker-C.txt'); $b = Remove-PrologueWifiSecrets -Root $t; $c = Remove-PrologueWifiSecrets -Root ''
                   Remove-Item $t -Recurse -Force; "$($a):$($gone):$($kept):$($b):$($c)" }; Expect = '2:True:True:0:0' }
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
if ($Notify) { Invoke-NotifyPhase; return }
if (-not (Test-Elevated)) { throw 'the prologue needs Administrator: it reads and changes the disk and the boot configuration' }
if (-not (Test-UefiBoot)) { throw 'this machine is not UEFI-booted; the boot handoff does not apply' }
if ($Abort) { Invoke-AbortPhase; return }
if ($Probe) { Invoke-ProbeStart; return }
if ($Start) {
    # a refusal before any state exists is a stop too: the Wi-Fi passwords leave the stick (2026-09-27)
    try { Invoke-StartPhase } catch { if ($StickDrive) { try { Remove-PrologueWifiSecrets -Root (Get-DriveRoot $StickDrive) | Out-Null } catch { } }; throw }
    return
}
Invoke-ResumePhase
