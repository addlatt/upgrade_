<#
.SYNOPSIS
    Copy the built kit (dist/kit/stick) onto a mounted USB stick and verify it.

.DESCRIPTION
    Host-side helper for a physical visit. It is NOT the stick writer
    (Write-UpgradeStick.ps1, R16): it never partitions or formats - it copies
    the kit's files onto a stick that already carries a FAT32 volume, using
    robocopy /E, clears the runtime files a previous run left under upgrade_\
    (job, kickstart, markers, reports, records - never the evidence CSV), then
    re-hashes every file on the stick against the kit's SHA256SUMS and ejects.
    Refuses anything that is not a removable USB volume.

    Learned on the first physical stick (2026-09-12): a loosely seated stick
    drops off the bus under a multi-gigabyte write (event 51 bursts); seat it
    firmly and re-run if the copy fails.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File Copy-Kit.ps1 -Drive D:
#>
param(
    [Parameter(Mandatory = $true)][string]$Drive,
    [string]$Kit = '\\wsl.localhost\Ubuntu\home\addlatt\upgrade_\dist\kit\stick'
)
$ErrorActionPreference = 'Stop'
$l = $Drive.TrimEnd(':', '\').ToUpper()
$vol = Get-Volume -DriveLetter $l -ErrorAction Stop
$disk = Get-Partition -DriveLetter $l | Get-Disk
if ($disk.BusType -ne 'USB' -or $vol.DriveType -ne 'Removable') { throw "$l`: is $($disk.BusType)/$($vol.DriveType), not a removable USB volume; refusing" }
if (-not (Test-Path (Join-Path $Kit 'SHA256SUMS'))) { throw "no SHA256SUMS in $Kit - run ./make-kit.sh first" }
Write-Host "  stick $l`: '$($vol.FileSystemLabel)' $($disk.FriendlyName) $([math]::Round($disk.Size/1e9,1)) GB, $([math]::Round($vol.SizeRemaining/1e9,2)) GB free"

# stale runtime files from a previous run (the evidence CSV and captures stay)
foreach ($rel in @('upgrade_\job.json', 'upgrade_\ks.cfg', 'upgrade_\boot-verify', 'upgrade_\boot-install', 'upgrade_\prologue.json',
                   'upgrade_\prologue-return.json', 'upgrade_\outcome.json', 'upgrade_\rollback.json', 'upgrade_\report', 'upgrade_\reports',
                   'upgrade_\artifacts', 'upgrade_\esp-snapshot', 'upgrade_\staging', 'upgrade_\rollback', 'upgrade_\boots.log')) {
    $p = "$l`:\$rel"; if (Test-Path $p) { Remove-Item $p -Recurse -Force; Write-Host "  removed stale $rel" }
}

Write-Host "  copying $Kit -> $l`:\ (robocopy /E)..."
& robocopy $Kit "$l`:\" /E /R:2 /W:5 /NP /NFL /NDL | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with code $LASTEXITCODE (a stick dropping off the bus looks like this - re-seat it and re-run)" }

Write-Host '  verifying every file on the stick against SHA256SUMS...'
$bad = 0; $n = 0
foreach ($line in Get-Content (Join-Path $Kit 'SHA256SUMS')) {
    if ($line -notmatch '^([0-9a-f]{64})\s+\*?\./(.+)$') { continue }
    $want = $matches[1]; $rel = $matches[2] -replace '/', '\'
    $p = "$l`:\$rel"; $n++
    if (-not (Test-Path -LiteralPath $p)) { Write-Host "  MISSING $rel" -ForegroundColor Red; $bad++; continue }
    $got = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLower()
    if ($got -ne $want) { Write-Host "  MISMATCH $rel" -ForegroundColor Red; $bad++ }
}
if ($bad -gt 0) { throw "$bad of $n files failed verification on the stick" }
Write-Host "  $n files verified on $l`:" -ForegroundColor Green
Get-Content "$l`:\KIT-MANIFEST.txt" | Select-Object -First 3 | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }

# eject (Shell.Application, namespace 17 = My Computer)
try {
    $sh = New-Object -ComObject Shell.Application
    $sh.NameSpace(17).ParseName("$l`:\").InvokeVerb('Eject')
    Write-Host "  ejected $l`: - safe to unplug" -ForegroundColor Green
} catch { Write-Host "  could not eject automatically ($_); eject from the tray icon" -ForegroundColor Yellow }
