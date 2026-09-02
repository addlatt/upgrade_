# V3 / R19 - Windows side (guest, PowerShell Direct, ELEVATED). Windows PowerShell 5.1.
# Plants a deterministic, varied corpus under C:\v3corpus (once; -Replant to
# rebuild), hashes it plus every file under C:\Users, records the BitLocker /
# volume facts the row needs, writes it all to the OEMDRV volume's v3\ folder
# (fallback: C:\upgrade-rig-guest\v3\), then does a FULL shutdown (shutdown /s,
# never hybrid - the converter's last Windows exit is a reboot, so the volume
# settle-in reads was cleanly dismounted). -NoShutdown to skip the shutdown.
#
# Manifest format (UTF-8, no BOM, LF): sha256<TAB>size<TAB>relpath  where relpath is
# relative to the volume root with forward slashes; unreadable files carry
# ERR:<reason> in the sha column. The Linux side (guest/v3-read.sh) writes the
# same shape; v3-verdict.py compares them.
param(
    [int]$Seed = 20260901,
    [switch]$Replant,
    [switch]$NoShutdown,
    [string]$Config = 'unnamed'
)
$ErrorActionPreference = 'Continue'
$root = 'C:\v3corpus'
$local = 'C:\upgrade-rig-guest\v3'
New-Item -ItemType Directory -Force -Path $local | Out-Null
$t0 = Get-Date
function Log($m) { $line = ('{0:HH:mm:ss} {1}' -f (Get-Date), $m); Write-Host $line; Add-Content -Path (Join-Path $local 'plant.log') -Value $line }

# --- OEMDRV drive letter (same dance as v1b-mark.ps1) -----------------------
function Get-OemRoot {
    $vol = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'OEMDRV' } | Select-Object -First 1
    if (-not $vol) { return $null }
    if (-not $vol.DriveLetter) {
        $part = Get-Partition | Where-Object { $_.AccessPaths -contains $vol.Path } | Select-Object -First 1
        if ($part) { $part | Add-PartitionAccessPath -AssignDriveLetter | Out-Null }
        Start-Sleep -Seconds 2
        $vol = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'OEMDRV' } | Select-Object -First 1
    }
    if (-not $vol.DriveLetter) { return $null }
    return "$($vol.DriveLetter):\v3"
}
$oem = Get-OemRoot
if ($oem) { New-Item -ItemType Directory -Force -Path $oem | Out-Null; Log "OEMDRV at $oem" } else { Log "!! OEMDRV volume not found; output stays in $local" }
$out = if ($oem) { $oem } else { $local }

