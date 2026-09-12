<#
.SYNOPSIS
    upgrade_ - rollback: Windows first again, its fallback loader restored.

.DESCRIPTION
    Rollback is a mode of upgrade_, not a fourth module (docs/architecture.md,
    "Rollback is a mode of upgrade_"; decided 2026-08-30, RISKS R21). On the
    keep-Windows path Windows was never touched: the converter installed
    Linux beside it, put Fedora first in the firmware boot order and let
    shim take the removable-media fallback slot (EFI\Boot\bootx64.efi) on
    the shared ESP - keeping Windows' own copy in the snapshot %pre took to
    the stick before anything was changed. Rolling back is therefore a
    boot-order change plus one file, and it deletes nothing:

      1. read everything first - the job, the outcome (keep-windows, Windows
         kept, snapshot present), this machine's disk identity against the
         job (a moved stick must never touch a stranger's ESP), the
         snapshot's checksum for EFI/Boot/bootx64.efi, what sits there now
      2. refuse on any of those; otherwise
      3. mount the ESP, back the current fallback loader up to the stick,
         copy Windows' copy back from the snapshot, verify its sha256
      4. bcdedit: Windows Boot Manager first in the firmware order
         ({fwbootmgr} displayorder {bootmgr} /addfirst), any leftover
         one-shot cleared
      5. record upgrade_\rollback.json on the stick

    The Linux partitions and EFI\fedora stay exactly where they are: Linux
    remains bootable from the firmware's boot menu, and the space is only
    returned when the person asks for it. This runs from the kept Windows
    (booted from the GRUB menu); the settle-in side of the same rollback
    runs from Linux with efibootmgr and is not built yet.

.PARAMETER StickDrive
    The kit stick's drive letter (E:). Holds job.json, outcome.json and the
    ESP snapshot under upgrade_\.

.PARAMETER SelfTest
    Logic tests against fabricated inputs. Touches nothing.
#>
[CmdletBinding()]
param(
    [string]$StickDrive,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
$RollbackVersion = '0.1.0'
$FallbackRel = 'EFI/Boot/bootx64.efi'

# --- pure (self-tested) --------------------------------------------------------

function ConvertFrom-RollbackSums {
    # sha256sum lines ("<sha>  ./EFI/Boot/bootx64.efi") -> @{ 'efi/boot/bootx64.efi' = sha }
    # keyed lower-case with forward slashes: FAT is case-insensitive, and the
    # snapshot may say EFI/BOOT where Windows wrote EFI\Boot.
    param([string[]]$Lines)
    $h = @{}
    foreach ($l in @($Lines)) {
        if ($l -match '^([0-9a-fA-F]{64})\s[\s*](.+)$') { $h[(($matches[2].Trim() -replace '^\./', '') -replace '\\', '/').ToLower()] = $matches[1].ToLower() }
    }
    $h
}

function Get-RollbackPlan {
    # Every refusal, then the plan. Inputs are facts already read; nothing here touches the machine.
    param($Outcome, $Sums, [string]$CurrentSha, [string[]]$IdentityMismatches)
    $r = New-Object System.Collections.Generic.List[string]
    if (-not $Outcome) { $r.Add('no outcome.json on the stick - nothing says a conversion happened here') }
    else {
        if ("$($Outcome.schema)" -ne 'outcome/1') { $r.Add("outcome schema '$($Outcome.schema)' is not outcome/1") }
        if ("$($Outcome.path_taken)" -ne 'keep-windows') { $r.Add("the conversion's path was '$($Outcome.path_taken)', not keep-windows - there is no kept Windows to roll back to") }
        if (-not $Outcome.windows -or -not $Outcome.windows.kept) { $r.Add('the outcome says Windows was not kept') }
        if (-not $Outcome.cutover -or -not $Outcome.cutover.esp_snapshot) { $r.Add('the outcome names no ESP snapshot') }
    }
    foreach ($m in @($IdentityMismatches)) { $r.Add("identity: $m") }
    $want = $null
    if ($Sums) { $want = $Sums[$FallbackRel.ToLower()] }
    if (-not $want) { $r.Add("the snapshot has no checksum for $FallbackRel - nothing to restore from") }
    if ($r.Count -gt 0) { return @{ Refusals = $r.ToArray(); Restore = $false; WantSha = $want } }
    @{ Refusals = @(); Restore = ($CurrentSha -ne $want); WantSha = $want; CurrentSha = $CurrentSha
       Reason = $(if ($CurrentSha -ne $want) { 'the fallback slot holds something other than Windows'' copy' } else { 'the fallback slot already holds Windows'' copy' }) }
}

function Compare-RollbackIdentity {
    param($Job, $Disk)
    $m = New-Object System.Collections.Generic.List[string]
    if ("$($Job.identity.system_disk.unique_id)" -ne "$($Disk.UniqueId)") { $m.Add("system disk unique id: job '$($Job.identity.system_disk.unique_id)', machine '$($Disk.UniqueId)'") }
    if ("$($Job.identity.system_disk.size_bytes)" -ne "$($Disk.Size)") { $m.Add("system disk size: job $($Job.identity.system_disk.size_bytes), machine $($Disk.Size)") }
    $m.ToArray()
}

function Get-DisplayOrderTokens {
    param([string]$Text)
    if ($Text -match '(?m)^\s*displayorder\s+(.+(?:\r?\n\s{20,}.+)*)') { return @((($matches[1] -replace '\s+', ' ').Trim()) -split ' ' | Where-Object { $_ }) }
    @()
}

function Test-WindowsFirst {
    param([string[]]$Tokens)
    (@($Tokens).Count -gt 0) -and ("$($Tokens[0])" -eq '{bootmgr}')
}

function Get-DriveRoot {
    param([string]$Letter)
    $l = $Letter.TrimEnd(':', '\').ToUpper()
    if ($l.Length -ne 1) { throw "StickDrive must be a single drive letter, got '$Letter'." }
    "${l}:\"
}

function ConvertTo-RollbackJson { param($Obj) (($Obj | ConvertTo-Json -Depth 8) -replace "`r`n", "`n") }

# --- live -------------------------------------------------------------------------

function Test-Elevated {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-FreeLetter {
    $used = @((Get-PSDrive -PSProvider FileSystem | ForEach-Object { $_.Name.ToUpper() }) + @(Get-Volume | ForEach-Object { "$($_.DriveLetter)".ToUpper() }))
    foreach ($c in 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z') { if ($c -notin $used) { return $c } }
    throw 'no free drive letter to mount the ESP on'
}

function Mount-Esp {
    $l = Get-FreeLetter
    & mountvol "${l}:" /S 2>&1 | Out-Null
    if (-not (Test-Path "${l}:\EFI")) { throw "mountvol ${l}: /S did not expose an EFI directory" }
    "${l}:\"
}

function Dismount-Esp { param([string]$Root) & mountvol $Root.TrimEnd('\') /D 2>&1 | Out-Null }

function Get-Sha { param([string]$Path) if (Test-Path $Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLower() } else { $null } }

function Invoke-Rollback {
    $root = Get-DriveRoot $StickDrive
    if (-not (Test-Path $root)) { throw "stick $root not found" }
    Write-Host ''; Write-Host "  upgrade_  rollback $RollbackVersion" -ForegroundColor Cyan
    Write-Host '  Windows Boot Manager first again; its fallback loader restored from the snapshot. Deletes nothing.' -ForegroundColor DarkGray
    $jobPath = Join-Path $root 'upgrade_\job.json'; $outPath = Join-Path $root 'upgrade_\outcome.json'
    $job = if (Test-Path $jobPath) { Get-Content $jobPath -Raw | ConvertFrom-Json } else { $null }
    $outcome = if (Test-Path $outPath) { Get-Content $outPath -Raw | ConvertFrom-Json } else { $null }
    $snapDir = Join-Path $root 'upgrade_\esp-snapshot'
    $sums = if (Test-Path (Join-Path $snapDir 'SHA256SUMS')) { ConvertFrom-RollbackSums (Get-Content (Join-Path $snapDir 'SHA256SUMS')) } else { $null }
    $part = Get-Partition -DriveLetter C -ErrorAction Stop; $disk = Get-Disk -Number $part.DiskNumber -ErrorAction Stop
    $mm = if ($job) { @(Compare-RollbackIdentity -Job $job -Disk ([ordered]@{ UniqueId = "$($disk.UniqueId)"; Size = [long]$disk.Size })) } else { @('no job.json on the stick') }
    $esp = Mount-Esp
    try {
        $fallback = Join-Path $esp 'EFI\Boot\bootx64.efi'
        $cur = Get-Sha $fallback
        $plan = Get-RollbackPlan -Outcome $outcome -Sums $sums -CurrentSha "$cur" -IdentityMismatches $mm
        if ($plan.Refusals.Count -gt 0) {
            Write-Host ''; Write-Host '  REFUSED - nothing changed:' -ForegroundColor Red
            foreach ($x in $plan.Refusals) { Write-Host "    - $x" -ForegroundColor Red }
            Write-Host ''; exit 2
        }
        $before = (& bcdedit /enum '{fwbootmgr}' 2>&1) -join "`n"
        $orderBefore = Get-DisplayOrderTokens $before
        Write-Host "  fallback slot now: $cur"; Write-Host "  Windows' copy:     $($plan.WantSha)"; Write-Host "  firmware order:    $($orderBefore -join ' ')"
        $restored = $false; $backup = $null
        if ($plan.Restore) {
            $bdir = Join-Path $root 'upgrade_\rollback'; New-Item -ItemType Directory -Path $bdir -Force | Out-Null
            $backup = 'upgrade_/rollback/bootx64.efi.before'
            Copy-Item -LiteralPath $fallback -Destination (Join-Path $root 'upgrade_\rollback\bootx64.efi.before') -Force
            $src = Get-ChildItem -Path $snapDir -Recurse -File | Where-Object { ($_.FullName.Substring($snapDir.Length).TrimStart('\') -replace '\\', '/').ToLower() -eq $FallbackRel.ToLower() } | Select-Object -First 1
            if (-not $src) { throw "the snapshot directory has no $FallbackRel although its manifest lists one" }
            if ((Get-Sha $src.FullName) -ne $plan.WantSha) { throw "the snapshot's $FallbackRel does not match its own manifest; refusing to copy a file that fails its checksum" }
            Copy-Item -LiteralPath $src.FullName -Destination $fallback -Force
            $after = Get-Sha $fallback
            if ($after -ne $plan.WantSha) { throw "after the copy the fallback loader's sha256 is $after, not $($plan.WantSha)" }
            $restored = $true
            Write-Host "  restored EFI\Boot\bootx64.efi from the snapshot (previous copy saved to $backup)" -ForegroundColor Green
        } else { Write-Host "  $($plan.Reason); nothing to restore" }
        & bcdedit /deletevalue '{fwbootmgr}' bootsequence 2>&1 | Out-Null
        & bcdedit /set '{fwbootmgr}' displayorder '{bootmgr}' /addfirst 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'bcdedit could not put Windows Boot Manager first' }
        $afterText = (& bcdedit /enum '{fwbootmgr}' 2>&1) -join "`n"
        $orderAfter = Get-DisplayOrderTokens $afterText
        $winFirst = Test-WindowsFirst $orderAfter
        Write-Host "  firmware order now: $($orderAfter -join ' ')  (Windows first: $winFirst)" -ForegroundColor $(if ($winFirst) { 'Green' } else { 'Red' })
        $rec = [ordered]@{
            schema = 'rollback/1'; rollback_version = $RollbackVersion; job_id = "$($job.job_id)"
            created_utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
            fallback_loader = [ordered]@{ restored = $restored; sha_before = $cur; sha_after = (Get-Sha $fallback); snapshot_sha = $plan.WantSha; backup = $backup }
            boot_order = [ordered]@{ before = @($orderBefore); after = @($orderAfter); windows_first = $winFirst }
            linux_left_in_place = $true
        }
        [IO.File]::WriteAllText((Join-Path $root 'upgrade_\rollback.json'), (ConvertTo-RollbackJson $rec), (New-Object Text.UTF8Encoding($false)))
        Write-Host "  record: $($root)upgrade_\rollback.json" -ForegroundColor Cyan
        if (-not $winFirst) { exit 3 }
        Write-Host ''; Write-Host '  Done. The next start boots Windows directly. Linux is still in the firmware boot menu; its space is untouched.' -ForegroundColor Green; Write-Host ''
    } finally { Dismount-Esp $esp }
}

# --- self-test ---------------------------------------------------------------------

function Invoke-SelfTest {
    $sums = ConvertFrom-RollbackSums @(
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  ./EFI/Boot/bootx64.efi',
        'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  ./EFI/Microsoft/Boot/bootmgfw.efi')
    $out = [pscustomobject]@{ schema = 'outcome/1'; path_taken = 'keep-windows'; windows = [pscustomobject]@{ kept = $true }; cutover = [pscustomobject]@{ esp_snapshot = [pscustomobject]@{ path = 'upgrade_/esp-snapshot' } } }
    $shim = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'
    $win = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    $job = [pscustomobject]@{ identity = [pscustomobject]@{ system_disk = [pscustomobject]@{ unique_id = 'eui.1'; size_bytes = 100 } } }
    $cases = @(
        @{ Name = 'sums: paths keyed lower-case with forward slashes, ./ stripped'; Run = { "$($sums['efi/boot/bootx64.efi'])/$($sums.Count)" }; Expect = "$win/2" }
        @{ Name = 'sums: an EFI\BOOT backslash upper-case line keys the same'; Run = { (ConvertFrom-RollbackSums @("$win  EFI\BOOT\BOOTX64.EFI"))['efi/boot/bootx64.efi'] }; Expect = $win }
        @{ Name = 'plan: shim in the slot -> restore'; Run = { $p = Get-RollbackPlan -Outcome $out -Sums $sums -CurrentSha $shim -IdentityMismatches @(); "$($p.Refusals.Count):$($p.Restore):$($p.WantSha)" }; Expect = "0:True:$win" }
        @{ Name = 'plan: Windows already in the slot -> nothing to restore, still allowed (order only)'; Run = { $p = Get-RollbackPlan -Outcome $out -Sums $sums -CurrentSha $win -IdentityMismatches @(); "$($p.Refusals.Count):$($p.Restore)" }; Expect = '0:False' }
        @{ Name = 'refuse: no outcome'; Run = { [bool]((Get-RollbackPlan -Outcome $null -Sums $sums -CurrentSha $shim -IdentityMismatches @()).Refusals -match 'no outcome') }; Expect = $true }
        @{ Name = 'refuse: clean-slate outcome has no kept Windows'; Run = { $o = $out | ConvertTo-Json -Depth 5 | ConvertFrom-Json; $o.path_taken = 'clean-slate'; [bool]((Get-RollbackPlan -Outcome $o -Sums $sums -CurrentSha $shim -IdentityMismatches @()).Refusals -match 'not keep-windows') }; Expect = $true }
        @{ Name = 'refuse: outcome without a snapshot'; Run = { $o = $out | ConvertTo-Json -Depth 5 | ConvertFrom-Json; $o.cutover.esp_snapshot = $null; [bool]((Get-RollbackPlan -Outcome $o -Sums $sums -CurrentSha $shim -IdentityMismatches @()).Refusals -match 'snapshot') }; Expect = $true }
        @{ Name = 'refuse: unknown outcome schema'; Run = { $o = $out | ConvertTo-Json -Depth 5 | ConvertFrom-Json; $o.schema = 'outcome/2'; [bool]((Get-RollbackPlan -Outcome $o -Sums $sums -CurrentSha $shim -IdentityMismatches @()).Refusals -match 'outcome/1') }; Expect = $true }
        @{ Name = 'refuse: snapshot manifest lacks the fallback loader'; Run = { [bool]((Get-RollbackPlan -Outcome $out -Sums (ConvertFrom-RollbackSums @("$win  ./EFI/Microsoft/Boot/bootmgfw.efi")) -CurrentSha $shim -IdentityMismatches @()).Refusals -match 'nothing to restore') }; Expect = $true }
        @{ Name = 'refuse: an identity mismatch (moved stick) refuses everything'; Run = { $p = Get-RollbackPlan -Outcome $out -Sums $sums -CurrentSha $shim -IdentityMismatches @('system disk unique id: x'); "$($p.Refusals.Count):$($p.Restore)" }; Expect = '1:False' }
        @{ Name = 'identity: same disk, no mismatch; other disk, two'; Run = { "$(@(Compare-RollbackIdentity -Job $job -Disk @{ UniqueId = 'eui.1'; Size = 100 }).Count)/$(@(Compare-RollbackIdentity -Job $job -Disk @{ UniqueId = 'eui.2'; Size = 1 }).Count)" }; Expect = '0/2' }
        @{ Name = 'displayorder: parsed from bcdedit text, continuation lines included'
           Run = { $t = "Firmware Boot Manager`n---------------------`nidentifier              {fwbootmgr}`ndisplayorder            {a1b2c3d4-0000-0000-0000-000000000001}`n                        {bootmgr}`n                        {a1b2c3d4-0000-0000-0000-000000000002}`ntimeout                 0"; (Get-DisplayOrderTokens $t) -join ',' }; Expect = '{a1b2c3d4-0000-0000-0000-000000000001},{bootmgr},{a1b2c3d4-0000-0000-0000-000000000002}' }
        @{ Name = 'windows first: true only when {bootmgr} leads'; Run = { "$(Test-WindowsFirst @('{bootmgr}','{x}'))/$(Test-WindowsFirst @('{x}','{bootmgr}'))/$(Test-WindowsFirst @())" }; Expect = 'True/False/False' }
        @{ Name = 'drive: e normalizes to E:\, a path is refused'; Run = { "$(Get-DriveRoot 'e')/$(try { Get-DriveRoot 'E:\x'; 'accepted' } catch { 'refused' })" }; Expect = 'E:\/refused' }
    )
    $failed = 0
    Write-Host ''; Write-Host "  upgrade_  rollback $RollbackVersion  -  SELF-TEST" -ForegroundColor Cyan; Write-Host ''
    foreach ($c in $cases) {
        $got = & $c.Run
        if ("$got" -eq "$($c.Expect)") { Write-Host "    PASS  $($c.Name)" -ForegroundColor Green }
        else { Write-Host "    FAIL  $($c.Name)  (expected '$($c.Expect)', got '$got')" -ForegroundColor Red; $failed++ }
    }
    Write-Host ''
    if ($failed -gt 0) { Write-Host "  $failed check(s) failed" -ForegroundColor Red; exit 1 }
    Write-Host '  all checks passed' -ForegroundColor Green; Write-Host ''
}

if ($SelfTest) { Invoke-SelfTest; return }
if (-not $StickDrive) { throw 'give -StickDrive X: (or -SelfTest)' }
if (-not (Test-Elevated)) { throw 'rollback needs Administrator: it writes the ESP and the firmware boot order' }
if ($env:firmware_type -ne 'UEFI') { throw 'this machine is not UEFI-booted' }
Invoke-Rollback
