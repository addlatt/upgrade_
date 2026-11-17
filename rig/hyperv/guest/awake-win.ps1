# rig/hyperv/guest/awake-win.ps1: the Windows programs' stay-awake request,
# inside the bench's Windows (roadmap item 16, RISKS R33). PowerShell 5.1,
# elevated (powercfg /requests needs it).
#
# Runs the real scanner from C:\upgr39 twice and samples `powercfg /requests`
# (Windows' own list of who is holding the computer awake) while it runs and
# after it ends: once from this PowerShell Direct session (no desktop), once
# on the signed-in person's desktop through a scheduled task, the way the
# window starts it. One line out: AWAKEJSON=<base64 of JSON>.
#
# This guest has no sleep state at all (powercfg /a), so no idle sleep can be
# shown here: the request being listed is what the rig can show.
$ErrorActionPreference = 'Continue'
$bin = 'C:\upgr39'; $w = "$bin\awake"
$stick = (Get-Volume -FileSystemLabel UPGV0).DriveLetter + ':'
function Sections([string]$q) {
    $h = @{}; $cur = ''
    foreach ($l in ($q -split "`r?`n")) { if ($l -match '^([A-Z]+):\s*$') { $cur = $Matches[1]; $h[$cur] = @() } elseif ($cur -and $l -match 'upgrade-scan') { $h[$cur] += $l.Trim() } }
    return @($h.Keys | Where-Object { $h[$_].Count } | Sort-Object)
}
function Watch([scriptblock]$running) {
    $seen = @{}; $n = 0
    while (& $running) { $n++; foreach ($s in (Sections (powercfg /requests | Out-String))) { $seen[$s] = 1 }; Start-Sleep -Milliseconds 300 }
    Start-Sleep -Milliseconds 500
    return @{ Samples = $n; While = @($seen.Keys | Sort-Object); After = @(Sections (powercfg /requests | Out-String)) }
}
$r = [ordered]@{ ScanVersion = (& "$bin\upgrade-scan.exe" --version) -join ' '; SleepStates = ((powercfg /a | Out-String) -replace '\s+', ' ').Trim(); Before = @(Sections (powercfg /requests | Out-String)) }
Remove-Item $w -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $w | Out-Null

$p = Start-Process -FilePath "$bin\upgrade-scan.exe" -ArgumentList 'scan','--json','--out',$w,'--kit',"$stick\" -PassThru -WindowStyle Hidden -RedirectStandardOutput "$w\out1.txt"
$r.NoDesktop = Watch { -not $p.HasExited }

Set-Content "$w\run.cmd" "@echo off`r`n$bin\upgrade-scan.exe scan --json --out $w --kit $stick\ > $w\out2.txt 2>&1`r`necho RC=%ERRORLEVEL% > $w\done.txt`r`n" -Encoding ASCII
$a = New-ScheduledTaskAction -Execute cmd.exe -Argument "/c $w\run.cmd"
$pr = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Highest
Register-ScheduledTask -TaskName 'upgr39 awake' -Action $a -Principal $pr -Force | Out-Null
$r.SignedIn = ((quser 2>&1 | Out-String) -replace '\s+', ' ').Trim()
Start-ScheduledTask -TaskName 'upgr39 awake'
$end = (Get-Date).AddSeconds(180)
$r.Desktop = Watch { -not (Test-Path "$w\done.txt") -and (Get-Date) -lt $end }
$r.DesktopRan = Test-Path "$w\done.txt"
Unregister-ScheduledTask -TaskName 'upgr39 awake' -Confirm:$false
Remove-Item $w -Recurse -Force -ErrorAction SilentlyContinue
'AWAKEJSON=' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($r | ConvertTo-Json -Depth 5 -Compress)))
