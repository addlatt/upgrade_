# VMD spoof session driver (V5 / R1 level 3). Run ELEVATED in the rig guest.
# Drives the scanner's storage-mode check through the real Windows PnP -> WMI
# pipeline against the spoofed Intel VMD device (8086:9a0b, class 0104), and
# writes all evidence to the host share so the host side can read it back.
#
# SPOOF ONLY: a green/red result here closes plumbing, never a real-hardware
# clause (CLAUDE.md rule #5).
$ErrorActionPreference = 'Continue'

$share = '\\10.0.2.4\qemu'
$out   = Join-Path $share 'rig\vm\artifacts\vmd-scan-out'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$log = Join-Path $out 'results.txt'
Remove-Item $log -ErrorAction SilentlyContinue
function Log($m){ $m | Tee-Object -FilePath $log -Append }

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
Log "=== VMD spoof scan run $(Get-Date -Format o) ==="
Log "Elevated: $admin  Computer: $env:COMPUTERNAME"

# --- Step 3: PnP sanity -------------------------------------------------------
Log ""
Log "=== PnP: device VEN_8086&DEV_9A0B ==="
$devs = @(Get-PnpDevice | Where-Object { $_.InstanceId -match 'VEN_8086&DEV_9A0B' })
if (-not $devs) { Log "  !! no VEN_8086&DEV_9A0B device present" }
foreach ($d in $devs) {
    Log ("Name         : {0}" -f $d.FriendlyName)
    Log ("Class        : {0}  Status: {1}" -f $d.Class, $d.Status)
    Log ("InstanceId   : {0}" -f $d.InstanceId)
    $hw = (Get-PnpDeviceProperty -InstanceId $d.InstanceId -KeyName 'DEVPKEY_Device_HardwareIds' -ErrorAction SilentlyContinue).Data
    $co = (Get-PnpDeviceProperty -InstanceId $d.InstanceId -KeyName 'DEVPKEY_Device_CompatibleIds' -ErrorAction SilentlyContinue).Data
    $svc = (Get-PnpDeviceProperty -InstanceId $d.InstanceId -KeyName 'DEVPKEY_Device_Service' -ErrorAction SilentlyContinue).Data
    Log ("HardwareIDs  : {0}" -f ($hw  -join ' | '))
    Log ("CompatibleIDs: {0}" -f ($co  -join ' | '))
    Log ("Service      : {0}" -f $svc)
}

# --- Sync current code into C:\upgrade_ (avoid a stale copy giving a false OK) -
Log ""
Log "=== syncing current repo subset into C:\upgrade_ ==="
foreach ($sub in 'data','evaluate','dist') {
    $null = robocopy (Join-Path $share $sub) (Join-Path 'C:\upgrade_' $sub) /E /NFL /NDL /NJH /NJS /NP /R:1 /W:1
    Log ("  synced {0} (robocopy exit {1})" -f $sub, $LASTEXITCODE)
}

# --- Step 4: run both scanners through the real pipeline ----------------------
function Invoke-Scanner($name, $path) {
    Log ""
    Log "=== $name : $path ==="
    if (-not (Test-Path $path)) { Log "  !! not found"; return }
    $console = Join-Path $out ("{0}-console.txt" -f $name)
    & $path -Json -OutDir $out *>&1 | Out-File -FilePath $console -Encoding UTF8
    $j = Get-ChildItem $out -Filter 'upgrade-report-*.json' -ErrorAction SilentlyContinue |
         Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $j) { Log "  !! no JSON report produced"; return }
    $data = Get-Content $j.FullName -Raw | ConvertFrom-Json
    $sc = $data.Checks | Where-Object { $_.Title -eq 'Storage controller mode' } | Select-Object -First 1
    Log ("Storage controller mode: [{0}] {1}" -f $sc.Status.ToUpper(), $sc.Detail)
    Log ("Verdict: {0}" -f $data.Verdict.Level)
    Log ("(json: {0})" -f $j.Name)
    # rename the JSON so the two runs don't clobber each other
    Rename-Item $j.FullName (Join-Path $out ("report-{0}.json" -f $name)) -Force
    Get-ChildItem $out -Filter 'upgrade-report-*.txt' | Remove-Item -Force -ErrorAction SilentlyContinue
}

Invoke-Scanner 'source' 'C:\upgrade_\evaluate\windows\upgrade-scan.ps1'
Invoke-Scanner 'dist'   'C:\upgrade_\dist\upgrade-scan.ps1'

# --- Step 5: machine capture (hardware-only) ----------------------------------
Log ""
Log "=== DumpMachine -> machine-capture-vm.json ==="
& 'C:\upgrade_\evaluate\windows\upgrade-scan.ps1' -DumpMachine (Join-Path $out 'machine-capture-vm.json') *>&1 |
    Out-File -FilePath (Join-Path $out 'dump-console.txt') -Encoding UTF8
if (Test-Path (Join-Path $out 'machine-capture-vm.json')) { Log "  capture written to share" }
else { Log "  !! capture missing" }

Log ""
Log "=== DONE ==="
