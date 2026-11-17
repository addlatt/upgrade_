# rig/hyperv/guest/r39-arm.ps1: one arm of the R39 bench, inside the bench's
# Windows (PowerShell Direct, elevated). Windows PowerShell 5.1.
#
# Plants what the arm names on the disk Windows starts from, runs the real
# scanner and the real job writer from C:\upgr39, reports what each said as
# one line (R39JSON=<base64 of JSON>), and takes the plant away again.
#
#   partition: a 512 MB partition of GPT type "Linux filesystem", never
#              formatted, made in 1 GB shrunk off C: (grown back after)
#   folder:    EFI\fedora\shimx64.efi on the boot partition (a 3-byte file)
#
# Hyper-V has no USB, so the job writer always refuses the rig's SCSI "stick"
# (R16), and PowerShell Direct is not the desktop, so it also refuses "who is
# signed in". Both stand in every arm; what the arm reads is whether R39's
# refusal stands beside them. A job written with no Linux there is a real
# machine's row.
#
# Arms: none, partition, folder, both, erase (both planted, the erase
# sentence given), removed (planted, then taken away before the programs run).
param([Parameter(Mandatory=$true)][ValidateSet('none','partition','folder','both','erase','removed')][string]$Arm, [string]$Stick = '')
$ErrorActionPreference = 'Stop'
$LINUX = '{0fc63daf-8483-4772-8e79-3d69d8477de4}'
$ERASE = 'I confirm that everything on this computer will be deleted and nothing will be kept'
$bin = 'C:\upgr39'
$disk = (Get-Partition -DriveLetter C).DiskNumber
if (-not $Stick) { $Stick = (Get-Volume -FileSystemLabel UPGV0).DriveLetter + ':' }

function Esp([scriptblock]$do) {
    $l = 'Z','Y','X','W' | Where-Object { -not (Test-Path "${_}:\") } | Select-Object -First 1
    cmd /c "mountvol ${l}: /S" | Out-Null
    try { & $do "${l}:" } finally { cmd /c "mountvol ${l}: /D" | Out-Null }
}
function Unplant {
    Get-Partition -DiskNumber $disk | Where-Object { $_.GptType -eq $LINUX } | ForEach-Object { Remove-Partition -DiskNumber $disk -PartitionNumber $_.PartitionNumber -Confirm:$false }
    $max = (Get-PartitionSupportedSize -DriveLetter C).SizeMax
    if (($max - (Get-Partition -DriveLetter C).Size) -gt 1MB) { Resize-Partition -DriveLetter C -Size $max }
    Esp { param($e) if (Test-Path "$e\EFI\fedora") { Remove-Item "$e\EFI\fedora" -Recurse -Force } }
}
function Plant-Partition {
    Resize-Partition -DriveLetter C -Size ((Get-Partition -DriveLetter C).Size - 1GB)
    $p = New-Partition -DiskNumber $disk -Size 512MB -GptType $LINUX
    return $p.PartitionNumber
}
function Plant-Folder {
    Esp { param($e) New-Item -ItemType Directory -Path "$e\EFI\fedora" -Force | Out-Null; [IO.File]::WriteAllBytes("$e\EFI\fedora\shimx64.efi", [byte[]](0x72,0x33,0x39)) }
}
function Layout {
    $parts = @(Get-Partition -DiskNumber $disk | ForEach-Object { '{0}:{1}:{2}' -f $_.PartitionNumber, $_.GptType, $_.Size })
    $efi = Esp { param($e) @(Get-ChildItem "$e\EFI" -Force | ForEach-Object Name) -join ',' }
    return @{ Partitions = $parts; EspFolders = $efi }
}

$r = [ordered]@{ Arm = $Arm; Planted = @(); PlantedPartition = $null }
Unplant
$r.LayoutBefore = Layout
if ($Arm -in 'partition','both','erase','removed') { $r.PlantedPartition = Plant-Partition; $r.Planted += 'partition' }
if ($Arm -in 'folder','both','erase','removed') { Plant-Folder; $r.Planted += 'folder' }
if ($Arm -eq 'removed') { Unplant }
$r.LayoutAtRun = Layout

$ErrorActionPreference = 'Continue'
$reports = "$bin\reports-$Arm"; $out = "$bin\out-$Arm"
Remove-Item $reports, $out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $reports, $out -Force | Out-Null
$r.ScanVersion = (& "$bin\upgrade-scan.exe" --version) -join ' '
$r.JobVersion = (& "$bin\upgrade-job.exe" --version) -join ' '
$scan = & "$bin\upgrade-scan.exe" scan --json --out $reports --kit "$Stick\" 2>&1 | ForEach-Object { "$_" }
$r.ScanExit = $LASTEXITCODE
$report = Get-ChildItem $reports -Filter *.json | Sort-Object LastWriteTime | Select-Object -Last 1
$r.ScanVerdict = $null; $r.ScanLine = $null
if ($report) {
    $j = Get-Content $report.FullName -Raw | ConvertFrom-Json
    $r.ScanVerdict = $j.Verdict
    $c = @($j.Checks | Where-Object { $_.Title -eq 'Linux already here' })
    if ($c.Count) { $r.ScanLine = @{ Status = "$($c[0].Status)"; Detail = $c[0].Detail } }
    $r.ScanTitles = @($j.Checks | ForEach-Object { $_.Title }).Count
}
[IO.File]::WriteAllText("$bin\pw.txt", '$6$rounds=5000$saltsaltsaltsalt$0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ./0123456789abcdefghijklmnopqrstuv' + "`n")
$a = @('write','--stick',$Stick,'--out',$out,'--scan',$reports,'--desktop','kde','--start-at','desktop','--if-cannot-keep','stop','--password-hash-file',"$bin\pw.txt",'--kickstart',"$out\ks.cfg",'--stick-label','UPGV0','--manifest',"$Stick\SHA256SUMS")
if ($Arm -eq 'erase') { $a += @('--erase-everything', $ERASE) }
$job = & "$bin\upgrade-job.exe" @a 2>&1 | ForEach-Object { "$_" }
$r.JobExit = $LASTEXITCODE
$r.JobOut = ($job -join "`n")
$r.JobWritten = Test-Path "$out\job.json"
$r.JobPath = $null
if ($r.JobWritten) { $r.JobPath = (Get-Content "$out\job.json" -Raw | ConvertFrom-Json).intent.path }
# the job writer's own read of the disk, as it hands it to its judging half
$ff = "$bin\facts-$Arm.json"
& "$bin\upgrade-job.exe" facts --scan $reports --stick $Stick --out $ff 2>&1 | Out-Null
$r.Facts = $null
if (Test-Path $ff) {
    $f = (Get-Content $ff -Raw | ConvertFrom-Json).Facts
    $r.Facts = @{ LinuxPartitions = @($f.LinuxPartitions); LinuxBootFolders = @($f.LinuxBootFolders); OtherLinuxError = $f.OtherLinuxError }
    Remove-Item $ff -Force
}
Remove-Item "$bin\pw.txt" -Force -ErrorAction SilentlyContinue
# a written job carries the bench's Wi-Fi profiles and the password hash: nothing of it is kept
Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue

$ErrorActionPreference = 'Stop'
Unplant
$r.LayoutAfter = Layout
'R39JSON=' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($r | ConvertTo-Json -Depth 6 -Compress)))