# --- corpus --------------------------------------------------------------------
$utf8 = New-Object System.Text.UTF8Encoding($false)
function Write-Random([string]$path, [long]$size, [int]$fileSeed) {
    $rng = New-Object System.Random($fileSeed)
    $fs = [IO.File]::Open($path, 'Create', 'Write', 'None')
    try {
        $buf = New-Object byte[] (1MB)
        $left = $size
        while ($left -gt 0) {
            $n = [int][Math]::Min($left, $buf.Length)
            $rng.NextBytes($buf)
            $fs.Write($buf, 0, $n)
            $left -= $n
        }
    } finally { $fs.Close() }
}
function Write-Text([string]$path, [int]$size, [int]$fileSeed) {
    $rng = New-Object System.Random($fileSeed)
    $words = 'the','quick','brown','fox','jumps','over','lazy','dog','lorem','ipsum','bitlocker','fedora','upgrade'
    $sb = New-Object System.Text.StringBuilder
    while ($sb.Length -lt $size) { [void]$sb.Append($words[$rng.Next($words.Length)]).Append(' ') }
    [IO.File]::WriteAllText($path, $sb.ToString().Substring(0, $size), $utf8)
}
$planted = 0
if ($Replant -and (Test-Path $root)) { Log "replant: removing $root"; Remove-Item -Recurse -Force $root }
if (-not (Test-Path $root)) {
    Log "planting corpus at $root (seed $Seed)"
    foreach ($d in 'small','medium','large','big','compressed','sparse','names','dup') { New-Item -ItemType Directory -Force -Path (Join-Path $root $d) | Out-Null }
    $rng = New-Object System.Random($Seed)
    # small: MFT-resident sizes, sector/cluster boundaries, and random 0..4096
    $edge = 0,1,2,3,511,512,513,699,700,701,1023,1024,1025,4095,4096,4097
    for ($i = 0; $i -lt 2000; $i++) {
        $size = if ($i -lt $edge.Length * 4) { $edge[$i % $edge.Length] } else { $rng.Next(0, 4097) }
        Write-Random (Join-Path $root ('small\{0:D5}.bin' -f $i)) $size ($Seed + $i); $planted++
    }
    for ($i = 0; $i -lt 600; $i++) { Write-Random (Join-Path $root ('medium\{0:D4}.bin' -f $i)) ($rng.Next(4096, 1048577)) ($Seed + 10000 + $i); $planted++ }
    for ($i = 0; $i -lt 30; $i++)  { Write-Random (Join-Path $root ('large\{0:D3}.bin' -f $i)) ([long]$rng.Next(1, 33) * 1MB + $rng.Next(0, 4096)) ($Seed + 20000 + $i); $planted++ }
    Write-Random (Join-Path $root 'big\100mib.bin') (100MB) ($Seed + 30000); $planted++
    Write-Random (Join-Path $root 'big\200mib-plus-1.bin') (200MB + 1) ($Seed + 30001); $planted++
    # NTFS-compressed subtree: compressible text, compressed with compact.exe
    for ($i = 0; $i -lt 200; $i++) { Write-Text (Join-Path $root ('compressed\{0:D4}.txt' -f $i)) ($rng.Next(8192, 262145)) ($Seed + 40000 + $i); $planted++ }
    $c = & compact.exe /c /s:"$root\compressed" /q 2>&1 | Select-Object -Last 2; Log "compact: $c"
    # sparse: 64 MiB file, data at three offsets, holes between
    $sp = Join-Path $root 'sparse\64mib-holes.bin'
    $fs = [IO.File]::Open($sp, 'Create', 'Write', 'None'); $fs.SetLength(64MB); $fs.Close()
    & fsutil.exe sparse setflag $sp | Out-Null
    & fsutil.exe sparse setrange $sp 0 67108864 | Out-Null
    $fs = [IO.File]::Open($sp, 'Open', 'Write', 'None')
    $r2 = New-Object System.Random($Seed + 50000); $chunk = New-Object byte[] 65536
    foreach ($off in 0, 20MB, (64MB - 65536)) { $r2.NextBytes($chunk); $fs.Position = $off; $fs.Write($chunk, 0, $chunk.Length) }
    $fs.Close(); $planted++
    Log ("sparse: " + ((& fsutil.exe sparse queryflag $sp) -join ' '))
    # names: what real user folders contain
    $names = 'café.txt', 'naïve résumé.docx', ([char]0x65E5 + [char]0x672C + [char]0x8A9E + ' ' + [char]0x30D5 + [char]0x30A1 + [char]0x30A4 + [char]0x30EB + '.bin'),
             ('emoji ' + [char]::ConvertFromUtf32(0x1F600) + '.txt'), 'two  spaces.txt', "a'b&c#d%e(f)g.txt", 'UPPER.lower.MiXeD', '.dotfile', 'no_ext',
             ('x' * 200 + '.txt'), 'semi;colon,comma=eq+plus.txt', 'tilde~1.txt', ([char]0x0410 + [char]0x0411 + [char]0x0412 + '.txt')
    $k = 0; foreach ($n in $names) { Write-Random (Join-Path $root ('names\' + $n)) ($rng.Next(1, 20000)) ($Seed + 60000 + $k); $k++; $planted++ }
    $deep = Join-Path $root 'names'; for ($i = 0; $i -lt 20; $i++) { $deep = Join-Path $deep ('level{0:D2}' -f $i) }
    New-Item -ItemType Directory -Force -Path $deep | Out-Null
    Write-Random (Join-Path $deep 'deep.bin') 12345 ($Seed + 61000); $planted++
    # dup: identical content in three files
    for ($i = 0; $i -lt 3; $i++) { Write-Random (Join-Path $root ('dup\same{0}.bin' -f $i)) 777777 ($Seed + 70000); $planted++ }
    Log "planted $planted files"
} else { Log "corpus already present at $root (use -Replant to rebuild)" }

# --- quiesce what writes under C:\Users while we hash (OneDrive rewrites its logs/DBs
# until shutdown; seen as false "mismatches" 2026-09-01). The product has the same
# problem at harvest time - R8/R19 design input, not something to hide here.
foreach ($proc in 'OneDrive','msedge','SearchApp','StartMenuExperienceHost') { Get-Process $proc -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 3

# --- hashing ---------------------------------------------------------------------
$sha = [System.Security.Cryptography.SHA256]::Create()
function Hash-File([string]$path) {
    try {
        $fs = [IO.File]::Open($path, 'Open', 'Read', [IO.FileShare]'ReadWrite, Delete')
        try { $h = $sha.ComputeHash($fs); $len = $fs.Length } finally { $fs.Close() }
        return @(([BitConverter]::ToString($h) -replace '-', '').ToLower(), $len)
    } catch { return @(('ERR:' + ($_.Exception.GetType().Name)), -1) }
}
# manual walk: never follow reparse points (junctions/symlinks), never throw
function Walk([string]$dir, [System.Collections.Generic.List[string]]$acc) {
    try { $entries = [IO.Directory]::EnumerateFileSystemEntries($dir) } catch { return }
    foreach ($e in $entries) {
        try { $a = [IO.File]::GetAttributes($e) } catch { continue }
        if ($a -band [IO.FileAttributes]::ReparsePoint) { continue }
        if ($a -band [IO.FileAttributes]::Directory) { Walk $e $acc } else { $acc.Add($e) }
    }
}
function Manifest([string]$top, [string]$file) {
    $acc = New-Object System.Collections.Generic.List[string]
    Walk $top $acc
    # ordinal order by relative path, so the file is deterministic (the verdict joins by path anyway)
    $byPath = New-Object 'System.Collections.Generic.SortedDictionary[string,string]' ([StringComparer]::Ordinal)
    $errs = 0
    foreach ($p in $acc) {
        $r = Hash-File $p
        if ($r[0] -like 'ERR:*') { $errs++ }
        $rel = $p.Substring(3) -replace '\\', '/'
        $byPath[$rel] = "{0}`t{1}`t{2}" -f $r[0], $r[1], $rel
    }
    [IO.File]::WriteAllText($file, (($byPath.Values -join "`n") + "`n"), $utf8)
    Log ("{0}: {1} files, {2} unreadable -> {3}" -f $top, $acc.Count, $errs, $file)
    return $acc.Count
}
$nCorpus = Manifest $root (Join-Path $out 'manifest-win-corpus.txt')
$nUsers  = Manifest 'C:\Users' (Join-Path $out 'manifest-win-users.txt')

# --- facts for the row ---------------------------------------------------------------
$blv = Get-BitLockerVolume -MountPoint C:
$part = Get-Partition -DriveLetter C
$hib = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -ErrorAction SilentlyContinue).HiberbootEnabled
$f = [ordered]@{
    timestamp        = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    config           = $Config
    windows_build    = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild
    bitlocker_method = "$($blv.EncryptionMethod)"
    bitlocker_status = "$($blv.VolumeStatus)"
    bitlocker_pct    = "$($blv.EncryptionPercentage)"
    bitlocker_protection = "$($blv.ProtectionStatus)"
    bitlocker_protectors = (($blv.KeyProtector | ForEach-Object { "$($_.KeyProtectorType)" }) -join '+')
    used_space_only  = (& manage-bde.exe -status C: | Select-String 'Used Space Only|Fully Encrypted|Encrypted' | ForEach-Object { $_.Line.Trim() }) -join ';'
    partition_bytes  = $part.Size
    volume_bytes     = (Get-Volume -DriveLetter C).Size
    hiberboot_enabled = "$hib"
    hiberfil_present = (Test-Path 'C:\hiberfil.sys')
    corpus_files     = $nCorpus
    users_files      = $nUsers
    seed             = $Seed
    plant_seconds    = [int]((Get-Date) - $t0).TotalSeconds
}
$fx = ($f.GetEnumerator() | ForEach-Object { '{0}={1}' -f $_.Key, $_.Value }) -join "`n"
[IO.File]::WriteAllText((Join-Path $out 'facts-win.txt'), $fx + "`n", $utf8)
Log $fx
Copy-Item (Join-Path $local 'plant.log') (Join-Path $out 'plant.log') -Force -ErrorAction SilentlyContinue
if ($oem) { Copy-Item (Join-Path $out '*.txt') $local -Force }
if (-not $NoShutdown) { Log "full shutdown in 5 s"; & shutdown.exe /s /f /t 5 }
